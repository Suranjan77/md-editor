use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Download and set up PDFium binaries for the current platform.
/// Returns the directory containing the PDFium shared library.
pub fn setup_pdfium() -> PathBuf {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_else(|_| {
        if cfg!(target_os = "windows") {
            "windows".to_string()
        } else if cfg!(target_os = "macos") {
            "macos".to_string()
        } else {
            "linux".to_string()
        }
    });

    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| {
        if cfg!(target_arch = "aarch64") {
            "aarch64".to_string()
        } else {
            "x86_64".to_string()
        }
    });

    let platform_slug = match (target_os.as_str(), target_arch.as_str()) {
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("windows", "x86_64") => "win-x64",
        ("windows", "aarch64") => "win-arm64",
        ("macos", "x86_64") => "mac-x64",
        ("macos", "aarch64") => "mac-arm64",
        _ => {
            println!("cargo:warning=Unsupported platform {}-{} for PDFium, skipping download", target_os, target_arch);
            // Return a dummy path; the app will fail gracefully at runtime
            return PathBuf::from("pdfium_not_available");
        }
    };

    let lib_filename = match target_os.as_str() {
        "windows" => "pdfium.dll",
        "macos" => "libpdfium.dylib",
        _ => "libpdfium.so",
    };

    // Cache directory: target/pdfium/<platform>/
    let out_dir = env::var("OUT_DIR").unwrap();
    let out_path = PathBuf::from(&out_dir);
    // Go up from OUT_DIR to the target directory (OUT_DIR is deeply nested)
    let target_dir = out_path
        .ancestors()
        .find(|p| p.file_name().map_or(false, |n| n == "target"))
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| out_path.clone());

    let cache_dir = target_dir.join("pdfium").join(platform_slug);
    let lib_path = cache_dir.join("lib").join(lib_filename);

    // If already downloaded, skip
    if lib_path.exists() {
        println!("cargo:warning=PDFium already cached at {}", lib_path.display());
        setup_resource_copy(&cache_dir, lib_filename);
        return cache_dir;
    }

    println!("cargo:warning=Downloading PDFium for {}...", platform_slug);

    let url = format!(
        "https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-{}.tgz",
        platform_slug
    );

    let archive_path = cache_dir.join("pdfium.tgz");
    fs::create_dir_all(&cache_dir).expect("Failed to create PDFium cache directory");

    // Download using curl (available on all platforms with Tauri dev tooling)
    let status = Command::new("curl")
        .args(["-L", "--fail", "-o", archive_path.to_str().unwrap(), &url])
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("cargo:warning=PDFium downloaded successfully");
        }
        Ok(s) => {
            println!(
                "cargo:warning=curl exited with status {}. PDFium will not be available.",
                s
            );
            return cache_dir;
        }
        Err(e) => {
            println!(
                "cargo:warning=Failed to run curl: {}. PDFium will not be available.",
                e
            );
            return cache_dir;
        }
    }

    // Extract using tar
    let status = Command::new("tar")
        .args(["xzf", archive_path.to_str().unwrap(), "-C", cache_dir.to_str().unwrap()])
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("cargo:warning=PDFium extracted successfully to {}", cache_dir.display());
        }
        Ok(s) => {
            println!("cargo:warning=tar exited with status {}", s);
        }
        Err(e) => {
            println!("cargo:warning=Failed to run tar: {}", e);
        }
    }

    // Clean up the archive
    let _ = fs::remove_file(&archive_path);

    setup_resource_copy(&cache_dir, lib_filename);
    cache_dir
}

/// Copy the PDFium shared library to a known location for Tauri resource bundling.
fn setup_resource_copy(cache_dir: &Path, lib_filename: &str) {
    let lib_src = cache_dir.join("lib").join(lib_filename);
    if !lib_src.exists() {
        println!("cargo:warning=PDFium library not found at {}", lib_src.display());
        return;
    }

    // Copy to the src-tauri/pdfium/ directory for Tauri resource bundling
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let resource_dir = PathBuf::from(&manifest_dir).join("pdfium");
    fs::create_dir_all(&resource_dir).expect("Failed to create pdfium resource directory");

    let lib_dst = resource_dir.join(lib_filename);
    if !lib_dst.exists() || fs::metadata(&lib_src).unwrap().len() != fs::metadata(&lib_dst).map(|m| m.len()).unwrap_or(0) {
        fs::copy(&lib_src, &lib_dst).expect("Failed to copy PDFium library to resource directory");
        println!("cargo:warning=Copied PDFium library to {}", lib_dst.display());
    }

    // Tell Cargo to add the library directory to the linker search path
    println!("cargo:rustc-link-search=native={}", cache_dir.join("lib").display());
}
