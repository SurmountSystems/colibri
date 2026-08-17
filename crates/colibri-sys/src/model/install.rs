//! Model download / convert orchestration (feature `install`).
//!
//! Prefer pure-Rust / CLI download (`hf` or `hf-hub`). Quant convert that needs
//! torch still shells out to existing Python tools under `c/tools/` as a last
//! resort — that path is documented, not reimplemented.
//!
//! Multi-shard snapshot path: recursive tree list (hf-hub 1.x `list_tree`) or
//! pass `--include` patterns to `hf download`, materialize into `dest`, detect
//! incomplete transfers, then optional `ModelInfo::inspect` + registry register.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::model::{ModelInfo, ModelRegistry};
use crate::probe::{GB, disk_free_bytes};

/// Error message when an install is cancelled mid-flight.
pub const INSTALL_CANCELLED_MSG: &str = "install cancelled";

/// Error message when an install is paused mid-flight (cooperative, not a failure).
///
/// Same between-file stop as cancel on the hub path; callers should treat this as
/// a recoverable stop (resume by re-running install; completed files are skipped).
pub const INSTALL_PAUSED_MSG: &str = "install paused";

/// Why a cooperative install stop was requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallStopKind {
    /// Abort; UI treats as cancelled error.
    Cancel,
    /// Graceful pause after the current file; UI offers Resume.
    Pause,
}

/// Cooperative stop handle for install jobs (CLI kill + hub loop checks).
///
/// [`request`] and [`request_pause`] both stop the job between files (hub) or
/// kill the CLI child. The stop kind is reported via [`check_cancel`] message:
/// [`INSTALL_CANCELLED_MSG`] vs [`INSTALL_PAUSED_MSG`].
#[derive(Debug, Clone, Default)]
pub struct InstallCancel {
    flag: Arc<AtomicBool>,
    /// 0 = unset, 1 = cancel, 2 = pause (meaningful only when `flag` is true).
    kind: Arc<std::sync::atomic::AtomicU8>,
}

impl InstallCancel {
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            kind: Arc::new(std::sync::atomic::AtomicU8::new(0)),
        }
    }

    /// Request cancel; CLI child is killed and hub path stops between files.
    pub fn request(&self) {
        self.kind.store(1, Ordering::SeqCst);
        self.flag.store(true, Ordering::SeqCst);
    }

    /// Request pause; same cooperative stop as cancel, but returns paused message.
    pub fn request_pause(&self) {
        self.kind.store(2, Ordering::SeqCst);
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_requested(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Stop kind when a stop was requested; `None` if not requested.
    pub fn stop_kind(&self) -> Option<InstallStopKind> {
        if !self.is_requested() {
            return None;
        }
        match self.kind.load(Ordering::SeqCst) {
            2 => Some(InstallStopKind::Pause),
            _ => Some(InstallStopKind::Cancel),
        }
    }

    pub fn clear(&self) {
        self.flag.store(false, Ordering::SeqCst);
        self.kind.store(0, Ordering::SeqCst);
    }
}

fn check_cancel(cancel: Option<&InstallCancel>) -> Result<()> {
    match cancel.and_then(|c| c.stop_kind()) {
        Some(InstallStopKind::Pause) => Err(Error::Install(INSTALL_PAUSED_MSG.into())),
        Some(InstallStopKind::Cancel) => Err(Error::Install(INSTALL_CANCELLED_MSG.into())),
        None => Ok(()),
    }
}

/// Whether a local file under `dest` is complete enough to skip re-download.
///
/// **Heuristic (honest):**
/// - If `expected_size > 0` (HF tree metadata): skip only when the file exists
///   and `metadata.len() == expected_size`.
/// - If `expected_size == 0` (size unknown): skip when the file exists and is
///   **non-empty**. Zero-length files are never treated as complete.
///
/// Does **not** verify content hashes. A truncated or corrupted file that
/// happens to have the expected size would still be skipped.
pub fn local_file_is_complete(dest: &Path, relative_name: &str, expected_size: u64) -> bool {
    let path = dest.join(relative_name);
    let Ok(meta) = std::fs::metadata(&path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    let len = meta.len();
    if expected_size > 0 {
        len == expected_size
    } else {
        len > 0
    }
}

// ---------------------------------------------------------------------------
// hf-hub per-file download retries (transient body/decode/network failures)
// ---------------------------------------------------------------------------
//
// hf-hub 1.x has internal request retries, but its classifier treats body/decode
// errors as non-transient (see upstream `retry::is_transient_reqwest_error`).
// Large multi-shard installs hit "error decoding response body" mid-file; without
// product-level retries the whole install aborts. We retry **per file** so
// completed shards stay skipped via [`local_file_is_complete`].

/// Default total attempts per file (first try + retries).
pub const INSTALL_DOWNLOAD_MAX_ATTEMPTS: u32 = 6;
/// Default delay before the first retry (doubles each subsequent retry).
pub const INSTALL_DOWNLOAD_INITIAL_BACKOFF: Duration = Duration::from_secs(1);
/// Cap on per-retry wait (after exponential growth).
pub const INSTALL_DOWNLOAD_MAX_BACKOFF: Duration = Duration::from_secs(60);

/// Policy for per-file hf-hub download retries.
///
/// Defaults: 6 attempts, 1s initial backoff, 60s max delay. Optional env:
/// - `COLIBRI_INSTALL_MAX_ATTEMPTS` (total tries including the first; min 1)
/// - `COLIBRI_INSTALL_INITIAL_BACKOFF_MS`
/// - `COLIBRI_INSTALL_MAX_BACKOFF_MS`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallRetryPolicy {
    /// Total tries per file (including the first). Must be >= 1.
    pub max_attempts: u32,
    /// Delay before the first retry; then exponential: `initial * 2^retry_index`.
    pub initial_backoff: Duration,
    /// Upper bound for a single sleep between attempts.
    pub max_backoff: Duration,
}

impl Default for InstallRetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: INSTALL_DOWNLOAD_MAX_ATTEMPTS,
            initial_backoff: INSTALL_DOWNLOAD_INITIAL_BACKOFF,
            max_backoff: INSTALL_DOWNLOAD_MAX_BACKOFF,
        }
    }
}

impl InstallRetryPolicy {
    /// Load from env overrides when present; otherwise defaults.
    pub fn from_env() -> Self {
        let mut p = Self::default();
        if let Ok(raw) = std::env::var("COLIBRI_INSTALL_MAX_ATTEMPTS") {
            if let Ok(n) = raw.trim().parse::<u32>() {
                p.max_attempts = n.max(1);
            }
        } else if let Ok(raw) = std::env::var("COLIBRI_INSTALL_MAX_RETRIES") {
            // Treat as *extra* retries after the first try (name is "retries").
            if let Ok(n) = raw.trim().parse::<u32>() {
                p.max_attempts = n.saturating_add(1).max(1);
            }
        }
        if let Ok(raw) = std::env::var("COLIBRI_INSTALL_INITIAL_BACKOFF_MS") {
            if let Ok(ms) = raw.trim().parse::<u64>() {
                p.initial_backoff = Duration::from_millis(ms.max(1));
            }
        }
        if let Ok(raw) = std::env::var("COLIBRI_INSTALL_MAX_BACKOFF_MS") {
            if let Ok(ms) = raw.trim().parse::<u64>() {
                p.max_backoff = Duration::from_millis(ms.max(1));
            }
        }
        if p.max_backoff < p.initial_backoff {
            p.max_backoff = p.initial_backoff;
        }
        p
    }
}

/// Pure backoff schedule: delay after failure of attempt `failed_attempt` (1-based)
/// before the next try. `failed_attempt == 1` → `initial`; then doubles, capped.
///
/// Schedule with defaults (no jitter): 1s, 2s, 4s, 8s, 16s, 32s (capped at 60s).
pub fn backoff_before_retry(policy: &InstallRetryPolicy, failed_attempt: u32) -> Duration {
    if failed_attempt == 0 {
        return Duration::ZERO;
    }
    // retry_index 0 after first failure → initial; then *2 each time.
    let retry_index = failed_attempt.saturating_sub(1);
    let base_ms = policy.initial_backoff.as_millis();
    // Cap shift so 2^n does not overflow the shift amount.
    let shift = retry_index.min(20);
    let ms = base_ms.saturating_mul(1u128 << shift);
    let cap = policy.max_backoff.as_millis();
    Duration::from_millis(ms.min(cap) as u64)
}

