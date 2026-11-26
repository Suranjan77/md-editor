//! Unit tests for `daily`: task rollover, callout synthesis brief, code-block exclusion, and idempotency.

use std::path::PathBuf;

use super::*;

fn temp_vault(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("md_daily_{tag}_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn state_for(root: &Path) -> AppState {
    let state = AppState::new_in_memory();
    crate::vault::set_vault_root(&state, &root.to_string_lossy()).unwrap();
    state
}

#[test]
fn creates_daily_note_and_rolls_over_latest_unchecked_tasks_only() {
    let root = temp_vault("rollover");
    std::fs::create_dir_all(root.join("daily")).unwrap();
    std::fs::write(
        root.join("daily/2026-11-20.md"),
        "# 2026-11-20\n\n- [ ] Very old task\n",
    )
    .unwrap();
    std::fs::write(
        root.join("daily/2026-11-25.md"),
        concat!(
            "# 2026-11-25\n\n",
            "- [x] Completed task\n",
            "- [ ] Review attention math\n",
            "* [ ] Benchmark cache hit rate\n",
            "- [ ]   \n",
            "```markdown\n",
            "- [ ] Fake task inside fenced code\n",
            "```\n",
        ),
    )
    .unwrap();

    let state = state_for(&root);
    let outcome = open_or_create_daily_note(&state, "2026-11-26").unwrap();
    assert_eq!(
        outcome,
        DailyNoteOutcome {
            path: "daily/2026-11-26.md".to_string(),
            created: true,
        }
    );

    let text = std::fs::read_to_string(root.join("daily/2026-11-26.md")).unwrap();
    assert!(text.contains("# 2026-11-26"));
    assert!(text.contains("- [ ] Review attention math"));
    assert!(text.contains("- [ ] Benchmark cache hit rate"));
    assert!(!text.contains("Completed task"));
    assert!(!text.contains("Very old task"));
    assert!(!text.contains("Fake task inside fenced code"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn extracts_question_and_claim_callouts_across_subfolders_capped_at_five() {
    let root = temp_vault("synthesis");
    std::fs::create_dir_all(root.join("notes/ml")).unwrap();
    std::fs::create_dir_all(root.join("notes/zzz")).unwrap();
    std::fs::create_dir_all(root.join("daily")).unwrap();

    std::fs::write(
        root.join("daily/2026-11-25.md"),
        "> [!question] Ignored daily callout\n",
    )
    .unwrap();
    std::fs::write(
        root.join("notes/ml/Transformers.md"),
        concat!(
            "# Transformers\n\n",
            "> [!QUESTION] Why does attention scale quadratically?\n\n",
            "> [!claim]\n",
            "> FlashAttention avoids materializing the N x N matrix.\n\n",
            "```markdown\n",
            "> [!question] Fake callout inside code fence\n",
            "```\n",
        ),
    )
    .unwrap();
    std::fs::write(
        root.join("notes/zzz/Extra.md"),
        concat!(
            "> [!claim] One\n",
            "> [!question] Two\n",
            "> [!claim] Three\n",
            "> [!question] Four\n",
        ),
    )
    .unwrap();

    let state = state_for(&root);
    let outcome = open_or_create_daily_note(&state, "2026-11-26").unwrap();
    assert!(outcome.created);

    let text = std::fs::read_to_string(root.join("daily/2026-11-26.md")).unwrap();
    assert!(text.contains("## Synthesis Brief"));
    assert!(text.contains(
        "- **Question** in [[notes/ml/Transformers|Transformers]]: Why does attention scale quadratically?"
    ));
    assert!(text.contains(
        "- **Claim** in [[notes/ml/Transformers|Transformers]]: FlashAttention avoids materializing the N x N matrix."
    ));
    assert!(!text.contains("Ignored daily callout"));
    assert!(!text.contains("Fake callout inside code fence"));

    // Capped at 5 callout bullets.
    let brief_section = text.split("## Synthesis Brief\n").nth(1).unwrap();
    let bullet_count = brief_section
        .split("\n## Notes")
        .next()
        .unwrap()
        .lines()
        .filter(|l| l.starts_with("- "))
        .count();
    assert_eq!(bullet_count, 5);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn second_call_for_same_date_is_idempotent_and_preserves_existing_edits() {
    let root = temp_vault("idempotent");
    let state = state_for(&root);

    let first = open_or_create_daily_note(&state, "2026-11-26").unwrap();
    assert!(first.created);
    let custom = "# 2026-11-26\n\nMy custom daily writing.\n";
    crate::vault::save_file(&state, &first.path, custom).unwrap();

    let second = open_or_create_daily_note(&state, "2026-11-26").unwrap();
    assert_eq!(
        second,
        DailyNoteOutcome {
            path: "daily/2026-11-26.md".to_string(),
            created: false,
        }
    );
    assert_eq!(
        std::fs::read_to_string(root.join("daily/2026-11-26.md")).unwrap(),
        custom
    );

    assert!(open_or_create_daily_note(&state, "not-a-date").is_err());
    let _ = std::fs::remove_dir_all(&root);
}
