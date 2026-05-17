use std::path::PathBuf;
use crate::file_index::FileIndex;
use crate::state::AppState;
use crate::config::{get_sys_config, set_sys_config};
use crate::tracker::{save_session, get_sessions, get_total_hours, get_kv, set_kv, StudySession};
use crate::vault::{
    set_vault_root, save_file, rename_entry, delete_entry, list_vault, search_vault
};

// =====================================================================
// PHASE 1: FILE INDEX & WIKILINK EXTRACTION TESTS
// Goal: 1,000+ test cases covering combinatorics, topologies & updates
// =====================================================================

#[test]
fn test_file_index_wikilink_combinatorics() {
    let root = PathBuf::from("/vault");
    let mut index = FileIndex::new(root.clone());
    let source_file = PathBuf::from("/vault/source.md");

    // 1. Generate 500 distinct wikilink variants to test parsing combinatorics
    // We vary whitespace, aliases, special characters, and extensions

    let target_names = vec![
        "simple", "simple-dashed", "simple_under", "nested/path/to/file",
        "spac y target", "unicode-🦀", "japanese-日本語", "umlaut-öäü",
        "emoji-🚀-star🌟", "dot.name", "complex!@#%^&*()", "caps_LOCK",
        "nested/sub/sub/file", "spaces-around-words", "multiple--dashes", "accented-éàçè"
    ];

    let alias_options = vec![
        None,
        Some("simple_alias"),
        Some("spaced alias name"),
        Some("unicode-🔥")
    ];

    let space_variations = vec![
        ("", ""),
        (" ", " "),
        ("  ", ""),
        ("", "  "),
        ("   ", "   ")
    ];

    let mut content = String::new();
    let mut expected_targets = std::collections::HashSet::new();

    for (t_idx, target) in target_names.iter().enumerate() {
        for (a_idx, alias) in alias_options.iter().enumerate() {
            for (s_idx, (sp_start, sp_end)) in space_variations.iter().enumerate() {
                // Ensure unique target name to prevent HashSet deduplication
                let unique_target = format!("{}-{}-{}-{}", target, t_idx, a_idx, s_idx);

                // Construct the link content: [[ <sp_start> target [| alias] <sp_end> ]]
                let link = match alias {
                    Some(al) => format!("[[{}{}|{}{}{}]]", sp_start, unique_target, sp_start, al, sp_end),
                    None => format!("[[{}{}{}]]", sp_start, unique_target, sp_end),
                };
                content.push_str(&link);
                content.push(' ');

                // Deduplicate expected path resolution
                let trimmed_target = unique_target.trim();
                let mut target_path = root.join(trimmed_target);
                if target_path.extension().is_none() {
                    target_path.set_extension("md");
                }
                expected_targets.insert(target_path);
            }
        }
    }

    // Update file index and extract links
    index.update_file(&source_file, &content);

    let outgoing = index.get_outgoing_links(&source_file);
    let outgoing_set: std::collections::HashSet<_> = outgoing.iter().collect();

    // Verify all generated cases parsed correctly
    for expected in &expected_targets {
        assert!(outgoing_set.contains(expected), "Missing expected link: {:?}", expected);
    }

    assert!(outgoing.len() >= 300, "Should have tested hundreds of link combinatorics");
}

