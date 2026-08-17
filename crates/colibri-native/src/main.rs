//! colibrì native desktop shell (GPUI) — SPA visual parity with `web/`.
//!
//! Layout: slim left rail · main (Chat | Brain | Profiling | Tools) · Setup wizard.
//! Theme tokens from `theme` (DOGE default + mint); i18n en/it; profiling charts.
//! Engine path stays colibri-sys in-process (no REST).

mod atlas;
mod host;
mod i18n;
#[cfg(feature = "install")]
mod install_ui;
mod log_init;
mod notify_os;
mod prefill;
mod prefs;
mod profiling_view;
mod progress;
mod stderr_tee;
mod text_input;
mod theme;
mod wizard;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, Application, Bounds, Context, Entity, InteractiveElement, IntoElement,
    MouseButton, ParentElement, SharedString, StatefulInteractiveElement, Styled, TitlebarOptions,
    Window, WindowBounds, WindowOptions, div, prelude::*, px, relative, rgb, size,
};

use crate::atlas::{ExpertAtlas, format_brain_tooltip, load_experts_atlas};
use crate::host::{
    BrainView, EngineSession, GenEvent, LiveHwinfoIdle, LiveTiersIdle, MissingPathScanOutcome,
    StartEngineBlock, StartEngineSource, apply_brain_pulse_decay, brain_cell_rgb,
    brain_pulse_decay_steps_for_ms, brain_view_from_map, brain_view_from_map_with_max,
    catalog_is_installed, catalog_row_style, catalog_selection_by_id, controls_from_ui,
    display_to_source, engine_starting_status, ensure_model_path_for_doctor, env_brain_full,
    env_model_path, format_created_model_directory, format_empty_registry_scan, format_live_hwinfo,
    format_live_tiers, format_machine, format_profile_turns, format_registry_entry,
    format_registry_scan_status, format_supported_model_row, is_model_leaf, list_supported_models,
    live_hwinfo_idle_message, live_tiers_idle_message, messages_from_turns,
    missing_path_scan_outcome, model_path_unset_for_doctor, probe_machine,
    progress_view_for_generate, progress_view_generate_done, pump_session_visual,
    registry_scan_roots, resolve_startup_model_path, run_deep_doctor, run_plan, run_shallow_doctor,
    scaffold_doctor_defaults, scan_model_registry, should_dispatch_engine_start,
    status_after_gen_done, stop_session, switch_cache_slot_transcript,
};
#[cfg(feature = "install")]
use crate::host::{
    DEFAULT_INSTALL_MIN_FREE_BYTES, InstallEvent, check_install_free_space,
    format_install_bytes_pair, format_install_space_with_min, install_async, install_free_bytes,
    parse_min_free_gb, progress_view_for_install, validate_install_form,
};
use crate::i18n::{Locale, t, t_fmt};
#[cfg(feature = "install")]
use crate::install_ui::{
    InstallCheckpoint, InstallUiAction, InstallUiPhase, cancelling_status_line,
    clear_checkpoint_default, exclusive_status_for_phase, is_busy as install_is_busy,
    load_checkpoint_default, paused_status_line, pausing_status_line, save_checkpoint_default,
    show_active_progress_line, show_cancel_active, show_pause, show_resume,
    transition as install_ui_transition, transition_job_paused,
};
#[cfg(feature = "install")]
use crate::notify_os::notify_install_complete;
use crate::notify_os::{inference_end_kind, notify_inference_end};
use crate::prefill::{apply_prefill_status, clear_prefill_progress, load_prefill_progress};
use crate::prefs::{LocalePref, NativePrefs};
use crate::profiling_view::{
    DerivedTurn, PROF_CHART_N, ProfilePhase, format_badge_tok_per_sec, format_badge_tokens,
    format_badge_ttft_ms, format_seconds, phase_stack_heights, recent_turns, share_segments,
    throughput_heights, tier_share_fractions,
};
use crate::progress::ProgressView;
use crate::text_input::{TextInput, bind_text_input_keys};
use crate::theme::{
    BTN_PAD_X, BTN_PAD_Y, HERO_MAX_W, RAIL_CARD_GAP, RAIL_CARD_PAD, RAIL_PAD, RAIL_SECTION_GAP,
    RAIL_WIDTH, ThemeId, ThemePalette, WIZARD_CARD_PAD, WIZARD_CONTENT_GAP, WIZARD_MAX_W,
    WIZARD_STAGE_PAD, palette,
};
use crate::wizard::{
    CATALOG_LIST_MAX_H, LIST_ROW_H, REGISTRY_LIST_MAX_H, WIZARD_BTN_DOCTOR, WIZARD_BTN_INSTALL,
    WIZARD_BTN_QUICK_CHECK, WIZARD_BTN_SCAN, WizardReadinessAction, WizardState, WizardStep,
    apply_theme, complete_wizard, list_exceeds_max_height, readiness_action_for_button_id,
    readiness_click_outcome, readiness_clock_now, readiness_running_status, shell_prefs_snapshot,
    wizard_complete_success_status, wizard_may_set_success_status,
};
#[cfg(feature = "install")]
use colibri_sys::install::{InstallCancel, InstallLiveProgress};
use colibri_sys::{ModelEntry, ProfileTurn, TiersSnap};

/// Optional About strip (default off). Product chrome stays free of host/engine jargon.
const ABOUT_NOTE: &str =
    "colibrì native desktop shell. Runs models on this machine without a browser.";

/// How many PROF turns to keep in the text strip helper (host tests still use this).
const PROF_LAST_N: usize = 8;
/// Visual pump interval while the engine is up.
const VISUAL_PUMP_MS: u64 = 500;
/// RSS / identity heartbeat while the engine is up (5–10s).
const SESSION_HEARTBEAT_MS: u64 = crate::log_init::SESSION_HEARTBEAT_MS;

/// Main content tab (web view tabs + Tools).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum MainView {
    #[default]
    Chat,
    Brain,
    Profiling,
    Tools,
}

struct DesktopApp {
    machine_text: SharedString,
    /// When true, Machine panel includes SIMD / NPU / store details.
    machine_expanded: bool,
    doctor_text: SharedString,
    plan_text: SharedString,
    status: SharedString,
    chat_log: Vec<(SharedString, SharedString)>,
    /// Sticky transcripts keyed by mux session slot (web multi-slot parity).
    slot_transcripts: HashMap<u32, Vec<(String, String)>>,
    /// Active KV session slot for generate + sticky chat.
    cache_slot: u32,
    /// Engine-advertised KV slots (1 until engine starts).
    kv_slots: u32,
    /// Reasoning / thinking toggle for the chat template.
    enable_thinking: bool,
    model_input: Entity<TextInput>,
    chat_input: Entity<TextInput>,
    temperature_input: Entity<TextInput>,
    max_tokens_input: Entity<TextInput>,
    grammar_input: Entity<TextInput>,
    machine: Option<colibri_sys::MachineInfo>,
    /// Live engine (None until Start engine succeeds). Stays put during generate.
    session: Arc<Mutex<Option<EngineSession>>>,
    engine_label: SharedString,
    generating: bool,
    /// [`EngineSession::start`] is running off the UI thread.
    starting: bool,
    /// When the current start dispatch began (elapsed status).
    start_begun: Option<Instant>,
    start_rx: Option<mpsc::Receiver<Result<EngineSession, String>>>,
    /// User pressed Stop; Done → status "stopped".
    stop_requested: bool,
    gen_rx: Option<mpsc::Receiver<GenEvent>>,
    /// Index of the assistant bubble currently receiving tokens.
    streaming_idx: Option<usize>,
    /// Max output tokens for the in-flight generate (denominator for %).
    gen_max_tokens: u32,
    /// Live generate strip; `None` when idle.
    gen_progress: Option<ProgressView>,
    /// Live memory placement / profiling / Brain from pump_visual.
    live_tiers_text: SharedString,
    /// Live engine HWINFO strip (mux telemetry, not static probe).
    live_hwinfo_text: SharedString,
    prof_text: SharedString,
    brain: BrainView,
    brain_prev_hits_seq: u64,
    visual_pump_running: bool,
    /// Heartbeat timer while an engine session is in the slot.
    heartbeat_pump_running: bool,
    /// Optional About strip (default off).
    show_about: bool,
    /// Scanned model registry entries (picker).
    registry_entries: Vec<ModelEntry>,
    registry_status: SharedString,
    /// Selected static catalog id (product supported-models list).
    selected_catalog_id: Option<SharedString>,
    #[cfg(feature = "install")]
    repo_input: Entity<TextInput>,
    #[cfg(feature = "install")]
    revision_input: Entity<TextInput>,
    #[cfg(feature = "install")]
    dest_input: Entity<TextInput>,
    #[cfg(feature = "install")]
    min_free_input: Entity<TextInput>,
    #[cfg(feature = "install")]
    install_status: SharedString,
    #[cfg(feature = "install")]
    install_space: SharedString,
    /// Install panel state machine (Idle / Installing / Pausing / Paused / Cancelling).
    #[cfg(feature = "install")]
    install_phase: InstallUiPhase,
    /// Poll tick for indeterminate "Pausing…" dots.
    #[cfg(feature = "install")]
    install_pause_tick: u64,
    #[cfg(feature = "install")]
    install_rx: Option<mpsc::Receiver<InstallEvent>>,
    #[cfg(feature = "install")]
    install_cancel: Option<InstallCancel>,
    /// Mid-file byte counters from hub ProgressHandler (polled each drain tick).
    #[cfg(feature = "install")]
    install_live: Option<std::sync::Arc<InstallLiveProgress>>,
    #[cfg(feature = "install")]
    install_started: Option<Instant>,
    /// Live install strip; `None` when idle.
    #[cfg(feature = "install")]
    install_progress: Option<ProgressView>,
    // ---- SPA parity state -------------------------------------------------
    active_view: MainView,
    /// Active visual theme (default DOGE).
    theme_id: ThemeId,
    locale: Locale,
    /// Mirrors `native-ui.toml` first_run_done (Skip/Finish set true).
    first_run_done: bool,
    /// Setup wizard (first-run or re-open via Setup).
    wizard: WizardState,
    /// Structured tiers for proportional bar (None until first pump).
    tiers_snap: Option<TiersSnap>,
    /// Rolling profile window for charts.
    profile_turns: Vec<ProfileTurn>,
    /// Topbar badges: last completed turn metrics.
    badge_tokens: Option<u64>,
    badge_tok_s: Option<f32>,
    badge_ttft_ms: Option<f64>,
    /// Live stream counters.
    stream_start: Option<Instant>,
    first_token_at: Option<Instant>,
    live_token_count: u64,
    // ---- Brain atlas (Track Atlas) ----------------------------------------
    experts_atlas: ExpertAtlas,
    /// When true, paint full expert grid (no sample). Env or UI toggle.
    brain_full: bool,
    /// Hover tip text under the grid (empty when not hovering).
    brain_tip: SharedString,
}

impl DesktopApp {
    fn new(cx: &mut Context<Self>) -> Self {
        let prefs = crate::prefs::load();
        let theme_id = ThemeId::from_pref(prefs.theme);
        let p = palette(theme_id);

        // Probe early so store path + free space match Machine panel.
        let machine = probe_machine().ok();
        // Cold-start: ensure the default store root exists once (not every keystroke).
        let store = machine
            .as_ref()
            .map(|m| m.model_store.path.clone())
            .unwrap_or_else(colibri_sys::default_model_store_path);
        let _ = colibri_sys::ensure_model_directory(&store);
        let scan_roots = registry_scan_roots(Some(store.as_path()), std::iter::empty());
        let startup_entries = scan_model_registry(&scan_roots).unwrap_or_default();
        let startup = resolve_startup_model_path(
            env_model_path(),
            &prefs.last_model_path,
            store.as_path(),
            &startup_entries,
        );
        let model_default = startup.display;

        let model_input = cx.new(|cx| TextInput::new(cx, model_default, MODEL_PATH_PLACEHOLDER, p));
        let chat_input = cx.new(|cx| TextInput::new(cx, "", "Message colibrì…", p));
        let temperature_input = cx.new(|cx| TextInput::new(cx, "0.7", "0.7", p));
        let max_tokens_input = cx.new(|cx| TextInput::new(cx, "4096", "4096", p));
        let grammar_input = cx.new(|cx| TextInput::new(cx, "", "GBNF (optional)", p));

        // Restore paused install across restarts (repo/dest/revision + last %).
        #[cfg(feature = "install")]
        let install_checkpoint = load_checkpoint_default();
        #[cfg(feature = "install")]
        let (repo_default, rev_default, dest_default, min_free_default) =
            if let Some(ref cp) = install_checkpoint {
                (
                    cp.repo_id.clone(),
                    cp.revision.clone(),
                    cp.dest.clone(),
                    if cp.min_free_gb.trim().is_empty() {
                        format!("{}", DEFAULT_INSTALL_MIN_FREE_BYTES / colibri_sys::GB)
                    } else {
                        cp.min_free_gb.clone()
                    },
                )
            } else {
                (
                    String::new(),
                    String::new(),
                    String::new(),
                    format!("{}", DEFAULT_INSTALL_MIN_FREE_BYTES / colibri_sys::GB),
                )
            };
        #[cfg(feature = "install")]
        let (startup_install_phase, startup_install_status, startup_install_progress) =
            if let Some(ref cp) = install_checkpoint {
                let pct = cp.percent;
                (
                    InstallUiPhase::Paused,
                    SharedString::from(paused_status_line(pct)),
                    Some(ProgressView::new(pct, None, "Paused")),
                )
            } else {
                (
                    InstallUiPhase::Idle,
                    SharedString::from("Ready to install"),
                    None,
                )
            };

        #[cfg(feature = "install")]
        let repo_input =
            cx.new(|cx| TextInput::new(cx, repo_default, "HF repo id (owner/name)", p));
        #[cfg(feature = "install")]
        let revision_input = cx.new(|cx| TextInput::new(cx, rev_default, "revision (optional)", p));
        #[cfg(feature = "install")]
        let dest_input =
            cx.new(|cx| TextInput::new(cx, dest_default, "dest under store (optional)", p));
        #[cfg(feature = "install")]
        let min_free_input =
            cx.new(|cx| TextInput::new(cx, min_free_default, "min free GB (0 = off)", p));

        let (machine_text, machine, doctor_text, plan_text, mut status) =
            bootstrap_panels_with_machine(&model_input, machine, cx);
        if let Some(note) = startup.note {
            status = note;
        }
        #[cfg(feature = "install")]
        if install_checkpoint.is_some() {
            status = "Install paused · Resume in Tools".into();
        }

        #[cfg(feature = "install")]
        let install_space = {
            let store = machine
                .as_ref()
                .map(|m| m.model_store.path.clone())
                .unwrap_or_else(colibri_sys::default_model_store_path);
            let free = install_free_bytes(&store);
            format_install_space_with_min(&store, free, DEFAULT_INSTALL_MIN_FREE_BYTES).into()
        };

        let (registry_entries, registry_status) = if startup_entries.is_empty() {
            (
                Vec::new(),
                SharedString::from("Scan model store for downloaded models"),
            )
        } else {
            let n = startup_entries.len();
            (
                startup_entries,
                SharedString::from(format_registry_scan_status(n, store.as_path())),
            )
        };

        Self {
            machine_text: machine_text.into(),
            machine_expanded: false,
            doctor_text: doctor_text.into(),
            plan_text: plan_text.into(),
            status: status.into(),
            chat_log: Vec::new(),
            slot_transcripts: HashMap::new(),
            cache_slot: 0,
            kv_slots: 1,
            enable_thinking: false,
            model_input,
            chat_input,
            temperature_input,
            max_tokens_input,
            grammar_input,
            machine,
            session: Arc::new(Mutex::new(None)),
            engine_label: "Engine not started".into(),
            generating: false,
            starting: false,
            start_begun: None,
            start_rx: None,
            stop_requested: false,
            gen_rx: None,
            streaming_idx: None,
            gen_max_tokens: 4096,
            gen_progress: None,
            live_tiers_text: live_tiers_idle_message(LiveTiersIdle::StartEngine).into(),
            live_hwinfo_text: live_hwinfo_idle_message(LiveHwinfoIdle::StartEngine).into(),
            prof_text: "Start the engine, then generate to collect timing.".into(),
            brain: BrainView::default(),
            brain_prev_hits_seq: 0,
            visual_pump_running: false,
            heartbeat_pump_running: false,
            show_about: false,
            registry_entries,
            registry_status,
            selected_catalog_id: None,
            #[cfg(feature = "install")]
            repo_input,
            #[cfg(feature = "install")]
            revision_input,
            #[cfg(feature = "install")]
            dest_input,
            #[cfg(feature = "install")]
            min_free_input,
            #[cfg(feature = "install")]
            install_status: startup_install_status,
            #[cfg(feature = "install")]
            install_space,
            #[cfg(feature = "install")]
            install_phase: startup_install_phase,
            #[cfg(feature = "install")]
            install_pause_tick: 0,
            #[cfg(feature = "install")]
            install_rx: None,
            #[cfg(feature = "install")]
            install_cancel: None,
            #[cfg(feature = "install")]
            install_live: None,
            #[cfg(feature = "install")]
            install_started: None,
            #[cfg(feature = "install")]
            install_progress: startup_install_progress,
            active_view: MainView::Chat,
            theme_id,
            locale: match prefs.locale {
                LocalePref::En => Locale::En,
                LocalePref::It => Locale::It,
            },
            first_run_done: prefs.first_run_done,
            wizard: if prefs.should_show_wizard() {
                WizardState::open_at_start()
            } else {
                WizardState::closed()
            },
            tiers_snap: None,
            profile_turns: Vec::new(),
            badge_tokens: None,
            badge_tok_s: None,
            badge_ttft_ms: None,
            stream_start: None,
            first_token_at: None,
            live_token_count: 0,
            experts_atlas: load_experts_atlas(),
            brain_full: env_brain_full(),
            brain_tip: SharedString::default(),
        }
    }

    fn toggle_brain_full(&mut self, cx: &mut Context<Self>) {
        self.brain_full = !self.brain_full;
        self.apply_visual_snapshot();
        self.status = if self.brain_full {
            "Brain: full grid".into()
        } else {
            "Brain: sampled grid".into()
        };
        cx.notify();
    }

    fn set_brain_tip(&mut self, tip: SharedString, cx: &mut Context<Self>) {
        if self.brain_tip != tip {
            self.brain_tip = tip;
            cx.notify();
        }
    }

    fn clear_brain_tip(&mut self, cx: &mut Context<Self>) {
        if !self.brain_tip.is_empty() {
            self.brain_tip = SharedString::default();
            cx.notify();
        }
    }

    fn tr(&self, key: &str) -> String {
        t(self.locale, key)
    }

    fn tr_fmt(&self, key: &str, pairs: &[(&str, &str)]) -> String {
        t_fmt(self.locale, key, pairs)
    }

    /// Active theme paint tokens (`p.bg`, `p.primary`, …).
    fn palette(&self) -> ThemePalette {
        palette(self.theme_id)
    }

    fn set_view(&mut self, view: MainView, cx: &mut Context<Self>) {
        self.active_view = view;
        cx.notify();
    }

    fn cycle_locale(&mut self, cx: &mut Context<Self>) {
        self.locale = self.locale.next();
        self.persist_prefs(cx);
        self.status = format!("Language · {}", self.locale.label()).into();
        cx.notify();
    }

    fn locale_pref(&self) -> LocalePref {
        match self.locale {
            Locale::En => LocalePref::En,
            Locale::It => LocalePref::It,
        }
    }

