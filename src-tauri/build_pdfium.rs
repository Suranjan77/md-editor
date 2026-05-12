use std::env;
use std::fs;
use std::fs::File;
use std::io::copy;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use tar::Archive;

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
            println!(
                "cargo:warning=Unsupported platform {}-{} for PDFium, skipping download",
                target_os, target_arch
            );
            return PathBuf::from("pdfium_not_available");
        }
    };

    let lib_filename = match target_os.as_str() {
        "windows" => "pdfium.dll",
        "macos" => "libpdfium.dylib",
        _ => "libpdfium.so",
    };

    let out_dir = env::var("OUT_DIR").unwrap();
    let out_path = PathBuf::from(&out_dir);
    let target_dir = out_path
        .ancestors()
        .find(|p| p.file_name().is_some_and(|n| n == "target"))
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| out_path.clone());
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let profile_dir = out_path
        .ancestors()
        .find(|p| p.file_name().is_some_and(|n| n == profile.as_str()))
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| target_dir.join(&profile));

    let cache_dir = target_dir.join("pdfium").join(platform_slug);
    let lib_subdir = if target_os == "windows" { "bin" } else { "lib" };
    let lib_path = cache_dir.join(lib_subdir).join(lib_filename);

    if lib_path.exists() {
        println!("cargo:warning=PDFium already cached at {}", lib_path.display());
        setup_resource_copy(&cache_dir, &profile_dir, lib_filename);
        emit_pdfium_paths(&target_dir, &profile_dir);
        return cache_dir;
    }

    println!("cargo:warning=Downloading PDFium for {}...", platform_slug);

    let url = format!(
        "https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-{}.tgz",
        platform_slug
    );

    let archive_path = cache_dir.join("pdfium.tgz");
    fs::create_dir_all(&cache_dir).expect("Failed to create PDFium cache directory");

    if let Err(e) = download_to_file(&url, &archive_path) {
        println!(
            "cargo:warning=Failed to download PDFium archive: {}. PDFium will not be available.",
            e
        );
        return cache_dir;
    }

    if let Err(e) = extract_tgz(&archive_path, &cache_dir) {
        println!(
            "cargo:warning=Failed to extract PDFium archive: {}. PDFium may not be available.",
            e
        );
        return cache_dir;
    }

    let _ = fs::remove_file(&archive_path);

    setup_resource_copy(&cache_dir, &profile_dir, lib_filename);
    emit_pdfium_paths(&target_dir, &profile_dir);
    cache_dir
}

fn download_to_file(url: &str, archive_path: &Path) -> Result<(), String> {
    let mut response = ureq::get(url)
        .call()
        .map_err(|e| format!("request failed: {e}"))?;
    let mut reader = response.body_mut().as_reader();
    let mut out = File::create(archive_path).map_err(|e| format!("create file failed: {e}"))?;
    copy(&mut reader, &mut out).map_err(|e| format!("write archive failed: {e}"))?;
    println!("cargo:warning=PDFium downloaded successfully");
    Ok(())
}

fn extract_tgz(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let file = File::open(archive_path).map_err(|e| format!("open archive failed: {e}"))?;
    let tar = GzDecoder::new(file);
    let mut archive = Archive::new(tar);
    archive
        .unpack(destination)
        .map_err(|e| format!("extract failed: {e}"))?;
    println!(
        "cargo:warning=PDFium extracted successfully to {}",
        destination.display()
    );
    Ok(())
}

fn setup_resource_copy(cache_dir: &Path, profile_dir: &Path, lib_filename: &str) {
    let lib_subdir = if lib_filename.ends_with(".dll") {
        "bin"
    } else {
        "lib"
    };
    let lib_src = cache_dir.join(lib_subdir).join(lib_filename);
    if !lib_src.exists() {
        println!("cargo:warning=PDFium library not found at {}", lib_src.display());
        return;
    }

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let resource_dir = PathBuf::from(&manifest_dir).join("pdfium");
    fs::create_dir_all(&resource_dir).expect("Failed to create pdfium resource directory");

    let lib_dst = resource_dir.join(lib_filename);
    if !lib_dst.exists()
        || fs::metadata(&lib_src).unwrap().len() != fs::metadata(&lib_dst).map(|m| m.len()).unwrap_or(0)
    {
        fs::copy(&lib_src, &lib_dst).expect("Failed to copy PDFium library to resource directory");
    }

    fs::create_dir_all(profile_dir).expect("Failed to create Cargo profile directory");
    let profile_lib_dst = profile_dir.join(lib_filename);
    if !profile_lib_dst.exists()
        || fs::metadata(&lib_src).unwrap().len()
            != fs::metadata(&profile_lib_dst).map(|m| m.len()).unwrap_or(0)
    {
        fs::copy(&lib_src, &profile_lib_dst)
            .expect("Failed to copy PDFium library to Cargo profile directory");
    }

    println!(
        "cargo:rustc-link-search=native={}",
        cache_dir.join(lib_subdir).display()
    );
}

fn emit_pdfium_paths(target_dir: &Path, profile_dir: &Path) {
    println!("cargo:rustc-env=PDFIUM_TARGET_DIR={}", target_dir.display());
    println!(
        "cargo:rustc-env=PDFIUM_TARGET_PROFILE_DIR={}",
        profile_dir.display()
    );
}
