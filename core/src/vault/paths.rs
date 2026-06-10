use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::FileEntry;
use crate::domain::VaultPath;

const IMAGE_EXTENSIONS: [&str; 6] = ["jpeg", "jpg", "png", "svg", "webp", "avif"];

pub fn resolve_vault_path(vault_root: &Path, vault_path: &str) -> Result<PathBuf, String> {
    let vault_path = VaultPath::new(vault_path).map_err(|error| error.to_string())?;
    Ok(vault_root.join(vault_path.as_path()))
}

pub(super) fn read_file(abs_path: &Path) -> Result<String, String> {
    fs::read_to_string(abs_path)
        .map_err(|e| format!("Failed to read file {}: {}", abs_path.display(), e))
}

pub(super) fn read_image(abs_path: &Path) -> Result<Vec<u8>, String> {
    if !abs_path
        .extension()
        .is_some_and(|ext| is_image(ext.to_str().unwrap_or("")))
    {
        return Err(format!("Not an image: {}", abs_path.display()));
    }
    fs::read(abs_path).map_err(|e| format!("Failed to read image {}: {}", abs_path.display(), e))
}

pub(super) fn write_file(abs_path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = abs_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {}", e))?;
    }
    fs::write(abs_path, content)
        .map_err(|e| format!("Failed to write file {}: {}", abs_path.display(), e))
}

pub fn is_image(ext: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&ext)
}

pub(super) fn is_markdown_path(abs_path: &Path) -> bool {
    abs_path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext == "md" || ext == "markdown")
}

pub(super) fn list_vault_entries(root: &Path) -> Result<Vec<FileEntry>, String> {
    let mut entries = Vec::new();
    list_vault_recursive(root, root, &mut entries)?;
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    Ok(entries)
}

fn list_vault_recursive(
    root: &Path,
    dir: &Path,
    entries: &mut Vec<FileEntry>,
) -> Result<(), String> {
    let read_dir = fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?;

    for entry in read_dir {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            entries.push(FileEntry {
                path: path_to_relative_string(&path, root),
                name,
                is_dir: true,
            });
            list_vault_recursive(root, &path, entries)?;
        } else if path
            .extension()
            .map(|ext| {
                ext == "md"
                    || ext == "markdown"
                    || ext == "pdf"
                    || is_image(ext.to_str().unwrap_or(""))
            })
            .unwrap_or(false)
        {
            entries.push(FileEntry {
                path: path_to_relative_string(&path, root),
                name,
                is_dir: false,
            });
        }
    }

    Ok(())
}

pub fn path_to_relative_string(abs_path: &Path, root: &Path) -> String {
    abs_path
        .strip_prefix(root)
        .unwrap_or(abs_path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn list_all_md_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    list_all_md_files_recursive(root, &mut files)?;
    Ok(files)
}

fn list_all_md_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let read_dir = fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?;

    for entry in read_dir {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            list_all_md_files_recursive(&path, files)?;
        } else if path
            .extension()
            .is_some_and(|ext| ext == "md" || ext == "markdown")
        {
            files.push(path);
        }
    }
    Ok(())
}

pub fn list_all_pdf_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    list_all_pdf_files_recursive(root, &mut files)?;
    Ok(files)
}

fn list_all_pdf_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let read_dir = fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?;

    for entry in read_dir {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            list_all_pdf_files_recursive(&path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "pdf") {
            files.push(path);
        }
    }
    Ok(())
}