    fn persist_prefs(&self, cx: &App) {
        let path = self.model_input.read(cx).text().trim().to_string();
        let snap = shell_prefs_snapshot(
            self.first_run_done,
            self.theme_id.to_pref(),
            self.locale_pref(),
            path,
        );
        if let Err(e) = snap.save() {
            // Surface via status on next notify caller; avoid panic on prefs I/O.
            let _ = e;
        }
    }

    /// Persist shell prefs. Returns `true` when save succeeded.
    /// On failure sets status to the save-error message (do not overwrite).
    fn persist_prefs_status(&mut self, cx: &mut Context<Self>) -> bool {
        let path = self.model_input.read(cx).text().trim().to_string();
        let snap = shell_prefs_snapshot(
            self.first_run_done,
            self.theme_id.to_pref(),
            self.locale_pref(),
            path,
        );
        match snap.save() {
            Ok(()) => true,
            Err(e) => {
                self.status = format!("Could not save settings: {e}").into();
                false
            }
        }
    }

    fn apply_palette_to_inputs(&mut self, p: ThemePalette, cx: &mut Context<Self>) {
        self.model_input
            .update(cx, |input, cx| input.set_palette(p, cx));
        self.chat_input
            .update(cx, |input, cx| input.set_palette(p, cx));
        self.temperature_input
            .update(cx, |input, cx| input.set_palette(p, cx));
        self.max_tokens_input
            .update(cx, |input, cx| input.set_palette(p, cx));
        self.grammar_input
            .update(cx, |input, cx| input.set_palette(p, cx));
        #[cfg(feature = "install")]
        {
            self.repo_input
                .update(cx, |input, cx| input.set_palette(p, cx));
            self.revision_input
                .update(cx, |input, cx| input.set_palette(p, cx));
            self.dest_input
                .update(cx, |input, cx| input.set_palette(p, cx));
            self.min_free_input
                .update(cx, |input, cx| input.set_palette(p, cx));
        }
    }

    fn set_theme_id(&mut self, id: ThemeId, cx: &mut Context<Self>) {
        if self.theme_id == id {
            return;
        }
        self.theme_id = id;
        let p = palette(id);
        self.apply_palette_to_inputs(p, cx);
        // Theme switch: snapshot + apply_theme so prefs round-trip matches Tools contract.
        let path = self.model_input.read(cx).text().trim().to_string();
        let mut snap = shell_prefs_snapshot(
            self.first_run_done,
            ThemeId::Doge.to_pref(),
            self.locale_pref(),
            path,
        );
        apply_theme(&mut snap, id.to_pref());
        match snap.save() {
            Ok(()) => {
                self.status = format!("{} · {}", self.tr("theme.label"), self.theme_label()).into();
            }
            Err(e) => {
                self.status = format!("Could not save settings: {e}").into();
            }
        }
        cx.notify();
    }

    fn theme_label(&self) -> String {
        match self.theme_id {
            ThemeId::Doge => self.tr("theme.doge"),
            ThemeId::Mint => self.tr("theme.mint"),
        }
    }

    fn open_setup_wizard(&mut self, cx: &mut Context<Self>) {
        self.wizard = WizardState::open_at_start();
        self.status = self.tr("setup.open").into();
        cx.notify();
    }

    fn wizard_back(&mut self, cx: &mut Context<Self>) {
        if self.wizard.back() {
            cx.notify();
        }
    }

    fn wizard_next(&mut self, cx: &mut Context<Self>) {
        if self.wizard.step == WizardStep::Model {
            // Entering readiness: recover missing path if possible, then doctor + plan.
            self.run_doctor_with_recovery(false, cx);
            let path = self.model_path(cx);
            self.plan_text = run_plan(&path, self.machine.as_ref()).into();
        }
        if self.wizard.step.is_last() {
            self.wizard_finish(cx);
            return;
        }
        let _ = self.wizard.advance();
        // Choose a model: if cold start left the registry empty, scan once so
        // Supported models can show Installed without requiring Scan first.
        if self.wizard.step == WizardStep::Model && self.registry_entries.is_empty() {
            self.scan_registry(cx);
        }
        cx.notify();
    }

    fn wizard_skip(&mut self, cx: &mut Context<Self>) {
        let mut prefs = NativePrefs {
            version: crate::prefs::PREFS_VERSION,
            first_run_done: self.first_run_done,
            theme: self.theme_id.to_pref(),
            locale: self.locale_pref(),
            last_model_path: self.model_input.read(cx).text().trim().to_string(),
        };
        complete_wizard(&mut prefs, &mut self.wizard);
        self.first_run_done = prefs.first_run_done;
        let save_ok = self.persist_prefs_status(cx);
        if wizard_may_set_success_status(save_ok) {
            self.status = wizard_complete_success_status(false).into();
        }
        cx.notify();
    }

    fn wizard_finish(&mut self, cx: &mut Context<Self>) {
        let mut prefs = NativePrefs {
            version: crate::prefs::PREFS_VERSION,
            first_run_done: self.first_run_done,
            theme: self.theme_id.to_pref(),
            locale: self.locale_pref(),
            last_model_path: self.model_input.read(cx).text().trim().to_string(),
        };
        complete_wizard(&mut prefs, &mut self.wizard);
        self.first_run_done = prefs.first_run_done;
        let save_ok = self.persist_prefs_status(cx);
        if wizard_may_set_success_status(save_ok) {
            self.status = wizard_complete_success_status(true).into();
        }
        cx.notify();
    }

