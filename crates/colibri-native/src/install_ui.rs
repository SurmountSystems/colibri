//! Pure install panel state machine (Pause / Resume / Cancel) + pause checkpoint.
//!
//! No GPUI. Native shell paints buttons and status from [`InstallUiPhase`].
//! Pause is cooperative (wait for current file); mid-file cannot stop early.
//!
//! **UI honesty:** while Pausing / Paused / Cancelling, do not paint active
//! "Downloading..." progress prose. Progress bar may stay at the last known %.
//!
//! **Restart:** a paused job writes [`InstallCheckpoint`] next to native-ui
//! prefs so Resume works after app quit.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Install panel phase for the Tools download form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InstallUiPhase {
    /// No job running; form ready for Install / Resume after prior pause.
    #[default]
    Idle,
    /// Background install in progress.
    Installing,
    /// User asked to pause; waiting for current file to finish.
    Pausing,
    /// Job stopped after pause; completed shards on disk; Resume available.
    Paused,
    /// User asked to cancel; waiting for cooperative stop.
    Cancelling,
}

/// Inputs that move the install UI state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallUiAction {
    /// Start a new install (from Idle or after Done/Error).
    Start,
    /// Resume after [`InstallUiPhase::Paused`] (same form fields).
    Resume,
    /// Request cooperative pause.
    Pause,
    /// Request cooperative cancel.
    Cancel,
    /// Background job finished successfully.
    JobDone,
    /// Background job stopped for pause.
    JobPaused,
    /// Background job stopped for cancel.
    JobCancelled,
    /// Background job failed (non-cancel).
    JobError,
}

/// Apply one action to the current phase. Invalid pairs leave the phase unchanged.
pub fn transition(phase: InstallUiPhase, action: InstallUiAction) -> InstallUiPhase {
    use InstallUiAction::*;
    use InstallUiPhase::*;
    match (phase, action) {
        (Idle, Start) | (Paused, Start) | (Paused, Resume) => Installing,
        (Installing, Pause) => Pausing,
        (Installing, Cancel) | (Pausing, Cancel) => Cancelling,
        // Pause completed (including race where UI was still Installing).
        (Pausing, JobPaused) | (Installing, JobPaused) => Paused,
        // Cancel intent wins if cancel was requested after pause.
        (Cancelling, JobPaused) => Idle,
        (Pausing, JobCancelled) | (Cancelling, JobCancelled) | (Installing, JobCancelled) => Idle,
        (Installing, JobDone) | (Pausing, JobDone) | (Cancelling, JobDone) => Idle,
        (Installing, JobError) | (Pausing, JobError) | (Cancelling, JobError) => Idle,
        _ => phase,
    }
}

/// Whether a background install task is outstanding (poll / block re-start).
pub fn is_busy(phase: InstallUiPhase) -> bool {
    matches!(
        phase,
        InstallUiPhase::Installing | InstallUiPhase::Pausing | InstallUiPhase::Cancelling
    )
}

/// Whether the Pause control should be shown and active.
pub fn show_pause(phase: InstallUiPhase) -> bool {
    matches!(phase, InstallUiPhase::Installing)
}

/// Whether the Resume control should be shown (Paused only).
pub fn show_resume(phase: InstallUiPhase) -> bool {
    matches!(phase, InstallUiPhase::Paused)
}

/// Whether Cancel should look active (job running).
pub fn show_cancel_active(phase: InstallUiPhase) -> bool {
    is_busy(phase)
}

/// Whether the progress strip may paint its active download line (`view.line()`).
///
/// **Exclusive with pause/cancel wait copy:** when false, the strip shows the
/// bar only (last %) and the status line under the form owns the prose.
/// Avoids "Downloading... N% · ETA" next to "Paused...".
pub fn show_active_progress_line(phase: InstallUiPhase) -> bool {
    matches!(phase, InstallUiPhase::Installing | InstallUiPhase::Idle)
}

/// Status line while waiting for cooperative pause (indeterminate wait).
///
/// `tick` advances with the poll timer so the trailing dots pulse.
/// Never includes "Downloading" or an ETA.
pub fn pausing_status_line(tick: u64) -> String {
    let dots = match tick % 3 {
        0 => ".",
        1 => "..",
        _ => "...",
    };
    format!("Pausing{dots} Waiting for current file to finish")
}

