//! Default-on native file log (`$XDG_DATA_HOME/colibri/logs/native.log`).
//!
//! Dual-writes stderr + a small rotating file. `COLIBRI_LOG=off` / `0` skips
//! init. Do not log prompts, generate tokens, HF tokens, or API keys.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use colibri_sys::{
    DEFAULT_NATIVE_LOG_FILTER, default_native_log_path, format_session_heartbeat_line,
    native_log_enabled, native_log_filter_from, sanitize_log_text, session_identity_now,
};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, fmt};

/// Rotate when the active file exceeds this size (4 MiB).
pub const NATIVE_LOG_ROTATE_BYTES: u64 = 4 * 1024 * 1024;
/// Keep this many rotated backups (`native.log.1` …).
pub const NATIVE_LOG_BACKUP_COUNT: u32 = 2;
/// RSS / identity sample while an engine session is up (5–10s).
pub const SESSION_HEARTBEAT_MS: u64 = 8_000;

/// One short heartbeat line (pid, comm, cgroup, flavor, RSS, swap). No prompts.
pub fn session_heartbeat_line(kind: Option<&str>) -> String {
    format_session_heartbeat_line(kind, &session_identity_now())
}

/// Append a heartbeat line to `path`. Returns the line written.
pub fn write_session_heartbeat_to(path: &Path, kind: Option<&str>) -> io::Result<String> {
    let line = session_heartbeat_line(kind);
    append_native_log_line(path, &line)?;
    Ok(line)
}

/// Emit one heartbeat through tracing (file + stderr when logging is on).
pub fn log_session_heartbeat(kind: Option<&str>) {
    let line = session_heartbeat_line(kind);
    tracing::info!(target: "colibri_native", "{line}");
}

/// Continue the 8s identity pump only while an engine session occupies the slot.
pub fn session_heartbeat_pump_should_continue(engine_slot_occupied: bool) -> bool {
    engine_slot_occupied
}

/// Size-rotating append file. Rotates before a write that would exceed `max_bytes`.
pub struct RotatingFile {
    path: PathBuf,
    file: File,
    len: u64,
    max_bytes: u64,
    backups: u32,
}

impl RotatingFile {
    /// Open (or create) `path` for append. Creates parent directories.
    pub fn open(path: impl AsRef<Path>, max_bytes: u64, backups: u32) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let len = file.metadata()?.len();
        Ok(Self {
            path,
            file,
            len,
            max_bytes,
            backups,
        })
    }

    /// Shift `path` → `path.1` → `path.2`, dropping the oldest.
    pub fn rotate_files(path: &Path, backups: u32) -> io::Result<()> {
        if backups == 0 {
            let _ = fs::remove_file(path);
            return Ok(());
        }
        let oldest = rotated_log_path(path, backups);
        let _ = fs::remove_file(&oldest);
        for i in (1..backups).rev() {
            let from = rotated_log_path(path, i);
            let to = rotated_log_path(path, i + 1);
            if from.exists() {
                fs::rename(&from, &to)?;
            }
        }
        if path.exists() {
            fs::rename(path, rotated_log_path(path, 1))?;
        }
        Ok(())
    }

    fn maybe_rotate(&mut self) -> io::Result<()> {
        if self.max_bytes == 0 || self.len < self.max_bytes {
            return Ok(());
        }
        self.file.flush()?;
        Self::rotate_files(&self.path, self.backups)?;
        self.file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)?;
        self.len = 0;
        Ok(())
    }
}

fn rotated_log_path(path: &Path, n: u32) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".{n}"));
    PathBuf::from(name)
}

impl Write for RotatingFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let raw = String::from_utf8_lossy(buf);
        let clean = sanitize_log_text(&raw);
        let bytes = clean.as_bytes();
        self.maybe_rotate()?;
        let n = self.file.write(bytes)?;
        self.len = self.len.saturating_add(n as u64);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// Append one sanitized line to `path` (panic-hook backstop).
