//! File logger behind the `log` facade: `md-editor.log` in `~/.local/state/md-editor/`, rotated at 1 MB × 3, levels from `MD_EDITOR_LOG`, oversized lines truncated and repeats collapsed, plus a panic hook that logs a backtrace.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use log::{Level, LevelFilter, Log, Metadata, Record};

/// Environment variable naming the levels: a comma-separated list of a bare
/// level for the app's own crates (`error`, `warn`, `info` (default), `debug`,
/// `trace` or `off`) and `target=level` overrides, e.g. `debug,iced_winit=info`.
pub const LOG_LEVEL_ENV: &str = "MD_EDITOR_LOG";

/// Level for crates other than our own unless a directive says otherwise.
///
/// Dependencies log freely at INFO — iced_winit dumps each window's attributes,
/// icon pixels included, which once made every launch write ~85 MB.
const DEPENDENCY_LEVEL: LevelFilter = LevelFilter::Warn;

/// Longest line written; anything longer is cut with a marker. Keeps one
/// runaway `Debug` dump from defeating the size-based rotation.
const MAX_LINE_BYTES: usize = 8 * 1024;

/// A run of identical messages is summarised at least this often.
const REPEAT_SUMMARY_EVERY: std::time::Duration = std::time::Duration::from_secs(1);

const LOG_FILE_NAME: &str = "md-editor.log";
/// Rotate once the live file passes this size.
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// Rotated files kept beside the live one: `md-editor.log.1` … `.N`.
const ROTATED_FILES: u32 = 3;

struct FileLogger {
    filter: Filter,
    sink: Mutex<Sink>,
}

/// Which records pass: one level for our crates, one for dependencies, and
/// per-target overrides matched by module-path prefix.
#[derive(Debug, Clone, PartialEq)]
struct Filter {
    app: LevelFilter,
    dependencies: LevelFilter,
    overrides: Vec<(String, LevelFilter)>,
}

impl Filter {
    fn level_for(&self, target: &str) -> LevelFilter {
        let matches = |prefix: &str| {
            target == prefix
                || target
                    .strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with("::"))
        };
        // Longest matching prefix wins, so `iced=error,iced_winit=info` works
        // either way round.
        if let Some((_, level)) = self
            .overrides
            .iter()
            .filter(|(prefix, _)| matches(prefix))
            .max_by_key(|(prefix, _)| prefix.len())
        {
            return *level;
        }
        if target.starts_with("md_editor") {
            self.app
        } else {
            self.dependencies
        }
    }

    /// The most verbose level any record could pass at, for `log::set_max_level`.
    fn max_level(&self) -> LevelFilter {
        self.overrides
            .iter()
            .map(|(_, level)| *level)
            .chain([self.app, self.dependencies])
            .max()
            .unwrap_or(LevelFilter::Info)
    }
}

/// Parse `MD_EDITOR_LOG`. Unknown pieces are ignored rather than fatal: a typo
/// must not silence the log.
fn parse_filter(value: Option<&str>) -> Filter {
    let mut filter = Filter {
        app: LevelFilter::Info,
        dependencies: DEPENDENCY_LEVEL,
        overrides: Vec::new(),
    };
    for part in value.unwrap_or_default().split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        match part.split_once('=') {
            Some((target, level)) => {
                if let Ok(level) = level.trim().parse() {
                    filter.overrides.push((target.trim().to_string(), level));
                }
            }
            None => {
                if let Ok(level) = part.parse() {
                    filter.app = level;
                }
            }
        }
    }
    filter
}

/// Cut `line` to at most [`MAX_LINE_BYTES`] on a char boundary, keeping the
/// trailing newline.
fn truncate_line(line: String) -> String {
    if line.len() <= MAX_LINE_BYTES {
        return line;
    }
    let mut cut = MAX_LINE_BYTES;
    while !line.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{} …[truncated {} bytes]\n", &line[..cut], line.len() - cut)
}

struct Sink {
    path: PathBuf,
    file: Option<File>,
    written: u64,
    repeat: Repeat,
}

/// The last message written and how many identical ones were swallowed since.
#[derive(Default)]
struct Repeat {
    key: String,
    count: u32,
    since: Option<std::time::Instant>,
}