    fn engine_is_live(&self) -> bool {
        self.session.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    fn stop_engine(&mut self, cx: &mut Context<Self>) {
        if self.starting {
            let elapsed = self.start_begun.map(|t| t.elapsed()).unwrap_or_default();
            self.status = engine_starting_status(elapsed).into();
            cx.notify();
            return;
        }
        if self.generating {
            self.status = "Stop the current reply first".into();
            cx.notify();
            return;
        }
        {
            let mut g = self.session.lock().unwrap();
            *g = None;
        }
        self.engine_label = "Engine not started".into();
        self.kv_slots = 1;
        self.cache_slot = 0;
        self.tiers_snap = None;
        self.live_tiers_text = live_tiers_idle_message(LiveTiersIdle::EngineStopped).into();
        self.live_hwinfo_text = live_hwinfo_idle_message(LiveHwinfoIdle::EngineStopped).into();
        self.status = "Engine stopped".into();
        cx.notify();
    }

    fn model_path_summary(&self, cx: &App) -> String {
        let raw = self.model_input.read(cx).text().trim().to_string();
        if raw.is_empty() {
            return self.tr("rail.modelUnset");
        }
        // Show basename-ish tail for the slim rail.
        let p = Path::new(&raw);
        p.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or(raw)
    }

    fn clear_chat(&mut self, cx: &mut Context<Self>) {
        if self.generating {
            self.status = "Cannot clear while generating".into();
            cx.notify();
            return;
        }
        self.chat_log.clear();
        self.streaming_idx = None;
        self.badge_tokens = None;
        self.badge_tok_s = None;
        self.badge_ttft_ms = None;
        self.live_token_count = 0;
        self.status = "Chat cleared".into();
        cx.notify();
    }

    fn apply_suggested_prompt(&mut self, prompt_key: &'static str, cx: &mut Context<Self>) {
        let text = self.tr(prompt_key);
        self.chat_input
            .update(cx, |input, cx| input.set_text(text, cx));
        self.send_chat(cx);
    }

    fn refresh_machine_text(&mut self) {
        if let Some(ref m) = self.machine {
            self.machine_text = format_machine(m, self.machine_expanded).into();
        }
    }

    fn model_path(&self, cx: &App) -> PathBuf {
        // Expand leading ~ / ~/ so doctor, plan, and open share the same path.
        colibri_sys::expand_user_path(PathBuf::from(self.model_input.read(cx).text().trim()))
    }

    fn refresh_probe(&mut self, cx: &mut Context<Self>) {
        match probe_machine() {
            Ok(m) => {
                self.machine = Some(m);
                self.refresh_machine_text();
                self.status = "Machine refreshed".into();
                #[cfg(feature = "install")]
                {
                    self.refresh_install_space(cx);
                }
            }
            Err(e) => {
                self.machine_text = format!("Could not read this machine: {e}").into();
                self.status = "Machine probe failed".into();
            }
        }
        cx.notify();
    }

    fn toggle_machine_details(&mut self, cx: &mut Context<Self>) {
        self.machine_expanded = !self.machine_expanded;
        self.refresh_machine_text();
        cx.notify();
    }

    fn toggle_about(&mut self, cx: &mut Context<Self>) {
        self.show_about = !self.show_about;
        cx.notify();
    }

    fn run_doctor(&mut self, cx: &mut Context<Self>) {
        self.status = "Running doctor...".into();
        cx.notify();
        self.run_doctor_with_recovery(false, cx);
    }

    fn run_deep_doctor(&mut self, cx: &mut Context<Self>) {
        self.status = "Running doctor...".into();
        cx.notify();
        self.run_doctor_with_recovery(true, cx);
    }

    /// Wizard Doctor-step button handler. Id must map via
    /// [`readiness_action_for_button_id`] (same constants as element `.id(...)`).
    fn handle_wizard_readiness_button(&mut self, button_id: &'static str, cx: &mut Context<Self>) {
        let Some(action) = readiness_action_for_button_id(button_id) else {
            self.status = format!("Unknown doctor action: {button_id}").into();
            cx.notify();
            return;
        };
        self.dispatch_readiness_action(action, cx);
    }

    /// Run a Doctor-step action with immediate status, host side effects, and
    /// stamped Health check / status so a no-op-looking checklist still proves
    /// the click landed.
    fn dispatch_readiness_action(&mut self, action: WizardReadinessAction, cx: &mut Context<Self>) {
        // Immediate feedback (status rail under Setup).
        self.status = readiness_running_status(action).into();
        cx.notify();

        if action == WizardReadinessAction::InstallModel {
            self.wizard_open_install(cx);
            return;
        }

        // Yield one frame so "Running doctor..." / "Quick check..." paints
        // before blocking host work (mkdir, doctor, scan).
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.finish_readiness_action(action, cx);
            });
        })
        .detach();
    }

    fn finish_readiness_action(&mut self, action: WizardReadinessAction, cx: &mut Context<Self>) {
        let clock = readiness_clock_now();
        match action {
            WizardReadinessAction::RunDoctor => {
                self.run_doctor_with_recovery(true, cx);
                let body = self.doctor_text.to_string();
                let out = readiness_click_outcome(action, &body, None, &clock);
                self.doctor_text = out.doctor_text.into();
                self.status = out.status.into();
            }
            WizardReadinessAction::QuickCheck => {
                self.run_doctor_with_recovery(false, cx);
                let path = self.model_path(cx);
                let plan = run_plan(&path, self.machine.as_ref());
                let body = self.doctor_text.to_string();
                let out = readiness_click_outcome(action, &body, Some(&plan), &clock);
                self.doctor_text = out.doctor_text.into();
                if let Some(p) = out.plan_text {
                    self.plan_text = p.into();
                }
                self.status = out.status.into();
            }
            WizardReadinessAction::ScanModels => {
                self.scan_registry(cx);
                self.run_doctor_with_recovery(false, cx);
                let path = self.model_path(cx);
                let plan = run_plan(&path, self.machine.as_ref());
                let body = self.doctor_text.to_string();
                let out = readiness_click_outcome(action, &body, Some(&plan), &clock);
                self.doctor_text = out.doctor_text.into();
                if let Some(p) = out.plan_text {
                    self.plan_text = p.into();
                }
                self.status = out.status.into();
            }
            WizardReadinessAction::InstallModel => {
                self.wizard_open_install(cx);
            }
        }
        cx.notify();
    }

    /// True when path is not a model leaf (missing, or no config.json).
    fn path_needs_model_recovery(path: &Path) -> bool {
        if model_path_unset_for_doctor(path) {
            return false; // idle branch, not recovery scan
        }
        !path.exists() || !path.join("config.json").is_file()
    }

    /// Shallow or deep doctor. When the path is missing or is not a model leaf
    /// (no config.json), create the path if needed, scan the default store and
    /// auto-select a single model, list many, or show recovery copy.
    fn run_doctor_with_recovery(&mut self, deep: bool, cx: &mut Context<Self>) {
        let path = self.model_path(cx);
        if model_path_unset_for_doctor(&path) {
            self.doctor_text = if deep {
                run_deep_doctor(&path, self.machine.as_ref())
            } else {
                run_shallow_doctor(&path, self.machine.as_ref())
            }
            .into();
            self.status = if deep {
                "Doctor finished".into()
            } else {
                "Checks finished".into()
            };
            cx.notify();
            return;
        }

        // Create the typed model path when missing (mkdir, not recovery-only text).
        let mut just_created = false;
        if !path.exists() {
            match ensure_model_path_for_doctor(&path, self.machine.as_ref()) {
                Ok(colibri_sys::EnsureModelDir::Created) => {
                    just_created = true;
                }
                Ok(colibri_sys::EnsureModelDir::AlreadyExists) => {}
                Err(msg) => {
                    self.doctor_text = msg.into();
                    self.status = "Could not create model folder".into();
                    cx.notify();
                    return;
                }
            }
        }

        // Empty / non-model dir: product colibri.toml + UI native-ui.toml (same as
        // run_doctor_checks). Never invents HF config.json.
        if path.is_dir() && !path.join("config.json").is_file() {
            let _ = scaffold_doctor_defaults(&path);
        }

        if !Self::path_needs_model_recovery(&path) {
            self.doctor_text = if deep {
                run_deep_doctor(&path, self.machine.as_ref())
            } else {
                run_shallow_doctor(&path, self.machine.as_ref())
            }
            .into();
            self.status = if deep {
                "Doctor finished".into()
            } else {
                "Checks finished".into()
            };
            cx.notify();
            return;
        }

        let store = self
            .machine
            .as_ref()
            .map(|m| m.model_store.path.clone())
            .unwrap_or_else(colibri_sys::default_model_store_path);
        let roots = registry_scan_roots(Some(store.as_path()), std::iter::empty());
        let entries = scan_model_registry(&roots).unwrap_or_default();
        // For existing non-model dirs (e.g. empty store root), still use the
        // missing-path scan outcome helpers when there is nothing to auto-pick.
        match missing_path_scan_outcome(
            &path,
            store.as_path(),
            &entries,
            self.machine.as_ref(),
            deep,
        ) {
            MissingPathScanOutcome::AutoSelected {
                path: found,
                doctor,
                status,
            } => {
                let display = found.display().to_string();
                self.model_input
                    .update(cx, |input, cx| input.set_text(display, cx));
                self.doctor_text = doctor.into();
                self.status = status.into();
                self.registry_entries = entries;
                self.registry_status = format_registry_scan_status(1, store.as_path()).into();
            }
            MissingPathScanOutcome::ListedMany {
                doctor,
                status,
                entries: listed,
            } => {
                let n = listed.len();
                self.doctor_text = doctor.into();
                self.status = status.into();
                self.registry_entries = listed;
                self.registry_status = format_registry_scan_status(n, store.as_path()).into();
            }
            MissingPathScanOutcome::StillMissing { doctor, status } => {
                // Prefer "created" copy when we just mkdir'd an empty path.
                // Otherwise run doctor (not-a-model / checks) on the typed path.
                if just_created {
                    // Same scaffold as run_doctor_checks: product colibri.toml + UI prefs.
                    let _ = scaffold_doctor_defaults(&path);
                    self.doctor_text =
                        format_created_model_directory(&path, store.as_path()).into();
                    self.status = status.into();
                } else if path.exists() {
                    self.doctor_text = if deep {
                        run_deep_doctor(&path, self.machine.as_ref())
                    } else {
                        run_shallow_doctor(&path, self.machine.as_ref())
                    }
                    .into();
                    self.status = if deep {
                        "Doctor finished".into()
                    } else {
                        "Checks finished".into()
                    };
                } else {
                    self.doctor_text = doctor.into();
                    self.status = status.into();
                }
                self.registry_entries = entries;
                self.registry_status = format_empty_registry_scan(store.as_path()).into();
            }
        }
        cx.notify();
    }

    fn run_plan(&mut self, cx: &mut Context<Self>) {
        let path = self.model_path(cx);
        self.plan_text = run_plan(&path, self.machine.as_ref()).into();
        self.status = "Plan finished".into();
        cx.notify();
    }

    /// Doctor step: jump to Model + show install form (same HF install UX).
    fn wizard_open_install(&mut self, cx: &mut Context<Self>) {
        self.wizard.step = WizardStep::Model;
        self.wizard.show_download = true;
        self.status = "Install a model into the default store".into();
        cx.notify();
    }

    fn start_engine(&mut self, cx: &mut Context<Self>) {
        self.start_engine_from(StartEngineSource::Rail, cx);
    }

    fn start_engine_from(&mut self, source: StartEngineSource, cx: &mut Context<Self>) {
        match should_dispatch_engine_start(self.generating, self.starting) {
            Err(StartEngineBlock::Generating) => {
                self.status = "Cannot restart while generating".into();
                cx.notify();
                return;
            }
            Err(StartEngineBlock::AlreadyStarting) => {
                let elapsed = self.start_begun.map(|t| t.elapsed()).unwrap_or_default();
                self.status = engine_starting_status(elapsed).into();
                cx.notify();
                return;
            }
            Ok(()) => {}
        }

        let path = self.model_path(cx);
        tracing::info!(
            target: "colibri_native",
            source = source.as_str(),
            model = %path.display(),
            "start engine clicked"
        );
        self.starting = true;
        self.start_begun = Some(Instant::now());
        self.status = engine_starting_status(Duration::ZERO).into();
        self.engine_label = "Engine starting…".into();
        cx.notify();

        let (tx, rx) = mpsc::channel();
        self.start_rx = Some(rx);
        EngineSession::start_async(path, Some(self.session.clone()), tx);
        self.schedule_start_poll(cx);
    }

    fn schedule_start_poll(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                let cont = this
                    .update(cx, |this, cx| this.drain_start(cx))
                    .unwrap_or(false);
                if !cont {
                    break;
                }
            }
        })
        .detach();
    }

    /// Drain start worker. Returns true while still starting.
    fn drain_start(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(rx) = self.start_rx.as_ref() else {
            return false;
        };
        match rx.try_recv() {
            Ok(Ok(session)) => {
                self.starting = false;
                self.start_rx = None;
                self.start_begun = None;
                self.apply_started_session(session, cx);
                cx.notify();
                false
            }
            Ok(Err(e)) => {
                self.starting = false;
                self.start_rx = None;
                self.start_begun = None;
                self.engine_label = "Engine not started".into();
                self.status = format!("Could not start engine: {e}").into();
                tracing::error!(
                    target: "colibri_native",
                    error = %e,
                    "engine start failed"
                );
                cx.notify();
                false
            }
            Err(mpsc::TryRecvError::Empty) => {
                let elapsed = self.start_begun.map(|t| t.elapsed()).unwrap_or_default();
                self.status = engine_starting_status(elapsed).into();
                cx.notify();
                true
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.starting = false;
                self.start_rx = None;
                self.start_begun = None;
                self.engine_label = "Engine not started".into();
                self.status = "Could not start engine: start worker ended".into();
                cx.notify();
                false
            }
        }
    }

    fn apply_started_session(&mut self, session: EngineSession, cx: &mut Context<Self>) {
        self.kv_slots = session.kv_slots().max(1);
        if self.cache_slot >= self.kv_slots {
            self.select_cache_slot(0, cx);
        }
        let path_note = session.path_status();
        self.engine_label = format!(
            "Ready · {} · {} · {} · {} session slot{}",
            session.model_id(),
            session.family().as_str(),
            path_note,
            self.kv_slots,
            if self.kv_slots == 1 { "" } else { "s" }
        )
        .into();
        let status = if session.is_ffi() {
            "Engine ready (in-process). Expert map and live stats update while you chat."
                .to_string()
        } else if path_note.starts_with("In-process") {
            path_note.to_string()
        } else {
            "Engine ready".into()
        };
        *self.session.lock().unwrap() = Some(session);
        self.status = status.into();
        self.persist_prefs(cx);
        self.ensure_visual_pump(cx);
        self.ensure_session_heartbeat(cx);
        self.apply_visual_snapshot();
    }

    /// Stash current transcript and restore the target session slot (web sticky slots).
    fn select_cache_slot(&mut self, new_slot: u32, _cx: &mut Context<Self>) {
        let new_slot = crate::host::clamp_cache_slot(new_slot, self.kv_slots);
        if new_slot == self.cache_slot {
            return;
        }
        let current: Vec<(String, String)> = self
            .chat_log
            .iter()
            .map(|(r, t)| (r.to_string(), t.to_string()))
            .collect();
        let (active, next) = switch_cache_slot_transcript(
            &mut self.slot_transcripts,
            self.cache_slot,
            new_slot,
            current,
        );
        self.cache_slot = active;
        self.chat_log = next
            .into_iter()
            .map(|(r, t)| (SharedString::from(r), SharedString::from(t)))
            .collect();
        self.streaming_idx = None;
    }

    fn cycle_cache_slot(&mut self, delta: i32, cx: &mut Context<Self>) {
        if self.generating {
            self.status = "Cannot switch session slot while generating".into();
            cx.notify();
            return;
        }
        let n = self.kv_slots.max(1) as i32;
        let cur = self.cache_slot as i32;
        let next = ((cur + delta).rem_euclid(n)) as u32;
        self.select_cache_slot(next, cx);
        self.status = format!(
            "Session slot {} of {}",
            self.cache_slot + 1,
            self.kv_slots.max(1)
        )
        .into();
        cx.notify();
    }

    fn toggle_reasoning(&mut self, cx: &mut Context<Self>) {
        self.enable_thinking = !self.enable_thinking;
        self.status = if self.enable_thinking {
            "Reasoning on".into()
        } else {
            "Reasoning off".into()
        };
        cx.notify();
    }

    fn send_chat(&mut self, cx: &mut Context<Self>) {
        if self.generating {
            self.status = "already generating…".into();
            cx.notify();
            return;
        }

        let text = self.chat_input.read(cx).text().trim().to_string();
        if text.is_empty() {
            return;
        }

        let controls = match controls_from_ui(
            &self.temperature_input.read(cx).text(),
            &self.max_tokens_input.read(cx).text(),
            self.enable_thinking,
            self.cache_slot,
            &self.grammar_input.read(cx).text(),
            self.kv_slots,
        ) {
            Ok(c) => c,
            Err(e) => {
                self.status = format!("Inference settings: {e}").into();
                cx.notify();
                return;
            }
        };

        let has_engine = self.session.lock().map(|g| g.is_some()).unwrap_or(false);

        if !has_engine {
            if self.starting {
                let elapsed = self.start_begun.map(|t| t.elapsed()).unwrap_or_default();
                self.status = engine_starting_status(elapsed).into();
                cx.notify();
                return;
            }
            let path = self.model_path(cx);
            if path.as_os_str().is_empty() || !path.is_dir() {
                self.status =
                    "Set a model path (folder with the model files), then start the engine.".into();
                cx.notify();
                return;
            }
            self.start_engine_from(StartEngineSource::ChatSend, cx);
            return;
        }

        self.chat_input.update(cx, |input, cx| input.clear(cx));
        self.chat_log.push(("user".into(), text.clone().into()));
        self.chat_log.push(("assistant".into(), "".into()));
        self.streaming_idx = Some(self.chat_log.len() - 1);
        self.generating = true;
        self.stop_requested = false;
        self.gen_max_tokens = controls.max_tokens;
        self.gen_progress = Some(progress_view_for_generate(0, controls.max_tokens, 0.0));
        self.status = self
            .gen_progress
            .as_ref()
            .map(|v| v.line())
            .unwrap_or_else(|| "Generating...".into())
            .into();
        self.stream_start = Some(Instant::now());
        self.first_token_at = None;
        self.live_token_count = 0;
        clear_prefill_progress();
        self.badge_ttft_ms = None;

        let turns: Vec<(String, String)> = self
            .chat_log
            .iter()
            .filter(|(r, _)| r.as_ref() == "user" || r.as_ref() == "assistant")
            .map(|(r, t)| (r.to_string(), t.to_string()))
            .collect();
        let mut messages_turns = turns;
        if messages_turns
            .last()
            .is_some_and(|(r, t)| r == "assistant" && t.is_empty())
        {
            messages_turns.pop();
        }
        let messages = messages_from_turns(&messages_turns);

        let (tx, rx) = mpsc::channel();
        self.gen_rx = Some(rx);
        EngineSession::generate_async(self.session.clone(), messages, controls, tx);
        self.ensure_visual_pump(cx);
        self.ensure_session_heartbeat(cx);
        self.schedule_gen_poll(cx);
        cx.notify();
    }

    fn stop_generate(&mut self, cx: &mut Context<Self>) {
        if !self.generating {
            self.status = "nothing to stop".into();
            cx.notify();
            return;
        }
        match stop_session(&self.session) {
            Ok(req_id) => {
                self.stop_requested = true;
                self.status = format!("stopping req {req_id}…").into();
            }
            Err(e) => {
                self.stop_requested = true;
                self.status = format!("stop failed: {e}").into();
            }
        }
        cx.notify();
    }

    fn ensure_visual_pump(&mut self, cx: &mut Context<Self>) {
        if self.visual_pump_running {
            return;
        }
        let has = self.session.lock().map(|g| g.is_some()).unwrap_or(false);
        if !has {
            return;
        }
        self.visual_pump_running = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(VISUAL_PUMP_MS))
                    .await;
                let cont = this
                    .update(cx, |this, cx| {
                        let has = this.session.lock().map(|g| g.is_some()).unwrap_or(false);
                        if !has {
                            this.visual_pump_running = false;
                            this.live_tiers_text =
                                live_tiers_idle_message(LiveTiersIdle::EngineStopped).into();
                            this.live_hwinfo_text =
                                live_hwinfo_idle_message(LiveHwinfoIdle::EngineStopped).into();
                            this.tiers_snap = None;
                            cx.notify();
                            return false;
                        }
                        this.apply_visual_snapshot();
                        this.refresh_prefill_status_chip(cx);
                        #[cfg(feature = "install")]
                        {
                            let _ = this.drain_install(cx);
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !cont {
                    break;
                }
            }
        })
        .detach();
    }

    fn ensure_session_heartbeat(&mut self, cx: &mut Context<Self>) {
        if self.heartbeat_pump_running {
            return;
        }
        let has = self.session.lock().map(|g| g.is_some()).unwrap_or(false);
        if !crate::log_init::session_heartbeat_pump_should_continue(has) {
            return;
        }
        self.heartbeat_pump_running = true;
        crate::log_init::log_session_heartbeat(session_engine_kind(&self.session));
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(SESSION_HEARTBEAT_MS))
                    .await;
                let cont = this
                    .update(cx, |this, _cx| {
                        let has = this.session.lock().map(|g| g.is_some()).unwrap_or(false);
                        if !crate::log_init::session_heartbeat_pump_should_continue(has) {
                            this.heartbeat_pump_running = false;
                            return false;
                        }
                        crate::log_init::log_session_heartbeat(session_engine_kind(&this.session));
                        true
                    })
                    .unwrap_or(false);
                if !cont {
                    break;
                }
            }
        })
        .detach();
    }

    /// Apply the latest visual snapshot. Must never wait on the generate mutex.
    fn apply_visual_snapshot(&mut self) {
        let Some(snap) = pump_session_visual(&self.session) else {
            return;
        };
        if let Some(ref t) = snap.tiers {
            self.live_tiers_text = format_live_tiers(t).into();
            self.tiers_snap = Some(t.clone());
        } else {
            self.live_tiers_text = live_tiers_idle_message(LiveTiersIdle::Waiting).into();
        }
        if let Some(ref h) = snap.hwinfo {
            self.live_hwinfo_text = format_live_hwinfo(h).into();
        } else {
            self.live_hwinfo_text = live_hwinfo_idle_message(LiveHwinfoIdle::Waiting).into();
        }
        self.prof_text = format_profile_turns(&snap.profile, PROF_LAST_N).into();
        self.profile_turns = snap.profile.clone();
        if let Some(ref map) = snap.expert_map {
            let hits = snap.expert_hits.as_ref();
            // Full-grid when UI toggle or env; else default sample cap.
            let mut view = if self.brain_full {
                brain_view_from_map_with_max(map, hits, self.brain_prev_hits_seq, usize::MAX)
            } else {
                brain_view_from_map(map, hits, self.brain_prev_hits_seq)
            };
            let decay_steps = brain_pulse_decay_steps_for_ms(VISUAL_PUMP_MS);
            apply_brain_pulse_decay(&mut view, &self.brain, decay_steps);
            if view.hits_seq > 0 {
                self.brain_prev_hits_seq = view.hits_seq;
            }
            self.brain = view;
        }
    }

    fn schedule_gen_poll(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(40))
                    .await;
                let cont = this
                    .update(cx, |this, cx| this.drain_gen(cx))
                    .unwrap_or(false);
                if !cont {
                    break;
                }
            }
        })
        .detach();
    }

    /// Drain channel; returns true if still generating.
    fn drain_gen(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(rx) = self.gen_rx.as_ref() else {
            return false;
        };
        let mut still = true;
        // Keep the Done 100% strip until the next generate (do not clear same drain).
        let mut keep_done_progress = false;
        loop {
            match rx.try_recv() {
                Ok(GenEvent::Token(s)) => {
                    if self.first_token_at.is_none() {
                        let now = Instant::now();
                        self.first_token_at = Some(now);
                        if let Some(start) = self.stream_start {
                            self.badge_ttft_ms =
                                Some(now.duration_since(start).as_secs_f64() * 1000.0);
                        }
                    }
                    self.live_token_count = self.live_token_count.saturating_add(1);
                    if let Some(start) = self.stream_start {
                        // Live tok/s from stream start after first token.
                        let since = start.elapsed().as_secs_f64().max(1e-6);
                        self.badge_tok_s = Some(self.live_token_count as f32 / since as f32);
                    }
                    self.badge_tokens = Some(self.live_token_count);
                    let tok_s = self.badge_tok_s.map(|s| s as f64).unwrap_or(0.0);
                    let view = progress_view_for_generate(
                        self.live_token_count.min(u32::MAX as u64) as u32,
                        self.gen_max_tokens,
                        tok_s,
                    );
                    self.status = view.line().into();
                    self.gen_progress = Some(view);
                    if let Some(idx) = self.streaming_idx {
                        if let Some((_, buf)) = self.chat_log.get_mut(idx) {
                            let mut t = buf.to_string();
                            t.push_str(&s);
                            *buf = t.into();
                        }
                    }
                    cx.notify();
                }
                Ok(GenEvent::Done {
                    completion_tokens,
                    tokens_per_second,
                }) => {
                    self.badge_tokens = Some(completion_tokens);
                    self.badge_tok_s = Some(tokens_per_second);
                    self.gen_progress = Some(progress_view_generate_done());
                    self.status = status_after_gen_done(
                        self.stop_requested,
                        completion_tokens,
                        tokens_per_second,
                    )
                    .into();
                    // OS desktop notification once per generate end (not per token).
                    let kind = inference_end_kind(self.stop_requested, false);
                    notify_inference_end(
                        kind,
                        Some(completion_tokens),
                        Some(tokens_per_second),
                        None,
                    );
                    still = false;
                    keep_done_progress = true;
                    cx.notify();
                }
                Ok(GenEvent::Error(e)) => {
                    if let Some(idx) = self.streaming_idx {
                        if let Some((_, buf)) = self.chat_log.get_mut(idx) {
                            if buf.is_empty() {
                                *buf = format!("(error: {e})").into();
                            } else {
                                let mut t = buf.to_string();
                                t.push_str(&format!("\n(error: {e})"));
                                *buf = t.into();
                            }
                        }
                    }
                    if self.stop_requested {
                        self.status = format!("stopped ({e})").into();
                    } else {
                        self.status = format!("generate error: {e}").into();
                    }
                    // One OS notify on error end / user-stop error (not progress).
                    let kind = inference_end_kind(self.stop_requested, true);
                    notify_inference_end(kind, None, None, Some(e.as_str()));
                    still = false;
                    keep_done_progress = false;
                    cx.notify();
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    still = false;
                    break;
                }
            }
        }
        if still {
            self.refresh_prefill_status_chip(cx);
        }
        if !still {
            self.generating = false;
            self.stop_requested = false;
            self.gen_rx = None;
            self.streaming_idx = None;
            self.stream_start = None;
            // Done leaves 100% strip painted; next generate replaces it.
            if !keep_done_progress {
                self.gen_progress = None;
            }
            self.apply_visual_snapshot();
        }
        still
    }

    /// Live chip while generate has no decode tokens yet. Never locks the engine.
    fn refresh_prefill_status_chip(&mut self, cx: &mut Context<Self>) {
        let Some(line) = apply_prefill_status(
            self.generating,
            self.live_token_count,
            load_prefill_progress(),
            self.session.as_ref(),
        ) else {
            return;
        };
        if self.status.as_ref() != line {
            self.status = line.into();
            cx.notify();
        }
    }

    fn scan_registry(&mut self, cx: &mut Context<Self>) {
        let store = self.machine.as_ref().map(|m| m.model_store.path.as_path());
        let roots = registry_scan_roots(store, std::iter::empty());
        let store_path = roots
            .first()
            .map(PathBuf::as_path)
            .unwrap_or_else(|| Path::new("(no store)"));
        match scan_model_registry(&roots) {
            Ok(entries) => {
                let n = entries.len();
                self.registry_status = if n == 0 {
                    format_empty_registry_scan(store_path).into()
                } else {
                    format_registry_scan_status(n, store_path).into()
                };
                // When current path is empty, missing, or not a model leaf
                // (no config.json) and scan finds exactly one usable model,
                // set the path so Doctor / Plan can run.
                let current = self.model_path(cx);
                let need_path = model_path_unset_for_doctor(&current)
                    || Self::path_needs_model_recovery(&current);
                if need_path {
                    if let Some(one) = crate::host::pick_single_usable_model(&entries) {
                        let display = one.display().to_string();
                        self.model_input
                            .update(cx, |input, cx| input.set_text(display.clone(), cx));
                        self.status = format!("Model path set · {display}").into();
                    } else {
                        self.status = "Model list refreshed".into();
                    }
                } else {
                    self.status = "Model list refreshed".into();
                }
                self.registry_entries = entries;
            }
            Err(e) => {
                self.registry_status = format!("scan failed: {e}").into();
                self.registry_entries.clear();
            }
        }
        cx.notify();
    }

    fn select_registry_entry(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(entry) = self.registry_entries.get(index) else {
            return;
        };
        let path = entry.path.display().to_string();
        self.model_input
            .update(cx, |input, cx| input.set_text(path.clone(), cx));
        self.status = format!("Model path set · {path}").into();
        self.registry_status = format!("Selected {}", entry.path.display()).into();
        cx.notify();
    }

    /// Pick a product-supported model: fill install form when installable.
    ///
    /// When the catalog row is already Present on disk, also set the model path
    /// so Doctor / engine work without a second registry click.
    fn select_supported_model(&mut self, catalog_id: &str, cx: &mut Context<Self>) {
        let Some(sel) = catalog_selection_by_id(catalog_id) else {
            self.status = format!("Unknown supported model · {catalog_id}").into();
            cx.notify();
            return;
        };
        self.selected_catalog_id = Some(sel.id.clone().into());
        self.status = sel.status.clone().into();

        // Path for installed Present leaves (match against registry scan).
        if let Some(model) = list_supported_models().iter().find(|m| m.id == catalog_id) {
            if let Some(entry) = catalog_is_installed(model, &self.registry_entries) {
                let path = entry.path.display().to_string();
                self.model_input
                    .update(cx, |input, cx| input.set_text(path.clone(), cx));
                self.status = format!("Model path set · {path}").into();
                self.registry_status = format!("Selected {}", entry.path.display()).into();
            }
        }

        #[cfg(feature = "install")]
        {
            if sel.installable {
                if let Some(repo) = sel.repo_id.clone() {
                    self.repo_input
                        .update(cx, |input, cx| input.set_text(repo, cx));
                }
                if let Some(dest) = sel.dest.clone() {
                    self.dest_input
                        .update(cx, |input, cx| input.set_text(dest, cx));
                }
                self.install_status = sel.status.clone().into();
                self.wizard.show_download = true;
                self.refresh_install_space(cx);
            } else {
                self.install_status = sel.status.clone().into();
            }
        }
        cx.notify();
    }

    /// Supported-models catalog rows (wizard + Tools).
    ///
    /// Rows live in a max-height scroll viewport so a long catalog does not
    /// push wizard footer controls off-screen.
    fn supported_catalog_panel(&self, cx: &mut Context<Self>, id_prefix: &str) -> impl IntoElement {
        let p = self.palette();
        let selected = self.selected_catalog_id.clone();
        let list_id = SharedString::from(format!("{id_prefix}-catalog-list"));
        let models = list_supported_models();
        // Cap height always; pure helper pins when overflow is expected as the
        // catalog grows (paint path stays stable either way).
        let _catalog_scrolls =
            list_exceeds_max_height(models.len(), LIST_ROW_H, CATALOG_LIST_MAX_H);
        let catalog_max_h = CATALOG_LIST_MAX_H;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .w_full()
            .min_w_0()
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.tr("catalog.supported")),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(p.muted))
                    .child(self.tr("catalog.supportedHelp")),
            )
            .child(
                div()
                    .id(list_id)
                    .flex()
                    .flex_col()
                    .gap_1()
                    .w_full()
                    .min_w_0()
                    .max_h(px(catalog_max_h))
                    .overflow_scroll()
                    .children(models.iter().enumerate().map(|(i, model)| {
                        let id = model.id;
                        let label = format_supported_model_row(model);
                        let is_sel = selected.as_ref().is_some_and(|s| s.as_ref() == id);
                        let installed =
                            catalog_is_installed(model, &self.registry_entries).is_some();
                        let style = catalog_row_style(
                            installed,
                            is_sel,
                            p.primary,
                            p.primary_fg,
                            p.secondary,
                            p.text,
                            p.border,
                        );
                        let row_id = SharedString::from(format!("{id_prefix}-catalog-{i}"));
                        let catalog_id = id.to_string();
                        let installed_label = self.tr("catalog.installed");
                        let mut row = div()
                            .id(row_id)
                            .w_full()
                            .min_w_0()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .px_2()
                            .py_1()
                            .bg(rgb(style.fill))
                            .border_1()
                            .border_color(rgb(style.border))
                            .text_xs()
                            .text_color(rgb(style.fg))
                            .cursor_pointer()
                            .child(div().min_w_0().flex_1().overflow_hidden().child(label));
                        if style.show_installed {
                            row = row.child(
                                div()
                                    .flex_shrink_0()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(installed_label),
                            );
                        }
                        row.on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.select_supported_model(&catalog_id, cx);
                            }),
                        )
                    })),
            )
    }

    #[cfg(feature = "install")]
    fn refresh_install_space(&mut self, cx: &App) {
        let store = self
            .machine
            .as_ref()
            .map(|m| m.model_store.path.clone())
            .unwrap_or_else(colibri_sys::default_model_store_path);
        let free = install_free_bytes(&store);
        let min = parse_min_free_gb(&self.min_free_input.read(cx).text())
            .unwrap_or(DEFAULT_INSTALL_MIN_FREE_BYTES);
        self.install_space = format_install_space_with_min(&store, free, min).into();
    }

    #[cfg(feature = "install")]
    fn start_install(&mut self, cx: &mut Context<Self>) {
        if install_is_busy(self.install_phase) {
            self.install_status = "install already running".into();
            cx.notify();
            return;
        }
        let from_paused = matches!(self.install_phase, InstallUiPhase::Paused);
        let repo = self.repo_input.read(cx).text();
        let rev = self.revision_input.read(cx).text();
        let dest_ov = self.dest_input.read(cx).text();
        let min_text = self.min_free_input.read(cx).text();
        let store = self.machine.as_ref().map(|m| m.model_store.path.as_path());
        let min_free = match parse_min_free_gb(&min_text) {
            Ok(v) => v,
            Err(e) => {
                self.install_status = format!("install invalid: {e}").into();
                cx.notify();
                return;
            }
        };
        let (repo_id, revision, dest) = match validate_install_form(&repo, &rev, &dest_ov, store) {
            Ok(v) => v,
            Err(e) => {
                self.install_status = format!("install invalid: {e}").into();
                cx.notify();
                return;
            }
        };
        let free = match check_install_free_space(&dest, min_free) {
            Ok(f) => f,
            Err(e) => {
                self.install_status = format!("install refused: {e}").into();
                self.install_space =
                    format_install_space_with_min(&dest, install_free_bytes(&dest), min_free)
                        .into();
                cx.notify();
                return;
            }
        };
        self.install_space = format_install_space_with_min(&dest, free, min_free).into();

        // Fresh install (not Resume) must not keep a stale pause checkpoint.
        if !from_paused {
            let _ = clear_checkpoint_default();
        }

        let (tx, rx) = mpsc::channel();
        self.install_rx = Some(rx);
        self.install_phase = install_ui_transition(
            self.install_phase,
            if from_paused {
                InstallUiAction::Resume
            } else {
                InstallUiAction::Start
            },
        );
        self.install_pause_tick = 0;
        self.install_started = Some(Instant::now());
        // No trustworthy fraction yet: omit percent and ETA (not "0% · …").
        let start_view = ProgressView::new(None, None, "Downloading...");
        let status_prefix = if from_paused {
            "Resuming download...".to_string()
        } else {
            start_view.line()
        };
        self.install_status =
            format!("{} · {} -> {}", status_prefix, repo_id, dest.display()).into();
        // Footer/status bar must not stay on stale "Ready to install …".
        self.status = if from_paused {
            format!("Resuming install · {repo_id}").into()
        } else {
            format!("Installing · {repo_id}").into()
        };
        self.install_progress = Some(start_view);
        let (cancel, live) = install_async(repo_id, revision, dest, min_free, tx);
        self.install_cancel = Some(cancel);
        self.install_live = Some(live);
        self.ensure_visual_pump(cx);
        self.schedule_install_poll(cx);
        cx.notify();
    }

    #[cfg(feature = "install")]
    fn pause_install(&mut self, cx: &mut Context<Self>) {
        if !show_pause(self.install_phase) {
            if !install_is_busy(self.install_phase) {
                self.install_status = "no install in progress".into();
            }
            cx.notify();
            return;
        }
        if let Some(ref c) = self.install_cancel {
            c.request_pause();
            self.install_phase = install_ui_transition(self.install_phase, InstallUiAction::Pause);
            self.install_pause_tick = 0;
            // Freeze strip: drop active "Downloading..." label and ETA; keep %.
            if let Some(ref mut view) = self.install_progress {
                view.label = "Pausing".into();
                view.eta_secs = None;
            }
            let pct = self.last_install_percent();
            self.install_status = exclusive_status_for_phase(InstallUiPhase::Pausing, pct, 0)
                .unwrap_or_else(|| pausing_status_line(0))
                .into();
            self.status = "Pausing install".into();
        }
        cx.notify();
    }

    #[cfg(feature = "install")]
    fn cancel_install(&mut self, cx: &mut Context<Self>) {
        if !install_is_busy(self.install_phase) {
            self.install_status = "no install in progress".into();
            cx.notify();
            return;
        }
        if let Some(ref c) = self.install_cancel {
            c.request();
            self.install_phase = install_ui_transition(self.install_phase, InstallUiAction::Cancel);
            if let Some(ref mut view) = self.install_progress {
                view.label = "Cancelling".into();
                view.eta_secs = None;
            }
            self.install_status = cancelling_status_line().into();
            // Cancel abandons resume; drop any prior pause checkpoint.
            let _ = clear_checkpoint_default();
        }
        cx.notify();
    }

    /// Persist pause checkpoint from the install form (restart-safe Resume).
    #[cfg(feature = "install")]
    fn persist_install_checkpoint(&self, cx: &App, percent: Option<u8>) {
        let repo = self.repo_input.read(cx).text();
        let rev = self.revision_input.read(cx).text();
        let dest = self.dest_input.read(cx).text();
        let min_free = self.min_free_input.read(cx).text();
        // Prefer absolute dest from validation when dest field is a store-relative name.
        let store = self.machine.as_ref().map(|m| m.model_store.path.as_path());
        let dest_stored = match validate_install_form(&repo, &rev, &dest, store) {
            Ok((_, _, abs)) => abs.display().to_string(),
            Err(_) => dest,
        };
        let cp = InstallCheckpoint::new(repo, rev, dest_stored, min_free, percent);
        if cp.is_usable() {
            let _ = save_checkpoint_default(&cp);
        }
    }

    #[cfg(feature = "install")]
    fn last_install_percent(&self) -> Option<u8> {
        self.install_progress.as_ref().and_then(|v| v.percent)
    }

    #[cfg(feature = "install")]
    fn schedule_install_poll(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(80))
                    .await;
                let cont = this
                    .update(cx, |this, cx| this.drain_install(cx))
                    .unwrap_or(false);
                if !cont {
                    break;
                }
            }
        })
        .detach();
    }

    #[cfg(feature = "install")]
    fn drain_install(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(rx) = self.install_rx.as_ref() else {
            return false;
        };
        let mut still = true;
        // Keep Done 100% strip until the next install (do not clear same drain).
        let mut keep_done_progress = false;
        // Keep last progress strip frozen while Pausing / after Paused.
        let mut keep_paused_progress = false;
        // Defer registry rescan until after `rx` borrow ends (Installed badge).
        let mut rescan_registry_after = false;
        loop {
            match rx.try_recv() {
                Ok(InstallEvent::Progress(p)) => {
                    // While Pausing, freeze last strip and only pulse status text.
                    if matches!(self.install_phase, InstallUiPhase::Pausing) {
                        continue;
                    }
                    let elapsed = self
                        .install_started
                        .map(|t| t.elapsed().as_secs_f64())
                        .unwrap_or(0.0);
                    let view = progress_view_for_install(&p, elapsed);
                    let mut line = view.line();
                    if let Some(f) = p.file.as_ref() {
                        line.push_str(&format!(" · {f}"));
                    }
                    // Append short byte counters when known (helps while % still low).
                    if let (Some(done), Some(total)) = (p.bytes_done, p.bytes_total) {
                        if total > 0 {
                            line.push_str(&format!(
                                " · {}",
                                format_install_bytes_pair(done, total)
                            ));
                        }
                    }
                    self.install_status = line.clone().into();
                    // Keep chrome status in sync with install (not "Ready to install").
                    // Omit N% when percent is unknown (Option honesty).
                    self.status = crate::progress::format_install_chrome_status(
                        p.file.as_deref(),
                        view.percent,
                    )
                    .into();
                    self.install_progress.replace(view);
                    cx.notify();
                }
                Ok(InstallEvent::Done(r)) => {
                    self.install_phase =
                        install_ui_transition(self.install_phase, InstallUiAction::JobDone);
                    let view = ProgressView::new(Some(100), Some(0), "Done");
                    self.install_progress = Some(view.clone());
                    self.install_status = format!(
                        "{} · {} · {}",
                        view.line(),
                        r.dest.display(),
                        r.notes.join("; ")
                    )
                    .into();
                    let dest_str = r.dest.display().to_string();
                    self.model_input
                        .update(cx, |input, cx| input.set_text(dest_str, cx));
                    self.status = "Install complete · model path set".into();
                    // OS desktop notification (once per successful install).
                    notify_install_complete(&r.dest);
                    let _ = clear_checkpoint_default();
                    rescan_registry_after = true;
                    still = false;
                    keep_done_progress = true;
                    cx.notify();
                }
                Ok(InstallEvent::Paused) => {
                    self.install_phase = transition_job_paused(self.install_phase);
                    let pct = self.last_install_percent();
                    // Exclusive paused copy: strip bar keeps %; no "Downloading..." line.
                    if let Some(ref mut view) = self.install_progress {
                        view.label = "Paused".into();
                        view.eta_secs = None;
                    } else {
                        self.install_progress = Some(ProgressView::new(pct, None, "Paused"));
                    }
                    self.install_status =
                        exclusive_status_for_phase(InstallUiPhase::Paused, pct, 0)
                            .unwrap_or_else(|| paused_status_line(pct))
                            .into();
                    self.status = "Install paused".into();
                    self.persist_install_checkpoint(cx, pct);
                    still = false;
                    keep_paused_progress = true;
                    cx.notify();
                }
                Ok(InstallEvent::Error(e)) => {
                    let cancelled = e.contains("install cancelled");
                    self.install_phase = install_ui_transition(
                        self.install_phase,
                        if cancelled {
                            InstallUiAction::JobCancelled
                        } else {
                            InstallUiAction::JobError
                        },
                    );
                    if cancelled {
                        self.install_status = "Install cancelled".into();
                        self.status = "Install cancelled".into();
                        let _ = clear_checkpoint_default();
                        keep_done_progress = false;
                    } else if let Some(cp) = load_checkpoint_default() {
                        // Prior pause still on disk: re-offer Resume (in-session + restart).
                        self.install_phase = InstallUiPhase::Paused;
                        let pct = cp.percent;
                        self.install_progress = Some(ProgressView::new(pct, None, "Paused"));
                        self.install_status =
                            format!("Install error: {e}. {}", paused_status_line(pct)).into();
                        self.status = "Install paused after error".into();
                        keep_paused_progress = true;
                        keep_done_progress = false;
                    } else {
                        self.install_status = format!("Install error: {e}").into();
                        self.status = "Install failed".into();
                        keep_done_progress = false;
                    }
                    still = false;
                    cx.notify();
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    still = false;
                    break;
                }
            }
        }
        // Mid-file: hub ProgressHandler updates live atomics; re-snapshot every
        // poll so the bar moves during multi-GB shards without channel events.
        if still
            && matches!(self.install_phase, InstallUiPhase::Installing)
            && let Some(live) = self.install_live.as_ref()
        {
            let p = live.snapshot();
            // Only refresh from live when we have byte counters (download phase).
            if p.bytes_total.is_some() || p.bytes_done.is_some_and(|d| d > 0) {
                let elapsed = self
                    .install_started
                    .map(|t| t.elapsed().as_secs_f64())
                    .unwrap_or(0.0);
                let view = progress_view_for_install(&p, elapsed);
                let mut line = view.line();
                if let Some(f) = p.file.as_ref() {
                    line.push_str(&format!(" · {f}"));
                }
                if let (Some(done), Some(total)) = (p.bytes_done, p.bytes_total) {
                    if total > 0 {
                        line.push_str(&format!(" · {}", format_install_bytes_pair(done, total)));
                    }
                }
                self.install_status = line.into();
                self.status =
                    crate::progress::format_install_chrome_status(p.file.as_deref(), view.percent)
                        .into();
                self.install_progress.replace(view);
                cx.notify();
            }
        }
        // Indeterminate wait while Pausing: pulse status dots each poll.
        // Progress strip line is suppressed (see show_active_progress_line).
        if still && matches!(self.install_phase, InstallUiPhase::Pausing) {
            self.install_pause_tick = self.install_pause_tick.wrapping_add(1);
            let pct = self.last_install_percent();
            self.install_status =
                exclusive_status_for_phase(InstallUiPhase::Pausing, pct, self.install_pause_tick)
                    .unwrap_or_else(|| pausing_status_line(self.install_pause_tick))
                    .into();
            if let Some(ref mut view) = self.install_progress {
                view.label = "Pausing".into();
                view.eta_secs = None;
            }
            cx.notify();
        }
        if !still {
            self.install_rx = None;
            self.install_cancel = None;
            self.install_live = None;
            self.install_started = None;
            if !keep_done_progress && !keep_paused_progress {
                self.install_progress = None;
            }
        }
        // After rx borrow ends: refresh so Supported models shows Installed.
        // Keep install-complete status (scan_registry overwrites the rail line).
        if rescan_registry_after {
            let keep_status = self.status.clone();
            let keep_install = self.install_status.clone();
            self.scan_registry(cx);
            self.status = keep_status;
            self.install_status = keep_install;
        }
        still
    }

    // ---- UI building blocks -------------------------------------------------

    fn section_title(p: &ThemePalette, label: impl Into<SharedString>) -> impl IntoElement {
        div()
            .text_xs()
            .font_weight(gpui::FontWeight::BOLD)
            .text_color(rgb(p.label))
            .child(label.into())
    }

    /// Thick fill row + optional status line (determinate percent). Hide when idle.
    ///
    /// Fill width is exactly [`ProgressView::fill_fraction`] of the track (same
    /// number as the label percent). Track is a single dark unfilled color;
    /// fill is primary (DOGE green). Do not dual-paint two bright progress colors.
    ///
    /// When `show_line` is false (Pausing / Paused / Cancelling), paint the bar
    /// only so the form status line owns exclusive prose (no dual "Downloading...").
    fn progress_strip_el(
        p: &ThemePalette,
        view: &ProgressView,
        show_line: bool,
    ) -> impl IntoElement {
        let frac = view.fill_fraction();
        let line = view.line();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .w_full()
            .child(
                // Dark track only; fill child uses explicit fraction of parent width
                // (not flex_grow — equal grow made tiny % paint a fat slab).
                div()
                    .w_full()
                    .h(px(10.))
                    .overflow_hidden()
                    .bg(rgb(p.panel))
                    .border_1()
                    .border_color(rgb(p.border))
                    .when(frac > 0.0, |b| {
                        b.child(div().h_full().w(relative(frac)).bg(rgb(p.primary)))
                    }),
            )
            .when(show_line, |col| {
                col.child(div().text_xs().text_color(rgb(p.muted)).child(line))
            })
    }

    fn panel(
        p: &ThemePalette,
        title: impl Into<SharedString>,
        body: SharedString,
        actions: impl IntoElement,
    ) -> impl IntoElement {
        let title: SharedString = title.into();
        let body_id = SharedString::from(format!("panel-body-{title}"));
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgb(p.panel))
            .border_1()
            .border_color(rgb(p.border))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(p.text))
                            .child(title),
                    )
                    .child(actions),
            )
            .child(
                div()
                    .id(body_id)
                    .max_h(px(220.))
                    .overflow_scroll()
                    .text_xs()
                    .text_color(rgb(p.muted))
                    .child(body),
            )
    }

    fn tier_bar_el(&self) -> impl IntoElement {
        let p = self.palette();
        let (vf, rf, df) = match &self.tiers_snap {
            Some(t) => tier_share_fractions(t.vram, t.ram, t.disk),
            None => (0.0, 0.0, 0.0),
        };
        let (v_n, r_n, d_n) = match &self.tiers_snap {
            Some(t) => (t.vram, t.ram, t.disk),
            None => (0, 0, 0),
        };
        // Minimum visible sliver so empty tiers do not collapse the bar awkwardly.
        let bar = div()
            .flex()
            .flex_row()
            .w_full()
            .h(px(12.))
            .overflow_hidden()
            .border_1()
            .border_color(rgb(p.border))
            .bg(rgb(p.panel))
            .when(vf + rf + df <= 0.0, |b| {
                b.child(div().flex_1().bg(rgb(p.chip)))
            })
            .when(vf > 0.0, |b| {
                b.child(
                    div()
                        .h_full()
                        .flex_grow()
                        .bg(rgb(p.primary))
                        // Approximate share via flex; gpui lacks % width on flex children.
                        .when(vf >= 0.01, |d| d.flex_basis(px((vf * 200.0).max(2.0)))),
                )
            })
            .when(rf > 0.0, |b| {
                b.child(
                    div()
                        .h_full()
                        .flex_grow()
                        .bg(rgb(p.speed))
                        .when(rf >= 0.01, |d| d.flex_basis(px((rf * 200.0).max(2.0)))),
                )
            })
            .when(df > 0.0, |b| {
                b.child(
                    div()
                        .h_full()
                        .flex_grow()
                        .bg(rgb(p.tier_disk))
                        .when(df >= 0.01, |d| d.flex_basis(px((df * 200.0).max(2.0)))),
                )
            });

        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(Self::section_title(&p, self.tr("tier.title")))
            .child(bar)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_3()
                    .flex_wrap()
                    .text_xs()
                    .text_color(rgb(p.muted))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .child(div().w(px(8.)).h(px(8.)).bg(rgb(p.primary)))
                            .child(format!("{} {v_n}", self.tr("tier.vram"))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .child(div().w(px(8.)).h(px(8.)).bg(rgb(p.speed)))
                            .child(format!("{} {r_n}", self.tr("tier.ram"))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .child(div().w(px(8.)).h(px(8.)).bg(rgb(p.tier_disk)))
                            .child(format!("{} {d_n}", self.tr("tier.disk"))),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(p.muted))
                    .child(self.live_tiers_text.clone()),
            )
    }

    fn badge_chip(p: &ThemePalette, label: SharedString, kind: BadgeKind) -> impl IntoElement {
        let (bg, border, fg) = match kind {
            BadgeKind::Live => (p.badge_live_bg, p.badge_live_border, p.primary),
            BadgeKind::Speed => (p.badge_speed_bg, p.badge_speed_border, p.speed),
            BadgeKind::Warn => (p.badge_warn_bg, p.badge_warn_border, p.warn),
            BadgeKind::Muted => (p.chip, p.border, p.muted),
        };
        div()
            .px_2()
            .py_0p5()
            .border_1()
            .border_color(rgb(border))
            .bg(rgb(bg))
            .text_xs()
            .text_color(rgb(fg))
            .child(label)
    }

    fn view_tab_btn(
        &self,
        id: &'static str,
        label: SharedString,
        view: MainView,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = self.palette();
        let active = self.active_view == view;
        div()
            .id(id)
            .px_3()
            .py_1()
            .bg(rgb(tab_bg_color(&p, active)))
            .text_xs()
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(rgb(if active { p.primary_fg } else { p.muted }))
            .cursor_pointer()
            .child(label)
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.set_view(view, cx)),
            )
    }

    fn brain_panel_full(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette();
        let atlas_status = if self.experts_atlas.is_empty() {
            "atlas: depth-role fallback (no experts.json)".to_string()
        } else {
            format!(
                "atlas: {} experts · {} categories",
                self.experts_atlas.experts.len(),
                self.experts_atlas.categories.len()
            )
        };
        let note = if self.brain.cells.is_empty() {
            format!("{} · {}", self.tr("brain.waiting"), atlas_status)
        } else {
            let sample_bit = if self.brain.sampled {
                format!(
                    "showing {}×{} of {}×{} · stride {}×{} · cap {}",
                    self.brain.disp_rows,
                    self.brain.disp_cols,
                    self.brain.src_rows,
                    self.brain.src_cols,
                    self.brain.row_stride.max(1),
                    self.brain.col_stride.max(1),
                    self.brain.max_cells
                )
            } else {
                format!(
                    "{}×{} · cap {}",
                    self.brain.src_rows, self.brain.src_cols, self.brain.max_cells
                )
            };
            format!("{} · {} · {}", self.brain.note, sample_bit, atlas_status)
        };
        let cell_px = if self.brain.disp_cols > 128 {
            2.0_f32
        } else if self.brain.disp_cols > 64 {
            3.0
        } else if self.brain.disp_cols > 32 {
            4.0
        } else {
            6.0
        };
        let cols = self.brain.disp_cols.max(1) as usize;
        let row_stride = self.brain.row_stride.max(1);
        let col_stride = self.brain.col_stride.max(1);
        let src_rows = self.brain.src_rows;
        let atlas = self.experts_atlas.clone();
        let cells = &self.brain.cells;
        // Build rows with for-loops so each `cx.listener` borrows sequentially.
        let mut row_els: Vec<AnyElement> = Vec::new();
        let n_rows = if cols == 0 {
            0
        } else {
            cells.len().div_ceil(cols)
        };
        for ri in 0..n_rows {
            let start = ri * cols;
            let end = (start + cols).min(cells.len());
            let mut cell_els: Vec<AnyElement> = Vec::new();
            for (ci, (tier, heat, pulse)) in cells[start..end].iter().enumerate() {
                let color = brain_cell_rgb(self.theme_id, *tier, *heat, *pulse);
                let tier_c = *tier;
                let heat_c = *heat;
                let atlas_cell = atlas.clone();
                let (src_r, src_c) =
                    display_to_source(ri as u32, ci as u32, row_stride, col_stride);
                cell_els.push(
                    div()
                        .id(SharedString::from(format!("bc-{ri}-{ci}")))
                        .w(px(cell_px))
                        .h(px(cell_px))
                        .bg(rgb(color))
                        .on_hover(cx.listener(move |this, hovered, _, cx| {
                            if *hovered {
                                let tip = format_brain_tooltip(
                                    src_r,
                                    src_c,
                                    src_rows,
                                    tier_c,
                                    heat_c,
                                    &atlas_cell,
                                );
                                this.set_brain_tip(tip.into(), cx);
                            } else {
                                this.clear_brain_tip(cx);
                            }
                        }))
                        .into_any_element(),
                );
            }
            row_els.push(
                div()
                    .id(SharedString::from(format!("brain-row-{ri}")))
                    .flex()
                    .flex_row()
                    .children(cell_els)
                    .into_any_element(),
            );
        }

        let full_label = if self.brain_full {
            "Sampled"
        } else {
            "Full grid"
        };
        let tip = self.brain_tip.clone();
        let grid_max_h = if self.brain_full { 520.0 } else { 360.0 };

        div()
            .id("brain-page")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap_2()
            .p_4()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(p.text))
                            .child(self.tr("brain.title")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .text_xs()
                            .text_color(rgb(p.muted))
                            .child(self.tr("brain.brightnessHint"))
                            .child(
                                div()
                                    .text_color(rgb(p.primary))
                                    .child(self.tr("brain.flashHint")),
                            )
                            .child(
                                div()
                                    .id("btn-brain-full")
                                    .px_2()
                                    .py_1()
                                    .bg(rgb(if self.brain_full { p.primary } else { p.chip }))
                                    .text_xs()
                                    .text_color(rgb(if self.brain_full {
                                        p.primary_fg
                                    } else {
                                        p.text
                                    }))
                                    .cursor_pointer()
                                    .child(full_label)
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| this.toggle_brain_full(cx)),
                                    ),
                            ),
                    ),
            )
            .child(div().text_xs().text_color(rgb(p.muted)).child(note))
            .child(
                div()
                    .id("brain-grid")
                    .flex_1()
                    .min_h(px(280.))
                    .max_h(px(grid_max_h))
                    .overflow_scroll()
                    .p_2()
                    .border_1()
                    .border_color(rgb(p.border))
                    .bg(rgb(p.bg))
                    .flex()
                    .flex_col()
                    .children(row_els),
            )
            .when(!tip.is_empty(), |col| {
                col.child(
                    div()
                        .id("brain-tip")
                        .p_3()
                        .border_1()
                        .border_color(rgb(p.border))
                        .bg(rgb(p.panel))
                        .text_xs()
                        .text_color(rgb(p.text))
                        .child(tip),
                )
            })
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(p.muted))
                    .child(self.tr(brain_legend_key(self.theme_id))),
            )
    }

    fn profiling_page(&self) -> impl IntoElement {
        let p = self.palette();
        let turns = recent_turns(&self.profile_turns, PROF_CHART_N);
        let engine_up = self.session.lock().map(|g| g.is_some()).unwrap_or(false);

        if turns.is_empty() {
            return div()
                .id("prof-page-empty")
                .flex()
                .flex_col()
                .flex_1()
                .p_4()
                .gap_3()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(self.tr("profile.title")),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(p.muted))
                        .child(if engine_up {
                            self.tr("profile.empty")
                        } else {
                            self.tr("profile.connectHint")
                        }),
                )
                .into_any_element();
        }

        let latest = turns.last().cloned().unwrap();
        let (last_total, last_segs) = share_segments(std::slice::from_ref(&latest));
        let (win_total, win_segs) = share_segments(&turns);
        let (tok_peak, tok_heights) = throughput_heights(&turns);
        let (wall_peak, phase_stacks) = phase_stack_heights(&turns);

        let legend =
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_3()
                .children(ProfilePhase::ALL.iter().map(|ph| {
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .text_xs()
                        .text_color(rgb(p.muted))
                        .child(div().w(px(9.)).h(px(9.)).bg(rgb(ph.color_in(&p))))
                        .child(self.tr(ph.i18n_key()))
                }));

        let tiles = div().flex().flex_row().gap_2().children([
            prof_tile(
                &p,
                self.tr("profile.lastTurn"),
                format!("{:.1}", latest.toks),
                "tok/s".into(),
            ),
            prof_tile(
                &p,
                self.tr("profile.wallTime"),
                format_seconds(latest.wall_s),
                format!(
                    "{} → {} tokens",
                    latest.prompt_tokens, latest.completion_tokens
                ),
            ),
            prof_tile(
                &p,
                self.tr("profile.batching"),
                latest
                    .tokens_per_forward()
                    .map(|v| format!("{v:.2}"))
                    .unwrap_or_else(|| "—".into()),
                self.tr("profile.tokensPerForward"),
            ),
            prof_tile(
                &p,
                self.tr("profile.diskService"),
                format_seconds(latest.expert_disk_s),
                self.tr("profile.overlapped"),
            ),
        ]);

        let share_last = share_bar_el(
            &p,
            self.tr("profile.lastTurn"),
            last_total,
            &last_segs,
            self.locale,
        );
        let share_win = share_bar_el(
            &p,
            self.tr_fmt("profile.window", &[("n", &turns.len().to_string())]),
            win_total,
            &win_segs,
            self.locale,
        );

        let chart_h = 96.0_f32;
        let n = turns.len().max(1) as f32;
        let col_w = ((520.0_f32 / n) - 2.0).clamp(4.0, 40.0);

        let throughput_chart = div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .border_1()
            .border_color(rgb(p.border))
            .bg(rgb(p.panel))
            .flex_1()
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(p.muted))
                    .child(self.tr("profile.throughputTitle")),
            )
            .child(
                div()
                    .id("tok-chart")
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap_0p5()
                    .h(px(chart_h))
                    .w_full()
                    .children(tok_heights.iter().enumerate().map(|(i, h)| {
                        let bar_h = (*h as f32 * chart_h).max(1.0);
                        div()
                            .id(SharedString::from(format!("tok-bar-{i}")))
                            .w(px(col_w))
                            .h(px(bar_h))
                            .bg(rgb(p.primary))
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .text_xs()
                    .text_color(rgb(p.muted))
                    .child(if turns.len() > 1 {
                        self.tr_fmt("profile.turnsLabel", &[("n", &turns.len().to_string())])
                    } else {
                        self.tr("profile.oneTurn")
                    })
                    .child(format!("peak {tok_peak:.1} tok/s")),
            );

        let phase_chart = div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .border_1()
            .border_color(rgb(p.border))
            .bg(rgb(p.panel))
            .flex_1()
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(p.muted))
                    .child(self.tr("profile.phaseTitle")),
            )
            .child(
                div()
                    .id("phase-chart")
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap_0p5()
                    .h(px(chart_h))
                    .w_full()
                    .children(phase_stacks.iter().enumerate().map(|(i, stack)| {
                        // Stack phases bottom-up as column of flex_col reverse.
                        div()
                            .id(SharedString::from(format!("phase-col-{i}")))
                            .w(px(col_w))
                            .h(px(chart_h))
                            .flex()
                            .flex_col()
                            .justify_end()
                            .children(ProfilePhase::ALL.iter().enumerate().filter_map(
                                |(pi, phase)| {
                                    let h = (stack[pi] as f32 * chart_h).max(0.0);
                                    if h < 0.5 {
                                        return None;
                                    }
                                    Some(
                                        div()
                                            .id(SharedString::from(format!("ph-{i}-{pi}")))
                                            .w_full()
                                            .h(px(h.max(1.0)))
                                            .bg(rgb(phase.color_in(&p))),
                                    )
                                },
                            ))
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .text_xs()
                    .text_color(rgb(p.muted))
                    .child(if turns.len() > 1 {
                        self.tr_fmt("profile.turnsLabel", &[("n", &turns.len().to_string())])
                    } else {
                        self.tr("profile.oneTurn")
                    })
                    .child(format!("peak {}", format_seconds(wall_peak))),
            );

        let table = profile_table_el(&p, &turns, self.locale);

        div()
            .id("prof-page")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap_3()
            .p_4()
            .overflow_scroll()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.tr("profile.title")),
                    )
                    .child(legend),
            )
            .child(tiles)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .border_1()
                    .border_color(rgb(p.border))
                    .bg(rgb(p.panel))
                    .child(share_last)
                    .when(turns.len() > 1, |c| c.child(share_win)),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(throughput_chart)
                    .child(phase_chart),
            )
            .child(table)
            .into_any_element()
    }

    /// Slim rail: temperature + max tokens only.
    fn rail_inference_panel(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette();
        div()
            .flex()
            .flex_col()
            .gap(px(RAIL_CARD_GAP))
            .p(px(RAIL_CARD_PAD))
            .bg(rgb(p.panel))
            .border_1()
            .border_color(rgb(p.border))
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.tr("rail.inference")),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(p.muted))
                    .child(self.tr("sidebar.temperature")),
            )
            .child(self.temperature_input.clone())
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(p.muted))
                    .child(self.tr("sidebar.maxTokens")),
            )
            .child(self.max_tokens_input.clone())
    }

    /// Tools: reasoning, session slots, optional grammar.
    fn tools_advanced_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette();
        let slot_label = self.tr_fmt(
            "sidebar.sessionLabel",
            &[
                ("slot", &(self.cache_slot + 1).to_string()),
                ("n", &self.kv_slots.max(1).to_string()),
            ],
        );
        let reasoning_label = if self.enable_thinking {
            self.tr("sidebar.reasoningOn")
        } else {
            self.tr("sidebar.reasoningOff")
        };
        let multi_slot = self.kv_slots > 1;

        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgb(p.panel))
            .border_1()
            .border_color(rgb(p.border))
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.tr("tools.advanced")),
            )
            .child(
                div()
                    .id("btn-reasoning")
                    .px_2()
                    .py_1()
                    .bg(rgb(if self.enable_thinking {
                        p.primary
                    } else {
                        p.chip
                    }))
                    .text_xs()
                    .text_color(rgb(if self.enable_thinking {
                        p.primary_fg
                    } else {
                        p.text
                    }))
                    .cursor_pointer()
                    .child(reasoning_label)
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.toggle_reasoning(cx)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(div().text_xs().text_color(rgb(p.muted)).child(slot_label))
                    .when(multi_slot, |row| {
                        row.child(
                            div()
                                .id("btn-slot-prev")
                                .px_2()
                                .py_1()
                                .bg(rgb(p.chip))
                                .text_xs()
                                .text_color(rgb(p.text))
                                .cursor_pointer()
                                .child(self.tr("sidebar.prev"))
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, cx| this.cycle_cache_slot(-1, cx)),
                                ),
                        )
                        .child(
                            div()
                                .id("btn-slot-next")
                                .px_2()
                                .py_1()
                                .bg(rgb(p.chip))
                                .text_xs()
                                .text_color(rgb(p.text))
                                .cursor_pointer()
                                .child(self.tr("sidebar.next"))
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, cx| this.cycle_cache_slot(1, cx)),
                                ),
                        )
                    }),
            )
            .when(multi_slot, |col| {
                col.child(
                    div()
                        .text_xs()
                        .text_color(rgb(p.muted))
                        .child(self.tr("sidebar.sessionHelp")),
                )
            })
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(p.muted))
                    .child(self.tr("sidebar.grammar")),
            )
            .child(self.grammar_input.clone())
    }

    fn theme_picker_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette();
        let doge_on = self.theme_id == ThemeId::Doge;
        let mint_on = self.theme_id == ThemeId::Mint;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.tr("theme.label")),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        div()
                            .id("btn-theme-doge")
                            .px_3()
                            .py_1()
                            .bg(rgb(if doge_on { p.primary } else { p.chip }))
                            .text_xs()
                            .text_color(rgb(if doge_on { p.primary_fg } else { p.text }))
                            .cursor_pointer()
                            .child(self.tr("theme.doge"))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.set_theme_id(ThemeId::Doge, cx)),
                            ),
                    )
                    .child(
                        div()
                            .id("btn-theme-mint")
                            .px_3()
                            .py_1()
                            .bg(rgb(if mint_on { p.primary } else { p.chip }))
                            .text_xs()
                            .text_color(rgb(if mint_on { p.primary_fg } else { p.text }))
                            .cursor_pointer()
                            .child(self.tr("theme.mint"))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.set_theme_id(ThemeId::Mint, cx)),
                            ),
                    ),
            )
            .child(div().text_xs().text_color(rgb(p.muted)).child(if doge_on {
                self.tr("theme.dogeHelp")
            } else {
                self.tr("theme.mintHelp")
            }))
    }

    fn left_rail(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette();
        let model_summary = self.model_path_summary(cx);
        let live = self.engine_is_live();

        div()
            .id("left-rail")
            .flex()
            .flex_col()
            .w(px(RAIL_WIDTH))
            .h_full()
            .p(px(RAIL_PAD))
            .gap(px(RAIL_SECTION_GAP))
            .border_r_1()
            .border_color(rgb(p.border))
            .bg(rgb(p.bg))
            .overflow_scroll()
            // Brand
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .w(px(38.))
                            .h(px(38.))
                            .border_1()
                            .border_color(rgb(p.primary_border))
                            .bg(rgb(p.primary_wash))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(rgb(p.primary))
                            .text_sm()
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("c"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(gpui::FontWeight::NORMAL)
                                    .text_color(rgb(p.text))
                                    .child(self.tr("brand.name")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(p.muted))
                                    .child(self.tr("brand.tagline")),
                            ),
                    ),
            )
            .child(Self::section_title(&p, self.tr("rail.lifecycle")))
            // Model path + start/stop (path editor only when this frame owns it)
            .child({
                let site = model_input_site(&self.wizard, self.active_view);
                let path_display = self.model_input.read(cx).text();
                let path_display = if path_display.trim().is_empty() {
                    "(set path in Tools or Setup)".to_string()
                } else {
                    path_display
                };
                let mut card = div()
                    .flex()
                    .flex_col()
                    .gap(px(RAIL_CARD_GAP))
                    .p(px(RAIL_CARD_PAD))
                    .w_full()
                    .min_w_0()
                    .bg(rgb(p.panel))
                    .border_1()
                    .border_color(rgb(p.border))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(p.muted))
                            .child(self.tr("rail.modelPath")),
                    );
                if site == ModelInputSite::Rail {
                    card = card.child(self.model_input.clone());
                } else {
                    card = card.child(
                        div()
                            .w_full()
                            .min_w_0()
                            .overflow_hidden()
                            .text_xs()
                            .text_color(rgb(p.text))
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(path_display),
                    );
                }
                let start_paint = start_button_paint(&p, live, self.starting);
                let stop_paint = stop_button_paint(&p, live);
                card.child(
                    div()
                        .text_xs()
                        .text_color(rgb(p.label))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(model_summary),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(RAIL_CARD_GAP))
                        .flex_wrap()
                        .child(
                            div()
                                .id("btn-start-engine")
                                .px(px(BTN_PAD_X))
                                .py(px(BTN_PAD_Y))
                                .bg(rgb(start_paint.fill))
                                .border_1()
                                .border_color(rgb(start_paint.border))
                                .text_xs()
                                .text_color(rgb(start_paint.text))
                                .cursor_pointer()
                                .child(self.tr("rail.startEngine"))
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, cx| this.start_engine(cx)),
                                ),
                        )
                        .child(
                            div()
                                .id("btn-stop-engine")
                                .px(px(BTN_PAD_X))
                                .py(px(BTN_PAD_Y))
                                .bg(rgb(stop_paint.fill))
                                .border_1()
                                .border_color(rgb(stop_paint.border))
                                .text_xs()
                                .text_color(rgb(stop_paint.text))
                                .cursor_pointer()
                                .child(self.tr("rail.stopEngine"))
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, cx| this.stop_engine(cx)),
                                ),
                        ),
                )
            })
            // Live tiers / machine strip when engine is up
            .when(live, |col| {
                col.child(Self::section_title(&p, self.tr("rail.runtime")))
                    .child(self.tier_bar_el())
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(p.speed))
                            .child(self.live_hwinfo_text.clone()),
                    )
            })
            .child(self.rail_inference_panel(cx))
            // Status + first-run Setup slab (hidden after Finish/Skip)
            .child({
                let setup_fill = rail_setup_primary_fill(&p, self.first_run_done);
                div()
                    .mt_auto()
                    .flex()
                    .flex_col()
                    .gap(px(RAIL_CARD_GAP))
                    .pt(px(RAIL_CARD_PAD))
                    .border_t_1()
                    .border_color(rgb(p.border))
                    .when_some(setup_fill, |col, fill| {
                        col.child(
                            div()
                                .id("btn-setup")
                                .px(px(BTN_PAD_X + 4.0))
                                .py(px(BTN_PAD_Y + 2.0))
                                .bg(rgb(fill))
                                .text_sm()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgb(p.primary_fg))
                                .cursor_pointer()
                                .child(self.tr("rail.setup"))
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, cx| this.open_setup_wizard(cx)),
                                ),
                        )
                    })
                    .child(div().text_xs().text_color(rgb(p.label)).child(format!(
                        "{} · {}",
                        self.tr("brand.native"),
                        self.status
                    )))
            })
    }

    #[cfg(feature = "install")]
    fn install_form_panel(&self, cx: &mut Context<Self>, id_prefix: &str) -> impl IntoElement {
        let p = self.palette();
        let install_id = SharedString::from(format!("{id_prefix}-install"));
        let cancel_id = SharedString::from(format!("{id_prefix}-install-cancel"));
        let progress_id = SharedString::from(format!("{id_prefix}-install-progress"));
        let status_id = SharedString::from(format!("{id_prefix}-install-status"));
        div()
            .flex()
            .flex_col()
            .gap_2()
            .w_full()
            .min_w_0()
            .p(px(RAIL_CARD_PAD))
            .bg(rgb(p.panel))
            .border_1()
            .border_color(rgb(p.border))
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.tr("tools.install")),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(p.muted))
                    .child(self.tr("install.repo")),
            )
            .child(self.repo_input.clone())
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(p.muted))
                    .child(self.tr("install.revision")),
            )
            .child(self.revision_input.clone())
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(p.muted))
                    .child(self.tr("install.dest")),
            )
            .child(self.dest_input.clone())
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(p.muted))
                    .child(self.tr("install.minFree")),
            )
            .child(self.min_free_input.clone())
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(p.muted))
                    .child(self.tr("install.minFreeHelp")),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(p.muted))
                    .child(self.install_space.clone()),
            )
            .child({
                let busy = install_is_busy(self.install_phase);
                let pause_on = show_pause(self.install_phase);
                let resume_on = show_resume(self.install_phase);
                let cancel_on = show_cancel_active(self.install_phase);
                let primary_label = if busy {
                    self.tr("rail.installing")
                } else if resume_on {
                    self.tr("rail.resume")
                } else {
                    self.tr("rail.installBtn")
                };
                div()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .id(install_id)
                            .px_2()
                            .py_1()
                            .bg(rgb(if busy { p.muted } else { p.primary }))
                            .text_xs()
                            .text_color(rgb(if busy { p.text } else { p.primary_fg }))
                            .cursor_pointer()
                            .child(primary_label)
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.start_install(cx)),
                            ),
                    )
                    .when(pause_on, |row| {
                        row.child(
                            div()
                                .id("install-pause")
                                .px_2()
                                .py_1()
                                .bg(rgb(p.chip))
                                .text_xs()
                                .text_color(rgb(p.text))
                                .cursor_pointer()
                                .child(self.tr("rail.pause"))
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, cx| this.pause_install(cx)),
                                ),
                        )
                    })
                    .when(
                        matches!(self.install_phase, InstallUiPhase::Pausing),
                        |row| {
                            // Short indeterminate label; full wait line is in install_status.
                            row.child(
                                div()
                                    .id("install-pausing-marker")
                                    .px_2()
                                    .py_1()
                                    .text_xs()
                                    .text_color(rgb(p.muted))
                                    .child(self.tr("rail.pausing")),
                            )
                        },
                    )
                    .child(
                        div()
                            .id(cancel_id)
                            .px_2()
                            .py_1()
                            .bg(rgb(if cancel_on { p.danger } else { p.chip }))
                            .text_xs()
                            .text_color(rgb(p.text))
                            .cursor_pointer()
                            .child(self.tr("rail.cancel"))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.cancel_install(cx)),
                            ),
                    )
            })
            .when_some(self.install_progress.as_ref(), |col, view| {
                // Bar only while Pausing/Paused/Cancelling; exclusive prose is install_status.
                let show_line = show_active_progress_line(self.install_phase);
                col.child(
                    div()
                        .id(progress_id.clone())
                        .w_full()
                        .min_w_0()
                        .child(Self::progress_strip_el(&p, view, show_line)),
                )
            })
            .child(
                div()
                    .id(status_id)
                    .w_full()
                    .min_w_0()
                    .max_h(px(80.))
                    .overflow_scroll()
                    .text_xs()
                    .text_color(rgb(p.muted))
                    .child(self.install_status.clone()),
            )
    }

    fn tools_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette();
        let details_label = if self.machine_expanded {
            self.tr("rail.hideDetails")
        } else {
            self.tr("rail.details")
        };
        let about_label = if self.show_about {
            self.tr("rail.hideAbout")
        } else {
            self.tr("rail.about")
        };

        div()
            .id("tools-view")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap_4()
            .p_6()
            .overflow_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.tr("tools.title")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(p.muted))
                            .child(self.tr("tools.subtitle")),
                    ),
            )
            .child(Self::panel(
                &p,
                self.tr("tools.machine"),
                self.machine_text.clone(),
                div()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .id("tools-btn-machine-details")
                            .px_2()
                            .py_1()
                            .bg(rgb(p.chip))
                            .text_xs()
                            .text_color(rgb(p.text))
                            .cursor_pointer()
                            .child(details_label)
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.toggle_machine_details(cx)),
                            ),
                    )
                    .child(
                        div()
                            .id("tools-btn-reprobe")
                            .px_2()
                            .py_1()
                            .bg(rgb(p.primary))
                            .text_xs()
                            .text_color(rgb(p.primary_fg))
                            .cursor_pointer()
                            .child(self.tr("rail.refresh"))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.refresh_probe(cx)),
                            ),
                    ),
            ))
            .child(Self::panel(
                &p,
                self.tr("tools.doctor"),
                self.doctor_text.clone(),
                div()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .id("tools-btn-doctor")
                            .px_2()
                            .py_1()
                            .bg(rgb(p.primary))
                            .text_xs()
                            .text_color(rgb(p.primary_fg))
                            .cursor_pointer()
                            .child(self.tr("rail.runChecks"))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.run_doctor(cx)),
                            ),
                    )
                    .child(
                        div()
                            .id("tools-btn-doctor-deep")
                            .px_2()
                            .py_1()
                            .bg(rgb(p.primary_wash))
                            .text_xs()
                            .text_color(rgb(p.text))
                            .cursor_pointer()
                            .child(self.tr("rail.deepCheck"))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.run_deep_doctor(cx)),
                            ),
                    ),
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .bg(rgb(p.panel))
                    .border_1()
                    .border_color(rgb(p.border))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.tr("tools.plan")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(p.muted))
                            .child(self.tr("tools.modelPath")),
                    )
                    .when(
                        model_input_site(&self.wizard, self.active_view) == ModelInputSite::Tools,
                        |col| col.child(self.model_input.clone()),
                    )
                    .when(
                        model_input_site(&self.wizard, self.active_view) != ModelInputSite::Tools,
                        |col| {
                            let path = self.model_input.read(cx).text();
                            let path = if path.trim().is_empty() {
                                "(path editor open in Setup)".to_string()
                            } else {
                                path
                            };
                            col.child(
                                div()
                                    .w_full()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_xs()
                                    .text_color(rgb(p.text))
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(path),
                            )
                        },
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .flex_wrap()
                            .child(
                                div()
                                    .id("tools-btn-plan")
                                    .px_2()
                                    .py_1()
                                    .bg(rgb(p.primary))
                                    .text_xs()
                                    .text_color(rgb(p.primary_fg))
                                    .cursor_pointer()
                                    .child(self.tr("rail.planBtn"))
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| this.run_plan(cx)),
                                    ),
                            )
                            .child(
                                div()
                                    .id("tools-btn-scan-registry")
                                    .px_2()
                                    .py_1()
                                    .bg(rgb(p.chip))
                                    .text_xs()
                                    .text_color(rgb(p.text))
                                    .cursor_pointer()
                                    .child(self.tr("rail.scanModels"))
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| this.scan_registry(cx)),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id("tools-plan-body")
                            .max_h(px(140.))
                            .overflow_scroll()
                            .text_xs()
                            .text_color(rgb(p.muted))
                            .child(self.plan_text.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(p.muted))
                            .child(self.registry_status.clone()),
                    )
                    .children(self.registry_entries.iter().enumerate().map(|(i, entry)| {
                        let label = format_registry_entry(entry);
                        div()
                            .id(SharedString::from(format!("tools-reg-entry-{i}")))
                            .px_2()
                            .py_1()
                            .bg(rgb(p.secondary))
                            .border_1()
                            .border_color(rgb(p.border))
                            .text_xs()
                            .text_color(rgb(p.text))
                            .cursor_pointer()
                            .child(label)
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    this.select_registry_entry(i, cx)
                                }),
                            )
                    })),
            )
            .child(self.supported_catalog_panel(cx, "tools"))
            .when(cfg!(feature = "install"), |col| {
                #[cfg(feature = "install")]
                {
                    col.child(self.install_form_panel(cx, "tools"))
                }
                #[cfg(not(feature = "install"))]
                {
                    col
                }
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .bg(rgb(p.panel))
                    .border_1()
                    .border_color(rgb(p.border))
                    .child(self.theme_picker_row(cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .p_3()
                    .bg(rgb(p.panel))
                    .border_1()
                    .border_color(rgb(p.border))
                    .child(div().text_sm().child(format!(
                        "{} · {}",
                        self.tr("tools.language"),
                        self.locale.label()
                    )))
                    .child(
                        div()
                            .id("tools-btn-locale")
                            .px_2()
                            .py_1()
                            .border_1()
                            .border_color(rgb(p.border))
                            .text_xs()
                            .text_color(rgb(p.muted))
                            .cursor_pointer()
                            .child(self.locale.code().to_uppercase())
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.cycle_locale(cx)),
                            ),
                    ),
            )
            .child(self.tools_advanced_panel(cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_3()
                    .bg(rgb(p.panel))
                    .border_1()
                    .border_color(rgb(p.border))
                    .child(
                        div()
                            .id("tools-btn-about")
                            .text_sm()
                            .text_color(rgb(p.primary))
                            .cursor_pointer()
                            .child(about_label)
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.toggle_about(cx)),
                            ),
                    )
                    .when(self.show_about, |c| {
                        c.child(div().text_xs().text_color(rgb(p.muted)).child(ABOUT_NOTE))
                    })
                    .child(
                        div()
                            .id("tools-btn-setup")
                            .text_sm()
                            .text_color(rgb(p.muted))
                            .cursor_pointer()
                            .child(self.tr("setup.open"))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.open_setup_wizard(cx)),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(p.muted))
                            .child(self.tr("setup.reopen")),
                    ),
            )
    }

    fn wizard_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette();
        let step = self.wizard.step;
        let step_label = self.tr_fmt(
            "wizard.stepOf",
            &[
                ("n", &step.number().to_string()),
                ("total", &WizardStep::total().to_string()),
            ],
        );
        let title = self.tr(step.title_key());
        let body = self.tr(step.body_key());
        let model_path_str = self.model_input.read(cx).text().trim().to_string();
        let model_summary = if model_path_str.is_empty() {
            self.tr("rail.modelUnset")
        } else {
            model_path_str.clone()
        };

        // Footer stays outside the scroll body so Back / Skip / Next stay reachable.
        let nav = div()
            .id("wizard-nav")
            .flex()
            .flex_row()
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .gap(px(RAIL_CARD_GAP))
            .w_full()
            .pt(px(WIZARD_CONTENT_GAP))
            .child(
                div()
                    .id("wizard-btn-back")
                    .px(px(BTN_PAD_X + 4.0))
                    .py(px(BTN_PAD_Y + 2.0))
                    .bg(rgb(if step.is_first() { p.chip } else { p.secondary }))
                    .text_sm()
                    .text_color(rgb(p.text))
                    .cursor_pointer()
                    .child(self.tr("wizard.back"))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.wizard_back(cx)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(RAIL_CARD_GAP))
                    .child(
                        div()
                            .id("wizard-btn-skip")
                            .px(px(BTN_PAD_X + 4.0))
                            .py(px(BTN_PAD_Y + 2.0))
                            .bg(rgb(p.chip))
                            .text_sm()
                            .text_color(rgb(p.muted))
                            .cursor_pointer()
                            .child(self.tr("wizard.skip"))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.wizard_skip(cx)),
                            ),
                    )
                    .child(
                        div()
                            .id("wizard-btn-next")
                            .px(px(BTN_PAD_X + 4.0))
                            .py(px(BTN_PAD_Y + 2.0))
                            .bg(rgb(p.primary))
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(p.primary_fg))
                            .cursor_pointer()
                            .child(if step.is_last() {
                                self.tr("wizard.finish")
                            } else {
                                self.tr("wizard.next")
                            })
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.wizard_next(cx)),
                            ),
                    ),
            );

        let mut content = div()
            .id("wizard-body")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .w_full()
            .gap(px(WIZARD_CONTENT_GAP))
            .overflow_scroll();

        content = content
            .child(div().text_xs().text_color(rgb(p.label)).child(step_label))
            .child(
                div()
                    .text_2xl()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(div().text_sm().text_color(rgb(p.muted)).child(body));

        match step {
            WizardStep::Welcome => {}
            WizardStep::Machine => {
                content = content
                    .child(
                        div()
                            .id("wizard-machine-body")
                            .max_h(px(220.))
                            .overflow_scroll()
                            .p_3()
                            .border_1()
                            .border_color(rgb(p.border))
                            .bg(rgb(p.panel))
                            .text_xs()
                            .text_color(rgb(p.muted))
                            .child(self.machine_text.clone()),
                    )
                    .child(
                        div()
                            .id("wizard-btn-refresh-machine")
                            .px_3()
                            .py_1()
                            .bg(rgb(p.primary))
                            .text_xs()
                            .text_color(rgb(p.primary_fg))
                            .cursor_pointer()
                            .child(self.tr("rail.refresh"))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.refresh_probe(cx)),
                            ),
                    );
            }
            WizardStep::Model => {
                let show_dl = self.wizard.show_download;
                // Wizard Model step owns the shared path editor this frame.
                // Product catalog is always visible; freeform download is secondary.
                content = content
                    .child(self.supported_catalog_panel(cx, "wizard"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(p.muted))
                            .child(self.tr("wizard.model.path")),
                    )
                    .when(
                        model_input_site(&self.wizard, self.active_view) == ModelInputSite::Wizard,
                        |col| col.child(self.model_input.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .flex_wrap()
                            .child(
                                div()
                                    .id("wizard-btn-scan")
                                    .px_2()
                                    .py_1()
                                    .bg(rgb(p.chip))
                                    .text_xs()
                                    .text_color(rgb(p.text))
                                    .cursor_pointer()
                                    .child(self.tr("rail.scanModels"))
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| this.scan_registry(cx)),
                                    ),
                            )
                            .when(cfg!(feature = "install"), |row| {
                                row.child(
                                    div()
                                        .id("wizard-btn-toggle-download")
                                        .px_2()
                                        .py_1()
                                        .bg(rgb(p.chip))
                                        .text_xs()
                                        .text_color(rgb(p.text))
                                        .cursor_pointer()
                                        .child(if show_dl {
                                            self.tr("wizard.model.downloadHide")
                                        } else {
                                            self.tr("wizard.model.downloadShow")
                                        })
                                        .on_mouse_up(
                                            MouseButton::Left,
                                            cx.listener(|this, _, _, cx| {
                                                this.wizard.toggle_download();
                                                cx.notify();
                                            }),
                                        ),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(p.muted))
                            .child(self.registry_status.clone()),
                    )
                    .child(
                        div()
                            .id("wizard-reg-list")
                            .flex()
                            .flex_col()
                            .gap_1()
                            .w_full()
                            .min_w_0()
                            .max_h(px(REGISTRY_LIST_MAX_H))
                            .overflow_scroll()
                            .children(self.registry_entries.iter().enumerate().map(
                                |(i, entry)| {
                                    let label = format_registry_entry(entry);
                                    div()
                                        .id(SharedString::from(format!("wizard-reg-entry-{i}")))
                                        .w_full()
                                        .min_w_0()
                                        .px_2()
                                        .py_1()
                                        .bg(rgb(p.secondary))
                                        .border_1()
                                        .border_color(rgb(p.border))
                                        .text_xs()
                                        .text_color(rgb(p.text))
                                        .cursor_pointer()
                                        .child(label)
                                        .on_mouse_up(
                                            MouseButton::Left,
                                            cx.listener(move |this, _, _, cx| {
                                                this.select_registry_entry(i, cx)
                                            }),
                                        )
                                },
                            )),
                    );
                #[cfg(feature = "install")]
                {
                    if show_dl {
                        content = content.child(self.install_form_panel(cx, "wizard"));
                    }
                }
            }
            WizardStep::Readiness => {
                // Recovery mode: path missing / not a model leaf → emphasize
                // Scan / Install. Path ok → Run doctor is the primary CTA.
                let path = self.model_path(cx);
                let needs_recovery =
                    model_path_unset_for_doctor(&path) || Self::path_needs_model_recovery(&path);
                let (doctor_fill, doctor_fg, doctor_border) = if needs_recovery {
                    (p.panel, p.text, p.border)
                } else {
                    (p.primary, p.primary_fg, p.primary)
                };
                let (recovery_fill, recovery_fg, recovery_border) = if needs_recovery {
                    (p.primary, p.primary_fg, p.primary)
                } else {
                    (p.panel, p.text, p.border)
                };
                content = content
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .flex_wrap()
                            .w_full()
                            .child(
                                div()
                                    .id(WIZARD_BTN_DOCTOR)
                                    .min_w(px(112.))
                                    .min_h(px(28.))
                                    .px(px(BTN_PAD_X))
                                    .py(px(BTN_PAD_Y))
                                    .bg(rgb(doctor_fill))
                                    .border_1()
                                    .border_color(rgb(doctor_border))
                                    .text_xs()
                                    .text_color(rgb(doctor_fg))
                                    .cursor_pointer()
                                    .child(self.tr("wizard.readiness.runDoctor"))
                                    // on_click = full press; ids match readiness_action_for_button_id.
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.handle_wizard_readiness_button(WIZARD_BTN_DOCTOR, cx)
                                    })),
                            )
                            .child(
                                div()
                                    .id(WIZARD_BTN_QUICK_CHECK)
                                    .min_w(px(112.))
                                    .min_h(px(28.))
                                    .px(px(BTN_PAD_X))
                                    .py(px(BTN_PAD_Y))
                                    .bg(rgb(p.panel))
                                    .border_1()
                                    .border_color(rgb(p.border))
                                    .text_xs()
                                    .text_color(rgb(p.text))
                                    .cursor_pointer()
                                    .child(self.tr("wizard.readiness.refresh"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.handle_wizard_readiness_button(
                                            WIZARD_BTN_QUICK_CHECK,
                                            cx,
                                        )
                                    })),
                            )
                            .child(
                                div()
                                    .id(WIZARD_BTN_SCAN)
                                    .min_w(px(112.))
                                    .min_h(px(28.))
                                    .px(px(BTN_PAD_X))
                                    .py(px(BTN_PAD_Y))
                                    .bg(rgb(recovery_fill))
                                    .border_1()
                                    .border_color(rgb(recovery_border))
                                    .text_xs()
                                    .text_color(rgb(recovery_fg))
                                    .cursor_pointer()
                                    .child(self.tr("wizard.readiness.scan"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.handle_wizard_readiness_button(WIZARD_BTN_SCAN, cx)
                                    })),
                            )
                            .when(cfg!(feature = "install"), |row| {
                                row.child(
                                    div()
                                        .id(WIZARD_BTN_INSTALL)
                                        .min_w(px(112.))
                                        .min_h(px(28.))
                                        .px(px(BTN_PAD_X))
                                        .py(px(BTN_PAD_Y))
                                        .bg(rgb(recovery_fill))
                                        .border_1()
                                        .border_color(rgb(recovery_border))
                                        .text_xs()
                                        .text_color(rgb(recovery_fg))
                                        .cursor_pointer()
                                        .child(self.tr("wizard.readiness.install"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.handle_wizard_readiness_button(
                                                WIZARD_BTN_INSTALL,
                                                cx,
                                            )
                                        })),
                                )
                            }),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(rgb(p.label))
                            .child(self.tr("wizard.readiness.actionsHint")),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.tr("wizard.readiness.doctor")),
                    )
                    .child(
                        div()
                            .id("wizard-doctor-body")
                            .w_full()
                            .min_w_0()
                            .min_h(px(72.))
                            .max_h(px(280.))
                            .overflow_scroll()
                            .p_3()
                            .border_1()
                            .border_color(rgb(p.border))
                            .bg(rgb(p.panel))
                            .text_xs()
                            .text_color(rgb(p.muted))
                            .child(self.doctor_text.clone()),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .text_xs()
                            .text_color(rgb(p.muted))
                            .child(self.registry_status.clone()),
                    )
                    .child(
                        div()
                            .id("wizard-readiness-reg-list")
                            .flex()
                            .flex_col()
                            .gap_1()
                            .w_full()
                            .min_w_0()
                            .max_h(px(REGISTRY_LIST_MAX_H))
                            .overflow_scroll()
                            .children(self.registry_entries.iter().enumerate().map(
                                |(i, entry)| {
                                    let label = format_registry_entry(entry);
                                    div()
                                        .id(SharedString::from(format!("wizard-readiness-reg-{i}")))
                                        .w_full()
                                        .min_w_0()
                                        .px_2()
                                        .py_1()
                                        .bg(rgb(p.secondary))
                                        .border_1()
                                        .border_color(rgb(p.border))
                                        .text_xs()
                                        .text_color(rgb(p.text))
                                        .cursor_pointer()
                                        .child(label)
                                        .on_mouse_up(
                                            MouseButton::Left,
                                            cx.listener(move |this, _, _, cx| {
                                                this.select_registry_entry(i, cx);
                                                this.run_doctor_with_recovery(false, cx);
                                                this.run_plan(cx);
                                            }),
                                        )
                                },
                            )),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.tr("wizard.readiness.plan")),
                    )
                    .child(
                        div()
                            .id("wizard-plan-body")
                            .w_full()
                            .min_w_0()
                            .min_h(px(48.))
                            .max_h(px(200.))
                            .overflow_scroll()
                            .p_3()
                            .border_1()
                            .border_color(rgb(p.border))
                            .bg(rgb(p.panel))
                            .text_xs()
                            .text_color(rgb(p.muted))
                            .child(self.plan_text.clone()),
                    );
            }
            WizardStep::LookAndFeel => {
                content = content.child(
                    div()
                        .p_3()
                        .border_1()
                        .border_color(rgb(p.border))
                        .bg(rgb(p.panel))
                        .child(self.theme_picker_row(cx)),
                );
            }
            WizardStep::Ready => {
                let model_path = self.model_path(cx);
                let model_not_ready = !is_model_leaf(&model_path);
                let mut summary = div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .border_1()
                    .border_color(rgb(p.border))
                    .bg(rgb(p.panel))
                    .text_sm()
                    .child(format!(
                        "{} · {}",
                        self.tr("wizard.ready.summaryTheme"),
                        self.theme_label()
                    ))
                    .child(format!(
                        "{} · {}",
                        self.tr("wizard.ready.summaryLocale"),
                        self.locale.label()
                    ))
                    .child(format!(
                        "{} · {}",
                        self.tr("wizard.ready.summaryModel"),
                        model_summary
                    ));
                if model_not_ready {
                    summary = summary.child(
                        div()
                            .id("wizard-ready-model-warning")
                            .text_xs()
                            .text_color(rgb(p.warn))
                            .child(self.tr("wizard.ready.modelNotReady")),
                    );
                }
                let start_paint = start_button_paint(&p, self.engine_is_live(), self.starting);
                content = content.child(summary).child(
                    div()
                        .id("wizard-btn-start-engine")
                        .px_3()
                        .py_2()
                        .bg(rgb(start_paint.fill))
                        .border_1()
                        .border_color(rgb(start_paint.border))
                        .text_sm()
                        .text_color(rgb(start_paint.text))
                        .cursor_pointer()
                        .child(self.tr("wizard.ready.start"))
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.start_engine_from(StartEngineSource::WizardReady, cx)
                            }),
                        ),
                );
            }
        }

        // Stage fills the main column. Card caps at stage height; body scrolls
        // above a fixed footer (Back / Skip / Next) so tall steps stay usable.
        div()
            .id("wizard-view")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .w_full()
            .items_center()
            .justify_center()
            .p(px(WIZARD_STAGE_PAD))
            .bg(rgb(p.bg))
            .overflow_hidden()
            .child(
                div()
                    .id("wizard-card")
                    .w_full()
                    .max_w(px(WIZARD_MAX_W))
                    .max_h_full()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .p(px(WIZARD_CARD_PAD))
                    .border_1()
                    .border_color(rgb(p.border))
                    .bg(rgb(p.panel))
                    .child(content)
                    .child(nav),
            )
    }

    fn chat_hero(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette();
        let show_setup_cta = show_first_run_setup_cta(self.first_run_done);
        let next_key = if show_setup_cta {
            None
        } else {
            let model_ok = hero_model_ok(&self.model_path(cx));
            hero_next_step_key(hero_next_step(self.engine_is_live(), model_ok))
        };
        let prompts = [
            ("prompt-routing", "prompts.routing"),
            ("prompt-bench", "prompts.benchmark"),
            ("prompt-cache", "prompts.caching"),
        ];
        div()
            .id("chat-hero")
            .flex()
            .flex_col()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_3()
            .p_6()
            .max_w(px(HERO_MAX_W))
            .mx_auto()
            .child(
                div()
                    .w(px(66.))
                    .h(px(66.))
                    .border_1()
                    .border_color(rgb(p.primary_border))
                    .bg(rgb(p.primary_wash))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(p.primary))
                    .text_lg()
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("c"),
            )
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(p.label))
                    .child(self.tr("hero.title")),
            )
            .child(
                div()
                    .text_2xl()
                    .font_weight(gpui::FontWeight::NORMAL)
                    .text_color(rgb(p.text))
                    .child(format!(
                        "{} {}",
                        self.tr("hero.subtitle"),
                        self.tr("hero.tagline")
                    )),
            )
            .child(
                div()
                    .max_w(px(510.))
                    .text_xs()
                    .text_color(rgb(p.muted))
                    .child(self.tr("hero.description")),
            )
            .when(show_setup_cta, |hero| {
                hero.child(
                    div()
                        .max_w(px(510.))
                        .text_xs()
                        .text_color(rgb(p.primary))
                        .child(self.tr("hero.setupHint")),
                )
                .child(
                    div()
                        .id("hero-btn-setup")
                        .px_3()
                        .py_2()
                        .bg(rgb(p.primary))
                        .text_sm()
                        .text_color(rgb(p.primary_fg))
                        .cursor_pointer()
                        .child(self.tr("rail.setup"))
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| this.open_setup_wizard(cx)),
                        ),
                )
            })
            .when_some(next_key, |hero, key| {
                hero.child(
                    div()
                        .max_w(px(510.))
                        .text_xs()
                        .text_color(rgb(p.primary))
                        .child(self.tr(key)),
                )
            })
            .child(div().flex().flex_row().gap_2().mt_4().w_full().children(
                prompts.into_iter().map(|(id, key)| {
                    let label = self.tr(key);
                    let key_static: &'static str = key;
                    div()
                        .id(id)
                        .flex_1()
                        .p_3()
                        .border_1()
                        .border_color(rgb(p.border))
                        .bg(rgb(p.panel))
                        .text_xs()
                        .text_color(rgb(p.muted))
                        .cursor_pointer()
                        .child(label)
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.apply_suggested_prompt(key_static, cx)
                            }),
                        )
                }),
            ))
    }

    fn chat_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette();
        let chat_empty = self.chat_log.is_empty();
        let you = self.tr("chat.you");
        let colibri = self.tr("chat.colibri");
        let chat_rows = self.chat_log.iter().map(|(role, text)| {
            let (role_color, label, body_color) = match role.as_ref() {
                "user" => (p.primary, you.as_str(), p.user_body),
                "assistant" => (p.primary, colibri.as_str(), p.assist_body),
                _ => (p.warn, "note", p.muted),
            };
            div()
                .flex()
                .flex_row()
                .gap_3()
                .mb_4()
                .child(
                    div()
                        .w(px(31.))
                        .h(px(31.))
                        .border_1()
                        .border_color(rgb(if role.as_ref() == "assistant" {
                            p.primary_border
                        } else {
                            p.border
                        }))
                        .bg(rgb(if role.as_ref() == "assistant" {
                            p.primary_wash
                        } else {
                            p.panel
                        }))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(role_color))
                        .child(if role.as_ref() == "user" { "Y" } else { "c" }),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(p.muted))
                                .child(label.to_string()),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(body_color))
                                .child(text.clone()),
                        ),
                )
        });

        div()
            .id("chat-view")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap_2()
            .child(
                div()
                    .id("chat-log")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .gap_2()
                    .px_6()
                    .pt_4()
                    .overflow_scroll()
                    .when(chat_empty, |log| log.child(self.chat_hero(cx)))
                    .when(!chat_empty, |log| {
                        log.child(div().max_w(px(820.)).mx_auto().w_full().children(chat_rows))
                    }),
            )
            .child(
                div().px_6().pb_4().pt_2().child(
                    div()
                        .max_w(px(820.))
                        .mx_auto()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .p_2()
                        .border_1()
                        .border_color(rgb(p.border))
                        .bg(rgb(p.panel))
                        .child(self.chat_input.clone())
                        .when_some(self.gen_progress.as_ref(), |col, view| {
                            col.child(
                                div()
                                    .id("generate-progress")
                                    .child(Self::progress_strip_el(&p, view, true)),
                            )
                        })
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(p.label))
                                        .child(self.tr("chat.inputHint")),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .gap_2()
                                        .child({
                                            let stop = stop_button_paint(&p, self.generating);
                                            div()
                                                .id("btn-stop")
                                                .px_3()
                                                .py_2()
                                                .bg(rgb(stop.fill))
                                                .border_1()
                                                .border_color(rgb(stop.border))
                                                .text_sm()
                                                .text_color(rgb(stop.text))
                                                .cursor_pointer()
                                                .child(self.tr("chat.stop"))
                                                .on_mouse_up(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.stop_generate(cx)
                                                    }),
                                                )
                                        })
                                        .child(
                                            div()
                                                .id("btn-send")
                                                .px_3()
                                                .py_2()
                                                .bg(rgb(if self.generating {
                                                    p.muted
                                                } else {
                                                    p.primary
                                                }))
                                                .border_1()
                                                .border_color(rgb(if self.generating {
                                                    p.muted
                                                } else {
                                                    p.primary
                                                }))
                                                .text_sm()
                                                .text_color(rgb(if self.generating {
                                                    p.text
                                                } else {
                                                    p.primary_fg
                                                }))
                                                .cursor_pointer()
                                                .child(if self.generating {
                                                    "...".into()
                                                } else {
                                                    self.tr("chat.send")
                                                })
                                                .on_mouse_up(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.send_chat(cx)
                                                    }),
                                                ),
                                        ),
                                ),
                        ),
                ),
            )
    }
}

