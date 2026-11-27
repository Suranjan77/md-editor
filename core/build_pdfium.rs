//! Downloads, verifies and caches the platform PDFium library, then copies it next to the build output.

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
    // Pinned so CI and release builds are reproducible. `chromium/8165` is the
    // build the test suite runs against with pdfium-render 0.9.1. To move the
    // pin, update this tag and `pinned_sha256` together.
    const DEFAULT_PDFIUM_RELEASE: &str = "chromium/8165";
    println!("cargo:rerun-if-env-changed=PDFIUM_RELEASE");
    println!("cargo:rerun-if-env-changed=PDFIUM_SHA256");
    println!("cargo:rerun-if-env-changed=PDFIUM_ARCHIVE");
    let release = env::var("PDFIUM_RELEASE").unwrap_or_else(|_| DEFAULT_PDFIUM_RELEASE.to_string());
    let release_slug: String = release
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect();
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

    let cache_dir = target_dir
        .join("pdfium")
        .join(format!("{platform_slug}-{release_slug}"));
    let lib_subdir = if target_os == "windows" { "bin" } else { "lib" };
    let lib_path = cache_dir.join(lib_subdir).join(lib_filename);

    let expected_sha = env::var("PDFIUM_SHA256").ok().or_else(|| {
        (release == DEFAULT_PDFIUM_RELEASE)
            .then(|| pinned_sha256(platform_slug))
            .flatten()
            .map(str::to_string)
    });
    let checksum_marker = cache_dir.join(".sha256");
    let checksum_matches = expected_sha.as_ref().is_none_or(|expected| {
        fs::read_to_string(&checksum_marker)
            .ok()
            .is_some_and(|recorded| recorded.trim().eq_ignore_ascii_case(expected.trim()))
    });
    if lib_path.exists() && checksum_matches {
        println!(
            "cargo:warning=PDFium already cached at {}",
            lib_path.display()
        );
        setup_resource_copy(&cache_dir, &profile_dir, lib_filename);
        emit_pdfium_paths(&target_dir, &profile_dir);
        return cache_dir;
    }

    // The default release is pinned and verified against `pinned_sha256`.
    // Override with the `PDFIUM_RELEASE` env var (e.g. `chromium/6996`, or
    // `latest`); set `PDFIUM_SHA256` to the hex digest of the platform `.tgz`
    // to verify an override — the build then fails loudly on mismatch instead
    // of trusting whatever bytes the network returned.
    println!(
        "cargo:warning=Downloading PDFium ({release}) for {}...",
        platform_slug
    );

    let url = if release == "latest" {
        format!(
            "https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-{}.tgz",
            platform_slug
        )
    } else {
        // Release tags contain a slash (e.g. `chromium/6996`) which must be
        // percent-encoded in the download path.
        format!(
            "https://github.com/bblanchon/pdfium-binaries/releases/download/{}/pdfium-{}.tgz",
            release.replace('/', "%2F"),
            platform_slug
        )
    };

    let archive_path = cache_dir.join("pdfium.tgz");
    fs::create_dir_all(&cache_dir).expect("Failed to create PDFium cache directory");

    // Packagers (the Arch PKGBUILD) download the archive themselves, as a
    // declared source, and point `PDFIUM_ARCHIVE` at it; it is still verified
    // below.
    let fetched = match env::var("PDFIUM_ARCHIVE") {
        Ok(local) => fs::copy(&local, &archive_path)
            .map(|_| ())
            .map_err(|e| format!("copying {local} failed: {e}")),
        Err(_) => download_to_file(&url, &archive_path),
    };
    if let Err(e) = fetched {
        println!(
            "cargo:warning=Failed to download PDFium archive: {}. PDFium will not be available.",
            e
        );
        return cache_dir;
    }

    if let Some(expected) = expected_sha {
        match verify_sha256(&archive_path, &expected) {
            Ok(()) => {
                fs::write(&checksum_marker, expected.trim())
                    .expect("Failed to record verified PDFium checksum");
                println!("cargo:warning=PDFium archive checksum verified");
            }
            Err(e) => {
                let _ = fs::remove_file(&archive_path);
                panic!("PDFium archive checksum verification failed: {e}");
            }
        }
    } else {
        println!(
            "cargo:warning=PDFIUM_SHA256 not set — PDFium archive integrity is unverified. \
             Set PDFIUM_RELEASE and PDFIUM_SHA256 to pin and verify the download."
        );
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

fn verify_sha256(archive_path: &Path, expected_hex: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};

    let mut file = File::open(archive_path).map_err(|e| format!("open archive failed: {e}"))?;
    let mut hasher = Sha256::new();
    copy(&mut file, &mut hasher).map_err(|e| format!("hashing failed: {e}"))?;
    let actual = hasher.finalize();
    let actual_hex: String = actual.iter().map(|b| format!("{b:02x}")).collect();

    let expected = expected_hex.trim().to_lowercase();
    if actual_hex == expected {
        Ok(())
    } else {
        Err(format!("expected {expected}, got {actual_hex}"))
    }
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
        println!(
            "cargo:warning=PDFium library not found at {}",
            lib_src.display()
        );
        return;
    }

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let resource_dir = PathBuf::from(&manifest_dir).join("pdfium");
    fs::create_dir_all(&resource_dir).expect("Failed to create pdfium resource directory");

    let lib_dst = resource_dir.join(lib_filename);
    if !lib_dst.exists()
        || fs::metadata(&lib_src).unwrap().len()
            != fs::metadata(&lib_dst).map(|m| m.len()).unwrap_or(0)
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

/// SHA-256 of each platform archive of the pinned `DEFAULT_PDFIUM_RELEASE`
/// (from the release's asset digests on bblanchon/pdfium-binaries).
fn pinned_sha256(platform_slug: &str) -> Option<&'static str> {
    Some(match platform_slug {
        "linux-x64" => "ba7865e16c0271daab75256790be06c26272190afe6e2a70eae0bb39bb99089c",
        "linux-arm64" => "338f5dcc907a20331812ae8ab01b16ff18966f4a247d790c8ad8fb49029d768b",
        "win-x64" => "ca7338ae7779b8b1b82f4580305c4d762d214f8df1d54b47647fe3cc75989ded",
        "win-arm64" => "d65e405c59cffe6fe36c0935b90d40ddf3ce75ef284f3168a28dcf00d0388131",
        "mac-x64" => "07285915b564e46a432a9909dbc1f15892885ca6489e3faeefc70dee77ed371a",
        "mac-arm64" => "112e74d717f3e0310273b70f3c8780e8a61ff11b34834124465c8305c841d350",
        _ => return None,
    })
}
