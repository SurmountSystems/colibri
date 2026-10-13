//! Installation diagnostics (standard + deep paths).
//!
//! Port of `c/doctor.py`:
//! - `run_doctor` (standard checks + plan)
//! - `deep_container_report` (safetensors headers/layouts, shard sequence,
//!   required core tensors, index agreement, mirror admission)
//! - helpers: `_safetensors_header`, `_tensor_layout`, `_shard_sequence_report`,
//!   `cuda_linkage`, `missing_shared_libraries`
//!
//! Schema version 1 JSON report. Deep path does not hash payloads.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Result;
use crate::linkage::{hip_process_rebuild_next_step, probe_process_gpu_linkage};
use crate::plan::{PlacementPlan, PlanOptions};
use crate::probe::{
    GB, GpuDevice, apply_gpu_memory_classification, discover_gpus, gpu_free_vram_near_zero,
    memory_available, memory_total, ssd_probe_pending, ssd_probe_state,
};

/// Max safetensors header size (512 MiB), matching `doctor.SAFETENSORS_MAX_HEADER`.
pub const SAFETENSORS_MAX_HEADER: u64 = 512 << 20;
/// Max model index size (same bound as header).
pub const MODEL_INDEX_MAX_BYTES: u64 = SAFETENSORS_MAX_HEADER;
/// Runtime shard count cap (`doctor.MAX_SAFETENSORS_SHARDS`).
pub const MAX_SAFETENSORS_SHARDS: usize = 512;

/// Dtype → element size for deep layout checks (`doctor.SAFETENSORS_DTYPES`).
///
/// Must stay aligned with `st_dtype_code` / `st_dtype_esz` in `c/st.h`.
/// Includes integer index maps (I64/U64, e.g. DeepSeek tid2eid) and native
/// FP8 weight dtypes so thorough doctor does not false-fail valid checkpoints.
const SAFETENSORS_DTYPES: &[(&str, u64)] = &[
    ("BF16", 2),
    ("F16", 2),
    ("F32", 4),
    ("U8", 1),
    ("I8", 1),
    // FP8 spellings accepted by st_dtype_code (1 byte each).
    ("F8_E4M3", 1),
    ("F8_E4M3FN", 1),
    ("float8_e4m3fn", 1),
    ("F8_E8M0", 1),
    ("F8_E8M0FNU", 1),
    // Integer index maps (8 bytes); engine code 6.
    ("I64", 8),
    ("U64", 8),
];

/// GLM / Inkling / Kimi / Olmoe core tensors (HF-style names the engines load).
/// Matches historical `doctor.REQUIRED_CORE_TENSORS` and `c/colibri.c` / `c/inkling.c`.
const REQUIRED_CORE_TENSORS_GLM: &[&str] = &[
    "model.embed_tokens.weight",
    "model.norm.weight",
    "lm_head.weight",
];

/// DeepSeek-V4 checkpoint names. The engine loads these via `coli_st_find`
/// (`c/deepseek_v4.c`: `embed.weight`, `norm.weight`, `head.weight`), not the
/// HF `model.embed_tokens` / `lm_head` spellings.
const REQUIRED_CORE_TENSORS_DEEPSEEK_V4: &[&str] = &["embed.weight", "norm.weight", "head.weight"];

/// Required core tensor names for a model family.
///
/// Incomplete installs of the correct family still fail: every name in the
/// returned list must be present. Wrong family names (e.g. GLM names on a
/// DeepSeek-V4 tree) are not accepted as substitutes.
pub fn required_core_tensors(family: crate::model::ModelFamily) -> &'static [&'static str] {
    use crate::model::ModelFamily;
    match family {
        ModelFamily::DeepseekV4 => REQUIRED_CORE_TENSORS_DEEPSEEK_V4,
        ModelFamily::Glm | ModelFamily::Inkling | ModelFamily::Kimi | ModelFamily::Olmoe => {
            REQUIRED_CORE_TENSORS_GLM
        }
    }
}

/// Names from `required` that are absent from the scanned tensor set.
fn missing_required_core<'a>(
    required: &[&'a str],
    tensor_sources: &HashMap<String, String>,
) -> Vec<&'a str> {
    required
        .iter()
        .copied()
        .filter(|n| !tensor_sources.contains_key(*n))
        .collect()
}

/// Fail summary when core tensors are missing (includes names for operators).
fn missing_core_summary(missing: &[&str]) -> String {
    if missing.is_empty() {
        return "required core tensors are present".to_string();
    }
    format!(
        "{} required core tensor(s) are missing: {}",
        missing.len(),
        missing.join(", ")
    )
}

/// One check line in the doctor report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub id: String,
    /// `pass` | `warn` | `fail` | `skip`
    pub status: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// Full doctor report (schema_version 1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub schema_version: u32,
    /// `ok` | `warning` | `error`
    pub status: String,
    pub model: String,
    /// `standard` | `deep`
    pub mode: String,
    pub checks: Vec<DoctorCheck>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<PlacementPlan>,
}

/// Options for [`run_doctor`].
#[derive(Debug, Clone, Default)]
pub struct DoctorOptions {
    pub ram_gb: f64,
    pub context: u32,
    pub gpu_indices: Option<Vec<u32>>,
    pub vram_gb: f64,
    pub engine_path: Option<PathBuf>,
    pub available_memory: Option<u64>,
    pub available_disk: Option<u64>,
    pub deep: bool,
    pub mirror_dir: Option<PathBuf>,
    /// Injected GPU inventory (tests / hosts that already probed). `None` = discover.
    pub gpus: Option<Vec<GpuDevice>>,
    /// Injected CUDA/HIP linkage (tests). `None` = probe engine binary.
    pub linkage: Option<AcceleratorLinkage>,
    /// Override whether the in-process (FFI) engine counts as ready for
    /// `engine.binary`. `None` = detect from Cargo `ffi` feature and
    /// `COLIBRI_FORCE_PROCESS` (same rules as native prefer-FFI).
    pub in_process_engine: Option<bool>,
}

/// CUDA / HIP linkage without loading the runtime.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AcceleratorLinkage {
    pub linked: bool,
    pub missing: bool,
    /// `"cuda"`, `"hip"`, or empty when neither is linked / unknown.
    pub kind: String,
}

fn check(id: &str, status: &str, summary: impl Into<String>) -> DoctorCheck {
    DoctorCheck {
        id: id.into(),
        status: status.into(),
        summary: summary.into(),
        details: None,
    }
}

fn check_details(
    id: &str,
    status: &str,
    summary: impl Into<String>,
    details: serde_json::Value,
) -> DoctorCheck {
    DoctorCheck {
        id: id.into(),
        status: status.into(),
        summary: summary.into(),
        details: Some(details),
    }
}

/// CUDA/HIP linkage without loading the runtime.
///
/// Port of `doctor.cuda_linkage` with HIP kind detection (Linux `libamdhip64` /
/// Windows `coli_hip.dll`). Implementation: [`crate::linkage::probe_process_gpu_linkage`].
pub fn cuda_linkage(engine_path: &Path) -> AcceleratorLinkage {
    probe_cuda_linkage(engine_path)
}

/// Internal probe; returns full [`AcceleratorLinkage`].
fn probe_cuda_linkage(engine_path: &Path) -> AcceleratorLinkage {
    let p = probe_process_gpu_linkage(engine_path);
    AcceleratorLinkage {
        linked: p.linked,
        missing: p.missing,
        kind: p.kind,
    }
}

/// Infer a dominant vendor label from selected GPUs (`nvidia` / `amd` / empty).
fn gpu_vendor_label(gpus: &[GpuDevice]) -> String {
    let mut nvidia = false;
    let mut amd = false;
    for g in gpus {
        match g.vendor.as_str() {
            "nvidia" => nvidia = true,
            "amd" => amd = true,
            _ => {
                let low = g.name.to_ascii_lowercase();
                if low.contains("nvidia") || low.contains("geforce") || low.contains("rtx ") {
                    nvidia = true;
                } else if low.contains("amd") || low.contains("radeon") || low.contains("instinct")
                {
                    amd = true;
                }
            }
        }
    }
    match (nvidia, amd) {
        (true, false) => "nvidia".into(),
        (false, true) => "amd".into(),
        _ => String::new(),
    }
}

/// Build accelerator.cuda check summary (stable id; vendor-aware wording).
///
/// `engine_basename` is used only for the AMD CPU-only operational next-step
/// (rebuild process engine with `HIP=1`).
fn accelerator_check(
    selected: &[GpuDevice],
    linkage: &AcceleratorLinkage,
    requested_missing: bool,
    disabled: bool,
    available_memory: u64,
    engine_basename: &str,
) -> DoctorCheck {
    const ID: &str = "accelerator.cuda";
    if disabled {
        return check(ID, "skip", "GPU use was explicitly disabled");
    }
    if requested_missing {
        return check(ID, "fail", "one or more requested GPUs were not detected");
    }
    let vendor = gpu_vendor_label(selected);
    let runtime = if !linkage.kind.is_empty() {
        linkage.kind.as_str()
    } else if vendor == "amd" {
        "hip"
    } else if vendor == "nvidia" {
        "cuda"
    } else {
        ""
    };
    let low_free = selected.iter().any(gpu_free_vram_near_zero);
    let any_integrated = selected.iter().any(|g| g.integrated);
    let device_indices: Vec<u32> = selected.iter().map(|g| g.index).collect();
    let carve_outs: Vec<serde_json::Value> = selected
        .iter()
        .map(|g| {
            serde_json::json!({
                "index": g.index,
                "total_bytes": g.total_bytes,
                "free_bytes": g.free_bytes,
                "used_bytes": g.total_bytes.saturating_sub(g.free_bytes),
                "integrated": g.integrated,
                "gtt_total_bytes": g.gtt_total_bytes,
                "gtt_free_bytes": g.gtt_free_bytes,
            })
        })
        .collect();
    let mut details = serde_json::json!({
        "devices": device_indices,
        "vendor": vendor,
        "runtime": runtime,
        "linked": linkage.linked,
        "missing": linkage.missing,
        "low_free_vram": low_free,
        "integrated": any_integrated,
        "shared_system_memory": any_integrated,
        "system_memory_available_bytes": available_memory,
        "system_memory_total_bytes": memory_total(),
        "vram_carve_out": carve_outs,
        "sources": selected.iter().map(|g| g.source.as_str()).collect::<Vec<_>>(),
    });

    if selected.is_empty() {
        return check(ID, "skip", "no GPU detected; CPU path is available");
    }
    if linkage.missing {
        let summary = match runtime {
            "hip" => "HIP runtime library (libamdhip64) is missing",
            "cuda" => "CUDA runtime library is missing",
            _ => "GPU runtime library is missing",
        };
        if runtime == "hip" || vendor == "amd" {
            if let Some(obj) = details.as_object_mut() {
                obj.insert(
                    "hint".into(),
                    serde_json::Value::String(hip_process_rebuild_next_step(engine_basename)),
                );
            }
        }
        return check_details(ID, "fail", summary, details);
    }
    if linkage.linked {
        let mut summary = match (runtime, vendor.as_str()) {
            ("hip", _) | (_, "amd") => "HIP engine and AMD device(s) are available".to_string(),
            ("cuda", _) | (_, "nvidia") => {
                "CUDA engine and NVIDIA device(s) are available".to_string()
            }
            _ => "GPU engine and devices are available".to_string(),
        };
        // On UMA the BIOS VRAM window is not the GPU budget. A busy carve-out
        // must not warn (or drive Overall) as if it were discrete VRAM.
        let status = if low_free && !any_integrated {
            summary.push_str("; free VRAM is near zero (display compositor may own most of it)");
            "warn"
        } else if any_integrated {
            summary.push_str("; shared system memory (UMA), not discrete VRAM only");
            "pass"
        } else {
            "pass"
        };
        return check_details(ID, status, summary, details);
    }
    // Devices present, engine not GPU-linked (CPU-only process binary).
    let mut summary = match vendor.as_str() {
        "amd" => "AMD GPU detected but the engine is CPU-only (build with HIP=1)".to_string(),
        "nvidia" => "NVIDIA GPU detected but the engine is CPU-only".to_string(),
        _ => "GPU detected but the engine is CPU-only".to_string(),
    };
    if any_integrated {
        summary.push_str("; GPU shares system memory (UMA)");
    }
    if vendor == "amd" {
        if let Some(obj) = details.as_object_mut() {
            obj.insert(
                "hint".into(),
                serde_json::Value::String(hip_process_rebuild_next_step(engine_basename)),
            );
        }
    }
    check_details(ID, "warn", summary, details)
}