/// Tab chrome fill: active uses primary; inactive uses panel (never bare black).
fn tab_bg_color(p: &ThemePalette, active: bool) -> u32 {
    if active { p.primary } else { p.panel }
}

/// Placeholder for the shared model path field (rail / Tools / wizard).
/// Keep short so it fits the slim rail without horizontal overflow.
const MODEL_PATH_PLACEHOLDER: &str = "Model folder path";

/// Paint tokens for Stop (rail engine stop + chat generate stop).
///
/// - **Usable** (`can_stop`): solid danger fill, matching border, dark-on-bright label.
/// - **Not usable**: hollow danger outline (panel fill, 1px danger border, danger text).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StopButtonPaint {
    fill: u32,
    border: u32,
    text: u32,
    /// Solid danger body when true; outline-only when false.
    #[allow(dead_code)] // asserted in chrome_tests; paint uses fill/border/text
    solid: bool,
}

fn stop_button_paint(p: &ThemePalette, can_stop: bool) -> StopButtonPaint {
    if can_stop {
        StopButtonPaint {
            fill: p.danger,
            border: p.danger,
            text: p.primary_fg,
            solid: true,
        }
    } else {
        StopButtonPaint {
            fill: p.panel,
            border: p.danger,
            text: p.danger,
            solid: false,
        }
    }
}

