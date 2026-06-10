//! Vault module: paths, fs operations, search, reference repair.
//!
//! P2.T2: implementation lives in submodules; `application::VaultService` is
//! the public entry point. The re-exports below are transitional (die in
//! P2.T6).

mod fs;
mod paths;
mod reference_repair;
mod search;

pub use fs::{
    create_dir, create_file, delete_entry, get_backlinks, get_mixed_backlinks, list_vault,
    open_file, read_vault_image, rename_entry, save_file, save_file_with_markdown_link_targets,
    set_vault_root,
};
pub use paths::{
    is_image, list_all_md_files, list_all_pdf_files, path_to_relative_string, resolve_vault_path,
};
pub use reference_repair::repair_rename_references;
pub use search::{
    list_registered_pdf_paths, search_cached_pdf_text, search_result_preview, search_vault,
    search_vault_unified, search_vault_unified_query,
};

#[cfg(test)]
mod vault_scale_tests {
    use crate::state::AppState;
    use crate::vault::{
        delete_entry, list_vault, rename_entry, save_file, search_vault, set_vault_root,
    };

    #[test]
    fn test_vault_recursive_listings_and_operations() {
        let state =
            AppState::try_new_in_memory().expect("in-memory application state should initialize");
        let temp_dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("test_vault_listing");
        if temp_dir.exists() {
            let _ = std::fs::remove_dir_all(&temp_dir);
        }
        std::fs::create_dir_all(&temp_dir).unwrap();

        let vault_path_str = temp_dir.to_string_lossy().to_string();

        // Create nested subdirectories: sub_0, sub_0/sub_1, ..., sub_0/.../sub_9
        let mut current_sub = temp_dir.clone();
        for i in 0..10 {
            current_sub = current_sub.join(format!("sub_{}", i));
            std::fs::create_dir_all(&current_sub).unwrap();

            // Write 10 files in this sub directory: various extensions
            // .md, .pdf, .png, .txt (txt should be ignored!)
            std::fs::write(current_sub.join("file.md"), "Link to [[other_file]]").unwrap();
            std::fs::write(current_sub.join("image.png"), "fake image content").unwrap();
            std::fs::write(current_sub.join("document.pdf"), "fake pdf content").unwrap();
            std::fs::write(current_sub.join("ignored.txt"), "this should be ignored").unwrap();
            std::fs::write(
                current_sub.join(".hidden_file.md"),
                "this should be ignored",
            )
            .unwrap();
        }

        // Set vault root
        let entries = set_vault_root(&state, &vault_path_str).expect("Failed to set vault root");

        // Expected files to be included: 10 md files, 10 png files, 10 pdf files = 30 files, plus the 10 subdirectories!
        // Total entries should be 40!
        assert_eq!(entries.len(), 40);

        for entry in &entries {
            // Assert ignored extensions or hidden files are NOT in the list
            assert!(
                !entry.name.ends_with(".txt"),
                "TXT files must be ignored: {}",
                entry.name
            );
            assert!(
                !entry.name.starts_with('.'),
                "Hidden files must be ignored: {}",
                entry.name
            );

            if entry.is_dir {
                assert!(
                    entry.name.starts_with("sub_"),
                    "Directory name must start with sub_: {}",
                    entry.name
                );
            } else {
                let has_valid_ext = entry.name.ends_with(".md")
                    || entry.name.ends_with(".png")
                    || entry.name.ends_with(".pdf");
                assert!(
                    has_valid_ext,
                    "Invalid file extension in vault list: {}",
                    entry.name
                );
            }
        }

        // Assert sorting order of the listed entries: dirs first, then files case-insensitive
        for j in 0..39 {
            let current = &entries[j];
            let next = &entries[j + 1];
            match (current.is_dir, next.is_dir) {
                (true, false) => {} // Correct
                (false, true) => panic!("Directories must be listed before files!"),
                _ => {
                    // If both are dirs or both are files, they must be sorted alphabetically case-insensitively
                    assert!(
                        current.name.to_lowercase() <= next.name.to_lowercase(),
                        "Alphabetical sorting order violated! {} vs {}",
                        current.name,
                        next.name
                    );
                }
            }
        }

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_vault_fts5_indexing_and_search() {
        let state =
            AppState::try_new_in_memory().expect("in-memory application state should initialize");
        let temp_dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("test_vault_search");
        if temp_dir.exists() {
            let _ = std::fs::remove_dir_all(&temp_dir);
        }
        std::fs::create_dir_all(&temp_dir).unwrap();

        let vault_path_str = temp_dir.to_string_lossy().to_string();

        // 1. Create 300 markdown files with content that has specific search phrases
        // 100 with "rust guidelines", 100 with "md-editor space", 100 with "lorem ipsum"
        for i in 0..100 {
            let path_rust = temp_dir.join(format!("rust_file_{}.md", i));
            let path_space = temp_dir.join(format!("space_file_{}.md", i));
            let path_lorem = temp_dir.join(format!("lorem_file_{}.md", i));

            std::fs::write(
                &path_rust,
                format!(
                    "This is standard rust coding guidelines document version {}",
                    i
                ),
            )
            .unwrap();
            std::fs::write(
                &path_space,
                format!(
                    "We study md-editor space propulsion using quantum dynamics version {}",
                    i
                ),
            )
            .unwrap();
            std::fs::write(
                &path_lorem,
                format!(
                    "Lorem ipsum dolor sit amet, consectetur adipiscing elit version {}",
                    i
                ),
            )
            .unwrap();
        }

        // Trigger full vault index via set_vault_root
        set_vault_root(&state, &vault_path_str).expect("Failed to set vault root");

        // 2. Perform FTS5 searches
        let results_rust = search_vault(&state, "rust coding").expect("Search failed");
        assert_eq!(results_rust.len(), 100);
        for r in &results_rust {
            let lower_ctx = r.context.to_lowercase();
            assert!(lower_ctx.contains("rust") && lower_ctx.contains("coding"));
            assert!(r.context.contains("<b>") && r.context.contains("</b>"));
        }

        let results_space = search_vault(&state, "md-editor space").expect("Search failed");
        assert_eq!(results_space.len(), 100);
        for r in &results_space {
            let lower_ctx = r.context.to_lowercase();
            assert!(lower_ctx.contains("md-editor") && lower_ctx.contains("space"));
            assert!(r.context.contains("<b>") && r.context.contains("</b>"));
        }

        let results_lorem = search_vault(&state, "consectetur adipiscing").expect("Search failed");
        assert_eq!(results_lorem.len(), 100);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_vault_file_lifecycle_renames_deletes() {
        let state =
            AppState::try_new_in_memory().expect("in-memory application state should initialize");
        let temp_dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("test_vault_lifecycle");
        if temp_dir.exists() {
            let _ = std::fs::remove_dir_all(&temp_dir);
        }
        std::fs::create_dir_all(&temp_dir).unwrap();

        let vault_path_str = temp_dir.to_string_lossy().to_string();
        set_vault_root(&state, &vault_path_str).expect("Failed to set vault root");

        // 1. Mass create 200 files
        for i in 0..200 {
            let rel_path = format!("note_{}.md", i);
            // We write content linking to next note to test index updates
            let next_note = format!("note_{}", (i + 1) % 200);
            let content = format!("Link to [[{}]].", next_note);
            save_file(&state, &rel_path, &content).expect("Failed to save file");
        }

        // Verify index backlinks
        {
            let index = state.file_index.lock().unwrap();
            for i in 0..200 {
                let abs_path = temp_dir.join(format!("note_{}.md", i));
                assert_eq!(index.get_backlinks(&abs_path).len(), 1);
            }
        }

        // 2. Mass rename 200 files: note_i.md -> renamed_note_i.md
        for i in 0..200 {
            let old_path = format!("note_{}.md", i);
            let new_path = format!("renamed_note_{}.md", i);
            rename_entry(&state, &old_path, &new_path).expect("Failed to rename file");
        }

        // Verify renames and index updates
        {
            // Old files should not be listed, and new files should be present
            let entries = list_vault(&state).expect("Failed to list vault");
            assert_eq!(entries.len(), 200);
            for entry in &entries {
                assert!(entry.name.starts_with("renamed_note_"));
            }

            // Old index entries should be removed from the FileIndex
            let index = state.file_index.lock().unwrap();
            for i in 0..200 {
                let old_abs = temp_dir.join(format!("note_{}.md", i));
                assert!(index.get_outgoing_links(&old_abs).is_empty());
            }
        }

        // 3. Mass delete 200 files
        for i in 0..200 {
            let rel_path = format!("renamed_note_{}.md", i);
            delete_entry(&state, &rel_path).expect("Failed to delete file");
        }

        // Verify empty vault and empty index
        let entries = list_vault(&state).expect("Failed to list vault");
        assert!(entries.is_empty());

        {
            let index = state.file_index.lock().unwrap();
            for i in 0..200 {
                let new_abs = temp_dir.join(format!("renamed_note_{}.md", i));
                assert!(index.get_outgoing_links(&new_abs).is_empty());
                assert!(index.get_backlinks(&new_abs).is_empty());
            }
        }

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
