//! Unit tests for `tracker_sidecar`: export on save/delete, missing-file recovery, three-way merge, and malformed JSONL resilience.

use std::path::{Path, PathBuf};

use super::*;
use crate::tracker::{delete_session, get_sessions, save_session};

fn base(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("md_tracker_sidecar_{tag}_{}", uuid::Uuid::new_v4()))
}

fn session(uid: &str, date: &str, hours: f32, notes: &str, updated_at: i64) -> StudySession {
    StudySession {
        id: 0,
        uid: uid.to_string(),
        date: date.to_string(),
        hours,
        activity_type: "Study".to_string(),
        phase: "Focus".to_string(),
        notes: Some(notes.to_string()),
        updated_at,
    }
}

struct Machine {
    state: AppState,
}

impl Machine {
    fn new(vault: &Path) -> Self {
        std::fs::create_dir_all(vault).unwrap();
        let state = AppState::new_in_memory();
        *state.vault_root.lock().unwrap() = Some(vault.to_path_buf());
        Self { state }
    }

    fn uids(&self) -> Vec<String> {
        let mut uids: Vec<String> = get_sessions(&self.state)
            .unwrap()
            .into_iter()
            .map(|s| s.uid)
            .collect();
        uids.sort();
        uids
    }
}

#[test]
fn save_and_delete_export_sorted_jsonl_and_missing_file_rewrites() {
    let dir = base("export");
    let vault = dir.join("vault");
    let m = Machine::new(&vault);

    save_session(&m.state, session("u2", "2026-11-26", 1.5, "second", 20)).unwrap();
    save_session(&m.state, session("u1", "2026-11-25", 1.0, "first", 10)).unwrap();

    let file = sidecar_path(&vault);
    assert!(file.exists());
    assert!(vault.join(SIDECAR_DIR).join(".gitattributes").exists());
    let lines: Vec<String> = std::fs::read_to_string(&file)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("\"u1\""));
    assert!(lines[1].contains("\"u2\""));

    // Deleting the sidecar file does NOT delete SQLite sessions; sync rewrites it.
    std::fs::remove_file(&file).unwrap();
    assert!(!m.state.sync_tracker_sidecar().unwrap());
    assert!(file.exists());
    assert_eq!(m.uids(), vec!["u1", "u2"]);

    // Deleting a session in SQLite removes it from the sidecar.
    let id_u1 = get_sessions(&m.state)
        .unwrap()
        .into_iter()
        .find(|s| s.uid == "u1")
        .unwrap()
        .id;
    delete_session(&m.state, id_u1).unwrap();
    let after = std::fs::read_to_string(&file).unwrap();
    assert!(!after.contains("\"u1\""));
    assert!(after.contains("\"u2\""));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn three_way_merge_across_two_machines_handles_additions_and_deletions() {
    let dir = base("merge");
    let vault_a = dir.join("vault_a");
    let vault_b = dir.join("vault_b");

    let a = Machine::new(&vault_a);
    let b = Machine::new(&vault_b);

    // Seed shared session s1 on A and sync to B.
    save_session(&a.state, session("s1", "2026-11-20", 1.0, "shared", 10)).unwrap();
    save_session(
        &a.state,
        session("s2", "2026-11-21", 2.0, "to delete on A", 10),
    )
    .unwrap();
    std::fs::create_dir_all(sidecar_path(&vault_b).parent().unwrap()).unwrap();
    std::fs::copy(sidecar_path(&vault_a), sidecar_path(&vault_b)).unwrap();
    assert!(b.state.sync_tracker_sidecar().unwrap());
    assert_eq!(b.uids(), vec!["s1", "s2"]);

    // Machine A deletes s2 and adds s3.
    let s2_id = get_sessions(&a.state)
        .unwrap()
        .into_iter()
        .find(|s| s.uid == "s2")
        .unwrap()
        .id;
    delete_session(&a.state, s2_id).unwrap();
    save_session(&a.state, session("s3", "2026-11-22", 1.5, "from A", 20)).unwrap();

    // Machine B has an unexported local session s4 in SQLite (or file never saw it yet).
    let b_scope = b.state.current_tracker_scope().unwrap();
    {
        let db = b.state.db.lock().unwrap();
        db.execute(
            "INSERT INTO tracker_sessions
                (date, hours, activity_type, phase, notes, uid, updated_at, vault_scope)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                "2026-11-23",
                0.5_f32,
                "Study",
                "Focus",
                Some("from B"),
                "s4",
                25_i64,
                b_scope,
            ],
        )
        .unwrap();
    }
    // B receives A's sidecar: s2 (deleted on A) is pruned, s3 (added on A) is imported, s4 (local new) survives.
    std::fs::copy(sidecar_path(&vault_a), sidecar_path(&vault_b)).unwrap();
    assert!(b.state.sync_tracker_sidecar().unwrap());
    assert_eq!(b.uids(), vec!["s1", "s3", "s4"]);

    // Now copy B's merged file back to A and sync A: A receives s4.
    std::fs::copy(sidecar_path(&vault_b), sidecar_path(&vault_a)).unwrap();
    assert!(a.state.sync_tracker_sidecar().unwrap());
    assert_eq!(a.uids(), vec!["s1", "s3", "s4"]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn skips_malformed_jsonl_lines_and_keeps_latest_updated_at() {
    let dir = base("malformed");
    let vault = dir.join("vault");
    let m = Machine::new(&vault);

    let file = sidecar_path(&vault);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        &file,
        concat!(
            "not json at all\n",
            "{\"uid\":\"\",\"date\":\"2026-11-26\",\"hours\":1.0,\"activity_type\":\"Study\",\"phase\":\"Focus\",\"notes\":null,\"updated_at\":1}\n",
            "{\"uid\":\"dup\",\"date\":\"2026-11-26\",\"hours\":1.0,\"activity_type\":\"Study\",\"phase\":\"Focus\",\"notes\":\"old\",\"updated_at\":10}\n",
            "{\"uid\":\"dup\",\"date\":\"2026-11-26\",\"hours\":2.5,\"activity_type\":\"Study\",\"phase\":\"Focus\",\"notes\":\"new\",\"updated_at\":20}\n",
        ),
    )
    .unwrap();

    assert!(m.state.sync_tracker_sidecar().unwrap());
    let loaded = get_sessions(&m.state).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].uid, "dup");
    assert_eq!(loaded[0].notes.as_deref(), Some("new"));
    assert!((loaded[0].hours - 2.5).abs() < 0.001);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn switching_vaults_isolates_sessions_and_does_not_leak_sidecar() {
    let dir = base("cross_vault");
    let vault_alpha = dir.join("alpha");
    let vault_beta = dir.join("beta");
    std::fs::create_dir_all(&vault_alpha).unwrap();
    std::fs::create_dir_all(&vault_beta).unwrap();

    let state = AppState::new_in_memory();
    crate::vault::set_vault_root(&state, vault_alpha.to_str().unwrap()).unwrap();
    save_session(
        &state,
        session("alpha-1", "2026-11-26", 2.0, "alpha study", 10),
    )
    .unwrap();
    assert!(sidecar_path(&vault_alpha).exists());
    assert_eq!(get_sessions(&state).unwrap().len(), 1);
    assert!((crate::tracker::get_total_hours(&state).unwrap() - 2.0).abs() < 0.001);

    // Open brand-new empty Vault Beta on the same AppState.
    crate::vault::set_vault_root(&state, vault_beta.to_str().unwrap()).unwrap();
    assert!(
        !sidecar_path(&vault_beta).exists(),
        "Vault Alpha sessions must not create a sidecar in Vault Beta"
    );
    assert!(get_sessions(&state).unwrap().is_empty());
    assert!(crate::tracker::get_total_hours(&state).unwrap().abs() < 0.001);

    // Log a session in Vault Beta and ensure Vault Alpha stays intact when switching back.
    save_session(
        &state,
        session("beta-1", "2026-11-26", 1.0, "beta study", 20),
    )
    .unwrap();
    assert!(sidecar_path(&vault_beta).exists());

    crate::vault::set_vault_root(&state, vault_alpha.to_str().unwrap()).unwrap();
    let alpha_sessions = get_sessions(&state).unwrap();
    assert_eq!(alpha_sessions.len(), 1);
    assert_eq!(alpha_sessions[0].uid, "alpha-1");

    crate::vault::set_vault_root(&state, vault_beta.to_str().unwrap()).unwrap();
    let beta_sessions = get_sessions(&state).unwrap();
    assert_eq!(beta_sessions.len(), 1);
    assert_eq!(beta_sessions[0].uid, "beta-1");

    let _ = std::fs::remove_dir_all(&dir);
}