/// Unresolved shared libraries (POSIX ldd).
///
/// Port of `doctor.missing_shared_libraries`.
pub fn missing_shared_libraries(engine_path: &Path) -> Vec<String> {
    #[cfg(unix)]
    {
        if !engine_path.is_file() {
            return vec![];
        }
        let out = match Command::new("ldd").arg(engine_path).output() {
            Ok(o) => o,
            Err(_) => return vec![],
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let mut missing = Vec::new();
        for line in text.lines() {
            if line.contains("not found") {
                let name = line.split("=>").next().unwrap_or("").trim();
                if !name.is_empty() {
                    missing.push(name.to_string());
                }
            }
        }
        missing.sort();
        missing.dedup();
        missing
    }
    #[cfg(not(unix))]
    {
        let _ = engine_path;
        vec![]
    }
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Whether doctor should treat the in-process engine as available.
fn doctor_in_process_available(opts: &DoctorOptions) -> bool {
    if let Some(v) = opts.in_process_engine {
        return v;
    }
    #[cfg(feature = "ffi")]
    {
        crate::ffi::ffi_available()
    }
    #[cfg(not(feature = "ffi"))]
    {
        false
    }
}

/// Process binary linkage, optionally merged with in-process GPU embed.
///
/// Explicit `opts.linkage` wins (tests inject fixtures). Otherwise probe the
/// process engine path, then if that is not GPU-linked, treat built-in
/// `ffi_hip_linked` / `ffi_cuda_linked` as accelerator-linked.
fn resolve_doctor_linkage(engine: &Path, opts: &DoctorOptions) -> AcceleratorLinkage {
    if let Some(ref injected) = opts.linkage {
        return injected.clone();
    }
    let process = probe_cuda_linkage(engine);
    #[cfg(feature = "ffi")]
    let (ffi_hip, ffi_cuda) = (crate::ffi::ffi_hip_linked(), crate::ffi::ffi_cuda_linked());
    #[cfg(not(feature = "ffi"))]
    let (ffi_hip, ffi_cuda) = (false, false);
    // Always call merge so unit tests cover the pure function without feature=ffi.
    merge_in_process_gpu_linkage(process, ffi_hip, ffi_cuda)
}

/// Pure merge for doctor accelerator honesty (unit-tested without live GPU).
///
/// When the process engine is already GPU-linked, keep that result. When it is
/// not, an in-process HIP or CUDA embed still means the host is not CPU-only
/// for GPU kernels.
pub(crate) fn merge_in_process_gpu_linkage(
    process: AcceleratorLinkage,
    ffi_hip: bool,
    ffi_cuda: bool,
) -> AcceleratorLinkage {
    if process.linked {
        return process;
    }
    if ffi_hip {
        return AcceleratorLinkage {
            linked: true,
            missing: false,
            kind: "hip".into(),
        };
    }
    if ffi_cuda {
        return AcceleratorLinkage {
            linked: true,
            missing: false,
            kind: "cuda".into(),
        };
    }
    process
}

/// Resolve the process engine path for doctor: explicit override, then
/// `COLI_ENGINE` / `COLIBRI_ENGINE`, then family-aware [`locate_engine`],
/// else the family's basename for messaging.
fn resolve_doctor_engine_path(model: &Path, opts: &DoctorOptions) -> PathBuf {
    if let Some(ref p) = opts.engine_path {
        return p.clone();
    }
    let family = crate::model::model_arch(model);
    let env_override = std::env::var_os("COLI_ENGINE")
        .or_else(|| std::env::var_os("COLIBRI_ENGINE"))
        .map(PathBuf::from);
    if let Some(ref p) = env_override {
        if p.is_file() {
            return p.clone();
        }
    }
    #[cfg(feature = "runtime")]
    {
        use crate::engine::{EngineLocate, locate_engine};
        if let Ok(found) = locate_engine(EngineLocate {
            family,
            override_path: env_override,
            search_roots: vec![],
        }) {
            return found;
        }
    }
    PathBuf::from(family.engine_basename())
}

/// Summary when the external process binary is missing and FFI is not ready.
///
/// Short line for the checklist: models are fine; the process engine path is not.
/// Recovery steps live in check details (`hint`).
///
/// **Family-agnostic:** `path` is whatever doctor resolved (override, locate, or
/// [`ModelFamily::engine_basename`] for Glm/colibri, Inkling, Kimi, DeepseekV4,
/// Olmoe). Wording never names a single model family.
fn external_engine_not_found_summary(path: &Path) -> String {
    format!(
        "Model files look ready; process engine binary not found ({}).",
        path.display()
    )
}

/// Operational recovery text for process-only engine-missing fail (details.hint).
///
/// Same string for every model family; not DeepSeek-only or GLM-only.
fn external_engine_not_found_hint() -> &'static str {
    "Set COLIBRI_ENGINE or COLI_ENGINE to a built process engine, build with `make -C c <engine>`, or rebuild colibri-native with the ffi feature (default: install + ffi)."
}

/// Build the `engine.binary` check from process path state and FFI readiness.
///
/// - Process executable ready → pass (or fail if shared libs missing).
/// - Process missing + in-process FFI available → pass ("in-process engine").
/// - Process missing + no FFI → fail: model ready + binary not found (never
///   "engine is not built"); recovery in details.hint.
fn engine_binary_check(engine: &Path, in_process_available: bool) -> DoctorCheck {
    if is_executable(engine) {
        let unresolved = missing_shared_libraries(engine);
        if !unresolved.is_empty() {
            return check_details(
                "engine.binary",
                "fail",
                format!(
                    "engine cannot load: {} (install the runtime package, e.g. libgomp1, and retry)",
                    unresolved.join(", ")
                ),
                serde_json::json!({ "path": engine.display().to_string(), "missing": unresolved }),
            );
        }
        return check_details(
            "engine.binary",
            "pass",
            "engine executable is ready",
            serde_json::json!({ "path": engine.display().to_string() }),
        );
    }
    if engine.is_file() {
        return check(
            "engine.binary",
            "fail",
            "engine exists but is not executable",
        );
    }
    // Missing process binary.
    if in_process_available {
        return check_details(
            "engine.binary",
            "pass",
            "in-process engine is available",
            serde_json::json!({
                "path": engine.display().to_string(),
                "mode": "ffi",
            }),
        );
    }
    check_details(
        "engine.binary",
        "fail",
        external_engine_not_found_summary(engine),
        serde_json::json!({
            "path": engine.display().to_string(),
            "hint": external_engine_not_found_hint(),
        }),
    )
}

fn dtype_size(dtype: &str) -> Option<u64> {
    SAFETENSORS_DTYPES
        .iter()
        .find(|(n, _)| *n == dtype)
        .map(|(_, s)| *s)
}

/// Read one bounded safetensors header without touching tensor payloads.
///
/// Port of `doctor._safetensors_header`.
/// Returns `(file_size, raw_header_bytes, header_json)`.
pub fn safetensors_header(path: &Path) -> std::result::Result<(u64, Vec<u8>, Value), String> {
    let mut stream = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let file_size = stream.metadata().map_err(|e| e.to_string())?.len();
    let mut raw_length = [0u8; 8];
    stream
        .read_exact(&mut raw_length)
        .map_err(|_| "short safetensors header".to_string())?;
    let header_length = u64::from_le_bytes(raw_length);
    if !(2..=SAFETENSORS_MAX_HEADER).contains(&header_length)
        || header_length > file_size.saturating_sub(8)
    {
        return Err(format!(
            "invalid safetensors header length: {header_length}"
        ));
    }
    let mut raw_header = vec![0u8; header_length as usize];
    stream
        .read_exact(&mut raw_header)
        .map_err(|_| "short safetensors header body".to_string())?;
    let header: Value = serde_json::from_slice(&raw_header)
        .map_err(|e| format!("invalid safetensors JSON: {e}"))?;
    if !header.is_object() {
        return Err("safetensors header is not a JSON object".into());
    }
    Ok((file_size, raw_header, header))
}

/// Validate tensor meta against payload size; return `[start, end)`.
///
/// Port of `doctor._tensor_layout`.
pub fn tensor_layout(meta: &Value, payload_size: u64) -> std::result::Result<(u64, u64), String> {
    let obj = meta
        .as_object()
        .ok_or_else(|| "tensor metadata is not an object".to_string())?;
    let dtype = obj
        .get("dtype")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "unsupported dtype: null".to_string())?;
    let elem = dtype_size(dtype).ok_or_else(|| format!("unsupported dtype: {dtype:?}"))?;
    let offsets = obj
        .get("data_offsets")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "data_offsets must contain exactly two integers".to_string())?;
    if offsets.len() != 2 {
        return Err("data_offsets must contain exactly two integers".into());
    }
    // Reject booleans (serde_json treats bool separately from number).
    let start = offsets[0]
        .as_u64()
        .ok_or_else(|| "data_offsets must contain exactly two integers".to_string())?;
    let end = offsets[1]
        .as_u64()
        .ok_or_else(|| "data_offsets must contain exactly two integers".to_string())?;
    if end < start || end > payload_size {
        return Err(format!(
            "data_offsets [{start}, {end}] exceed payload size {payload_size}"
        ));
    }
    let shape = obj
        .get("shape")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "shape must contain only non-negative integers".to_string())?;
    let mut elements: u128 = 1;
    for dim in shape {
        let d = dim
            .as_u64()
            .ok_or_else(|| "shape must contain only non-negative integers".to_string())?;
        elements = elements
            .checked_mul(d as u128)
            .ok_or_else(|| "shape element count exceeds int64".to_string())?;
        if elements > (i64::MAX as u128) {
            return Err("shape element count exceeds int64".into());
        }
    }
    if dtype != "U8" && dtype != "I8" {
        let expected = elements * elem as u128;
        if (end - start) as u128 != expected {
            return Err("shape and dtype disagree with the tensor byte span".into());
        }
    }
    Ok((start, end))
}