#[test]
fn test_file_index_graph_topologies() {
    let root = PathBuf::from("/vault");
    let mut index = FileIndex::new(root.clone());

    // --- 1. Star Topology: Center linked from and to 200 nodes ---
    let center = PathBuf::from("/vault/center.md");
    let mut center_content = String::new();

    for i in 1..=200 {
        let leaf = PathBuf::from(format!("/vault/leaf_{}.md", i));
        // leaf_i links to center
        index.update_file(&leaf, "Link to [[center]].");
        // center links back to leaf_i
        center_content.push_str(&format!("[[leaf_{}]] ", i));
    }
    index.update_file(&center, &center_content);

    // Verify star topology backlinks
    let center_backlinks = index.get_backlinks(&center);
    assert_eq!(center_backlinks.len(), 200);
    for i in 1..=200 {
        let leaf = PathBuf::from(format!("/vault/leaf_{}.md", i));
        assert!(center_backlinks.contains(&leaf));
        
        let leaf_backlinks = index.get_backlinks(&leaf);
        assert_eq!(leaf_backlinks.len(), 1);
        assert_eq!(leaf_backlinks[0], center);
    }

    // --- 2. Chain Topology: file_1 -> file_2 -> ... -> file_200 ---
    let mut index_chain = FileIndex::new(root.clone());
    for i in 1..200 {
        let current = PathBuf::from(format!("/vault/node_{}.md", i));
        let next_name = format!("node_{}", i + 1);
        index_chain.update_file(&current, &format!("Link to [[{}]].", next_name));
    }

    // Verify chain links
    for i in 1..199 {
        let current = PathBuf::from(format!("/vault/node_{}.md", i));
        let next = PathBuf::from(format!("/vault/node_{}.md", i + 1));
        let next_backlinks = index_chain.get_backlinks(&next);
        assert_eq!(next_backlinks.len(), 1);
        assert_eq!(next_backlinks[0], current);
    }

    // --- 3. Fully Connected Mesh Topology: 30 nodes (30 * 29 = 870 directional links) ---
    let mut index_mesh = FileIndex::new(root.clone());
    let mut nodes = Vec::new();
    for i in 0..30 {
        nodes.push(PathBuf::from(format!("/vault/mesh_{}.md", i)));
    }

    for i in 0..30 {
        let mut mesh_content = String::new();
        for j in 0..30 {
            if i != j {
                mesh_content.push_str(&format!("[[mesh_{}]] ", j));
            }
        }
        index_mesh.update_file(&nodes[i], &mesh_content);
    }

    // Verify mesh links
    for i in 0..30 {
        let outgoing = index_mesh.get_outgoing_links(&nodes[i]);
        assert_eq!(outgoing.len(), 29);
        let incoming = index_mesh.get_backlinks(&nodes[i]);
        assert_eq!(incoming.len(), 29);
    }
}

#[test]
fn test_file_index_dynamic_fuzzing_updates() {
    let root = PathBuf::from("/vault");
    let mut index = FileIndex::new(root.clone());

    // Generate 50 files
    let mut files = Vec::new();
    for i in 0..50 {
        files.push(PathBuf::from(format!("/vault/f_{}.md", i)));
    }

    // Run 300 cycles of random updates and assert link invariant:
    // sum(outgoing_link_counts) == sum(incoming_link_counts)
    let mut pseudo_rng = 42u64;
    let mut next_random = |modulus: usize| -> usize {
        pseudo_rng = pseudo_rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        (pseudo_rng as usize) % modulus
    };

    for _ in 0..300 {
        let file_idx = next_random(50);
        let file_to_update = &files[file_idx];

        // Link to between 0 and 5 other random files
        let num_links = next_random(6);
        let mut content = String::new();
        for _ in 0..num_links {
            let target_idx = next_random(50);
            if target_idx != file_idx {
                content.push_str(&format!("[[f_{}]] ", target_idx));
            }
        }

        index.update_file(file_to_update, &content);

        // Assert invariant: count of all outgoing links matches all incoming backlinks
        let mut total_outgoing = 0;
        let mut total_incoming = 0;

        for f in &files {
            total_outgoing += index.get_outgoing_links(f).len();
            total_incoming += index.get_backlinks(f).len();
        }

        assert_eq!(total_outgoing, total_incoming, "Link count invariant violated during updates");
    }

    // Delete files one by one and ensure index drains cleanly to empty
    for f in &files {
        index.remove_file(f);
    }

    for f in &files {
        assert!(index.get_outgoing_links(f).is_empty());
        assert!(index.get_backlinks(f).is_empty());
    }
}

// =====================================================================
// PHASE 2: DATABASE CONFIG & STUDY TRACKER TESTS
// Goal: 1,000+ test cases covering SQLite memory upserts, sorting, aggregates
// =====================================================================