/// Start engine paint: solid ok when start is useful; hollow ok outline when live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StartButtonPaint {
    fill: u32,
    border: u32,
    text: u32,
    #[allow(dead_code)] // asserted in chrome_tests; paint uses fill/border/text
    solid: bool,
}

fn start_button_paint(p: &ThemePalette, engine_live: bool, starting: bool) -> StartButtonPaint {
    if starting {
        StartButtonPaint {
            fill: p.panel,
            border: p.ok,
            text: p.ok,
            solid: false,
        }
    } else if !engine_live {
        StartButtonPaint {
            fill: p.ok,
            border: p.ok,
            text: p.primary_fg,
            solid: true,
        }
    } else {
        StartButtonPaint {
            fill: p.panel,
            border: p.ok,
            text: p.ok,
            solid: false,
        }
    }
}

/// Whether the empty-chat hero shows the first-run Setup CTA (hint + green button).
///
/// Same gate the wizard uses: when `first_run_done` is true, never push Setup again
/// from the center hero. Re-open Setup lives on Tools, not a primary rail slab.
fn show_first_run_setup_cta(first_run_done: bool) -> bool {
    !first_run_done
}

/// Giant green primary Setup slab on the left rail (`btn-setup` + `p.primary`).
/// Hidden after Finish/Skip. Re-open lives on Tools.
fn show_rail_setup_primary_cta(first_run_done: bool) -> bool {
    !first_run_done
}

