use pdfium_render::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::OnceLock;

static PDFIUM_GLOBAL_LOCK: Mutex<()> = Mutex::new(());

pub fn with_pdfium_access<T>(f: impl FnOnce() -> T) -> T {
    let _guard = PDFIUM_GLOBAL_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f()
}

pub fn bind_pdfium() -> Result<Pdfium, String> {
    static BIND_RESULT: OnceLock<Result<(), String>> = OnceLock::new();

    let res = BIND_RESULT.get_or_init(|| {
        let lib_name = Pdfium::pdfium_platform_library_name();
        let exe_path = std::env::current_exe().ok();
        let candidates = pdfium_library_candidates(
            exe_path.as_deref(),
            Path::new(env!("CARGO_MANIFEST_DIR")),
            Path::new(&lib_name),
        );

        let mut bound = false;
        for candidate in candidates {
            if candidate.exists() {
                match Pdfium::bind_to_library(candidate) {
                    Ok(bindings) => {
                        let _ = Pdfium::new(bindings);
                        bound = true;
                        break;
                    }
                    Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => {
                        bound = true;
                        break;
                    }
                    Err(_) => {}
                }
            }
        }

        if !bound {
            match Pdfium::bind_to_library(lib_name) {
                Ok(bindings) => {
                    let _ = Pdfium::new(bindings);
                }
                Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => {}
                Err(e) => {
                    return Err(format!("{e:?}"));
                }
            }
        }
        Ok(())
    });

    match res {
        Ok(()) => Ok(Pdfium::default()),
        Err(e) => Err(e.clone()),
    }
}

pub fn pdfium_library_candidates(
    exe_path: Option<&Path>,
    manifest_dir: &Path,
    lib_name: &Path,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(exe_dir) = exe_path.and_then(Path::parent) {
        candidates.push(exe_dir.join("resources").join(lib_name));
        candidates.push(exe_dir.join(lib_name));

        if exe_dir.file_name().is_some_and(|name| name == "MacOS")
            && exe_dir
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "Contents")
            && let Some(contents_dir) = exe_dir.parent()
        {
            candidates.push(contents_dir.join("Resources").join(lib_name));
        }
    }

    candidates.push(manifest_dir.join("pdfium").join(lib_name));
    candidates
}