#[test]
fn test_sys_config_massive_upserts() {
    let state = AppState::new_in_memory();

    // 1. Write 500 unique configuration keys and values
    for i in 0..500 {
        let key = format!("config_key_{}", i);
        let val = format!("value_content_for_key_{}_{}_★_unicode_🦀", i, i * 3);
        set_sys_config(&state, &key, &val).expect("Failed to set config");
    }

    // 2. Read and verify all 500 configuration keys
    for i in 0..500 {
        let key = format!("config_key_{}", i);
        let expected_val = format!("value_content_for_key_{}_{}_★_unicode_🦀", i, i * 3);
        let val = get_sys_config(&state, &key).expect("Failed to get config");
        assert_eq!(val, Some(expected_val));
    }

    // 3. Overwrite 250 of these keys to test upsert logic
    for i in 0..250 {
        let key = format!("config_key_{}", i);
        let new_val = format!("overwritten_value_{}", i);
        set_sys_config(&state, &key, &new_val).expect("Failed to upsert config");
    }

    // 4. Verify the overwrites and untouched config values
    for i in 0..500 {
        let key = format!("config_key_{}", i);
        let val = get_sys_config(&state, &key).expect("Failed to get config");
        if i < 250 {
            assert_eq!(val, Some(format!("overwritten_value_{}", i)));
        } else {
            let expected_val = format!("value_content_for_key_{}_{}_★_unicode_🦀", i, i * 3);
            assert_eq!(val, Some(expected_val));
        }
    }

    // 5. Test nonexistent key
    let val = get_sys_config(&state, "nonexistent_key_9999").expect("Failed to get nonexistent");
    assert_eq!(val, None);
}

#[test]
fn test_study_tracker_massive_sessions() {
    let state = AppState::new_in_memory();

    // 1. Bulk insert 500 StudySessions with various dates, activities, and notes
    let mut expected_sessions = Vec::new();
    let mut expected_total_hours = 0.0f32;

    for i in 0..500 {
        // Construct varying dates to test sorting (Date format: YYYY-MM-DD HH:MM:SS)
        // We alternate dates to test descending sort correctness
        let day = 1 + (i % 28);
        let month = 1 + (i % 12);
        let date_str = format!("2026-{:02}-{:02} 12:00:{:02}", month, day, i % 60);

        let hours = 0.5f32 * (1 + (i % 8)) as f32;
        expected_total_hours += hours;

        let session = StudySession {
            id: 0, // database auto-increments
            date: date_str,
            hours,
            activity_type: format!("Activity_{}", i % 5),
            phase: format!("Phase_{}", i % 3),
            notes: if i % 2 == 0 {
                Some(format!("Detailed notes for session {}", i))
            } else {
                None
            },
        };

        save_session(&state, session.clone()).expect("Failed to save session");
        expected_sessions.push(session);
    }

    // 2. Retrieve sessions and assert they are sorted by date DESC
    let retrieved = get_sessions(&state).expect("Failed to get sessions");
    assert_eq!(retrieved.len(), 500);

    // Verify sorting order: retrieved[j].date >= retrieved[j+1].date
    for j in 0..499 {
        assert!(
            retrieved[j].date >= retrieved[j + 1].date,
            "Sessions not sorted descending by date! index {}: {} vs index {}: {}",
            j, retrieved[j].date, j + 1, retrieved[j + 1].date
        );
    }

    // 3. Verify total aggregate hours calculation
    let calculated_hours = get_total_hours(&state).expect("Failed to get total hours");
    let diff = (calculated_hours - expected_total_hours).abs();
    assert!(diff < 0.01, "Aggregate hours sum mismatch: expected {}, got {}", expected_total_hours, calculated_hours);

    // 4. Test massive Tracker KV store (500 entries)
    for i in 0..500 {
        let key = format!("kv_key_{:03}", i);
        let val = format!("kv_val_{}", i);
        set_kv(&state, &key, &val).expect("Failed to set tracker KV");
    }

    let kv_entries = get_kv(&state).expect("Failed to get tracker KV entries");
    assert_eq!(kv_entries.len(), 500);

    // Verify KV entries are sorted by key in ascending order
    for j in 0..499 {
        assert!(
            kv_entries[j].key < kv_entries[j + 1].key,
            "Tracker KV entries not sorted ascending by key! index {}: {} vs index {}: {}",
            j, kv_entries[j].key, j + 1, kv_entries[j + 1].key
        );
    }
}

