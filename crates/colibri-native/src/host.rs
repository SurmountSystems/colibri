//! Thin helpers around colibri-sys for the GPUI shell.
//!
//! Inference defaults to a **C engine subprocess** (serve mux) unless this
//! binary is built with Cargo feature `ffi`. With `feature = "ffi"`, start
//! tries multi-family CPU static open/generate first and falls back to the
//! process path on open/generate failure. `COLIBRI_FORCE_PROCESS=1` always
//! forces the process path (and still wins over any prefer-FFI flag).
//! `ColibriConfig::prefer_process` stays process-prefer for library embeds;
//! only this native host flips the start default under `feature = "ffi"`.
//! On pure FFI, Brain / PROF / HWINFO come from `coli_*_visual_poll` (GLM full;
//! other families may return empty until fill lands). Mid-generate STOP is
//! cooperative via the token callback (no mux multi-slot STOP).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(feature = "ffi")]
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use colibri_sys::{
    ChatMessage, ChatRenderOptions, ClientFrame, ColibriConfig, DoctorOptions, ENGINE_CHILD_NICE,
    EngineDuplex, EngineHandle, EngineLocate, EnsureModelDir, ExpertHits, ExpertMap, GB,
    HwinfoSnap, MachineInfo, ModelEntry, ModelFamily, ModelRegistry, ModelStatus, PlacementPlan,
    PlanOptions, ProbeOptions, ProfileTurn, ServerFrame, SupportedModel, TiersSnap, VisualSnapshot,
    default_model_store_path, ensure_model_directory, environment_for_plan, expand_user_path,
    force_process_from_env, format_engine_start_log, format_generate_log, locate_engine,
    model_arch, plan_cannot_hold_one_expert_slot, ram_overcommit_from_env, render_chat, run_doctor,
    set_current_thread_nice, supported_model_by_id, supported_models,
};

use crate::prefs;

#[cfg(feature = "ffi")]
use colibri_sys::ffi::{self as coli_ffi, FfiEngine, FfiFamily, FfiGenerateOptions};

/// Kill-switch env key (same string as colibri-sys).
pub const FORCE_PROCESS_ENV: &str = "COLIBRI_FORCE_PROCESS";

/// Product-owned store/path notes TOML (not a Hugging Face transformers config).
///
/// Written next to an empty model path / store root so the folder is scaffolded
/// without inventing a fake `config.json`. Real model leaves still need HF
/// `config.json` + weights for inference.
pub const STORE_CONFIG_FILE_NAME: &str = "colibri.toml";

/// Default body for [`STORE_CONFIG_FILE_NAME`].
pub fn default_store_colibri_toml() -> &'static str {
    "# Colibri path notes (product-owned). Not a Hugging Face model config.\n\
     # A model leaf needs config.json + weight files.\n\
     # Use Install a model or set the path to a folder that has those files.\n\
     \n\
     version = 1\n"
}

/// Ensure `dir/colibri.toml` exists. Returns `true` when this call wrote the file.
///
/// Does not create `dir` (caller must ensure the directory exists). Does not
/// write a transformers `config.json`.
pub fn ensure_store_colibri_toml(dir: &Path) -> std::io::Result<bool> {
    if !dir.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            "model path is not a directory",
        ));
    }
    let path = dir.join(STORE_CONFIG_FILE_NAME);
    if path.is_file() {
        return Ok(false);
    }
    fs::write(&path, default_store_colibri_toml())?;
    Ok(true)
}

/// Best-effort scaffold for doctor: store `colibri.toml` + UI `native-ui.toml`.
///
/// Returns whether `colibri.toml` was written on this call. Prefs errors are
/// ignored so doctor copy still focuses on the model path.
pub fn scaffold_doctor_defaults(model_dir: &Path) -> bool {
    let wrote_store = ensure_store_colibri_toml(model_dir).unwrap_or(false);
    let _ = prefs::ensure_default_prefs_file();
    wrote_store
}

#[cfg(feature = "install")]
use colibri_sys::disk_free_bytes;
#[cfg(feature = "install")]
use colibri_sys::install::{
    INSTALL_CANCELLED_MSG, INSTALL_PAUSED_MSG, InstallCancel, InstallLiveProgress, InstallOptions,
    InstallProgress, InstallResult, InstallSource, install_model_cancellable_live,
};

/// Resolve model path from env (`COLIBRI_MODEL` then `COLI_MODEL`) or empty.
///
/// Expands a leading `~` / `~/` so env values like `~/.models/foo` work.
pub fn env_model_path() -> Option<PathBuf> {
    std::env::var_os("COLIBRI_MODEL")
        .or_else(|| std::env::var_os("COLI_MODEL"))
        .map(|p| expand_user_path(PathBuf::from(p)))
}

/// Resolve engine binary override from env.
///
/// Expands a leading `~` / `~/` on the override path.
pub fn env_engine_path() -> Option<PathBuf> {
    std::env::var_os("COLI_ENGINE")
        .or_else(|| std::env::var_os("COLIBRI_ENGINE"))
        .map(|p| expand_user_path(PathBuf::from(p)))
}

/// Probe the host machine (sync; cheap on Linux).
pub fn probe_machine() -> Result<MachineInfo, String> {
    MachineInfo::probe_with(&ProbeOptions::default()).map_err(|e| e.to_string())
}

/// Short product summary for the Machine panel (default view).
///
/// RAM, cores, and GPU name(s) when present. Use [`format_machine_details`] for
/// SIMD / NPU / store inventory.
pub fn format_machine_summary(m: &MachineInfo) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "Memory: {:.1} GB free of {:.1} GB",
        m.available_memory as f64 / GB as f64,
        m.total_memory as f64 / GB as f64
    ));
    let core_label = if m.logical_cores != m.physical_cores {
        format!(
            "CPU: {} cores ({} threads)",
            m.physical_cores, m.logical_cores
        )
    } else {
        format!("CPU: {} cores", m.physical_cores)
    };
    if let Some(ref name) = m.cpu.model_name {
        lines.push(format!("{core_label} · {name}"));
    } else {
        lines.push(core_label);
    }
    if m.gpus.is_empty() {
        lines.push("GPU: none detected".into());
    } else {
        for g in &m.gpus {
            let vram_gb = g.total_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
            let mut line = if vram_gb > 0.05 {
                format!("GPU: {} ({:.0} GB)", g.name, vram_gb)
            } else {
                format!("GPU: {}", g.name)
            };
            if let Some(ref arch) = g.arch {
                line.push_str(&format!(" · {arch}"));
            } else if !g.vendor.is_empty() {
                line.push_str(&format!(" · {}", g.vendor));
            }
            lines.push(line);
        }
    }
    lines.join("\n")
}

/// Advanced machine inventory: swap, SIMD, NPU, model store path, CPU generation.
pub fn format_machine_details(m: &MachineInfo) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "Swap: {:.1} GB free of {:.1} GB",
        m.swap_free as f64 / GB as f64,
        m.swap_total as f64 / GB as f64
    ));
    if m.sockets > 1 {
        lines.push(format!("CPU sockets: {}", m.sockets));
    }
    if let Some(ref hint) = m.cpu.generation_hint {
        lines.push(format!("CPU generation: {hint}"));
    }
    let present: Vec<&str> = m
        .cpu
        .simd
        .iter()
        .filter(|s| s.present)
        .map(|s| s.name.as_str())
        .collect();
    if present.is_empty() {
        lines.push("SIMD: none reported".into());
    } else {
        lines.push(format!("SIMD: {}", present.join(", ")));
    }
    lines.push(format!(
        "Model store: {} ({:.1} GB free)",
        m.model_store.path.display(),
        m.model_store.free_bytes as f64 / GB as f64
    ));
    if m.npus.is_empty() {
        lines.push("NPU: none detected".into());
    } else {
        for n in &m.npus {
            lines.push(format!("NPU: {} ({})", n.name, n.kind));
        }
    }
    for g in &m.gpus {
        let mut line = format!(
            "GPU{} free {:.1} / {:.1} GB",
            g.index,
            g.free_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
            g.total_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
        );
        if !g.source.is_empty() {
            line.push_str(&format!(" · via {}", g.source));
        }
        lines.push(line);
    }
    lines.join("\n")
}

/// Machine panel body: summary only, or summary plus advanced details.
pub fn format_machine(m: &MachineInfo, expanded: bool) -> String {
    if expanded {
        format!(
            "{}\n\nDetails\n{}",
            format_machine_summary(m),
            format_machine_details(m)
        )
    } else {
        format_machine_summary(m)
    }
}

/// Map doctor overall status to a short product label.
fn doctor_overall_label(status: &str) -> &'static str {
    match status {
        "ok" => "Pass",
        "warning" => "Warning",
        "error" => "Fail",
        other if other.eq_ignore_ascii_case("ok") => "Pass",
        other if other.eq_ignore_ascii_case("warning") => "Warning",
        other if other.eq_ignore_ascii_case("error") => "Fail",
        _ => "Unknown",
    }
}

/// Checklist mark for one doctor check (`pass` / `warn` / `fail` / `skip`).
fn doctor_check_mark(status: &str) -> &'static str {
    match status {
        "pass" => "pass",
        "warn" => "warn",
        "fail" => "fail",
        "skip" => "skip",
        other if other.eq_ignore_ascii_case("pass") => "pass",
        other if other.eq_ignore_ascii_case("warn") => "warn",
        other if other.eq_ignore_ascii_case("fail") => "fail",
        other if other.eq_ignore_ascii_case("skip") => "skip",
        _ => "?",
    }
}

/// Sort rank so fail rows paint first in short Doctor panels (fail, warn, pass, skip).
fn doctor_check_sort_rank(status: &str) -> u8 {
    match status {
        "fail" => 0,
        other if other.eq_ignore_ascii_case("fail") => 0,
        "warn" => 1,
        other if other.eq_ignore_ascii_case("warn") => 1,
        "pass" => 2,
        other if other.eq_ignore_ascii_case("pass") => 2,
        "skip" => 3,
        other if other.eq_ignore_ascii_case("skip") => 3,
        _ => 4,
    }
}

/// Plain-English depth line for a doctor report (`standard` / `deep`).
fn doctor_depth_label(mode: &str) -> &'static str {
    match mode {
        "deep" => "Depth: thorough (tensor headers and shards)",
        "standard" => "Depth: quick",
        other if other.eq_ignore_ascii_case("deep") => {
            "Depth: thorough (tensor headers and shards)"
        }
        other if other.eq_ignore_ascii_case("standard") => "Depth: quick",
        _ => "Depth: quick",
    }
}

/// True when the model path is unset for doctor (empty / whitespace only).
///
/// A deliberate `.` (cwd) is **not** unset and still runs real doctor.
pub fn model_path_unset_for_doctor(path: &Path) -> bool {
    path.as_os_str().is_empty() || path.to_string_lossy().trim().is_empty()
}

/// Friendly checklist when no model is selected (host policy; no sys doctor call).
pub fn format_idle_doctor_checklist() -> String {
    "Overall: Idle\n\
     Path: (none selected)\n\
     Set a model path, or use Scan for models / Install a model.\n"
        .into()
}

/// Compact recovery checklist when the model folder path does not exist and
/// was not created (legacy / pre-create branch). Prefer
/// [`format_could_not_create_model_directory`] when create failed.
///
/// Four short lines max. Buttons (Scan for models / Install a model) do the
/// work; no env-var soup, no multi-paragraph essay, no sys doctor fail dump.
pub fn format_missing_model_directory(path: &Path, store: &Path) -> String {
    format!(
        "Overall: Needs model\n\
         Path: {}\n\
         This folder is missing.\n\
         Default store: {}\n",
        path.display(),
        store.display()
    )
}

/// Path was missing; doctor created the folder (and store notes TOML). Empty store ready for install.
pub fn format_created_model_directory(path: &Path, store: &Path) -> String {
    format!(
        "Overall: Needs model\n\
         Path: {}\n\
         Created this folder and default colibri.toml.\n\
         Install a model or scan. Default store: {}\n",
        path.display(),
        store.display()
    )
}

/// Path was missing and `create_dir_all` failed (permissions, parent is a file, …).
pub fn format_could_not_create_model_directory(
    path: &Path,
    store: &Path,
    err: &std::io::Error,
) -> String {
    format!(
        "Overall: Needs model\n\
         Path: {}\n\
         Could not create this folder: {err}\n\
         Default store: {}\n",
        path.display(),
        store.display()
    )
}

/// Compact recovery when the path exists but is not a model leaf (no HF config.json).
///
/// Prefer this over dumping sys doctor fail lines. When `toml_created` is true,
/// mention that doctor just wrote product-owned `colibri.toml` here.
pub fn format_not_a_model_folder(path: &Path, store: &Path) -> String {
    format_not_a_model_folder_ex(path, store, false)
}

/// Same as [`format_not_a_model_folder`] with an optional "created colibri.toml" line.
pub fn format_not_a_model_folder_ex(path: &Path, store: &Path, toml_created: bool) -> String {
    if toml_created {
        format!(
            "Overall: Needs model\n\
             Path: {}\n\
             Created default colibri.toml here.\n\
             This folder is not a model yet. Use Install a model or paste a model folder.\n",
            path.display()
        )
    } else {
        format!(
            "Overall: Needs model\n\
             Path: {}\n\
             This folder is not a model yet. Use Install a model or paste a model folder.\n\
             Default store: {}\n",
            path.display(),
            store.display()
        )
    }
}

/// Short memory-plan copy when the path is not a model leaf yet.
pub fn format_plan_not_a_model_folder() -> String {
    "No memory plan yet. Not a model folder yet. Install a model or paste a model folder.".into()
}

/// Plain status body when Start engine is blocked because the path is not a model.
///
/// Prefixed by the UI with `Could not start engine: `.
pub const ENGINE_START_NOT_A_MODEL: &str = "this folder is not a model yet. Use Install a model or choose a folder with config.json and weights.";

/// Start refuse when even one expert slot cannot fit in available RAM.
pub const ENGINE_START_RAM_TOO_SMALL: &str = "not enough RAM for even one expert working set. Free memory or set COLI_RAM_OVERCOMMIT=1 to start anyway.";
/// Inspect failed and overcommit is off: refuse rather than skip the RAM gate.
pub const ENGINE_START_RAM_UNMEASURABLE: &str = "could not measure RAM for this folder. Free memory or set COLI_RAM_OVERCOMMIT=1 to start anyway.";

/// Keys this process wrote via [`apply_plan_env_for_ffi`]. Operator-set keys
/// (present before we wrote them) stay sticky across Start.
static PLAN_ENV_WRITTEN: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);

static FFI_OPEN_ATTEMPTS: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
pub(crate) fn ffi_open_attempts() -> usize {
    FFI_OPEN_ATTEMPTS.load(Ordering::SeqCst)
}

#[cfg(test)]
pub(crate) fn reset_ffi_open_attempts() {
    FFI_OPEN_ATTEMPTS.store(0, Ordering::SeqCst);
}

fn record_ffi_open_attempt() {
    FFI_OPEN_ATTEMPTS.fetch_add(1, Ordering::SeqCst);
}

/// RAM vs one-slot floor. Doctor stays warn; Start is the gate.
///
/// `available_memory` injects a fixture (tests). `None` probes the machine.
/// Does not call `open_engine`.
pub fn preflight_ram_for_engine_start(
    model: &Path,
    available_memory: Option<u64>,
    overcommit: bool,
) -> Result<(), String> {
    if overcommit {
        return Ok(());
    }
    let opts = PlanOptions {
        available_memory,
        ..Default::default()
    };
    let plan = match PlacementPlan::build(model, &opts) {
        Ok(p) => p,
        Err(_) => return Err(ENGINE_START_RAM_UNMEASURABLE.into()),
    };
    let available = available_memory.unwrap_or(plan.tiers.ram.available_bytes);
    let floor = plan
        .tiers
        .ram
        .dense_bytes
        .saturating_add(plan.tiers.ram.runtime_bytes)
        .saturating_add(plan.model.per_cap_bytes.max(1));
    if plan_cannot_hold_one_expert_slot(&plan) || (available > 0 && floor > available) {
        return Err(ENGINE_START_RAM_TOO_SMALL.into());
    }
    Ok(())
}

/// Same order as Start: leaf check, then RAM gate. Does not open the engine.
pub fn preflight_then_maybe_open(
    model: &Path,
    available_memory: Option<u64>,
    overcommit: bool,
) -> Result<PathBuf, String> {
    let model = preflight_model_for_engine_start(model)?;
    preflight_ram_for_engine_start(&model, available_memory, overcommit)?;
    Ok(model)
}

fn apply_plan_env_for_ffi(model: &Path) {
    let plan = match PlacementPlan::build(model, &PlanOptions::default()) {
        Ok(p) => p,
        Err(_) => return,
    };
    let env = environment_for_plan(&plan, None, false);
    let mut guard = PLAN_ENV_WRITTEN.lock().unwrap_or_else(|e| e.into_inner());
    let written = guard.get_or_insert_with(HashSet::new);
    for key in ["RAM_GB", "OMP_NUM_THREADS"] {
        let already_set = std::env::var_os(key).is_some();
        let we_wrote = written.contains(key);
        if already_set && !we_wrote {
            // Operator (or parent process) set this before we did. Leave it.
            continue;
        }
        if let Some(v) = env.get(key) {
            // Refresh values this function wrote so a later Start sees the new plan.
            unsafe {
                std::env::set_var(key, v);
            }
            written.insert(key);
        }
    }
}

#[cfg(test)]
fn clear_plan_env_written_for_tests() {
    let mut guard = PLAN_ENV_WRITTEN.lock().unwrap_or_else(|e| e.into_inner());
    *guard = None;
}

/// Cooperative FFI generate stop must not start a serve child.
fn ffi_generate_error_should_fallback(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    !(lower.contains("stopped") || lower.contains("cancel"))
}

/// Working-set refuse from C open must not start a serve child (that child loads again).
fn ffi_open_error_should_fallback(err: &str) -> bool {
    if err == ENGINE_START_RAM_TOO_SMALL || err == ENGINE_START_RAM_UNMEASURABLE {
        return false;
    }
    let lower = err.to_ascii_lowercase();
    !(lower.contains("not enough ram") || lower.contains("could not measure ram"))
}

/// True when `path` is a directory with Hugging Face `config.json` (model leaf).
pub fn is_model_leaf(path: &Path) -> bool {
    path.is_dir() && path.join("config.json").is_file()
}

/// Expand and validate the model path before spawning or opening the engine.
///
/// Rejects empty, missing, non-directory, and non-leaf paths without calling
/// open or process serve. On success returns the expanded absolute-ish path.
pub fn preflight_model_for_engine_start(model: &Path) -> Result<PathBuf, String> {
    let model = expand_user_path(model);
    if model_path_unset_for_doctor(&model) || !is_model_leaf(&model) {
        return Err(ENGINE_START_NOT_A_MODEL.into());
    }
    Ok(model)
}

/// Map lab engine-start errors to plain status text (no `Could not start engine:` prefix).
///
/// Covers serve protocol EOF / READY handshake failures and missing engine binary.
/// Always includes the model path on mapped lab failures so the operator can see
/// what was attempted.
pub fn map_engine_start_error(err: &str, model: &Path) -> String {
    // Already product-plain (preflight or prior map).
    if err.starts_with("this folder is not a model yet")
        || err.starts_with("engine quit before it was ready")
        || err.starts_with("engine binary not found")
        || err.starts_with("not enough RAM")
        || err.starts_with("could not measure RAM")
    {
        return err.to_string();
    }

    let lower = err.to_ascii_lowercase();
    let path = model.display();

    if lower.contains("eof before ready")
        || lower.contains("serve protocol error")
        || lower.contains("waiting for ready")
    {
        return format!(
            "engine quit before it was ready (often bad model path or missing engine). Model: {path}"
        );
    }

    if lower.contains("not built or not on search path")
        || lower.contains("override not found")
        || (lower.contains("coli_engine")
            && (lower.contains("not found") || lower.contains("override")))
    {
        return format!(
            "engine binary not found. Build the engine or set COLI_ENGINE. Model: {path}"
        );
    }

    // Drop the internal "engine start failed:" wrapper when present so status stays shorter.
    let trimmed = err
        .strip_prefix("engine start failed: ")
        .unwrap_or(err)
        .trim();
    if trimmed != err {
        return map_engine_start_error(trimmed, model);
    }
    err.to_string()
}

/// Format a doctor report as a checklist (not a raw CLI dump).
///
/// When overall is Fail, the first fail summary is appended on the Overall line
/// so short Doctor panels still show why. Check rows are ordered fail, then
/// warn, then pass/skip (stable within each group).
pub fn format_doctor_checklist(report: &colibri_sys::DoctorReport) -> String {
    let overall = doctor_overall_label(&report.status);
    let model_line = if report.model.is_empty() || report.model == "." {
        "Model: (none selected)".to_string()
    } else {
        format!("Model: {}", report.model)
    };
    let depth = doctor_depth_label(&report.mode);
    let overall_line = if report.status.eq_ignore_ascii_case("error") {
        match report
            .checks
            .iter()
            .find(|c| c.status.eq_ignore_ascii_case("fail"))
            .map(|c| c.summary.trim())
            .filter(|s| !s.is_empty())
        {
            Some(reason) => format!("Overall: {overall} · {reason}"),
            None => format!("Overall: {overall}"),
        }
    } else {
        format!("Overall: {overall}")
    };
    let mut out = format!("{overall_line}\n{model_line}\n{depth}\n");
    if report.checks.is_empty() {
        out.push_str("No checks reported.\n");
        return out;
    }
    out.push('\n');
    let mut ordered: Vec<&colibri_sys::DoctorCheck> = report.checks.iter().collect();
    ordered.sort_by_key(|c| doctor_check_sort_rank(&c.status));
    for c in ordered {
        let mark = doctor_check_mark(&c.status);
        // Prefer human summary; fall back to id when summary is empty.
        let label = if c.summary.trim().is_empty() {
            c.id.as_str()
        } else {
            c.summary.as_str()
        };
        out.push_str(&format!("[{mark}] {label}\n"));
    }
    out
}

/// Ensure the expanded model path exists on disk (create parents if needed).
///
/// Used by doctor / recovery only. Returns whether this call created the path.
/// On create failure, returns the formatted short error checklist.
pub fn ensure_model_path_for_doctor(
    model: &Path,
    machine: Option<&MachineInfo>,
) -> Result<EnsureModelDir, String> {
    let store = machine
        .map(|m| m.model_store.path.as_path())
        .map(Path::to_path_buf)
        .unwrap_or_else(default_model_store_path);
    match ensure_model_directory(model) {
        Ok(outcome) => Ok(outcome),
        Err(e) => Err(format_could_not_create_model_directory(model, &store, &e)),
    }
}

/// Run doctor for a model path (may be missing; checks still report).
///
/// Empty path returns an idle checklist without calling sys doctor (avoids
/// treating cwd as a model). Missing path (after `~` expand) is created with
/// `create_dir_all` when possible; success returns a short "created" checklist,
/// failure a plain create error. Existing non-model directories get a short
/// recovery checklist and a product `colibri.toml` scaffold (never a fake HF
/// `config.json`). Real model leaves (with `config.json`) run full sys doctor.
/// Also ensures UI prefs TOML (`native-ui.toml`) exists when missing.
/// `deep: false` is the quick checklist. `deep: true` also validates
/// safetensors headers, shard sequence, required tensors, index, and optional
/// mirror when the path has model files.
///
/// Expands a leading `~` / `~/` on the model path. Engine path uses env override
/// when set, otherwise family-aware [`locate_engine`] so doctor matches start.
pub fn run_doctor_checks(model: &Path, machine: Option<&MachineInfo>, deep: bool) -> String {
    if model_path_unset_for_doctor(model) {
        return format_idle_doctor_checklist();
    }
    let model = expand_user_path(model);
    let store = machine
        .map(|m| m.model_store.path.as_path())
        .map(Path::to_path_buf)
        .unwrap_or_else(default_model_store_path);
    if !model.exists() {
        match ensure_model_path_for_doctor(&model, machine) {
            Ok(EnsureModelDir::Created) => {
                // Folder ready; scaffold product TOML; still no HF model until install.
                let _ = scaffold_doctor_defaults(&model);
                return format_created_model_directory(&model, &store);
            }
            Ok(EnsureModelDir::AlreadyExists) => {
                // Race: path appeared between exists check and ensure.
            }
            Err(msg) => return msg,
        }
    }
    // Empty / non-model directory: short recovery + product TOML (not a long check dump).
    if model.is_dir() && !model.join("config.json").is_file() {
        let toml_created = scaffold_doctor_defaults(&model);
        return format_not_a_model_folder_ex(&model, &store, toml_created);
    }
    // Real model leaf: still ensure UI prefs exist; do not touch store colibri.toml.
    let _ = prefs::ensure_default_prefs_file();
    let mut opts = DoctorOptions {
        deep,
        ..Default::default()
    };
    if let Some(m) = machine {
        opts.available_memory = Some(m.available_memory);
        opts.available_disk = Some(m.model_store.free_bytes);
        opts.ram_gb = m.available_memory as f64 / GB as f64;
        opts.gpus = Some(m.gpus.clone());
    }
    // Prefer env override; else pre-resolve via locate_engine (family-aware).
    // Doctor also locates when engine_path is None; setting it keeps the host
    // and start path aligned when search succeeds.
    if let Some(engine) = env_engine_path() {
        opts.engine_path = Some(engine);
    } else {
        let family = model_arch(&model);
        if let Ok(found) = locate_engine(EngineLocate {
            family,
            override_path: None,
            search_roots: vec![],
        }) {
            opts.engine_path = Some(found);
        }
    }
    // When this binary is built with `ffi` and not kill-switched, tell doctor
    // so engine.binary can pass as in-process instead of "not built".
    #[cfg(feature = "ffi")]
    {
        if !resolve_prefer_process() {
            opts.in_process_engine = Some(true);
        }
    }
    match run_doctor(&model, &opts) {
        Ok(report) => format_doctor_checklist(&report),
        Err(e) => format!("Doctor could not run: {e}"),
    }
}

/// Run shallow (quick) doctor for a model path.
pub fn run_shallow_doctor(model: &Path, machine: Option<&MachineInfo>) -> String {
    run_doctor_checks(model, machine, false)
}

/// Run thorough (deep) doctor for a model path.
pub fn run_deep_doctor(model: &Path, machine: Option<&MachineInfo>) -> String {
    run_doctor_checks(model, machine, true)
}

/// Empty-store scan message: short, no env-var soup (buttons handle recovery).
pub fn format_empty_registry_scan(store: &Path) -> String {
    format!(
        "No models under {} (folders with config.json, depth ≤2).",
        store.display()
    )
}