/// Filename-declared shard sequence check.
///
/// Port of `doctor._shard_sequence_report`.
pub fn shard_sequence_report(shard_names: &[String]) -> (String, String, Option<Value>) {
    let hf_re = Regex::new(r"^model-(\d+)-of-(\d+)\.safetensors$").unwrap();
    let out_re = Regex::new(r"^out-(\d+)\.safetensors$").unwrap();
    let mut hf_shards: Vec<(u64, u64)> = Vec::new();
    let mut out_shards: Vec<u64> = Vec::new();
    for name in shard_names {
        if let Some(c) = hf_re.captures(name) {
            let index: u64 = c[1].parse().unwrap_or(0);
            let total: u64 = c[2].parse().unwrap_or(0);
            hf_shards.push((index, total));
        } else if let Some(c) = out_re.captures(name) {
            out_shards.push(c[1].parse().unwrap_or(0));
        }
    }
    if !hf_shards.is_empty() && !out_shards.is_empty() {
        return (
            "fail".into(),
            "model mixes filename-declared shard schemes".into(),
            Some(serde_json::json!({
                "huggingface_shards": hf_shards.len(),
                "converter_shards": out_shards.len(),
            })),
        );
    }
    if !hf_shards.is_empty() {
        let declared: HashSet<u64> = hf_shards.iter().map(|(_, t)| *t).collect();
        if declared.len() != 1 {
            return (
                "fail".into(),
                "shard filenames declare different totals".into(),
                None,
            );
        }
        let total = *declared.iter().next().unwrap();
        let found: HashSet<u64> = hf_shards.iter().map(|(i, _)| *i).collect();
        let in_range: HashSet<u64> = found
            .iter()
            .copied()
            .filter(|i| *i >= 1 && *i <= total)
            .collect();
        let missing = total.saturating_sub(in_range.len() as u64);
        let unexpected = found.difference(&in_range).count();
        let duplicates = hf_shards.len() - found.len();
        if missing > 0 || unexpected > 0 || duplicates > 0 {
            return (
                "fail".into(),
                "declared shard sequence is incomplete or inconsistent".into(),
                Some(serde_json::json!({
                    "declared_shards": total,
                    "found_shards": found.len(),
                    "missing_shards": missing,
                    "unexpected_shards": unexpected,
                    "duplicate_shards": duplicates,
                })),
            );
        }
        return (
            "pass".into(),
            "all filename-declared shards are present".into(),
            Some(serde_json::json!({
                "declared_shards": total,
                "found_shards": found.len(),
            })),
        );
    }
    if !out_shards.is_empty() {
        let found: HashSet<u64> = out_shards.iter().copied().collect();
        let first = *found.iter().min().unwrap();
        let last = *found.iter().max().unwrap();
        let missing = last + 1 - found.len() as u64;
        let duplicates = out_shards.len() - found.len();
        if missing > 0 || duplicates > 0 {
            return (
                "fail".into(),
                "converter shard numbering contains gaps or duplicates".into(),
                Some(serde_json::json!({
                    "first_shard": first,
                    "last_shard": last,
                    "found_shards": found.len(),
                    "missing_shards": missing,
                    "duplicate_shards": duplicates,
                    "tail_completeness_declared": false,
                })),
            );
        }
        return (
            "pass".into(),
            "converter shard numbering is contiguous".into(),
            Some(serde_json::json!({
                "first_shard": first,
                "last_shard": last,
                "found_shards": found.len(),
                "tail_completeness_declared": false,
            })),
        );
    }
    (
        "skip".into(),
        "shard filenames do not declare a sequence".into(),
        None,
    )
}

/// Intermediate deep report (mirrors Python `deep_container_report` return).
#[derive(Debug, Clone)]
pub struct DeepContainerReport {
    pub container: Value,
    pub sequence_status: String,
    pub sequence_summary: String,
    pub sequence_details: Option<Value>,
    pub required_status: String,
    pub required_summary: String,
    pub required_details: Value,
    pub index_status: String,
    pub index_summary: String,
    pub index_details: Option<Value>,
    pub mirror_status: String,
    pub mirror_summary: String,
    pub mirror_details: Option<Value>,
}

/// Validate all tensor headers/layouts and runtime-equivalent mirror admission.
///
/// Port of `doctor.deep_container_report`.
pub fn deep_container_report(
    model: &Path,
    mirror_dir: Option<&Path>,
) -> std::result::Result<DeepContainerReport, String> {
    let mut shards: Vec<PathBuf> = std::fs::read_dir(model)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| x == "safetensors")
        })
        .collect();
    shards.sort();
    if shards.is_empty() {
        return Err("no safetensors shards found".into());
    }
    if shards.len() > MAX_SAFETENSORS_SHARDS {
        return Err(format!(
            "more than {MAX_SAFETENSORS_SHARDS} safetensors shards are not supported by the runtime"
        ));
    }

    let mut tensor_sources: HashMap<String, String> = HashMap::new();
    let mut shard_headers: HashMap<String, (u64, Vec<u8>)> = HashMap::new();
    let mut tensor_count: usize = 0;
    let mut header_bytes: u64 = 0;
    let mut payload_bytes: u64 = 0;
    let mut shard_names: Vec<String> = Vec::new();

    for shard in &shards {
        let name = shard
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        shard_names.push(name.clone());
        let (file_size, raw_header, header) =
            safetensors_header(shard).map_err(|e| format!("{name}: {e}"))?;
        let payload_size = file_size - 8 - raw_header.len() as u64;
        let obj = header
            .as_object()
            .ok_or_else(|| format!("{name}: safetensors header is not a JSON object"))?;
        let mut ranges: Vec<(u64, u64, String)> = Vec::new();
        for (tname, meta) in obj {
            if tname == "__metadata__" {
                if !meta.is_object() {
                    return Err(format!("{name}: __metadata__ is not an object"));
                }
                continue;
            }
            if let Some(prev) = tensor_sources.get(tname) {
                return Err(format!("duplicate tensor '{tname}' in {prev} and {name}"));
            }
            let (start, end) = tensor_layout(meta, payload_size)
                .map_err(|e| format!("{name}: tensor '{tname}': {e}"))?;
            tensor_sources.insert(tname.clone(), name.clone());
            tensor_count += 1;
            ranges.push((start, end, tname.clone()));
        }
        ranges.retain(|r| r.0 != r.1);
        ranges.sort_by_key(|r| r.0);
        for window in ranges.windows(2) {
            let previous = &window[0];
            let current = &window[1];
            if current.0 < previous.1 {
                return Err(format!(
                    "{name}: tensors '{}' and '{}' overlap",
                    previous.2, current.2
                ));
            }
        }
        header_bytes += raw_header.len() as u64;
        payload_bytes += payload_size;
        shard_headers.insert(name, (file_size, raw_header));
    }

    // Index
    let index_path = model.join("model.safetensors.index.json");
    let (index_status, index_summary, index_details) = if index_path.is_file() {
        match validate_model_index(&index_path, &tensor_sources, &shard_headers) {
            Ok(details) => (
                "pass".to_string(),
                "model index matches every scanned tensor".to_string(),
                Some(details),
            ),
            Err(e) => (
                "fail".to_string(),
                format!("model index is invalid: {e}"),
                None,
            ),
        }
    } else {
        (
            "skip".to_string(),
            "model index is not present".to_string(),
            None,
        )
    };

    let family = crate::model::model_arch(model);
    let required = required_core_tensors(family);
    let missing_core = missing_required_core(required, &tensor_sources);
    let (required_status, required_summary) = if missing_core.is_empty() {
        (
            "pass".to_string(),
            "required core tensors are present".to_string(),
        )
    } else {
        ("fail".to_string(), missing_core_summary(&missing_core))
    };
    let required_details = serde_json::json!({
        "family": family.as_str(),
        "required_tensors": required.len(),
        "required_names": required,
        "missing_tensors": missing_core,
    });

    let (mirror_status, mirror_summary, mirror_details) = mirror_report(mirror_dir, &shard_headers);

    let (seq_status, seq_summary, seq_details) = shard_sequence_report(&shard_names);

    Ok(DeepContainerReport {
        container: serde_json::json!({
            "shards": shards.len(),
            "tensors": tensor_count,
            "header_bytes": header_bytes,
            "payload_bytes": payload_bytes,
            "payload_hashing": false,
        }),
        sequence_status: seq_status,
        sequence_summary: seq_summary,
        sequence_details: seq_details,
        required_status,
        required_summary,
        required_details,
        index_status,
        index_summary,
        index_details,
        mirror_status,
        mirror_summary,
        mirror_details,
    })
}

fn validate_model_index(
    index_path: &Path,
    tensor_sources: &HashMap<String, String>,
    shard_headers: &HashMap<String, (u64, Vec<u8>)>,
) -> std::result::Result<Value, String> {
    let meta = std::fs::metadata(index_path).map_err(|e| e.to_string())?;
    let index_size = meta.len();
    if index_size > MODEL_INDEX_MAX_BYTES {
        return Err(format!("model index exceeds {MODEL_INDEX_MAX_BYTES} bytes"));
    }
    let raw = std::fs::read(index_path).map_err(|e| e.to_string())?;
    if raw.len() as u64 != index_size {
        return Err("model index changed while reading".into());
    }
    let document: Value = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    let obj = document
        .as_object()
        .ok_or_else(|| "model index is not a JSON object".to_string())?;
    let weight_map = obj
        .get("weight_map")
        .and_then(|v| v.as_object())
        .ok_or_else(|| "weight_map is not an object".to_string())?;
    for (name, shard) in weight_map {
        if !shard.is_string() {
            return Err("weight_map keys and values must be strings".into());
        }
        let _ = name;
    }
    let wm: HashMap<String, String> = weight_map
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
        .collect();
    let wm_keys: HashSet<&str> = wm.keys().map(|s| s.as_str()).collect();
    let ts_keys: HashSet<&str> = tensor_sources.keys().map(|s| s.as_str()).collect();
    let missing_tensors = wm_keys.difference(&ts_keys).count();
    let unindexed_tensors = ts_keys.difference(&wm_keys).count();
    let misplaced = wm
        .iter()
        .filter(|(name, shard)| {
            tensor_sources
                .get(name.as_str())
                .is_some_and(|s| s != shard.as_str())
        })
        .count();
    let unknown_shards: HashSet<&str> = wm
        .values()
        .map(|s| s.as_str())
        .filter(|s| !shard_headers.contains_key(*s))
        .collect();
    if missing_tensors > 0 || unindexed_tensors > 0 || misplaced > 0 || !unknown_shards.is_empty() {
        return Err(format!(
            "index disagrees with scanned tensors (missing={missing_tensors}, unindexed={unindexed_tensors}, misplaced={misplaced}, unknown_shards={})",
            unknown_shards.len()
        ));
    }
    Ok(serde_json::json!({ "indexed_tensors": wm.len() }))
}