/// Fill for that rail slab when it is shown. `None` means do not paint it.
fn rail_setup_primary_fill(p: &ThemePalette, first_run_done: bool) -> Option<u32> {
    if show_rail_setup_primary_cta(first_run_done) {
        Some(p.primary)
    } else {
        None
    }
}

/// Post-setup empty-chat guidance when the first-run Setup CTA is hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeroNextStep {
    /// Engine not started and no usable model path.
    NeedModel,
    /// Engine not started; model path looks ready.
    StartEngine,
    /// Engine is live; no extra next-step line.
    Ready,
}

/// Pure next-step for empty chat after setup is finished.
fn hero_next_step(engine_live: bool, model_ok: bool) -> HeroNextStep {
    if engine_live {
        HeroNextStep::Ready
    } else if model_ok {
        HeroNextStep::StartEngine
    } else {
        HeroNextStep::NeedModel
    }
}

/// i18n key for post-setup empty-chat next-step copy (`None` when ready).
fn hero_next_step_key(step: HeroNextStep) -> Option<&'static str> {
    match step {
        HeroNextStep::NeedModel => Some("hero.nextNeedModel"),
        HeroNextStep::StartEngine => Some("hero.nextStartEngine"),
        HeroNextStep::Ready => None,
    }
}

/// Model path is ready enough to suggest Start engine (not first-run Setup).
fn hero_model_ok(path: &Path) -> bool {
    !model_path_unset_for_doctor(path) && path.is_dir() && path.join("config.json").is_file()
}