/// Status after a successful non-empty registry scan.
pub fn format_registry_scan_status(count: usize, store: &Path) -> String {
    format!(
        "{count} model(s) under {} · click a row to set model path",
        store.display()
    )
}

/// Models worth offering for auto-select / picker (have config.json).
pub fn usable_registry_models(entries: &[ModelEntry]) -> Vec<&ModelEntry> {
    entries
        .iter()
        .filter(|e| {
            matches!(
                e.status,
                ModelStatus::Present | ModelStatus::Incomplete | ModelStatus::MissingTokenizer
            )
        })
        .collect()
}

/// When exactly one usable model is under the store, return its path.
pub fn pick_single_usable_model(entries: &[ModelEntry]) -> Option<PathBuf> {
    let usable = usable_registry_models(entries);
    if usable.len() == 1 {
        Some(usable[0].path.clone())
    } else {
        None
    }
}

/// Doctor text when the typed path is not a model leaf and scan found several models.
pub fn format_missing_path_many_models(
    missing: &Path,
    store: &Path,
    entries: &[ModelEntry],
) -> String {
    let usable = usable_registry_models(entries);
    let mut out = if missing.exists() {
        format_not_a_model_folder(missing, store)
    } else {
        format_missing_model_directory(missing, store)
    };
    out.push_str(&format!(
        "Found {} model(s) under the store. Click a row:\n",
        usable.len()
    ));
    for e in usable.iter().take(12) {
        out.push_str(&format!("· {}\n", e.path.display()));
    }
    if usable.len() > 12 {
        out.push_str(&format!("· ...and {} more\n", usable.len() - 12));
    }
    out
}

/// Result of scanning the default store when the current model path is missing.
#[derive(Debug, Clone)]
pub enum MissingPathScanOutcome {
    /// Exactly one usable model; host should set the path and re-run doctor.
    AutoSelected {
        path: PathBuf,
        doctor: String,
        status: String,
    },
    /// Several models; host should fill the registry picker and show this text.
    ListedMany {
        doctor: String,
        status: String,
        entries: Vec<ModelEntry>,
    },
    /// Store empty or no usable models; recovery-only doctor text.
    StillMissing { doctor: String, status: String },
}

/// Scan-aware recovery when `missing` does not exist.
///
/// Pure over already-scanned `entries` (depth ≤2 under the store). When one
/// model is found, runs doctor on it and returns the path to apply.
pub fn missing_path_scan_outcome(
    missing: &Path,
    store: &Path,
    entries: &[ModelEntry],
    machine: Option<&MachineInfo>,
    deep: bool,
) -> MissingPathScanOutcome {
    let path_note = if missing.exists() {
        format!("Path is not a model folder yet: {}", missing.display())
    } else {
        format!("Path was missing: {}", missing.display())
    };
    if let Some(path) = pick_single_usable_model(entries) {
        let checklist = run_doctor_checks(&path, machine, deep);
        let doctor = format!(
            "{path_note}. Found one model under the default store and set it.\n\n{checklist}"
        );
        let status = format!("Using only model under {}", store.display());
        return MissingPathScanOutcome::AutoSelected {
            path,
            doctor,
            status,
        };
    }
    let usable = usable_registry_models(entries);
    if usable.len() > 1 {
        let doctor = format_missing_path_many_models(missing, store, entries);
        let status = format!("{} models under store · pick one", usable.len());
        return MissingPathScanOutcome::ListedMany {
            doctor,
            status,
            entries: entries.to_vec(),
        };
    }
    let doctor = if missing.exists() {
        format_not_a_model_folder(missing, store)
    } else {
        format_missing_model_directory(missing, store)
    };
    MissingPathScanOutcome::StillMissing {
        doctor,
        status: format!("Scan or install under {}", store.display()),
    }
}

/// What to put in the model path field on cold start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupModelPath {
    /// Text for the rail / wizard path field.
    pub display: String,
    /// Optional status line (auto-pick note, default store hint).
    pub note: Option<String>,
}

/// Resolve the initial model path for the rail and wizard.
///
/// Order: `COLIBRI_MODEL` / `COLI_MODEL` → last prefs path when it exists →
/// auto-pick when prefs path is missing and the store has one model → first
/// usable store model when prefs empty → default model store path (product
/// store, not a random `~/.models`).
pub fn resolve_startup_model_path(
    env: Option<PathBuf>,
    prefs_last: &str,
    store: &Path,
    store_entries: &[ModelEntry],
) -> StartupModelPath {
    if let Some(p) = env {
        return StartupModelPath {
            display: p.display().to_string(),
            note: None,
        };
    }
    let prefs = prefs_last.trim();
    if !prefs.is_empty() {
        let expanded = expand_user_path(Path::new(prefs));
        if expanded.exists() {
            return StartupModelPath {
                display: prefs.to_string(),
                note: None,
            };
        }
        if let Some(one) = pick_single_usable_model(store_entries) {
            return StartupModelPath {
                display: one.display().to_string(),
                note: Some(format!(
                    "Saved path {} is missing; using the only model under {}",
                    expanded.display(),
                    store.display()
                )),
            };
        }
        // Keep the typed path so Doctor can show recovery; do not invent ~/.models.
        return StartupModelPath {
            display: prefs.to_string(),
            note: Some(format!(
                "Saved path {} is missing · Scan models or Install under {}",
                expanded.display(),
                store.display()
            )),
        };
    }
    if let Some(one) = pick_single_usable_model(store_entries) {
        return StartupModelPath {
            display: one.display().to_string(),
            note: None,
        };
    }
    let usable = usable_registry_models(store_entries);
    if let Some(first) = usable.first() {
        return StartupModelPath {
            display: first.path.display().to_string(),
            note: Some(format!(
                "{} models under {}; first selected · pick another via Scan",
                usable.len(),
                store.display()
            )),
        };
    }
    // Empty prefs, empty store: show the product default store path.
    StartupModelPath {
        display: store.display().to_string(),
        note: Some(format!(
            "Default model store · Scan models or Install under {}",
            store.display()
        )),
    }
}

/// Build placement plan text for a model path.
///
/// Expands a leading `~` / `~/` on the model path. When the path exists but is
/// not a model leaf (no HF `config.json`), returns short recovery copy instead
/// of a raw "missing config.json" error.
pub fn run_plan(model: &Path, machine: Option<&MachineInfo>) -> String {
    if model_path_unset_for_doctor(model) {
        return "No memory plan yet. Set a model path first.".into();
    }
    let model = expand_user_path(model);
    if !model.exists() {
        // Keep short: Health check already owns recovery copy for missing path.
        return "No memory plan yet. Fix the model path above first.".into();
    }
    if model.is_dir() && !model.join("config.json").is_file() {
        return format_plan_not_a_model_folder();
    }
    let mut opts = PlanOptions::default();
    if let Some(m) = machine {
        opts.available_memory = Some(m.available_memory);
        opts.gpus = Some(m.gpus.clone());
        opts.physical_cpus = Some(m.physical_cores);
        opts.cpu_sockets = Some(m.sockets);
        opts.available_disk = Some(m.model_store.free_bytes);
    }
    match PlacementPlan::build(&model, &opts) {
        Ok(plan) => format_plan(&plan),
        Err(e) => {
            let msg = e.to_string();
            // Belt-and-suspenders: never surface only "missing config.json" in the UI.
            if msg.contains("missing config.json") {
                return format_plan_not_a_model_folder();
            }
            format!("Could not build memory plan: {msg}")
        }
    }
}

/// Plain readiness copy for wizard step 4 and Tools plan panel.
///
/// Lab field dump lives in [`format_plan_lab`] for debugging only.
fn format_plan(plan: &PlacementPlan) -> String {
    format_plan_readiness(plan)
}

/// Wizard/Tools-facing pass/fail readiness (no lab field names).
pub(crate) fn format_plan_readiness(plan: &PlacementPlan) -> String {
    let hit_pct = plan.projected_hit_rate * 100.0;
    let ram_gb = plan.tiers.ram.budget_bytes as f64 / GB as f64;
    let vram_gb = plan.tiers.vram.budget_bytes as f64 / GB as f64;
    let bottleneck = plain_bottleneck_label(&plan.expected_bottleneck, &plan.bottleneck_class);
    let ready = plan.warnings.is_empty();

    let mut out = if ready {
        "Memory plan: ready to run\n".to_string()
    } else {
        "Memory plan: review warnings before start\n".to_string()
    };
    out.push_str(&format!("Expected cache hit rate: {hit_pct:.0}%\n"));
    out.push_str(&format!("Memory on GPU: {vram_gb:.1} GB\n"));
    out.push_str(&format!("System RAM budget: {ram_gb:.1} GB\n"));
    if plan.model.expert_count > 0 {
        out.push_str(&format!("Experts: {}\n", plan.model.expert_count));
    }
    if plan.model.shards > 0 {
        out.push_str(&format!("Weight files: {}\n", plan.model.shards));
    }
    out.push_str(&format!("Likely limit: {bottleneck}\n"));
    for n in &plan.notes {
        if !n.is_empty() {
            out.push_str(n);
            out.push('\n');
        }
    }
    for w in &plan.warnings {
        out.push_str(&format!("Warning: {w}\n"));
    }
    for d in plan.decisions.iter().take(4) {
        if !d.reason.is_empty() {
            out.push_str(&format!("· {}\n", d.reason));
        }
    }
    out
}

/// Map planner bottleneck fields to a short plain-English limit label.
fn plain_bottleneck_label(expected: &str, class: &str) -> String {
    let c = class.to_ascii_lowercase();
    let e = expected.to_ascii_lowercase();
    let hay = format!("{c} {e}");
    if hay.contains("vram") || hay.contains("gpu") {
        "GPU memory".into()
    } else if hay.contains("ram") || hay.contains("host") || hay.contains("system") {
        "system RAM".into()
    } else if hay.contains("disk") || hay.contains("ssd") || hay.contains("io") {
        "disk I/O".into()
    } else if expected.trim().is_empty() && class.trim().is_empty() {
        "none detected".into()
    } else if !expected.trim().is_empty() {
        expected.trim().to_string()
    } else {
        class.trim().to_string()
    }
}

/// Detailed lab dump (field names). Prefer [`format_plan_readiness`] in the UI.
#[allow(dead_code)]
fn format_plan_lab(plan: &PlacementPlan) -> String {
    let mut out = format!(
        "version={} policy={} hit={:.1}% bottleneck={}\n",
        plan.version,
        plan.policy.name,
        plan.projected_hit_rate * 100.0,
        plan.bottleneck_class
    );
    out.push_str(&format!(
        "model shards={} expert_count={} dense_bytes={}\n",
        plan.model.shards, plan.model.expert_count, plan.model.dense_bytes
    ));
    out.push_str(&format!(
        "ram budget {:.2} GB · cap/layer {}\n",
        plan.tiers.ram.budget_bytes as f64 / GB as f64,
        plan.tiers.ram.cache_slots_per_layer
    ));
    out.push_str(&format!(
        "expected_bottleneck={} ssd_probe_state={}\n",
        plan.expected_bottleneck, plan.ssd_probe_state
    ));
    for w in &plan.warnings {
        out.push_str(&format!("warn: {w}\n"));
    }
    for d in plan.decisions.iter().take(8) {
        out.push_str(&format!("  {}: {}\n", d.target, d.reason));
    }
    out
}

// ---------------------------------------------------------------------------
// Live visual / PROF formatting
// ---------------------------------------------------------------------------

/// One-line live memory-placement strip (GPU / system RAM / disk expert counts).
pub fn format_live_tiers(t: &TiersSnap) -> String {
    format!(
        "Experts in memory · GPU {} ({:.1} GB) · System RAM {} ({:.1} GB) · Disk {}",
        t.vram, t.vram_gb, t.ram, t.ram_gb, t.disk
    )
}

/// Empty-state copy for the live memory-placement strip.
pub fn live_tiers_idle_message(kind: LiveTiersIdle) -> &'static str {
    match kind {
        LiveTiersIdle::StartEngine => "Memory placement: start the engine to see live counts",
        LiveTiersIdle::EngineStopped => "Memory placement: engine stopped",
        LiveTiersIdle::Waiting => "Memory placement: waiting for live data…",
    }
}

/// Why the live tiers strip has no snapshot yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveTiersIdle {
    StartEngine,
    EngineStopped,
    Waiting,
}

/// One-line live engine hardware strip (from mux HWINFO, not the static probe).
///
/// Plain English labels (Phase A tone). Omits empty CPU/GPU names rather than
/// dumping raw frame field names.
pub fn format_live_hwinfo(h: &HwinfoSnap) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!(
        "RAM {:.1} / {:.1} GB free",
        h.ram_avail_gb, h.ram_total_gb
    ));
    if h.cores > 0 {
        parts.push(format!("{} cores", h.cores));
    }
    let cpu = h.cpu.trim();
    if !cpu.is_empty() {
        parts.push(cpu.to_string());
    }
    if h.gpus > 0 || h.vram_total_gb > 0.05 || !h.gpu.trim().is_empty() {
        let gpu_name = h.gpu.trim();
        let gpu_bit = if !gpu_name.is_empty() {
            if h.vram_total_gb > 0.05 {
                format!("GPU {gpu_name} ({:.0} GB)", h.vram_total_gb)
            } else {
                format!("GPU {gpu_name}")
            }
        } else if h.gpus > 0 {
            if h.vram_total_gb > 0.05 {
                format!("{} GPU(s) · {:.0} GB VRAM", h.gpus, h.vram_total_gb)
            } else {
                format!("{} GPU(s)", h.gpus)
            }
        } else {
            format!("{:.0} GB VRAM", h.vram_total_gb)
        };
        parts.push(gpu_bit);
    }
    format!("Engine hardware · {}", parts.join(" · "))
}

/// Empty-state copy for the live engine hardware strip.
pub fn live_hwinfo_idle_message(kind: LiveHwinfoIdle) -> &'static str {
    match kind {
        LiveHwinfoIdle::StartEngine => "Engine hardware: start the engine to see live stats",
        LiveHwinfoIdle::EngineStopped => "Engine hardware: engine stopped",
        LiveHwinfoIdle::Waiting => "Engine hardware: waiting for live data…",
    }
}

/// Why the live HWINFO strip has no snapshot yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveHwinfoIdle {
    StartEngine,
    EngineStopped,
    Waiting,
}

/// Format last N profile turns as a compact multi-line table (plain labels).
pub fn format_profile_turns(turns: &[ProfileTurn], last_n: usize) -> String {
    if turns.is_empty() {
        return "No timing data yet. Generate once to collect turns.".into();
    }
    let start = turns.len().saturating_sub(last_n);
    let shown = turns.len() - start;
    let mut out = format!("Recent turns (last {shown})\n");
    out.push_str("  #  wall    prompt  out    tok/s   disk   wait   matmul  attn\n");
    for (i, p) in turns[start..].iter().enumerate() {
        let idx = start + i + 1;
        let tok_s = if p.wall_s > 0.0 {
            p.completion_tokens as f64 / p.wall_s
        } else {
            0.0
        };
        out.push_str(&format!(
            "  {idx:<2} {wall:>6.2}s {prompt:>6} {comp:>6} {tok_s:>6.1}  {disk:>5.2}  {wait:>5.2}  {mm:>6.2}  {attn:>5.2}\n",
            wall = p.wall_s,
            prompt = p.prompt_tokens,
            comp = p.completion_tokens,
            tok_s = tok_s,
            disk = p.expert_disk_s,
            wait = p.expert_wait_s,
            mm = p.expert_matmul_s,
            attn = p.attention_s,
        ));
    }
    out
}

/// Default max cells drawn in the Brain panel (full maps can be 19k+ experts).
/// Override at runtime with full-grid mode (`COLIBRI_BRAIN_FULL=1` / UI toggle).
pub const BRAIN_MAX_CELLS: usize = 2048;

/// Sampled display grid for Brain. Full atlas may be larger; we stride-sample.
#[derive(Debug, Clone, Default)]
pub struct BrainView {
    /// Source map rows/cols before sampling (for status text).
    pub src_rows: u32,
    /// Source map cols before sampling (for status text).
    pub src_cols: u32,
    pub disp_rows: u32,
    pub disp_cols: u32,
    /// Source row index step for display row `dr` → `dr * row_stride`.
    pub row_stride: u32,
    /// Source col index step for display col `dc` → `dc * col_stride`.
    pub col_stride: u32,
    /// Cap used when building this view (`BRAIN_MAX_CELLS` or full-res).
    pub max_cells: usize,
    /// Display cell: (tier 0..2, heat 0..63, hit pulse 0..1).
    pub cells: Vec<(u8, u8, f32)>,
    pub hits_seq: u64,
    pub sampled: bool,
    pub note: String,
}