fn mirror_report(
    mirror_dir: Option<&Path>,
    shard_headers: &HashMap<String, (u64, Vec<u8>)>,
) -> (String, String, Option<Value>) {
    let Some(mirror_dir) = mirror_dir else {
        return (
            "skip".into(),
            "no mirror directory is configured".into(),
            None,
        );
    };
    let mirror_path = mirror_dir;
    if !mirror_path.is_dir() {
        return (
            "warn".into(),
            "configured mirror directory is unavailable".into(),
            Some(serde_json::json!({
                "path": mirror_path.display().to_string(),
                "accepted_shards": 0,
            })),
        );
    }
    let mut accepted = 0usize;
    let mut missing = 0usize;
    let mut divergent = 0usize;
    for (name, (primary_size, primary_header)) in shard_headers {
        let candidate = mirror_path.join(name);
        if !candidate.is_file() {
            missing += 1;
            continue;
        }
        match safetensors_header(&candidate) {
            Ok((mirror_size, mirror_header, _)) => {
                if mirror_size == *primary_size && mirror_header == *primary_header {
                    accepted += 1;
                } else {
                    divergent += 1;
                }
            }
            Err(_) => divergent += 1,
        }
    }
    let (status, summary) = if divergent > 0 {
        (
            "warn",
            "one or more mirror shards would be rejected by the runtime",
        )
    } else if accepted > 0 {
        (
            "pass",
            "mirror shards satisfy runtime size and header admission",
        )
    } else {
        (
            "warn",
            "configured mirror contains no admissible primary shards",
        )
    };
    (
        status.into(),
        summary.into(),
        Some(serde_json::json!({
            "path": mirror_path.display().to_string(),
            "accepted_shards": accepted,
            "missing_shards": missing,
            "divergent_shards": divergent,
            "partial_mirror_allowed": true,
        })),
    )
}

fn push_deep_checks(checks: &mut Vec<DoctorCheck>, deep: &DeepContainerReport) {
    checks.push(check_details(
        "model.container",
        "pass",
        "all tensor headers and layouts are internally consistent",
        deep.container.clone(),
    ));
    if let Some(ref d) = deep.sequence_details {
        checks.push(check_details(
            "model.shard_sequence",
            &deep.sequence_status,
            deep.sequence_summary.clone(),
            d.clone(),
        ));
    } else {
        checks.push(check(
            "model.shard_sequence",
            &deep.sequence_status,
            deep.sequence_summary.clone(),
        ));
    }
    checks.push(check_details(
        "model.required",
        &deep.required_status,
        deep.required_summary.clone(),
        deep.required_details.clone(),
    ));
    if let Some(ref d) = deep.index_details {
        checks.push(check_details(
            "model.index",
            &deep.index_status,
            deep.index_summary.clone(),
            d.clone(),
        ));
    } else {
        checks.push(check(
            "model.index",
            &deep.index_status,
            deep.index_summary.clone(),
        ));
    }
    if let Some(ref d) = deep.mirror_details {
        checks.push(check_details(
            "storage.mirror",
            &deep.mirror_status,
            deep.mirror_summary.clone(),
            d.clone(),
        ));
    } else {
        checks.push(check(
            "storage.mirror",
            &deep.mirror_status,
            deep.mirror_summary.clone(),
        ));
    }
}

fn push_deep_fail(checks: &mut Vec<DoctorCheck>, error: String) {
    checks.push(check("model.container", "fail", error));
    checks.push(check(
        "model.shard_sequence",
        "skip",
        "shard sequence check requires a valid container",
    ));
    checks.push(check(
        "model.required",
        "skip",
        "required-tensor check requires a valid container",
    ));
    checks.push(check(
        "model.index",
        "skip",
        "index check requires a valid container",
    ));
    checks.push(check(
        "storage.mirror",
        "skip",
        "mirror check requires a valid container",
    ));
}

/// Collect a doctor report (standard, or deep when `opts.deep`).
///
/// Port of `doctor.run_doctor`. When `deep: true`, mode is `"deep"` and
/// container/sequence/required/index/mirror checks are appended.
pub fn run_doctor(model: impl AsRef<Path>, opts: &DoctorOptions) -> Result<DoctorReport> {
    let model = model.as_ref();
    let model_str = model.display().to_string();
    let mut checks = Vec::new();
    let mut plan: Option<PlacementPlan> = None;

    // model.path
    if model.is_dir() {
        #[cfg(unix)]
        let readable = {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(model)
                .map(|m| m.permissions().mode() & 0o444 != 0)
                .unwrap_or(false)
        };
        #[cfg(not(unix))]
        let readable = true;
        if readable {
            checks.push(check_details(
                "model.path",
                "pass",
                "model directory is readable",
                serde_json::json!({ "path": model_str }),
            ));
        } else {
            checks.push(check(
                "model.path",
                "fail",
                "model directory is not readable",
            ));
        }
    } else {
        checks.push(check(
            "model.path",
            "fail",
            "model directory does not exist",
        ));
    }

    // config
    let config = model.join("config.json");
    let valid_config = std::fs::read_to_string(&config)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .is_some_and(|v| v.is_object());
    checks.push(check(
        "model.config",
        if valid_config { "pass" } else { "fail" },
        if valid_config {
            "config.json is valid"
        } else {
            "config.json is missing or invalid"
        },
    ));

    let tokenizer = model.join("tokenizer.json");
    checks.push(check(
        "model.tokenizer",
        if tokenizer.is_file() { "pass" } else { "fail" },
        if tokenizer.is_file() {
            "tokenizer.json found"
        } else {
            "tokenizer.json is missing"
        },
    ));

    // persistence
    if model.is_dir() {
        #[cfg(unix)]
        let writable = {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(model)
                .map(|m| m.permissions().mode() & 0o200 != 0)
                .unwrap_or(false)
        };
        #[cfg(not(unix))]
        let writable = true;
        if writable {
            checks.push(check(
                "storage.persistence",
                "pass",
                "model directory can store usage and KV state",
            ));
        } else {
            checks.push(check(
                "storage.persistence",
                "warn",
                "model directory is read-only; disable persistence or change permissions",
            ));
        }
    } else {
        checks.push(check(
            "storage.persistence",
            "skip",
            "persistence requires a model directory",
        ));
    }

    // engine (process binary and/or in-process FFI)
    let engine = resolve_doctor_engine_path(model, opts);
    checks.push(engine_binary_check(
        &engine,
        doctor_in_process_available(opts),
    ));

    let available_memory = opts.available_memory.unwrap_or_else(memory_available);
    let mut detected_gpus = opts.gpus.clone().unwrap_or_else(discover_gpus);
    apply_gpu_memory_classification(&mut detected_gpus, available_memory.max(memory_total()));
    // Process-engine ldd first; when missing/CPU-only, in-process CUDA/HIP
    // embed (ffi_cuda_linked / ffi_hip_linked) still counts as GPU-linked so
    // AMD hosts with ffi-hip are not warned as "CPU-only" solely because the
    // process binary is absent or CPU-built.
    let linkage = resolve_doctor_linkage(&engine, opts);
    let mut selected_gpus = detected_gpus.clone();
    if let Some(ref indices) = opts.gpu_indices {
        let wanted: HashSet<u32> = indices.iter().copied().collect();
        selected_gpus.retain(|g| wanted.contains(&g.index));
    }

    let gpu_disabled = opts.gpu_indices.as_ref().is_some_and(|v| v.is_empty());
    let requested_missing = if let Some(ref indices) = opts.gpu_indices {
        if indices.is_empty() {
            false
        } else {
            let unique: HashSet<u32> = indices.iter().copied().collect();
            selected_gpus.len() != unique.len()
        }
    } else {
        false
    };
    let engine_basename = engine
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_else(|| crate::model::model_arch(model).engine_basename());
    checks.push(accelerator_check(
        &selected_gpus,
        &linkage,
        requested_missing,
        gpu_disabled,
        available_memory,
        engine_basename,
    ));

    let plan_opts = PlanOptions {
        policy: "quality".into(),
        ram_gb: opts.ram_gb,
        context: if opts.context == 0 {
            4096
        } else {
            opts.context
        },
        gpu_indices: opts.gpu_indices.clone(),
        vram_gb: opts.vram_gb,
        available_memory: Some(available_memory),
        available_disk: opts.available_disk,
        gpus: Some(detected_gpus),
        ..Default::default()
    };

    match PlacementPlan::build(model, &plan_opts) {
        Ok(p) => {
            checks.push(check_details(
                "model.shards",
                "pass",
                "safetensors headers are valid",
                serde_json::json!({
                    "shards": p.model.shards,
                    "model_bytes": p.model.model_bytes,
                }),
            ));
            let disk = &p.tiers.disk;
            let disk_status = if disk.available_bytes < GB {
                "warn"
            } else {
                "pass"
            };
            let disk_summary = if disk_status == "warn" {
                "less than 1 GB is free for runtime state"
            } else {
                "model backing store is available"
            };
            checks.push(check("storage.disk", disk_status, disk_summary));

            // Capacity tightness is warn (may run poorly). Fail is reserved for
            // broken install / unreadable model / missing engine path.
            let ram = &p.tiers.ram;
            let (ram_status, ram_summary) = if available_memory == 0 {
                ("warn", "available RAM could not be measured")
            } else if ram.budget_bytes > available_memory {
                ("warn", "planned RAM budget exceeds available memory")
            } else if ram.cache_slots_per_layer < 1 {
                (
                    "warn",
                    "RAM budget cannot hold one expert slot per sparse layer",
                )
            } else {
                ("pass", "RAM budget is viable")
            };
            checks.push(check("memory.ram", ram_status, ram_summary));

            if p.warnings.is_empty() {
                checks.push(check(
                    "placement.plan",
                    "pass",
                    "tier placement has no warnings",
                ));
            } else {
                checks.push(check("placement.plan", "warn", p.warnings.join("; ")));
            }

            if let Some(gbs) = p.ssd_probe_gbs {
                checks.push(check(
                    "storage.ssd_probe",
                    "pass",
                    format!("F_NOCACHE probe: {gbs:.1} GB/s (cached, .coli_ssd)"),
                ));
            } else if let Some(msg) = ssd_probe_pending(&p.ssd_probe_state) {
                checks.push(check("storage.ssd_probe", "skip", msg));
            } else {
                checks.push(check(
                    "storage.ssd_probe",
                    "skip",
                    "no cached probe yet; measured on the first Metal+darwin engine start",
                ));
            }
            plan = Some(p);
        }
        Err(e) => {
            checks.push(check("model.shards", "fail", e.to_string()));
            checks.push(check(
                "storage.disk",
                "skip",
                "storage check requires a valid model",
            ));
            checks.push(check(
                "memory.ram",
                "skip",
                "RAM projection requires a valid model",
            ));
            checks.push(check(
                "placement.plan",
                "skip",
                "placement requires a valid model",
            ));
            checks.push(check(
                "storage.ssd_probe",
                "skip",
                "probe surfacing requires a valid model",
            ));
            let _ = ssd_probe_state(model);
        }
    }

    if opts.deep {
        match deep_container_report(model, opts.mirror_dir.as_deref()) {
            Ok(deep) => push_deep_checks(&mut checks, &deep),
            Err(e) => push_deep_fail(&mut checks, e),
        }
    }

    let statuses: HashSet<&str> = checks.iter().map(|c| c.status.as_str()).collect();
    let status = if statuses.contains("fail") {
        "error"
    } else if statuses.contains("warn") {
        "warning"
    } else {
        "ok"
    };

    Ok(DoctorReport {
        schema_version: 1,
        status: status.into(),
        model: model_str,
        mode: if opts.deep { "deep" } else { "standard" }.into(),
        checks,
        plan,
    })
}