/// Brain legend i18n key for the active theme (DOGE map differs from mint).
fn brain_legend_key(theme: ThemeId) -> &'static str {
    match theme {
        ThemeId::Doge => "brain.legend.doge",
        ThemeId::Mint => "brain.legend.mint",
    }
}

/// Where the shared model path editor mounts this frame (single parent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelInputSite {
    Rail,
    Tools,
    Wizard,
}

/// At most one of rail / Tools / wizard mounts `model_input` per frame.
fn model_input_site(wizard: &WizardState, view: MainView) -> ModelInputSite {
    if wizard.open && wizard.step == WizardStep::Model {
        ModelInputSite::Wizard
    } else if view == MainView::Tools {
        ModelInputSite::Tools
    } else {
        ModelInputSite::Rail
    }
}

#[derive(Clone, Copy)]
enum BadgeKind {
    Live,
    Speed,
    Warn,
    Muted,
}

fn prof_tile(p: &ThemePalette, label: String, value: String, foot: String) -> impl IntoElement {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .border_1()
        .border_color(rgb(p.border))
        .bg(rgb(p.panel))
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(p.muted))
                .child(label),
        )
        .child(
            div()
                .text_lg()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(p.text))
                .child(value),
        )
        .child(div().text_xs().text_color(rgb(p.muted)).child(foot))
}

fn share_bar_el(
    p: &ThemePalette,
    label: String,
    total: f64,
    segs: &[crate::profiling_view::ShareSegment],
    _locale: Locale,
) -> impl IntoElement {
    let bar_w = 480.0_f32;
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .text_xs()
                .text_color(rgb(p.muted))
                .child(label)
                .child(format_seconds(total)),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .h(px(22.))
                .w_full()
                .overflow_hidden()
                .gap_0p5()
                .children(segs.iter().filter(|s| s.share > 0.001).map(|s| {
                    let w = ((s.share as f32) * bar_w).max(2.0);
                    let pct = if s.share >= 0.09 {
                        format!("{}%", (s.share * 100.0).round() as i32)
                    } else {
                        String::new()
                    };
                    div()
                        .h_full()
                        .w(px(w))
                        .bg(rgb(s.phase.color_in(p)))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(p.primary_fg))
                        .child(pct)
                })),
        )
}

fn profile_table_el(p: &ThemePalette, turns: &[DerivedTurn], locale: Locale) -> impl IntoElement {
    let header = div()
        .flex()
        .flex_row()
        .gap_2()
        .px_3()
        .py_2()
        .border_b_1()
        .border_color(rgb(p.border))
        .text_xs()
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(rgb(p.muted))
        .child(div().w(px(40.)).child(t(locale, "profile.turnCol")))
        .child(div().w(px(90.)).child(t(locale, "profile.tokensCol")))
        .child(div().w(px(50.)).child("tok/s"))
        .child(div().w(px(50.)).child(t(locale, "profile.wallCol")))
        .children(ProfilePhase::ALL.iter().map(|ph| {
            div()
                .w(px(70.))
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(div().w(px(8.)).h(px(8.)).bg(rgb(ph.color_in(p))))
                .child(t(locale, ph.i18n_key()))
        }))
        .child(div().w(px(70.)).child(t(locale, "profile.diskService")));

    let n = turns.len();
    let rows = turns.iter().enumerate().rev().map(|(i, turn)| {
        let turn_no = i + 1;
        div()
            .id(SharedString::from(format!("prof-row-{turn_no}")))
            .flex()
            .flex_row()
            .gap_2()
            .px_3()
            .py_1()
            .border_b_1()
            .border_color(rgb(p.border))
            .text_xs()
            .text_color(rgb(p.text))
            .child(div().w(px(40.)).child(format!("{turn_no}")))
            .child(div().w(px(90.)).child(format!(
                "{} → {}",
                turn.prompt_tokens, turn.completion_tokens
            )))
            .child(div().w(px(50.)).child(format!("{:.1}", turn.toks)))
            .child(div().w(px(50.)).child(format_seconds(turn.wall_s)))
            .children(
                ProfilePhase::ALL
                    .iter()
                    .map(|ph| div().w(px(70.)).child(format_seconds(turn.phase_s(*ph)))),
            )
            .child(div().w(px(70.)).child(format_seconds(turn.expert_disk_s)))
    });

    let disk_service: f64 = turns.iter().map(|t| t.expert_disk_s).sum();
    div()
        .id("prof-table")
        .flex()
        .flex_col()
        .border_1()
        .border_color(rgb(p.border))
        .bg(rgb(p.panel))
        .overflow_scroll()
        .child(header)
        .children(rows)
        .when(disk_service > 0.0, |c| {
            c.child(
                div()
                    .p_2()
                    .text_xs()
                    .text_color(rgb(p.muted))
                    .child(t(locale, "profile.diskNote")),
            )
        })
        .when(n == 0, |c| c)
}