/// Env full-grid paint: `COLIBRI_BRAIN_FULL` / `COLI_BRAIN_FULL` = 1|true|yes.
pub fn env_brain_full() -> bool {
    std::env::var_os("COLIBRI_BRAIN_FULL")
        .or_else(|| std::env::var_os("COLI_BRAIN_FULL"))
        .map(|v| {
            let s = v.to_string_lossy();
            s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

/// Map a display cell under stride sampling back to the source expert index.
pub fn display_to_source(disp_r: u32, disp_c: u32, row_stride: u32, col_stride: u32) -> (u32, u32) {
    (
        disp_r.saturating_mul(row_stride.max(1)),
        disp_c.saturating_mul(col_stride.max(1)),
    )
}

/// Build a display grid with the default cell cap ([`BRAIN_MAX_CELLS`]).
pub fn brain_view_from_map(map: &ExpertMap, hits: Option<&ExpertHits>, prev_seq: u64) -> BrainView {
    brain_view_from_map_with_max(map, hits, prev_seq, BRAIN_MAX_CELLS)
}

/// Build a display grid from expert map + hits, stride-sampling if over `max_cells`.
///
/// Pass `usize::MAX` (or `rows*cols`) for full-resolution paint. Default product
/// path uses [`BRAIN_MAX_CELLS`] (2048) so large MoE maps stay light.
pub fn brain_view_from_map_with_max(
    map: &ExpertMap,
    hits: Option<&ExpertHits>,
    prev_seq: u64,
    max_cells: usize,
) -> BrainView {
    let src_rows = map.rows.max(1);
    let src_cols = map.cols.max(1);
    let total = (src_rows as usize).saturating_mul(src_cols as usize);
    let max_cells = max_cells.max(1);
    let (disp_rows, disp_cols, row_stride, col_stride, sampled) = if total <= max_cells {
        (src_rows, src_cols, 1u32, 1u32, false)
    } else {
        // Prefer keeping aspect; choose strides so product of display dims ≤ limit.
        let aspect = src_cols as f64 / src_rows as f64;
        let mut dr = ((max_cells as f64 / aspect).sqrt()).floor() as u32;
        dr = dr.clamp(1, src_rows);
        let mut dc = (max_cells as u32 / dr).max(1).min(src_cols);
        while (dr as usize).saturating_mul(dc as usize) > max_cells {
            if dc > 1 {
                dc -= 1;
            } else if dr > 1 {
                dr -= 1;
                dc = (max_cells as u32 / dr).max(1).min(src_cols);
            } else {
                break;
            }
        }
        // Stride so we cover the source grid with at most dr×dc samples.
        let row_stride = src_rows.div_ceil(dr).max(1);
        let col_stride = src_cols.div_ceil(dc).max(1);
        let disp_rows = src_rows.div_ceil(row_stride);
        let disp_cols = src_cols.div_ceil(col_stride);
        // Final safety: if ceil still overshoots, clamp by increasing stride.
        let mut row_stride = row_stride;
        let mut col_stride = col_stride;
        let mut disp_rows = disp_rows;
        let mut disp_cols = disp_cols;
        while (disp_rows as usize).saturating_mul(disp_cols as usize) > max_cells {
            if disp_cols >= disp_rows && col_stride < src_cols {
                col_stride += 1;
                disp_cols = src_cols.div_ceil(col_stride);
            } else if row_stride < src_rows {
                row_stride += 1;
                disp_rows = src_rows.div_ceil(row_stride);
            } else {
                break;
            }
        }
        (disp_rows, disp_cols, row_stride, col_stride, true)
    };

    let hits_seq = hits.map(|h| h.seq).unwrap_or(0);
    let pulse_on = hits.is_some() && hits_seq != prev_seq && hits_seq > 0;

    let mut cells = Vec::with_capacity((disp_rows as usize) * (disp_cols as usize));
    for dr in 0..disp_rows {
        for dc in 0..disp_cols {
            let (r, c) = display_to_source(dr, dc, row_stride, col_stride);
            let tier = map.tier_at(r, c).unwrap_or(0);
            let heat = map.heat_at(r, c).unwrap_or(0);
            let idx = (r as usize) * (src_cols as usize) + (c as usize);
            let hit = pulse_on && hits.is_some_and(|h| h.hit(idx));
            cells.push((tier, heat, if hit { 1.0 } else { 0.0 }));
        }
    }

    let note = if sampled {
        format!("sampled {disp_rows}×{disp_cols} of {src_rows}×{src_cols} (max {max_cells} cells)")
    } else {
        format!("full {src_rows}×{src_cols}")
    };

    BrainView {
        src_rows,
        src_cols,
        disp_rows,
        disp_cols,
        row_stride,
        col_stride,
        max_cells,
        cells,
        hits_seq,
        sampled,
        note,
    }
}

/// RGB for a Brain cell from theme + tier + heat + hit pulse (0..1).
///
/// - **Mint**: soft SPA curve matching web `Brain.tsx`
///   (`lum = 0.35 + 0.65 * min(heat / 24, 1)`), plus a warm pulse flash.
/// - **DOGE**: pure 8-color discrete map only (no soft midtones). Every return
///   value is one of [`crate::theme::DOGE_EIGHT`].
pub fn brain_cell_rgb(theme: crate::theme::ThemeId, tier: u8, heat: u8, pulse: f32) -> u32 {
    match theme {
        crate::theme::ThemeId::Doge => brain_cell_rgb_doge(tier, heat, pulse),
        crate::theme::ThemeId::Mint => brain_cell_rgb_mint(tier, heat, pulse),
    }
}

/// Soft mint / SPA heat map (web `TIER_RGB` + heat/24 lum + warm pulse).
fn brain_cell_rgb_mint(tier: u8, heat: u8, pulse: f32) -> u32 {
    // Base: disk gray, RAM blue, VRAM green (matches web TIER_RGB).
    let (br, bg, bb) = match tier {
        2 => (78u32, 214, 165), // VRAM
        1 => (90u32, 155, 216), // RAM
        _ => (58u32, 71, 80),   // disk
    };
    let heat_f = (heat as f32 / 24.0).clamp(0.0, 1.0);
    let lum = 0.35 + 0.65 * heat_f;
    let mut r = br as f32 * lum;
    let mut g = bg as f32 * lum;
    let mut b = bb as f32 * lum;
    // Hit pulse: flash toward warm white.
    if pulse > 0.05 {
        r = r + (255.0 - r) * pulse * 0.85;
        g = g + (240.0 - g) * pulse * 0.55;
        b = b + (180.0 - b) * pulse * 0.25;
    }
    let ri = r.clamp(0.0, 255.0) as u32;
    let gi = g.clamp(0.0, 255.0) as u32;
    let bi = b.clamp(0.0, 255.0) as u32;
    (ri << 16) | (gi << 8) | bi
}

/// DOGE pure discrete map: tier chooses family, heat/pulse pick among the eight.
///
/// | State | Color |
/// |-------|-------|
/// | hit pulse | White |
/// | heat 0 (never / cold) | Black |
/// | disk + warm | Blue |
/// | disk + hot (≥12) | Magenta |
/// | RAM + warm | Cyan |
/// | RAM + hot | Magenta |
/// | VRAM + warm | Green |
/// | VRAM + hot | Yellow |
fn brain_cell_rgb_doge(tier: u8, heat: u8, pulse: f32) -> u32 {
    use crate::theme::{
        DOGE_BLACK, DOGE_BLUE, DOGE_CYAN, DOGE_GREEN, DOGE_MAGENTA, DOGE_WHITE, DOGE_YELLOW,
    };
    if pulse > 0.05 {
        return DOGE_WHITE;
    }
    if heat == 0 {
        return DOGE_BLACK;
    }
    let hot = heat >= 12;
    match tier {
        2 => {
            if hot {
                DOGE_YELLOW
            } else {
                DOGE_GREEN
            }
        }
        1 => {
            if hot {
                DOGE_MAGENTA
            } else {
                DOGE_CYAN
            }
        }
        _ => {
            if hot {
                DOGE_MAGENTA
            } else {
                DOGE_BLUE
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Brain hits pulse decay (web RAF parity)
// ---------------------------------------------------------------------------

/// Web `Brain.tsx` multiplies per-cell pulse by this each RAF frame (~60 Hz).
pub const BRAIN_PULSE_RAF_FACTOR: f32 = 0.94;
/// Pulse intensities at or below this are treated as off (web uses ~0.01).
pub const BRAIN_PULSE_FLOOR: f32 = 0.01;
/// Nominal milliseconds per web RAF frame used when mapping pump cadence.
pub const BRAIN_PULSE_RAF_MS: u64 = 16;

/// How many web-style `*= 0.94` steps fit in `elapsed_ms` (at least 1 when >0).
pub fn brain_pulse_decay_steps_for_ms(elapsed_ms: u64) -> u32 {
    if elapsed_ms == 0 {
        return 0;
    }
    ((elapsed_ms + BRAIN_PULSE_RAF_MS / 2) / BRAIN_PULSE_RAF_MS).max(1) as u32
}

/// Apply `steps` web RAF pulse decays (`*= 0.94` each); zero below floor.
pub fn brain_pulse_after_decay(pulse: f32, steps: u32) -> f32 {
    if pulse <= BRAIN_PULSE_FLOOR {
        return 0.0;
    }
    if steps == 0 {
        return pulse;
    }
    let p = pulse * BRAIN_PULSE_RAF_FACTOR.powi(steps as i32);
    if p <= BRAIN_PULSE_FLOOR { 0.0 } else { p }
}

/// Carry multi-frame pulse decay across visual pump ticks.
///
/// Fresh hits (`new.pulse >= 0.99`) stay lit; other cells decay the previous
/// view's pulse over `decay_steps` RAF-equivalent frames. Dimension mismatch
/// drops previous pulses (map resample / resize).
pub fn apply_brain_pulse_decay(view: &mut BrainView, prev: &BrainView, decay_steps: u32) {
    if view.disp_rows != prev.disp_rows
        || view.disp_cols != prev.disp_cols
        || view.cells.len() != prev.cells.len()
    {
        return;
    }
    for (cell, prev_cell) in view.cells.iter_mut().zip(prev.cells.iter()) {
        if cell.2 >= 0.99 {
            // Fresh hit from this seq change.
            continue;
        }
        cell.2 = brain_pulse_after_decay(prev_cell.2, decay_steps);
    }
}

/// Status line after a generate finishes (Done frame).
pub fn status_after_gen_done(
    stop_requested: bool,
    completion_tokens: u64,
    tokens_per_second: f32,
) -> String {
    if stop_requested {
        "stopped".into()
    } else {
        format!("done · {completion_tokens} tok · {tokens_per_second:.2} tok/s")
    }
}

// ---------------------------------------------------------------------------
// Generate / stop session
// ---------------------------------------------------------------------------

/// Events from a background generate job back to the UI thread.
#[derive(Debug, Clone)]
pub enum GenEvent {
    Token(String),
    Done {
        completion_tokens: u64,
        tokens_per_second: f32,
    },
    Error(String),
}

/// Events from a background install job.
#[cfg(feature = "install")]
#[derive(Debug, Clone)]
pub enum InstallEvent {
    Progress(InstallProgress),
    Done(InstallResult),
    /// Cooperative pause finished (current file done; not an error).
    Paused,
    Error(String),
}

// ---------------------------------------------------------------------------
// Model registry scan (picker)
// ---------------------------------------------------------------------------

/// Default roots to scan: model store path (and optional extra).
pub fn registry_scan_roots(
    model_store: Option<&Path>,
    extra: impl IntoIterator<Item = PathBuf>,
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let store = model_store
        .map(Path::to_path_buf)
        .unwrap_or_else(default_model_store_path);
    roots.push(store);
    for p in extra {
        if !roots.iter().any(|r| r == &p) {
            roots.push(p);
        }
    }
    roots
}

/// Scan registry roots and return inventory entries (sorted by path).
pub fn scan_model_registry(roots: &[PathBuf]) -> Result<Vec<ModelEntry>, String> {
    let mut reg = ModelRegistry::open(roots.iter().cloned());
    reg.refresh().map_err(|e| e.to_string())?;
    Ok(reg.entries().to_vec())
}

/// One-line label for a registry row (status · family · size · path).
pub fn format_registry_entry(entry: &ModelEntry) -> String {
    let status = match entry.status {
        ModelStatus::Present => "ok",
        ModelStatus::Incomplete => "incomplete",
        ModelStatus::MissingTokenizer => "no tokenizer",
        ModelStatus::MissingConfig => "no config",
        ModelStatus::Unreadable => "unreadable",
    };
    let name = entry
        .path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| entry.path.display().to_string());
    let size_s = if entry.model_bytes > 0 {
        format!("{:.1} GB", entry.model_bytes as f64 / GB as f64)
    } else {
        "-".into()
    };
    format!(
        "[{status}] {} · {} · {size_s} · {}",
        entry.family.as_str(),
        name,
        entry.path.display()
    )
}

/// Shared request bookkeeping so Stop and generate share the same mux id.
#[derive(Debug)]
struct ReqBook {
    next_req: u64,
    active_req: Option<u64>,
}

impl Default for ReqBook {
    fn default() -> Self {
        Self {
            next_req: 1,
            active_req: None,
        }
    }
}

impl ReqBook {
    /// Allocate the next req id and mark it active. Errors if already generating.
    fn begin(&mut self) -> Result<u64, String> {
        if self.active_req.is_some() {
            return Err("already generating".into());
        }
        let id = self.next_req;
        self.next_req = self.next_req.saturating_add(1);
        self.active_req = Some(id);
        Ok(id)
    }

    /// Clear `active_req` only when it still matches `id`.
    fn clear_matching(&mut self, id: u64) {
        if self.active_req == Some(id) {
            self.active_req = None;
        }
    }
}

// ---------------------------------------------------------------------------
// Inference controls (temperature, max tokens, reasoning, session slot, grammar)
// ---------------------------------------------------------------------------

/// Web-aligned sampling / submit bounds (see web sidebar).
pub const TEMPERATURE_MIN: f32 = 0.0;
pub const TEMPERATURE_MAX: f32 = 2.0;
pub const MAX_TOKENS_MIN: u32 = 1;
pub const MAX_TOKENS_MAX: u32 = 32768;
pub const KV_SLOTS_MIN: u32 = 1;
pub const KV_SLOTS_MAX: u32 = 16;
/// Default top-p when the UI does not expose a control.
pub const DEFAULT_TOP_P: f32 = 0.95;

/// Per-request inference knobs for [`EngineSession::generate_async`].
#[derive(Debug, Clone, PartialEq)]
pub struct GenerateControls {
    pub temperature: f32,
    pub max_tokens: u32,
    /// Open thinking / reasoning prefix in the chat template.
    pub enable_thinking: bool,
    /// Mux KV cache slot (`cache_slot` / ClientFrame `slot`).
    pub cache_slot: u32,
    /// Optional GBNF grammar for structured output.
    pub grammar: Option<String>,
    pub top_p: f32,
}

impl Default for GenerateControls {
    fn default() -> Self {
        Self {
            temperature: 0.7,
            max_tokens: 4096,
            enable_thinking: false,
            cache_slot: 0,
            grammar: None,
            top_p: DEFAULT_TOP_P,
        }
    }
}

impl GenerateControls {
    /// Clamp numeric fields into product bounds. Slot is clamped with
    /// [`clamp_cache_slot`] when `kv_slots` is known.
    pub fn clamped(mut self, kv_slots: u32) -> Self {
        self.temperature = clamp_temperature(self.temperature);
        self.max_tokens = clamp_max_tokens(self.max_tokens);
        self.cache_slot = clamp_cache_slot(self.cache_slot, kv_slots);
        self.top_p = self.top_p.clamp(0.0, 1.0);
        if let Some(ref g) = self.grammar {
            let t = g.trim();
            if t.is_empty() {
                self.grammar = None;
            } else {
                self.grammar = Some(t.to_string());
            }
        }
        self
    }
}

pub fn clamp_temperature(t: f32) -> f32 {
    if !t.is_finite() {
        return GenerateControls::default().temperature;
    }
    t.clamp(TEMPERATURE_MIN, TEMPERATURE_MAX)
}

pub fn clamp_max_tokens(n: u32) -> u32 {
    n.clamp(MAX_TOKENS_MIN, MAX_TOKENS_MAX)
}

/// Clamp slot into `0 .. kv_slots` (exclusive end). `kv_slots` of 0 is treated as 1.
pub fn clamp_cache_slot(slot: u32, kv_slots: u32) -> u32 {
    let n = kv_slots.max(1);
    slot.min(n.saturating_sub(1))
}

pub fn clamp_kv_slots(n: u32) -> u32 {
    n.clamp(KV_SLOTS_MIN, KV_SLOTS_MAX)
}

/// Parse temperature text from the UI (empty → default).
pub fn parse_temperature(text: &str) -> Result<f32, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(GenerateControls::default().temperature);
    }
    let v: f32 = t.parse().map_err(|_| {
        format!("temperature must be a number between {TEMPERATURE_MIN} and {TEMPERATURE_MAX}")
    })?;
    if !v.is_finite() {
        return Err("temperature must be finite".into());
    }
    Ok(clamp_temperature(v))
}

/// Parse max-tokens text from the UI (empty → default).
pub fn parse_max_tokens(text: &str) -> Result<u32, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(GenerateControls::default().max_tokens);
    }
    let v: u32 = t.parse().map_err(|_| {
        format!("max tokens must be an integer between {MAX_TOKENS_MIN} and {MAX_TOKENS_MAX}")
    })?;
    Ok(clamp_max_tokens(v))
}

/// Optional GBNF field: empty → None.
pub fn parse_grammar_field(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// Resolve engine KV slots from env (`COLIBRI_KV_SLOTS` / `KV_SLOTS`) or default 1.
pub fn env_kv_slots() -> u32 {
    std::env::var("COLIBRI_KV_SLOTS")
        .or_else(|_| std::env::var("KV_SLOTS"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .map(clamp_kv_slots)
        .unwrap_or(1)
}

/// Sticky multi-slot chat: switch active slot, stashing and restoring transcripts.
///
/// `by_slot` maps slot index → turns `(role, text)`. Returns the transcript for
/// `new_slot` (empty vec if first visit).
pub fn switch_cache_slot_transcript(
    by_slot: &mut std::collections::HashMap<u32, Vec<(String, String)>>,
    active_slot: u32,
    new_slot: u32,
    current_log: Vec<(String, String)>,
) -> (u32, Vec<(String, String)>) {
    by_slot.insert(active_slot, current_log);
    let next = by_slot.remove(&new_slot).unwrap_or_default();
    (new_slot, next)
}

/// Build [`GenerateControls`] from UI strings + flags (clamped for `kv_slots`).
pub fn controls_from_ui(
    temperature_text: &str,
    max_tokens_text: &str,
    enable_thinking: bool,
    cache_slot: u32,
    grammar_text: &str,
    kv_slots: u32,
) -> Result<GenerateControls, String> {
    let temperature = parse_temperature(temperature_text)?;
    let max_tokens = parse_max_tokens(max_tokens_text)?;
    let grammar = parse_grammar_field(grammar_text);
    Ok(GenerateControls {
        temperature,
        max_tokens,
        enable_thinking,
        cache_slot,
        grammar,
        top_p: DEFAULT_TOP_P,
    }
    .clamped(kv_slots))
}

// ---------------------------------------------------------------------------
// Engine path routing (native host: FFI-first when feature=ffi; else process)
// ---------------------------------------------------------------------------

/// Env key: truthy value prefers in-process FFI when linked.
///
/// Under Cargo `feature = "ffi"`, the native host already defaults to try FFI
/// first, so this env is largely redundant (kept for explicit opt-in and for
/// builds without `feature = "ffi"`, where it still sets `prefer_process =
/// false` even though open cannot link). `COLIBRI_FORCE_PROCESS` always wins.
pub const PREFER_FFI_ENV: &str = "COLIBRI_PREFER_FFI";

/// Parse a truthy env flag (same matrix as `COLIBRI_FORCE_PROCESS`).
///
/// Unset, empty, `0`, `false`, `no`, `off` (case-insensitive) → false.
/// Any other non-empty value → true.
pub fn env_flag_truthy(value: Option<impl AsRef<std::ffi::OsStr>>) -> bool {
    colibri_sys::env_force_process(value)
}

/// True when `COLIBRI_PREFER_FFI` is set truthy in the process environment.
pub fn env_prefer_ffi() -> bool {
    env_flag_truthy(std::env::var_os(PREFER_FFI_ENV))
}

/// True when `COLIBRI_FORCE_PROCESS` is set truthy (always forces process path).
pub fn env_force_process_path() -> bool {
    force_process_from_env()
}

/// Resolve `ColibriConfig.prefer_process` for desktop start from pure flags.
///
/// Order (host-side only; kill-switch still applied in `must_use_process`):
/// 1. `force_process` truthy → `true` (process; always wins)
/// 2. With Cargo `feature = "ffi"` → `false` (try FFI first; process fallback
///    on open failure still applies in [`EngineSession::start`])
/// 3. Without `feature = "ffi"`: `prefer_ffi` truthy → `false`, else `true`
///    (process-only builds have no static link; process remains the path)
///
/// Crate-wide `ColibriConfig` default stays `prefer_process = true` for library
/// embeds; only this native host resolution flips under `feature = "ffi"`.
pub fn resolve_prefer_process_from_flags(force_process: bool, prefer_ffi: bool) -> bool {
    if force_process {
        return true;
    }
    #[cfg(feature = "ffi")]
    {
        // Native product default under feature=ffi: try in-process first.
        // prefer_ffi is accepted but redundant (default already FFI-first).
        let _ = prefer_ffi;
        false
    }
    #[cfg(not(feature = "ffi"))]
    {
        if prefer_ffi {
            return false;
        }
        true
    }
}

/// Resolve `ColibriConfig.prefer_process` for desktop start from process env.
pub fn resolve_prefer_process() -> bool {
    resolve_prefer_process_from_flags(env_force_process_path(), env_prefer_ffi())
}

/// Whether start should try in-process FFI open for this family + config.
///
/// Requires: not forced to process, config prefers FFI path, Cargo `ffi`
/// linked, and [`FfiFamily::from_model_family`] maps the model family.
pub fn should_try_ffi_open(cfg: &ColibriConfig, family: ModelFamily) -> bool {
    if !cfg.prefer_ffi_path() {
        return false;
    }
    #[cfg(feature = "ffi")]
    {
        match FfiFamily::from_model_family(family) {
            Some(f) => coli_ffi::ffi_family_available(f),
            None => false,
        }
    }
    #[cfg(not(feature = "ffi"))]
    {
        let _ = family;
        false
    }
}

/// Plain-English backend label for status / engine chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnginePathKind {
    Process,
    /// Present in the enum always so status helpers stay stable without `ffi`.
    Ffi,
}

impl EnginePathKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Process => "engine process",
            Self::Ffi => "in-process FFI",
        }
    }
}

/// Status line for a path kind (Brain note on pure FFI).
pub fn engine_path_status_line(kind: EnginePathKind) -> &'static str {
    match kind {
        EnginePathKind::Process => EnginePathKind::Process.as_str(),
        EnginePathKind::Ffi => "in-process FFI · live visual poll",
    }
}

/// Send-safe box around [`FfiEngine`] (raw C pointers; single-owner via Mutex).
#[cfg(feature = "ffi")]
struct FfiEngineSend(FfiEngine);

// SAFETY: Host serializes all access through `Mutex<FfiEngineSend>` and the
// generate request book (one active generate). C engines are not re-entrant
// per handle; we never share the raw pointer across concurrent generates.
#[cfg(feature = "ffi")]
unsafe impl Send for FfiEngineSend {}

/// Live backend for a started session.
enum LiveEngine {
    Process(EngineHandle),
    #[cfg(feature = "ffi")]
    Ffi {
        /// Shared so generate can hold only this lock (not the session mutex).
        engine: Arc<Mutex<FfiEngineSend>>,
        model_path: PathBuf,
    },
}

/// Live engine session (process default; optional in-process FFI).
///
/// The session stays in the UI slot for the whole process lifetime. Process
/// generate clones [`EngineHandle`] (cheap Arc) so Stop and visual pump can run
/// concurrently. FFI generate holds the engine mutex for the call; visual pump
/// uses [`pump_visual_try_lock`] and keeps the last snapshot when generate
/// holds that mutex (never block the UI thread). GLM fills Brain/PROF; other
/// families may still return empty until fill lands.
pub struct EngineSession {
    live: LiveEngine,
    family: ModelFamily,
    model_id: String,
    kv_slots: u32,
    req: Arc<Mutex<ReqBook>>,
    /// Cooperative cancel for in-process generate (mux STOP is process-only).
    #[cfg(feature = "ffi")]
    cancel: Arc<AtomicBool>,
    /// Short product line for the engine chip (path + size / fallback note).
    path_status: String,
    /// Last successful visual poll. Returned when generate holds the FFI mutex.
    last_visual: Mutex<VisualSnapshot>,
}

impl EngineSession {
    /// Start engine for `model` (optional plan apply). Blocking.
    ///
    /// With Cargo `feature = "ffi"` and no `COLIBRI_FORCE_PROCESS`, tries
    /// in-process open first for mappable families. On failure, falls back to
    /// the engine process with a clear status note. Without `feature = "ffi"`,
    /// process only.
    pub fn start(model: &Path) -> Result<Self, String> {
        let t0 = Instant::now();
        tracing::info!(
            target: "colibri_native",
            "{}",
            format_engine_start_log("begin", model, None, None, None)
        );
        let result = Self::start_blocking_inner(model);
        let ms = t0.elapsed().as_millis() as u64;
        match &result {
            Ok(session) => {
                let kind = if session.is_ffi() { "ffi" } else { "process" };
                tracing::info!(
                    target: "colibri_native",
                    "{}",
                    format_engine_start_log("end", model, Some(kind), Some(ms), None)
                );
            }
            Err(e) => {
                tracing::error!(
                    target: "colibri_native",
                    "{}",
                    format_engine_start_log("end", model, None, Some(ms), Some(e))
                );
            }
        }
        result
    }

    fn start_blocking_inner(model: &Path) -> Result<Self, String> {
        // Fail fast: empty store / non-leaf must not spawn serve (avoids EOF before READY).
        let model = preflight_then_maybe_open(model, None, ram_overcommit_from_env())?;
        apply_plan_env_for_ffi(&model);
        let family = model_arch(&model);
        let kv_slots = env_kv_slots();
        let mut cfg = ColibriConfig::default()
            .model(model.clone())
            .max_tokens(256)
            .kv_slots(kv_slots)
            .prefer_process(resolve_prefer_process());
        if let Some(engine) = env_engine_path() {
            cfg = cfg.engine(engine);
        }

        let model_id = model
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("local-model")
            .to_string();

        let try_ffi = should_try_ffi_open(&cfg, family);
        #[allow(unused_mut)] // mutated only when feature = "ffi"
        let mut fallback_note: Option<String> = None;

        #[cfg(feature = "ffi")]
        if try_ffi {
            if let Some(ffi_fam) = FfiFamily::from_model_family(family) {
                record_ffi_open_attempt();
                match coli_ffi::open_engine(ffi_fam, &model) {
                    Ok(eng) => {
                        let size = eng.size_info();
                        let disk_gb = size.disk_bytes as f64 / GB as f64;
                        let path_status = format!(
                            "in-process FFI ({}) · {:.1} GB on disk · live visual poll",
                            ffi_fam.as_str(),
                            disk_gb
                        );
                        return Ok(Self {
                            live: LiveEngine::Ffi {
                                engine: Arc::new(Mutex::new(FfiEngineSend(eng))),
                                model_path: model,
                            },
                            family,
                            model_id,
                            kv_slots: 1,
                            req: Arc::new(Mutex::new(ReqBook::default())),
                            cancel: Arc::new(AtomicBool::new(false)),
                            path_status,
                            last_visual: Mutex::new(VisualSnapshot::default()),
                        });
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if !ffi_open_error_should_fallback(&msg) {
                            return Err(if msg.to_ascii_lowercase().contains("not enough ram") {
                                ENGINE_START_RAM_TOO_SMALL.to_string()
                            } else if msg.to_ascii_lowercase().contains("could not measure ram") {
                                ENGINE_START_RAM_UNMEASURABLE.to_string()
                            } else {
                                msg
                            });
                        }
                        fallback_note = Some(format!(
                            "In-process open failed ({e}); using engine process"
                        ));
                        tracing::warn!(
                            target: "colibri_native",
                            error = %e,
                            "in-process open failed; falling back to engine process"
                        );
                    }
                }
            }
        }
        #[cfg(not(feature = "ffi"))]
        {
            let _ = try_ffi;
        }

        Self::start_process(&model, family, model_id, cfg, fallback_note)
            .map_err(|e| map_engine_start_error(&e, &model))
    }

    /// Start on a worker thread. The caller must not invoke [`Self::start`]
    /// on the GPUI UI thread.
    pub fn start_async(
        model: PathBuf,
        drop_previous: Option<Arc<Mutex<Option<EngineSession>>>>,
        tx: mpsc::Sender<Result<Self, String>>,
    ) {
        spawn_engine_start(model, drop_previous, tx);
    }