/// Status when the job has fully paused.
///
/// `percent` is the last trustworthy fill when known. Never "Downloading".
pub fn paused_status_line(percent: Option<u8>) -> String {
    match percent {
        Some(p) => format!(
            "Paused at {}% · Resume to continue downloading remaining files.",
            p.min(100)
        ),
        None => "Paused · Resume to continue downloading remaining files.".into(),
    }
}

/// Status while waiting for cooperative cancel.
pub fn cancelling_status_line() -> &'static str {
    "Cancelling..."
}

/// Exclusive install status copy for phases that must not show active download prose.
///
/// Returns `None` for Installing / Idle (caller uses progress view line + details).
pub fn exclusive_status_for_phase(
    phase: InstallUiPhase,
    percent: Option<u8>,
    pause_tick: u64,
) -> Option<String> {
    match phase {
        InstallUiPhase::Pausing => Some(pausing_status_line(pause_tick)),
        InstallUiPhase::Paused => Some(paused_status_line(percent)),
        InstallUiPhase::Cancelling => Some(cancelling_status_line().into()),
        InstallUiPhase::Installing | InstallUiPhase::Idle => None,
    }
}

/// True when `line` looks like active download progress (not pause/cancel wait).
///
/// Matches live progress prose only. Paused copy may say "downloading remaining
/// files" as resume guidance; that must not count as active.
#[cfg(test)]
pub fn line_looks_like_active_download(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("downloading...")
        || lower.starts_with("downloading")
        || lower.contains("resuming download")
}

/// Job reported pause; same as [`transition`] with [`InstallUiAction::JobPaused`].
pub fn transition_job_paused(phase: InstallUiPhase) -> InstallUiPhase {
    transition(phase, InstallUiAction::JobPaused)
}

// ---------------------------------------------------------------------------
// Pause checkpoint (survives app restart)
// ---------------------------------------------------------------------------

/// File name under the colibri config directory (next to `native-ui.toml`).
pub const CHECKPOINT_FILE_NAME: &str = "install-checkpoint.toml";

/// Durable form + progress for a paused install (no secrets).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallCheckpoint {
    pub repo_id: String,
    pub revision: String,
    pub dest: String,
    /// Min free GB field as the user typed (e.g. `"50"` or `"0"`).
    #[serde(default)]
    pub min_free_gb: String,
    /// Last known trustworthy percent when paused, if any.
    #[serde(default)]
    pub percent: Option<u8>,
}

impl InstallCheckpoint {
    pub fn new(
        repo_id: impl Into<String>,
        revision: impl Into<String>,
        dest: impl Into<String>,
        min_free_gb: impl Into<String>,
        percent: Option<u8>,
    ) -> Self {
        Self {
            repo_id: repo_id.into(),
            revision: revision.into(),
            dest: dest.into(),
            min_free_gb: min_free_gb.into(),
            percent: percent.map(|p| p.min(100)),
        }
    }

    /// Enough to prefill the form and offer Resume.
    pub fn is_usable(&self) -> bool {
        !self.repo_id.trim().is_empty() && !self.dest.trim().is_empty()
    }
}

/// Platform default checkpoint path (same config dir as native-ui.toml).
pub fn default_checkpoint_path() -> PathBuf {
    match crate::prefs::default_prefs_path().parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(CHECKPOINT_FILE_NAME),
        _ => PathBuf::from(CHECKPOINT_FILE_NAME),
    }
}

/// Write checkpoint TOML (creates parent dirs).
pub fn save_checkpoint(path: &Path, cp: &InstallCheckpoint) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let text = toml::to_string_pretty(cp)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    fs::write(path, text)
}

/// Load a usable checkpoint, or `None` when missing / corrupt / unusable.
pub fn load_checkpoint(path: &Path) -> Option<InstallCheckpoint> {
    let text = fs::read_to_string(path).ok()?;
    let cp: InstallCheckpoint = toml::from_str(&text).ok()?;
    if cp.is_usable() { Some(cp) } else { None }
}