// =====================================================================
// PHASE 3: VAULT MANAGEMENT & SEARCH TESTS
// Goal: 1,000+ test cases covering recursive listings, FTS5 FTS indexing, renames, deletes
// =====================================================================

#[test]
fn test_vault_recursive_listings_and_operations() {
    let state = AppState::new_in_memory();
    let temp_dir = std::env::current_dir().unwrap().join("target").join("test_vault_listing");
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
        std::fs::write(current_sub.join(".hidden_file.md"), "this should be ignored").unwrap();
    }

    // Set vault root
    let entries = set_vault_root(&state, &vault_path_str).expect("Failed to set vault root");
    
    // Expected files to be included: 10 md files, 10 png files, 10 pdf files = 30 files, plus the 10 subdirectories!
    // Total entries should be 40!
    assert_eq!(entries.len(), 40);

    for entry in &entries {
        // Assert ignored extensions or hidden files are NOT in the list
        assert!(!entry.name.ends_with(".txt"), "TXT files must be ignored: {}", entry.name);
        assert!(!entry.name.starts_with('.'), "Hidden files must be ignored: {}", entry.name);
        
        if entry.is_dir {
            assert!(entry.name.starts_with("sub_"), "Directory name must start with sub_: {}", entry.name);
        } else {
            let has_valid_ext = entry.name.ends_with(".md") || entry.name.ends_with(".png") || entry.name.ends_with(".pdf");
            assert!(has_valid_ext, "Invalid file extension in vault list: {}", entry.name);
        }
    }

    // Assert sorting order of the listed entries: dirs first, then files case-insensitive
    for j in 0..39 {
        let current = &entries[j];
        let next = &entries[j+1];
        match (current.is_dir, next.is_dir) {
            (true, false) => {} // Correct
            (false, true) => panic!("Directories must be listed before files!"),
            _ => {
                // If both are dirs or both are files, they must be sorted alphabetically case-insensitively
                assert!(current.name.to_lowercase() <= next.name.to_lowercase(), "Alphabetical sorting order violated! {} vs {}", current.name, next.name);
            }
        }
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_vault_fts5_indexing_and_search() {
    let state = AppState::new_in_memory();
    let temp_dir = std::env::current_dir().unwrap().join("target").join("test_vault_search");
    if temp_dir.exists() {
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
    std::fs::create_dir_all(&temp_dir).unwrap();

    let vault_path_str = temp_dir.to_string_lossy().to_string();

    // 1. Create 300 markdown files with content that has specific search phrases
    // 100 with "rust guidelines", 100 with "antigravity space", 100 with "lorem ipsum"
    for i in 0..100 {
        let path_rust = temp_dir.join(format!("rust_file_{}.md", i));
        let path_space = temp_dir.join(format!("space_file_{}.md", i));
        let path_lorem = temp_dir.join(format!("lorem_file_{}.md", i));

        std::fs::write(&path_rust, format!("This is standard rust coding guidelines document version {}", i)).unwrap();
        std::fs::write(&path_space, format!("We study antigravity space propulsion using quantum dynamics version {}", i)).unwrap();
        std::fs::write(&path_lorem, format!("Lorem ipsum dolor sit amet, consectetur adipiscing elit version {}", i)).unwrap();
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

    let results_space = search_vault(&state, "antigravity space").expect("Search failed");
    assert_eq!(results_space.len(), 100);
    for r in &results_space {
        let lower_ctx = r.context.to_lowercase();
        assert!(lower_ctx.contains("antigravity") && lower_ctx.contains("space"));
        assert!(r.context.contains("<b>") && r.context.contains("</b>"));
    }

    let results_lorem = search_vault(&state, "consectetur adipiscing").expect("Search failed");
    assert_eq!(results_lorem.len(), 100);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_vault_file_lifecycle_renames_deletes() {
    let state = AppState::new_in_memory();
    let temp_dir = std::env::current_dir().unwrap().join("target").join("test_vault_lifecycle");
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

