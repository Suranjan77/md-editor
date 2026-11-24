use super::*;

#[test]
fn level_defaults_to_info_and_parses_names() {
    assert_eq!(parse_level(None), LevelFilter::Info);
    assert_eq!(parse_level(Some("nonsense")), LevelFilter::Info);
    assert_eq!(parse_level(Some("debug")), LevelFilter::Debug);
    assert_eq!(parse_level(Some(" WARN ")), LevelFilter::Warn);
    assert_eq!(parse_level(Some("off")), LevelFilter::Off);
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