pub fn append_native_log_line(path: &Path, line: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    let clean = sanitize_log_text(line);
    writeln!(f, "{clean}")?;
    f.flush()
}

/// Shared writer for tracing-subscriber.
#[derive(Clone)]
pub struct SharedRotating(pub Arc<Mutex<RotatingFile>>);

impl Write for SharedRotating {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("native log lock poisoned"))?
            .write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("native log lock poisoned"))?
            .flush()
    }
}

impl<'a> MakeWriter<'a> for SharedRotating {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// What [`init_native_logging`] decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeLogInit {
    /// File logging skipped (`COLIBRI_LOG=off` / `0`).
    Disabled,
    /// Subscriber installed (or already installed) for `path`.
    Enabled { path: PathBuf },
}

/// Install tracing to `default_native_log_path` + stderr. Safe to call once.
pub fn init_native_logging() -> NativeLogInit {
    init_native_logging_with(
        native_log_enabled(),
        std::env::var("RUST_LOG").ok().as_deref(),
        default_native_log_path(),
    )
}

/// Testable init: `enabled` is the parsed `COLIBRI_LOG` decision.
pub fn init_native_logging_with(
    enabled: bool,
    rust_log: Option<&str>,
    path: PathBuf,
) -> NativeLogInit {
    if !enabled {
        return NativeLogInit::Disabled;
    }
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let filter = EnvFilter::try_new(native_log_filter_from(rust_log))
        .unwrap_or_else(|_| EnvFilter::new(DEFAULT_NATIVE_LOG_FILTER));
    let stderr_layer = fmt::layer()
        .with_writer(io::stderr)
        .with_ansi(false)
        .with_target(true);

    let file = match RotatingFile::open(&path, NATIVE_LOG_ROTATE_BYTES, NATIVE_LOG_BACKUP_COUNT) {
        Ok(f) => f,
        Err(_) => {
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(stderr_layer)
                .try_init();
            install_panic_hook(Some(path.clone()));
            let _ = write_session_heartbeat_to(&path, None);
            log_session_heartbeat(None);
            return NativeLogInit::Enabled { path };
        }
    };
    let shared = SharedRotating(Arc::new(Mutex::new(file)));
    let file_layer = fmt::layer()
        .with_writer(shared)
        .with_ansi(false)
        .with_target(true);
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .try_init();
    install_panic_hook(Some(path.clone()));
    log_session_heartbeat(None);
    NativeLogInit::Enabled { path }
}

fn install_panic_hook(log_path: Option<PathBuf>) {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = format!("panic: {info}");
        tracing::error!(target: "colibri_native", "{msg}");
        if let Some(ref path) = log_path {
            let _ = append_native_log_line(path, &msg);
        }
        prev(info);
    }));
}

/// Desktop app id for Mutter / WM_CLASS (not the window title).
pub fn native_app_id() -> &'static str {
    "org.colibri.native"
}

#[cfg(test)]
mod tests {
    use super::*;

    static PANIC_HOOK_TEST: Mutex<()> = Mutex::new(());

    #[test]
    fn panic_hook_writes_panic_line_to_log_file() {
        // take_hook / set_hook are process-global.
        let _serial = PANIC_HOOK_TEST.lock().unwrap_or_else(|e| e.into_inner());
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("native.log");

        let prev = std::panic::take_hook();
        install_panic_hook(Some(path.clone()));
        let panicked = std::panic::catch_unwind(|| {
            panic!("colibri-native panic-hook contract");
        });
        std::panic::set_hook(prev);

        assert!(panicked.is_err(), "catch_unwind must observe the panic");
        let body = fs::read_to_string(&path).unwrap_or_default();
        assert!(
            body.contains("panic:"),
            "after a Rust panic the log file must contain a panic: line, got: {body:?}"
        );
    }