    fn start_process(
        model: &Path,
        family: ModelFamily,
        model_id: String,
        cfg: ColibriConfig,
        fallback_note: Option<String>,
    ) -> Result<Self, String> {
        let plan = PlacementPlan::build(
            model,
            &PlanOptions {
                context: cfg.ctx,
                ..Default::default()
            },
        );

        let handle = match plan {
            Ok(ref p) => EngineHandle::start_with_plan(cfg, p),
            Err(e) => {
                tracing_warn_plan(&e.to_string());
                EngineHandle::start_blocking(cfg)
            }
        }
        .map_err(|e| format!("engine start failed: {e}"))?;

        let kv_slots = handle.config().kv_slots.max(1);
        // Subscribe interest for visual frames when duplex is used.
        let duplex = EngineDuplex::new(handle.clone(), model_id.clone());
        let _hello = duplex.hello();
        let mut duplex = duplex;
        let _ = duplex.handle(&ClientFrame::Subscribe {
            mask: colibri_sys::Subscribe::ALL.0,
        });

        let path_status = match fallback_note {
            Some(note) => note,
            None => "engine process".into(),
        };

        Ok(Self {
            live: LiveEngine::Process(handle),
            family,
            model_id,
            kv_slots,
            req: Arc::new(Mutex::new(ReqBook::default())),
            #[cfg(feature = "ffi")]
            cancel: Arc::new(AtomicBool::new(false)),
            path_status,
            last_visual: Mutex::new(VisualSnapshot::default()),
        })
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn family(&self) -> ModelFamily {
        self.family
    }

    /// Backend kind for status text.
    pub fn path_kind(&self) -> EnginePathKind {
        match &self.live {
            LiveEngine::Process(_) => EnginePathKind::Process,
            #[cfg(feature = "ffi")]
            LiveEngine::Ffi { .. } => EnginePathKind::Ffi,
        }
    }

    /// Short product line for the engine chip (includes fallback notes).
    pub fn path_status(&self) -> &str {
        // Keep env key names and path labels referenced from product code paths.
        let _ = (FORCE_PROCESS_ENV, PREFER_FFI_ENV);
        let _ = (
            EnginePathKind::Process.as_str(),
            EnginePathKind::Ffi.as_str(),
            engine_path_status_line(EnginePathKind::Ffi),
        );
        &self.path_status
    }

    /// True when this session is in-process FFI (embed poll, not serve mux).
    pub fn is_ffi(&self) -> bool {
        matches!(self.path_kind(), EnginePathKind::Ffi)
    }

    /// Mux KV slots advertised at engine start (`KV_SLOTS`, 1–16). FFI is 1.
    pub fn kv_slots(&self) -> u32 {
        self.kv_slots
    }

    /// Pump telemetry into the handle snapshot and return a clone.
    ///
    /// Process: serve mux absorb. FFI: `FfiEngine::pump_visual` → `coli_*_visual_poll`
    /// via [`pump_visual_try_lock`] (never waits on the generate mutex).
    pub fn pump_visual(&self) -> VisualSnapshot {
        let snap = match &self.live {
            LiveEngine::Process(engine) => {
                engine.pump_visual();
                engine.visual_snapshot()
            }
            #[cfg(feature = "ffi")]
            LiveEngine::Ffi { engine, .. } => {
                let last = self
                    .last_visual
                    .lock()
                    .map(|g| g.clone())
                    .unwrap_or_default();
                pump_visual_try_lock(engine, last, |guard| {
                    guard
                        .0
                        .pump_visual()
                        .unwrap_or_else(|_| guard.0.visual_snapshot())
                })
            }
        };
        if let Ok(mut cache) = self.last_visual.lock() {
            *cache = snap.clone();
        }
        snap
    }

    /// Send STOP for the in-flight generate (if any).
    ///
    /// Process: mux `STOP`. FFI: cooperative cancel flag checked in the token
    /// callback (may finish the current step before stopping).
    pub fn stop_active(&self) -> Result<u64, String> {
        let req_id = self
            .req
            .lock()
            .map_err(|_| "request book poisoned".to_string())?
            .active_req
            .ok_or_else(|| "nothing generating".to_string())?;
        match &self.live {
            LiveEngine::Process(engine) => {
                engine
                    .with_client(|c| c.stop_request(req_id))
                    .map_err(|e| format!("stop: {e}"))?;
            }
            #[cfg(feature = "ffi")]
            LiveEngine::Ffi { .. } => {
                request_ffi_generate_cancel(&self.cancel);
            }
        }
        Ok(req_id)
    }

    /// Render chat + submit on a background thread.
    ///
    /// Process path: [`EngineDuplex`] on a cloned handle. FFI path: in-process
    /// generate under the engine mutex; on failure, falls back to starting a
    /// process engine for this model and re-running the generate once.
    pub fn generate_async(
        slot: Arc<Mutex<Option<EngineSession>>>,
        messages: Vec<ChatMessage>,
        controls: GenerateControls,
        tx: mpsc::Sender<GenEvent>,
    ) {
        thread::spawn(move || {
            // This worker (process mux client or FFI generate) is not GPUI.
            let _ = set_current_thread_nice(ENGINE_CHILD_NICE);
            let clear_active = |book: &Arc<Mutex<ReqBook>>, id: u64| {
                if let Ok(mut b) = book.lock() {
                    b.clear_matching(id);
                }
            };

            // Snapshot routing decision + process clone under the session lock.
            enum GenRoute {
                Process {
                    engine: EngineHandle,
                    family: ModelFamily,
                    model_id: String,
                    req_book: Arc<Mutex<ReqBook>>,
                    req_id: u64,
                    kv_slots: u32,
                },
                #[cfg(feature = "ffi")]
                Ffi {
                    family: ModelFamily,
                    model_id: String,
                    model_path: PathBuf,
                    engine: Arc<Mutex<FfiEngineSend>>,
                    req_book: Arc<Mutex<ReqBook>>,
                    req_id: u64,
                    kv_slots: u32,
                    cancel: Arc<AtomicBool>,
                },
            }

            let route = {
                let g = match slot.lock() {
                    Ok(g) => g,
                    Err(_) => {
                        let _ = tx.send(GenEvent::Error("engine lock poisoned".into()));
                        return;
                    }
                };
                let Some(session) = g.as_ref() else {
                    let _ = tx.send(GenEvent::Error(
                        "no engine session (set model path and Start engine)".into(),
                    ));
                    return;
                };
                let mut book = match session.req.lock() {
                    Ok(b) => b,
                    Err(_) => {
                        let _ = tx.send(GenEvent::Error("request book poisoned".into()));
                        return;
                    }
                };
                let req_id = match book.begin() {
                    Ok(id) => id,
                    Err(e) => {
                        let _ = tx.send(GenEvent::Error(e));
                        return;
                    }
                };
                match &session.live {
                    LiveEngine::Process(engine) => GenRoute::Process {
                        engine: engine.clone(),
                        family: session.family,
                        model_id: session.model_id.clone(),
                        req_book: session.req.clone(),
                        req_id,
                        kv_slots: session.kv_slots,
                    },
                    #[cfg(feature = "ffi")]
                    LiveEngine::Ffi {
                        model_path, engine, ..
                    } => GenRoute::Ffi {
                        family: session.family,
                        model_id: session.model_id.clone(),
                        model_path: model_path.clone(),
                        engine: Arc::clone(engine),
                        req_book: session.req.clone(),
                        req_id,
                        kv_slots: session.kv_slots,
                        cancel: session.cancel.clone(),
                    },
                }
            };

            let kind = match &route {
                GenRoute::Process { .. } => "process",
                #[cfg(feature = "ffi")]
                GenRoute::Ffi { .. } => "ffi",
            };
            let req_id = match &route {
                GenRoute::Process { req_id, .. } => *req_id,
                #[cfg(feature = "ffi")]
                GenRoute::Ffi { req_id, .. } => *req_id,
            };
            let mut gen_trace = GenerateTrace::begin(kind, req_id);

            match route {
                GenRoute::Process {
                    engine,
                    family,
                    model_id,
                    req_book,
                    req_id,
                    kv_slots,
                } => {
                    let controls = controls.clamped(kv_slots);
                    let render_opts = ChatRenderOptions {
                        enable_thinking: controls.enable_thinking,
                        reasoning_effort: None,
                    };
                    let prompt = match render_chat(&messages, family, &render_opts) {
                        Ok(p) => p,
                        Err(e) => {
                            clear_active(&req_book, req_id);
                            let msg = format!("chat template: {e}");
                            gen_trace.fail(&msg);
                            let _ = tx.send(GenEvent::Error(msg));
                            return;
                        }
                    };
                    generate_process(engine, model_id, req_id, prompt, &controls, &tx);
                    clear_active(&req_book, req_id);
                }
                #[cfg(feature = "ffi")]
                GenRoute::Ffi {
                    family,
                    model_id,
                    model_path,
                    engine: ffi_engine,
                    req_book,
                    req_id,
                    kv_slots,
                    cancel,
                } => {
                    let controls = controls.clamped(kv_slots);
                    let render_opts = ChatRenderOptions {
                        enable_thinking: controls.enable_thinking,
                        reasoning_effort: None,
                    };
                    let prompt = match render_chat(&messages, family, &render_opts) {
                        Ok(p) => p,
                        Err(e) => {
                            clear_active(&req_book, req_id);
                            let msg = format!("chat template: {e}");
                            gen_trace.fail(&msg);
                            let _ = tx.send(GenEvent::Error(msg));
                            return;
                        }
                    };

                    cancel.store(false, Ordering::SeqCst);
                    let ffi_result = {
                        let mut guard = match ffi_engine.lock() {
                            Ok(g) => g,
                            Err(_) => {
                                clear_active(&req_book, req_id);
                                let msg = "FFI engine lock poisoned".to_string();
                                gen_trace.fail(&msg);
                                let _ = tx.send(GenEvent::Error(msg));
                                return;
                            }
                        };
                        generate_ffi(&mut guard.0, &prompt, &controls, &cancel, &tx)
                    };

                    match ffi_result {
                        Ok(()) => {
                            clear_active(&req_book, req_id);
                        }
                        Err(e) => {
                            if !ffi_generate_error_should_fallback(&e) {
                                clear_active(&req_book, req_id);
                                let _ = tx.send(GenEvent::Done {
                                    completion_tokens: 0,
                                    tokens_per_second: 0.0,
                                });
                                return;
                            }
                            // Fall back: start process for this model and re-run generate.
                            tracing::warn!(
                                target: "colibri_native",
                                error = %e,
                                "in-process generate failed; falling back to engine process"
                            );
                            let cfg = ColibriConfig::default()
                                .model(&model_path)
                                .max_tokens(256)
                                .kv_slots(env_kv_slots())
                                .prefer_process(true);
                            let cfg = if let Some(eng_path) = env_engine_path() {
                                cfg.engine(eng_path)
                            } else {
                                cfg
                            };
                            match Self::start_process(
                                &model_path,
                                family,
                                model_id.clone(),
                                cfg,
                                Some(format!(
                                    "In-process generate failed ({e}); using engine process"
                                )),
                            ) {
                                Ok(new_session) => {
                                    let process_handle = match &new_session.live {
                                        LiveEngine::Process(h) => h.clone(),
                                        LiveEngine::Ffi { .. } => {
                                            clear_active(&req_book, req_id);
                                            let _ = tx.send(GenEvent::Error(
                                                "fallback start did not produce process engine"
                                                    .into(),
                                            ));
                                            return;
                                        }
                                    };
                                    if let Ok(mut g) = slot.lock() {
                                        *g = Some(new_session);
                                    }
                                    generate_process(
                                        process_handle,
                                        model_id,
                                        req_id,
                                        prompt,
                                        &controls,
                                        &tx,
                                    );
                                    clear_active(&req_book, req_id);
                                }
                                Err(start_err) => {
                                    clear_active(&req_book, req_id);
                                    let plain = map_engine_start_error(&start_err, &model_path);
                                    let msg = format!(
                                        "in-process generate failed ({e}); process fallback also failed: {plain}"
                                    );
                                    gen_trace.fail(&msg);
                                    let _ = tx.send(GenEvent::Error(msg));
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}

/// Process-path generate via [`EngineDuplex`].
fn generate_process(
    engine: EngineHandle,
    model_id: String,
    req_id: u64,
    prompt: String,
    controls: &GenerateControls,
    tx: &mpsc::Sender<GenEvent>,
) {
    let frame = ClientFrame::Submit {
        req_id,
        slot: controls.cache_slot,
        max_tokens: controls.max_tokens,
        temperature: controls.temperature,
        top_p: controls.top_p,
        prompt,
        grammar: controls.grammar.clone(),
    };

    let mut duplex = EngineDuplex::new(engine, model_id);
    let mut saw_terminal = false;
    let result = duplex.handle_with(&frame, |sf| {
        match sf {
            ServerFrame::Token { utf8, .. } => {
                let s = String::from_utf8_lossy(&utf8).into_owned();
                if !s.is_empty() {
                    let _ = tx.send(GenEvent::Token(s));
                }
            }
            ServerFrame::Done {
                completion_tokens,
                tokens_per_second,
                ..
            } => {
                saw_terminal = true;
                let _ = tx.send(GenEvent::Done {
                    completion_tokens,
                    tokens_per_second,
                });
            }
            ServerFrame::Error { message, .. } => {
                saw_terminal = true;
                let _ = tx.send(GenEvent::Error(message));
            }
            _ => {}
        }
        Ok(())
    });
    if let Err(e) = result {
        if !saw_terminal {
            let _ = tx.send(GenEvent::Error(format!("generate: {e}")));
        }
    }
}

/// In-process generate (feature `ffi`). Streams token-count progress; V4 also
/// appends detokenized text when the session API exposes it.
#[cfg(feature = "ffi")]
fn generate_ffi(
    engine: &mut FfiEngine,
    prompt: &str,
    controls: &GenerateControls,
    cancel: &AtomicBool,
    tx: &mpsc::Sender<GenEvent>,
) -> Result<(), String> {
    let max_new = controls.max_tokens.min(i32::MAX as u32) as i32;
    let opts = FfiGenerateOptions {
        max_new_tokens: max_new.max(1),
    };
    coli_ffi::clear_embed_stop();
    let start = Instant::now();
    let mut count: u64 = 0;

    // DeepSeek V4: session generate + detokenized buffer when available.
    if let FfiEngine::DeepseekV4(v4) = engine {
        let mut session = v4
            .create_session(coli_ffi::V4SessionCreateOptions::default())
            .map_err(|e| e.to_string())?;
        let stats = session
            .generate(
                prompt,
                coli_ffi::V4GenerateOptions {
                    max_new_tokens: opts.max_new_tokens,
                    stop_at_sentence: false,
                    no_dspark: false,
                },
                |ev| {
                    if cancel.load(Ordering::SeqCst) {
                        return Err(colibri_sys::Error::engine("stopped"));
                    }
                    count = count.saturating_add(1);
                    let _ = ev;
                    Ok(())
                },
            )
            .map_err(|e| e.to_string())?;
        match session.generated_text() {
            Ok(text) if !text.is_empty() => {
                let _ = tx.send(GenEvent::Token(text));
            }
            _ => {
                if count > 0 {
                    let _ = tx.send(GenEvent::Token(format!("[in-process · {count} tokens]")));
                }
            }
        }
        let completion = if stats.generated_tokens > 0 {
            stats.generated_tokens as u64
        } else {
            count
        };
        let wall = start.elapsed().as_secs_f32().max(1e-6);
        let tps = completion as f32 / wall;
        let _ = tx.send(GenEvent::Done {
            completion_tokens: completion,
            tokens_per_second: tps,
        });
        return Ok(());
    }

    engine
        .generate(prompt, opts, |ev| {
            if cancel.load(Ordering::SeqCst) {
                return Err(colibri_sys::Error::engine("stopped"));
            }
            count = count.saturating_add(1);
            let _ = ev;
            // Token ids only on multi-family CPU API; pulse UI with a soft mark.
            if count == 1 || count % 8 == 0 {
                let _ = tx.send(GenEvent::Token("·".into()));
            }
            Ok(())
        })
        .map_err(|e| e.to_string())?;

    if count > 0 {
        let _ = tx.send(GenEvent::Token(format!(
            "\n[in-process · {count} tokens · full chat text needs engine process for detokenize stream]"
        )));
    }
    let wall = start.elapsed().as_secs_f32().max(1e-6);
    let tps = count as f32 / wall;
    let _ = tx.send(GenEvent::Done {
        completion_tokens: count,
        tokens_per_second: tps,
    });
    Ok(())
}

/// Pump FFI visual without waiting on generate.
///
/// When the generate worker holds the engine mutex, return `last` immediately.
/// `coli_*_visual_poll` is not safe on the same Model* as generate; try_lock
/// is the serializer. Never call this helper's poll while generate holds the
/// same Model*.
pub fn pump_visual_try_lock<T>(
    engine: &Mutex<T>,
    last: VisualSnapshot,
    poll: impl FnOnce(&mut T) -> VisualSnapshot,
) -> VisualSnapshot {
    match engine.try_lock() {
        Ok(mut guard) => poll(&mut guard),
        Err(std::sync::TryLockError::WouldBlock) => last,
        Err(std::sync::TryLockError::Poisoned(_)) => last,
    }
}

/// Pump visual from a live session slot (if any).
///
/// Uses try_lock so a 500ms UI pump never waits on generate or a busy session.
/// On miss, the caller keeps the last painted snapshot.
pub fn pump_session_visual(slot: &Arc<Mutex<Option<EngineSession>>>) -> Option<VisualSnapshot> {
    let g = match slot.try_lock() {
        Ok(g) => g,
        Err(std::sync::TryLockError::WouldBlock) => return None,
        Err(std::sync::TryLockError::Poisoned(_)) => return None,
    };
    let session = g.as_ref()?;
    Some(session.pump_visual())
}

/// Set the cooperative FFI cancel flag. Does not take the engine mutex.
#[cfg(feature = "ffi")]
pub fn request_ffi_generate_cancel(cancel: &AtomicBool) {
    cancel.store(true, Ordering::SeqCst);
    coli_ffi::request_embed_stop();
}

/// Begin/end generate lines for native.log. No prompt text, no tokens.
struct GenerateTrace {
    kind: &'static str,
    req_id: u64,
    t0: Instant,
    error: Option<String>,
}

impl GenerateTrace {
    fn begin(kind: &'static str, req_id: u64) -> Self {
        tracing::info!(
            target: "colibri_native",
            "{}",
            format_generate_log("begin", Some(kind), Some(req_id), None, None)
        );
        Self {
            kind,
            req_id,
            t0: Instant::now(),
            error: None,
        }
    }

    fn fail(&mut self, e: impl Into<String>) {
        self.error = Some(e.into());
    }
}

impl Drop for GenerateTrace {
    fn drop(&mut self) {
        tracing::info!(
            target: "colibri_native",
            "{}",
            format_generate_log(
                "end",
                Some(self.kind),
                Some(self.req_id),
                Some(self.t0.elapsed().as_millis() as u64),
                self.error.as_deref(),
            )
        );
    }
}

/// Stop the in-flight generate if present.
pub fn stop_session(slot: &Arc<Mutex<Option<EngineSession>>>) -> Result<u64, String> {
    let g = slot
        .lock()
        .map_err(|_| "engine lock poisoned".to_string())?;
    let session = g.as_ref().ok_or_else(|| "no engine session".to_string())?;
    session.stop_active()
}

fn tracing_warn_plan(msg: &str) {
    tracing::warn!(
        target: "colibri_native",
        "plan warning (starting without plan): {msg}"
    );
}

/// Why a Start engine click must not open a model on this turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartEngineBlock {
    /// A generate is in flight.
    Generating,
    /// An open is already running off the UI thread.
    AlreadyStarting,
}

/// Source of a Start engine click (for logs only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartEngineSource {
    WizardReady,
    Rail,
    ChatSend,
}

impl StartEngineSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WizardReady => "wizard_ready",
            Self::Rail => "rail",
            Self::ChatSend => "chat_send",
        }
    }
}

/// Second Start / chat send must not begin another open on the UI thread.
pub fn should_dispatch_engine_start(
    generating: bool,
    starting: bool,
) -> Result<(), StartEngineBlock> {
    if generating {
        return Err(StartEngineBlock::Generating);
    }
    if starting {
        return Err(StartEngineBlock::AlreadyStarting);
    }
    Ok(())
}

/// Living status while mmap / READY wait runs off the UI thread.
pub fn engine_starting_status(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    if secs == 0 {
        "Starting engine…".into()
    } else {
        format!("Starting engine… still starting ({secs}s)")
    }
}

/// Run `work` on a background thread and send the result. Returns immediately.
pub fn dispatch_blocking_start<F, T>(work: F) -> mpsc::Receiver<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        // Engine start (mmap / FFI open / drop previous) is not GPUI.
        let _ = set_current_thread_nice(ENGINE_CHILD_NICE);
        let _ = tx.send(work());
    });
    rx
}

/// Spawn [`EngineSession::start`] off the caller thread.
///
/// When `drop_previous` is set, the previous session is dropped on this worker
/// (unmap / process teardown) so the UI thread does not block.
pub fn spawn_engine_start(
    model: PathBuf,
    drop_previous: Option<Arc<Mutex<Option<EngineSession>>>>,
    tx: mpsc::Sender<Result<EngineSession, String>>,
) {
    let _worker = dispatch_blocking_start(move || {
        if let Some(slot) = drop_previous
            && let Ok(mut g) = slot.lock()
        {
            *g = None;
        }
        let result = EngineSession::start(&model);
        let _ = tx.send(result);
    });
}

/// Build ChatMessage list from UI turns (user / assistant only for MVP).
pub fn messages_from_turns(turns: &[(String, String)]) -> Vec<ChatMessage> {
    let mut out = vec![ChatMessage::system(
        "You are a helpful assistant running inside colibrì native GPUI.",
    )];
    for (role, text) in turns {
        match role.as_str() {
            "user" => out.push(ChatMessage::user(text.clone())),
            "assistant" => out.push(ChatMessage::assistant(text.clone())),
            _ => {}
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Supported-model catalog (product list; not local disk registry)
// ---------------------------------------------------------------------------

/// Result of picking a row from the static supported-model catalog.
///
/// Maps onto the install form when `installable`; convert-only entries leave
/// form fields empty and set a plain operational status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogSelection {
    /// Catalog id (`glm-5.2-colibri`, …).
    pub id: String,
    /// Product display name.
    pub display_name: String,
    /// HF `owner/name` when installable.
    pub repo_id: Option<String>,
    /// Default dest folder name under the store (repo name segment).
    pub dest: Option<String>,
    /// Status line for the install / model step.
    pub status: String,
    /// True when [`Self::repo_id`] is set and Install can run.
    pub installable: bool,
}

/// Map a catalog entry onto install-form fields (pure; no I/O).
pub fn catalog_selection_from_model(model: &SupportedModel) -> CatalogSelection {
    match model.hf_repo {
        Some(repo) => {
            let dest = repo
                .rsplit_once('/')
                .map(|(_, name)| name.to_string())
                .unwrap_or_else(|| repo.to_string());
            CatalogSelection {
                id: model.id.to_string(),
                display_name: model.display_name.to_string(),
                repo_id: Some(repo.to_string()),
                dest: Some(dest),
                status: format!("Ready to install {}", model.display_name),
                installable: true,
            }
        }
        None => {
            let note = model
                .notes
                .unwrap_or("not available as a Hugging Face snapshot");
            CatalogSelection {
                id: model.id.to_string(),
                display_name: model.display_name.to_string(),
                repo_id: None,
                dest: None,
                status: format!("{} · {}", model.display_name, note),
                installable: false,
            }
        }
    }
}

/// Lookup by catalog id and map to install form fields.
pub fn catalog_selection_by_id(id: &str) -> Option<CatalogSelection> {
    supported_model_by_id(id).map(catalog_selection_from_model)
}

/// One-line label for a supported-model picker row.
pub fn format_supported_model_row(model: &SupportedModel) -> String {
    let mut s = model.display_name.to_string();
    if let Some(disk) = model.disk_hint {
        s.push_str(" · ");
        s.push_str(disk);
    }
    if model.hf_repo.is_none() {
        s.push_str(" · convert-only");
    } else if let Some(repo) = model.hf_repo {
        s.push_str(" · ");
        s.push_str(repo);
    }
    s
}

/// Re-export catalog list for hosts that import via `host`.
pub fn list_supported_models() -> &'static [SupportedModel] {
    supported_models()
}

/// True when `path` ends with `owner/name` path components (nested HF layout).
fn path_ends_with_owner_name(path: &Path, owner: &str, name: &str) -> bool {
    use std::path::Component;
    let mut comps = path.components().rev();
    match (comps.next(), comps.next()) {
        (Some(Component::Normal(leaf)), Some(Component::Normal(parent))) => {
            leaf == name && parent == owner
        }
        _ => false,
    }
}

/// Find a Present registry leaf that matches a supported catalog model.
///
/// Match order (first hit wins):
/// 1. folder name == HF repo leaf (`DeepSeek-V4-Flash-0731`)
/// 2. folder name == `owner__name` (empty-dest install layout)
/// 3. path ends with `owner/name` components (nested HF layout)
///
/// Convert-only catalog rows (`hf_repo` None) never match.
/// Incomplete / non-Present status does not count as installed.
pub fn catalog_is_installed<'a>(
    model: &SupportedModel,
    entries: &'a [ModelEntry],
) -> Option<&'a ModelEntry> {
    let repo = model.hf_repo?;
    let (owner, name) = repo.split_once('/')?;
    if owner.is_empty() || name.is_empty() {
        return None;
    }
    let owner_underscore_name = format!("{owner}__{name}");

    for entry in entries {
        if entry.status != ModelStatus::Present {
            continue;
        }
        let folder = entry
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        // 1. Catalog dest / repo leaf name.
        if folder == name {
            return Some(entry);
        }
        // 2. Empty-override install layout: owner__name.
        if folder == owner_underscore_name {
            return Some(entry);
        }
        // 3. Nested HF layout: …/owner/name.
        if path_ends_with_owner_name(&entry.path, owner, name) {
            return Some(entry);
        }
    }
    None
}

/// Paint roles for one supported-catalog row (pure; GPUI-free).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogRowStyle {
    pub fill: u32,
    pub fg: u32,
    pub border: u32,
    /// Show the operational "Installed" badge on the right.
    pub show_installed: bool,
}

/// Row colors for the supported-models catalog.
///
/// - Selected: primary fill/fg (selection wins over installed white).
/// - Installed, not selected: solid DOGE white fill + black text.
/// - Default: secondary / theme text.
pub fn catalog_row_style(
    installed: bool,
    selected: bool,
    primary: u32,
    primary_fg: u32,
    secondary: u32,
    text: u32,
    border: u32,
) -> CatalogRowStyle {
    if selected {
        CatalogRowStyle {
            fill: primary,
            fg: primary_fg,
            border: primary,
            show_installed: installed,
        }
    } else if installed {
        CatalogRowStyle {
            fill: crate::theme::DOGE_WHITE,
            fg: crate::theme::DOGE_BLACK,
            border: crate::theme::DOGE_WHITE,
            show_installed: true,
        }
    } else {
        CatalogRowStyle {
            fill: secondary,
            fg: text,
            border,
            show_installed: false,
        }
    }
}

// ---------------------------------------------------------------------------
// HF install (feature-gated)
// ---------------------------------------------------------------------------

/// True when `path` is `root` or a descendant (lexical component prefix).
///
/// Rejects `..` components in either path. Used so install dest cannot escape
/// the model store before the path exists on disk (no canonicalize required).
#[cfg(feature = "install")]
fn path_is_under_store(path: &Path, root: &Path) -> bool {
    use std::path::Component;
    let has_parent = |p: &Path| p.components().any(|c| matches!(c, Component::ParentDir));
    if has_parent(path) || has_parent(root) {
        return false;
    }
    let root_comps: Vec<_> = root.components().collect();
    let path_comps: Vec<_> = path.components().collect();
    if path_comps.len() < root_comps.len() {
        return false;
    }
    path_comps[..root_comps.len()] == root_comps[..]
}

/// Resolve install destination under `store`.
///
/// - Empty override → `store/owner__name`
/// - Relative override → `store/<override>` (no `..` components)
/// - Absolute override → allowed only when already under `store`
#[cfg(feature = "install")]
fn resolve_install_dest(
    dest_override: &str,
    store: &Path,
    owner: &str,
    name: &str,
) -> Result<PathBuf, String> {
    use std::path::Component;
    let raw = dest_override.trim();
    let dest = if raw.is_empty() {
        store.join(format!("{owner}__{name}"))
    } else {
        let p = Path::new(raw);
        if p.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err("destination must not contain '..'".into());
        }
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            store.join(p)
        }
    };
    if dest.as_os_str().is_empty() {
        return Err("destination path is empty".into());
    }
    if !path_is_under_store(&dest, store) {
        return Err(format!(
            "destination must be under model store ({})",
            store.display()
        ));
    }
    Ok(dest)
}

/// Validate install form fields without touching the network.
///
/// Returns `(repo_id, revision, dest)` or an error message.
/// Dest is always under the model store (relative join, or absolute only if
/// already a store descendant). `..` path segments are rejected.
#[cfg(feature = "install")]
pub fn validate_install_form(
    repo_id: &str,
    revision: &str,
    dest_override: &str,
    model_store: Option<&Path>,
) -> Result<(String, Option<String>, PathBuf), String> {
    let repo = repo_id.trim();
    if repo.is_empty() {
        return Err("repo id is required (e.g. org/model)".into());
    }
    if !repo.contains('/') || repo.starts_with('/') || repo.ends_with('/') {
        return Err("repo id must look like owner/name".into());
    }
    if repo.contains("..") || repo.contains('\\') {
        return Err("repo id must not contain path traversal".into());
    }
    let parts: Vec<&str> = repo.split('/').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err("repo id must be exactly owner/name".into());
    }
    for p in &parts {
        if p.chars()
            .any(|c| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'))
        {
            return Err("repo id has invalid characters".into());
        }
    }

    let rev = {
        let r = revision.trim();
        if r.is_empty() {
            None
        } else {
            if r.contains("..") || r.contains('/') || r.contains('\\') {
                return Err("revision must be a single ref name".into());
            }
            Some(r.to_string())
        }
    };

    let store = model_store
        .map(Path::to_path_buf)
        .unwrap_or_else(default_model_store_path);

    let dest = resolve_install_dest(dest_override, &store, parts[0], parts[1])?;

    Ok((repo.to_string(), rev, dest))
}

/// Free bytes at dest (or parent / model store) for the install panel.
#[cfg(feature = "install")]
pub fn install_free_bytes(dest: &Path) -> u64 {
    disk_free_bytes(dest)
        .or_else(|_| dest.parent().map(disk_free_bytes).unwrap_or(Ok(0)))
        .unwrap_or(0)
}

/// Free space line with explicit min-free threshold (GB).
#[cfg(feature = "install")]
pub fn format_install_space_with_min(dest: &Path, free: u64, min_free_bytes: u64) -> String {
    if min_free_bytes == 0 {
        format!(
            "dest {} · free {:.2} GB · min free: off",
            dest.display(),
            free as f64 / GB as f64
        )
    } else {
        format!(
            "dest {} · free {:.2} GB · min {:.1} GB",
            dest.display(),
            free as f64 / GB as f64,
            min_free_bytes as f64 / GB as f64
        )
    }
}

/// Default minimum free disk for install (1 decimal GB). Set field to 0 to skip.
#[cfg(feature = "install")]
pub const DEFAULT_INSTALL_MIN_FREE_BYTES: u64 = GB;

/// Parse min-free field: empty → default; `0` → skip gate; otherwise GB as f64.
#[cfg(feature = "install")]
pub fn parse_min_free_gb(text: &str) -> Result<u64, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(DEFAULT_INSTALL_MIN_FREE_BYTES);
    }
    let gb: f64 = t
        .parse()
        .map_err(|_| format!("min free GB must be a number (got {t:?})"))?;
    if gb < 0.0 {
        return Err("min free GB must be >= 0".into());
    }
    if gb == 0.0 {
        return Ok(0);
    }
    Ok((gb * GB as f64) as u64)
}

/// Refuse install when free space is below the threshold (clear UI message).
#[cfg(feature = "install")]
pub fn check_install_free_space(dest: &Path, min_free_bytes: u64) -> Result<u64, String> {
    let free = install_free_bytes(dest);
    if min_free_bytes > 0 && free < min_free_bytes {
        return Err(format!(
            "not enough free space: need ~{:.1} GB free, have {:.2} GB at {}",
            min_free_bytes as f64 / GB as f64,
            free as f64 / GB as f64,
            dest.display()
        ));
    }
    Ok(free)
}

// ---------------------------------------------------------------------------
// Determinate progress (install + generate) — host helpers over progress math
// ---------------------------------------------------------------------------

/// Units completed per second from elapsed wall time. Zero when not measurable.
pub fn progress_rate(done: u64, elapsed_secs: f64) -> f64 {
    if !elapsed_secs.is_finite() || elapsed_secs <= 0.0 || done == 0 {
        return 0.0;
    }
    done as f64 / elapsed_secs
}

/// Compact `done/total` byte pair for install status (GiB when large).
///
/// Examples: `12.5/372.0 GiB`, `512/1024 MiB`, `1200/5000 B`.
pub fn format_install_bytes_pair(done: u64, total: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    if total >= (GIB as u64) || done >= (GIB as u64) {
        return format!("{:.1}/{:.1} GiB", done as f64 / GIB, total as f64 / GIB);
    }
    if total >= (MIB as u64) || done >= (MIB as u64) {
        return format!("{:.0}/{:.0} MiB", done as f64 / MIB, total as f64 / MIB);
    }
    if total >= (KIB as u64) || done >= (KIB as u64) {
        return format!("{:.0}/{:.0} KiB", done as f64 / KIB, total as f64 / KIB);
    }
    format!("{done}/{total} B")
}