/// Exit code matching Python doctor (1 on error, else 0).
pub fn exit_code(report: &DoctorReport) -> i32 {
    if report.status == "error" { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Write a minimal U8 safetensors shard (port of test_doctor.write_shard).
    fn write_shard(path: &Path, tensors: &[(&str, usize)]) {
        let mut offset = 0usize;
        let mut header = serde_json::Map::new();
        let mut payload = Vec::new();
        for (name, size) in tensors {
            header.insert(
                (*name).into(),
                serde_json::json!({
                    "dtype": "U8",
                    "shape": [*size],
                    "data_offsets": [offset, offset + size],
                }),
            );
            payload.extend(std::iter::repeat_n(0u8, *size));
            offset += size;
        }
        let raw = serde_json::to_vec(&Value::Object(header)).unwrap();
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(&(raw.len() as u64).to_le_bytes()).unwrap();
        f.write_all(&raw).unwrap();
        f.write_all(&payload).unwrap();
    }

    fn fixture_model() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let model = tmp.path().join("model");
        std::fs::create_dir(&model).unwrap();
        std::fs::write(
            model.join("config.json"),
            r#"{"num_hidden_layers":2,"n_routed_experts":2,"kv_lora_rank":4,"qk_rope_head_dim":2,"qk_nope_head_dim":3,"v_head_dim":5,"num_attention_heads":2}"#,
        )
        .unwrap();
        std::fs::write(model.join("tokenizer.json"), "{}").unwrap();
        write_shard(
            &model.join("model.safetensors"),
            &[
                ("model.embed_tokens.weight", 100),
                ("model.norm.weight", 8),
                ("lm_head.weight", 100),
                ("model.layers.0.self_attn.q_a_proj.weight", 200),
                ("model.layers.1.mlp.experts.0.gate_proj.weight", 30),
                ("model.layers.1.mlp.experts.0.up_proj.weight", 30),
                ("model.layers.1.mlp.experts.1.gate_proj.weight", 30),
                ("model.layers.1.mlp.experts.1.up_proj.weight", 30),
            ],
        );
        (tmp, model)
    }

    fn by_id(report: &DoctorReport) -> HashMap<&str, &DoctorCheck> {
        report.checks.iter().map(|c| (c.id.as_str(), c)).collect()
    }

    fn deep_opts(engine: PathBuf) -> DoctorOptions {
        DoctorOptions {
            ram_gb: 16.0,
            context: 32,
            gpu_indices: Some(vec![]),
            engine_path: Some(engine),
            available_memory: Some(32 * GB),
            available_disk: Some(100 * GB),
            deep: true,
            ..Default::default()
        }
    }

    #[test]
    fn doctor_missing_model() {
        let opts = DoctorOptions {
            engine_path: Some(PathBuf::from("/nonexistent-engine-xyz")),
            available_memory: Some(64 * GB),
            available_disk: Some(500 * GB),
            ..Default::default()
        };
        let report = run_doctor("/nonexistent-model-xyz", &opts).unwrap();
        assert_eq!(report.schema_version, 1);
        assert_eq!(report.status, "error");
        assert!(
            report
                .checks
                .iter()
                .any(|c| c.id == "model.path" && c.status == "fail")
        );
    }

    #[test]
    fn deep_validates_every_tensor_layout() {
        let (_tmp, model) = fixture_model();
        let engine = model.parent().unwrap().join("glm");
        std::fs::write(&engine, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&engine).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&engine, perms).unwrap();
        }
        let report = run_doctor(&model, &deep_opts(engine)).unwrap();
        assert_eq!(report.mode, "deep");
        let checks = by_id(&report);
        assert_eq!(checks["model.container"].status, "pass");
        let details = checks["model.container"].details.as_ref().unwrap();
        assert_eq!(details["shards"], 1);
        assert_eq!(details["tensors"], 8);
        assert_eq!(details["payload_hashing"], false);
        assert_eq!(checks["model.shard_sequence"].status, "skip");
        assert_eq!(checks["model.required"].status, "pass");
        assert_eq!(checks["model.index"].status, "skip");
        assert_eq!(checks["storage.mirror"].status, "skip");
    }

    /// Contract: integer index maps (I64/U64) and runtime FP8 dtypes from
    /// `c/st.h` `st_dtype_code` / `st_dtype_esz` are valid for deep layout.
    /// DeepSeek-V4 stores expert routing as I64 `tid2eid`; thorough doctor
    /// must not false-fail on those tensors.
    #[test]
    fn tensor_layout_accepts_i64_routing_table() {
        let meta = serde_json::json!({
            "dtype": "I64",
            "shape": [4, 2],
            "data_offsets": [0, 64],
        });
        assert_eq!(tensor_layout(&meta, 64).unwrap(), (0, 64));
    }

    #[test]
    fn tensor_layout_accepts_u64_and_f8_runtime_dtypes() {
        let u64_meta = serde_json::json!({
            "dtype": "U64",
            "shape": [2],
            "data_offsets": [0, 16],
        });
        assert_eq!(tensor_layout(&u64_meta, 16).unwrap(), (0, 16));

        for dtype in [
            "F8_E4M3",
            "F8_E4M3FN",
            "float8_e4m3fn",
            "F8_E8M0",
            "F8_E8M0FNU",
        ] {
            let meta = serde_json::json!({
                "dtype": dtype,
                "shape": [8],
                "data_offsets": [0, 8],
            });
            assert_eq!(
                tensor_layout(&meta, 8).unwrap(),
                (0, 8),
                "runtime dtype {dtype} must pass layout"
            );
        }
    }

    #[test]
    fn tensor_layout_rejects_unknown_dtype() {
        let meta = serde_json::json!({
            "dtype": "F64",
            "shape": [1],
            "data_offsets": [0, 8],
        });
        let err = tensor_layout(&meta, 8).unwrap_err();
        assert!(
            err.contains("unsupported dtype"),
            "expected unsupported dtype message, got {err}"
        );
    }

    #[test]
    fn tensor_layout_rejects_i64_span_mismatch() {
        let meta = serde_json::json!({
            "dtype": "I64",
            "shape": [4, 2],
            "data_offsets": [0, 32],
        });
        let err = tensor_layout(&meta, 64).unwrap_err();
        assert!(
            err.contains("shape and dtype disagree with the tensor byte span"),
            "expected span mismatch, got {err}"
        );
    }

    #[test]
    fn deep_rejects_overlapping_tensor_ranges() {
        let (_tmp, model) = fixture_model();
        let header = serde_json::json!({
            "first": {"dtype": "U8", "shape": [4], "data_offsets": [0, 4]},
            "second": {"dtype": "U8", "shape": [4], "data_offsets": [2, 6]},
        });
        let raw = serde_json::to_vec(&header).unwrap();
        let mut f = std::fs::File::create(model.join("model.safetensors")).unwrap();
        f.write_all(&(raw.len() as u64).to_le_bytes()).unwrap();
        f.write_all(&raw).unwrap();
        f.write_all(&[0u8; 6]).unwrap();

        let engine = model.parent().unwrap().join("glm");
        std::fs::write(&engine, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&engine).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&engine, perms).unwrap();
        }
        let report = run_doctor(&model, &deep_opts(engine)).unwrap();
        let checks = by_id(&report);
        assert_eq!(checks["model.container"].status, "fail");
        assert!(checks["model.container"].summary.contains("overlap"));
    }

    #[test]
    fn deep_rejects_gap_in_converter_shard_sequence() {
        let (_tmp, model) = fixture_model();
        write_shard(&model.join("out-00000.safetensors"), &[("zero.weight", 1)]);
        write_shard(&model.join("out-00002.safetensors"), &[("two.weight", 1)]);
        let engine = model.parent().unwrap().join("glm");
        std::fs::write(&engine, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&engine).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&engine, perms).unwrap();
        }
        let report = run_doctor(&model, &deep_opts(engine)).unwrap();
        let checks = by_id(&report);
        assert_eq!(checks["model.container"].status, "pass");
        assert_eq!(checks["model.shard_sequence"].status, "fail");
        let d = checks["model.shard_sequence"].details.as_ref().unwrap();
        assert_eq!(d["missing_shards"], 1);
    }

    #[test]
    fn deep_rejects_incomplete_hf_shard_sequence() {
        let (_tmp, model) = fixture_model();
        // Replace single shard with incomplete HF naming (1-of-2 only).
        std::fs::remove_file(model.join("model.safetensors")).unwrap();
        write_shard(
            &model.join("model-00001-of-00002.safetensors"),
            &[
                ("model.embed_tokens.weight", 10),
                ("model.norm.weight", 4),
                ("lm_head.weight", 10),
            ],
        );
        let engine = model.parent().unwrap().join("glm");
        std::fs::write(&engine, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&engine).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&engine, perms).unwrap();
        }
        let report = run_doctor(&model, &deep_opts(engine)).unwrap();
        let checks = by_id(&report);
        assert_eq!(checks["model.shard_sequence"].status, "fail");
        let d = checks["model.shard_sequence"].details.as_ref().unwrap();
        assert_eq!(d["declared_shards"], 2);
        assert_eq!(d["missing_shards"], 1);
    }

    fn amd_gpu_fixture(free: u64, total: u64) -> GpuDevice {
        GpuDevice {
            index: 0,
            name: "AMD Radeon 860M Graphics".into(),
            total_bytes: total,
            free_bytes: free,
            vendor: "amd".into(),
            source: "rocm-smi".into(),
            arch: Some("gfx1152".into()),
            integrated: true,
            ..Default::default()
        }
    }

    fn doctor_with_gpus(
        model: &Path,
        engine: PathBuf,
        gpus: Vec<GpuDevice>,
        linkage: AcceleratorLinkage,
        gpu_indices: Option<Vec<u32>>,
    ) -> DoctorReport {
        let opts = DoctorOptions {
            ram_gb: 16.0,
            context: 32,
            gpu_indices,
            engine_path: Some(engine),
            available_memory: Some(32 * GB),
            available_disk: Some(100 * GB),
            gpus: Some(gpus),
            linkage: Some(linkage),
            ..Default::default()
        };
        run_doctor(model, &opts).unwrap()
    }

    #[test]
    fn accelerator_amd_cpu_engine_warns_without_nvidia_wording() {
        let (_tmp, model) = fixture_model();
        let engine = model.parent().unwrap().join("glm");
        std::fs::write(&engine, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&engine).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&engine, perms).unwrap();
        }
        let total = 4 * GB;
        let free = total / 2;
        let report = doctor_with_gpus(
            &model,
            engine,
            vec![amd_gpu_fixture(free, total)],
            AcceleratorLinkage::default(),
            None,
        );
        let checks = by_id(&report);
        let acc = checks["accelerator.cuda"];
        assert_eq!(acc.status, "warn");
        assert!(
            acc.summary.contains("AMD"),
            "summary should name AMD: {}",
            acc.summary
        );
        assert!(
            !acc.summary.contains("NVIDIA"),
            "must not say NVIDIA for AMD devices: {}",
            acc.summary
        );
        assert!(acc.summary.contains("CPU-only"));
        // Operational next step: rebuild process engine with HIP=1 (not CPU-only forever).
        let details = acc.details.as_ref().expect("details");
        let hint = details["hint"].as_str().expect("hint for AMD CPU-only");
        assert!(
            hint.contains("HIP=1") && hint.contains("make -C c"),
            "hint should tell operator to rebuild process with HIP=1: {hint}"
        );
        assert!(
            hint.contains("COLI_ENGINE") || hint.contains("COLIBRI_ENGINE"),
            "hint should mention engine override env: {hint}"
        );
        // ffi-hip may appear as alternate; process HIP=1 must stay primary.
        assert!(
            hint.find("HIP=1").unwrap()
                < hint
                    .to_ascii_lowercase()
                    .find("ffi-hip")
                    .unwrap_or(usize::MAX),
            "process HIP=1 should be listed before ffi-hip alternate: {hint}"
        );
    }

    #[test]
    fn accelerator_amd_hip_linked_not_cpu_only() {
        let (_tmp, model) = fixture_model();
        let engine = model.parent().unwrap().join("colibri");
        std::fs::write(&engine, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&engine).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&engine, perms).unwrap();
        }
        let linkage = AcceleratorLinkage {
            linked: true,
            missing: false,
            kind: "hip".into(),
        };
        let report = doctor_with_gpus(
            &model,
            engine,
            vec![amd_gpu_fixture(3 * GB, 4 * GB)],
            linkage,
            None,
        );
        let acc = by_id(&report)["accelerator.cuda"];
        assert_eq!(acc.status, "pass", "{}", acc.summary);
        assert!(acc.summary.contains("HIP"), "{}", acc.summary);
        assert!(!acc.summary.contains("CPU-only"), "{}", acc.summary);
        let details = acc.details.as_ref().unwrap();
        assert_eq!(details["linked"], true);
        assert_eq!(details["runtime"], "hip");
        // No rebuild-HIP hint when already linked.
        assert!(
            details.get("hint").is_none(),
            "HIP-linked should not attach rebuild hint: {:?}",
            details.get("hint")
        );
    }

    #[test]
    fn accelerator_amd_hip_pass_and_low_vram_warn() {
        let (_tmp, model) = fixture_model();
        let engine = model.parent().unwrap().join("glm");
        std::fs::write(&engine, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&engine).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&engine, perms).unwrap();
        }
        let linkage = AcceleratorLinkage {
            linked: true,
            missing: false,
            kind: "hip".into(),
        };
        let total = 4 * GB;
        // Healthy free VRAM → pass
        let report = doctor_with_gpus(
            &model,
            engine.clone(),
            vec![amd_gpu_fixture(3 * GB, total)],
            linkage.clone(),
            None,
        );
        let acc = by_id(&report)["accelerator.cuda"];
        assert_eq!(acc.status, "pass", "{}", acc.summary);
        assert!(acc.summary.contains("HIP"));
        assert!(acc.summary.contains("AMD"));
        assert!(!acc.summary.contains("NVIDIA"));

        // Near-zero free on a discrete GPU → warn with compositor note
        let discrete = GpuDevice {
            index: 0,
            name: "AMD Radeon RX 7900 XTX".into(),
            total_bytes: 24 * GB,
            free_bytes: 100 * 1024 * 1024,
            vendor: "amd".into(),
            source: "rocm-smi".into(),
            integrated: false,
            ..Default::default()
        };
        let report = doctor_with_gpus(&model, engine, vec![discrete], linkage, None);
        let acc = by_id(&report)["accelerator.cuda"];
        assert_eq!(acc.status, "warn", "{}", acc.summary);
        assert!(acc.summary.contains("free VRAM"));
        let d = acc.details.as_ref().unwrap();
        assert_eq!(d["low_free_vram"], true);
        assert_eq!(d["runtime"], "hip");
        assert_eq!(d["vendor"], "amd");
        assert_eq!(d["integrated"], false);
    }

    #[test]
    fn accelerator_no_gpu_skips_without_nvidia_wording() {
        let (_tmp, model) = fixture_model();
        let engine = model.parent().unwrap().join("glm");
        std::fs::write(&engine, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&engine).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&engine, perms).unwrap();
        }
        let report = doctor_with_gpus(&model, engine, vec![], AcceleratorLinkage::default(), None);
        let acc = by_id(&report)["accelerator.cuda"];
        assert_eq!(acc.status, "skip");
        assert!(acc.summary.contains("no GPU detected"));
        assert!(!acc.summary.contains("NVIDIA"));
    }

    #[test]
    fn accelerator_missing_hip_runtime_message() {
        let (_tmp, model) = fixture_model();
        let engine = model.parent().unwrap().join("glm");
        std::fs::write(&engine, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&engine).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&engine, perms).unwrap();
        }
        let report = doctor_with_gpus(
            &model,
            engine,
            vec![amd_gpu_fixture(2 * GB, 4 * GB)],
            AcceleratorLinkage {
                linked: false,
                missing: true,
                kind: "hip".into(),
            },
            Some(vec![0]),
        );
        let acc = by_id(&report)["accelerator.cuda"];
        assert_eq!(acc.status, "fail");
        assert!(
            acc.summary.contains("HIP") && acc.summary.contains("libamdhip64"),
            "{}",
            acc.summary
        );
        assert!(!acc.summary.contains("CUDA runtime"));
    }

    #[test]
    fn merge_in_process_hip_counts_when_process_cpu_only() {
        let process = AcceleratorLinkage::default();
        let merged = super::merge_in_process_gpu_linkage(process, true, false);
        assert!(merged.linked);
        assert!(!merged.missing);
        assert_eq!(merged.kind, "hip");
    }

    #[test]
    fn merge_in_process_cuda_counts_when_process_cpu_only() {
        let process = AcceleratorLinkage::default();
        let merged = super::merge_in_process_gpu_linkage(process, false, true);
        assert!(merged.linked);
        assert_eq!(merged.kind, "cuda");
    }

    #[test]
    fn merge_in_process_preserves_process_hip_over_ffi_cuda() {
        let process = AcceleratorLinkage {
            linked: true,
            missing: false,
            kind: "hip".into(),
        };
        let merged = super::merge_in_process_gpu_linkage(process.clone(), false, true);
        assert_eq!(merged, process);
    }

    #[test]
    fn merge_in_process_neither_leaves_cpu() {
        let process = AcceleratorLinkage::default();
        let merged = super::merge_in_process_gpu_linkage(process.clone(), false, false);
        assert_eq!(merged, process);
        assert!(!merged.linked);
    }

    /// End-to-end: injected CPU process linkage stays warn unless merge is used.
    /// When process is CPU-only and doctor injects no linkage, real ffi_hip_linked
    /// is compile-gated; here we prove the accelerator path with HIP linkage
    /// injection (same status doctor gets after merge).
    #[test]
    fn accelerator_amd_with_hip_linkage_not_cpu_only_warn() {
        let (_tmp, model) = fixture_model();
        let engine = model.parent().unwrap().join("glm");
        std::fs::write(&engine, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&engine).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&engine, perms).unwrap();
        }
        // Simulate post-merge state: in-process HIP linked (process path empty).
        let linkage =
            super::merge_in_process_gpu_linkage(AcceleratorLinkage::default(), true, false);
        let report = doctor_with_gpus(
            &model,
            engine,
            vec![amd_gpu_fixture(3 * GB, 4 * GB)],
            linkage,
            None,
        );
        let acc = by_id(&report)["accelerator.cuda"];
        assert_eq!(acc.status, "pass", "{}", acc.summary);
        assert!(acc.summary.contains("HIP"), "{}", acc.summary);
        assert!(
            !acc.summary.contains("CPU-only"),
            "ffi-hip linked must not warn CPU-only: {}",
            acc.summary
        );
    }

    #[test]
    fn accelerator_uma_details_note_shared_memory() {
        let (_tmp, model) = fixture_model();
        let engine = model.parent().unwrap().join("glm");
        std::fs::write(&engine, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&engine).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&engine, perms).unwrap();
        }
        let linkage = AcceleratorLinkage {
            linked: true,
            missing: false,
            kind: "hip".into(),
        };
        let total = 4 * GB;
        let free = 100 * 1024 * 1024;
        let report = doctor_with_gpus(
            &model,
            engine,
            vec![amd_gpu_fixture(free, total)],
            linkage,
            None,
        );
        let acc = by_id(&report)["accelerator.cuda"];
        assert_eq!(
            acc.status, "pass",
            "UMA busy carve-out must not warn: {}",
            acc.summary
        );
        assert!(
            acc.summary.contains("UMA") || acc.summary.contains("shared system memory"),
            "UMA note missing: {}",
            acc.summary
        );
        assert!(
            !acc.summary.contains("near zero") && !acc.summary.contains("carve-out is busy"),
            "UMA must not scare about carve-out as discrete VRAM: {}",
            acc.summary
        );
        let d = acc.details.as_ref().unwrap();
        assert_eq!(d["integrated"], true);
        assert_eq!(d["shared_system_memory"], true);
        assert!(d["system_memory_available_bytes"].as_u64().unwrap() > 0);
        let carve = d["vram_carve_out"].as_array().unwrap();
        assert_eq!(carve.len(), 1);
        assert_eq!(carve[0]["total_bytes"], total);
        assert_eq!(carve[0]["free_bytes"], free);
        assert_eq!(carve[0]["integrated"], true);
    }

    /// UMA + busy BIOS carve-out + large unified RAM + HIP-linked engine:
    /// accelerator and placement must not warn as if the carve-out were discrete VRAM.
    #[test]
    fn uma_busy_carveout_does_not_drive_accelerator_or_plan_warn() {
        let (_tmp, model) = fixture_model();
        let engine = model.parent().unwrap().join("glm");
        std::fs::write(&engine, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&engine).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&engine, perms).unwrap();
        }
        let linkage = AcceleratorLinkage {
            linked: true,
            missing: false,
            kind: "hip".into(),
        };
        let carve_total = (4.3 * GB as f64) as u64;
        let carve_free = (0.4 * GB as f64) as u64;
        let opts = DoctorOptions {
            ram_gb: 40.0,
            context: 32,
            engine_path: Some(engine),
            available_memory: Some(72 * GB),
            available_disk: Some(500 * GB),
            gpus: Some(vec![amd_gpu_fixture(carve_free, carve_total)]),
            linkage: Some(linkage),
            ..Default::default()
        };
        let report = run_doctor(&model, &opts).unwrap();
        let checks = by_id(&report);
        let acc = checks["accelerator.cuda"];
        assert_ne!(
            acc.status, "fail",
            "UMA carve-out must not fail doctor: {}",
            acc.summary
        );
        assert_ne!(
            acc.status, "warn",
            "UMA busy carve-out must not warn accelerator as discrete VRAM: {}",
            acc.summary
        );
        assert!(
            !acc.summary.contains("carve-out is busy"),
            "accelerator must not use carve-out-busy scare: {}",
            acc.summary
        );
        assert!(
            !acc.summary.contains("near zero"),
            "UMA must not scare about carve-out near zero as if it were discrete VRAM: {}",
            acc.summary
        );
        let d = acc.details.as_ref().unwrap();
        assert_eq!(d["integrated"], true);
        assert_eq!(d["shared_system_memory"], true);
        let plan_check = checks["placement.plan"];
        assert!(
            !plan_check.summary.contains("carve-out is busy"),
            "placement.plan must not warn that the BIOS carve-out is busy: {}",
            plan_check.summary
        );
        if let Some(plan) = report.plan.as_ref() {
            assert!(
                plan.tiers.vram.devices[0].usable_bytes > carve_total,
                "plan usable_bytes must be the unified RAM share, not the {} carve-out",
                carve_total
            );
            assert!(
                !plan
                    .warnings
                    .iter()
                    .any(|w| w.contains("carve-out is busy")),
                "plan warnings must omit carve-out-busy scare: {:?}",
                plan.warnings
            );
        }
        let carve_drove_overall = report.status == "warning"
            && report.checks.iter().all(|c| {
                c.status != "warn"
                    || c.summary.contains("carve-out")
                    || c.summary.contains("near zero")
            });
        assert!(
            !carve_drove_overall,
            "carve-out stats must not drive Overall warning by themselves on UMA; status={} checks={:?}",
            report.status,
            report
                .checks
                .iter()
                .filter(|c| c.status == "warn")
                .map(|c| format!("{}: {}", c.id, c.summary))
                .collect::<Vec<_>>()
        );
    }

    /// Doctor `placement.plan` is warn iff `plan.warnings` is non-empty.
    /// Intended SSD overflow belongs in `notes`, so it must not warn
    /// placement.plan or drive Overall by itself.
    #[test]
    fn intended_cold_overflow_does_not_drive_placement_plan_warn() {
        use crate::model::ModelInfo;
        use crate::plan::PlanOptions;

        let info = ModelInfo {
            path: PathBuf::from("/tmp/fake-overflow-model"),
            family: Some(crate::model::ModelFamily::Glm),
            engine_id: "colibri".into(),
            model_type: Some("glm_moe_dsa".into()),
            shards: 1,
            model_bytes: 429 * GB,
            disk_bytes: 429 * GB,
            param_count: Some(1_000_000),
            dense_bytes: 2 * GB,
            expert_bytes: 400 * GB,
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
        };
        let available = (45.0 * GB as f64) as u64;
        let plan = PlacementPlan::build_from_info(
            &info,
            &PlanOptions {
                policy: "quality".into(),
                available_memory: Some(available),
                available_disk: Some(500 * GB),
                gpus: Some(vec![amd_gpu_fixture(
                    (0.4 * GB as f64) as u64,
                    (4.3 * GB as f64) as u64,
                )]),
                physical_cpus: Some(8),
                cpu_sockets: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        const COLD_MISS: &str =
            "cold expert misses may reach disk; normal decode speed depends on hit rate";
        assert!(
            plan.tiers.disk.cold_expert_bytes > 0,
            "fixture must intend overflow"
        );
        assert!(
            plan.notes.iter().any(|n| n == COLD_MISS),
            "intended overflow must be a note: notes={:?} warnings={:?}",
            plan.notes,
            plan.warnings
        );
        assert!(
            !plan
                .warnings
                .iter()
                .any(|w| w.contains("cold expert misses")),
            "intended overflow must not land in plan.warnings (doctor joins those as [warn]): {:?}",
            plan.warnings
        );
        // Same branch as run_doctor: empty warnings → placement.plan pass.
        assert!(
            plan.warnings.is_empty(),
            "notes-only overflow must not drive placement.plan warn / Overall: {:?}",
            plan.warnings
        );
    }

    #[test]
    fn deep_rejects_missing_core_tensor() {
        let (_tmp, model) = fixture_model();
        write_shard(
            &model.join("model.safetensors"),
            &[("model.embed_tokens.weight", 1)],
        );
        let engine = model.parent().unwrap().join("glm");
        std::fs::write(&engine, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&engine).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&engine, perms).unwrap();
        }
        let report = run_doctor(&model, &deep_opts(engine)).unwrap();
        let checks = by_id(&report);
        assert_eq!(checks["model.container"].status, "pass");
        assert_eq!(checks["model.required"].status, "fail");
        let missing = &checks["model.required"].details.as_ref().unwrap()["missing_tensors"];
        assert!(
            missing
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "model.norm.weight")
        );
        assert!(
            missing
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "lm_head.weight")
        );
        // Fail summary must name the missing tensors (operator-facing).
        assert!(
            checks["model.required"]
                .summary
                .contains("model.norm.weight"),
            "summary={}",
            checks["model.required"].summary
        );
        assert!(
            checks["model.required"].summary.contains("lm_head.weight"),
            "summary={}",
            checks["model.required"].summary
        );
    }

    /// Contract: DeepSeek-V4 engines load `embed.weight` / `norm.weight` /
    /// `head.weight` (`c/deepseek_v4.c`), not GLM HF names. Thorough doctor
    /// must not false-fail a complete V4 checkpoint against the GLM list.
    #[test]
    fn required_core_tensors_deepseek_v4_uses_engine_names() {
        use crate::model::ModelFamily;
        let names = required_core_tensors(ModelFamily::DeepseekV4);
        assert_eq!(names, &["embed.weight", "norm.weight", "head.weight"]);
        // Must not require the GLM spellings that false-fail V4 Flash.
        assert!(!names.contains(&"model.embed_tokens.weight"));
        assert!(!names.contains(&"model.norm.weight"));
        assert!(!names.contains(&"lm_head.weight"));
        // Other catalog families keep the HF/GLM list.
        for family in [
            ModelFamily::Glm,
            ModelFamily::Inkling,
            ModelFamily::Kimi,
            ModelFamily::Olmoe,
        ] {
            assert_eq!(
                required_core_tensors(family),
                &[
                    "model.embed_tokens.weight",
                    "model.norm.weight",
                    "lm_head.weight",
                ],
                "family={family:?}"
            );
        }
    }

    #[test]
    fn deep_accepts_deepseek_v4_core_tensor_names() {
        let tmp = tempfile::tempdir().unwrap();
        let model = tmp.path().join("model");
        std::fs::create_dir(&model).unwrap();
        std::fs::write(
            model.join("config.json"),
            r#"{"model_type":"deepseek_v4","num_hidden_layers":2}"#,
        )
        .unwrap();
        std::fs::write(model.join("tokenizer.json"), "{}").unwrap();
        // Synthetic V4-shaped core set (live Flash-0731 uses these names).
        write_shard(
            &model.join("model.safetensors"),
            &[
                ("embed.weight", 64),
                ("norm.weight", 8),
                ("head.weight", 64),
                ("layers.0.attn_norm.weight", 8),
            ],
        );
        let engine = tmp.path().join("deepseek_v4");
        std::fs::write(&engine, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&engine).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&engine, perms).unwrap();
        }
        let report = run_doctor(&model, &deep_opts(engine)).unwrap();
        let checks = by_id(&report);
        assert_eq!(
            checks["model.required"].status, "pass",
            "summary={} details={:?}",
            checks["model.required"].summary, checks["model.required"].details
        );
        let details = checks["model.required"].details.as_ref().unwrap();
        assert_eq!(details["family"], "deepseek_v4");
        assert_eq!(details["missing_tensors"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn deep_rejects_incomplete_deepseek_v4_core_tensors() {
        let tmp = tempfile::tempdir().unwrap();
        let model = tmp.path().join("model");
        std::fs::create_dir(&model).unwrap();
        std::fs::write(model.join("config.json"), r#"{"model_type":"deepseek_v4"}"#).unwrap();
        std::fs::write(model.join("tokenizer.json"), "{}").unwrap();
        // Incomplete: only embed — must still fail (do not weaken incomplete installs).
        write_shard(&model.join("model.safetensors"), &[("embed.weight", 16)]);
        let engine = tmp.path().join("deepseek_v4");
        std::fs::write(&engine, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&engine).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&engine, perms).unwrap();
        }
        let report = run_doctor(&model, &deep_opts(engine)).unwrap();
        let checks = by_id(&report);
        assert_eq!(checks["model.required"].status, "fail");
        let missing = checks["model.required"].details.as_ref().unwrap()["missing_tensors"]
            .as_array()
            .unwrap();
        assert!(missing.iter().any(|v| v == "norm.weight"), "{missing:?}");
        assert!(missing.iter().any(|v| v == "head.weight"), "{missing:?}");
        // Must not claim GLM names are the missing ones on a V4 tree.
        assert!(
            !missing.iter().any(|v| v == "lm_head.weight"),
            "{missing:?}"
        );
        assert!(
            checks["model.required"].summary.contains("norm.weight")
                && checks["model.required"].summary.contains("head.weight"),
            "summary={}",
            checks["model.required"].summary
        );
    }

    #[test]
    fn deep_rejects_glm_names_when_family_is_deepseek_v4() {
        // A V4 config with only GLM core names is still incomplete for the V4 engine.
        let tmp = tempfile::tempdir().unwrap();
        let model = tmp.path().join("model");
        std::fs::create_dir(&model).unwrap();
        std::fs::write(model.join("config.json"), r#"{"model_type":"deepseek_v4"}"#).unwrap();
        std::fs::write(model.join("tokenizer.json"), "{}").unwrap();
        write_shard(
            &model.join("model.safetensors"),
            &[
                ("model.embed_tokens.weight", 16),
                ("model.norm.weight", 8),
                ("lm_head.weight", 16),
            ],
        );
        let engine = tmp.path().join("deepseek_v4");
        std::fs::write(&engine, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&engine).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&engine, perms).unwrap();
        }
        let report = run_doctor(&model, &deep_opts(engine)).unwrap();
        let checks = by_id(&report);
        assert_eq!(checks["model.required"].status, "fail");
        let missing = checks["model.required"].details.as_ref().unwrap()["missing_tensors"]
            .as_array()
            .unwrap();
        assert_eq!(missing.len(), 3, "{missing:?}");
        assert!(missing.iter().any(|v| v == "embed.weight"));
        assert!(missing.iter().any(|v| v == "norm.weight"));
        assert!(missing.iter().any(|v| v == "head.weight"));
    }

    #[test]
    fn deep_validates_model_index() {
        let (_tmp, model) = fixture_model();
        let tensors = [
            "model.embed_tokens.weight",
            "model.norm.weight",
            "lm_head.weight",
            "model.layers.0.self_attn.q_a_proj.weight",
            "model.layers.1.mlp.experts.0.gate_proj.weight",
            "model.layers.1.mlp.experts.0.up_proj.weight",
            "model.layers.1.mlp.experts.1.gate_proj.weight",
            "model.layers.1.mlp.experts.1.up_proj.weight",
        ];
        let mut map = serde_json::Map::new();
        for t in tensors {
            map.insert(t.into(), Value::String("model.safetensors".into()));
        }
        std::fs::write(
            model.join("model.safetensors.index.json"),
            serde_json::to_string(&serde_json::json!({ "weight_map": map })).unwrap(),
        )
        .unwrap();
        let engine = model.parent().unwrap().join("glm");
        std::fs::write(&engine, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&engine).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&engine, perms).unwrap();
        }
        let report = run_doctor(&model, &deep_opts(engine)).unwrap();
        let checks = by_id(&report);
        assert_eq!(checks["model.index"].status, "pass");
        assert_eq!(
            checks["model.index"].details.as_ref().unwrap()["indexed_tensors"],
            8
        );
    }

    #[test]
    fn deep_mirror_pass_and_divergent_warn() {
        let (_tmp, model) = fixture_model();
        let mirror = model.parent().unwrap().join("mirror");
        std::fs::create_dir(&mirror).unwrap();
        std::fs::copy(
            model.join("model.safetensors"),
            mirror.join("model.safetensors"),
        )
        .unwrap();
        let engine = model.parent().unwrap().join("glm");
        std::fs::write(&engine, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&engine).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&engine, perms).unwrap();
        }
        let mut opts = deep_opts(engine.clone());
        opts.mirror_dir = Some(mirror.clone());
        let report = run_doctor(&model, &opts).unwrap();
        let checks = by_id(&report);
        assert_eq!(checks["storage.mirror"].status, "pass");
        assert_eq!(
            checks["storage.mirror"].details.as_ref().unwrap()["accepted_shards"],
            1
        );

        write_shard(
            &mirror.join("model.safetensors"),
            &[("different.weight", 620)],
        );
        let report = run_doctor(&model, &opts).unwrap();
        let checks = by_id(&report);
        assert_eq!(checks["storage.mirror"].status, "warn");
        assert_eq!(
            checks["storage.mirror"].details.as_ref().unwrap()["divergent_shards"],
            1
        );
    }

    #[test]
    fn shard_sequence_hf_pass() {
        let names = vec![
            "model-00001-of-00002.safetensors".into(),
            "model-00002-of-00002.safetensors".into(),
        ];
        let (st, _, d) = shard_sequence_report(&names);
        assert_eq!(st, "pass");
        assert_eq!(d.unwrap()["declared_shards"], 2);
    }

    /// Every [`ModelFamily`] variant (product engines). Glm and Olmoe share
    /// basename `colibri`; still assert both so routing does not drop a family.
    fn all_model_families() -> [crate::model::ModelFamily; 5] {
        use crate::model::ModelFamily;
        [
            ModelFamily::Glm,
            ModelFamily::Inkling,
            ModelFamily::Kimi,
            ModelFamily::DeepseekV4,
            ModelFamily::Olmoe,
        ]
    }

    #[test]
    fn engine_missing_without_ffi_says_model_ready_binary_not_found() {
        let missing = PathBuf::from("/no/such/colibri-engine-binary-xyz");
        let check = engine_binary_check(&missing, false);
        assert_eq!(check.id, "engine.binary");
        assert_eq!(check.status, "fail");
        assert!(
            check.summary.contains("Model files look ready"),
            "summary={}",
            check.summary
        );
        assert!(
            check.summary.contains("process engine binary not found"),
            "summary={}",
            check.summary
        );
        assert!(
            check.summary.contains(missing.to_string_lossy().as_ref()),
            "summary should include path: {}",
            check.summary
        );
        assert!(
            !check.summary.contains("not built"),
            "must not say not built: {}",
            check.summary
        );
        let details = check.details.as_ref().expect("details");
        let hint = details["hint"].as_str().expect("hint string");
        assert!(
            hint.contains("COLIBRI_ENGINE") || hint.contains("COLI_ENGINE"),
            "hint={}",
            hint
        );
        assert!(
            hint.contains("make") || hint.contains("ffi"),
            "hint should mention make or ffi rebuild: {hint}"
        );
    }

    /// Process-only fail summary is family-agnostic: every
    /// [`ModelFamily::engine_basename`] path gets the same shape, not
    /// DeepSeek-only or GLM-only wording.
    #[test]
    fn engine_missing_process_only_fail_summary_all_family_basenames() {
        for family in all_model_families() {
            let base = family.engine_basename();
            let path = PathBuf::from(format!("/no/such/engines/{base}"));
            let check = engine_binary_check(&path, false);
            assert_eq!(check.id, "engine.binary", "family={family:?} base={base}");
            assert_eq!(
                check.status, "fail",
                "family={family:?} summary={}",
                check.summary
            );
            assert!(
                check.summary.contains("Model files look ready"),
                "family={family:?} summary={}",
                check.summary
            );
            assert!(
                check.summary.contains("process engine binary not found"),
                "family={family:?} summary={}",
                check.summary
            );
            assert!(
                check.summary.contains(base),
                "family={family:?} summary must include basename {base}: {}",
                check.summary
            );
            assert!(
                !check.summary.to_lowercase().contains("deepseek") || base.contains("deepseek"),
                "must not hardcode DeepSeek for non-v4 family={family:?}: {}",
                check.summary
            );
            assert!(
                !check.summary.contains("not built"),
                "family={family:?} must not say not built: {}",
                check.summary
            );
            let details = check.details.as_ref().expect("details");
            assert_eq!(
                details["path"].as_str().unwrap_or(""),
                path.to_string_lossy().as_ref(),
                "family={family:?}"
            );
            let hint = details["hint"].as_str().expect("hint");
            assert!(
                hint.contains("COLIBRI_ENGINE") || hint.contains("COLI_ENGINE"),
                "family={family:?} hint={hint}"
            );
            assert!(
                hint.contains("make") || hint.contains("ffi"),
                "family={family:?} hint={hint}"
            );
            // Hint must stay operational and family-neutral (no per-model slogans).
            assert!(
                !hint.to_lowercase().contains("deepseek") && !hint.to_lowercase().contains("glm"),
                "hint must not name a single family: {hint}"
            );
        }
    }

    #[test]
    fn engine_missing_with_ffi_passes_in_process() {
        let missing = PathBuf::from("/no/such/colibri-engine-binary-xyz");
        let check = engine_binary_check(&missing, true);
        assert_eq!(check.id, "engine.binary");
        assert_eq!(check.status, "pass");
        assert!(
            check.summary.to_lowercase().contains("in-process"),
            "summary={}",
            check.summary
        );
        assert!(!check.summary.contains("not built"), "{}", check.summary);
    }

    /// In-process pass when external is missing, for every distinct engine basename.
    #[test]
    fn engine_missing_with_ffi_passes_all_family_basenames() {
        let mut seen = HashSet::new();
        for family in all_model_families() {
            let base = family.engine_basename();
            if !seen.insert(base) {
                continue;
            }
            let path = PathBuf::from(format!("/no/such/engines/{base}"));
            let check = engine_binary_check(&path, true);
            assert_eq!(
                check.status, "pass",
                "family={family:?} base={base} summary={}",
                check.summary
            );
            assert!(
                check.summary.to_lowercase().contains("in-process"),
                "family={family:?} summary={}",
                check.summary
            );
            assert!(
                !check.summary.contains("not built"),
                "family={family:?} {}",
                check.summary
            );
            let mode = check
                .details
                .as_ref()
                .and_then(|d| d.get("mode"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            assert_eq!(mode, "ffi", "family={family:?}");
        }
        // Distinct basenames: colibri, inkling, kimi_k3, deepseek_v4.
        assert_eq!(seen.len(), 4, "expected four distinct basenames: {seen:?}");
    }

    /// Capacity-tight RAM is a warning (may run poorly), not a hard Fail.
    /// Fail is reserved for broken install / unreadable model / no engine.
    #[test]
    fn memory_ram_capacity_tight_is_warn_not_fail() {
        let (_tmp, model) = fixture_model();
        let engine = model.parent().unwrap().join("glm");
        std::fs::write(&engine, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&engine).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&engine, perms).unwrap();
        }
        // 2 GiB free + ram_gb=2 floors the plan budget to 8 GiB → budget > available.
        let opts = DoctorOptions {
            ram_gb: 2.0,
            context: 32,
            engine_path: Some(engine),
            available_memory: Some(2 * GB),
            available_disk: Some(100 * GB),
            gpu_indices: Some(vec![]),
            gpus: Some(vec![]),
            in_process_engine: Some(true),
            ..Default::default()
        };
        let report = run_doctor(&model, &opts).unwrap();
        let checks = by_id(&report);
        let ram = checks["memory.ram"];
        assert_eq!(
            ram.status, "warn",
            "capacity tight must be warn not fail: {}",
            ram.summary
        );
        assert!(
            ram.summary
                .contains("planned RAM budget exceeds available memory")
                || ram
                    .summary
                    .contains("RAM budget cannot hold one expert slot per sparse layer"),
            "unexpected ram summary: {}",
            ram.summary
        );
        // Path / engine healthy → overall Warning, not Fail (error).
        assert_eq!(
            checks["model.path"].status, "pass",
            "{}",
            checks["model.path"].summary
        );
        assert_eq!(
            checks["engine.binary"].status, "pass",
            "{}",
            checks["engine.binary"].summary
        );
        assert!(
            !report.checks.iter().any(|c| c.status == "fail"),
            "no fail checks expected when only capacity is tight; checks={:?}",
            report
                .checks
                .iter()
                .map(|c| (&c.id, &c.status, &c.summary))
                .collect::<Vec<_>>()
        );
        assert_eq!(report.status, "warning", "overall must be warning");
    }

    #[test]
    fn doctor_engine_missing_no_ffi_message_contract() {
        let (_tmp, model) = fixture_model();
        let opts = DoctorOptions {
            engine_path: Some(PathBuf::from("/no/such/colibri-engine-binary-xyz")),
            available_memory: Some(64 * GB),
            available_disk: Some(500 * GB),
            gpu_indices: Some(vec![]),
            in_process_engine: Some(false),
            ..Default::default()
        };
        let report = run_doctor(&model, &opts).unwrap();
        let checks = by_id(&report);
        let eng = checks["engine.binary"];
        assert_eq!(eng.status, "fail");
        assert!(
            eng.summary.contains("Model files look ready")
                && eng.summary.contains("process engine binary not found"),
            "{}",
            eng.summary
        );
        assert!(!eng.summary.contains("not built"), "{}", eng.summary);
        let hint = eng
            .details
            .as_ref()
            .and_then(|d| d.get("hint"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(
            hint.contains("COLIBRI_ENGINE") || hint.contains("COLI_ENGINE"),
            "hint={}",
            hint
        );
    }

    #[test]
    fn doctor_engine_missing_with_in_process_passes() {
        let (_tmp, model) = fixture_model();
        let opts = DoctorOptions {
            engine_path: Some(PathBuf::from("/no/such/colibri-engine-binary-xyz")),
            available_memory: Some(64 * GB),
            available_disk: Some(500 * GB),
            gpu_indices: Some(vec![]),
            in_process_engine: Some(true),
            ..Default::default()
        };
        let report = run_doctor(&model, &opts).unwrap();
        let checks = by_id(&report);
        let eng = checks["engine.binary"];
        assert_eq!(eng.status, "pass", "{}", eng.summary);
        assert!(
            eng.summary.to_lowercase().contains("in-process"),
            "{}",
            eng.summary
        );
        assert!(!eng.summary.contains("not built"), "{}", eng.summary);
        // Engine check must not force overall error by itself.
        assert!(
            !report.checks.iter().any(|c| {
                c.id == "engine.binary" && c.status == "fail" && c.summary.contains("not built")
            }),
            "no not-built fail"
        );
    }

    #[cfg(feature = "ffi")]
    #[test]
    fn doctor_ffi_feature_defaults_to_in_process_when_binary_missing() {
        // Without FORCE_PROCESS, feature=ffi makes in-process available.
        if crate::force_process_from_env() {
            return;
        }
        let (_tmp, model) = fixture_model();
        let opts = DoctorOptions {
            engine_path: Some(PathBuf::from("/no/such/colibri-engine-binary-xyz")),
            available_memory: Some(64 * GB),
            available_disk: Some(500 * GB),
            gpu_indices: Some(vec![]),
            // None → detect from feature + env
            in_process_engine: None,
            ..Default::default()
        };
        let report = run_doctor(&model, &opts).unwrap();
        let eng = by_id(&report)["engine.binary"];
        assert_eq!(eng.status, "pass", "{}", eng.summary);
        assert!(
            eng.summary.to_lowercase().contains("in-process"),
            "{}",
            eng.summary
        );
        assert!(!eng.summary.contains("not built"), "{}", eng.summary);
    }
}
