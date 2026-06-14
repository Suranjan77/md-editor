use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use tar::Archive;

const RELEASE: &str = "chromium%2F7869";
const BUILD: &str = "7869";

struct PdfiumPackage {
    asset: &'static str,
    sha256: &'static str,
    library: &'static str,
}

fn main() {
    println!("cargo:rerun-if-env-changed=PDFIUM_LIB_DIR");

    if env::var_os("CARGO_FEATURE_PDFIUM").is_none() {
        return;
    }

    if let Some(dir) = env::var_os("PDFIUM_LIB_DIR").map(PathBuf::from) {
        configure(&dir).unwrap_or_else(|error| panic!("{error}"));
        return;
    }

    let package = package_for_target(
        &env::var("CARGO_CFG_TARGET_OS").unwrap_or_default(),
        &env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default(),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let cache_dir = target_dir().join("pdfium").join(BUILD).join(package.asset);

    acquire(&cache_dir, &package).unwrap_or_else(|error| panic!("{error}"));
    let library_dir = Path::new(package.library)
        .parent()
        .map(|parent| cache_dir.join(parent))
        .unwrap_or(cache_dir);
    configure(&library_dir).unwrap_or_else(|error| panic!("{error}"));
}

fn package_for_target(os: &str, arch: &str) -> Result<PdfiumPackage, String> {
    match (os, arch) {
        ("windows", "x86_64") => Ok(PdfiumPackage {
            asset: "win-x64",
            sha256: "d1a2b39c300f62daeec94f3a648a31d83d18605707bfdc5504d818d42cab13ce",
            library: "bin/pdfium.dll",
        }),
        ("windows", "aarch64") => Ok(PdfiumPackage {
            asset: "win-arm64",
            sha256: "9f1b42f841d99dc1a592bf2c14419166476f4074321e5c6aef5ee92c217cdfaf",
            library: "bin/pdfium.dll",
        }),
        ("linux", "x86_64") => Ok(PdfiumPackage {
            asset: "linux-x64",
            sha256: "6aeb4be0f790bf309c6b1e665552351845fe921a78d21697a1a9cb8ce427bb23",
            library: "lib/libpdfium.so",
        }),
        ("linux", "aarch64") => Ok(PdfiumPackage {
            asset: "linux-arm64",
            sha256: "26213696d0457ba07469cc23b8b112a2f0d316ceea0866a20b42d5216d603a93",
            library: "lib/libpdfium.so",
        }),
        ("macos", "x86_64") => Ok(PdfiumPackage {
            asset: "mac-x64",
            sha256: "00de12ca1b9729119e7fd4901cee1f0e591367f20cf98e221a446bbf55c155d2",
            library: "lib/libpdfium.dylib",
        }),
        ("macos", "aarch64") => Ok(PdfiumPackage {
            asset: "mac-arm64",
            sha256: "935a50329d5f72466b2058f92f2c4a8f9e541abc8f3149b1994d078dec4190e1",
            library: "lib/libpdfium.dylib",
        }),
        _ => Err(format!(
            "automatic PDFium download does not support target {arch}-{os}; \
             set PDFIUM_LIB_DIR to a compatible library directory"
        )),
    }
}

fn target_dir() -> PathBuf {
    let workspace = Path::new(&env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default())
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    match env::var_os("CARGO_TARGET_DIR").map(PathBuf::from) {
        Some(dir) if dir.is_absolute() => dir,
        Some(dir) => workspace.join(dir),
        None => workspace.join("target"),
    }
}

fn acquire(cache_dir: &Path, package: &PdfiumPackage) -> Result<(), String> {
    if valid_install(cache_dir, package) {
        return Ok(());
    }

    fs::create_dir_all(cache_dir).map_err(|error| {
        format!(
            "could not create PDFium cache {}: {error}",
            cache_dir.display()
        )
    })?;
    let archive_path = cache_dir.join(format!("pdfium-{}.tgz", package.asset));
    if !valid_archive(&archive_path, package.sha256)? {
        let url = format!(
            "https://github.com/bblanchon/pdfium-binaries/releases/download/{RELEASE}/pdfium-{}.tgz",
            package.asset
        );
        println!(
            "cargo:warning=downloading PDFium {BUILD} for {}",
            package.asset
        );
        download(&url, &archive_path)?;
        if !valid_archive(&archive_path, package.sha256)? {
            return Err(format!(
                "PDFium archive checksum mismatch: {}",
                archive_path.display()
            ));
        }
    }

    Archive::new(GzDecoder::new(
        File::open(&archive_path).map_err(io_error("opening PDFium archive"))?,
    ))
    .unpack(cache_dir)
    .map_err(io_error("extracting PDFium archive"))?;

    if !valid_install(cache_dir, package) {
        return Err(format!(
            "PDFium archive did not contain {}",
            package.library
        ));
    }
    Ok(())
}

fn valid_install(cache_dir: &Path, package: &PdfiumPackage) -> bool {
    cache_dir.join(package.library).is_file()
        && fs::read_to_string(cache_dir.join("VERSION"))
            .is_ok_and(|version| version.lines().any(|line| line == format!("BUILD={BUILD}")))
}

fn valid_archive(path: &Path, expected: &str) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let mut file = File::open(path).map_err(io_error("opening cached PDFium archive"))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(io_error("hashing cached PDFium archive"))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()) == expected)
}

fn download(url: &str, destination: &Path) -> Result<(), String> {
    let temp = destination.with_extension("tgz.part");
    if destination.exists() {
        fs::remove_file(destination).map_err(io_error("removing invalid PDFium archive"))?;
    }
    let response = ureq::get(url)
        .call()
        .map_err(|error| format!("could not download PDFium from {url}: {error}"))?;
    let mut reader = response.into_reader();
    let mut output = File::create(&temp).map_err(io_error("creating PDFium download"))?;
    io::copy(&mut reader, &mut output).map_err(io_error("writing PDFium download"))?;
    fs::rename(&temp, destination).map_err(io_error("committing PDFium download"))
}

fn configure(dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!(
            "PDFIUM_LIB_DIR is not a directory: {}",
            dir.display()
        ));
    }
    println!("cargo:rustc-env=MD_PDFIUM_LIB_DIR={}", dir.display());
    Ok(())
}

fn io_error(context: &'static str) -> impl FnOnce(io::Error) -> String {
    move |error| format!("{context}: {error}")
}
