use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const BINARY: &str = "md3-shell";
const PRODUCT: &str = "md-editor";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("md3-xtask: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let archive_only = match env::args().nth(1).as_deref() {
        Some("dist") => env::args().any(|arg| arg == "--archive-only"),
        _ => return Err("usage: cargo xtask dist [--archive-only]".to_string()),
    };
    dist(archive_only)
}

fn dist(archive_only: bool) -> Result<(), String> {
    let root = workspace_root()?;
    let platform = Platform::host()?;
    let arch = release_arch()?;
    let version = workspace_version(&root)?;
    let pdfium_lib = env::var_os("PDFIUM_LIB")
        .map(PathBuf::from)
        .ok_or_else(|| "PDFIUM_LIB must point to platform PDFium library".to_string())?;
    require_file(&pdfium_lib)?;

    run_command(
        Command::new("cargo").current_dir(&root).args([
            "build",
            "--release",
            "--locked",
            "-p",
            "md3-shell",
            "--features",
            "pdfium",
        ]),
        "building release binary",
    )?;

    let dist = root.join("dist");
    let stage = dist.join("stage");
    recreate_dir(&stage)?;
    fs::create_dir_all(&dist).map_err(io_error("creating dist directory"))?;
    let package = create_layout(&root, &stage, platform, &pdfium_lib, &version)?;
    let stem = format!("{PRODUCT}-v{version}-{}-{arch}", platform.slug());
    let mut artifacts = Vec::new();

    match platform {
        Platform::Windows => {
            let archive = dist.join(format!("{stem}-portable.zip"));
            powershell_archive(&package, &archive)?;
            artifacts.push(archive);
            if !archive_only {
                artifacts.push(nsis_installer(&root, &dist, &package, &stem, &version)?);
            }
        }
        Platform::Linux => {
            let archive = dist.join(format!("{stem}-portable.tar.gz"));
            tar_archive(&package, &archive)?;
            artifacts.push(archive);
            if !archive_only {
                artifacts.push(appimage(&root, &dist, &package, &stem)?);
            }
        }
        Platform::Macos => {
            let archive = dist.join(format!("{stem}-portable.tar.gz"));
            tar_archive(&package, &archive)?;
            artifacts.push(archive);
            if !archive_only {
                artifacts.push(dmg(&dist, &package, &stem)?);
            }
        }
    }
    write_checksums(&dist, &artifacts)?;
    fs::remove_dir_all(stage).map_err(io_error("removing staging directory"))?;
    Ok(())
}

#[derive(Clone, Copy)]
enum Platform {
    Windows,
    Linux,
    Macos,
}

impl Platform {
    fn host() -> Result<Self, String> {
        match env::consts::OS {
            "windows" => Ok(Self::Windows),
            "linux" => Ok(Self::Linux),
            "macos" => Ok(Self::Macos),
            other => Err(format!("unsupported host OS: {other}")),
        }
    }

    fn slug(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Linux => "linux",
            Self::Macos => "macos",
        }
    }

    fn executable(self) -> &'static str {
        match self {
            Self::Windows => "md3-shell.exe",
            Self::Linux | Self::Macos => BINARY,
        }
    }

    fn packaged_executable(self) -> &'static str {
        match self {
            Self::Windows => "md-editor.exe",
            Self::Linux | Self::Macos => PRODUCT,
        }
    }

    fn pdfium(self) -> &'static str {
        match self {
            Self::Windows => "pdfium.dll",
            Self::Linux => "libpdfium.so",
            Self::Macos => "libpdfium.dylib",
        }
    }
}