/// Apply light full-jitter in `[50%, 100%]` of `delay` so concurrent installs
/// do not thundering-herd. Pure schedule tests use [`backoff_before_retry`] only.
fn apply_backoff_jitter(delay: Duration) -> Duration {
    if delay.is_zero() {
        return delay;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    // factor in [50, 100] percent of the scheduled delay.
    let pct = 50 + (nanos % 51);
    let ms = delay.as_millis();
    let jittered = (ms * u128::from(pct)) / 100;
    Duration::from_millis(jittered.max(1) as u64)
}

/// Sleep up to `total`, checking cooperative cancel/pause about every 100ms.
fn sleep_interruptible(total: Duration, cancel: Option<&InstallCancel>) -> Result<()> {
    let step = Duration::from_millis(100);
    let mut left = total;
    while left > Duration::ZERO {
        check_cancel(cancel)?;
        let slice = left.min(step);
        thread::sleep(slice);
        left = left.saturating_sub(slice);
    }
    Ok(())
}

/// Classify a Display / error string as a **transient** download failure worth retrying.
///
/// True for transport/body-decode/timeout/reset and common 5xx / 429 text.
/// False for permanent Hub cases (404, auth, forbidden, invalid params).
pub fn is_transient_download_error_message(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();

    // Permanent first (do not infinite-retry).
    const PERMANENT: &[&str] = &[
        "entry not found",
        "repository not found",
        "revision not found",
        "bucket not found",
        "authentication required",
        "401 unauthorized",
        "403 forbidden",
        "404 not found",
        "invalid parameter",
        "gated repo",
        "gated repository",
        "not authenticated",
        "access denied",
        "permission denied",
        "cache is not enabled",
        "file not found in local cache",
        "install cancelled",
        "install paused",
    ];
    for p in PERMANENT {
        if lower.contains(p) {
            return false;
        }
    }

    // Explicit permanent HTTP client statuses (when not already caught above).
    if lower.contains("http error: 400")
        || lower.contains("http error: 401")
        || lower.contains("http error: 403")
        || lower.contains("http error: 404")
        || lower.contains("http error: 409")
        || lower.contains("http error: 422")
    {
        return false;
    }

    const TRANSIENT: &[&str] = &[
        // Operator case + reqwest body path (hf-hub does NOT retry these).
        "error decoding response body",
        "decoding response body",
        "error reading a body from connection",
        "connection reset",
        "connection aborted",
        "connection refused",
        "broken pipe",
        "unexpected eof",
        "timed out",
        "timeout",
        "temporarily unavailable",
        "try again",
        "rate limited",
        "too many requests",
        "http request error",
        "incomplete message",
        "error sending request",
        "connection closed",
        "reset by peer",
        "network is unreachable",
        "name or service not known",
        "temporary failure",
        "503",
        "502",
        "504",
        "500 internal",
        "http error: 408",
        "http error: 425",
        "http error: 429",
        "http error: 500",
        "http error: 502",
        "http error: 503",
        "http error: 504",
        "cache lock timed out",
    ];
    for t in TRANSIENT {
        if lower.contains(t) {
            return true;
        }
    }
    false
}

/// Classify a typed [`hf_hub::HFError`] for product-level download retries.
///
/// Broader than hf-hub's internal `is_transient`: **body/decode** errors and
/// [`HFError::RateLimited`] are treated as retryable.
fn is_transient_hf_error(err: &hf_hub::HFError) -> bool {
    use hf_hub::HFError;
    match err {
        HFError::Request { source, .. } => {
            if source.is_timeout()
                || source.is_connect()
                || source.is_body()
                || source.is_decode()
                || source.is_request()
            {
                return true;
            }
            is_transient_download_error_message(&source.to_string())
                || is_transient_download_error_message(&err.to_string())
        }
        HFError::RateLimited { .. } => true,
        HFError::Http { context } => {
            let s = context.status.as_u16();
            matches!(s, 408 | 425 | 429 | 500..=599)
        }
        HFError::CacheLockTimeout { .. } => true,
        HFError::Io(e) => matches!(
            e.kind(),
            std::io::ErrorKind::Interrupted
                | std::io::ErrorKind::TimedOut
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::UnexpectedEof
                | std::io::ErrorKind::WouldBlock
                | std::io::ErrorKind::NotConnected
        ),
        HFError::Xet { source, .. } => is_transient_download_error_message(&source.to_string()),
        HFError::AuthRequired { .. }
        | HFError::RepoNotFound { .. }
        | HFError::RevisionNotFound { .. }
        | HFError::EntryNotFound { .. }
        | HFError::BucketNotFound { .. }
        | HFError::Forbidden { .. }
        | HFError::Conflict { .. }
        | HFError::LocalEntryNotFound { .. }
        | HFError::CacheNotEnabled
        | HFError::InvalidParameter(_)
        | HFError::Json(_)
        | HFError::Url(_)
        | HFError::DiffParse(_) => false,
        HFError::MalformedResponse { .. } => true,
        HFError::Other(msg) => is_transient_download_error_message(msg),
        // non_exhaustive catch-all: fall back to Display text.
        other => is_transient_download_error_message(&other.to_string()),
    }
}

/// Run `op` up to `policy.max_attempts` times with exponential backoff on transient errors.
///
/// `op` receives the 1-based attempt number. `on_retry` is called after a transient
/// failure when another attempt remains (with the delay that will be used).
/// `sleep_fn` is the wait between attempts (inject a no-op in unit tests).
pub fn retry_transient<T, E, F, S, R>(
    policy: &InstallRetryPolicy,
    is_transient: impl Fn(&E) -> bool,
    mut op: F,
    mut sleep_fn: S,
    mut on_retry: R,
) -> std::result::Result<T, E>
where
    F: FnMut(u32) -> std::result::Result<T, E>,
    S: FnMut(Duration),
    R: FnMut(u32, &E, Duration),
    E: std::fmt::Display,
{
    let max = policy.max_attempts.max(1);
    let mut last_err: Option<E> = None;
    for attempt in 1..=max {
        match op(attempt) {
            Ok(v) => return Ok(v),
            Err(e) => {
                let transient = is_transient(&e);
                let has_more = attempt < max;
                if transient && has_more {
                    let delay = apply_backoff_jitter(backoff_before_retry(policy, attempt));
                    on_retry(attempt, &e, delay);
                    sleep_fn(delay);
                    last_err = Some(e);
                    continue;
                }
                return Err(e);
            }
        }
    }
    // Unreachable when max >= 1, but keep a path for empty max after clamp.
    Err(last_err.expect("retry_transient: no attempts"))
}

/// Format the exhausted-retry install error (plain American English).
pub fn format_download_retries_exhausted(
    file_name: &str,
    attempts: u32,
    last_error: &str,
) -> String {
    format!("hf-hub download_file {file_name} failed after {attempts} attempts: {last_error}")
}

/// Source of a model install job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InstallSource {
    /// Hugging Face repo id, optional revision.
    HuggingFace {
        repo_id: String,
        revision: Option<String>,
        /// Optional filename filter (HF snapshot / `--include` patterns).
        /// Glob-style: `*` matches within one path segment; `**` is treated as `*`.
        allow_patterns: Option<Vec<String>>,
    },
    /// Local directory copy / register only.
    LocalPath { path: PathBuf },
}

/// Progress callback payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallProgress {
    pub phase: String,
    pub message: String,
    pub bytes_done: Option<u64>,
    pub bytes_total: Option<u64>,
    /// Optional current file name (multi-shard downloads).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Optional file index / total for multi-file snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_done: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_total: Option<u32>,
}

/// Sum completed prior-file bytes with the in-flight file's partial bytes.
///
/// Pure helper so multi-shard downloads can report mid-file progress without
/// double-counting. Saturating add; does not clamp to a total.
pub fn aggregate_download_bytes(completed_prior: u64, current_file_partial: u64) -> u64 {
    completed_prior.saturating_add(current_file_partial)
}

/// Build a download-phase [`InstallProgress`] from multi-file + mid-file counters.
///
/// - `completed_prior`: bytes of fully finished files
/// - `current_partial`: bytes written so far for the active file (0 at start)
/// - `bytes_total`: hub tree sum when known (`None` / 0 → omitted)
pub fn download_progress_event(
    message: impl Into<String>,
    completed_prior: u64,
    current_partial: u64,
    bytes_total: Option<u64>,
    file: Option<String>,
    files_done: Option<u32>,
    files_total: Option<u32>,
) -> InstallProgress {
    let done = aggregate_download_bytes(completed_prior, current_partial);
    let bytes_total = bytes_total.filter(|t| *t > 0);
    InstallProgress {
        phase: "download".into(),
        message: message.into(),
        bytes_done: Some(done),
        bytes_total,
        file,
        files_done,
        files_total,
    }
}