/// Remove checkpoint file if present (Done / Cancel / clear).
pub fn clear_checkpoint(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Save to the platform default path (best effort for hosts).
pub fn save_checkpoint_default(cp: &InstallCheckpoint) -> std::io::Result<()> {
    save_checkpoint(&default_checkpoint_path(), cp)
}

/// Load from the platform default path.
pub fn load_checkpoint_default() -> Option<InstallCheckpoint> {
    load_checkpoint(&default_checkpoint_path())
}

/// Clear the platform default checkpoint.
pub fn clear_checkpoint_default() -> std::io::Result<()> {
    clear_checkpoint(&default_checkpoint_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_to_pause_to_resume() {
        let mut p = InstallUiPhase::Idle;
        p = transition(p, InstallUiAction::Start);
        assert_eq!(p, InstallUiPhase::Installing);
        assert!(is_busy(p));
        assert!(show_pause(p));
        p = transition(p, InstallUiAction::Pause);
        assert_eq!(p, InstallUiPhase::Pausing);
        assert!(is_busy(p));
        assert!(!show_pause(p));
        p = transition_job_paused(p);
        assert_eq!(p, InstallUiPhase::Paused);
        assert!(!is_busy(p));
        assert!(show_resume(p));
        p = transition(p, InstallUiAction::Resume);
        assert_eq!(p, InstallUiPhase::Installing);
    }

    #[test]
    fn install_to_cancel() {
        let mut p = transition(InstallUiPhase::Idle, InstallUiAction::Start);
        p = transition(p, InstallUiAction::Cancel);
        assert_eq!(p, InstallUiPhase::Cancelling);
        p = transition(p, InstallUiAction::JobCancelled);
        assert_eq!(p, InstallUiPhase::Idle);
        assert!(!show_resume(p));
    }

    #[test]
    fn pause_then_cancel_while_waiting() {
        let mut p = transition(InstallUiPhase::Idle, InstallUiAction::Start);
        p = transition(p, InstallUiAction::Pause);
        assert_eq!(p, InstallUiPhase::Pausing);
        p = transition(p, InstallUiAction::Cancel);
        assert_eq!(p, InstallUiPhase::Cancelling);
        p = transition(p, InstallUiAction::JobCancelled);
        assert_eq!(p, InstallUiPhase::Idle);
    }

    #[test]
    fn job_done_from_installing() {
        let p = transition(InstallUiPhase::Installing, InstallUiAction::JobDone);
        assert_eq!(p, InstallUiPhase::Idle);
    }

    #[test]
    fn job_paused_while_installing_lands_paused() {
        // Defensive: event arrives before UI flipped to Pausing.
        assert_eq!(
            transition_job_paused(InstallUiPhase::Installing),
            InstallUiPhase::Paused
        );
    }

    #[test]
    fn pausing_status_pulses_dots() {
        let a = pausing_status_line(0);
        let b = pausing_status_line(1);
        let c = pausing_status_line(2);
        assert!(a.contains("Pausing."), "{a}");
        assert!(b.contains("Pausing.."), "{b}");
        assert!(c.contains("Pausing..."), "{c}");
        assert!(a.contains("Waiting for current file to finish"));
        assert_ne!(a, b);
        assert!(
            !line_looks_like_active_download(&a),
            "pausing must not look like download: {a}"
        );
    }

    #[test]
    fn invalid_actions_are_noops() {
        assert_eq!(
            transition(InstallUiPhase::Idle, InstallUiAction::Pause),
            InstallUiPhase::Idle
        );
        assert_eq!(
            transition(InstallUiPhase::Paused, InstallUiAction::Pause),
            InstallUiPhase::Paused
        );
        assert_eq!(
            transition(InstallUiPhase::Cancelling, InstallUiAction::Start),
            InstallUiPhase::Cancelling
        );
    }

    #[test]
    fn resume_from_paused_only() {
        assert!(!show_resume(InstallUiPhase::Idle));
        assert!(!show_resume(InstallUiPhase::Installing));
        assert!(show_resume(InstallUiPhase::Paused));
    }

    // --- Exclusive status (no dual Downloading + Paused) --------------------

    #[test]
    fn show_active_progress_line_only_while_installing_or_idle() {
        assert!(show_active_progress_line(InstallUiPhase::Installing));
        assert!(show_active_progress_line(InstallUiPhase::Idle));
        assert!(!show_active_progress_line(InstallUiPhase::Pausing));
        assert!(!show_active_progress_line(InstallUiPhase::Paused));
        assert!(!show_active_progress_line(InstallUiPhase::Cancelling));
    }

    #[test]
    fn paused_status_never_says_downloading() {
        let with_pct = paused_status_line(Some(16));
        assert!(with_pct.contains("Paused"), "{with_pct}");
        assert!(with_pct.contains("16%"), "{with_pct}");
        assert!(
            !line_looks_like_active_download(&with_pct),
            "must not claim active download: {with_pct}"
        );
        // "downloading remaining files" is resume guidance, not "Downloading..."
        assert!(
            !with_pct.to_ascii_lowercase().contains("downloading..."),
            "{with_pct}"
        );

        let no_pct = paused_status_line(None);
        assert!(no_pct.contains("Paused"), "{no_pct}");
        assert!(!no_pct.contains('%'), "{no_pct}");
        assert!(!line_looks_like_active_download(&no_pct), "{no_pct}");
    }

    #[test]
    fn exclusive_status_paused_vs_installing() {
        assert!(
            exclusive_status_for_phase(InstallUiPhase::Installing, Some(16), 0).is_none(),
            "installing uses progress view line, not exclusive pause copy"
        );
        let paused =
            exclusive_status_for_phase(InstallUiPhase::Paused, Some(16), 0).expect("paused line");
        assert!(paused.contains("Paused at 16%"), "{paused}");
        assert!(!line_looks_like_active_download(&paused), "{paused}");

        let pausing =
            exclusive_status_for_phase(InstallUiPhase::Pausing, Some(16), 1).expect("pausing");
        assert!(pausing.contains("Pausing"), "{pausing}");
        assert!(!line_looks_like_active_download(&pausing), "{pausing}");
    }

    #[test]
    fn active_download_line_detected() {
        assert!(line_looks_like_active_download(
            "Downloading... 16% · about 2 hours left"
        ));
        assert!(line_looks_like_active_download("Resuming download..."));
        assert!(!line_looks_like_active_download(
            "Paused at 16% · Resume to continue downloading remaining files."
        ));
    }

    // --- Checkpoint load / save / clear -------------------------------------

    #[test]
    fn checkpoint_round_trip_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CHECKPOINT_FILE_NAME);
        let original = InstallCheckpoint::new(
            "openai/gpt-oss-20b",
            "main",
            "/models/gpt-oss-20b",
            "50",
            Some(16),
        );
        save_checkpoint(&path, &original).unwrap();
        assert!(path.is_file());
        let loaded = load_checkpoint(&path).expect("load");
        assert_eq!(loaded, original);
        assert!(loaded.is_usable());
    }

    #[test]
    fn checkpoint_missing_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no-checkpoint.toml");
        assert!(load_checkpoint(&path).is_none());
    }

    #[test]
    fn checkpoint_corrupt_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CHECKPOINT_FILE_NAME);
        fs::write(&path, "not { valid [[[ toml").unwrap();
        assert!(load_checkpoint(&path).is_none());
    }

    #[test]
    fn checkpoint_empty_repo_unusable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CHECKPOINT_FILE_NAME);
        let bad = InstallCheckpoint::new("", "main", "/dest", "0", None);
        assert!(!bad.is_usable());
        save_checkpoint(&path, &bad).unwrap();
        assert!(
            load_checkpoint(&path).is_none(),
            "empty repo must not restore Resume"
        );
    }

    #[test]
    fn clear_checkpoint_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CHECKPOINT_FILE_NAME);
        let cp = InstallCheckpoint::new("r/m", "main", "/d", "0", Some(5));
        save_checkpoint(&path, &cp).unwrap();
        assert!(path.is_file());
        clear_checkpoint(&path).unwrap();
        assert!(!path.exists());
        // Idempotent when already gone.
        clear_checkpoint(&path).unwrap();
        assert!(load_checkpoint(&path).is_none());
    }

    #[test]
    fn default_checkpoint_path_next_to_prefs_dir() {
        let prefs = crate::prefs::default_prefs_path();
        let cp = default_checkpoint_path();
        assert_eq!(prefs.parent(), cp.parent());
        assert!(cp.ends_with(CHECKPOINT_FILE_NAME), "{}", cp.display());
    }
}