/// Plain-English label for an install phase code.
#[cfg(feature = "install")]
pub fn install_phase_label(phase: &str) -> &'static str {
    match phase {
        "download" => "Downloading...",
        "inspect" => "Checking files...",
        "register" => "Registering...",
        "done" => "Done",
        _ => "Working...",
    }
}

/// Rate in the unit preferred by [`crate::progress::install_progress`] (bytes/s
/// when byte totals are known, else files/s).
#[cfg(feature = "install")]
pub fn install_rate_from_progress(p: &InstallProgress, elapsed_secs: f64) -> f64 {
    if let (Some(done), Some(total)) = (p.bytes_done, p.bytes_total) {
        if total > 0 {
            return progress_rate(done, elapsed_secs);
        }
    }
    if let (Some(done), Some(total)) = (p.files_done, p.files_total) {
        if total > 0 {
            return progress_rate(done as u64, elapsed_secs);
        }
    }
    0.0
}

/// Build a [`crate::progress::ProgressView`] from a live install progress event.
///
/// - `"done"` → 100%
/// - `"inspect"` / `"register"` → high phase floors (no drop to unknown)
/// - download with trustworthy counters → real percent + optional ETA
/// - download without counters / zero done → **omit** percent and ETA (no fake 0%)
#[cfg(feature = "install")]
pub fn progress_view_for_install(
    p: &InstallProgress,
    elapsed_secs: f64,
) -> crate::progress::ProgressView {
    use crate::progress::{ProgressView, install_progress_view};
    let label = install_phase_label(&p.phase);
    // Post-download phases have known high floors; download uses real counters only.
    if let Some(floor) = install_phase_percent_floor(&p.phase) {
        return ProgressView::new(Some(floor), Some(0), label);
    }
    let rate = install_rate_from_progress(p, elapsed_secs);
    // Hub path fills real counters; CLI with no totals gets label only (no
    // invented 5% floor — Option honesty beats inaccurate anything).
    install_progress_view(
        label,
        p.bytes_done,
        p.bytes_total,
        p.files_done,
        p.files_total,
        rate,
    )
}

/// Synthetic percent for known post-download phases only.
///
/// Download without counters no longer uses a floor; callers omit percent.
#[cfg(feature = "install")]
pub fn install_phase_percent_floor(phase: &str) -> Option<u8> {
    match phase {
        "inspect" => Some(95),
        "register" => Some(98),
        "done" => Some(100),
        // download / unknown: no trustworthy fraction without counters
        _ => None,
    }
}

/// Build generate progress from live token count vs configured max output.
pub fn progress_view_for_generate(
    generated: u32,
    max_output: u32,
    tok_per_sec: f64,
) -> crate::progress::ProgressView {
    crate::progress::generate_progress_view("Generating...", generated, max_output, tok_per_sec)
}

/// Force 100% generate strip (Done frame / early stop still shows complete bar).
pub fn progress_view_generate_done() -> crate::progress::ProgressView {
    crate::progress::ProgressView::new(Some(100), Some(0), "Generating...")
}

/// Default install options for the GPUI install panel.
///
/// Prefer the hub path (`prefer_cli: false`) so the UI gets file/byte totals
/// during download instead of a frozen 0% CLI transfer. CLI remains available
/// when callers set `prefer_cli: true` on [`InstallOptions`] directly.
#[cfg(feature = "install")]
pub fn install_options_for_ui(dest: PathBuf, min_free_bytes: u64) -> InstallOptions {
    InstallOptions {
        dest,
        prefer_cli: false,
        min_free_bytes,
        inspect_after: true,
        register: false,
    }
}

/// Background install with prefer-cli default; progress on channel + live atomics.
///
/// Returns cancel handle and [`InstallLiveProgress`] for mid-file UI polls
/// (hub ProgressHandler updates atomics while a multi-GB shard downloads).
///
/// [`InstallCancel::request`] cancels; [`InstallCancel::request_pause`] pauses
/// after the current file.
#[cfg(feature = "install")]
pub fn install_async(
    repo_id: String,
    revision: Option<String>,
    dest: PathBuf,
    min_free_bytes: u64,
    tx: mpsc::Sender<InstallEvent>,
) -> (InstallCancel, std::sync::Arc<InstallLiveProgress>) {
    let cancel = InstallCancel::new();
    let cancel_bg = cancel.clone();
    let live = std::sync::Arc::new(InstallLiveProgress::new());
    let live_bg = live.clone();
    thread::spawn(move || {
        let source = InstallSource::HuggingFace {
            repo_id,
            revision,
            allow_patterns: None,
        };
        let opts = install_options_for_ui(dest, min_free_bytes);
        let result = install_model_cancellable_live(&source, &opts, &cancel_bg, live_bg, |p| {
            let _ = tx.send(InstallEvent::Progress(p));
        });
        match result {
            Ok(r) => {
                let _ = tx.send(InstallEvent::Done(r));
            }
            Err(e) => {
                let msg = e.to_string();
                // Pause is not a red error; cancel and other failures stay Error.
                if msg.contains(INSTALL_PAUSED_MSG) {
                    let _ = tx.send(InstallEvent::Paused);
                } else {
                    let msg = if msg.contains(INSTALL_CANCELLED_MSG) {
                        INSTALL_CANCELLED_MSG.to_string()
                    } else {
                        msg
                    };
                    let _ = tx.send(InstallEvent::Error(msg));
                }
            }
        }
    });
    (cancel, live)
}

#[cfg(test)]
mod tests {
    use super::*;
    use colibri_sys::MachineInfo;
    use std::ffi::OsString;
    use std::sync::Mutex as StdMutex;

    static PLAN_ENV_TEST: StdMutex<()> = StdMutex::new(());

    struct RestoreEnv {
        keys: Vec<(&'static str, Option<OsString>)>,
    }

    impl RestoreEnv {
        fn capture(keys: &[&'static str]) -> Self {
            Self {
                keys: keys.iter().map(|k| (*k, std::env::var_os(k))).collect(),
            }
        }
    }

    impl Drop for RestoreEnv {
        fn drop(&mut self) {
            for (k, v) in &self.keys {
                unsafe {
                    match v {
                        Some(val) => std::env::set_var(k, val),
                        None => std::env::remove_var(k),
                    }
                }
            }
            clear_plan_env_written_for_tests();
        }
    }

    fn sample_turn(completion_tokens: u32) -> ProfileTurn {
        ProfileTurn {
            wall_s: 1.0,
            prompt_tokens: 10,
            completion_tokens,
            expert_disk_s: 0.1,
            expert_wait_s: 0.0,
            expert_matmul_s: 0.5,
            attention_s: 0.2,
            lm_head_s: 0.05,
            forwards: 30,
        }
    }

    #[test]
    fn format_machine_summary_is_short() {
        let Ok(m) = MachineInfo::probe() else {
            return;
        };
        let s = format_machine_summary(&m);
        assert!(s.contains("Memory:"), "{s}");
        assert!(s.contains("CPU:"), "{s}");
        assert!(s.contains("GPU:"), "{s}");
        // Advanced inventory stays out of the default summary.
        assert!(!s.contains("SIMD:"), "{s}");
        assert!(!s.contains("Model store:"), "{s}");
        assert!(!s.contains("NPU:"), "{s}");
    }

    #[test]
    fn format_machine_details_includes_advanced() {
        let Ok(m) = MachineInfo::probe() else {
            return;
        };
        let s = format_machine_details(&m);
        assert!(s.contains("SIMD:"), "{s}");
        assert!(s.contains("Model store:"), "{s}");
        assert!(s.contains("NPU:"), "{s}");
    }

    #[test]
    fn format_machine_expanded_combines_summary_and_details() {
        let Ok(m) = MachineInfo::probe() else {
            return;
        };
        let short = format_machine(&m, false);
        let long = format_machine(&m, true);
        assert!(short.contains("Memory:"), "{short}");
        assert!(!short.contains("SIMD:"), "{short}");
        assert!(long.contains("Memory:"), "{long}");
        assert!(long.contains("Details"), "{long}");
        assert!(long.contains("SIMD:"), "{long}");
    }

    #[test]
    fn model_path_unset_for_doctor_empty_and_whitespace() {
        assert!(model_path_unset_for_doctor(Path::new("")));
        assert!(model_path_unset_for_doctor(Path::new("   ")));
        assert!(model_path_unset_for_doctor(Path::new("\t")));
        // Deliberate cwd is a real path for doctor.
        assert!(!model_path_unset_for_doctor(Path::new(".")));
        assert!(!model_path_unset_for_doctor(Path::new("/tmp/model")));
    }

    #[test]
    fn format_idle_doctor_checklist_is_idle_not_fail() {
        let s = format_idle_doctor_checklist();
        assert!(s.contains("Overall: Idle"), "{s}");
        assert!(s.contains("Path: (none selected)"), "{s}");
        assert!(
            s.to_lowercase().contains("scan") || s.to_lowercase().contains("model path"),
            "{s}"
        );
        assert!(!s.contains("Overall: Fail"), "{s}");
        assert!(!s.contains("[fail]"), "{s}");
        assert!(!s.contains("COLIBRI_"), "{s}");
    }

    #[test]
    fn run_shallow_doctor_empty_path_is_idle() {
        let s = run_shallow_doctor(Path::new(""), None);
        assert!(s.contains("Overall: Idle"), "{s}");
        assert!(!s.contains("Overall: Fail"), "{s}");
        let deep = run_deep_doctor(Path::new(""), None);
        assert!(deep.contains("Overall: Idle"), "{deep}");
    }

    #[test]
    fn expand_user_path_tilde_for_doctor_and_plan() {
        // Shared helper contract: ~/… must not stay literal for host path work.
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .expect("HOME or USERPROFILE for tilde test");
        let expanded = expand_user_path(Path::new("~/.models"));
        assert_eq!(expanded, PathBuf::from(home).join(".models"));
        assert!(!expanded.to_string_lossy().starts_with('~'));
    }

    #[test]
    fn doctor_missing_process_never_says_not_built() {
        // Uncreatable absolute path (no permission under /no): short recovery,
        // not "engine is not built".
        let s = run_shallow_doctor(Path::new("/no/such/colibri-model-dir-xyz"), None);
        assert!(
            !s.contains("not built"),
            "doctor checklist must not say not built: {s}"
        );
        assert!(
            s.contains("Could not create") || s.to_lowercase().contains("needs model"),
            "{s}"
        );
        assert!(s.contains("Default store:"), "{s}");
    }

    #[test]
    fn format_missing_model_directory_is_compact() {
        let path = Path::new("/home/user/.models");
        let store = Path::new("/home/user/.local/share/colibri/models");
        let s = format_missing_model_directory(path, store);
        assert!(s.contains("Overall: Needs model"), "{s}");
        assert!(s.contains("Path: /home/user/.models"), "{s}");
        assert!(s.contains("This folder is missing."), "{s}");
        assert!(
            s.contains("Default store: /home/user/.local/share/colibri/models"),
            "{s}"
        );
        // Compact: 4 content lines, no env soup, no essay, no check dump.
        let non_empty = s.lines().filter(|l| !l.trim().is_empty()).count();
        assert!(
            non_empty <= 5,
            "missing-path copy must stay short ({non_empty} lines): {s}"
        );
        assert!(
            s.len() < 220,
            "missing-path copy too long ({}): {s}",
            s.len()
        );
        assert!(!s.contains("COLIBRI_"), "{s}");
        assert!(!s.contains("Hugging Face"), "{s}");
        assert!(!s.contains("[fail]"), "{s}");
        assert!(!s.contains("tokenizer"), "{s}");
        assert!(!s.contains("config.json"), "{s}");
    }

    #[test]
    fn format_missing_model_directory_leads_with_recovery() {
        let path = Path::new("/home/user/.models");
        let store = Path::new("/home/user/.local/share/colibri/models");
        let s = format_missing_model_directory(path, store);
        assert!(s.starts_with("Overall: Needs model"), "{s}");
        assert!(s.contains("This folder is missing."), "{s}");
        assert!(s.contains("Default store:"), "{s}");
        assert!(!s.contains("[fail] config"), "{s}");
    }

    #[test]
    fn format_created_model_directory_is_compact() {
        let path = Path::new("/home/user/.models");
        let store = Path::new("/home/user/.local/share/colibri/models");
        let s = format_created_model_directory(path, store);
        assert!(s.contains("Overall: Needs model"), "{s}");
        assert!(s.contains("Path: /home/user/.models"), "{s}");
        assert!(
            s.contains("Created this folder") && s.contains("colibri.toml"),
            "{s}"
        );
        assert!(
            s.contains("Install a model") || s.to_lowercase().contains("scan"),
            "{s}"
        );
        assert!(!s.contains("This folder is missing."), "{s}");
        let non_empty = s.lines().filter(|l| !l.trim().is_empty()).count();
        assert!(
            non_empty <= 5,
            "created-path copy too long ({non_empty}): {s}"
        );
        assert!(
            s.len() < 280,
            "created-path copy too long ({}): {s}",
            s.len()
        );
    }

    #[test]
    fn format_not_a_model_folder_is_plain_english() {
        let path = Path::new("/home/user/.models");
        let store = Path::new("/home/user/.local/share/colibri/models");
        let s = format_not_a_model_folder(path, store);
        assert!(s.contains("Overall: Needs model"), "{s}");
        assert!(s.contains("not a model yet"), "{s}");
        assert!(
            s.contains("Install a model") || s.contains("paste a model"),
            "{s}"
        );
        // Must not lead with raw config.json as the only recovery line.
        assert!(!s.contains("This folder has no config.json."), "{s}");
        let with_toml = format_not_a_model_folder_ex(path, store, true);
        assert!(
            with_toml.contains("Created default colibri.toml"),
            "{with_toml}"
        );
        assert!(with_toml.contains("not a model yet"), "{with_toml}");
    }

    #[test]
    fn is_model_leaf_requires_config_json() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_model_leaf(dir.path()));
        assert!(!is_model_leaf(Path::new("/no/such/path-xyz")));
        assert!(!is_model_leaf(Path::new("")));
        std::fs::write(dir.path().join("config.json"), br#"{"model_type":"glm"}"#).unwrap();
        assert!(is_model_leaf(dir.path()));
    }

    #[test]
    fn preflight_rejects_empty_and_non_model_without_open() {
        let err = preflight_model_for_engine_start(Path::new("")).unwrap_err();
        assert_eq!(err, ENGINE_START_NOT_A_MODEL);
        assert!(err.contains("not a model yet"), "{err}");
        assert!(err.contains("Install a model"), "{err}");
        // Lab protocol wording must never be the preflight path.
        assert!(!err.contains("EOF"), "{err}");
        assert!(!err.contains("READY"), "{err}");
        assert!(!err.contains("serve protocol"), "{err}");

        let dir = tempfile::tempdir().unwrap();
        // Empty store-style folder (no config.json).
        let err = preflight_model_for_engine_start(dir.path()).unwrap_err();
        assert_eq!(err, ENGINE_START_NOT_A_MODEL);

        // Missing path.
        let missing = dir.path().join("nope-missing");
        let err = preflight_model_for_engine_start(&missing).unwrap_err();
        assert_eq!(err, ENGINE_START_NOT_A_MODEL);

        // Real leaf passes preflight (still may fail later on weights/binary).
        let leaf = dir.path().join("leaf");
        std::fs::create_dir(&leaf).unwrap();
        std::fs::write(leaf.join("config.json"), br#"{"model_type":"glm"}"#).unwrap();
        let ok = preflight_model_for_engine_start(&leaf).unwrap();
        assert_eq!(ok, leaf);
    }

    fn write_tiny_glm_leaf(dir: &Path) -> PathBuf {
        use std::io::Write;
        let leaf = dir.join("leaf");
        std::fs::create_dir(&leaf).unwrap();
        std::fs::write(
            leaf.join("config.json"),
            r#"{"model_type":"glm","num_hidden_layers":8,"n_routed_experts":16,"kv_lora_rank":32,"qk_rope_head_dim":8,"qk_nope_head_dim":24,"v_head_dim":32,"num_attention_heads":4}"#,
        )
        .unwrap();
        let mut header = serde_json::Map::new();
        header.insert(
            "model.layers.0.mlp.experts.0.gate_proj.weight".into(),
            serde_json::json!({
                "dtype": "U8",
                "shape": [64],
                "data_offsets": [0, 64],
            }),
        );
        let raw = serde_json::to_vec(&serde_json::Value::Object(header)).unwrap();
        let mut f = std::fs::File::create(leaf.join("model.safetensors")).unwrap();
        f.write_all(&(raw.len() as u64).to_le_bytes()).unwrap();
        f.write_all(&raw).unwrap();
        f.write_all(&[0u8; 64]).unwrap();
        leaf
    }

    #[test]
    fn preflight_ram_refuses_without_calling_open() {
        reset_ffi_open_attempts();
        let dir = tempfile::tempdir().unwrap();
        let leaf = write_tiny_glm_leaf(dir.path());
        // Tiny available RAM: plan runtime reserve alone exceeds 64 MiB.
        let err = preflight_then_maybe_open(&leaf, Some(64 * 1024 * 1024), false).unwrap_err();
        assert_eq!(err, ENGINE_START_RAM_TOO_SMALL);
        assert!(
            !err.contains("—") && !err.contains("--"),
            "plain English, no dash theater: {err}"
        );
        assert_eq!(
            ffi_open_attempts(),
            0,
            "Start refuse must not call coli_glm_engine_open"
        );
    }

    #[test]
    fn preflight_ram_refuse_copy_is_engine_start_ram_too_small() {
        let dir = tempfile::tempdir().unwrap();
        let leaf = write_tiny_glm_leaf(dir.path());
        let err = preflight_then_maybe_open(&leaf, Some(64 * 1024 * 1024), false).unwrap_err();
        assert_eq!(err, ENGINE_START_RAM_TOO_SMALL);
    }

    #[test]
    fn preflight_ram_inspect_failure_does_not_skip_gate() {
        let dir = tempfile::tempdir().unwrap();
        let leaf = dir.path().join("bad-leaf");
        std::fs::create_dir_all(&leaf).unwrap();
        std::fs::write(leaf.join("config.json"), "{").unwrap();
        let err = preflight_ram_for_engine_start(&leaf, Some(64 * 1024 * 1024), false).unwrap_err();
        assert_eq!(err, ENGINE_START_RAM_UNMEASURABLE);
        let ok = preflight_ram_for_engine_start(&leaf, Some(64 * 1024 * 1024), true);
        assert!(ok.is_ok(), "overcommit may skip inspect-fail gate: {ok:?}");
    }

    #[test]
    fn apply_plan_env_for_ffi_setdefaults_ram_gb_and_omp_when_unset() {
        let _serial = PLAN_ENV_TEST.lock().unwrap_or_else(|e| e.into_inner());
        let _restore = RestoreEnv::capture(&["RAM_GB", "OMP_NUM_THREADS"]);
        unsafe {
            std::env::remove_var("RAM_GB");
            std::env::remove_var("OMP_NUM_THREADS");
        }
        clear_plan_env_written_for_tests();
        let dir = tempfile::tempdir().unwrap();
        let leaf = write_tiny_glm_leaf(dir.path());
        apply_plan_env_for_ffi(&leaf);
        assert!(
            std::env::var("RAM_GB")
                .ok()
                .filter(|s| !s.is_empty())
                .is_some(),
            "unset RAM_GB must be setdefault from the plan"
        );
        assert!(
            std::env::var("OMP_NUM_THREADS")
                .ok()
                .filter(|s| !s.is_empty())
                .is_some(),
            "unset OMP_NUM_THREADS must be setdefault from the plan"
        );
        unsafe {
            std::env::set_var("RAM_GB", "7");
        }
        clear_plan_env_written_for_tests();
        apply_plan_env_for_ffi(&leaf);
        assert_eq!(
            std::env::var("RAM_GB").unwrap(),
            "7",
            "operator-set RAM_GB must not be overwritten"
        );
    }

    #[test]
    fn apply_plan_env_refreshes_ram_gb_on_second_start_unless_operator_set() {
        let _serial = PLAN_ENV_TEST.lock().unwrap_or_else(|e| e.into_inner());
        let _restore = RestoreEnv::capture(&["RAM_GB", "OMP_NUM_THREADS"]);
        unsafe {
            std::env::remove_var("RAM_GB");
            std::env::remove_var("OMP_NUM_THREADS");
        }
        clear_plan_env_written_for_tests();
        let dir = tempfile::tempdir().unwrap();
        let leaf = write_tiny_glm_leaf(dir.path());
        apply_plan_env_for_ffi(&leaf);
        let first = std::env::var("RAM_GB").expect("first Start writes RAM_GB");
        assert_ne!(first, "999");
        unsafe {
            std::env::set_var("RAM_GB", "999");
        }
        apply_plan_env_for_ffi(&leaf);
        let second = std::env::var("RAM_GB").expect("second Start keeps RAM_GB");
        assert_ne!(
            second, "999",
            "values this function wrote must refresh; stale 999 must not stick (first was {first})"
        );
        assert!(
            second.parse::<f64>().ok().is_some_and(|g| g > 0.0),
            "refreshed RAM_GB must be a plan number, got {second}"
        );
    }

    #[test]
    fn ffi_generate_stopped_does_not_start_process_fallback() {
        assert!(!ffi_generate_error_should_fallback("stopped"));
        assert!(!ffi_generate_error_should_fallback("engine: stopped"));
        assert!(!ffi_generate_error_should_fallback("cancelled"));
        assert!(ffi_generate_error_should_fallback("OOM"));
        assert!(ffi_generate_error_should_fallback("model_dir required"));
    }

    #[test]
    fn ffi_open_ram_error_does_not_start_process_fallback() {
        assert!(!ffi_open_error_should_fallback(ENGINE_START_RAM_TOO_SMALL));
        assert!(!ffi_open_error_should_fallback(
            ENGINE_START_RAM_UNMEASURABLE
        ));
        assert!(ffi_open_error_should_fallback(
            "no safetensors weights found"
        ));
    }

    #[test]
    fn preflight_ram_overcommit_skips_refuse() {
        reset_ffi_open_attempts();
        let dir = tempfile::tempdir().unwrap();
        let leaf = write_tiny_glm_leaf(dir.path());
        let result = preflight_then_maybe_open(&leaf, Some(64 * 1024 * 1024), true);
        assert!(
            result.is_ok(),
            "COLI_RAM_OVERCOMMIT must skip the Start refuse: {result:?}"
        );
        assert_eq!(
            ffi_open_attempts(),
            0,
            "test helper must not really open the engine"
        );
    }

    #[test]
    fn map_engine_start_error_protocol_eof_is_plain() {
        let model = Path::new("/home/user/.models");
        let lab = "engine start failed: serve protocol error: EOF before READY";
        let plain = map_engine_start_error(lab, model);
        assert!(
            plain.starts_with("engine quit before it was ready"),
            "{plain}"
        );
        assert!(
            plain.contains("bad model path") || plain.contains("missing engine"),
            "{plain}"
        );
        assert!(plain.contains("/home/user/.models"), "{plain}");
        assert!(!plain.contains("EOF before READY"), "{plain}");
        assert!(!plain.contains("serve protocol error"), "{plain}");
        assert!(!plain.contains("engine start failed"), "{plain}");

        // Nested variants still map.
        let bare = map_engine_start_error("EOF before READY", model);
        assert!(
            bare.starts_with("engine quit before it was ready"),
            "{bare}"
        );
        let wait = map_engine_start_error(
            "serve protocol error: waiting for READY: broken pipe",
            model,
        );
        assert!(
            wait.starts_with("engine quit before it was ready"),
            "{wait}"
        );
    }

    #[test]
    fn map_engine_start_error_missing_binary_is_plain() {
        let model = Path::new("/home/user/.models/glm");
        let lab = "engine start failed: engine error: colibri engine is not built or not on search path; set COLI_ENGINE or build with `make -C c colibri`";
        let plain = map_engine_start_error(lab, model);
        assert!(plain.starts_with("engine binary not found"), "{plain}");
        assert!(
            plain.contains("COLI_ENGINE") || plain.contains("Build"),
            "{plain}"
        );
        assert!(plain.contains("/home/user/.models/glm"), "{plain}");
        assert!(!plain.contains("not on search path"), "{plain}");
    }

    #[test]
    fn map_engine_start_error_passes_through_preflight() {
        let model = Path::new("~/.models");
        let plain = map_engine_start_error(ENGINE_START_NOT_A_MODEL, model);
        assert_eq!(plain, ENGINE_START_NOT_A_MODEL);
    }

    #[test]
    fn engine_session_start_preflight_rejects_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        // No config.json: must fail before serve / open.
        // Match (not unwrap_err): EngineSession is not Debug.
        let err = match EngineSession::start(dir.path()) {
            Err(e) => e,
            Ok(_) => panic!("expected preflight rejection for empty store"),
        };
        assert_eq!(err, ENGINE_START_NOT_A_MODEL);
        assert!(!err.contains("EOF"), "{err}");
        assert!(!err.contains("READY"), "{err}");
        assert!(!err.contains("serve protocol"), "{err}");
        assert!(!err.contains("COLIBRI_MODEL"), "{err}");
    }

    #[test]
    fn format_could_not_create_model_directory_names_error() {
        let path = Path::new("/home/user/.models");
        let store = Path::new("/home/user/.local/share/colibri/models");
        let err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Permission denied");
        let s = format_could_not_create_model_directory(path, store, &err);
        assert!(s.contains("Could not create this folder:"), "{s}");
        assert!(s.contains("Permission denied"), "{s}");
        assert!(s.contains("Default store:"), "{s}");
        assert!(!s.contains("This folder is missing."), "{s}");
    }

    #[test]
    fn run_shallow_doctor_creates_missing_path() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("new-models-dir");
        assert!(!path.exists());
        let s = run_shallow_doctor(&path, None);
        assert!(path.is_dir(), "doctor must create missing model path");
        assert!(
            s.contains("Created this folder") && s.contains("colibri.toml"),
            "{s}"
        );
        assert!(s.contains("Overall: Needs model"), "{s}");
        assert!(!s.contains("This folder is missing."), "{s}");
        assert!(!s.contains("[fail] config.json"), "{s}");
        assert!(!s.contains("COLIBRI_"), "{s}");
        assert!(
            path.join(STORE_CONFIG_FILE_NAME).is_file(),
            "doctor must write default colibri.toml"
        );
        assert!(
            !path.join("config.json").exists(),
            "must not invent HF config.json"
        );
        let deep = run_deep_doctor(&path, None);
        // Second run: path exists, empty → not-a-model (not "created folder" again).
        assert!(
            deep.contains("not a model yet") || deep.contains("Needs model"),
            "{deep}"
        );
        assert!(!deep.contains("Created this folder"), "{deep}");
        // Second run does not re-claim colibri.toml create.
        assert!(!deep.contains("Created default colibri.toml"), "{deep}");
    }