/// Shared mid-file progress snapshot for hosts that poll while a hub download
/// blocks (hf-hub ProgressHandler updates atomics from worker threads).
///
/// UI should call [`InstallLiveProgress::snapshot`] on a timer so the bar moves
/// during multi-GB shards even when channel events are sparse.
#[derive(Debug, Default)]
pub struct InstallLiveProgress {
    phase: std::sync::Mutex<String>,
    message: std::sync::Mutex<String>,
    file: std::sync::Mutex<Option<String>>,
    bytes_done: std::sync::atomic::AtomicU64,
    /// 0 means unknown / omit.
    bytes_total: std::sync::atomic::AtomicU64,
    files_done: std::sync::atomic::AtomicU32,
    files_total: std::sync::atomic::AtomicU32,
    /// 0 = totals unknown (files_total not set).
    has_files: std::sync::atomic::AtomicBool,
}

impl InstallLiveProgress {
    pub fn new() -> Self {
        Self {
            phase: std::sync::Mutex::new(String::new()),
            message: std::sync::Mutex::new(String::new()),
            file: std::sync::Mutex::new(None),
            bytes_done: std::sync::atomic::AtomicU64::new(0),
            bytes_total: std::sync::atomic::AtomicU64::new(0),
            files_done: std::sync::atomic::AtomicU32::new(0),
            files_total: std::sync::atomic::AtomicU32::new(0),
            has_files: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Publish a full progress event into shared state (and return a clone for channels).
    pub fn publish(&self, p: &InstallProgress) {
        if let Ok(mut g) = self.phase.lock() {
            *g = p.phase.clone();
        }
        if let Ok(mut g) = self.message.lock() {
            *g = p.message.clone();
        }
        if let Ok(mut g) = self.file.lock() {
            *g = p.file.clone();
        }
        if let Some(d) = p.bytes_done {
            self.bytes_done.store(d, Ordering::Relaxed);
        }
        if let Some(t) = p.bytes_total {
            if t > 0 {
                self.bytes_total.store(t, Ordering::Relaxed);
            }
            // Some(0) / omitted: keep prior total
        }
        if let Some(fd) = p.files_done {
            self.files_done.store(fd, Ordering::Relaxed);
        }
        if let Some(ft) = p.files_total {
            self.files_total.store(ft, Ordering::Relaxed);
            self.has_files.store(true, Ordering::Relaxed);
        }
    }

    /// Fast path for mid-file byte ticks (hot path from ProgressHandler).
    pub fn set_bytes_done(&self, done: u64) {
        self.bytes_done.store(done, Ordering::Relaxed);
    }

    /// Snapshot as [`InstallProgress`] for UI math.
    pub fn snapshot(&self) -> InstallProgress {
        let phase = self
            .phase
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|_| "download".into());
        let message = self.message.lock().map(|g| g.clone()).unwrap_or_default();
        let file = self.file.lock().ok().and_then(|g| g.clone());
        let bytes_done = self.bytes_done.load(Ordering::Relaxed);
        let bt = self.bytes_total.load(Ordering::Relaxed);
        let bytes_total = if bt > 0 { Some(bt) } else { None };
        let has_files = self.has_files.load(Ordering::Relaxed);
        let (files_done, files_total) = if has_files {
            (
                Some(self.files_done.load(Ordering::Relaxed)),
                Some(self.files_total.load(Ordering::Relaxed)),
            )
        } else {
            (None, None)
        };
        InstallProgress {
            phase: if phase.is_empty() {
                "download".into()
            } else {
                phase
            },
            message,
            bytes_done: Some(bytes_done),
            bytes_total,
            file,
            files_done,
            files_total,
        }
    }
}

/// Result of an install job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallResult {
    pub dest: PathBuf,
    pub source: String,
    pub notes: Vec<String>,
    /// Present when post-install inspect succeeded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_info: Option<ModelInfoSummary>,
}

/// Lightweight inspect summary (avoids shipping full config in install JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfoSummary {
    pub shards: usize,
    pub model_bytes: u64,
    /// Weight size on disk (bytes); same as `model_bytes`.
    #[serde(default)]
    pub disk_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param_count: Option<u64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub engine_id: String,
    pub has_config: bool,
    pub has_tokenizer: bool,
    pub family: Option<String>,
}

impl From<&ModelInfo> for ModelInfoSummary {
    fn from(m: &ModelInfo) -> Self {
        Self {
            shards: m.shards,
            model_bytes: m.model_bytes,
            disk_bytes: if m.disk_bytes > 0 {
                m.disk_bytes
            } else {
                m.model_bytes
            },
            param_count: m.param_count,
            engine_id: m.engine_id.clone(),
            has_config: m.has_config,
            has_tokenizer: m.has_tokenizer,
            family: m.family.map(|f| f.as_str().to_string()),
        }
    }
}

/// Options for download orchestration.
#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub dest: PathBuf,
    /// Prefer `hf` CLI if on PATH; else try `hf-hub` crate.
    pub prefer_cli: bool,
    /// Minimum free disk bytes required before download (0 = skip check).
    pub min_free_bytes: u64,
    /// After download, run `ModelInfo::inspect` and fill [`InstallResult::model_info`].
    pub inspect_after: bool,
    /// When set, register the dest path into this registry after a successful install.
    /// Not used for `LocalPath` when the path is already the source.
    pub register: bool,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            dest: PathBuf::from("."),
            prefer_cli: true,
            min_free_bytes: 0,
            inspect_after: true,
            register: false,
        }
    }
}

/// Abstraction over `hf download` for unit tests.
pub trait HfCliRunner {
    fn available(&self) -> bool;
    /// Download snapshot. When `cancel` is set and requested, abort (kill child when applicable).
    fn download(
        &self,
        repo_id: &str,
        revision: Option<&str>,
        include: &[String],
        dest: &Path,
        cancel: Option<&InstallCancel>,
    ) -> Result<()>;
}

/// Real process `hf` CLI.
#[derive(Debug, Default)]
pub struct SystemHfCli;

impl HfCliRunner for SystemHfCli {
    fn available(&self) -> bool {
        Command::new("hf")
            .arg("--help")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn download(
        &self,
        repo_id: &str,
        revision: Option<&str>,
        include: &[String],
        dest: &Path,
        cancel: Option<&InstallCancel>,
    ) -> Result<()> {
        check_cancel(cancel)?;
        let mut cmd = Command::new("hf");
        cmd.arg("download")
            .arg(repo_id)
            .arg("--local-dir")
            .arg(dest);
        if let Some(rev) = revision {
            cmd.arg("--revision").arg(rev);
        }
        for p in include {
            cmd.arg("--include").arg(p);
        }
        // Spawn + poll so we can kill the child when cancel is requested.
        // Lower child priority so long downloads do not starve the UI process.
        crate::process_priority::apply_low_compute_priority(&mut cmd);
        let mut child = cmd
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::Install(e.to_string()))?;
        loop {
            if let Err(e) = check_cancel(cancel) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
            match child
                .try_wait()
                .map_err(|e| Error::Install(e.to_string()))?
            {
                Some(status) => {
                    if !status.success() {
                        // Prefer cancel message if the race lost to kill.
                        check_cancel(cancel)?;
                        return Err(Error::Install(format!(
                            "`hf download` failed with status {status}"
                        )));
                    }
                    return Ok(());
                }
                None => thread::sleep(Duration::from_millis(50)),
            }
        }
    }
}

/// Space check against destination filesystem.
pub fn ensure_space(dest: &Path, min_free: u64) -> Result<()> {
    if min_free == 0 {
        return Ok(());
    }
    let free =
        disk_free_bytes(dest).or_else(|_| dest.parent().map(disk_free_bytes).unwrap_or(Ok(0)))?;
    if free < min_free {
        return Err(Error::Install(format!(
            "insufficient disk space: need ~{:.1} GB free, have {:.1} GB at {}",
            min_free as f64 / GB as f64,
            free as f64 / GB as f64,
            dest.display()
        )));
    }
    Ok(())
}

/// Simple HF allow-pattern match (`*` = any chars; `?` = one char).
///
/// Patterns without a slash also match basename-only (HF snapshot semantics).
pub fn match_allow_pattern(path: &str, pattern: &str) -> bool {
    let pat = pattern.replace("**", "*");
    if glob_match(&pat, path) {
        return true;
    }
    if !pattern.contains('/') {
        if let Some(base) = path.rsplit('/').next() {
            return glob_match(&pat, base);
        }
    }
    false
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let pb: Vec<char> = pattern.chars().collect();
    let tb: Vec<char> = text.chars().collect();
    fn rec(p: &[char], t: &[char]) -> bool {
        match (p.first(), t.first()) {
            (None, None) => true,
            (Some('*'), _) => {
                // consume * greedily with backtrack
                for i in 0..=t.len() {
                    if rec(&p[1..], &t[i..]) {
                        return true;
                    }
                }
                false
            }
            (Some('?'), Some(_)) => rec(&p[1..], &t[1..]),
            (Some(a), Some(b)) if a == b => rec(&p[1..], &t[1..]),
            _ => false,
        }
    }
    rec(&pb, &tb)
}

