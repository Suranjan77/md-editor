//! Unit tests for fenced code block detection, output formatting, timeouts, and idempotent updates.

use super::*;

#[test]
fn detects_enclosing_fence_and_replaces_existing_output_idempotently() {
    let note = concat!(
        "# Title\n\n",
        "```sh\n",
        "echo hello\n",
        "```\n",
        "> [!output]\n",
        "> old\n\n",
        "After\n",
    );
    let fence = enclosing_fence(note, 3).expect("inside fence");
    assert_eq!(fence.lang, "sh");
    assert_eq!(fence.body, "echo hello");
    assert_eq!(fence.open_line, 2);
    assert_eq!(fence.close_line, 4);
    assert_eq!(fence.existing_output, Some((5, 7)));

    let formatted = format_output_callout(&RunOutput {
        stdout: "hello\n".into(),
        stderr: String::new(),
        status_code: Some(0),
        truncated: false,
    });
    let (start, end, replacement) = compute_output_edit(note, "\n", &fence, &formatted);
    let mut updated = note.to_string();
    let start_b = updated.char_indices().nth(start).unwrap().0;
    let end_b = updated.char_indices().nth(end).unwrap().0;
    updated.replace_range(start_b..end_b, &replacement);
    assert!(updated.contains("```\n> [!output]\n> hello\n\nAfter"));
    assert!(!updated.contains("> old"));

    // Outside any fence returns None.
    assert!(enclosing_fence(note, 0).is_none());
}

#[test]
fn inserts_new_output_callout_below_closing_fence() {
    let note = "```sh\necho one\n```\nNext line";
    let fence = enclosing_fence(note, 1).unwrap();
    assert_eq!(fence.existing_output, None);
    let (start, end, repl) = compute_output_edit(note, "\n", &fence, "> [!output]\n> one");
    assert_eq!(start, end);
    let mut updated = note.to_string();
    let pos = updated.char_indices().nth(start).unwrap().0;
    updated.insert_str(pos, &repl);
    assert_eq!(
        updated,
        "```sh\necho one\n```\n> [!output]\n> one\nNext line"
    );
}

#[test]
fn runs_shell_captures_stdout_and_nonzero_status() {
    let ok = run_fence_command("sh", "printf 'hi\\n'", None).unwrap();
    assert_eq!(ok.status_code, Some(0));
    assert_eq!(ok.stdout, "hi\n");

    let fail = run_fence_command("sh", "echo oops >&2; exit 3", None).unwrap();
    assert_eq!(fail.status_code, Some(3));
    let formatted = format_output_callout(&fail);
    assert!(formatted.starts_with("> [!output] Exit 3\n"));
    assert!(formatted.contains("> [stderr]\n> oops"));

    assert!(run_fence_command("rust", "fn main() {}", None).is_err());
}

#[test]
fn enforces_five_second_timeout_and_kills_child() {
    let started = Instant::now();
    let err = run_fence_command("sh", "exec sleep 20", None).unwrap_err();
    assert!(err.contains("Timed out"), "{err}");
    assert!(started.elapsed() < Duration::from_secs(7));
}

/// A timeout ends what the block started too, not just the shell.
#[cfg(target_os = "linux")]
#[test]
fn timeout_kills_background_children() {
    let dir = std::env::temp_dir().join(format!("md-runner-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    // The background job would write its file 6 s in, 1 s after the timeout.
    let err = run_fence_command(
        "sh",
        "(sleep 6; touch late) & sleep 20",
        Some(dir.as_path()),
    )
    .unwrap_err();
    assert!(err.contains("Timed out"), "{err}");
    std::thread::sleep(Duration::from_secs(2));
    assert!(
        !dir.join("late").exists(),
        "a background child outlived the timeout"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
