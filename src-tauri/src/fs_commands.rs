use std::fs;
use std::path::{Path, PathBuf};

use crate::ipc_types::{FileEntry, SearchResult};

const IMAGE_EXTENSIONS: [&str; 6] = ["jpeg", "jpg", "png", "svg", "webp", "avif"];

/// Search for a query in all markdown files in the vault.
pub fn search_vault(root: &Path, query: &str) -> Result<Vec<SearchResult>, String> {
    let mut results = Vec::new();
    let query_lower = query.to_lowercase();
    let md_files = list_all_md_files(root)?;

    for path in md_files {
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read file {}: {}", path.display(), e))?;
        
        let relative_path = path_to_relative_string(&path, root);
        
        for (line_num, line) in content.lines().enumerate() {
            if line.to_lowercase().contains(&query_lower) {
                results.push(SearchResult {
                    path: relative_path.clone(),
                    line: line_num + 1,
                    context: line.trim().to_string(),
                });
            }
        }
    }
    Ok(results)
}

fn list_all_md_files(root: &Path) -> Result<Vec<PathBuf>, String> {
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
        } else if path.extension().map_or(false, |e| e == "md" || e == "markdown") {
            files.push(path);
        }
    }
    Ok(())
}

/// Read file content from disk.
pub fn read_file(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("Failed to read file {}: {}", path.display(), e))
}

pub fn read_image(path: &PathBuf) -> Result<Vec<u8>, String> {
    if !is_image(path.extension().unwrap().to_str().unwrap()) {
        return Err(format!("The file is not an image: {}", path.display()));
    }
    return fs::read(path)
        .map_err(|e| format!("Failed to read image file {}: {}", path.display(), e));
}

/// Write content to disk.
pub fn write_file(path: &Path, content: &str) -> Result<(), String> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {}", e))?;
    }
    fs::write(path, content).map_err(|e| format!("Failed to write file {}: {}", path.display(), e))
}

/// Create a new empty file.
pub fn create_file(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Err(format!("File already exists: {}", path.display()));
    }
    write_file(path, "")
}

/// Create a new directory.
pub fn create_dir(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Err(format!("Directory already exists: {}", path.display()));
    }
    fs::create_dir_all(path).map_err(|e| format!("Failed to create directory {}: {}", path.display(), e))
}

/// Rename a file or directory.
pub fn rename_file(old_path: &Path, new_path: &Path) -> Result<(), String> {
    if new_path.exists() {
        return Err(format!("Target already exists: {}", new_path.display()));
    }
    fs::rename(old_path, new_path)
        .map_err(|e| format!("Failed to rename {}: {}", old_path.display(), e))
}

/// Delete a file or directory.
pub fn delete_entry(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        fs::remove_dir_all(path).map_err(|e| format!("Failed to delete directory {}: {}", path.display(), e))
    } else {
        fs::remove_file(path).map_err(|e| format!("Failed to delete file {}: {}", path.display(), e))
    }
}

/// Recursively list all markdown files in a directory.
pub fn list_vault(root: &Path) -> Result<Vec<FileEntry>, String> {
    let mut entries = Vec::new();
    list_vault_recursive(root, root, &mut entries)?;
    // Sort: directories first, then alphabetically
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

        // Skip hidden files/dirs
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
            .map(|e| e == "md" || e == "markdown" || is_image(&e.to_str().unwrap()))
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

fn path_to_relative_string(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Resolve a vault-relative path to an absolute path.
pub fn resolve_vault_path(vault_root: &Path, relative_path: &str) -> PathBuf {
    vault_root.join(relative_path)
}

pub fn is_image(extention: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&extention)
}