/// Filter remote file names by optional allow patterns (empty/None = all).
pub fn filter_by_allow_patterns(names: &[String], allow: Option<&[String]>) -> Vec<String> {
    match allow {
        None | Some([]) => names.to_vec(),
        Some(patterns) => names
            .iter()
            .filter(|n| patterns.iter().any(|p| match_allow_pattern(n, p)))
            .cloned()
            .collect(),
    }
}

/// Detect incomplete HF / hub downloads under `dest`.
///
/// Looks for common partial markers: `*.incomplete`, `*.tmp`, `.cache/huggingface`
/// lock leftovers, and zero-length `*.safetensors`.
pub fn detect_incomplete_download(dest: &Path) -> Vec<String> {
    let mut issues = Vec::new();
    if !dest.is_dir() {
        return issues;
    }
    fn walk(dir: &Path, issues: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for ent in rd.flatten() {
            let p = ent.path();
            let name = ent.file_name().to_string_lossy().into_owned();
            if p.is_dir() {
                if name == ".cache" || name.ends_with(".lock") {
                    issues.push(format!("cache/lock present: {}", p.display()));
                }
                walk(&p, issues);
                continue;
            }
            if name.ends_with(".incomplete")
                || name.ends_with(".tmp")
                || name.ends_with(".download")
            {
                issues.push(format!("partial file: {}", p.display()));
            }
            if name.ends_with(".safetensors") {
                if let Ok(meta) = p.metadata() {
                    if meta.len() == 0 {
                        issues.push(format!("zero-length shard: {}", p.display()));
                    }
                }
            }
        }
    }
    walk(dest, &mut issues);
    issues
}

/// Download a model into `opts.dest` using the system `hf` CLI / hf-hub.
pub fn install_model(
    source: &InstallSource,
    opts: &InstallOptions,
    on_progress: impl FnMut(InstallProgress),
) -> Result<InstallResult> {
    install_model_with(source, opts, &SystemHfCli, None, None, None, on_progress)
}

/// Same as [`install_model`] with a cancel handle (kill CLI child / stop hub loop).
pub fn install_model_cancellable(
    source: &InstallSource,
    opts: &InstallOptions,
    cancel: &InstallCancel,
    on_progress: impl FnMut(InstallProgress),
) -> Result<InstallResult> {
    install_model_with(
        source,
        opts,
        &SystemHfCli,
        None,
        Some(cancel),
        None,
        on_progress,
    )
}

/// Like [`install_model_cancellable`] plus shared live progress for mid-file UI polls.
pub fn install_model_cancellable_live(
    source: &InstallSource,
    opts: &InstallOptions,
    cancel: &InstallCancel,
    live: Arc<InstallLiveProgress>,
    on_progress: impl FnMut(InstallProgress),
) -> Result<InstallResult> {
    install_model_with(
        source,
        opts,
        &SystemHfCli,
        None,
        Some(cancel),
        Some(live),
        on_progress,
    )
}

/// Install with injectable CLI runner, optional registry, cancel, and live progress.
pub fn install_model_with(
    source: &InstallSource,
    opts: &InstallOptions,
    cli: &dyn HfCliRunner,
    registry: Option<&mut ModelRegistry>,
    cancel: Option<&InstallCancel>,
    live: Option<Arc<InstallLiveProgress>>,
    mut on_progress: impl FnMut(InstallProgress),
) -> Result<InstallResult> {
    let mut emit = |p: InstallProgress| {
        if let Some(ref live) = live {
            live.publish(&p);
        }
        on_progress(p);
    };
    check_cancel(cancel)?;
    ensure_space(&opts.dest, opts.min_free_bytes)?;
    std::fs::create_dir_all(&opts.dest)?;

    let mut result = match source {
        InstallSource::LocalPath { path } => {
            check_cancel(cancel)?;
            emit(InstallProgress {
                phase: "register".into(),
                message: format!("register local path {}", path.display()),
                bytes_done: None,
                bytes_total: None,
                file: None,
                files_done: None,
                files_total: None,
            });
            InstallResult {
                dest: path.clone(),
                source: path.display().to_string(),
                notes: vec!["local path; no download".into()],
                model_info: None,
            }
        }
        InstallSource::HuggingFace {
            repo_id,
            revision,
            allow_patterns,
        } => {
            check_cancel(cancel)?;
            emit(InstallProgress {
                phase: "download".into(),
                message: format!("fetching {repo_id}"),
                bytes_done: None,
                bytes_total: None,
                file: None,
                files_done: None,
                files_total: None,
            });
            let include = allow_patterns.clone().unwrap_or_default();
            let mut notes = Vec::new();
            if opts.prefer_cli && cli.available() {
                cli.download(repo_id, revision.as_deref(), &include, &opts.dest, cancel)?;
                notes.push("downloaded via `hf` CLI".into());
            } else {
                download_via_hf_hub(
                    repo_id,
                    revision.as_deref(),
                    allow_patterns.as_deref(),
                    &opts.dest,
                    cancel,
                    live.clone(),
                    &mut emit,
                )?;
                notes.push("downloaded via hf-hub crate (list_tree + download_file)".into());
            }
            check_cancel(cancel)?;
            let incomplete = detect_incomplete_download(&opts.dest);
            if !incomplete.is_empty() {
                return Err(Error::Install(format!(
                    "incomplete download under {}: {}",
                    opts.dest.display(),
                    incomplete.join("; ")
                )));
            }
            InstallResult {
                dest: opts.dest.clone(),
                source: repo_id.clone(),
                notes,
                model_info: None,
            }
        }
    };

    check_cancel(cancel)?;
    if opts.inspect_after {
        emit(InstallProgress {
            phase: "inspect".into(),
            message: format!("inspect {}", result.dest.display()),
            bytes_done: None,
            bytes_total: None,
            file: None,
            files_done: None,
            files_total: None,
        });
        match ModelInfo::inspect(&result.dest) {
            Ok(info) => {
                result
                    .notes
                    .push(format!("inspect ok: {} shards", info.shards));
                result.model_info = Some(ModelInfoSummary::from(&info));
            }
            Err(e) => {
                result.notes.push(format!("inspect deferred/failed: {e}"));
            }
        }
    }

    if opts.register {
        if let Some(reg) = registry {
            emit(InstallProgress {
                phase: "register".into(),
                message: format!("registry register {}", result.dest.display()),
                bytes_done: None,
                bytes_total: None,
                file: None,
                files_done: None,
                files_total: None,
            });
            reg.register(&result.dest)?;
            result.notes.push("registered in model registry".into());
        }
    }

    emit(InstallProgress {
        phase: "done".into(),
        message: format!("install complete at {}", result.dest.display()),
        bytes_done: None,
        bytes_total: None,
        file: None,
        files_done: None,
        files_total: None,
    });
    Ok(result)
}