    #[test]
    fn run_shallow_doctor_empty_dir_writes_colibri_toml() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("empty-store");
        fs::create_dir_all(&path).unwrap();
        assert!(!path.join(STORE_CONFIG_FILE_NAME).exists());
        let s = run_shallow_doctor(&path, None);
        assert!(
            path.join(STORE_CONFIG_FILE_NAME).is_file(),
            "empty non-model dir must get colibri.toml"
        );
        assert!(
            !path.join("config.json").exists(),
            "must not invent HF config.json"
        );
        assert!(s.contains("Overall: Needs model"), "{s}");
        assert!(s.contains("Created default colibri.toml"), "{s}");
        assert!(s.contains("not a model yet"), "{s}");
        assert!(!s.contains("This folder has no config.json."), "{s}");
        // Idempotent: second doctor does not rewrite message as "created".
        let again = run_shallow_doctor(&path, None);
        assert!(!again.contains("Created default colibri.toml"), "{again}");
        assert!(again.contains("not a model yet"), "{again}");
    }

    #[test]
    fn run_plan_empty_dir_is_not_raw_missing_config() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("empty-for-plan");
        fs::create_dir_all(&path).unwrap();
        let s = run_plan(&path, None);
        assert!(s.to_lowercase().contains("no memory plan"), "{s}");
        assert!(
            s.to_lowercase().contains("not a model") || s.to_lowercase().contains("install"),
            "{s}"
        );
        assert!(
            !s.contains("missing config.json") || s.to_lowercase().contains("not a model"),
            "must not only dump raw missing config.json: {s}"
        );
        assert!(!s.contains("Could not build memory plan:"), "{s}");
    }

    #[test]
    fn ensure_store_colibri_toml_is_idempotent() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("store");
        fs::create_dir_all(&path).unwrap();
        assert!(ensure_store_colibri_toml(&path).unwrap());
        let body = fs::read_to_string(path.join(STORE_CONFIG_FILE_NAME)).unwrap();
        assert!(body.contains("version"), "{body}");
        assert!(body.contains("Not a Hugging Face"), "{body}");
        assert!(!ensure_store_colibri_toml(&path).unwrap());
    }

    #[test]
    fn run_shallow_doctor_real_model_leaf_still_runs_checks() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("real-model");
        fs::create_dir_all(&path).unwrap();
        // Minimal HF leaf: config.json only (shards may fail/warn; doctor still runs).
        fs::write(path.join("config.json"), br#"{"model_type":"glm"}"#).unwrap();
        let s = run_shallow_doctor(&path, None);
        // Full doctor checklist, not the empty-folder recovery copy.
        assert!(
            s.contains("Overall:") && !s.contains("Created default colibri.toml"),
            "{s}"
        );
        assert!(
            s.contains("[pass]") || s.contains("[fail]") || s.contains("[warn]"),
            "expected real doctor check marks: {s}"
        );
        // Scaffold must not invent store notes when this is already a model leaf
        // under doctor full path... actually we only skip store toml on model leaf.
        assert!(
            !path.join(STORE_CONFIG_FILE_NAME).exists(),
            "model leaf must not get store scaffold colibri.toml"
        );
    }

    #[test]
    fn run_shallow_doctor_uncreatable_path_reports_error() {
        let root = tempfile::tempdir().unwrap();
        let blocker = root.path().join("not-a-dir");
        std::fs::write(&blocker, b"x").unwrap();
        let path = blocker.join("models");
        let s = run_shallow_doctor(&path, None);
        assert!(s.contains("Could not create this folder:"), "{s}");
        assert!(!s.contains("This folder is missing."), "{s}");
        assert!(!s.contains("[fail] config.json"), "{s}");
        assert!(!path.exists());
        let deep = run_deep_doctor(&path, None);
        assert!(deep.contains("Could not create this folder:"), "{deep}");
    }

    #[test]
    fn run_shallow_doctor_unwritable_root_is_recovery_not_check_dump() {
        let s = run_shallow_doctor(Path::new("/no/such/colibri-model-dir-xyz"), None);
        assert!(s.contains("Could not create this folder:"), "{s}");
        assert!(s.contains("Default store:"), "{s}");
        assert!(!s.contains("[fail] config.json"), "{s}");
        assert!(!s.contains("COLIBRI_"), "{s}");
        let deep = run_deep_doctor(Path::new("/no/such/colibri-model-dir-xyz"), None);
        assert!(deep.contains("Could not create this folder:"), "{deep}");
    }

    #[test]
    fn resolve_startup_model_path_empty_prefs_uses_store() {
        let store = Path::new("/tmp/colibri-default-store");
        let s = resolve_startup_model_path(None, "", store, &[]);
        assert_eq!(s.display, store.display().to_string());
        assert!(s.note.is_some());
        let note = s.note.unwrap();
        assert!(
            note.to_lowercase().contains("store") || note.to_lowercase().contains("install"),
            "{note}"
        );
    }

    #[test]
    fn resolve_startup_model_path_env_wins() {
        let store = Path::new("/tmp/store");
        let s = resolve_startup_model_path(
            Some(PathBuf::from("/env/model")),
            "/prefs/model",
            store,
            &[],
        );
        assert_eq!(s.display, "/env/model");
        assert!(s.note.is_none());
    }

    #[test]
    fn resolve_startup_model_path_missing_prefs_auto_picks_single() {
        let store = Path::new("/tmp/store");
        let entry = ModelEntry {
            path: PathBuf::from("/tmp/store/only-model"),
            family: ModelFamily::Glm,
            engine_id: "coli".into(),
            status: ModelStatus::Present,
            model_bytes: 1,
            disk_bytes: 1,
            param_count: None,
            shards: 1,
            model_type: None,
            note: None,
        };
        // Use a path that cannot exist on the host (not `~/.models`, which operators
        // often create as an empty store root).
        let s = resolve_startup_model_path(
            None,
            "/no/such/colibri-prefs-model-xyz-never",
            store,
            std::slice::from_ref(&entry),
        );
        assert_eq!(s.display, "/tmp/store/only-model");
        assert!(s.note.as_ref().is_some_and(|n| n.contains("missing")));
    }

    #[test]
    fn pick_single_usable_model_none_when_many() {
        let a = ModelEntry {
            path: PathBuf::from("/a"),
            family: ModelFamily::Glm,
            engine_id: "coli".into(),
            status: ModelStatus::Present,
            model_bytes: 1,
            disk_bytes: 1,
            param_count: None,
            shards: 1,
            model_type: None,
            note: None,
        };
        let mut b = a.clone();
        b.path = PathBuf::from("/b");
        assert!(pick_single_usable_model(std::slice::from_ref(&a)).is_some());
        assert!(pick_single_usable_model(&[a, b]).is_none());
    }

    #[test]
    fn missing_path_scan_auto_selects_one() {
        let missing = Path::new("/home/user/.models");
        let store = Path::new("/tmp/store");
        let entry = ModelEntry {
            path: PathBuf::from("/tmp/store/m"),
            family: ModelFamily::Glm,
            engine_id: "coli".into(),
            status: ModelStatus::Present,
            model_bytes: 1,
            disk_bytes: 1,
            param_count: None,
            shards: 1,
            model_type: None,
            note: None,
        };
        // Path /tmp/store/m does not exist on disk → doctor will still be
        // recovery for that path, but outcome is AutoSelected.
        match missing_path_scan_outcome(missing, store, &[entry], None, false) {
            MissingPathScanOutcome::AutoSelected {
                path,
                doctor,
                status,
            } => {
                assert_eq!(path, PathBuf::from("/tmp/store/m"));
                assert!(
                    doctor.contains("Path was missing") || doctor.contains("Found one model"),
                    "{doctor}"
                );
                assert!(
                    status.to_lowercase().contains("only model")
                        || status.to_lowercase().contains("store"),
                    "{status}"
                );
            }
            other => panic!("expected AutoSelected, got {other:?}"),
        }
    }

    #[test]
    fn missing_path_scan_lists_many() {
        let missing = Path::new("/gone");
        let store = Path::new("/tmp/store");
        let a = ModelEntry {
            path: PathBuf::from("/tmp/store/a"),
            family: ModelFamily::Glm,
            engine_id: "coli".into(),
            status: ModelStatus::Present,
            model_bytes: 1,
            disk_bytes: 1,
            param_count: None,
            shards: 1,
            model_type: None,
            note: None,
        };
        let mut b = a.clone();
        b.path = PathBuf::from("/tmp/store/b");
        match missing_path_scan_outcome(missing, store, &[a, b], None, false) {
            MissingPathScanOutcome::ListedMany {
                doctor,
                status,
                entries,
            } => {
                assert_eq!(entries.len(), 2);
                assert!(doctor.contains("Found 2 model"), "{doctor}");
                assert!(status.contains("2 models"), "{status}");
            }
            other => panic!("expected ListedMany, got {other:?}"),
        }
    }

    #[test]
    fn run_plan_missing_path_defers_to_health_check() {
        let s = run_plan(Path::new("/no/such/plan-model-xyz"), None);
        assert!(s.to_lowercase().contains("no memory plan"), "{s}");
        assert!(s.to_lowercase().contains("path"), "{s}");
        // Do not duplicate the Health check recovery essay.
        assert!(!s.contains("Default store:"), "{s}");
        assert!(!s.contains("COLIBRI_"), "{s}");
        assert!(s.len() < 120, "plan missing-path copy too long: {s}");
    }

    #[test]
    fn format_empty_registry_scan_is_short() {
        let store = Path::new("/tmp/colibri-models-test");
        let s = format_empty_registry_scan(store);
        assert!(s.contains("/tmp/colibri-models-test"), "{s}");
        assert!(s.contains("config.json"), "{s}");
        assert!(s.contains("depth"), "{s}");
        assert!(!s.contains("COLIBRI_"), "{s}");
        assert!(s.len() < 120, "empty scan status too long: {s}");
    }

    #[test]
    fn format_doctor_checklist_is_not_cli_dump() {
        let report = colibri_sys::DoctorReport {
            schema_version: 1,
            status: "error".into(),
            model: "/tmp/demo-model".into(),
            mode: "standard".into(),
            checks: vec![
                colibri_sys::DoctorCheck {
                    id: "model.path".into(),
                    status: "fail".into(),
                    summary: "Model path is missing".into(),
                    details: None,
                },
                colibri_sys::DoctorCheck {
                    id: "engine.binary".into(),
                    status: "pass".into(),
                    summary: "Engine binary found".into(),
                    details: None,
                },
                colibri_sys::DoctorCheck {
                    id: "ram.budget".into(),
                    status: "warn".into(),
                    summary: "RAM is tight for this model".into(),
                    details: None,
                },
            ],
            plan: None,
        };
        let s = format_doctor_checklist(&report);
        assert!(s.contains("Overall: Fail"), "{s}");
        assert!(s.contains("Model: /tmp/demo-model"), "{s}");
        assert!(s.contains("Depth: quick"), "{s}");
        assert!(s.contains("[fail] Model path is missing"), "{s}");
        assert!(s.contains("[pass] Engine binary found"), "{s}");
        assert!(s.contains("[warn] RAM is tight"), "{s}");
        assert!(!s.contains("status="), "{s}");
        assert!(!s.contains("mode="), "{s}");
    }

    /// Fail checks must surface near the top so a short Doctor panel cannot
    /// look all-green while Overall is Fail (early passes + later fail).
    #[test]
    fn format_doctor_checklist_surfaces_fail_near_top() {
        let fail_summary = "RAM budget cannot hold one expert slot per sparse layer";
        let report = colibri_sys::DoctorReport {
            schema_version: 1,
            status: "error".into(),
            model: "/models/DeepSeek-V4-Flash".into(),
            mode: "standard".into(),
            checks: vec![
                colibri_sys::DoctorCheck {
                    id: "model.path".into(),
                    status: "pass".into(),
                    summary: "model directory is readable".into(),
                    details: None,
                },
                colibri_sys::DoctorCheck {
                    id: "model.config".into(),
                    status: "pass".into(),
                    summary: "config.json is valid".into(),
                    details: None,
                },
                colibri_sys::DoctorCheck {
                    id: "model.tokenizer".into(),
                    status: "pass".into(),
                    summary: "tokenizer.json found".into(),
                    details: None,
                },
                colibri_sys::DoctorCheck {
                    id: "storage.persistence".into(),
                    status: "pass".into(),
                    summary: "model directory can store usage and KV state".into(),
                    details: None,
                },
                colibri_sys::DoctorCheck {
                    id: "engine.binary".into(),
                    status: "pass".into(),
                    summary: "in-process engine is available".into(),
                    details: None,
                },
                colibri_sys::DoctorCheck {
                    id: "memory.ram".into(),
                    status: "fail".into(),
                    summary: fail_summary.into(),
                    details: None,
                },
                colibri_sys::DoctorCheck {
                    id: "placement.plan".into(),
                    status: "warn".into(),
                    summary: fail_summary.into(),
                    details: None,
                },
            ],
            plan: None,
        };
        let s = format_doctor_checklist(&report);
        assert!(s.contains("Overall: Fail"), "{s}");
        // Overall line (or immediate follow-on) names the first fail reason.
        let first_line = s.lines().next().unwrap_or("");
        assert!(
            first_line.contains("Overall: Fail") && first_line.contains(fail_summary),
            "Overall line must include fail reason; got: {first_line}\nfull:\n{s}"
        );
        // Fail rows emit before any pass so short panels show the problem first.
        let fail_pos = s
            .find(&format!("[fail] {fail_summary}"))
            .expect("fail row present");
        let first_pass = s.find("[pass]").expect("pass row present");
        assert!(
            fail_pos < first_pass,
            "fail must sort before pass; fail@{fail_pos} pass@{first_pass}\n{s}"
        );
        // Warn after fail, before pass.
        let warn_pos = s.find("[warn]").expect("warn row present");
        assert!(
            fail_pos < warn_pos && warn_pos < first_pass,
            "order fail, warn, pass; fail@{fail_pos} warn@{warn_pos} pass@{first_pass}\n{s}"
        );
    }

    #[test]
    fn format_doctor_checklist_deep_depth_label() {
        let report = colibri_sys::DoctorReport {
            schema_version: 1,
            status: "ok".into(),
            model: "/models/demo".into(),
            mode: "deep".into(),
            checks: vec![colibri_sys::DoctorCheck {
                id: "model.container".into(),
                status: "pass".into(),
                summary: "all tensor headers and layouts are internally consistent".into(),
                details: None,
            }],
            plan: None,
        };
        let s = format_doctor_checklist(&report);
        assert!(s.contains("Depth: thorough"), "{s}");
        assert!(s.contains("[pass] all tensor headers"), "{s}");
        assert!(!s.contains("mode=deep"), "{s}");
    }

    #[test]
    fn live_tiers_idle_messages_are_plain() {
        let start = live_tiers_idle_message(LiveTiersIdle::StartEngine);
        assert!(start.contains("start the engine"), "{start}");
        assert!(!start.contains("TIERS"), "{start}");
        assert!(!start.to_lowercase().contains("mux"), "{start}");
    }

    #[test]
    fn env_model_empty_without_env() {
        // Helpers are callable; when unset they return None (smoke + type).
        // Do not assert env absence globally (other tests / CI may set vars).
        let _ = env_model_path();
        let _ = env_engine_path();
    }

    #[test]
    fn messages_from_turns_orders_roles() {
        let msgs = messages_from_turns(&[
            ("user".into(), "hi".into()),
            ("assistant".into(), "hello".into()),
        ]);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[1].content, "hi");
        assert_eq!(msgs[2].content, "hello");
    }

    #[test]
    fn format_live_tiers_line() {
        let t = TiersSnap {
            vram: 10,
            ram: 20,
            disk: 100,
            vram_gb: 4.0,
            ram_gb: 16.0,
        };
        let s = format_live_tiers(&t);
        assert!(s.contains("GPU 10"), "{s}");
        assert!(s.contains("System RAM 20"), "{s}");
        assert!(s.contains("Disk 100"), "{s}");
        assert!(s.contains("4.0"), "{s}");
        assert!(s.contains("16.0"), "{s}");
        assert!(s.contains("GB"), "{s}");
        assert!(!s.contains("live experts"), "{s}");
    }

    #[test]
    fn format_live_hwinfo_plain_labels() {
        let h = HwinfoSnap {
            cores: 16,
            ram_total_gb: 64.0,
            ram_avail_gb: 40.5,
            gpus: 1,
            vram_total_gb: 24.0,
            cpu: "AMD Ryzen 9".into(),
            gpu: "Radeon RX 7900".into(),
        };
        let s = format_live_hwinfo(&h);
        assert!(s.starts_with("Engine hardware"), "{s}");
        assert!(s.contains("RAM 40.5 / 64.0 GB free"), "{s}");
        assert!(s.contains("16 cores"), "{s}");
        assert!(s.contains("AMD Ryzen 9"), "{s}");
        assert!(s.contains("GPU Radeon RX 7900"), "{s}");
        assert!(s.contains("24 GB"), "{s}");
        // No raw protocol field dump names.
        assert!(!s.contains("ram_total"), "{s}");
        assert!(!s.contains("vram_total"), "{s}");
        assert!(!s.contains("HWINFO"), "{s}");
    }

    #[test]
    fn format_live_hwinfo_omits_empty_names() {
        let h = HwinfoSnap {
            cores: 8,
            ram_total_gb: 32.0,
            ram_avail_gb: 10.0,
            gpus: 0,
            vram_total_gb: 0.0,
            cpu: String::new(),
            gpu: String::new(),
        };
        let s = format_live_hwinfo(&h);
        assert!(s.contains("8 cores"), "{s}");
        assert!(s.contains("RAM 10.0 / 32.0 GB free"), "{s}");
        assert!(!s.contains("GPU"), "{s}");
    }

    #[test]
    fn live_hwinfo_idle_messages() {
        assert!(live_hwinfo_idle_message(LiveHwinfoIdle::StartEngine).contains("start"));
        assert!(live_hwinfo_idle_message(LiveHwinfoIdle::EngineStopped).contains("stopped"));
        assert!(live_hwinfo_idle_message(LiveHwinfoIdle::Waiting).contains("waiting"));
    }

    #[test]
    fn format_profile_empty_and_nonempty() {
        let empty = format_profile_turns(&[], 5);
        assert!(
            empty.contains("No timing data") || empty.to_lowercase().contains("generate"),
            "{empty}"
        );
        let turns = vec![sample_turn(20)];
        let s = format_profile_turns(&turns, 5);
        assert!(s.contains("20"), "{s}");
        assert!(s.contains("tok/s"), "{s}");
        assert!(s.contains("wall"), "{s}");
        assert!(s.contains("prompt"), "{s}");
        assert!(s.contains("matmul"), "{s}");
        assert!(s.contains("attn"), "{s}");
        // No opaque single-letter wire abbreviations as the only labels.
        assert!(!s.contains("c=20"), "{s}");
        assert!(!s.contains(" mm="), "{s}");
    }

    #[test]
    fn format_profile_keeps_last_n_only() {
        let turns: Vec<_> = [11u32, 22, 33, 44, 55]
            .into_iter()
            .map(sample_turn)
            .collect();
        let s = format_profile_turns(&turns, 2);
        // Completion token columns for last two turns.
        assert!(s.contains("55"), "{s}");
        assert!(s.contains("44"), "{s}");
        assert!(!s.contains(" 11 "), "{s}");
        assert!(!s.contains(" 22 "), "{s}");
        assert!(!s.contains(" 33 "), "{s}");
        // last_n=0 → empty window (header only / no turn rows)
        let s0 = format_profile_turns(&turns, 0);
        assert!(!s0.contains("55"), "{s0}");
        // last_n > len → all turns
        let s_all = format_profile_turns(&turns, 99);
        assert!(s_all.contains("11") && s_all.contains("55"), "{s_all}");
        assert!(s_all.contains("last 5"), "{s_all}");
    }

    #[test]
    fn brain_view_samples_large_map() {
        let rows = 76u32;
        let cols = 256u32;
        let mut cells = vec![0u8; (rows * cols) as usize];
        // pack a few VRAM hot cells
        cells[0] = (2 << 6) | 40;
        cells[1] = (1 << 6) | 10;
        let map = ExpertMap { rows, cols, cells };
        let view = brain_view_from_map(&map, None, 0);
        assert!(view.sampled, "should sample large map");
        assert_eq!(view.src_rows, rows);
        assert_eq!(view.src_cols, cols);
        assert!(view.row_stride >= 1 && view.col_stride >= 1);
        assert_eq!(view.max_cells, BRAIN_MAX_CELLS);
        assert!(
            (view.disp_rows as usize) * (view.disp_cols as usize) <= BRAIN_MAX_CELLS,
            "disp {}×{}",
            view.disp_rows,
            view.disp_cols
        );
        assert!(view.note.contains("sampled"));
        // First sample is always source (0,0) → VRAM tier 2, heat 40.
        assert_eq!(view.cells[0].0, 2, "tier");
        assert_eq!(view.cells[0].1, 40, "heat");
        assert!((view.cells[0].2).abs() < f32::EPSILON);
    }

    #[test]
    fn brain_view_full_small_map() {
        // bytes: row0 heat 0..3 tier0; row1 tier1 heat 0..3 (64..67)
        let map = ExpertMap {
            rows: 2,
            cols: 4,
            cells: vec![0, 1, 2, 3, 64, 65, 66, 67],
        };
        let view = brain_view_from_map(&map, None, 0);
        assert!(!view.sampled);
        assert_eq!(view.disp_rows, 2);
        assert_eq!(view.disp_cols, 4);
        assert_eq!(view.row_stride, 1);
        assert_eq!(view.col_stride, 1);
        assert_eq!(view.cells.len(), 8);
        // Packed decode: (tier, heat, pulse)
        assert_eq!(view.cells[0], (0, 0, 0.0));
        assert_eq!(view.cells[3], (0, 3, 0.0));
        assert_eq!(view.cells[4], (1, 0, 0.0));
        assert_eq!(view.cells[7], (1, 3, 0.0));
    }

    #[test]
    fn brain_view_full_res_mode_no_sample_on_large_map() {
        let rows = 76u32;
        let cols = 256u32;
        let total = (rows * cols) as usize;
        let map = ExpertMap {
            rows,
            cols,
            cells: vec![0u8; total],
        };
        let view = brain_view_from_map_with_max(&map, None, 0, total);
        assert!(!view.sampled, "max_cells == total must not sample");
        assert_eq!(view.disp_rows, rows);
        assert_eq!(view.disp_cols, cols);
        assert_eq!(view.row_stride, 1);
        assert_eq!(view.col_stride, 1);
        assert_eq!(view.cells.len(), total);
        assert!(view.note.starts_with("full"), "{}", view.note);

        // usize::MAX same as full
        let full = brain_view_from_map_with_max(&map, None, 0, usize::MAX);
        assert!(!full.sampled);
        assert_eq!(full.cells.len(), total);
    }

    #[test]
    fn display_to_source_matches_brain_view_sampling() {
        let rows = 76u32;
        let cols = 256u32;
        // Distinct pack at known source cells so we can reverse-check.
        let mut cells = vec![0u8; (rows * cols) as usize];
        // Mark (0,0), and later fill after we know strides from a dry run.
        cells[0] = (2 << 6) | 40;
        let map = ExpertMap {
            rows,
            cols,
            cells: cells.clone(),
        };
        let view = brain_view_from_map(&map, None, 0);
        assert!(view.sampled);
        // Every display cell must map to the same source the sampler used.
        for dr in 0..view.disp_rows {
            for dc in 0..view.disp_cols {
                let (sr, sc) = display_to_source(dr, dc, view.row_stride, view.col_stride);
                assert_eq!(sr, dr * view.row_stride);
                assert_eq!(sc, dc * view.col_stride);
                assert!(sr < rows);
                assert!(sc < cols);
                let idx = (dr * view.disp_cols + dc) as usize;
                let expect_tier = map.tier_at(sr, sc).unwrap_or(0);
                let expect_heat = map.heat_at(sr, sc).unwrap_or(0);
                assert_eq!(view.cells[idx].0, expect_tier);
                assert_eq!(view.cells[idx].1, expect_heat);
            }
        }
        // Stride reverse for corner display cells
        let (sr, sc) = display_to_source(
            view.disp_rows - 1,
            view.disp_cols - 1,
            view.row_stride,
            view.col_stride,
        );
        assert!(sr < rows && sc < cols);
    }

    #[test]
    fn brain_view_hit_pulse_on_seq_change() {
        let map = ExpertMap {
            rows: 1,
            cols: 8,
            cells: vec![0; 8],
        };
        let hits = ExpertHits {
            rows: 1,
            cols: 8,
            bits: vec![0b0000_0001], // expert 0
            seq: 3,
        };
        let view = brain_view_from_map(&map, Some(&hits), 2);
        assert_eq!(view.hits_seq, 3);
        assert!((view.cells[0].2 - 1.0).abs() < f32::EPSILON);
        // same seq → no new pulse from map alone
        let view2 = brain_view_from_map(&map, Some(&hits), 3);
        assert!((view2.cells[0].2).abs() < f32::EPSILON);
    }

    #[test]
    fn brain_pulse_decay_math_matches_web_raf() {
        // One RAF step: 1.0 * 0.94
        let one = brain_pulse_after_decay(1.0, 1);
        assert!((one - 0.94).abs() < 1e-5, "one step {one}");
        // Two steps: 0.94^2
        let two = brain_pulse_after_decay(1.0, 2);
        assert!((two - 0.94 * 0.94).abs() < 1e-5, "two steps {two}");
        // Floor: many steps → 0
        let gone = brain_pulse_after_decay(1.0, 200);
        assert_eq!(gone, 0.0);
        // Already off stays off
        assert_eq!(brain_pulse_after_decay(0.0, 5), 0.0);
        // steps=0 preserves
        assert!((brain_pulse_after_decay(0.5, 0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn brain_pulse_decay_steps_for_ms_maps_pump_cadence() {
        assert_eq!(brain_pulse_decay_steps_for_ms(0), 0);
        assert_eq!(brain_pulse_decay_steps_for_ms(16), 1);
        // 500 ms visual pump ≈ 31 RAF frames at 16 ms.
        let steps = brain_pulse_decay_steps_for_ms(500);
        assert!((30..=32).contains(&steps), "500ms → {steps} steps");
    }

    #[test]
    fn apply_brain_pulse_decay_carries_and_preserves_fresh_hits() {
        let map = ExpertMap {
            rows: 1,
            cols: 4,
            cells: vec![0; 4],
        };
        let hits = ExpertHits {
            rows: 1,
            cols: 4,
            bits: vec![0b0000_0011], // experts 0 and 1
            seq: 2,
        };
        let lit = brain_view_from_map(&map, Some(&hits), 1);
        assert!((lit.cells[0].2 - 1.0).abs() < f32::EPSILON);
        assert!((lit.cells[1].2 - 1.0).abs() < f32::EPSILON);

        // Same seq rebuild → fresh map pulses are 0; decay carries previous.
        let mut next = brain_view_from_map(&map, Some(&hits), 2);
        apply_brain_pulse_decay(&mut next, &lit, 1);
        assert!(
            (next.cells[0].2 - 0.94).abs() < 1e-5,
            "cell0 decayed {}",
            next.cells[0].2
        );
        assert!(
            (next.cells[1].2 - 0.94).abs() < 1e-5,
            "cell1 decayed {}",
            next.cells[1].2
        );
        assert_eq!(next.cells[2].2, 0.0);

        // New seq with only expert 0 hit: 0 stays 1.0, 1 continues decaying.
        let hits2 = ExpertHits {
            rows: 1,
            cols: 4,
            bits: vec![0b0000_0001],
            seq: 3,
        };
        let mut fresh = brain_view_from_map(&map, Some(&hits2), 2);
        apply_brain_pulse_decay(&mut fresh, &next, 1);
        assert!(
            (fresh.cells[0].2 - 1.0).abs() < f32::EPSILON,
            "fresh hit stays 1"
        );
        assert!(
            (fresh.cells[1].2 - 0.94 * 0.94).abs() < 1e-5,
            "old hit keeps decaying {}",
            fresh.cells[1].2
        );

        // Dimension change: no carry.
        let mut wide = BrainView {
            disp_rows: 1,
            disp_cols: 8,
            cells: vec![(0, 0, 0.0); 8],
            ..BrainView::default()
        };
        apply_brain_pulse_decay(&mut wide, &lit, 1);
        assert!(wide.cells.iter().all(|c| c.2 == 0.0));
    }

    #[test]
    fn brain_cell_rgb_differs_by_tier() {
        use crate::theme::ThemeId;
        // Mint: soft bases differ even at heat 0 (lum floor).
        let disk = brain_cell_rgb(ThemeId::Mint, 0, 0, 0.0);
        let ram = brain_cell_rgb(ThemeId::Mint, 1, 0, 0.0);
        let vram = brain_cell_rgb(ThemeId::Mint, 2, 0, 0.0);
        assert_ne!(disk, ram);
        assert_ne!(ram, vram);
        let hot = brain_cell_rgb(ThemeId::Mint, 2, 63, 1.0);
        assert_ne!(vram, hot);
        // DOGE: tiers differ once heat is on (cold is pure black for all).
        let d = brain_cell_rgb(ThemeId::Doge, 0, 8, 0.0);
        let r = brain_cell_rgb(ThemeId::Doge, 1, 8, 0.0);
        let v = brain_cell_rgb(ThemeId::Doge, 2, 8, 0.0);
        assert_ne!(d, r);
        assert_ne!(r, v);
        assert_ne!(
            brain_cell_rgb(ThemeId::Doge, 2, 0, 0.0),
            brain_cell_rgb(ThemeId::Doge, 2, 24, 0.0)
        );
    }

    #[test]
    fn brain_cell_rgb_heat_saturates_at_24() {
        use crate::theme::ThemeId;
        // Web curve (mint): heat 24 → full tier brightness (lum=1.0); heat>24 same.
        let vram_24 = brain_cell_rgb(ThemeId::Mint, 2, 24, 0.0);
        let vram_63 = brain_cell_rgb(ThemeId::Mint, 2, 63, 0.0);
        assert_eq!(vram_24, vram_63, "heat≥24 must saturate");
        // Full VRAM base at heat=24, no pulse: (78, 214, 165)
        assert_eq!(vram_24, (78 << 16) | (214 << 8) | 165);
        // heat=12 is half-scale under /24 → brighter than old heat/63 mapping.
        let mid = brain_cell_rgb(ThemeId::Mint, 2, 12, 0.0);
        let r_mid = (mid >> 16) & 0xff;
        let r_full = (vram_24 >> 16) & 0xff;
        assert!(r_mid < r_full, "mid heat darker than full");
        // Under heat/63, heat=12 would be ~0.56 lum of base; under /24 ≈ 0.675.
        // Channel r ≈ 78 * 0.675 ≈ 52.6; under /63 ≈ 78 * (0.45+0.55*12/63) ≈ 43.
        assert!(r_mid >= 50, "heat=12 must follow /24 curve (got r={r_mid})");
        let cold = brain_cell_rgb(ThemeId::Mint, 2, 0, 0.0);
        let r_cold = (cold >> 16) & 0xff;
        // lum=0.35 at heat 0 → 78*0.35 ≈ 27
        assert_eq!(r_cold, (78.0 * 0.35) as u32);
    }

    #[test]
    fn doge_brain_cell_colors_are_pure_eight() {
        use crate::theme::{DOGE_EIGHT, ThemeId};
        for tier in 0..=3u8 {
            for &heat in &[0u8, 1, 8, 11, 12, 24, 63] {
                for &pulse in &[0.0f32, 0.04, 0.06, 0.5, 1.0] {
                    let c = brain_cell_rgb(ThemeId::Doge, tier, heat, pulse);
                    assert!(
                        DOGE_EIGHT.contains(&c),
                        "DOGE brain cell tier={tier} heat={heat} pulse={pulse} \
                         color 0x{c:06X} is not in the pure eight"
                    );
                }
            }
        }
    }

    #[test]
    fn status_after_gen_done_respects_stop_requested() {
        assert_eq!(status_after_gen_done(true, 10, 1.5), "stopped");
        let s = status_after_gen_done(false, 42, 3.5);
        assert!(s.contains("42"), "{s}");
        assert!(s.contains("3.50"), "{s}");
        assert!(s.starts_with("done"), "{s}");
    }

    #[test]
    fn stop_session_empty_slot_errors() {
        let slot: Arc<Mutex<Option<EngineSession>>> = Arc::new(Mutex::new(None));
        let err = stop_session(&slot).unwrap_err();
        assert!(err.contains("no engine session"), "{err}");
    }

    #[test]
    fn pump_session_visual_none_when_slot_empty() {
        let slot: Arc<Mutex<Option<EngineSession>>> = Arc::new(Mutex::new(None));
        assert!(pump_session_visual(&slot).is_none());
    }

    #[test]
    fn pump_visual_try_lock_returns_last_snapshot_when_mutex_held() {
        let engine = Arc::new(Mutex::new(0u32));
        let last = VisualSnapshot {
            hits_seq: 42,
            ..VisualSnapshot::default()
        };
        let held = Arc::clone(&engine);
        let (ready_tx, ready_rx) = mpsc::channel();
        let join = thread::spawn(move || {
            let _g = held.lock().unwrap();
            ready_tx.send(()).unwrap();
            thread::sleep(Duration::from_millis(400));
        });
        ready_rx.recv().unwrap();
        let start = Instant::now();
        let out = pump_visual_try_lock(&engine, last, |_| {
            panic!("poll must not run while generate holds the engine mutex");
        });
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(80),
            "visual pump must try_lock and return immediately, took {elapsed:?}"
        );
        assert_eq!(out.hits_seq, 42, "miss must keep the last snapshot");
        join.join().unwrap();
    }

    #[test]
    fn pump_visual_try_lock_polls_when_mutex_free() {
        let engine = Mutex::new(7u32);
        let last = VisualSnapshot {
            hits_seq: 1,
            ..VisualSnapshot::default()
        };
        let out = pump_visual_try_lock(&engine, last, |n| {
            assert_eq!(*n, 7);
            VisualSnapshot {
                hits_seq: 99,
                ..VisualSnapshot::default()
            }
        });
        assert_eq!(out.hits_seq, 99);
    }

    #[cfg(feature = "ffi")]
    #[test]
    fn request_ffi_generate_cancel_does_not_wait_on_engine_mutex() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let engine = Arc::new(Mutex::new(()));
        let cancel = AtomicBool::new(false);
        let held = Arc::clone(&engine);
        let (ready_tx, ready_rx) = mpsc::channel();
        let join = thread::spawn(move || {
            let _g = held.lock().unwrap();
            ready_tx.send(()).unwrap();
            thread::sleep(Duration::from_millis(400));
        });
        ready_rx.recv().unwrap();
        let start = Instant::now();
        request_ffi_generate_cancel(&cancel);
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(80),
            "Stop must set cancel without taking the engine mutex, took {elapsed:?}"
        );
        assert!(cancel.load(Ordering::SeqCst));
        assert!(
            engine.try_lock().is_err(),
            "generate must still hold the mutex"
        );
        join.join().unwrap();
    }

    #[test]
    fn pump_session_visual_does_not_block_when_session_mutex_held() {
        let slot: Arc<Mutex<Option<EngineSession>>> = Arc::new(Mutex::new(None));
        let held = Arc::clone(&slot);
        let (ready_tx, ready_rx) = mpsc::channel();
        let join = thread::spawn(move || {
            let _g = held.lock().unwrap();
            ready_tx.send(()).unwrap();
            thread::sleep(Duration::from_millis(400));
        });
        ready_rx.recv().unwrap();
        let start = Instant::now();
        let out = pump_session_visual(&slot);
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(80),
            "session visual pump must not wait on a held mutex, took {elapsed:?}"
        );
        assert!(out.is_none());
        join.join().unwrap();
    }

    #[test]
    fn should_dispatch_engine_start_blocks_generating_and_already_starting() {
        assert_eq!(should_dispatch_engine_start(false, false), Ok(()));
        assert_eq!(
            should_dispatch_engine_start(true, false),
            Err(StartEngineBlock::Generating)
        );
        assert_eq!(
            should_dispatch_engine_start(false, true),
            Err(StartEngineBlock::AlreadyStarting)
        );
        assert_eq!(
            should_dispatch_engine_start(true, true),
            Err(StartEngineBlock::Generating)
        );
    }

    #[test]
    fn engine_starting_status_includes_elapsed_seconds() {
        let zero = engine_starting_status(Duration::from_millis(20));
        assert!(zero.contains("Starting engine"), "{zero}");
        let later = engine_starting_status(Duration::from_secs(12));
        assert!(
            later.contains("12"),
            "living start line must show elapsed seconds: {later}"
        );
        assert!(later.to_ascii_lowercase().contains("start"), "{later}");
        assert!(
            !later.contains('%'),
            "do not invent a determinate percent for start: {later}"
        );
    }

    #[test]
    fn start_async_preflight_returns_without_loading_a_model() {
        let (tx, rx) = mpsc::channel();
        EngineSession::start_async(PathBuf::new(), None, tx);
        let result = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("start_async must send a result");
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("empty path is not a model"),
        };
        assert!(
            err.contains("not a model") || err.contains("Install a model"),
            "{err}"
        );
    }

    #[test]
    fn dispatch_blocking_start_does_not_run_on_caller_thread() {
        let caller = thread::current().id();
        let (started_tx, started_rx) = mpsc::channel();
        let rx = dispatch_blocking_start(move || {
            let worker = thread::current().id();
            let _ = started_tx.send(worker);
            thread::sleep(Duration::from_millis(40));
            worker
        });
        let worker_id = started_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("worker should start");
        assert_ne!(
            worker_id, caller,
            "engine start work must not run on the caller (UI) thread"
        );
        let result = rx.recv_timeout(Duration::from_secs(2)).expect("join");
        assert_eq!(result, worker_id);
        assert_ne!(result, caller);
    }

    #[cfg(unix)]
    #[test]
    fn dispatch_blocking_start_nices_worker_not_caller() {
        let caller = colibri_sys::process_priority::get_process_nice(0).expect("caller nice");
        let rx = dispatch_blocking_start(|| {
            colibri_sys::process_priority::get_process_nice(0).expect("worker nice")
        });
        let worker = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("worker result");
        assert_eq!(
            worker, ENGINE_CHILD_NICE,
            "engine-start worker must be niced"
        );
        let after = colibri_sys::process_priority::get_process_nice(0).expect("caller after");
        assert_eq!(
            caller, after,
            "dispatch_blocking_start must not nice the caller (GPUI)"
        );
    }

    #[test]
    fn generate_async_errors_when_no_session() {
        let slot: Arc<Mutex<Option<EngineSession>>> = Arc::new(Mutex::new(None));
        let (tx, rx) = mpsc::channel();
        let controls = GenerateControls {
            max_tokens: 16,
            ..Default::default()
        };
        EngineSession::generate_async(slot, vec![ChatMessage::user("hi")], controls, tx);
        let ev = rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        match ev {
            GenEvent::Error(msg) => assert!(msg.contains("no engine"), "{msg}"),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn clamp_temperature_bounds() {
        assert_eq!(clamp_temperature(0.0), 0.0);
        assert_eq!(clamp_temperature(2.0), 2.0);
        assert_eq!(clamp_temperature(-1.0), 0.0);
        assert_eq!(clamp_temperature(3.5), 2.0);
        assert_eq!(
            clamp_temperature(f32::NAN),
            GenerateControls::default().temperature
        );
    }

    #[test]
    fn clamp_max_tokens_bounds() {
        assert_eq!(clamp_max_tokens(0), 1);
        assert_eq!(clamp_max_tokens(1), 1);
        assert_eq!(clamp_max_tokens(32768), 32768);
        assert_eq!(clamp_max_tokens(999_999), 32768);
    }

    #[test]
    fn clamp_cache_slot_respects_kv_slots() {
        assert_eq!(clamp_cache_slot(0, 1), 0);
        assert_eq!(clamp_cache_slot(5, 1), 0);
        assert_eq!(clamp_cache_slot(3, 4), 3);
        assert_eq!(clamp_cache_slot(9, 4), 3);
        assert_eq!(clamp_cache_slot(0, 0), 0); // treated as 1 slot
    }

    #[test]
    fn parse_temperature_and_max_tokens() {
        assert!((parse_temperature("0.8").unwrap() - 0.8).abs() < 1e-6);
        assert!((parse_temperature("").unwrap() - 0.7).abs() < 1e-6);
        assert!((parse_temperature("9").unwrap() - 2.0).abs() < 1e-6);
        assert!(parse_temperature("x").is_err());
        assert_eq!(parse_max_tokens("256").unwrap(), 256);
        assert_eq!(parse_max_tokens("").unwrap(), 4096);
        assert_eq!(parse_max_tokens("0").unwrap(), 1);
        assert!(parse_max_tokens("nope").is_err());
    }

    #[test]
    fn parse_grammar_field_empty_is_none() {
        assert_eq!(parse_grammar_field(""), None);
        assert_eq!(parse_grammar_field("  "), None);
        assert_eq!(
            parse_grammar_field("root ::= \"a\"").as_deref(),
            Some("root ::= \"a\"")
        );
    }

    #[test]
    fn generate_controls_clamped_clears_blank_grammar() {
        let c = GenerateControls {
            temperature: 9.0,
            max_tokens: 0,
            enable_thinking: true,
            cache_slot: 99,
            grammar: Some("  ".into()),
            top_p: 1.5,
        }
        .clamped(4);
        assert!((c.temperature - 2.0).abs() < 1e-6);
        assert_eq!(c.max_tokens, 1);
        assert_eq!(c.cache_slot, 3);
        assert!(c.enable_thinking);
        assert_eq!(c.grammar, None);
        assert!((c.top_p - 1.0).abs() < 1e-6);
    }

    #[test]
    fn env_flag_truthy_matrix() {
        assert!(!env_flag_truthy(None::<&str>));
        assert!(!env_flag_truthy(Some("")));
        assert!(!env_flag_truthy(Some("0")));
        assert!(!env_flag_truthy(Some("false")));
        assert!(!env_flag_truthy(Some("NO")));
        assert!(!env_flag_truthy(Some("off")));
        assert!(env_flag_truthy(Some("1")));
        assert!(env_flag_truthy(Some("true")));
        assert!(env_flag_truthy(Some("yes")));
    }

    #[test]
    fn resolve_prefer_process_from_flags_force_always_process() {
        // COLIBRI_FORCE_PROCESS always wins, with or without feature=ffi.
        assert!(resolve_prefer_process_from_flags(true, false));
        assert!(resolve_prefer_process_from_flags(true, true));
    }

    #[test]
    fn resolve_prefer_process_from_flags_default_by_feature() {
        // Neither force nor prefer-FFI: feature=ffi → try FFI; else process.
        #[cfg(feature = "ffi")]
        {
            assert!(
                !resolve_prefer_process_from_flags(false, false),
                "native host with feature=ffi defaults to prefer_process=false (try FFI)"
            );
            // COLIBRI_PREFER_FFI is redundant under feature=ffi (still FFI-first).
            assert!(!resolve_prefer_process_from_flags(false, true));
        }
        #[cfg(not(feature = "ffi"))]
        {
            assert!(
                resolve_prefer_process_from_flags(false, false),
                "without feature=ffi, default remains process"
            );
            // Explicit prefer-FFI still sets prefer_process=false (no link yet).
            assert!(!resolve_prefer_process_from_flags(false, true));
        }
    }

    #[test]
    fn resolve_prefer_process_respects_force_over_prefer_ffi() {
        // Config + should_try_ffi_open composition (no process-env mutation).
        let process_default = ColibriConfig::default().prefer_process(true);
        assert!(process_default.must_use_process() || force_process_from_env());
        assert!(!should_try_ffi_open(&process_default, ModelFamily::Glm));

        let want_ffi = ColibriConfig::default().prefer_process(false);
        if force_process_from_env() {
            assert!(!want_ffi.prefer_ffi_path());
            assert!(!should_try_ffi_open(&want_ffi, ModelFamily::Glm));
        } else {
            #[cfg(feature = "ffi")]
            {
                assert!(want_ffi.prefer_ffi_path());
                assert!(should_try_ffi_open(&want_ffi, ModelFamily::Glm));
                assert!(should_try_ffi_open(&want_ffi, ModelFamily::Olmoe));
                assert!(should_try_ffi_open(&want_ffi, ModelFamily::Kimi));
                assert!(should_try_ffi_open(&want_ffi, ModelFamily::Inkling));
                assert!(should_try_ffi_open(&want_ffi, ModelFamily::DeepseekV4));
            }
            #[cfg(not(feature = "ffi"))]
            {
                // No static link → prefer_ffi_path stays false (must_use process).
                assert!(!want_ffi.prefer_ffi_path());
                assert!(!should_try_ffi_open(&want_ffi, ModelFamily::Glm));
            }
        }
    }

    #[test]
    fn host_start_config_matches_resolve_prefer_process_from_flags() {
        // Native start applies resolve_prefer_process(); mirror that onto config
        // the same way EngineSession::start does.
        let prefer_process = resolve_prefer_process();
        let cfg = ColibriConfig::default().prefer_process(prefer_process);
        if prefer_process || force_process_from_env() {
            assert!(cfg.must_use_process() || !cfg.prefer_ffi_path());
            assert!(!should_try_ffi_open(&cfg, ModelFamily::Glm));
        } else {
            #[cfg(feature = "ffi")]
            {
                assert!(cfg.prefer_ffi_path());
                assert!(should_try_ffi_open(&cfg, ModelFamily::Glm));
            }
            #[cfg(not(feature = "ffi"))]
            {
                assert!(!cfg.prefer_ffi_path());
                assert!(!should_try_ffi_open(&cfg, ModelFamily::Glm));
            }
        }
    }

    #[test]
    fn engine_path_kind_labels_are_plain() {
        assert_eq!(EnginePathKind::Process.as_str(), "engine process");
        assert_eq!(EnginePathKind::Ffi.as_str(), "in-process FFI");
        assert!(!EnginePathKind::Process.as_str().contains("mux"));
        let ffi_line = engine_path_status_line(EnginePathKind::Ffi);
        assert!(
            ffi_line.contains("live visual poll") || ffi_line.contains("in-process FFI"),
            "{ffi_line}"
        );
        assert!(
            !ffi_line.contains("Brain needs engine process"),
            "status should not claim Brain is process-only after visual poll ABI: {ffi_line}"
        );
        assert_eq!(FORCE_PROCESS_ENV, "COLIBRI_FORCE_PROCESS");
        assert_eq!(PREFER_FFI_ENV, "COLIBRI_PREFER_FFI");
    }

    #[test]
    fn resolve_prefer_process_env_matches_flags() {
        // Live env must agree with the pure helper (no mutation of process env).
        assert_eq!(
            resolve_prefer_process(),
            resolve_prefer_process_from_flags(env_force_process_path(), env_prefer_ffi())
        );
        if env_force_process_path() {
            assert!(resolve_prefer_process());
            return;
        }
        #[cfg(feature = "ffi")]
        {
            // Default under feature=ffi: prefer_process false (try FFI).
            assert!(!resolve_prefer_process());
        }
        #[cfg(not(feature = "ffi"))]
        {
            if env_prefer_ffi() {
                assert!(!resolve_prefer_process());
            } else {
                assert!(resolve_prefer_process());
            }
        }
    }

    #[test]
    fn controls_from_ui_builds_clamped() {
        let c = controls_from_ui("1.2", "128", true, 2, "root ::= \"x\"", 4).unwrap();
        assert!((c.temperature - 1.2).abs() < 1e-6);
        assert_eq!(c.max_tokens, 128);
        assert!(c.enable_thinking);
        assert_eq!(c.cache_slot, 2);
        assert_eq!(c.grammar.as_deref(), Some("root ::= \"x\""));
    }

    #[test]
    fn switch_cache_slot_transcript_is_sticky() {
        use std::collections::HashMap;
        let mut by_slot: HashMap<u32, Vec<(String, String)>> = HashMap::new();
        let log0 = vec![
            ("user".into(), "hello slot0".into()),
            ("assistant".into(), "hi0".into()),
        ];
        let (active, log1) = switch_cache_slot_transcript(&mut by_slot, 0, 1, log0.clone());
        assert_eq!(active, 1);
        assert!(log1.is_empty(), "fresh slot starts empty");
        assert_eq!(by_slot.get(&0).unwrap(), &log0);

        let log1_filled = vec![("user".into(), "on slot1".into())];
        let (active, restored) =
            switch_cache_slot_transcript(&mut by_slot, 1, 0, log1_filled.clone());
        assert_eq!(active, 0);
        assert_eq!(restored, log0);
        assert_eq!(by_slot.get(&1).unwrap(), &log1_filled);
    }

    #[test]
    fn clamp_kv_slots_bounds() {
        assert_eq!(clamp_kv_slots(0), 1);
        assert_eq!(clamp_kv_slots(1), 1);
        assert_eq!(clamp_kv_slots(16), 16);
        assert_eq!(clamp_kv_slots(99), 16);
    }

    #[test]
    fn req_book_allocates_and_blocks_overlapping() {
        let mut book = ReqBook::default();
        assert_eq!(book.begin().unwrap(), 1);
        assert_eq!(book.active_req, Some(1));
        let err = book.begin().unwrap_err();
        assert!(err.contains("already generating"), "{err}");
        book.clear_matching(1);
        assert_eq!(book.active_req, None);
        assert_eq!(book.begin().unwrap(), 2);
    }

    #[test]
    fn req_book_clear_only_matching_id() {
        let mut book = ReqBook::default();
        let id = book.begin().unwrap();
        assert_eq!(id, 1);
        book.clear_matching(99);
        assert_eq!(book.active_req, Some(1));
        book.clear_matching(1);
        assert_eq!(book.active_req, None);
    }

    #[test]
    fn catalog_selection_maps_installable_glm() {
        let sel = catalog_selection_by_id("glm-5.2-colibri").expect("glm in catalog");
        assert!(sel.installable);
        assert_eq!(
            sel.repo_id.as_deref(),
            Some("mastouri/GLM-5.2-colibri-int4-g64-with-int8-mtp")
        );
        assert_eq!(
            sel.dest.as_deref(),
            Some("GLM-5.2-colibri-int4-g64-with-int8-mtp")
        );
        assert!(
            sel.status.contains("Ready to install") && sel.status.contains("GLM-5.2"),
            "{}",
            sel.status
        );
    }

    #[test]
    fn catalog_selection_convert_only_olmoe() {
        let sel = catalog_selection_by_id("olmoe-colibri").expect("olmoe");
        assert!(!sel.installable);
        assert!(sel.repo_id.is_none());
        assert!(sel.dest.is_none());
        assert!(sel.status.contains("convert"), "{}", sel.status);
    }

    #[test]
    fn catalog_selection_unknown_id() {
        assert!(catalog_selection_by_id("no-such-model").is_none());
    }

    #[test]
    fn format_supported_model_row_includes_name() {
        let models = list_supported_models();
        assert!(!models.is_empty());
        let row = format_supported_model_row(&models[0]);
        assert!(row.contains(models[0].display_name), "{row}");
    }

    fn test_entry(path: &str, status: ModelStatus, family: ModelFamily) -> ModelEntry {
        ModelEntry {
            path: PathBuf::from(path),
            family,
            engine_id: "test".into(),
            status,
            model_bytes: 1,
            disk_bytes: 1,
            param_count: None,
            shards: 1,
            model_type: None,
            note: None,
        }
    }

    #[test]
    fn catalog_is_installed_matches_leaf_name_deepseek() {
        let m = supported_model_by_id("deepseek-v4-colibri").expect("deepseek in catalog");
        let entries = [test_entry(
            "/home/user/.local/share/colibri/models/DeepSeek-V4-Flash-0731",
            ModelStatus::Present,
            ModelFamily::DeepseekV4,
        )];
        let hit = catalog_is_installed(m, &entries).expect("leaf name should match");
        assert_eq!(
            hit.path.file_name().and_then(|s| s.to_str()),
            Some("DeepSeek-V4-Flash-0731")
        );
    }

    #[test]
    fn catalog_is_installed_matches_owner_double_underscore_name() {
        let m = supported_model_by_id("deepseek-v4-colibri").expect("deepseek");
        let entries = [test_entry(
            "/store/deepseek-ai__DeepSeek-V4-Flash-0731",
            ModelStatus::Present,
            ModelFamily::DeepseekV4,
        )];
        assert!(catalog_is_installed(m, &entries).is_some());
    }

    #[test]
    fn catalog_is_installed_matches_nested_owner_name() {
        let m = supported_model_by_id("deepseek-v4-colibri").expect("deepseek");
        let entries = [test_entry(
            "/store/deepseek-ai/DeepSeek-V4-Flash-0731",
            ModelStatus::Present,
            ModelFamily::DeepseekV4,
        )];
        assert!(catalog_is_installed(m, &entries).is_some());
    }

    #[test]
    fn catalog_is_installed_rejects_unrelated_folder() {
        let m = supported_model_by_id("deepseek-v4-colibri").expect("deepseek");
        let entries = [test_entry(
            "/store/some-other-model",
            ModelStatus::Present,
            ModelFamily::Glm,
        )];
        assert!(catalog_is_installed(m, &entries).is_none());
    }

    #[test]
    fn catalog_is_installed_requires_present_status() {
        let m = supported_model_by_id("deepseek-v4-colibri").expect("deepseek");
        let entries = [test_entry(
            "/store/DeepSeek-V4-Flash-0731",
            ModelStatus::Incomplete,
            ModelFamily::DeepseekV4,
        )];
        assert!(
            catalog_is_installed(m, &entries).is_none(),
            "Incomplete must not show Installed badge"
        );
    }

    #[test]
    fn catalog_is_installed_convert_only_never_matches() {
        let m = supported_model_by_id("olmoe-colibri").expect("olmoe");
        let entries = [test_entry(
            "/store/olmoe-something",
            ModelStatus::Present,
            ModelFamily::Olmoe,
        )];
        assert!(catalog_is_installed(m, &entries).is_none());
    }

    #[test]
    fn catalog_row_style_installed_unselected_is_solid_white() {
        let style = catalog_row_style(
            true, false, 0x00_FF_00, // primary green
            0x00_00_00, // primary_fg
            0x00_00_00, // secondary
            0xFF_FF_FF, // text
            0xFF_FF_FF, // border
        );
        assert_eq!(style.fill, crate::theme::DOGE_WHITE);
        assert_eq!(style.fg, crate::theme::DOGE_BLACK);
        assert!(style.show_installed);
    }

    #[test]
    fn catalog_row_style_selected_keeps_primary_and_badge() {
        let primary = 0x00_FF_00;
        let primary_fg = 0x00_00_00;
        let style = catalog_row_style(
            true, true, primary, primary_fg, 0x00_00_00, 0xFF_FF_FF, 0xFF_FF_FF,
        );
        assert_eq!(style.fill, primary);
        assert_eq!(style.fg, primary_fg);
        assert!(style.show_installed, "selected installed still shows badge");
    }

    #[test]
    fn catalog_row_style_not_installed_uses_secondary() {
        let secondary = 0x11_22_33;
        let text = 0xAA_BB_CC;
        let border = 0xDD_EE_FF;
        let style = catalog_row_style(false, false, 0, 0, secondary, text, border);
        assert_eq!(style.fill, secondary);
        assert_eq!(style.fg, text);
        assert_eq!(style.border, border);
        assert!(!style.show_installed);
    }

    #[test]
    fn catalog_installable_fields_pass_validate_when_install() {
        #[cfg(feature = "install")]
        {
            let store = Path::new("/tmp/store");
            for m in list_supported_models() {
                let sel = catalog_selection_from_model(m);
                if !sel.installable {
                    continue;
                }
                let repo = sel.repo_id.as_deref().unwrap();
                let dest = sel.dest.as_deref().unwrap_or("");
                assert!(
                    validate_install_form(repo, "", dest, Some(store)).is_ok(),
                    "catalog {} form invalid",
                    m.id
                );
            }
        }
    }

    #[cfg(feature = "install")]
    #[test]
    fn validate_install_rejects_bad_repo() {
        let store = Path::new("/tmp/store");
        assert!(validate_install_form("", "", "", Some(store)).is_err());
        assert!(validate_install_form("nslash", "", "", Some(store)).is_err());
        assert!(validate_install_form("../evil/x", "", "", Some(store)).is_err());
        assert!(validate_install_form("a/b/c", "", "", Some(store)).is_err());
        assert!(validate_install_form("org/mod", "..", "", Some(store)).is_err());
        assert!(validate_install_form("org/mod name", "", "", Some(store)).is_err());
        assert!(validate_install_form("/org/mod", "", "", Some(store)).is_err());
        assert!(validate_install_form("org/mod/", "", "", Some(store)).is_err());
        assert!(validate_install_form("org/mod", "refs/heads/main", "", Some(store)).is_err());
        assert!(validate_install_form("org\\mod", "", "", Some(store)).is_err());
        // Uppercase owner/name is allowed (ASCII alphanumeric).
        assert!(validate_install_form("ORG/Mod", "", "", Some(store)).is_ok());
    }

    #[cfg(feature = "install")]
    #[test]
    fn validate_install_accepts_owner_name() {
        let (repo, rev, dest) =
            validate_install_form("org/my-model", "main", "", Some(Path::new("/tmp/store")))
                .unwrap();
        assert_eq!(repo, "org/my-model");
        assert_eq!(rev.as_deref(), Some("main"));
        assert_eq!(dest, PathBuf::from("/tmp/store/org__my-model"));

        let (_, rev2, dest2) = validate_install_form(
            "org/my-model",
            "",
            "custom-dir",
            Some(Path::new("/tmp/store")),
        )
        .unwrap();
        assert!(rev2.is_none());
        assert_eq!(dest2, PathBuf::from("/tmp/store/custom-dir"));
    }

    #[cfg(feature = "install")]
    #[test]
    fn validate_install_rejects_dest_escape() {
        let store = Path::new("/tmp/store");
        assert!(
            validate_install_form("org/mod", "", "../escape", Some(store)).is_err(),
            "relative .. must fail"
        );
        assert!(
            validate_install_form("org/mod", "", "/tmp/elsewhere/mod", Some(store)).is_err(),
            "absolute outside store must fail"
        );
        assert!(
            validate_install_form("org/mod", "", "/tmp/store/../escape", Some(store)).is_err(),
            "absolute with .. must fail"
        );
    }

    #[cfg(feature = "install")]
    #[test]
    fn validate_install_accepts_absolute_under_store() {
        let store = Path::new("/tmp/store");
        let (_, _, dest) =
            validate_install_form("org/mod", "", "/tmp/store/custom-abs", Some(store)).unwrap();
        assert_eq!(dest, PathBuf::from("/tmp/store/custom-abs"));
    }

    #[cfg(feature = "install")]
    #[test]
    fn format_install_space_includes_dest_and_gb() {
        // GB is 1e9 decimal (probe unit).
        let s = format_install_space_with_min(
            Path::new("/tmp/store/mod"),
            8 * GB,
            DEFAULT_INSTALL_MIN_FREE_BYTES,
        );
        assert!(s.contains("/tmp/store/mod"), "{s}");
        assert!(s.contains("GB"), "{s}");
        assert!(s.contains("8.00"), "{s}");
        assert!(s.contains("min"), "{s}");
    }

    #[cfg(feature = "install")]
    #[test]
    fn install_options_prefer_hub_for_progress() {
        let opts = install_options_for_ui(PathBuf::from("/tmp/store/m"), 0);
        assert!(
            !opts.prefer_cli,
            "UI install prefers hub so download emits file/byte counters"
        );
        assert!(opts.inspect_after);
        assert!(!opts.register);
        assert_eq!(opts.min_free_bytes, 0);
        assert_eq!(opts.dest, PathBuf::from("/tmp/store/m"));
    }

    #[cfg(feature = "install")]
    #[test]
    fn parse_min_free_gb_default_and_zero() {
        assert_eq!(
            parse_min_free_gb("").unwrap(),
            DEFAULT_INSTALL_MIN_FREE_BYTES
        );
        assert_eq!(parse_min_free_gb("0").unwrap(), 0);
        assert_eq!(parse_min_free_gb("2").unwrap(), 2 * GB);
        assert!(parse_min_free_gb("nope").is_err());
    }

    #[cfg(feature = "install")]
    #[test]
    fn check_install_free_space_gate() {
        let dir = tempfile::tempdir().unwrap();
        // min 0 always ok
        assert!(check_install_free_space(dir.path(), 0).is_ok());
        // absurd threshold fails
        let err = check_install_free_space(dir.path(), u64::MAX / 4).unwrap_err();
        assert!(err.contains("not enough free space"), "{err}");
    }

    #[test]
    fn progress_rate_basic() {
        assert_eq!(progress_rate(0, 10.0), 0.0);
        assert_eq!(progress_rate(100, 0.0), 0.0);
        assert_eq!(progress_rate(100, -1.0), 0.0);
        assert!((progress_rate(100, 10.0) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn format_install_bytes_pair_scales() {
        assert_eq!(format_install_bytes_pair(100, 200), "100/200 B");
        // Under 1 MiB total → KiB scale.
        assert_eq!(
            format_install_bytes_pair(512 * 1024, 800 * 1024),
            "512/800 KiB"
        );
        // Crosses MiB threshold → MiB scale.
        let s_mib = format_install_bytes_pair(512 * 1024, 2 * 1024 * 1024);
        assert!(s_mib.contains("MiB"), "{s_mib}");
        let gib = 1024u64 * 1024 * 1024;
        let s = format_install_bytes_pair(gib / 2, 10 * gib);
        assert!(s.contains("GiB"), "{s}");
        assert!(s.contains('/'), "{s}");
    }

    #[cfg(feature = "install")]
    #[test]
    fn progress_view_zero_done_no_absurd_eta() {
        // Honesty: file-boundary event before mid-file bytes → omit % and ETA.
        let p = InstallProgress {
            phase: "download".into(),
            message: "get out-00000.safetensors".into(),
            bytes_done: Some(0),
            bytes_total: Some(372 * 1024 * 1024 * 1024),
            file: Some("out-00000.safetensors".into()),
            files_done: Some(0),
            files_total: Some(80),
        };
        let v = progress_view_for_install(&p, 120.0);
        assert_eq!(v.percent, None, "0 done must not claim 0%");
        assert_eq!(v.eta_secs, None, "must not show multi-day ETA at 0 bytes");
        assert_eq!(v.line(), "Downloading...");
        assert!(!v.line().contains('%'), "{}", v.line());
        assert!(!v.line().contains("Calculating"), "{}", v.line());
        assert!(!v.line().contains("hours left"), "{}", v.line());
        assert!(!v.line().contains("about "), "{}", v.line());
        assert_eq!(v.fill_fraction(), 0.0);
    }

    #[cfg(feature = "install")]
    #[test]
    fn progress_view_mid_file_partial_advances() {
        let p = InstallProgress {
            phase: "download".into(),
            message: "get shard".into(),
            bytes_done: Some(50 * 1024 * 1024), // 50 MiB into first file
            bytes_total: Some(500 * 1024 * 1024),
            file: Some("out-00000.safetensors".into()),
            files_done: Some(0),
            files_total: Some(4),
        };
        // 50 MiB in 10s → 5 MiB/s; remaining 450 MiB → 90s
        let v = progress_view_for_install(&p, 10.0);
        assert_eq!(v.percent, Some(10));
        assert!(v.eta_secs.is_some(), "line={}", v.line());
        assert!(v.line().contains("10%"), "line={}", v.line());
        assert!(v.line().contains("about "), "line={}", v.line());
    }

    #[test]
    fn progress_view_for_generate_midway() {
        let v = progress_view_for_generate(50, 100, 10.0);
        assert_eq!(v.percent, Some(50));
        assert_eq!(v.eta_secs, Some(5));
        assert!(v.line().starts_with("Generating... 50%"));
    }

    #[test]
    fn progress_view_generate_done_is_full() {
        let v = progress_view_generate_done();
        assert_eq!(v.percent, Some(100));
        assert_eq!(v.eta_secs, Some(0));
    }

    #[cfg(feature = "install")]
    #[test]
    fn install_phase_labels_are_plain_english() {
        assert_eq!(install_phase_label("download"), "Downloading...");
        assert_eq!(install_phase_label("inspect"), "Checking files...");
        assert_eq!(install_phase_label("register"), "Registering...");
        assert_eq!(install_phase_label("done"), "Done");
        assert_eq!(install_phase_label("mystery"), "Working...");
    }

    #[cfg(feature = "install")]
    #[test]
    fn progress_view_for_install_bytes_and_done() {
        let mid = InstallProgress {
            phase: "download".into(),
            message: "get shard".into(),
            bytes_done: Some(25),
            bytes_total: Some(100),
            file: Some("a.safetensors".into()),
            files_done: Some(0),
            files_total: Some(4),
        };
        // 25 B in 5s → 5 B/s; remaining 75 → eta 15s
        let v = progress_view_for_install(&mid, 5.0);
        assert_eq!(v.percent, Some(25));
        assert_eq!(v.eta_secs, Some(15));
        assert!(v.line().starts_with("Downloading... 25%"), "{}", v.line());

        let done = InstallProgress {
            phase: "done".into(),
            message: "ok".into(),
            bytes_done: None,
            bytes_total: None,
            file: None,
            files_done: None,
            files_total: None,
        };
        let v = progress_view_for_install(&done, 99.0);
        assert_eq!(v.percent, Some(100));
        assert_eq!(v.label, "Done");
    }

    #[cfg(feature = "install")]
    #[test]
    fn progress_view_for_install_falls_back_to_files() {
        let p = InstallProgress {
            phase: "download".into(),
            message: "get".into(),
            bytes_done: None,
            bytes_total: None,
            file: Some("x".into()),
            files_done: Some(2),
            files_total: Some(8),
        };
        // 2 files in 2s → 1 file/s; remaining 6 → eta 6
        let v = progress_view_for_install(&p, 2.0);
        assert_eq!(v.percent, Some(25));
        assert_eq!(v.eta_secs, Some(6));
    }

    #[cfg(feature = "install")]
    #[test]
    fn progress_view_for_install_cli_no_counters_omits_percent() {
        // Option honesty: no fake phase floor (was 5%). Label only.
        let p = InstallProgress {
            phase: "download".into(),
            message: "fetching".into(),
            bytes_done: None,
            bytes_total: None,
            file: None,
            files_done: None,
            files_total: None,
        };
        let v = progress_view_for_install(&p, 10.0);
        assert_eq!(v.percent, None, "CLI/no-counter must not invent a percent");
        assert_eq!(v.eta_secs, None);
        assert_eq!(v.line(), "Downloading...");
        assert!(!v.line().contains('%'), "{}", v.line());
        assert!(!v.line().contains("about "), "{}", v.line());
        assert_eq!(v.fill_fraction(), 0.0);
        assert_eq!(install_phase_percent_floor("download"), None);
    }

    #[cfg(feature = "install")]
    #[test]
    fn progress_view_inspect_register_stay_high() {
        let inspect = InstallProgress {
            phase: "inspect".into(),
            message: "check".into(),
            bytes_done: None,
            bytes_total: None,
            file: None,
            files_done: None,
            files_total: None,
        };
        let v = progress_view_for_install(&inspect, 1.0);
        assert!(
            v.percent.unwrap_or(0) >= 90,
            "inspect should stay high, got {:?}",
            v.percent
        );

        let reg = InstallProgress {
            phase: "register".into(),
            message: "reg".into(),
            bytes_done: None,
            bytes_total: None,
            file: None,
            files_done: None,
            files_total: None,
        };
        let v = progress_view_for_install(&reg, 1.0);
        assert!(
            v.percent.unwrap_or(0) >= 90,
            "register should stay high, got {:?}",
            v.percent
        );
    }

    fn stub_moe_info(expert_bytes: u64) -> colibri_sys::ModelInfo {
        use colibri_sys::ModelInfo;
        ModelInfo {
            path: Path::new("/tmp/fake-uma-model").to_path_buf(),
            family: Some(ModelFamily::Glm),
            engine_id: "colibri".into(),
            model_type: Some("glm_moe_dsa".into()),
            shards: 1,
            model_bytes: 10 * GB,
            disk_bytes: 10 * GB,
            param_count: Some(1_000_000),
            dense_bytes: 2 * GB,
            expert_bytes,
            expert_count: 100,
            expert_layers: 10,
            typical_expert_bytes: 100_000_000,
            per_cap_bytes: 600_000_000,
            has_config: true,
            has_tokenizer: true,
            config: serde_json::json!({
                "num_hidden_layers": 10,
                "n_routed_experts": 64,
                "kv_lora_rank": 32,
                "qk_rope_head_dim": 8,
                "num_attention_heads": 4,
                "qk_nope_head_dim": 24,
                "v_head_dim": 32,
                "num_parameters": 1_000_000,
            }),
            shard_names: vec!["model.safetensors".into()],
        }
    }

    /// Operator screenshot contract: Ryzen APU / UMA Memory plan must not
    /// prefix BIOS carve-out-busy as `Warning:`. Unified budget may appear
    /// as information. Discrete VRAM-busy stays a Warning.
    #[test]
    fn uma_memory_plan_ui_does_not_warn_carveout_busy() {
        // SAFETY: test-only env; this crate's plan tests do not race COLI_GPU_MEMORY.
        unsafe {
            std::env::remove_var("COLI_GPU_MEMORY");
        }
        // Operator-shaped APU: 0.4 GB free of 4.3 GB BIOS carve-out, large RAM.
        // `integrated` starts false (rocm-smi parse default); classification must
        // still treat this as UMA.
        let available = (81.2 * GB as f64) as u64;
        let carve_total = (4.3 * GB as f64) as u64;
        let carve_free = (0.4 * GB as f64) as u64;
        let info = stub_moe_info(80 * GB);
        let plan = PlacementPlan::build_from_info(
            &info,
            &PlanOptions {
                policy: "quality".into(),
                available_memory: Some(available),
                available_disk: Some(500 * GB),
                gpus: Some(vec![colibri_sys::GpuDevice {
                    index: 0,
                    name: "AMD Radeon 860M Graphics".into(),
                    total_bytes: carve_total,
                    free_bytes: carve_free,
                    vendor: "amd".into(),
                    source: "rocm-smi".into(),
                    arch: Some("gfx1152".into()),
                    integrated: false,
                    ..Default::default()
                }]),
                physical_cpus: Some(8),
                cpu_sockets: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        let text = format_plan_readiness(&plan);
        assert!(
            !text.contains("Warning: device VRAM carve-out is busy"),
            "Memory plan must not show the carve-out-busy Warning: {text}"
        );
        for line in text.lines() {
            if line.starts_with("Warning:") {
                assert!(
                    !line.contains("carve-out is busy")
                        && !(line.contains("carve-out")
                            && line.contains("only")
                            && line.contains("free")),
                    "UMA Memory plan must not prefix carve-out-busy as Warning: {text}"
                );
                assert!(
                    !line.contains("VRAM is already in use"),
                    "UMA Memory plan must not use the discrete VRAM-busy Warning: {text}"
                );
            }
        }
        assert!(
            plan.notes
                .iter()
                .any(|n| n.starts_with("using unified system memory budget")),
            "UMA Memory plan should mention the unified system memory budget as a note: notes={:?} {text}",
            plan.notes
        );
        assert!(
            text.lines().any(|l| {
                l.starts_with("using unified system memory budget") && !l.starts_with("Warning:")
            }),
            "unified-budget note must not be Warning-prefixed: {text}"
        );
    }

    /// Notes-only UMA plan (no cold-miss, no clamp) must stay ready to run.
    #[test]
    fn uma_memory_plan_notes_only_stays_ready() {
        unsafe {
            std::env::remove_var("COLI_GPU_MEMORY");
        }
        let available = (81.2 * GB as f64) as u64;
        let carve_total = (4.3 * GB as f64) as u64;
        let carve_free = (0.4 * GB as f64) as u64;
        // ~50 GB experts: fits in unified hot+warm on this RAM geometry, so
        // no cold-miss Warning and no UMA clamp Warning.
        let info = stub_moe_info(50 * GB);
        let plan = PlacementPlan::build_from_info(
            &info,
            &PlanOptions {
                policy: "quality".into(),
                available_memory: Some(available),
                available_disk: Some(500 * GB),
                gpus: Some(vec![colibri_sys::GpuDevice {
                    index: 0,
                    name: "AMD Radeon 860M Graphics".into(),
                    total_bytes: carve_total,
                    free_bytes: carve_free,
                    vendor: "amd".into(),
                    source: "rocm-smi".into(),
                    arch: Some("gfx1152".into()),
                    integrated: false,
                    ..Default::default()
                }]),
                physical_cpus: Some(8),
                cpu_sockets: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        let text = format_plan_readiness(&plan);
        assert!(
            text.contains("Memory plan: ready to run"),
            "notes-only UMA plan must stay ready: {text}"
        );
        assert!(
            !text.contains("Memory plan: review warnings before start"),
            "notes must not flip the plan to review-warnings: {text}"
        );
        assert!(
            plan.warnings.is_empty(),
            "fixture must be notes-only, got warnings={:?}",
            plan.warnings
        );
        assert!(
            text.lines().any(|l| {
                l.starts_with("using unified system memory budget") && !l.starts_with("Warning:")
            }),
            "unified-budget note must still appear: {text}"
        );
        for line in text.lines() {
            if line.starts_with("Warning:") {
                assert!(
                    !line.contains("carve-out is busy") && !line.contains("VRAM is already in use"),
                    "notes-only UMA plan must not Warning carve-out or discrete VRAM: {text}"
                );
            }
        }
    }

    /// Operator screenshot: 400+ GB MoE vs ~39 GB unified budget. Cold-miss
    /// copy must print as a Memory plan note, not `Warning:`, and must not
    /// flip the plan to "review warnings before start".
    #[test]
    fn intended_cold_overflow_memory_plan_is_note_not_warning() {
        unsafe {
            std::env::remove_var("COLI_GPU_MEMORY");
        }
        let available = (45.0 * GB as f64) as u64;
        let info = stub_moe_info(400 * GB);
        let plan = PlacementPlan::build_from_info(
            &info,
            &PlanOptions {
                policy: "quality".into(),
                available_memory: Some(available),
                available_disk: Some(500 * GB),
                gpus: Some(vec![colibri_sys::GpuDevice {
                    index: 0,
                    name: "AMD Radeon 860M Graphics".into(),
                    total_bytes: (4.3 * GB as f64) as u64,
                    free_bytes: (0.4 * GB as f64) as u64,
                    vendor: "amd".into(),
                    source: "rocm-smi".into(),
                    arch: Some("gfx1152".into()),
                    integrated: true,
                    ..Default::default()
                }]),
                physical_cpus: Some(8),
                cpu_sockets: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        let text = format_plan_readiness(&plan);
        const COLD_MISS: &str =
            "cold expert misses may reach disk; normal decode speed depends on hit rate";
        assert!(
            plan.tiers.disk.cold_expert_bytes > 0,
            "fixture must intend overflow: {text}"
        );
        assert!(
            text.contains(COLD_MISS),
            "Memory plan must still mention intended overflow: {text}"
        );
        assert!(
            !text.contains(&format!("Warning: {COLD_MISS}")),
            "Memory plan must not scare-prefix intended overflow: {text}"
        );
        assert!(
            text.contains("Memory plan: ready to run"),
            "notes-only overflow must stay ready: {text}"
        );
        assert!(
            !text.contains("Memory plan: review warnings before start"),
            "intended overflow must not flip the plan to review-warnings: {text}"
        );
        assert!(
            plan.warnings
                .iter()
                .all(|w| !w.contains("cold expert misses")),
            "cold-miss must not be in warnings: {:?}",
            plan.warnings
        );
    }

    #[test]
    fn discrete_memory_plan_ui_still_warns_vram_busy() {
        unsafe {
            std::env::remove_var("COLI_GPU_MEMORY");
        }
        let info = stub_moe_info(6 * GB);
        let plan = PlacementPlan::build_from_info(
            &info,
            &PlanOptions {
                available_memory: Some(64 * GB),
                available_disk: Some(500 * GB),
                gpus: Some(vec![colibri_sys::GpuDevice {
                    index: 0,
                    name: "AMD Radeon RX 7900 XTX".into(),
                    total_bytes: 24 * GB,
                    free_bytes: 6 * GB,
                    vendor: "amd".into(),
                    source: "rocm-smi".into(),
                    integrated: false,
                    ..Default::default()
                }]),
                physical_cpus: Some(16),
                cpu_sockets: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        let text = format_plan_readiness(&plan);
        assert!(
            text.lines()
                .any(|l| { l.starts_with("Warning:") && l.contains("VRAM is already in use") }),
            "discrete busy VRAM must stay a Warning: {text}"
        );
        assert!(
            !text.contains("Warning: device VRAM carve-out is busy"),
            "discrete warning must stay the VRAM-in-use wording: {text}"
        );
        assert!(
            !text.contains("unified system memory budget"),
            "discrete Memory plan must not mention the UMA unified-budget sentence: {text}"
        );
    }

    #[test]
    fn readiness_plan_copy_is_plain_english() {
        assert_eq!(plain_bottleneck_label("gpu vram", "vram"), "GPU memory");
        assert_eq!(plain_bottleneck_label("", "disk-io"), "disk I/O");
        assert_eq!(plain_bottleneck_label("", ""), "none detected");

        let empty = run_plan(Path::new(""), None);
        assert!(
            empty.to_lowercase().contains("model path")
                || empty.to_lowercase().contains("no memory plan"),
            "{empty}"
        );
        assert!(
            !empty.contains("COLI_MODEL") && !empty.contains("COLIBRI_"),
            "empty plan must not lead with lab env names: {empty}"
        );

        let sample = "Memory plan: ready to run\n\
Expected cache hit rate: 92%\n\
Memory on GPU: 16.0 GB\n\
System RAM budget: 32.0 GB\n\
Likely limit: GPU memory\n";
        assert!(!sample.contains("ssd_probe_state"));
        assert!(!sample.contains("dense_bytes="));
        assert!(!sample.contains("version="));
        assert!(sample.contains("Memory on GPU"));
    }

    #[test]
    fn scan_model_registry_temp_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("picked");
        std::fs::create_dir(&model).unwrap();
        std::fs::write(
            model.join("config.json"),
            br#"{"model_type":"glm_moe_dsa"}"#,
        )
        .unwrap();
        let entries = scan_model_registry(&[dir.path().to_path_buf()]).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(format_registry_entry(&entries[0]).contains("picked"));
        assert!(format_registry_entry(&entries[0]).contains("glm"));
    }

    #[test]
    fn registry_scan_roots_dedupes_store() {
        let store = PathBuf::from("/tmp/store-x");
        let roots = registry_scan_roots(Some(&store), [store.clone(), PathBuf::from("/extra")]);
        assert_eq!(roots.len(), 2);
        assert_eq!(roots[0], store);
        assert_eq!(roots[1], PathBuf::from("/extra"));
    }
}
