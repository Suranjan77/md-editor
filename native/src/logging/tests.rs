use super::*;

#[test]
fn level_defaults_to_info_and_parses_names() {
    let app = |value| parse_filter(value).app;
    assert_eq!(app(None), LevelFilter::Info);
    assert_eq!(app(Some("nonsense")), LevelFilter::Info);
    assert_eq!(app(Some("debug")), LevelFilter::Debug);
    assert_eq!(app(Some(" WARN ")), LevelFilter::Warn);
    assert_eq!(app(Some("off")), LevelFilter::Off);
}

#[test]
fn dependencies_default_to_warn_and_directives_override_by_prefix() {
    let filter = parse_filter(None);
    assert_eq!(filter.level_for("md_editor::app::ink"), LevelFilter::Info);
    assert_eq!(filter.level_for("md_editor_core::pdf"), LevelFilter::Info);
    assert_eq!(filter.level_for("iced_winit"), LevelFilter::Warn);
    assert_eq!(filter.level_for("wgpu_core::device"), LevelFilter::Warn);

    let filter = parse_filter(Some("debug, iced=error, iced_winit=info, bogus=loud"));
    assert_eq!(filter.app, LevelFilter::Debug);
    assert_eq!(filter.level_for("iced_winit"), LevelFilter::Info);
    assert_eq!(filter.level_for("iced_winit::program"), LevelFilter::Info);
    assert_eq!(filter.level_for("iced"), LevelFilter::Error);
    assert_eq!(filter.level_for("iced::widget"), LevelFilter::Error);
    // A prefix only matches whole path segments.
    assert_eq!(filter.level_for("iced_futures"), LevelFilter::Warn);
    assert_eq!(filter.max_level(), LevelFilter::Debug);
}

#[test]
fn oversized_lines_are_truncated_on_a_char_boundary() {
    let short = "short\n".to_string();
    assert_eq!(truncate_line(short.clone()), short);

    // Multi-byte chars straddling the cut must not panic or split.
    let long = "é".repeat(MAX_LINE_BYTES) + "\n";
    let cut = truncate_line(long);
    assert!(cut.len() < MAX_LINE_BYTES + 64, "{} bytes", cut.len());
    assert!(cut.contains("…[truncated"));
    assert!(cut.ends_with('\n'));
}

#[test]
fn a_huge_record_cannot_blow_the_file_budget() {
    let dir = std::env::temp_dir().join(format!("md-editor-log-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(LOG_FILE_NAME);

    // What iced_winit's window-attribute dump looked like: one 10 MB line.
    let huge = truncate_line("0, ".repeat(10 * 1024 * 1024 / 3) + "\n");
    let mut sink = Sink::open(path.clone());
    sink.write_record(&huge, &huge);
    drop(sink);

    let live = std::fs::metadata(&path).unwrap().len();
    assert!(live <= MAX_FILE_BYTES, "live file is {live} bytes");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn identical_messages_collapse_into_a_count() {
    let dir = std::env::temp_dir().join(format!("md-editor-log-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(LOG_FILE_NAME);

    let mut sink = Sink::open(path.clone());
    assert!(sink.write_record("WARN x: full", "t1 WARN x: full\n"));
    for _ in 0..99 {
        assert!(!sink.write_record("WARN x: full", "t2 WARN x: full\n"));
    }
    assert!(sink.write_record("INFO y: other", "t3 INFO y: other\n"));
    drop(sink);

    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.matches("WARN x: full").count(), 1, "{text}");
    assert!(text.contains("repeated 99 more times"), "{text}");
    assert!(text.ends_with("t3 INFO y: other\n"), "{text}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn sink_rotates_past_the_size_limit_and_keeps_three_files() {
    let dir = std::env::temp_dir().join(format!("md-editor-log-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(LOG_FILE_NAME);

    let line = "x".repeat(1023) + "\n";
    let mut sink = Sink::open(path.clone());
    // Five files' worth of lines: the live file plus three rotations survive.
    for _ in 0..(5 * 1024) {
        sink.write_line(&line);
    }
    drop(sink);

    let exists = |suffix: &str| dir.join(format!("{LOG_FILE_NAME}{suffix}")).exists();
    assert!(exists(""));
    assert!(exists(".1"));
    assert!(exists(".2"));
    assert!(exists(".3"));
    assert!(!exists(".4"));
    let live = std::fs::metadata(&path).unwrap().len();
    assert!(live <= MAX_FILE_BYTES, "live file is {live} bytes");

    std::fs::remove_dir_all(&dir).unwrap();
}
