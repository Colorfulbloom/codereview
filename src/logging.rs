//! Persistent file logging.
//!
//! Everything user-visible that goes wrong (and the key review-run events)
//! is appended to `.codereview/logs/code-review.log` in the project root, so
//! a failure can be diagnosed after the terminal scrolls away. Self-contained
//! on purpose: no logging-crate dependency, no global subscriber magic —
//! a `OnceLock<FileLogger>` and three functions.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Rotate when the log from previous runs exceeds this (one `.1` generation
/// is kept).
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
}

impl LogLevel {
    /// Fixed-width tag so log lines align.
    fn tag(self) -> &'static str {
        match self {
            LogLevel::Error => "ERROR",
            LogLevel::Warn => "WARN ",
            LogLevel::Info => "INFO ",
        }
    }
}

/// Append-only, timestamped log file behind a mutex.
pub struct FileLogger {
    file: Mutex<File>,
}

impl FileLogger {
    /// Open (append) the log at `path`, creating parent directories and
    /// rotating an oversized predecessor to `<path>.1`.
    pub fn open(path: &Path) -> std::io::Result<Self> {
        Self::open_with_limit(path, MAX_LOG_BYTES)
    }

    fn open_with_limit(path: &Path, max_bytes: u64) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if let Ok(meta) = std::fs::metadata(path)
            && meta.len() > max_bytes
        {
            let mut rotated = path.as_os_str().to_owned();
            rotated.push(".1");
            // Rotation is best-effort — a failure must never block logging.
            let _ = std::fs::rename(path, PathBuf::from(rotated));
        }

        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }

    /// Write one timestamped line. Failures are swallowed: logging must never
    /// take the app down.
    pub fn write(&self, level: LogLevel, message: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = format_line(&format_timestamp(now), level, message);
        if let Ok(mut file) = self.file.lock() {
            let _ = file.write_all(line.as_bytes());
        }
    }
}

static GLOBAL: OnceLock<FileLogger> = OnceLock::new();
static GLOBAL_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Install the global logger at `path`. Idempotent: the first successful call
/// wins; later calls return the original path. Returns the path in use.
pub fn init(path: &Path) -> std::io::Result<PathBuf> {
    if let Some(existing) = GLOBAL_PATH.get() {
        return Ok(existing.clone());
    }
    let logger = FileLogger::open(path)?;
    let _ = GLOBAL.set(logger);
    let _ = GLOBAL_PATH.set(path.to_path_buf());
    Ok(GLOBAL_PATH.get().expect("just set").clone())
}

/// The active log file path, if a logger has been installed.
pub fn path() -> Option<&'static Path> {
    GLOBAL_PATH.get().map(PathBuf::as_path)
}

/// Log an error. No-op until [`init`] has been called.
pub fn error(message: impl AsRef<str>) {
    write_global(LogLevel::Error, message.as_ref());
}

/// Log a warning. No-op until [`init`] has been called.
pub fn warn(message: impl AsRef<str>) {
    write_global(LogLevel::Warn, message.as_ref());
}

/// Log an informational event. No-op until [`init`] has been called.
pub fn info(message: impl AsRef<str>) {
    write_global(LogLevel::Info, message.as_ref());
}

fn write_global(level: LogLevel, message: &str) {
    if let Some(logger) = GLOBAL.get() {
        logger.write(level, message);
    }
}

fn format_line(timestamp: &str, level: LogLevel, message: &str) -> String {
    format!("{timestamp} [{}] {message}\n", level.tag())
}

/// UTC timestamp (`YYYY-MM-DD HH:MM:SSZ`) from seconds since the Unix epoch.
fn format_timestamp(unix_secs: u64) -> String {
    let days = (unix_secs / 86_400) as i64;
    let rem = unix_secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02}Z")
}

/// Days-since-epoch → (year, month, day), Howard Hinnant's civil-date
/// algorithm. Exact for the full Gregorian calendar.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if mo <= 2 { y + 1 } else { y }, mo, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn timestamps_are_utc_and_human_readable() {
        assert_eq!(format_timestamp(0), "1970-01-01 00:00:00Z");
        assert_eq!(format_timestamp(86_399), "1970-01-01 23:59:59Z");
        // 2026-01-01 00:00:00 UTC
        assert_eq!(format_timestamp(1_767_225_600), "2026-01-01 00:00:00Z");
    }

    #[test]
    fn log_lines_carry_timestamp_level_and_message() {
        let line = format_line("2026-01-01 00:00:00Z", LogLevel::Warn, "config invalid");
        assert_eq!(line, "2026-01-01 00:00:00Z [WARN ] config invalid\n");
        let line = format_line("2026-01-01 00:00:00Z", LogLevel::Error, "boom");
        assert!(line.contains("[ERROR]"));
    }

    #[test]
    fn logger_appends_lines_and_creates_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("logs/nested/code-review.log");

        let logger = FileLogger::open(&path).unwrap();
        logger.write(LogLevel::Info, "first");
        logger.write(LogLevel::Error, "second");

        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("[INFO ] first"));
        assert!(lines[1].ends_with("[ERROR] second"));
    }

    #[test]
    fn oversized_log_is_rotated_on_open() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("code-review.log");
        fs::write(&path, "x".repeat(64)).unwrap();

        // Tiny limit forces rotation of the existing file.
        let logger = FileLogger::open_with_limit(&path, 16).unwrap();
        logger.write(LogLevel::Info, "fresh");

        let rotated = fs::read_to_string(dir.path().join("code-review.log.1")).unwrap();
        assert_eq!(rotated.len(), 64);
        let current = fs::read_to_string(&path).unwrap();
        assert!(current.contains("fresh"));
        assert!(!current.contains("xxxx"));
    }

    #[test]
    fn small_log_is_not_rotated() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("code-review.log");
        fs::write(&path, "previous run\n").unwrap();

        let logger = FileLogger::open_with_limit(&path, 1024).unwrap();
        logger.write(LogLevel::Info, "next run");

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("previous run"));
        assert!(content.contains("next run"));
        assert!(!dir.path().join("code-review.log.1").exists());
    }

    #[test]
    fn global_functions_are_noops_before_init_then_write_after() {
        // Must not panic or create files when uninitialized.
        warn("ignored — no logger installed yet");

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("global.log");
        let installed = init(&path).unwrap();
        assert_eq!(installed, path);

        error("global error line");
        warn("global warn line");
        info("global info line");

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("[ERROR] global error line"));
        assert!(content.contains("[WARN ] global warn line"));
        assert!(content.contains("[INFO ] global info line"));

        // Second init is idempotent: keeps the first sink, doesn't error.
        let again = init(&dir.path().join("other.log")).unwrap();
        assert_eq!(again, path);
    }
}