    #[test]
    fn rotating_file_writes_and_redacts_secrets() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("native.log");
        {
            let mut w = RotatingFile::open(&path, NATIVE_LOG_ROTATE_BYTES, NATIVE_LOG_BACKUP_COUNT)
                .expect("open");
            writeln!(w, "start model=/tmp/glm hf_AbCdEfGhIjKlMnOpQrStUvWx").unwrap();
            w.flush().unwrap();
        }
        let body = fs::read_to_string(&path).expect("read");
        assert!(body.contains("model=/tmp/glm"), "{body}");
        assert!(
            !body.contains("hf_AbCdEfGhIjKlMnOpQrStUvWx"),
            "token must not hit the file: {body}"
        );
    }

    #[test]
    fn rotating_file_rotates_when_over_max() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("native.log");
        fs::write(&path, b"aaaa").unwrap();
        RotatingFile::rotate_files(&path, 2).expect("rotate");
        assert!(
            path.with_extension("log.1").is_file() || root.path().join("native.log.1").is_file(),
            "expected native.log.1 after rotate; dir={:?}",
            fs::read_dir(root.path())
                .unwrap()
                .filter_map(|e| e.ok().map(|e| e.file_name()))
                .collect::<Vec<_>>()
        );
        assert!(
            !path.exists() || fs::metadata(&path).map(|m| m.len()).unwrap_or(0) == 0,
            "active file should be gone or empty after rotate"
        );
    }

    #[test]
    fn append_native_log_line_creates_parent_and_writes() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("logs").join("native.log");
        append_native_log_line(&path, "engine start begin").unwrap();
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("engine start begin"), "{body}");
    }

    #[test]
    fn native_app_id_is_org_colibri_native() {
        assert_eq!(native_app_id(), "org.colibri.native");
    }

    #[test]
    fn session_heartbeat_pump_skips_when_slot_empty() {
        assert!(!session_heartbeat_pump_should_continue(false));
    }

    #[test]
    fn session_heartbeat_pump_stops_when_session_drops() {
        assert!(session_heartbeat_pump_should_continue(true));
        assert!(
            !session_heartbeat_pump_should_continue(false),
            "clearing the session slot must stop the pump"
        );
    }

    #[test]
    fn session_heartbeat_interval_is_five_to_ten_seconds() {
        assert!(
            (5_000..=10_000).contains(&SESSION_HEARTBEAT_MS),
            "heartbeat must sample every 5-10s, got {SESSION_HEARTBEAT_MS}"
        );
    }

    #[test]
    fn session_heartbeat_line_has_pid_flavor_no_prompt() {
        let line = session_heartbeat_line(None);
        let pid = std::process::id();
        assert!(line.contains("heartbeat"), "{line}");
        assert!(
            line.contains(&format!("pid={pid}")),
            "init heartbeat must include pid: {line}"
        );
        assert!(
            line.contains("flavor=cpu")
                || line.contains("flavor=HIP")
                || line.contains("flavor=CUDA"),
            "init heartbeat must include link flavor: {line}"
        );
        assert!(line.contains("rss_kb="), "{line}");
        assert!(line.contains("vmswap_kb="), "{line}");
        let lower = line.to_ascii_lowercase();
        assert!(!lower.contains("prompt="), "{line}");
    }

    #[test]
    fn write_session_heartbeat_to_file_is_one_short_line() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("native.log");
        let line = write_session_heartbeat_to(&path, Some("ffi")).expect("write heartbeat");
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("heartbeat"), "{body}");
        assert!(body.contains("kind=ffi"), "{body}");
        assert!(
            body.contains(&format!("pid={}", std::process::id())),
            "{body}"
        );
        assert_eq!(body.lines().count(), 1, "one short line: {body:?}");
        assert!(!line.to_ascii_lowercase().contains("prompt="), "{line}");
        assert!(!body.contains('\t'), "no prompt dump: {body:?}");
    }
}
