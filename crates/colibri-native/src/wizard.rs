//! Setup wizard step machine for colibri-native.
//!
//! Pure state (no GPUI). DesktopApp owns a [`WizardState`] and renders steps
//! with Back / Next / Skip / Finish. Skip and Finish both mark first-run done
//! via [`complete_wizard`].

use crate::prefs::{NativePrefs, ThemePref};

/// Ordered wizard steps (1-based product copy: Welcome … Ready).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WizardStep {
    #[default]
    Welcome,
    Machine,
    Model,
    Readiness,
    LookAndFeel,
    Ready,
}

impl WizardStep {
    pub const ALL: [WizardStep; 6] = [
        WizardStep::Welcome,
        WizardStep::Machine,
        WizardStep::Model,
        WizardStep::Readiness,
        WizardStep::LookAndFeel,
        WizardStep::Ready,
    ];

    /// 0-based index into [`WizardStep::ALL`].
    pub fn index(self) -> usize {
        match self {
            WizardStep::Welcome => 0,
            WizardStep::Machine => 1,
            WizardStep::Model => 2,
            WizardStep::Readiness => 3,
            WizardStep::LookAndFeel => 4,
            WizardStep::Ready => 5,
        }
    }

    /// Human step number (1..=6).
    pub fn number(self) -> usize {
        self.index() + 1
    }

    pub fn total() -> usize {
        Self::ALL.len()
    }

    pub fn from_index(i: usize) -> Option<Self> {
        Self::ALL.get(i).copied()
    }

    pub fn next(self) -> Option<Self> {
        Self::from_index(self.index() + 1)
    }

    pub fn prev(self) -> Option<Self> {
        self.index().checked_sub(1).and_then(Self::from_index)
    }

    pub fn is_first(self) -> bool {
        self.index() == 0
    }

    pub fn is_last(self) -> bool {
        self.index() + 1 == Self::total()
    }

    /// i18n key for the step title (`wizard.welcome.title`, …).
    pub fn title_key(self) -> &'static str {
        match self {
            WizardStep::Welcome => "wizard.welcome.title",
            WizardStep::Machine => "wizard.machine.title",
            WizardStep::Model => "wizard.model.title",
            WizardStep::Readiness => "wizard.readiness.title",
            WizardStep::LookAndFeel => "wizard.look.title",
            WizardStep::Ready => "wizard.ready.title",
        }
    }

    /// i18n key for the step body.
    pub fn body_key(self) -> &'static str {
        match self {
            WizardStep::Welcome => "wizard.welcome.body",
            WizardStep::Machine => "wizard.machine.body",
            WizardStep::Model => "wizard.model.body",
            WizardStep::Readiness => "wizard.readiness.body",
            WizardStep::LookAndFeel => "wizard.look.body",
            WizardStep::Ready => "wizard.ready.body",
        }
    }
}

/// Open/closed + current step for the setup wizard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardState {
    pub open: bool,
    pub step: WizardStep,
    /// Step 3: show optional download / install form.
    pub show_download: bool,
}

impl Default for WizardState {
    fn default() -> Self {
        Self::closed()
    }
}

impl WizardState {
    pub fn closed() -> Self {
        Self {
            open: false,
            step: WizardStep::Welcome,
            show_download: false,
        }
    }

    /// Open at Welcome (first-run or Setup button).
    pub fn open_at_start() -> Self {
        Self {
            open: true,
            step: WizardStep::Welcome,
            show_download: false,
        }
    }

    pub fn close(&mut self) {
        self.open = false;
        self.step = WizardStep::Welcome;
        self.show_download = false;
    }

    /// Advance one step. Returns false when already on the last step.
    pub fn advance(&mut self) -> bool {
        if let Some(n) = self.step.next() {
            self.step = n;
            true
        } else {
            false
        }
    }

    /// Go back one step. Returns false on the first step.
    pub fn back(&mut self) -> bool {
        if let Some(p) = self.step.prev() {
            self.step = p;
            true
        } else {
            false
        }
    }

    pub fn toggle_download(&mut self) {
        self.show_download = !self.show_download;
    }
}

/// Mark first-run complete and close the wizard (Skip or Finish).
pub fn complete_wizard(prefs: &mut NativePrefs, wizard: &mut WizardState) {
    prefs.first_run_done = true;
    wizard.close();
}

// ---------------------------------------------------------------------------
// Layout helpers (pure; paint uses theme tokens for the same numbers)
// ---------------------------------------------------------------------------