impl Sink {
    fn open(path: PathBuf) -> Self {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok();
        let written = file
            .as_ref()
            .and_then(|f| f.metadata().ok())
            .map_or(0, |m| m.len());
        Self {
            path,
            file,
            written,
            repeat: Repeat::default(),
        }
    }

    /// Write a record's line unless it repeats the previous message, in which
    /// case count it and summarise the run later. Returns whether the line was
    /// written (so the caller mirrors only those to stderr).
    fn write_record(&mut self, key: &str, line: &str) -> bool {
        if key == self.repeat.key {
            self.repeat.count += 1;
            let since = *self
                .repeat
                .since
                .get_or_insert_with(std::time::Instant::now);
            if since.elapsed() >= REPEAT_SUMMARY_EVERY {
                self.flush_repeats();
            }
            return false;
        }
        self.flush_repeats();
        self.repeat.key = key.to_string();
        self.write_line(line);
        true
    }

    /// Write the "repeated N times" line for a run of swallowed messages.
    fn flush_repeats(&mut self) {
        if self.repeat.count > 0 {
            let summary = format!(
                "{} (previous message repeated {} more times)\n",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                self.repeat.count
            );
            self.write_line(&summary);
        }
        self.repeat.count = 0;
        self.repeat.since = None;
    }

    fn write_line(&mut self, line: &str) {
        if self.written + line.len() as u64 > MAX_FILE_BYTES {
            self.rotate();
        }
        if let Some(file) = self.file.as_mut()
            && file.write_all(line.as_bytes()).is_ok()
        {
            self.written += line.len() as u64;
        }
    }

    fn rotate(&mut self) {
        self.file = None;
        rotate_files(&self.path, ROTATED_FILES);
        let repeat = std::mem::take(&mut self.repeat);
        *self = Self::open(std::mem::take(&mut self.path));
        self.repeat = repeat;
    }
}

/// Shift `path.N-1 → path.N … path → path.1`, dropping the oldest.
fn rotate_files(path: &Path, keep: u32) {
    let numbered = |n: u32| {
        let mut name = path.as_os_str().to_os_string();
        name.push(format!(".{n}"));
        PathBuf::from(name)
    };
    let _ = std::fs::remove_file(numbered(keep));
    for n in (1..keep).rev() {
        let _ = std::fs::rename(numbered(n), numbered(n + 1));
    }
    let _ = std::fs::rename(path, numbered(1));
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.filter.level_for(metadata.target())
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = truncate_line(format_line(record));
        // The timestamp is the first 23 bytes; the rest identifies a repeat.
        let key = line.get(24..).unwrap_or(&line);
        let written = self
            .sink
            .lock()
            .map(|mut sink| sink.write_record(key, &line))
            .unwrap_or(true);
        // Warnings and errors also reach a terminal, as the old `eprintln!`s did.
        if written && record.level() <= Level::Warn {
            let _ = std::io::stderr().write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Ok(mut sink) = self.sink.lock() {
            sink.flush_repeats();
            if let Some(file) = sink.file.as_mut() {
                let _ = file.flush();
            }
        }
    }
}

fn format_line(record: &Record) -> String {
    format!(
        "{} {:<5} {}: {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        record.level(),
        record.target(),
        record.args()
    )
}

/// Install the file logger and the panic hook. Call once, first thing in
/// `main`. Logging is best effort: if the file cannot be opened, warnings and
/// errors still reach stderr.
pub fn init() {
    let filter = parse_filter(std::env::var(LOG_LEVEL_ENV).ok().as_deref());
    let max_level = filter.max_level();
    let dir = md_editor_core::state::log_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(LOG_FILE_NAME);
    let logger = FileLogger {
        filter,
        sink: Mutex::new(Sink::open(path)),
    };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(max_level);
    }
    install_panic_hook();
    log::info!(
        "MD Editor {} starting ({} {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
}

/// Log every panic with a backtrace, then run the default hook so the
/// terminal output is unchanged.
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        log::error!("panic: {info}\n{backtrace}");
        log::logger().flush();
        default_hook(info);
    }));
}

#[cfg(test)]
mod tests;