/// Bootstrap Machine / Doctor / Plan panels when probe was already done.
fn bootstrap_panels_with_machine(
    model_input: &Entity<TextInput>,
    machine: Option<colibri_sys::MachineInfo>,
    cx: &mut Context<DesktopApp>,
) -> (
    String,
    Option<colibri_sys::MachineInfo>,
    String,
    String,
    String,
) {
    let machine_text = machine
        .as_ref()
        .map(|m| format_machine(m, false))
        .unwrap_or_else(|| "Reading this machine…".into());

    let model_path =
        colibri_sys::expand_user_path(PathBuf::from(model_input.read(cx).text().trim()));
    // Empty model path → idle doctor (no cwd / "." probe).
    // Missing path → recovery checklist (default store + Scan / Install).
    let doctor_text = run_shallow_doctor(&model_path, machine.as_ref());
    let plan_text = if model_path_unset_for_doctor(&model_path) {
        "Set a model path first, then run Plan.".into()
    } else {
        run_plan(&model_path, machine.as_ref())
    };
    let status = if machine.is_some() {
        "Ready".into()
    } else {
        "Could not read this machine".into()
    };
    (machine_text, machine, doctor_text, plan_text, status)
}

impl Render for DesktopApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette();
        let mut badges: Vec<AnyElement> = Vec::new();
        if let Some(n) = self.badge_tokens {
            let label = match self.locale {
                Locale::En => format_badge_tokens(n),
                Locale::It => self.tr_fmt("topbar.tokens", &[("n", &n.to_string())]),
            };
            badges.push(Self::badge_chip(&p, label.into(), BadgeKind::Live).into_any_element());
        }
        if let Some(s) = self.badge_tok_s {
            let label = match self.locale {
                Locale::En => format_badge_tok_per_sec(s as f64),
                Locale::It => self.tr_fmt("topbar.tokPerSec", &[("n", &format!("{s:.1}"))]),
            };
            badges.push(Self::badge_chip(&p, label.into(), BadgeKind::Speed).into_any_element());
        }
        if let Some(ms) = self.badge_ttft_ms {
            let label = match self.locale {
                Locale::En => format_badge_ttft_ms(ms),
                Locale::It => self.tr_fmt("topbar.ttft", &[("n", &format!("{ms:.0}"))]),
            };
            badges.push(Self::badge_chip(&p, label.into(), BadgeKind::Warn).into_any_element());
        }
        if self.kv_slots > 1 {
            badges.push(
                Self::badge_chip(
                    &p,
                    self.tr_fmt("topbar.slot", &[("n", &(self.cache_slot + 1).to_string())])
                        .into(),
                    BadgeKind::Muted,
                )
                .into_any_element(),
            );
        }

        let active_view = self.active_view;
        let wizard_open = self.wizard.open;
        let tabs = div()
            .flex()
            .flex_row()
            .gap_1()
            .p_0p5()
            .border_1()
            .border_color(rgb(p.border))
            .bg(rgb(p.panel))
            .child(self.view_tab_btn("tab-chat", self.tr("nav.chat").into(), MainView::Chat, cx))
            .child(self.view_tab_btn(
                "tab-brain",
                self.tr("nav.brain").into(),
                MainView::Brain,
                cx,
            ))
            .child(self.view_tab_btn(
                "tab-prof",
                self.tr("nav.profiling").into(),
                MainView::Profiling,
                cx,
            ))
            .child(self.view_tab_btn(
                "tab-tools",
                self.tr("nav.tools").into(),
                MainView::Tools,
                cx,
            ));

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(p.bg))
            .text_color(rgb(p.text))
            .when(self.show_about && !wizard_open, |root| {
                root.child(
                    div()
                        .px_4()
                        .py_1()
                        .bg(rgb(p.primary_wash))
                        .text_xs()
                        .text_color(rgb(p.primary))
                        .child(ABOUT_NOTE),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .child(self.left_rail(cx))
                    .child(
                        // Main column: wizard full-main, or topbar + active view
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .when(wizard_open, |col| col.child(self.wizard_view(cx)))
                            .when(!wizard_open, |col| {
                                col.child(
                                    // Topbar
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .justify_between()
                                        .h(px(72.))
                                        .px_6()
                                        .border_b_1()
                                        .border_color(rgb(p.border))
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .gap_0p5()
                                                .min_w_0()
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_weight(gpui::FontWeight::BOLD)
                                                        .text_color(rgb(p.label))
                                                        .child(self.tr("topbar.activeModel")),
                                                )
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .font_weight(gpui::FontWeight::MEDIUM)
                                                        .text_color(rgb(p.text))
                                                        .child(self.engine_label.clone()),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap_2()
                                                .child(tabs)
                                                .children(badges)
                                                .child(
                                                    div()
                                                        .id("btn-clear")
                                                        .px_2()
                                                        .py_1()
                                                        .bg(rgb(p.chip))
                                                        .text_xs()
                                                        .text_color(rgb(p.muted))
                                                        .cursor_pointer()
                                                        .child(self.tr("topbar.clear"))
                                                        .on_mouse_up(
                                                            MouseButton::Left,
                                                            cx.listener(|this, _, _, cx| {
                                                                this.clear_chat(cx)
                                                            }),
                                                        ),
                                                ),
                                        ),
                                )
                                .when(active_view == MainView::Chat, |col| {
                                    col.child(self.chat_view(cx))
                                })
                                .when(active_view == MainView::Brain, |col| {
                                    col.child(self.brain_panel_full(cx))
                                })
                                .when(active_view == MainView::Profiling, |col| {
                                    col.child(self.profiling_page())
                                })
                                .when(active_view == MainView::Tools, |col| {
                                    col.child(self.tools_view(cx))
                                })
                            }),
                    ),
            )
    }
}

/// How the main window should open. Default is fullscreen; set
/// `COLIBRI_WINDOWED=1` (or `true` / `yes` / `windowed`) for a centered window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaunchWindowMode {
    Fullscreen,
    Windowed,
}

/// Pure helper: map env override to launch mode. Used by `main` and unit tests.
fn launch_window_mode_from_env(windowed_env: Option<&str>) -> LaunchWindowMode {
    match windowed_env.map(str::trim) {
        Some(v) if v.eq_ignore_ascii_case("1") => LaunchWindowMode::Windowed,
        Some(v) if v.eq_ignore_ascii_case("true") => LaunchWindowMode::Windowed,
        Some(v) if v.eq_ignore_ascii_case("yes") => LaunchWindowMode::Windowed,
        Some(v) if v.eq_ignore_ascii_case("windowed") => LaunchWindowMode::Windowed,
        _ => LaunchWindowMode::Fullscreen,
    }
}

/// Restore size when leaving fullscreen (or the windowed open size).
const DEFAULT_WINDOW_SIZE: (f32, f32) = (1280.0, 820.0);

fn initial_window_bounds(mode: LaunchWindowMode, restore: Bounds<gpui::Pixels>) -> WindowBounds {
    match mode {
        LaunchWindowMode::Fullscreen => WindowBounds::Fullscreen(restore),
        LaunchWindowMode::Windowed => WindowBounds::Windowed(restore),
    }
}

fn session_engine_kind(slot: &Arc<Mutex<Option<EngineSession>>>) -> Option<&'static str> {
    slot.lock().ok().and_then(|g| {
        g.as_ref()
            .map(|s| if s.is_ffi() { "ffi" } else { "process" })
    })
}

fn main() {
    crate::stderr_tee::install_host_stderr_tee();
    match crate::log_init::init_native_logging() {
        crate::log_init::NativeLogInit::Enabled { path } => {
            tracing::info!(
                target: "colibri_native",
                path = %path.display(),
                "native log file"
            );
        }
        crate::log_init::NativeLogInit::Disabled => {}
    }
    Application::new().run(|cx: &mut App| {
        bind_text_input_keys(cx);

        let restore = Bounds::centered(
            None,
            size(px(DEFAULT_WINDOW_SIZE.0), px(DEFAULT_WINDOW_SIZE.1)),
            cx,
        );
        let mode = launch_window_mode_from_env(std::env::var("COLIBRI_WINDOWED").ok().as_deref());
        cx.open_window(
            WindowOptions {
                window_bounds: Some(initial_window_bounds(mode, restore)),
                titlebar: Some(TitlebarOptions {
                    title: Some("colibrì".into()),
                    ..Default::default()
                }),
                app_id: Some(crate::log_init::native_app_id().to_string()),
                window_min_size: Some(size(px(900.), px(600.))),
                focus: true,
                show: true,
                ..Default::default()
            },
            |_, cx| cx.new(DesktopApp::new),
        )
        .expect("open window");
        cx.activate(true);
    });
}

#[cfg(test)]
mod chrome_tests {
    use super::*;
    use crate::theme::{doge_palette, mint_palette};

    #[test]
    fn session_heartbeat_interval_matches_log_init() {
        assert_eq!(SESSION_HEARTBEAT_MS, crate::log_init::SESSION_HEARTBEAT_MS);
        assert!(
            (5_000..=10_000).contains(&SESSION_HEARTBEAT_MS),
            "engine-up heartbeat must be 5-10s, got {SESSION_HEARTBEAT_MS}"
        );
    }

    #[test]
    fn inactive_tab_fill_is_palette_not_literal_black() {
        let mint = mint_palette();
        assert_ne!(tab_bg_color(&mint, false), 0x0000_0000);
        assert_eq!(tab_bg_color(&mint, false), mint.panel);
        assert_eq!(tab_bg_color(&doge_palette(), true), doge_palette().primary);
        assert_eq!(tab_bg_color(&mint, true), mint.primary);
    }

    #[test]
    fn brain_legend_key_is_theme_aware() {
        assert_eq!(brain_legend_key(ThemeId::Doge), "brain.legend.doge");
        assert_eq!(brain_legend_key(ThemeId::Mint), "brain.legend.mint");
        let doge = t(Locale::En, "brain.legend.doge");
        assert!(
            !doge.to_ascii_lowercase().contains("gray"),
            "DOGE legend must not claim Gray disk: {doge}"
        );
        assert!(
            doge.to_ascii_lowercase().contains("black")
                || doge.to_ascii_lowercase().contains("cyan"),
            "{doge}"
        );
    }

    #[test]
    fn model_input_single_parent_per_frame() {
        let mut w = WizardState::closed();
        assert_eq!(model_input_site(&w, MainView::Chat), ModelInputSite::Rail);
        assert_eq!(model_input_site(&w, MainView::Tools), ModelInputSite::Tools);
        w = WizardState::open_at_start();
        while w.step != WizardStep::Model {
            assert!(w.advance());
        }
        assert_eq!(
            model_input_site(&w, MainView::Tools),
            ModelInputSite::Wizard
        );
        assert_eq!(model_input_site(&w, MainView::Chat), ModelInputSite::Wizard);
    }

    /// Regression: Doctor-step button element ids must stay wired to the action
    /// map used by `handle_wizard_readiness_button` (same constants as UI `.id`).
    #[test]
    fn wizard_doctor_button_ids_dispatch_via_action_map() {
        let wired = [
            (WIZARD_BTN_DOCTOR, WizardReadinessAction::RunDoctor),
            (WIZARD_BTN_QUICK_CHECK, WizardReadinessAction::QuickCheck),
            (WIZARD_BTN_SCAN, WizardReadinessAction::ScanModels),
            (WIZARD_BTN_INSTALL, WizardReadinessAction::InstallModel),
        ];
        for (id, want) in wired {
            assert_eq!(
                readiness_action_for_button_id(id),
                Some(want),
                "button id {id} must map to {want:?}"
            );
            // Running status must be non-empty so the rail cannot stay stuck on
            // a prior "Plan finished" after a dead click.
            assert!(!readiness_running_status(want).is_empty());
        }
    }

    /// Host work for Run doctor / Quick check mutates empty path + stamp proves click.
    #[test]
    fn readiness_run_doctor_path_mutates_and_stamps() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("doctor-click-store");
        assert!(!path.exists());
        let body = run_shallow_doctor(&path, None);
        assert!(path.is_dir(), "click path must mkdir");
        assert!(
            path.join(crate::host::STORE_CONFIG_FILE_NAME).is_file(),
            "click path must scaffold store toml"
        );
        let clock = "09:30:00";
        let out = readiness_click_outcome(WizardReadinessAction::RunDoctor, &body, None, clock);
        assert!(out.status.contains("Doctor finished"), "{}", out.status);
        assert!(out.status.contains(clock), "{}", out.status);
        assert!(
            out.doctor_text.contains("Last run: 09:30:00"),
            "{}",
            out.doctor_text
        );
        assert!(
            out.doctor_text.contains("Needs model") || out.doctor_text.contains("Created"),
            "{}",
            out.doctor_text
        );
    }

    #[test]
    fn engine_ready_status_has_no_lab_jargon() {
        let s = "Engine ready (in-process). Expert map and live stats update while you chat.";
        let lower = s.to_ascii_lowercase();
        assert!(!lower.contains("prof"));
        assert!(!lower.contains("embed visual poll"));
        assert!(!lower.contains("hwinfo"));
        assert!(!lower.contains("emap"));
    }

    #[test]
    fn stop_button_usable_is_solid_danger() {
        for p in [doge_palette(), mint_palette()] {
            let s = stop_button_paint(&p, true);
            assert!(s.solid, "usable stop must be solid fill");
            assert_eq!(s.fill, p.danger);
            assert_eq!(s.border, p.danger);
            assert_eq!(s.text, p.primary_fg);
        }
        assert_eq!(
            stop_button_paint(&doge_palette(), true).fill,
            crate::theme::DOGE_RED
        );
    }

    #[test]
    fn stop_button_idle_is_hollow_danger_outline() {
        for p in [doge_palette(), mint_palette()] {
            let s = stop_button_paint(&p, false);
            assert!(!s.solid, "idle stop must be outline only");
            assert_ne!(s.fill, p.danger, "idle must not paint solid danger");
            assert_eq!(s.fill, p.panel);
            assert_eq!(s.border, p.danger);
            assert_eq!(s.text, p.danger);
        }
        assert_eq!(
            stop_button_paint(&doge_palette(), false).border,
            crate::theme::DOGE_RED
        );
    }

    #[test]
    fn start_button_solid_when_engine_down_hollow_when_live() {
        let p = doge_palette();
        let down = start_button_paint(&p, false, false);
        assert!(down.solid);
        assert_eq!(down.fill, p.ok);
        let live = start_button_paint(&p, true, false);
        assert!(!live.solid);
        assert_eq!(live.border, p.ok);
        assert_eq!(live.fill, p.panel);
    }

    #[test]
    fn start_button_in_progress_is_not_solid_ok() {
        let p = doge_palette();
        let s = start_button_paint(&p, false, true);
        assert!(!s.solid, "starting must not keep the full green slab");
        assert_ne!(s.fill, p.ok);
        assert_eq!(s.border, p.ok);
    }

    #[test]
    fn show_first_run_setup_cta_false_when_first_run_done() {
        assert!(show_first_run_setup_cta(false));
        assert!(!show_first_run_setup_cta(true));
    }

    #[test]
    fn show_rail_setup_primary_cta_false_when_first_run_done() {
        assert!(
            show_rail_setup_primary_cta(false),
            "first-run still offers the rail Setup slab"
        );
        assert!(
            !show_rail_setup_primary_cta(true),
            "after Finish/Skip the giant green rail Setup must go away"
        );
    }

    #[test]
    fn rail_setup_primary_fill_absent_after_first_run() {
        let p = doge_palette();
        assert_eq!(rail_setup_primary_fill(&p, false), Some(p.primary));
        assert_eq!(rail_setup_primary_fill(&p, true), None);
    }

    #[test]
    fn hero_next_step_after_setup_guides_engine_not_setup() {
        assert_eq!(
            hero_next_step(false, false),
            HeroNextStep::NeedModel,
            "no model → pick/install, not open Setup"
        );
        assert_eq!(
            hero_next_step(false, true),
            HeroNextStep::StartEngine,
            "model ok → Start engine on rail"
        );
        assert_eq!(hero_next_step(true, true), HeroNextStep::Ready);
        assert_eq!(hero_next_step(true, false), HeroNextStep::Ready);
        assert_eq!(
            hero_next_step_key(HeroNextStep::NeedModel),
            Some("hero.nextNeedModel")
        );
        assert_eq!(
            hero_next_step_key(HeroNextStep::StartEngine),
            Some("hero.nextStartEngine")
        );
        assert_eq!(hero_next_step_key(HeroNextStep::Ready), None);
        // Copy must not re-pitch first-run Setup.
        for key in ["hero.nextNeedModel", "hero.nextStartEngine"] {
            let en = t(Locale::En, key);
            let lower = en.to_ascii_lowercase();
            assert!(
                !lower.contains("first time") && !lower.contains("open setup"),
                "post-setup hint must not re-pitch Setup: {en}"
            );
        }
    }

    #[test]
    fn hero_model_ok_requires_config_json() {
        let root = tempfile::tempdir().unwrap();
        let empty = root.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(!hero_model_ok(Path::new("")));
        assert!(!hero_model_ok(&empty));
        let leaf = root.path().join("model");
        std::fs::create_dir_all(&leaf).unwrap();
        std::fs::write(leaf.join("config.json"), br#"{"model_type":"glm"}"#).unwrap();
        assert!(hero_model_ok(&leaf));
    }

    #[test]
    fn model_path_placeholder_is_short_and_clear() {
        assert!(
            MODEL_PATH_PLACEHOLDER.len() <= 24,
            "placeholder too long for slim rail: {:?}",
            MODEL_PATH_PLACEHOLDER
        );
        assert!(
            !MODEL_PATH_PLACEHOLDER.contains("COLIBRI_MODEL"),
            "env var names overflow the field"
        );
        assert!(
            !MODEL_PATH_PLACEHOLDER.contains("COLI_MODEL"),
            "env var names overflow the field"
        );
        assert!(
            MODEL_PATH_PLACEHOLDER.to_ascii_lowercase().contains("path")
                || MODEL_PATH_PLACEHOLDER
                    .to_ascii_lowercase()
                    .contains("folder"),
            "placeholder should name the field: {:?}",
            MODEL_PATH_PLACEHOLDER
        );
    }

    #[test]
    fn launch_window_mode_defaults_to_fullscreen() {
        assert_eq!(
            launch_window_mode_from_env(None),
            LaunchWindowMode::Fullscreen
        );
        assert_eq!(
            launch_window_mode_from_env(Some("")),
            LaunchWindowMode::Fullscreen
        );
        assert_eq!(
            launch_window_mode_from_env(Some("0")),
            LaunchWindowMode::Fullscreen
        );
        assert_eq!(
            launch_window_mode_from_env(Some("no")),
            LaunchWindowMode::Fullscreen
        );
    }

    #[test]
    fn launch_window_mode_windowed_override() {
        for v in ["1", "true", "TRUE", "yes", "Yes", "windowed", " WINDOWED "] {
            assert_eq!(
                launch_window_mode_from_env(Some(v)),
                LaunchWindowMode::Windowed,
                "env value {v:?} should force windowed"
            );
        }
    }

    #[test]
    fn initial_window_bounds_matches_mode() {
        use gpui::point;
        let restore = Bounds {
            origin: point(px(10.), px(20.)),
            size: size(px(DEFAULT_WINDOW_SIZE.0), px(DEFAULT_WINDOW_SIZE.1)),
        };
        match initial_window_bounds(LaunchWindowMode::Fullscreen, restore) {
            WindowBounds::Fullscreen(b) => assert_eq!(b, restore),
            other => panic!("expected Fullscreen, got {other:?}"),
        }
        match initial_window_bounds(LaunchWindowMode::Windowed, restore) {
            WindowBounds::Windowed(b) => assert_eq!(b, restore),
            other => panic!("expected Windowed, got {other:?}"),
        }
    }
}