/// Default max height for the supported-models catalog list viewport (px).
///
/// Matches [`crate::theme::WIZARD_CATALOG_LIST_MAX_H`]. Kept here so pure tests
/// do not need GPUI/theme paint paths.
pub const CATALOG_LIST_MAX_H: f32 = 168.0;

/// Default max height for scanned registry lists inside wizard steps (px).
pub const REGISTRY_LIST_MAX_H: f32 = 112.0;

/// Approximate row height for catalog / registry density (px).
pub const LIST_ROW_H: f32 = 28.0;

/// Whether a list of `row_count` rows (each ~`row_h` tall) exceeds `max_h`.
///
/// When true, the paint path must put the rows in a max-height scroll viewport
/// so the list does not push the wizard footer off-screen.
pub fn list_exceeds_max_height(row_count: usize, row_h: f32, max_h: f32) -> bool {
    if row_count == 0 || row_h <= 0.0 || max_h < 0.0 {
        return false;
    }
    (row_count as f32) * row_h > max_h
}

/// Success status after Skip (`finish = false`) or Finish (`finish = true`).
///
/// Call only when prefs save succeeded. When save fails, keep the save-error
/// status already set by the persist path (do not overwrite with success).
pub fn wizard_complete_success_status(finish: bool) -> &'static str {
    if finish {
        "Setup complete"
    } else {
        "Setup skipped · you can open Setup anytime"
    }
}

/// Whether Skip/Finish may replace status with the success string.
///
/// `false` when save failed: UI must keep `"Could not save settings: …"`.
pub fn wizard_may_set_success_status(save_ok: bool) -> bool {
    save_ok
}

/// Snapshot shell prefs fields for save (no env re-apply).
pub fn shell_prefs_snapshot(
    first_run_done: bool,
    theme: ThemePref,
    locale: crate::prefs::LocalePref,
    last_model_path: impl Into<String>,
) -> NativePrefs {
    NativePrefs {
        version: crate::prefs::PREFS_VERSION,
        first_run_done,
        theme,
        locale,
        last_model_path: last_model_path.into(),
    }
}

/// Apply theme on a prefs snapshot (product + tests).
pub fn apply_theme(prefs: &mut NativePrefs, theme: ThemePref) {
    prefs.theme = theme;
}

// ---------------------------------------------------------------------------
// Doctor step (Readiness) button → action map
//
// Same ids the GPUI view uses. Handlers must dispatch via this map so a dead
// wire-up fails unit tests before it fails the operator.
// ---------------------------------------------------------------------------

/// Element ids for wizard Doctor-step action buttons (must match `main.rs`).
pub const WIZARD_BTN_DOCTOR: &str = "wizard-btn-doctor";
pub const WIZARD_BTN_QUICK_CHECK: &str = "wizard-btn-readiness-refresh";
pub const WIZARD_BTN_SCAN: &str = "wizard-btn-readiness-scan";
pub const WIZARD_BTN_INSTALL: &str = "wizard-btn-readiness-install";

/// Actions the Doctor step buttons can fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardReadinessAction {
    /// Thorough doctor + recovery (mkdir / scan when needed).
    RunDoctor,
    /// Shallow doctor + recovery, then memory plan.
    QuickCheck,
    /// Scan default store, then recovery doctor + plan.
    ScanModels,
    /// Jump to Model step with install form open.
    InstallModel,
}

/// Map a GPUI button element id to a Doctor-step action.
///
/// Returns `None` for unknown ids (wiring regression surface).
pub fn readiness_action_for_button_id(id: &str) -> Option<WizardReadinessAction> {
    match id {
        WIZARD_BTN_DOCTOR => Some(WizardReadinessAction::RunDoctor),
        WIZARD_BTN_QUICK_CHECK => Some(WizardReadinessAction::QuickCheck),
        WIZARD_BTN_SCAN => Some(WizardReadinessAction::ScanModels),
        WIZARD_BTN_INSTALL => Some(WizardReadinessAction::InstallModel),
        _ => None,
    }
}

/// Immediate status while work runs (status rail must change on click).
pub fn readiness_running_status(action: WizardReadinessAction) -> &'static str {
    match action {
        WizardReadinessAction::RunDoctor => "Running doctor...",
        WizardReadinessAction::QuickCheck => "Quick check...",
        WizardReadinessAction::ScanModels => "Scanning...",
        WizardReadinessAction::InstallModel => "Opening install...",
    }
}