fn create_layout(
    root: &Path,
    stage: &Path,
    platform: Platform,
    pdfium_lib: &Path,
    version: &str,
) -> Result<PathBuf, String> {
    let package = stage.join(PRODUCT);
    if matches!(platform, Platform::Macos) {
        let app = package.join("MD Editor.app");
        let contents = app.join("Contents");
        let executable_dir = contents.join("MacOS");
        let resources = contents.join("Resources");
        fs::create_dir_all(&executable_dir).map_err(io_error("creating app executable dir"))?;
        fs::create_dir_all(&resources).map_err(io_error("creating app resources dir"))?;
        copy(
            &root.join("target/release").join(platform.executable()),
            &executable_dir.join(platform.packaged_executable()),
        )?;
        copy(pdfium_lib, &resources.join(platform.pdfium()))?;
        fs::write(
            contents.join("Info.plist"),
            include_str!("../../packaging/macos/Info.plist")
                .replace("{{VERSION}}", version.trim_end_matches("-dev")),
        )
        .map_err(io_error("writing Info.plist"))?;
        create_icns(root, &resources)?;
        copy(&root.join("../LICENSE"), &package.join("LICENSE"))?;
        run_command(
            Command::new("codesign")
                .args(["--force", "--deep", "--sign", "-"])
                .arg(&app),
            "ad-hoc signing app bundle",
        )?;
    } else {
        let resources = package.join("resources");
        fs::create_dir_all(&resources).map_err(io_error("creating resources directory"))?;
        copy(
            &root.join("target/release").join(platform.executable()),
            &package.join(platform.packaged_executable()),
        )?;
        copy(pdfium_lib, &resources.join(platform.pdfium()))?;
        copy(&root.join("../LICENSE"), &package.join("LICENSE"))?;
        copy(
            &root.join("../md-editor.png"),
            &package.join("md-editor.png"),
        )?;
    }
    copy_licenses(&package)?;
    File::create(package.join("portable.flag")).map_err(io_error("creating portable marker"))?;
    Ok(package)
}

fn copy_licenses(package: &Path) -> Result<(), String> {
    let Some(source) = env::var_os("PDFIUM_LICENSE_DIR").map(PathBuf::from) else {
        return Ok(());
    };
    let destination = package.join("THIRD_PARTY_LICENSES/pdfium");
    copy_dir(&source, &destination)
}

fn copy_dir(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(io_error("creating license directory"))?;
    for entry in fs::read_dir(source).map_err(io_error("reading license directory"))? {
        let entry = entry.map_err(|error| format!("reading license entry: {error}"))?;
        let path = entry.path();
        let target = destination.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target)?;
        } else {
            copy(&path, &target)?;
        }
    }
    Ok(())
}

fn create_icns(root: &Path, resources: &Path) -> Result<(), String> {
    let iconset = resources.join("md-editor.iconset");
    fs::create_dir_all(&iconset).map_err(io_error("creating iconset"))?;
    for (size, name) in [
        (16, "icon_16x16.png"),
        (32, "icon_16x16@2x.png"),
        (32, "icon_32x32.png"),
        (64, "icon_32x32@2x.png"),
        (128, "icon_128x128.png"),
        (256, "icon_128x128@2x.png"),
        (256, "icon_256x256.png"),
        (512, "icon_256x256@2x.png"),
        (512, "icon_512x512.png"),
        (1024, "icon_512x512@2x.png"),
    ] {
        run_command(
            Command::new("sips")
                .args(["-z", &size.to_string(), &size.to_string()])
                .arg(root.join("../md-editor.png"))
                .arg("--out")
                .arg(iconset.join(name)),
            "creating app icon",
        )?;
    }
    run_command(
        Command::new("iconutil")
            .args(["-c", "icns"])
            .arg(&iconset)
            .arg("-o")
            .arg(resources.join("md-editor.icns")),
        "creating icns",
    )?;
    fs::remove_dir_all(iconset).map_err(io_error("removing iconset"))
}

fn powershell_archive(package: &Path, output: &Path) -> Result<(), String> {
    let script = format!(
        "Compress-Archive -Path '{}\\*' -DestinationPath '{}' -Force",
        package.display(),
        output.display()
    );
    run_command(
        Command::new("powershell").args(["-NoProfile", "-Command", &script]),
        "creating portable zip",
    )
}

fn tar_archive(package: &Path, output: &Path) -> Result<(), String> {
    let parent = package
        .parent()
        .ok_or_else(|| "package has no parent".to_string())?;
    run_command(
        Command::new("tar")
            .args(["-czf"])
            .arg(output)
            .arg("-C")
            .arg(parent)
            .arg(PRODUCT),
        "creating portable tarball",
    )
}

fn nsis_installer(
    root: &Path,
    dist: &Path,
    package: &Path,
    stem: &str,
    version: &str,
) -> Result<PathBuf, String> {
    let output = dist.join(format!("{stem}-setup.exe"));
    run_command(
        Command::new("makensis")
            .arg(format!("/DAPP_VERSION={version}"))
            .arg(format!("/DSOURCE_DIR={}", package.display()))
            .arg(format!("/DOUTPUT_FILE={}", output.display()))
            .arg(root.join("packaging/windows/installer.nsi")),
        "building NSIS installer",
    )?;
    require_file(&output)?;
    Ok(output)
}