/// Full snapshot via hf-hub 1.x: recursive `list_tree` then selective `download_file`.
///
/// Materializes each matching file under `dest` with its repo-relative path
/// (via `local_dir`). Prefer-cli path is unchanged; this is the fallback when
/// the system `hf` CLI is missing or skipped.
///
/// Mid-file byte progress: each `download_file` is wired with hf-hub's
/// [`hf_hub::progress::ProgressHandler`], which updates [`InstallLiveProgress`]
/// atomics (UI polls via [`InstallLiveProgress::snapshot`]). File-boundary
/// events still go through `on_progress` for channel consumers.
///
/// **Retries:** each `download_file` is retried on transient errors (body decode,
/// timeouts, resets, 5xx, 429) with exponential backoff ([`InstallRetryPolicy`]).
/// Permanent Hub errors (404, auth, forbidden) fail immediately. Completed local
/// files are still skipped for resume.
fn download_via_hf_hub(
    repo_id: &str,
    revision: Option<&str>,
    allow_patterns: Option<&[String]>,
    dest: &Path,
    cancel: Option<&InstallCancel>,
    live: Option<Arc<InstallLiveProgress>>,
    on_progress: &mut dyn FnMut(InstallProgress),
) -> Result<()> {
    use hf_hub::repository::RepoTreeEntry;
    use hf_hub::{HFClientSync, split_id};

    check_cancel(cancel)?;
    let policy = InstallRetryPolicy::from_env();
    let client = HFClientSync::new().map_err(|e| Error::Install(e.to_string()))?;
    let (owner, name) = split_id(repo_id);
    if owner.is_empty() || name.is_empty() {
        return Err(Error::Install(format!(
            "invalid Hugging Face repo id {repo_id:?}; expected owner/name"
        )));
    }
    let repo = client.model(owner, name);
    let rev = revision.map(str::to_string);

    // list_tree: light retry (same policy) so a single blip does not abort install.
    let entries = {
        let rev_list = rev.clone();
        let list_result: std::result::Result<_, hf_hub::HFError> = retry_transient(
            &policy,
            |e: &hf_hub::HFError| is_transient_hf_error(e),
            |_attempt| {
                repo.list_tree()
                    .recursive(true)
                    .maybe_revision(rev_list.clone())
                    .send()
            },
            |delay| {
                // Best-effort: ignore cancel errors during list backoff; check after.
                let _ = sleep_interruptible(delay, cancel);
            },
            |attempt, _err, delay| {
                on_progress(InstallProgress {
                    phase: "download".into(),
                    message: format!(
                        "Retrying list_tree (attempt {}/{}) after transient error; waiting {}s",
                        attempt + 1,
                        policy.max_attempts,
                        delay.as_secs().max(1)
                    ),
                    bytes_done: None,
                    bytes_total: None,
                    file: None,
                    files_done: None,
                    files_total: None,
                });
            },
        );
        match list_result {
            Ok(e) => e,
            Err(e) => {
                let msg = e.to_string();
                if is_transient_hf_error(&e) {
                    return Err(Error::Install(format!(
                        "hf-hub list_tree failed after {} attempts: {msg}",
                        policy.max_attempts
                    )));
                }
                return Err(Error::Install(format!("hf-hub list_tree: {msg}")));
            }
        }
    };
    // If cancel fired during list backoff, surface it cleanly.
    check_cancel(cancel)?;

    // Keep Hub-reported sizes so hosts can show byte + file determinate progress.
    let all: Vec<(String, u64)> = entries
        .into_iter()
        .filter_map(|e| match e {
            RepoTreeEntry::File { path, size, .. } => Some((path, size)),
            RepoTreeEntry::Directory { .. } => None,
        })
        .collect();
    let tree_count = all.len();
    let files = filter_entries_with_sizes(&all, allow_patterns);
    if files.is_empty() {
        return Err(Error::Install(format!(
            "no files matched allow_patterns in repo {repo_id} ({tree_count} tree files)"
        )));
    }
    let total = files.len() as u32;
    let bytes_total: u64 = files.iter().map(|(_, s)| *s).sum();
    let bytes_total_opt = if bytes_total > 0 {
        Some(bytes_total)
    } else {
        None
    };

    let mut bytes_done: u64 = 0;
    for (i, (file_name, size)) in files.iter().enumerate() {
        check_cancel(cancel)?;
        // Resume: skip shards already on disk with matching size (see
        // [`local_file_is_complete`]). Without this, re-install re-downloads all.
        if local_file_is_complete(dest, file_name, *size) {
            bytes_done = bytes_done.saturating_add(*size);
            on_progress(download_progress_event(
                format!("skip {file_name} (already complete)"),
                bytes_done,
                0,
                bytes_total_opt,
                Some(file_name.clone()),
                Some((i as u32).saturating_add(1)),
                Some(total),
            ));
            continue;
        }
        on_progress(download_progress_event(
            format!("get {file_name}"),
            bytes_done,
            0,
            bytes_total_opt,
            Some(file_name.clone()),
            Some(i as u32),
            Some(total),
        ));

        // Per-file retries: do not restart the whole snapshot on a flaky shard.
        let completed_prior = bytes_done;
        let file_name_owned = file_name.clone();
        let live_for_file = live.clone();
        let rev_file = rev.clone();
        let download_result: std::result::Result<(), hf_hub::HFError> = retry_transient(
            &policy,
            |e: &hf_hub::HFError| is_transient_hf_error(e),
            |_attempt| {
                // Cooperative cancel/pause: permanent for the classifier (no retry spin).
                if let Err(ce) = check_cancel(cancel) {
                    return Err(hf_hub::HFError::Other(ce.to_string()));
                }
                // Mid-file ticks: ProgressHandler updates live atomics (UI polls snapshot).
                let handler = HubDownloadProgress {
                    live: live_for_file.clone(),
                    completed_prior,
                    bytes_total: bytes_total_opt,
                    files_done: i as u32,
                    files_total: total,
                    file_name: file_name_owned.clone(),
                    last_publish: std::sync::Mutex::new(std::time::Instant::now()),
                };
                // `local_dir` writes the file under dest with its repo path structure.
                repo.download_file()
                    .filename(file_name_owned.as_str())
                    .local_dir(dest)
                    .maybe_revision(rev_file.clone())
                    .progress(handler)
                    .send()
                    .map(|_| ())
            },
            |delay| {
                let _ = sleep_interruptible(delay, cancel);
            },
            |attempt, err, delay| {
                on_progress(download_progress_event(
                    format!(
                        "Retrying download of {file_name_owned} (attempt {}/{}); last error: {err}; waiting {}s",
                        attempt + 1,
                        policy.max_attempts,
                        delay.as_secs().max(1)
                    ),
                    completed_prior,
                    0,
                    bytes_total_opt,
                    Some(file_name_owned.clone()),
                    Some(i as u32),
                    Some(total),
                ));
            },
        );

        match download_result {
            Ok(()) => {}
            Err(e) => {
                let msg = e.to_string();
                // Prefer cancel/pause messages over generic install wrap.
                // Error::Install display is "install error: {msg}"; HFError::Other is bare.
                if msg.contains(INSTALL_CANCELLED_MSG) || msg.contains(INSTALL_PAUSED_MSG) {
                    if msg.contains(INSTALL_PAUSED_MSG) {
                        return Err(Error::Install(INSTALL_PAUSED_MSG.into()));
                    }
                    return Err(Error::Install(INSTALL_CANCELLED_MSG.into()));
                }
                if is_transient_hf_error(&e) {
                    return Err(Error::Install(format_download_retries_exhausted(
                        file_name,
                        policy.max_attempts,
                        &msg,
                    )));
                }
                return Err(Error::Install(format!(
                    "hf-hub download_file {file_name}: {msg}"
                )));
            }
        }

        bytes_done = bytes_done.saturating_add(*size);
        on_progress(download_progress_event(
            format!("got {file_name}"),
            bytes_done,
            0,
            bytes_total_opt,
            Some(file_name.clone()),
            Some((i as u32).saturating_add(1)),
            Some(total),
        ));
    }
    check_cancel(cancel)?;
    on_progress(download_progress_event(
        format!("fetched {total} files"),
        bytes_done,
        0,
        bytes_total_opt,
        None,
        Some(total),
        Some(total),
    ));
    Ok(())
}

/// Throttle optional mid-file channel publishes (atomics always update).
const HUB_PROGRESS_MIN_INTERVAL: Duration = Duration::from_millis(250);

/// hf-hub download progress → live atomics (mid-file byte updates).
struct HubDownloadProgress {
    live: Option<Arc<InstallLiveProgress>>,
    completed_prior: u64,
    bytes_total: Option<u64>,
    files_done: u32,
    files_total: u32,
    file_name: String,
    last_publish: std::sync::Mutex<std::time::Instant>,
}

impl HubDownloadProgress {
    fn apply_partial(&self, current_partial: u64) {
        let done = aggregate_download_bytes(self.completed_prior, current_partial);
        let Some(live) = self.live.as_ref() else {
            return;
        };
        live.set_bytes_done(done);
        // Keep file / totals coherent for snapshot readers.
        let tick = download_progress_event(
            format!("get {}", self.file_name),
            self.completed_prior,
            current_partial,
            self.bytes_total,
            Some(self.file_name.clone()),
            Some(self.files_done),
            Some(self.files_total),
        );
        // Throttle full publish (mutex string clones) but always update bytes.
        let should_publish = {
            let mut last = self.last_publish.lock().unwrap_or_else(|e| e.into_inner());
            let now = std::time::Instant::now();
            if now.duration_since(*last) >= HUB_PROGRESS_MIN_INTERVAL {
                *last = now;
                true
            } else {
                false
            }
        };
        if should_publish {
            live.publish(&tick);
        }
    }
}

impl hf_hub::progress::ProgressHandler for HubDownloadProgress {
    fn on_progress(&self, event: &hf_hub::progress::ProgressEvent) {
        use hf_hub::progress::{DownloadEvent, ProgressEvent};
        match event {
            ProgressEvent::Download(DownloadEvent::Progress { files }) => {
                for f in files {
                    let matches = f.filename == self.file_name
                        || f.filename.ends_with(&self.file_name)
                        || files.len() == 1;
                    if matches {
                        self.apply_partial(f.bytes_completed);
                    }
                }
            }
            ProgressEvent::Download(DownloadEvent::AggregateProgress {
                bytes_completed, ..
            }) => {
                self.apply_partial(*bytes_completed);
            }
            _ => {}
        }
    }
}

/// Filter `(path, size)` pairs with the same allow-pattern rules as
/// [`filter_by_allow_patterns`].
fn filter_entries_with_sizes(
    entries: &[(String, u64)],
    allow: Option<&[String]>,
) -> Vec<(String, u64)> {
    match allow {
        None | Some([]) => entries.to_vec(),
        Some(patterns) => entries
            .iter()
            .filter(|(n, _)| patterns.iter().any(|p| match_allow_pattern(n, p)))
            .cloned()
            .collect(),
    }
}