/// Status after the action completes. Always includes a clock so a no-op-looking
/// checklist still proves the click landed.
pub fn readiness_done_status(action: WizardReadinessAction, clock: &str) -> String {
    match action {
        WizardReadinessAction::RunDoctor => format!("Doctor finished · Last run {clock}"),
        WizardReadinessAction::QuickCheck => format!("Quick check finished · Last run {clock}"),
        WizardReadinessAction::ScanModels => format!("Scan finished · Last run {clock}"),
        WizardReadinessAction::InstallModel => "Install a model into the default store".into(),
    }
}

/// Marker line appended to the Health check body after a click.
pub const DOCTOR_LAST_RUN_PREFIX: &str = "Last run: ";

/// Strip any prior `Last run: …` footer lines (idempotent stamp).
pub fn strip_doctor_last_run(body: &str) -> String {
    let mut lines: Vec<&str> = body.lines().collect();
    while lines
        .last()
        .is_some_and(|l| l.trim().is_empty() || l.trim_start().starts_with(DOCTOR_LAST_RUN_PREFIX))
    {
        lines.pop();
    }
    lines.join("\n")
}

/// Append `Last run: {clock}` so the Health check box always changes on click.
pub fn stamp_doctor_last_run(body: &str, clock: &str) -> String {
    let base = strip_doctor_last_run(body);
    let base = base.trim_end();
    if base.is_empty() {
        format!("{DOCTOR_LAST_RUN_PREFIX}{clock}")
    } else {
        format!("{base}\n{DOCTOR_LAST_RUN_PREFIX}{clock}")
    }
}

/// Local wall-clock `HH:MM:SS` for status / Last run (pure formatting of unix secs).
pub fn format_readiness_clock_secs(epoch_secs: u64) -> String {
    // Local time via chrono-free math on UTC offset is host-dependent; use
    // a simple UTC clock so tests are hermetic. UI still reads as a clock.
    let secs = epoch_secs % 86_400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

/// Current clock string for Doctor-step feedback (wall time when available).
pub fn readiness_clock_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_readiness_clock_secs(secs)
}

/// Pure outcome of a Doctor-step click after host work finished.
///
/// Used by unit tests and by the UI so status / stamp logic cannot drift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessClickOutcome {
    pub status: String,
    pub doctor_text: String,
    pub plan_text: Option<String>,
}

