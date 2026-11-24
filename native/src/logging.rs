//! File logger behind the `log` facade: `md-editor.log` in `~/.local/state/md-editor/`, rotated at 1 MB × 3, level from `MD_EDITOR_LOG`, plus a panic hook that logs a backtrace.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use log::{Level, LevelFilter, Log, Metadata, Record};

/// Environment variable naming the maximum level: `error`, `warn`, `info`
/// (default), `debug`, `trace` or `off`.
pub const LOG_LEVEL_ENV: &str = "MD_EDITOR_LOG";

const LOG_FILE_NAME: &str = "md-editor.log";
/// Rotate once the live file passes this size.
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// Rotated files kept beside the live one: `md-editor.log.1` … `.N`.
const ROTATED_FILES: u32 = 3;

struct FileLogger {
    level: LevelFilter,
    sink: Mutex<Sink>,
}

struct Sink {
    path: PathBuf,
    file: Option<File>,
    written: u64,
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
        }
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
        *self = Self::open(std::mem::take(&mut self.path));
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
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format_line(record);
        // Warnings and errors also reach a terminal, as the old `eprintln!`s did.
        if record.level() <= Level::Warn {
            let _ = std::io::stderr().write_all(line.as_bytes());
        }
        if let Ok(mut sink) = self.sink.lock() {
            sink.write_line(&line);
        }
    }

    fn flush(&self) {
        if let Ok(mut sink) = self.sink.lock()
            && let Some(file) = sink.file.as_mut()
        {
            let _ = file.flush();
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

fn parse_level(value: Option<&str>) -> LevelFilter {
    value
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(LevelFilter::Info)
}

/// Install the file logger and the panic hook. Call once, first thing in
/// `main`. Logging is best effort: if the file cannot be opened, warnings and
/// errors still reach stderr.
pub fn init() {
    let level = parse_level(std::env::var(LOG_LEVEL_ENV).ok().as_deref());
    let dir = md_editor_core::state::log_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(LOG_FILE_NAME);
    let logger = FileLogger {
        level,
        sink: Mutex::new(Sink::open(path)),
    };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(level);
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