/// Materialize a multi-file snapshot into `dest` (test / offline helper).
///
/// Copies `(relative_name, bytes)` pairs; used by unit tests to exercise
/// post-inspect without network.
pub fn materialize_snapshot(dest: &Path, files: &[(&str, &[u8])]) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for (name, bytes) in files {
        let target = dest.join(name);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(target, bytes)?;
    }
    Ok(())
}

/// Invoke a Python convert tool as a subprocess (last resort).
///
/// Example: `c/tools/convert_fp8_to_int4.py`. Requires a working Python env
/// with torch/safetensors. Not a Rust port of the kernels.
pub fn convert_subprocess(
    python: &Path,
    script: &Path,
    args: &[String],
) -> Result<std::process::ExitStatus> {
    let mut cmd = Command::new(python);
    cmd.arg(script).args(args);
    crate::process_priority::apply_low_compute_priority(&mut cmd);
    let status = cmd.status().map_err(|e| Error::Install(e.to_string()))?;
    Ok(status)
}

/// Live network install (ignored by default).
///
/// ```ignore
/// // cargo test -p colibri-sys --features install live_hf -- --ignored
/// ```
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    type MockCliCalls = Arc<Mutex<Vec<(String, Vec<String>)>>>;

    struct MockCli {
        available: bool,
        calls: MockCliCalls,
        files: Vec<(String, Vec<u8>)>,
    }

    impl HfCliRunner for MockCli {
        fn available(&self) -> bool {
            self.available
        }
        fn download(
            &self,
            repo_id: &str,
            _revision: Option<&str>,
            include: &[String],
            dest: &Path,
            cancel: Option<&InstallCancel>,
        ) -> Result<()> {
            check_cancel(cancel)?;
            self.calls
                .lock()
                .unwrap()
                .push((repo_id.to_string(), include.to_vec()));
            for (name, bytes) in &self.files {
                check_cancel(cancel)?;
                let target = dest.join(name);
                if let Some(p) = target.parent() {
                    std::fs::create_dir_all(p).unwrap();
                }
                std::fs::write(target, bytes).unwrap();
            }
            Ok(())
        }
    }

    /// Mock that blocks until cancel is requested (install-cancel contract).
    struct BlockingMockCli {
        available: bool,
        saw_download: Arc<AtomicBool>,
    }

    impl HfCliRunner for BlockingMockCli {
        fn available(&self) -> bool {
            self.available
        }
        fn download(
            &self,
            _repo_id: &str,
            _revision: Option<&str>,
            _include: &[String],
            _dest: &Path,
            cancel: Option<&InstallCancel>,
        ) -> Result<()> {
            self.saw_download.store(true, Ordering::SeqCst);
            // Poll like SystemHfCli until cancel or timeout.
            for _ in 0..200 {
                check_cancel(cancel)?;
                thread::sleep(Duration::from_millis(10));
            }
            Err(Error::Install(
                "blocking mock timed out without cancel".into(),
            ))
        }
    }

    fn tiny_safetensors_bytes() -> Vec<u8> {
        // Minimal U8 tensor "t" size 1.
        let header = br#"{"t":{"dtype":"U8","shape":[1],"data_offsets":[0,1]}}"#;
        let mut out = Vec::new();
        out.extend_from_slice(&(header.len() as u64).to_le_bytes());
        out.extend_from_slice(header);
        out.push(0);
        out
    }

    #[test]
    fn local_install_registers() {
        let dir = tempfile::tempdir().unwrap();
        let src = InstallSource::LocalPath {
            path: dir.path().to_path_buf(),
        };
        let opts = InstallOptions {
            dest: dir.path().to_path_buf(),
            prefer_cli: true,
            min_free_bytes: 0,
            inspect_after: false,
            register: false,
        };
        let result = install_model(&src, &opts, |_| {}).unwrap();
        assert_eq!(result.dest, dir.path());
    }

    #[test]
    fn allow_pattern_filters_siblings() {
        let names = vec![
            "config.json".into(),
            "model-00001-of-00002.safetensors".into(),
            "model-00002-of-00002.safetensors".into(),
            "README.md".into(),
        ];
        let pats = vec!["*.safetensors".into(), "config.json".into()];
        let got = filter_by_allow_patterns(&names, Some(&pats));
        assert_eq!(got.len(), 3);
        assert!(got.contains(&"config.json".into()));
        assert!(!got.iter().any(|n| n == "README.md"));
    }

    #[test]
    fn filter_entries_with_sizes_keeps_byte_totals() {
        let entries = vec![
            ("config.json".into(), 12u64),
            ("model.safetensors".into(), 1000u64),
            ("README.md".into(), 50u64),
        ];
        let pats = vec!["*.safetensors".into(), "config.json".into()];
        let got = filter_entries_with_sizes(&entries, Some(&pats));
        assert_eq!(got.len(), 2);
        let sum: u64 = got.iter().map(|(_, s)| *s).sum();
        assert_eq!(sum, 1012);
        assert!(!got.iter().any(|(n, _)| n == "README.md"));
    }

    #[test]
    fn aggregate_download_bytes_sums_prior_and_partial() {
        assert_eq!(aggregate_download_bytes(0, 0), 0);
        assert_eq!(aggregate_download_bytes(100, 0), 100);
        assert_eq!(aggregate_download_bytes(100, 25), 125);
        assert_eq!(aggregate_download_bytes(u64::MAX - 1, 5), u64::MAX);
    }

    #[test]
    fn download_progress_event_mid_file_partial() {
        // Prior files 1 GiB done; current shard 512 MiB of many.
        let prior = 1024 * 1024 * 1024u64;
        let partial = 512 * 1024 * 1024u64;
        let total = 10 * prior;
        let p = download_progress_event(
            "get out-00000.safetensors",
            prior,
            partial,
            Some(total),
            Some("out-00000.safetensors".into()),
            Some(1),
            Some(10),
        );
        assert_eq!(p.phase, "download");
        assert_eq!(p.bytes_done, Some(prior + partial));
        assert_eq!(p.bytes_total, Some(total));
        assert_eq!(p.file.as_deref(), Some("out-00000.safetensors"));
        assert_eq!(p.files_done, Some(1));
        assert_eq!(p.files_total, Some(10));
        // Percent would be ~15% — not stuck at 0 while first large shard runs.
        let pct = (p.bytes_done.unwrap() as u128 * 100 / total as u128) as u8;
        assert!((10..20).contains(&pct), "pct={pct}");
    }

    #[test]
    fn download_progress_event_zero_total_omits_bytes_total() {
        let p = download_progress_event("get x", 0, 10, Some(0), Some("x".into()), None, None);
        assert_eq!(p.bytes_done, Some(10));
        assert_eq!(p.bytes_total, None);
    }

    #[test]
    fn download_progress_event_unknown_total() {
        let p = download_progress_event("get x", 50, 25, None, Some("x".into()), Some(0), Some(3));
        assert_eq!(p.bytes_done, Some(75));
        assert_eq!(p.bytes_total, None);
        assert_eq!(p.files_done, Some(0));
        assert_eq!(p.files_total, Some(3));
    }

    #[test]
    fn match_allow_basename() {
        assert!(match_allow_pattern("foo/bar.safetensors", "*.safetensors"));
        assert!(!match_allow_pattern("foo/bar.bin", "*.safetensors"));
        assert!(match_allow_pattern("config.json", "config.json"));
    }

    #[test]
    fn detect_incomplete_flags_partials() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("x.safetensors.incomplete"), b"x").unwrap();
        std::fs::write(dir.path().join("empty.safetensors"), b"").unwrap();
        let issues = detect_incomplete_download(dir.path());
        assert!(issues.iter().any(|s| s.contains("partial")));
        assert!(issues.iter().any(|s| s.contains("zero-length")));
    }

    #[test]
    fn mocked_multishard_snapshot_creates_config() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model");
        let st = tiny_safetensors_bytes();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let cli = MockCli {
            available: true,
            calls: calls.clone(),
            files: vec![
                ("config.json".into(), br#"{"model_type":"glm"}"#.to_vec()),
                ("tokenizer.json".into(), b"{}".to_vec()),
                ("model-00001-of-00002.safetensors".into(), st.clone()),
                ("model-00002-of-00002.safetensors".into(), st),
            ],
        };
        let src = InstallSource::HuggingFace {
            repo_id: "org/tiny-multi".into(),
            revision: None,
            allow_patterns: Some(vec![
                "config.json".into(),
                "tokenizer.json".into(),
                "*.safetensors".into(),
            ]),
        };
        let opts = InstallOptions {
            dest: dest.clone(),
            prefer_cli: true,
            min_free_bytes: 0,
            inspect_after: true,
            register: true,
        };
        let mut reg = ModelRegistry::open(std::iter::empty::<PathBuf>());
        let mut phases = Vec::new();
        let result = install_model_with(&src, &opts, &cli, Some(&mut reg), None, None, |p| {
            phases.push(p.phase.clone());
        })
        .unwrap();
        assert!(dest.join("config.json").is_file());
        assert!(dest.join("model-00001-of-00002.safetensors").is_file());
        assert!(dest.join("model-00002-of-00002.safetensors").is_file());
        assert_eq!(result.model_info.as_ref().unwrap().shards, 2);
        assert_eq!(reg.entries().len(), 1);
        assert!(phases.contains(&"download".to_string()));
        assert!(phases.contains(&"inspect".to_string()));
        assert!(phases.contains(&"register".to_string()));
        let c = calls.lock().unwrap();
        assert_eq!(c[0].0, "org/tiny-multi");
        assert!(c[0].1.iter().any(|p| p == "*.safetensors"));
    }

    #[test]
    fn incomplete_download_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let cli = MockCli {
            available: true,
            calls,
            files: vec![
                ("config.json".into(), br#"{}"#.to_vec()),
                ("weights.safetensors.incomplete".into(), b"partial".to_vec()),
            ],
        };
        let src = InstallSource::HuggingFace {
            repo_id: "org/broken".into(),
            revision: None,
            allow_patterns: None,
        };
        let opts = InstallOptions {
            dest,
            prefer_cli: true,
            min_free_bytes: 0,
            inspect_after: false,
            register: false,
        };
        let err = install_model_with(&src, &opts, &cli, None, None, None, |_| {}).unwrap_err();
        assert!(err.to_string().contains("incomplete"));
    }

    #[test]
    fn ensure_space_refuses_when_below_threshold() {
        let dir = tempfile::tempdir().unwrap();
        // Unrealistically high threshold always fails on real volumes.
        let err = ensure_space(dir.path(), u64::MAX / 4).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("insufficient disk space"),
            "expected space gate message, got {msg}"
        );
    }

    #[test]
    fn ensure_space_skips_when_zero() {
        let dir = tempfile::tempdir().unwrap();
        ensure_space(dir.path(), 0).unwrap();
    }

    #[test]
    fn pre_set_cancel_aborts_before_download() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model");
        let cancel = InstallCancel::new();
        cancel.request();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let cli = MockCli {
            available: true,
            calls: calls.clone(),
            files: vec![("config.json".into(), br#"{}"#.to_vec())],
        };
        let src = InstallSource::HuggingFace {
            repo_id: "org/x".into(),
            revision: None,
            allow_patterns: None,
        };
        let opts = InstallOptions {
            dest,
            prefer_cli: true,
            min_free_bytes: 0,
            inspect_after: false,
            register: false,
        };
        let err =
            install_model_with(&src, &opts, &cli, None, Some(&cancel), None, |_| {}).unwrap_err();
        assert!(
            err.to_string().contains(INSTALL_CANCELLED_MSG),
            "got {}",
            err
        );
        assert!(calls.lock().unwrap().is_empty());
    }

    #[test]
    fn cancel_mid_download_via_mock_runner() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model");
        let cancel = InstallCancel::new();
        let saw = Arc::new(AtomicBool::new(false));
        let cli = BlockingMockCli {
            available: true,
            saw_download: saw.clone(),
        };
        let src = InstallSource::HuggingFace {
            repo_id: "org/slow".into(),
            revision: None,
            allow_patterns: None,
        };
        let opts = InstallOptions {
            dest,
            prefer_cli: true,
            min_free_bytes: 0,
            inspect_after: false,
            register: false,
        };
        let cancel_bg = cancel.clone();
        let handle = thread::spawn(move || {
            install_model_with(&src, &opts, &cli, None, Some(&cancel_bg), None, |_| {})
        });
        // Wait until mock enters download, then cancel.
        for _ in 0..100 {
            if saw.load(Ordering::SeqCst) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(saw.load(Ordering::SeqCst), "mock never entered download");
        cancel.request();
        let err = handle.join().unwrap().unwrap_err();
        assert!(
            err.to_string().contains(INSTALL_CANCELLED_MSG),
            "got {}",
            err
        );
    }

    #[test]
    fn materialize_snapshot_helper() {
        let dir = tempfile::tempdir().unwrap();
        materialize_snapshot(
            dir.path(),
            &[("a/b.txt", b"hi"), ("config.json", br#"{"x":1}"#)],
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a/b.txt")).unwrap(),
            "hi"
        );
    }

    #[test]
    fn local_file_is_complete_matches_size() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path();
        std::fs::write(dest.join("config.json"), br#"{"a":1}"#).unwrap();
        let len = std::fs::metadata(dest.join("config.json")).unwrap().len();
        assert!(local_file_is_complete(dest, "config.json", len));
        assert!(!local_file_is_complete(dest, "config.json", len + 1));
        assert!(!local_file_is_complete(dest, "missing.bin", 10));
    }

    #[test]
    fn local_file_is_complete_nested_and_zero_size_heuristic() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path();
        std::fs::create_dir_all(dest.join("weights")).unwrap();
        std::fs::write(dest.join("weights/model.safetensors"), b"abcd").unwrap();
        std::fs::write(dest.join("empty.bin"), b"").unwrap();
        // Known expected size.
        assert!(local_file_is_complete(dest, "weights/model.safetensors", 4));
        assert!(!local_file_is_complete(
            dest,
            "weights/model.safetensors",
            99
        ));
        // Unknown size (0): non-empty counts as complete; empty does not.
        assert!(local_file_is_complete(dest, "weights/model.safetensors", 0));
        assert!(!local_file_is_complete(dest, "empty.bin", 0));
        assert!(!local_file_is_complete(dest, "empty.bin", 1));
    }

    #[test]
    fn request_pause_returns_paused_message() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model");
        let cancel = InstallCancel::new();
        cancel.request_pause();
        assert_eq!(cancel.stop_kind(), Some(InstallStopKind::Pause));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let cli = MockCli {
            available: true,
            calls: calls.clone(),
            files: vec![("config.json".into(), br#"{}"#.to_vec())],
        };
        let src = InstallSource::HuggingFace {
            repo_id: "org/x".into(),
            revision: None,
            allow_patterns: None,
        };
        let opts = InstallOptions {
            dest,
            prefer_cli: true,
            min_free_bytes: 0,
            inspect_after: false,
            register: false,
        };
        let err =
            install_model_with(&src, &opts, &cli, None, Some(&cancel), None, |_| {}).unwrap_err();
        assert!(err.to_string().contains(INSTALL_PAUSED_MSG), "got {}", err);
        assert!(
            !err.to_string().contains(INSTALL_CANCELLED_MSG),
            "pause must not look like cancel: {}",
            err
        );
        assert!(calls.lock().unwrap().is_empty());
    }

    #[test]
    fn pause_mid_download_via_mock_runner() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model");
        let cancel = InstallCancel::new();
        let saw = Arc::new(AtomicBool::new(false));
        let cli = BlockingMockCli {
            available: true,
            saw_download: saw.clone(),
        };
        let src = InstallSource::HuggingFace {
            repo_id: "org/slow".into(),
            revision: None,
            allow_patterns: None,
        };
        let opts = InstallOptions {
            dest,
            prefer_cli: true,
            min_free_bytes: 0,
            inspect_after: false,
            register: false,
        };
        let cancel_bg = cancel.clone();
        let handle = thread::spawn(move || {
            install_model_with(&src, &opts, &cli, None, Some(&cancel_bg), None, |_| {})
        });
        for _ in 0..100 {
            if saw.load(Ordering::SeqCst) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(saw.load(Ordering::SeqCst), "mock never entered download");
        cancel.request_pause();
        let err = handle.join().unwrap().unwrap_err();
        assert!(err.to_string().contains(INSTALL_PAUSED_MSG), "got {}", err);
    }

    #[test]
    fn live_progress_publish_and_snapshot() {
        let live = InstallLiveProgress::new();
        let p = download_progress_event(
            "get a.bin",
            100,
            50,
            Some(1000),
            Some("a.bin".into()),
            Some(0),
            Some(4),
        );
        live.publish(&p);
        let s = live.snapshot();
        assert_eq!(s.bytes_done, Some(150));
        assert_eq!(s.bytes_total, Some(1000));
        assert_eq!(s.file.as_deref(), Some("a.bin"));
        live.set_bytes_done(200);
        assert_eq!(live.snapshot().bytes_done, Some(200));
    }

    /// Live multi-file pull against a tiny public repo (network).
    #[test]
    #[ignore = "live network: HF hub"]
    fn live_hf_snapshot_tiny() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("gpt2-tiny-meta");
        let src = InstallSource::HuggingFace {
            repo_id: "hf-internal-testing/tiny-random-gpt2".into(),
            revision: None,
            allow_patterns: Some(vec!["config.json".into(), "tokenizer.json".into()]),
        };
        let opts = InstallOptions {
            dest: dest.clone(),
            prefer_cli: true,
            min_free_bytes: 0,
            inspect_after: false,
            register: false,
        };
        // Force hub path when CLI missing; still OK if CLI present.
        let result = install_model(&src, &opts, |_| {}).unwrap();
        assert!(result.dest.join("config.json").is_file());
    }

    // --- Retry / backoff contracts (no live network) ---

    #[test]
    fn backoff_schedule_grows_exponentially_and_caps() {
        let policy = InstallRetryPolicy {
            max_attempts: 8,
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(60),
        };
        // failed_attempt 1 → 1s, then 2, 4, 8, 16, 32, 60(cap), 60(cap)
        assert_eq!(backoff_before_retry(&policy, 0), Duration::ZERO);
        assert_eq!(backoff_before_retry(&policy, 1), Duration::from_secs(1));
        assert_eq!(backoff_before_retry(&policy, 2), Duration::from_secs(2));
        assert_eq!(backoff_before_retry(&policy, 3), Duration::from_secs(4));
        assert_eq!(backoff_before_retry(&policy, 4), Duration::from_secs(8));
        assert_eq!(backoff_before_retry(&policy, 5), Duration::from_secs(16));
        assert_eq!(backoff_before_retry(&policy, 6), Duration::from_secs(32));
        assert_eq!(backoff_before_retry(&policy, 7), Duration::from_secs(60));
        assert_eq!(backoff_before_retry(&policy, 8), Duration::from_secs(60));
        // Strictly non-decreasing until cap.
        let mut prev = Duration::ZERO;
        for failed in 1..=8 {
            let d = backoff_before_retry(&policy, failed);
            assert!(d >= prev, "attempt {failed}: {d:?} < {prev:?}");
            assert!(d <= policy.max_backoff);
            prev = d;
        }
    }

    #[test]
    fn backoff_respects_custom_initial_and_cap() {
        let policy = InstallRetryPolicy {
            max_attempts: 5,
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(2),
        };
        assert_eq!(backoff_before_retry(&policy, 1), Duration::from_millis(500));
        assert_eq!(backoff_before_retry(&policy, 2), Duration::from_secs(1));
        assert_eq!(backoff_before_retry(&policy, 3), Duration::from_secs(2)); // would be 2s
        assert_eq!(backoff_before_retry(&policy, 4), Duration::from_secs(2)); // capped
    }

    #[test]
    fn default_retry_policy_matches_constants() {
        let p = InstallRetryPolicy::default();
        assert_eq!(p.max_attempts, INSTALL_DOWNLOAD_MAX_ATTEMPTS);
        assert_eq!(p.initial_backoff, INSTALL_DOWNLOAD_INITIAL_BACKOFF);
        assert_eq!(p.max_backoff, INSTALL_DOWNLOAD_MAX_BACKOFF);
        assert!(p.max_attempts >= 5 && p.max_attempts <= 8);
    }

    #[test]
    fn transient_classifier_matches_operator_body_decode() {
        // Exact class of the production install failure on multi-shard HF repos.
        let msg = "HTTP request error: error decoding response body";
        assert!(
            is_transient_download_error_message(msg),
            "body decode must be retryable"
        );
        assert!(is_transient_download_error_message(
            "hf-hub download_file out-00059.safetensors: HTTP request error: error decoding response body"
        ));
        assert!(is_transient_download_error_message(
            "connection reset by peer"
        ));
        assert!(is_transient_download_error_message("operation timed out"));
        assert!(is_transient_download_error_message("Rate limited"));
        assert!(is_transient_download_error_message(
            "HTTP error: 503 Service Unavailable"
        ));
        assert!(is_transient_download_error_message(
            "HTTP error: 429 Too Many Requests"
        ));
        assert!(is_transient_download_error_message(
            "HTTP error: 502 Bad Gateway"
        ));
    }

    #[test]
    fn permanent_classifier_rejects_auth_and_not_found() {
        assert!(!is_transient_download_error_message(
            "Entry not found: config.json in org/model"
        ));
        assert!(!is_transient_download_error_message(
            "Repository not found: org/missing"
        ));
        assert!(!is_transient_download_error_message(
            "Authentication required (url=https://huggingface.co)"
        ));
        assert!(!is_transient_download_error_message(
            "Forbidden (url=https://huggingface.co)"
        ));
        assert!(!is_transient_download_error_message(
            "HTTP error: 404 Not Found"
        ));
        assert!(!is_transient_download_error_message(
            "HTTP error: 401 Unauthorized"
        ));
        assert!(!is_transient_download_error_message(
            "Invalid parameter: empty filename"
        ));
        assert!(!is_transient_download_error_message(INSTALL_CANCELLED_MSG));
        assert!(!is_transient_download_error_message(INSTALL_PAUSED_MSG));
    }

    #[test]
    fn typed_hf_error_permanent_variants() {
        let e = hf_hub::HFError::EntryNotFound {
            path: "x".into(),
            repo_id: "a/b".into(),
            context: None,
        };
        assert!(!is_transient_hf_error(&e));
        let e = hf_hub::HFError::RepoNotFound {
            repo_id: "a/b".into(),
            context: None,
        };
        assert!(!is_transient_hf_error(&e));
        let e = hf_hub::HFError::LocalEntryNotFound { path: "x".into() };
        assert!(!is_transient_hf_error(&e));
        let e = hf_hub::HFError::InvalidParameter("bad".into());
        assert!(!is_transient_hf_error(&e));
        // Other with body-decode text still retryable (fallback path).
        let e = hf_hub::HFError::Other("HTTP request error: error decoding response body".into());
        assert!(is_transient_hf_error(&e));
        // Cancel text is permanent.
        let e = hf_hub::HFError::Other(format!("install error: {INSTALL_CANCELLED_MSG}"));
        assert!(!is_transient_hf_error(&e));
    }

    #[test]
    fn retry_wrapper_succeeds_after_transient_failures() {
        let policy = InstallRetryPolicy {
            max_attempts: 5,
            initial_backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(10),
        };
        let calls = Arc::new(AtomicBool::new(false));
        let mut n = 0u32;
        let mut sleeps = 0u32;
        let mut retries = 0u32;
        let result = retry_transient(
            &policy,
            |e: &&str| is_transient_download_error_message(e),
            |_attempt| {
                n += 1;
                if n < 3 {
                    Err("HTTP request error: error decoding response body")
                } else {
                    calls.store(true, Ordering::SeqCst);
                    Ok(42i32)
                }
            },
            |_d| {
                sleeps += 1;
            },
            |_attempt, _err, _d| {
                retries += 1;
            },
        );
        assert_eq!(result.unwrap(), 42);
        assert!(calls.load(Ordering::SeqCst));
        assert_eq!(n, 3);
        assert_eq!(retries, 2);
        assert_eq!(sleeps, 2);
    }

    #[test]
    fn retry_wrapper_stops_immediately_on_permanent_error() {
        let policy = InstallRetryPolicy {
            max_attempts: 6,
            initial_backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(10),
        };
        let mut n = 0u32;
        let mut retries = 0u32;
        let result: std::result::Result<(), &str> = retry_transient(
            &policy,
            |e: &&str| is_transient_download_error_message(e),
            |_attempt| {
                n += 1;
                Err("Entry not found: weights.safetensors in org/model")
            },
            |_| {},
            |_, _, _| {
                retries += 1;
            },
        );
        assert!(result.is_err());
        assert_eq!(n, 1, "must not retry permanent 404-class errors");
        assert_eq!(retries, 0);
    }

    #[test]
    fn retry_wrapper_exhausts_and_returns_last_error() {
        let policy = InstallRetryPolicy {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(5),
        };
        let mut n = 0u32;
        let result: std::result::Result<(), &str> = retry_transient(
            &policy,
            |e: &&str| is_transient_download_error_message(e),
            |_attempt| {
                n += 1;
                Err("HTTP request error: error decoding response body")
            },
            |_| {},
            |_, _, _| {},
        );
        assert!(result.is_err());
        assert_eq!(n, 3);
        let msg = format_download_retries_exhausted(
            "out-00059.safetensors",
            policy.max_attempts,
            result.unwrap_err(),
        );
        assert!(msg.contains("out-00059.safetensors"));
        assert!(msg.contains("3 attempts"));
        assert!(msg.contains("decoding response body"));
        assert!(msg.starts_with("hf-hub download_file "));
    }

    #[test]
    fn exhausted_error_message_is_plain_english() {
        let s = format_download_retries_exhausted(
            "out-00059.safetensors",
            6,
            "HTTP request error: error decoding response body",
        );
        assert_eq!(
            s,
            "hf-hub download_file out-00059.safetensors failed after 6 attempts: HTTP request error: error decoding response body"
        );
    }
}