/// Build the post-click UI strings from host results.
///
/// `doctor_body` / `plan_body` are the host outputs **before** last-run stamp.
/// Plan is only returned for QuickCheck and ScanModels.
pub fn readiness_click_outcome(
    action: WizardReadinessAction,
    doctor_body: &str,
    plan_body: Option<&str>,
    clock: &str,
) -> ReadinessClickOutcome {
    let doctor_text = match action {
        WizardReadinessAction::InstallModel => doctor_body.to_string(),
        _ => stamp_doctor_last_run(doctor_body, clock),
    };
    let plan_text = match action {
        WizardReadinessAction::QuickCheck | WizardReadinessAction::ScanModels => {
            plan_body.map(|s| s.to_string())
        }
        _ => None,
    };
    ReadinessClickOutcome {
        status: readiness_done_status(action, clock),
        doctor_text,
        plan_text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::{LocalePref, PREFS_VERSION, ThemePref, load_from_path};
    use std::path::Path;

    /// Load → set theme → save (temp-dir contract for Tools theme switch).
    fn save_theme_to_prefs_path(path: &Path, theme: ThemePref) -> std::io::Result<()> {
        let mut prefs = load_from_path(path);
        apply_theme(&mut prefs, theme);
        prefs.save_to_path(path)
    }

    #[test]
    fn steps_are_linear_and_count_six() {
        assert_eq!(WizardStep::total(), 6);
        assert_eq!(WizardStep::Welcome.number(), 1);
        assert_eq!(WizardStep::Ready.number(), 6);
        assert!(WizardStep::Welcome.is_first());
        assert!(WizardStep::Ready.is_last());
        assert_eq!(WizardStep::Welcome.next(), Some(WizardStep::Machine));
        assert_eq!(WizardStep::Ready.next(), None);
        assert_eq!(WizardStep::Welcome.prev(), None);
        assert_eq!(WizardStep::Machine.prev(), Some(WizardStep::Welcome));
    }

    #[test]
    fn advance_walks_all_steps() {
        let mut w = WizardState::open_at_start();
        assert!(w.open);
        assert_eq!(w.step, WizardStep::Welcome);
        for expected in [
            WizardStep::Machine,
            WizardStep::Model,
            WizardStep::Readiness,
            WizardStep::LookAndFeel,
            WizardStep::Ready,
        ] {
            assert!(w.advance());
            assert_eq!(w.step, expected);
        }
        assert!(!w.advance());
        assert_eq!(w.step, WizardStep::Ready);
    }

    #[test]
    fn back_from_middle() {
        let mut w = WizardState::open_at_start();
        w.advance();
        w.advance();
        assert_eq!(w.step, WizardStep::Model);
        assert!(w.back());
        assert_eq!(w.step, WizardStep::Machine);
    }

    #[test]
    fn skip_sets_first_run_done_and_closes() {
        let mut prefs = NativePrefs::default();
        assert!(!prefs.first_run_done);
        let mut w = WizardState::open_at_start();
        w.advance();
        complete_wizard(&mut prefs, &mut w);
        assert!(prefs.first_run_done);
        assert!(!w.open);
        assert_eq!(w.step, WizardStep::Welcome);
    }

    #[test]
    fn finish_same_as_skip_for_first_run_flag() {
        let mut prefs = NativePrefs::default();
        let mut w = WizardState::open_at_start();
        while w.advance() {}
        complete_wizard(&mut prefs, &mut w);
        assert!(prefs.first_run_done);
        assert!(!w.open);
    }

    #[test]
    fn theme_switch_saves_prefs_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("native-ui.toml");
        let initial = NativePrefs {
            version: PREFS_VERSION,
            first_run_done: true,
            theme: ThemePref::Doge,
            locale: LocalePref::En,
            last_model_path: String::new(),
        };
        initial.save_to_path(&path).unwrap();

        save_theme_to_prefs_path(&path, ThemePref::Mint).unwrap();
        let loaded = load_from_path(&path);
        assert_eq!(loaded.theme, ThemePref::Mint);
        assert!(loaded.first_run_done);

        save_theme_to_prefs_path(&path, ThemePref::Doge).unwrap();
        let loaded = load_from_path(&path);
        assert_eq!(loaded.theme, ThemePref::Doge);
    }

    #[test]
    fn shell_prefs_snapshot_fields() {
        let p = shell_prefs_snapshot(true, ThemePref::Mint, LocalePref::It, "/m");
        assert!(p.first_run_done);
        assert_eq!(p.theme, ThemePref::Mint);
        assert_eq!(p.locale, LocalePref::It);
        assert_eq!(p.last_model_path, "/m");
    }

    #[test]
    fn wizard_complete_status_preserves_save_error() {
        assert!(!wizard_may_set_success_status(false));
        assert!(wizard_may_set_success_status(true));
        assert_eq!(
            wizard_complete_success_status(false),
            "Setup skipped · you can open Setup anytime"
        );
        assert_eq!(wizard_complete_success_status(true), "Setup complete");
    }

    #[test]
    fn complete_wizard_then_save_round_trips_first_run() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("native-ui.toml");
        let mut prefs = NativePrefs::default();
        let mut w = WizardState::open_at_start();
        complete_wizard(&mut prefs, &mut w);
        prefs.save_to_path(&path).unwrap();
        let loaded = load_from_path(&path);
        assert!(loaded.first_run_done);
        assert!(!w.open);
        assert!(!loaded.should_show_wizard());
    }

    #[test]
    fn title_keys_are_stable() {
        assert_eq!(WizardStep::Readiness.title_key(), "wizard.readiness.title");
        assert_eq!(WizardStep::LookAndFeel.body_key(), "wizard.look.body");
    }

    #[test]
    fn readiness_button_ids_map_to_actions() {
        // Same ids the UI attaches; disconnecting a handler without updating
        // this map (or vice versa) is a regression.
        assert_eq!(
            readiness_action_for_button_id(WIZARD_BTN_DOCTOR),
            Some(WizardReadinessAction::RunDoctor)
        );
        assert_eq!(
            readiness_action_for_button_id(WIZARD_BTN_QUICK_CHECK),
            Some(WizardReadinessAction::QuickCheck)
        );
        assert_eq!(
            readiness_action_for_button_id(WIZARD_BTN_SCAN),
            Some(WizardReadinessAction::ScanModels)
        );
        assert_eq!(
            readiness_action_for_button_id(WIZARD_BTN_INSTALL),
            Some(WizardReadinessAction::InstallModel)
        );
        assert_eq!(readiness_action_for_button_id("wizard-btn-next"), None);
        assert_eq!(readiness_action_for_button_id(""), None);
    }

    #[test]
    fn readiness_running_status_is_visible() {
        assert_eq!(
            readiness_running_status(WizardReadinessAction::RunDoctor),
            "Running doctor..."
        );
        assert_eq!(
            readiness_running_status(WizardReadinessAction::QuickCheck),
            "Quick check..."
        );
        assert_eq!(
            readiness_running_status(WizardReadinessAction::ScanModels),
            "Scanning..."
        );
    }

    #[test]
    fn stamp_doctor_last_run_replaces_prior_footer() {
        let body = "Overall: Needs model\nPath: /m\n";
        let once = stamp_doctor_last_run(body, "12:00:01");
        assert!(once.contains("Overall: Needs model"));
        assert!(once.ends_with("Last run: 12:00:01"));
        let twice = stamp_doctor_last_run(&once, "12:00:09");
        assert_eq!(twice.matches("Last run:").count(), 1);
        assert!(twice.ends_with("Last run: 12:00:09"));
        assert!(!twice.contains("12:00:01"));
    }

    #[test]
    fn readiness_click_outcome_updates_status_and_doctor() {
        let body = "Overall: Needs model\nPath: /tmp/x\n";
        let out = readiness_click_outcome(WizardReadinessAction::RunDoctor, body, None, "15:04:05");
        assert_eq!(out.status, "Doctor finished · Last run 15:04:05");
        assert!(out.doctor_text.contains("Overall: Needs model"));
        assert!(out.doctor_text.contains("Last run: 15:04:05"));
        assert!(out.plan_text.is_none());

        let quick = readiness_click_outcome(
            WizardReadinessAction::QuickCheck,
            body,
            Some("No memory plan yet."),
            "15:04:06",
        );
        assert_eq!(quick.status, "Quick check finished · Last run 15:04:06");
        assert_eq!(quick.plan_text.as_deref(), Some("No memory plan yet."));
        // Must not bury doctor under bare "Plan finished".
        assert!(!quick.status.contains("Plan finished"));
    }

    #[test]
    fn format_readiness_clock_secs_is_hhmmss() {
        assert_eq!(format_readiness_clock_secs(0), "00:00:00");
        assert_eq!(format_readiness_clock_secs(3661), "01:01:01");
        assert_eq!(format_readiness_clock_secs(86_400 + 5), "00:00:05");
    }

    #[test]
    fn catalog_list_max_h_matches_theme_token() {
        // Pin so paint (theme) and pure helper stay in lockstep.
        assert_eq!(CATALOG_LIST_MAX_H, crate::theme::WIZARD_CATALOG_LIST_MAX_H);
        assert_eq!(
            REGISTRY_LIST_MAX_H,
            crate::theme::WIZARD_REGISTRY_LIST_MAX_H
        );
        assert_eq!(LIST_ROW_H, crate::theme::WIZARD_LIST_ROW_H);
    }

    #[test]
    fn catalog_row_count_triggers_scroll_path() {
        let max_h = CATALOG_LIST_MAX_H;
        let row_h = LIST_ROW_H;
        // Empty / few rows fit without scroll.
        assert!(!list_exceeds_max_height(0, row_h, max_h));
        assert!(!list_exceeds_max_height(1, row_h, max_h));
        // Product catalog is 5 models today (~140px) — under the cap.
        assert!(!list_exceeds_max_height(5, row_h, max_h));
        // Exact fit is not "exceeds".
        let exact = (max_h / row_h).floor() as usize;
        assert!(!list_exceeds_max_height(exact, row_h, max_h));
        // One more row forces the scroll viewport path.
        assert!(list_exceeds_max_height(exact + 1, row_h, max_h));
        // Many rows always need scroll.
        assert!(list_exceeds_max_height(20, row_h, max_h));
    }

    #[test]
    fn registry_list_scroll_threshold() {
        let max_h = REGISTRY_LIST_MAX_H;
        let row_h = LIST_ROW_H;
        assert!(!list_exceeds_max_height(3, row_h, max_h));
        assert!(list_exceeds_max_height(10, row_h, max_h));
    }
}