fn appimage(root: &Path, dist: &Path, package: &Path, stem: &str) -> Result<PathBuf, String> {
    let app_dir = dist.join("MD_Editor.AppDir");
    recreate_dir(&app_dir)?;
    let output = dist.join(format!("{stem}.AppImage"));
    let linuxdeploy = env::var_os("LINUXDEPLOY").unwrap_or_else(|| "linuxdeploy".into());
    run_command(
        Command::new(linuxdeploy)
            .env("ARCH", env::consts::ARCH)
            .env("OUTPUT", &output)
            .arg("--appdir")
            .arg(&app_dir)
            .arg("--executable")
            .arg(package.join(PRODUCT))
            .arg("--desktop-file")
            .arg(root.join("packaging/linux/md-editor.desktop"))
            .arg("--icon-file")
            .arg(root.join("../md-editor.png"))
            .arg("--library")
            .arg(package.join("resources/libpdfium.so"))
            .args(["--output", "appimage"]),
        "building AppImage",
    )?;
    require_file(&output)?;
    fs::remove_dir_all(app_dir).map_err(io_error("removing AppDir"))?;
    Ok(output)
}

fn dmg(dist: &Path, package: &Path, stem: &str) -> Result<PathBuf, String> {
    let output = dist.join(format!("{stem}.dmg"));
    if output.exists() {
        fs::remove_file(&output).map_err(io_error("removing old DMG"))?;
    }
    run_command(
        Command::new("hdiutil")
            .args(["create", "-volname", "MD Editor", "-srcfolder"])
            .arg(package)
            .args(["-ov", "-format", "UDZO"])
            .arg(&output),
        "building DMG",
    )?;
    require_file(&output)?;
    Ok(output)
}

fn write_checksums(dist: &Path, artifacts: &[PathBuf]) -> Result<(), String> {
    let mut output = String::new();
    for artifact in artifacts {
        let mut file = File::open(artifact).map_err(io_error("opening artifact"))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = file
                .read(&mut buffer)
                .map_err(io_error("reading artifact"))?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        let name = artifact
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "artifact name is not UTF-8".to_string())?;
        output.push_str(&format!("{:x}  {name}\n", hasher.finalize()));
    }
    fs::write(dist.join("SHA256SUMS"), output).map_err(io_error("writing checksums"))
}

fn workspace_root() -> Result<PathBuf, String> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask has no workspace parent".to_string())
}

fn workspace_version(root: &Path) -> Result<String, String> {
    let manifest =
        fs::read_to_string(root.join("Cargo.toml")).map_err(io_error("reading manifest"))?;
    let mut workspace_package = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            workspace_package = line == "[workspace.package]";
        } else if workspace_package
            && line.starts_with("version")
            && let Some((_, value)) = line.split_once('=')
        {
            return Ok(value.trim().trim_matches('"').to_string());
        }
    }
    Err("workspace version not found".to_string())
}

fn release_arch() -> Result<&'static str, String> {
    match env::consts::ARCH {
        "x86_64" => Ok("x64"),
        "aarch64" => Ok("arm64"),
        other => Err(format!("unsupported host architecture: {other}")),
    }
}

fn recreate_dir(path: &Path) -> Result<(), String> {
    if path.exists() {
        fs::remove_dir_all(path).map_err(io_error("removing directory"))?;
    }
    fs::create_dir_all(path).map_err(io_error("creating directory"))
}

fn copy(source: &Path, destination: &Path) -> Result<(), String> {
    require_file(source)?;
    fs::copy(source, destination)
        .map(|_| ())
        .map_err(io_error("copying package file"))
}

fn require_file(path: &Path) -> Result<(), String> {
    if path.is_file() {
        Ok(())
    } else {
        Err(format!("required file missing: {}", path.display()))
    }
}

fn run_command(command: &mut Command, action: &str) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|error| format!("{action}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{action}: command exited with {status}"))
    }
}

fn io_error(action: &'static str) -> impl Fn(io::Error) -> String {
    move |error| format!("{action}: {error}")
}
