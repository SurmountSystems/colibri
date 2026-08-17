//! Hardware probing and SSD cache grammar.
//!
//! Port of host logic from `c/resource_plan.py` (functions: `memory_available`,
//! `physical_cpu_count`, `cpu_socket_count`, `discover_gpus`, `parse_ssd_cache`,
//! `ssd_probe_state`), extended for host inventory:
//!
//! - total / available RAM and swap
//! - free (and total) space on the **model store** volume (discoverable default
//!   or `Some(path)` override — see [`crate::paths`])
//! - logical vs physical cores, hybrid (big.LITTLE) topology when exposed
//! - architecture, vendor, model, family/model/stepping generation hints
//! - SIMD / ISA flags (AVX-512, NEON, …) and host NPU (e.g. AMD XDNA via XRT)
//! - relevant shared libraries installed on the host
//!
//! C engines remain subprocesses; this module does not reimplement F_NOCACHE
//! measurement (the engine writes `.coli_ssd`).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::bytes::Regex;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::paths::{self, ModelStoreSource};

/// Decimal GB used by plan math (1e9), matching Python `GB = 1_000_000_000`.
pub const GB: u64 = 1_000_000_000;

/// One GPU device as discovered by nvidia-smi, rocm-smi, or AMD sysfs fallback.
///
/// `total_bytes` / `free_bytes` are always the **device VRAM carve-out** (what
/// rocm-smi/sysfs/nvidia-smi report for VRAM). On unified-memory APUs that
/// carve-out is a small window into system DDR; see [`GpuDevice::integrated`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GpuDevice {
    pub index: u32,
    pub name: String,
    /// Device VRAM carve-out total (not full system RAM on UMA APUs).
    pub total_bytes: u64,
    /// Device VRAM carve-out free.
    pub free_bytes: u64,
    /// Vendor tag: `nvidia`, `amd`, or empty when unknown / fixture omitted.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub vendor: String,
    /// Discovery source: `nvidia-smi`, `rocm-smi`, `sysfs`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    /// Optional architecture id (`gfx1152`, …) when the tool reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    /// True when the GPU shares system memory (UMA / APU / integrated).
    ///
    /// Set by [`apply_gpu_memory_classification`] (heuristics +
    /// `COLI_GPU_MEMORY` override). Plan uses this to budget hot experts from
    /// free system RAM instead of carve-out free − 2 GiB alone.
    #[serde(default)]
    pub integrated: bool,
    /// Optional GTT (host-visible) total from amdgpu sysfs `mem_info_gtt_total`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gtt_total_bytes: Option<u64>,
    /// Optional GTT free (total − used) when sysfs reports used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gtt_free_bytes: Option<u64>,
}

/// Parse `COLI_GPU_MEMORY` value: unified → `Some(true)`, discrete → `Some(false)`.
///
/// Accepts `unified` / `uma` / `integrated` / `shared` and
/// `discrete` / `dgpu` / `vram`. Unknown or empty → `None` (heuristics).
pub fn parse_gpu_memory_mode(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "unified" | "uma" | "integrated" | "shared" => Some(true),
        "discrete" | "dgpu" | "vram" => Some(false),
        _ => None,
    }
}

/// Env override for GPU memory model: `COLI_GPU_MEMORY=unified|discrete`.
///
/// Returns `Some(true)` = force unified, `Some(false)` = force discrete,
/// `None` = use heuristics.
pub fn gpu_memory_mode_override() -> Option<bool> {
    std::env::var("COLI_GPU_MEMORY")
        .ok()
        .as_deref()
        .and_then(parse_gpu_memory_mode)
}

/// AMD / mobile iGPU name patterns (Radeon 860M Graphics, 8060S, …).
///
/// Discrete RX / Instinct product names return false.
pub fn name_looks_like_integrated_gpu(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    if n.contains("instinct") {
        return false;
    }
    // "Radeon RX …" discrete desktop/mobile dGPU lines.
    if n.contains(" rx ") || n.contains("rx ") || n.contains(" radeon rx") {
        return false;
    }
    if n.contains("integrated") || n.contains("igpu") {
        return true;
    }
    // Radeon 860M / 780M / 760M / 890M style mobile iGPU.
    if n.contains("radeon") {
        // Digit run ending in M (860M) or S APU (8060S / 8050S).
        let bytes = n.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i].is_ascii_digit() {
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i > start
                    && i < bytes.len()
                    && (bytes[i] == b'm' || bytes[i] == b's')
                    && i - start >= 3
                {
                    // Digit run ending in M/S with ≥3 digits (860M, 8060S).
                    return true;
                }
            } else {
                i += 1;
            }
        }
        // "AMD Radeon Graphics" / "Radeon 860M Graphics" without RX.
        if n.contains("graphics") && !n.contains("rx") {
            return true;
        }
    }
    false
}

/// True for discrete AMD product lines (RX, Instinct) that must not soft-UMA.
fn name_looks_like_discrete_gpu(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("instinct")
        || n.contains(" rx ")
        || n.contains("rx ")
        || n.contains("radeon rx")
        || n.contains("geforce")
        || n.contains("rtx ")
        || n.contains("quadro")
        || n.contains("tesla")
}

/// Heuristic only (no env override): is this device integrated / UMA?
///
/// Signals (any sufficient path):
/// 1. AMD iGPU name patterns ([`name_looks_like_integrated_gpu`]).
/// 2. Soft: AMD + VRAM ≤ 8 GB + system RAM ≥ 16 GB (not discrete product name).
/// 3. Supporting: AMD + small VRAM + substantial GTT (≥ half of VRAM total).
pub fn infer_gpu_integrated(gpu: &GpuDevice, system_ram_bytes: u64) -> bool {
    if name_looks_like_integrated_gpu(&gpu.name) {
        return true;
    }
    if name_looks_like_discrete_gpu(&gpu.name) {
        return false;
    }
    let small_vram = gpu.total_bytes > 0 && gpu.total_bytes <= 8 * GB;
    let large_ram = system_ram_bytes >= 16 * GB;
    let is_amd = gpu.vendor == "amd"
        || gpu.name.to_ascii_lowercase().contains("amd")
        || gpu.name.to_ascii_lowercase().contains("radeon");
    if is_amd && small_vram && large_ram {
        return true;
    }
    let gtt_support = gpu
        .gtt_total_bytes
        .map(|g| g > 0 && g >= gpu.total_bytes / 2)
        .unwrap_or(false);
    if is_amd && small_vram && gtt_support {
        return true;
    }
    false
}

/// Apply `COLI_GPU_MEMORY` override (always wins) or heuristics to each device.
///
/// Call after discovery with current system RAM (total or available; total is
/// preferred for the ≥ 16 GB soft threshold). When override is set, every
/// device gets the same integrated flag.
pub fn apply_gpu_memory_classification(gpus: &mut [GpuDevice], system_ram_bytes: u64) {
    apply_gpu_memory_classification_with(gpus, system_ram_bytes, gpu_memory_mode_override());
}

/// Like [`apply_gpu_memory_classification`] with an injectable override (tests).
pub fn apply_gpu_memory_classification_with(
    gpus: &mut [GpuDevice],
    system_ram_bytes: u64,
    mode_override: Option<bool>,
) {
    if let Some(forced) = mode_override {
        for g in gpus.iter_mut() {
            g.integrated = forced;
        }
        return;
    }
    for g in gpus.iter_mut() {
        g.integrated = infer_gpu_integrated(g, system_ram_bytes);
    }
}

/// Options for [`MachineInfo::probe_with`].
#[derive(Debug, Clone, Default)]
pub struct ProbeOptions {
    /// Override model-store path for disk probe. `None` = discoverable default
    /// ([`paths::default_model_store_path`] / env / platform).
    ///
    /// Prefer [`MachineInfo::probe_for_config`] when you already hold a
    /// [`crate::ColibriConfig`] so hosts cannot forget the config override.
    pub model_store: Option<PathBuf>,
    /// Extra path to probe for free space (legacy; folded into model store when set alone).
    /// Prefer `model_store`. When both set, `model_store` wins for the primary volume fields.
    pub disk_path: Option<PathBuf>,
}

impl ProbeOptions {
    /// Build probe options from config: copies [`crate::ColibriConfig::model_store`].
    ///
    /// One-liner so hosts wire config → volume free space without hand-plumbing:
    /// `MachineInfo::probe_with(&ProbeOptions::from_config(&cfg))`.
    pub fn from_config(cfg: &crate::ColibriConfig) -> Self {
        Self {
            model_store: cfg.model_store.clone(),
            disk_path: None,
        }
    }
}

impl From<&crate::ColibriConfig> for ProbeOptions {
    fn from(cfg: &crate::ColibriConfig) -> Self {
        Self::from_config(cfg)
    }
}

/// Volume used for model install / default registry root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelStoreVolume {
    /// Resolved absolute or user path (directory may not exist yet).
    pub path: PathBuf,
    /// How the path was chosen.
    pub source: ModelStoreSource,
    /// Free bytes on the filesystem containing `path` (or its nearest existing ancestor).
    pub free_bytes: u64,
    /// Total filesystem size when available.
    pub total_bytes: Option<u64>,
}

/// CPU identity, topology, and ISA features.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CpuInfo {
    /// Kernel architecture string (`x86_64`, `aarch64`, …).
    pub architecture: String,
    pub vendor: Option<String>,
    pub model_name: Option<String>,
    /// CPUID family (x86) when known.
    pub family: Option<u32>,
    /// CPUID model when known.
    pub model: Option<u32>,
    pub stepping: Option<u32>,
    /// Human generation / microarchitecture hint (e.g. `Zen 5 (Strix Point)`).
    pub generation_hint: Option<String>,
    /// SMT: threads per core when known.
    pub threads_per_core: Option<u32>,
    /// Hybrid / big.LITTLE summary when capacities differ or OS reports hybrid.
    pub big_little: Option<BigLittleInfo>,
    /// Curated SIMD / matrix / crypto ISA features with presence flags.
    pub simd: Vec<SimdFeature>,
    /// Additional interesting raw flags not collapsed into `simd` (sorted unique).
    pub isa_flags: Vec<String>,
}

/// Heterogeneous core topology (big.LITTLE / Intel hybrid / etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BigLittleInfo {
    /// True when more than one distinct `cpu_capacity` (or equivalent) is seen.
    pub hybrid: bool,
    /// Distinct capacity values observed (Linux `cpu_capacity`).
    pub capacity_classes: Vec<u32>,
    pub note: String,
}

/// One named SIMD/ISA capability.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SimdFeature {
    /// Canonical name (`AVX512F`, `NEON`, `SVE`, …).
    pub name: String,
    /// Family bucket (`avx512`, `avx2`, `neon`, `sve`, `amx`, `crypto`, …).
    pub family: String,
    pub present: bool,
    /// Optional version / subset note (`vnni`, `bf16`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Neural processing unit / accelerator discovered on the host.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NpuDevice {
    /// Kind tag: `xdna`, `amdxdna`, `intel-npu`, `openvino`, `unknown`, …
    pub kind: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

/// Shared library of interest for inference / GPU / NPU stacks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostLibrary {
    /// Short name (`libcuda`, `libxrt_core`, `libomp`, …).
    pub name: String,
    /// Full path when resolved.
    pub path: String,
    /// Category: `cuda`, `rocm`, `vulkan`, `opencl`, `omp`, `xrt`, `onnx`, `openvino`, `blas`, `other`.
    pub category: String,
}

/// Machine snapshot used by placement planning and host inventory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineInfo {
    /// Reclaimable RAM bytes (MemAvailable / ullAvailPhys / vm_stat sum).
    pub available_memory: u64,
    /// Total physical RAM (MemTotal / ullTotalPhys / hw.memsize).
    pub total_memory: u64,
    /// Total swap / pagefile bytes (0 if none or unknown).
    pub swap_total: u64,
    /// Free swap / pagefile bytes.
    pub swap_free: u64,
    /// Physical CPU cores (not SMT siblings).
    pub physical_cores: u32,
    /// Logical CPUs (hardware threads / SMT).
    pub logical_cores: u32,
    /// CPU sockets (Linux lscpu; else 1).
    pub sockets: u32,
    /// CPU identity, SIMD, hybrid topology.
    pub cpu: CpuInfo,
    /// Discrete GPUs (NVIDIA preferred, else ROCm).
    pub gpus: Vec<GpuDevice>,
    /// NPUs / AI accelerators (XDNA, Intel NPU, …).
    pub npus: Vec<NpuDevice>,
    /// Model-store volume free space (default path or override).
    pub model_store: ModelStoreVolume,
    /// Host libraries relevant to Colibrì / GPU / NPU stacks.
    pub host_libraries: Vec<HostLibrary>,
    /// Free bytes on `disk_path` when an extra legacy path was probed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_free: Option<u64>,
    /// Path used for legacy disk free probe, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_path: Option<PathBuf>,
}

impl MachineInfo {
    /// Probe the local machine using the discoverable default model store path.
    ///
    /// Returns a full host inventory: total/available RAM, swap, physical and
    /// logical cores, CPU architecture/generation/SIMD, GPUs, NPUs, model-store
    /// volume free space, and relevant host libraries. See the public fields on
    /// this struct and nested types.
    pub fn probe() -> Result<Self> {
        Self::probe_with(&ProbeOptions::default())
    }

    /// Probe using the model-store path from [`crate::ColibriConfig`].
    ///
    /// Equivalent to `probe_with(&ProbeOptions::from_config(cfg))`. Prefer this
    /// (or `from_config`) so config `model_store: Some(path)` always sizes free
    /// space on the same volume used for install/registry.
    pub fn probe_for_config(cfg: &crate::ColibriConfig) -> Result<Self> {
        Self::probe_with(&ProbeOptions::from_config(cfg))
    }

    /// Probe machine and measure free disk at an explicit path (legacy helper).
    ///
    /// Prefer [`probe_with`] / [`probe_for_config`] and `ProbeOptions::model_store`.
    pub fn probe_with_disk(disk_path: Option<&Path>) -> Result<Self> {
        let opts = ProbeOptions {
            model_store: disk_path.map(|p| p.to_path_buf()),
            disk_path: None,
        };
        Self::probe_with(&opts)
    }

    /// Full inventory probe with optional model-store override.
    pub fn probe_with(opts: &ProbeOptions) -> Result<Self> {
        let mem = memory_snapshot();
        let physical_cores = physical_cpu_count();
        let logical_cores = logical_cpu_count();
        let sockets = cpu_socket_count();
        let cpu = probe_cpu_info(physical_cores, logical_cores);
        let gpus = discover_gpus();
        let npus = discover_npus();
        let host_libraries = discover_host_libraries();

        let store_arg = opts.model_store.as_deref().or(opts.disk_path.as_deref());
        let (store_path, store_source) = paths::resolve_model_store(store_arg);
        let (store_free, store_total) = disk_usage_bytes(&store_path).unwrap_or((0, None));
        let model_store = ModelStoreVolume {
            path: store_path,
            source: store_source,
            free_bytes: store_free,
            total_bytes: store_total,
        };

        // Legacy fields: only when disk_path was set distinctly from model_store.
        let (disk_free, disk_path) = if opts.disk_path.is_some() && opts.model_store.is_none() {
            (Some(model_store.free_bytes), Some(model_store.path.clone()))
        } else if let Some(ref p) = opts.disk_path {
            if opts.model_store.as_ref().is_some_and(|m| m != p) {
                let free = disk_free_bytes(p).ok();
                (free, Some(p.clone()))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        Ok(Self {
            available_memory: mem.available,
            total_memory: mem.total,
            swap_total: mem.swap_total,
            swap_free: mem.swap_free,
            physical_cores,
            logical_cores,
            sockets,
            cpu,
            gpus,
            npus,
            model_store,
            host_libraries,
            disk_free,
            disk_path,
        })
    }
}

/// Memory counters used internally.
struct MemorySnapshot {
    total: u64,
    available: u64,
    swap_total: u64,
    swap_free: u64,
}

fn memory_snapshot() -> MemorySnapshot {
    if let Some(s) = linux_memory_snapshot() {
        return s;
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(s) = macos_memory_snapshot() {
            return s;
        }
    }
    #[cfg(windows)]
    {
        if let Some(s) = windows_memory_snapshot() {
            return s;
        }
    }
    MemorySnapshot {
        total: 0,
        available: memory_available(),
        swap_total: 0,
        swap_free: 0,
    }
}

fn linux_memory_snapshot() -> Option<MemorySnapshot> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let mut total = 0u64;
    let mut available = 0u64;
    let mut swap_total = 0u64;
    let mut swap_free = 0u64;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let key = parts.next()?;
        let kb: u64 = parts.next()?.parse().ok()?;
        let bytes = kb.saturating_mul(1024);
        match key {
            "MemTotal:" => total = bytes,
            "MemAvailable:" => available = bytes,
            "SwapTotal:" => swap_total = bytes,
            "SwapFree:" => swap_free = bytes,
            _ => {}
        }
    }
    if total == 0 && available == 0 {
        return None;
    }
    Some(MemorySnapshot {
        total,
        available,
        swap_total,
        swap_free,
    })
}

#[cfg(target_os = "macos")]
fn macos_memory_snapshot() -> Option<MemorySnapshot> {
    let available = macos_memory_available().unwrap_or(0);
    let total = Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(0);
    // macOS swap via sysctl vm.swapusage is text; best-effort parse.
    let (swap_total, swap_free) = macos_swap_bytes().unwrap_or((0, 0));
    Some(MemorySnapshot {
        total,
        available,
        swap_total,
        swap_free,
    })
}

#[cfg(target_os = "macos")]
fn macos_swap_bytes() -> Option<(u64, u64)> {
    let out = Command::new("sysctl")
        .args(["-n", "vm.swapusage"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    // e.g. total = 2048.00M  used = 1024.00M  free = 1024.00M
    let parse_m = |label: &str| -> Option<u64> {
        let re = regex::Regex::new(&format!(r"{label}\s*=\s*([\d.]+)([MG])")).ok()?;
        let c = re.captures(&text)?;
        let n: f64 = c.get(1)?.as_str().parse().ok()?;
        let unit = c.get(2)?.as_str();
        let mult = if unit == "G" { 1e9 } else { 1e6 };
        Some((n * mult) as u64)
    };
    let total = parse_m("total")?;
    let free = parse_m("free").unwrap_or(0);
    Some((total, free))
}

#[cfg(windows)]
fn windows_memory_snapshot() -> Option<MemorySnapshot> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(lp_buffer: *mut MemoryStatusEx) -> i32;
    }
    let mut stat = MemoryStatusEx {
        dw_length: std::mem::size_of::<MemoryStatusEx>() as u32,
        ..Default::default()
    };
    let ok = unsafe { GlobalMemoryStatusEx(&mut stat) };
    if ok == 0 {
        return None;
    }
    Some(MemorySnapshot {
        total: stat.ull_total_phys,
        available: stat.ull_avail_phys,
        // page file includes RAM on Windows; report commit charge span as swap-like.
        swap_total: stat.ull_total_page_file.saturating_sub(stat.ull_total_phys),
        swap_free: stat
            .ull_avail_page_file
            .saturating_sub(stat.ull_avail_phys.min(stat.ull_avail_page_file)),
    })
}

/// Available memory in bytes (reclaimable without swapping).
///
/// Port of `resource_plan.memory_available`.
pub fn memory_available() -> u64 {
    if let Some(s) = linux_memory_snapshot() {
        return s.available;
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(v) = macos_memory_available() {
            return v;
        }
    }
    #[cfg(windows)]
    {
        if let Some(v) = windows_memory_available() {
            return v;
        }
    }
    0
}

/// Total physical RAM in bytes.
pub fn memory_total() -> u64 {
    memory_snapshot().total
}

/// Logical CPU count (hardware threads).
pub fn logical_cpu_count() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1)
        .max(1)
}

#[cfg(target_os = "macos")]
fn macos_memory_available() -> Option<u64> {
    let out = Command::new("vm_stat").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let page = {
        let re = regex::Regex::new(r"page size of (\d+) bytes").ok()?;
        re.captures(&text)
            .and_then(|c| c.get(1)?.as_str().parse().ok())
            .unwrap_or(4096u64)
    };
    let mut pages = 0u64;
    for key in [
        "Pages free",
        "Pages inactive",
        "Pages speculative",
        "Pages purgeable",
    ] {
        let re = regex::Regex::new(&format!(r"{key}:\s+(\d+)\.")).ok()?;
        if let Some(c) = re.captures(&text) {
            pages += c.get(1)?.as_str().parse::<u64>().ok()?;
        }
    }
    if pages > 0 {
        return Some(pages * page);
    }
    let out = Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    s.parse().ok()
}

/// Windows available RAM via `GlobalMemoryStatusEx` → `ullAvailPhys`.
///
/// Port of `resource_plan.memory_available` (win32 branch). Same reclaimable
/// definition as C `compat_meminfo`: standby/free/zero pages (no swap).
/// Fallback: `GetPhysicallyInstalledSystemMemory` (total installed, never 0
/// on a real machine) when the primary API returns nothing.
#[cfg(windows)]
fn windows_memory_available() -> Option<u64> {
    windows_memory_available_with(
        windows_global_memory_status_ex,
        windows_physically_installed_kb,
    )
}

/// Full MEMORYSTATUSEX (MSDN); Python's ctypes layout omits page-file fields
/// but the C ABI requires them for a correct `dwLength`.
#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct MemoryStatusEx {
    dw_length: u32,
    dw_memory_load: u32,
    ull_total_phys: u64,
    ull_avail_phys: u64,
    ull_total_page_file: u64,
    ull_avail_page_file: u64,
    ull_total_virtual: u64,
    ull_avail_virtual: u64,
    ull_avail_extended_virtual: u64,
}

#[cfg(windows)]
fn windows_global_memory_status_ex() -> Option<u64> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(lp_buffer: *mut MemoryStatusEx) -> i32;
    }
    let mut stat = MemoryStatusEx {
        dw_length: std::mem::size_of::<MemoryStatusEx>() as u32,
        ..Default::default()
    };
    // SAFETY: kernel32 GlobalMemoryStatusEx writes into a correctly sized
    // MEMORYSTATUSEX with dwLength set.
    let ok = unsafe { GlobalMemoryStatusEx(&mut stat) };
    if ok != 0 && stat.ull_avail_phys > 0 {
        Some(stat.ull_avail_phys)
    } else {
        None
    }
}

#[cfg(windows)]
fn windows_physically_installed_kb() -> Option<u64> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetPhysicallyInstalledSystemMemory(total_memory_in_kilobytes: *mut u64) -> i32;
    }
    let mut total_kb: u64 = 0;
    // SAFETY: pointer is a valid &mut u64 for the duration of the call.
    let ok = unsafe { GetPhysicallyInstalledSystemMemory(&mut total_kb) };
    if ok != 0 && total_kb > 0 {
        Some(total_kb)
    } else {
        None
    }
}

/// Pure selection logic for Windows available memory (unit-testable off-host).
///
/// Primary = `ullAvailPhys` when non-zero; else total installed KB × 1024.
#[cfg(any(windows, test))]
fn windows_memory_available_with(
    primary: impl FnOnce() -> Option<u64>,
    total_kb: impl FnOnce() -> Option<u64>,
) -> Option<u64> {
    if let Some(v) = primary() {
        if v > 0 {
            return Some(v);
        }
    }
    total_kb().map(|kb| kb.saturating_mul(1024))
}

/// Physical CPU core count (not SMT).
///
/// Port of `resource_plan.physical_cpu_count`.
pub fn physical_cpu_count() -> u32 {
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = Command::new("sysctl")
            .args(["-n", "hw.physicalcpu"])
            .output()
        {
            if let Ok(n) = String::from_utf8_lossy(&out.stdout).trim().parse::<u32>() {
                if n > 0 {
                    return n;
                }
            }
        }
    }
    // Linux: lscpu -p=core,socket, dedupe (core, socket).
    if let Ok(out) = Command::new("lscpu").args(["-p=core,socket"]).output() {
        let text = String::from_utf8_lossy(&out.stdout);
        let mut cores = std::collections::HashSet::new();
        for line in text.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split(',').collect();
            if fields.len() < 2 {
                continue;
            }
            let core = fields[fields.len() - 2].parse::<i32>();
            let socket = fields[fields.len() - 1].parse::<i32>();
            if let (Ok(c), Ok(s)) = (core, socket) {
                cores.insert((c, s));
            }
        }
        if !cores.is_empty() {
            return cores.len() as u32;
        }
    }
    let logical = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1);
    if logical == 0 {
        return 1;
    }
    tracing::warn!(
        logical,
        "physical-core probes unavailable; using logical CPUs (SMT may over-subscribe)"
    );
    logical
}

/// CPU socket count.
///
/// Port of `resource_plan.cpu_socket_count`.
pub fn cpu_socket_count() -> u32 {
    if !cfg!(target_os = "linux") {
        return 1;
    }
    if let Ok(out) = Command::new("lscpu").args(["-p=socket"]).output() {
        let text = String::from_utf8_lossy(&out.stdout);
        let mut sockets = std::collections::HashSet::new();
        for line in text.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Ok(s) = line.trim().parse::<i32>() {
                sockets.insert(s);
            }
        }
        if !sockets.is_empty() {
            return sockets.len() as u32;
        }
    }
    1
}

/// Discover GPUs (NVIDIA first, else ROCm / AMD sysfs).
///
/// Port of `resource_plan.discover_gpus`. AMD discovery tries PATH `rocm-smi`,
/// then well-known ROCm install paths, then a best-effort amdgpu sysfs fallback
/// (see [`discover_amd_gpus_sysfs`]). Applies UMA/integrated classification
/// ([`apply_gpu_memory_classification`]) using system RAM total.
pub fn discover_gpus() -> Vec<GpuDevice> {
    let mut devices = {
        let nvidia = discover_nvidia_gpus();
        if !nvidia.is_empty() {
            nvidia
        } else {
            discover_amd_gpus()
        }
    };
    let system_ram = memory_total().max(memory_available());
    apply_gpu_memory_classification(&mut devices, system_ram);
    devices
}

fn discover_nvidia_gpus() -> Vec<GpuDevice> {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=index,name,memory.total,memory.free",
            "--format=csv,noheader,nounits",
        ])
        .output();
    let Ok(out) = out else {
        return vec![];
    };
    if !out.status.success() {
        return vec![];
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut devices = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split(',').map(str::trim).collect();
        if fields.len() != 4 {
            continue;
        }
        let Ok(index) = fields[0].parse::<u32>() else {
            continue;
        };
        let (total_mib, free_mib) = match (fields[2].parse::<u64>(), fields[3].parse::<u64>()) {
            (Ok(t), Ok(f)) => (t, f),
            _ => {
                // Unified-memory [N/A]: fall back to system RAM figures (MiB).
                let total = mem_total_kib().unwrap_or(0) / 1024;
                let free = memory_available() / (1024 * 1024);
                (total, free)
            }
        };
        devices.push(GpuDevice {
            index,
            name: fields[1].to_string(),
            total_bytes: total_mib * 1024 * 1024,
            free_bytes: free_mib * 1024 * 1024,
            vendor: "nvidia".into(),
            source: "nvidia-smi".into(),
            arch: None,
            integrated: false,
            gtt_total_bytes: None,
            gtt_free_bytes: None,
        });
    }
    devices
}

fn mem_total_kib() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            return rest.split_whitespace().next()?.parse().ok();
        }
    }
    None
}

/// Candidate absolute paths for `rocm-smi` when it is not on PATH.
///
/// Order: `ROCM_PATH`, `ROCM_HOME`, `HIP_PATH` (each `…/bin/rocm-smi`), then
/// `/opt/rocm/bin/rocm-smi`. Does not include bare `rocm-smi` (PATH is tried first).
pub fn rocm_smi_path_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for key in ["ROCM_PATH", "ROCM_HOME", "HIP_PATH"] {
        if let Ok(root) = std::env::var(key) {
            if !root.is_empty() {
                out.push(PathBuf::from(root).join("bin").join("rocm-smi"));
            }
        }
    }
    out.push(PathBuf::from("/opt/rocm/bin/rocm-smi"));
    out
}

/// Run rocm-smi CSV inventory: PATH first, then [`rocm_smi_path_candidates`].
fn run_rocm_smi_csv() -> Option<String> {
    let args = ["--showmeminfo", "vram", "--showproductname", "--csv"];
    let try_bin = |bin: &Path| -> Option<String> {
        let out = Command::new(bin).args(args).output().ok()?;
        if !out.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    };
    if let Some(text) = try_bin(Path::new("rocm-smi")) {
        return Some(text);
    }
    for cand in rocm_smi_path_candidates() {
        if cand.is_file() {
            if let Some(text) = try_bin(&cand) {
                return Some(text);
            }
        }
    }
    None
}

/// Parse `rocm-smi --showmeminfo vram --showproductname --csv` stdout.
///
/// Column names drift across ROCm versions; match by substring. Values are
/// already in **bytes** (unlike nvidia-smi MiB). Public for unit fixtures.
pub fn parse_rocm_smi_csv(text: &str) -> Vec<GpuDevice> {
    // Skip leading warning lines until a CSV header with a device column.
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let mut header: Option<&str> = None;
    let mut rest: Vec<&str> = Vec::new();
    for line in lines.by_ref() {
        let low = line.to_ascii_lowercase();
        if low.contains("device") && line.contains(',') {
            header = Some(line);
            rest.extend(lines);
            break;
        }
    }
    let Some(header) = header else {
        return vec![];
    };
    let headers: Vec<&str> = header.split(',').map(str::trim).collect();
    let find_col = |needles: &[&str]| -> Option<usize> {
        headers.iter().position(|h| {
            let low = h.to_lowercase();
            needles.iter().all(|n| low.contains(n))
        })
    };
    let total_i = find_col(&["vram", "total", "memory"]);
    let used_i = find_col(&["vram", "used"]);
    let name_i = find_col(&["card", "series"])
        .or_else(|| find_col(&["card", "model"]))
        .or_else(|| find_col(&["product"]));
    let arch_i = find_col(&["gfx", "version"]).or_else(|| find_col(&["gfx"]));
    let dev_i = headers
        .iter()
        .position(|h| h.to_lowercase().contains("device"));
    let mut devices = Vec::new();
    for (i, line) in rest.into_iter().enumerate() {
        if line.trim().is_empty() || line.starts_with("WARNING:") {
            continue;
        }
        let fields: Vec<&str> = line.split(',').map(str::trim).collect();
        let index = dev_i
            .and_then(|di| fields.get(di))
            .and_then(|d| {
                d.chars()
                    .filter(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .ok()
            })
            .unwrap_or(i as u32);
        let total: u64 = total_i
            .and_then(|ti| fields.get(ti))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let used: u64 = used_i
            .and_then(|ui| fields.get(ui))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let free = total.saturating_sub(used);
        let name = name_i
            .and_then(|ni| fields.get(ni))
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("AMD GPU {index}"));
        let arch = arch_i
            .and_then(|ai| fields.get(ai))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s.to_ascii_lowercase().starts_with("gfx"));
        devices.push(GpuDevice {
            index,
            name,
            total_bytes: total,
            free_bytes: free,
            vendor: "amd".into(),
            source: "rocm-smi".into(),
            arch,
            integrated: false,
            gtt_total_bytes: None,
            gtt_free_bytes: None,
        });
    }
    devices
}

fn discover_amd_gpus() -> Vec<GpuDevice> {
    if let Some(text) = run_rocm_smi_csv() {
        let mut devices = parse_rocm_smi_csv(&text);
        if !devices.is_empty() {
            // Enrich with GTT from sysfs when present (supporting UMA signal).
            enrich_amd_gtt_from_sysfs(&mut devices, Path::new("/sys/class/drm"));
            return devices;
        }
    }
    discover_amd_gpus_sysfs()
}

/// Fill `gtt_*` on AMD devices from amdgpu sysfs when files exist.
///
/// Matches devices by discovery order (HIP ordinal 0..N) against amdgpu DRM
/// cards sorted by card number. Best-effort; leaves fields None on failure.
fn enrich_amd_gtt_from_sysfs(devices: &mut [GpuDevice], drm_root: &Path) {
    let sysfs = discover_amd_gpus_sysfs_from(drm_root);
    for (dst, src) in devices.iter_mut().zip(sysfs.iter()) {
        if dst.gtt_total_bytes.is_none() {
            dst.gtt_total_bytes = src.gtt_total_bytes;
            dst.gtt_free_bytes = src.gtt_free_bytes;
        }
    }
}

/// Best-effort AMD GPU inventory from amdgpu DRM sysfs when `rocm-smi` is missing.
///
/// **Limits:** DRM card ordinals may not match ROCm / HIP device indices used by
/// `COLI_GPU`. Product names are often unavailable (PCI id only). Free VRAM
/// comes from `mem_info_vram_used` and can be approximate under display load.
/// Prefer rocm-smi when present.
pub fn discover_amd_gpus_sysfs() -> Vec<GpuDevice> {
    discover_amd_gpus_sysfs_from(Path::new("/sys/class/drm"))
}

/// Sysfs AMD discovery with injectable DRM class root (tests).
pub fn discover_amd_gpus_sysfs_from(drm_root: &Path) -> Vec<GpuDevice> {
    let entries = match std::fs::read_dir(drm_root) {
        Ok(e) => e,
        Err(_) => return vec![],
    };
    let mut cards: Vec<(u32, PathBuf)> = Vec::new();
    for ent in entries.flatten() {
        let name = ent.file_name();
        let name = name.to_string_lossy();
        // Only primary nodes `cardN`, not `cardN-DP-1` or `renderD*`.
        if let Some(rest) = name.strip_prefix("card") {
            if rest.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(n) = rest.parse::<u32>() {
                    cards.push((n, ent.path()));
                }
            }
        }
    }
    cards.sort_by_key(|(n, _)| *n);
    let mut devices = Vec::new();
    for (card_n, card_path) in cards {
        let device_dir = card_path.join("device");
        let vendor = std::fs::read_to_string(device_dir.join("vendor"))
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        // AMD PCI vendor 0x1002.
        if vendor != "0x1002" && vendor != "1002" {
            continue;
        }
        // Prefer amdgpu driver nodes.
        let driver = std::fs::read_to_string(device_dir.join("uevent"))
            .unwrap_or_default()
            .lines()
            .find_map(|l| l.strip_prefix("DRIVER=").map(str::to_string));
        if let Some(ref d) = driver {
            if d != "amdgpu" {
                continue;
            }
        }
        let total = read_sysfs_u64(&device_dir.join("mem_info_vram_total")).unwrap_or(0);
        if total == 0 {
            continue;
        }
        let used = read_sysfs_u64(&device_dir.join("mem_info_vram_used")).unwrap_or(0);
        let free = total.saturating_sub(used);
        let gtt_total = read_sysfs_u64(&device_dir.join("mem_info_gtt_total"));
        let gtt_used = read_sysfs_u64(&device_dir.join("mem_info_gtt_used")).unwrap_or(0);
        let gtt_free = gtt_total.map(|t| t.saturating_sub(gtt_used));
        let pci_id = std::fs::read_to_string(device_dir.join("device"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let name = pci_id
            .map(|id| format!("AMD GPU (PCI {id}, drm card{card_n})"))
            .unwrap_or_else(|| format!("AMD GPU (drm card{card_n})"));
        // HIP/ROCm ordinal is typically 0..N-1 of amdgpu devices, not DRM card number.
        let index = devices.len() as u32;
        devices.push(GpuDevice {
            index,
            name,
            total_bytes: total,
            free_bytes: free,
            vendor: "amd".into(),
            source: "sysfs".into(),
            arch: None,
            integrated: false,
            gtt_total_bytes: gtt_total,
            gtt_free_bytes: gtt_free,
        });
    }
    devices
}

fn read_sysfs_u64(path: &Path) -> Option<u64> {
    let s = std::fs::read_to_string(path).ok()?;
    s.trim().parse().ok()
}

/// True when free VRAM is near zero relative to total (iGPU / display load).
///
/// Used by doctor/plan messaging. Threshold: free &lt; 256 MiB or free &lt; 5% of total.
pub fn gpu_free_vram_near_zero(g: &GpuDevice) -> bool {
    if g.total_bytes == 0 {
        return g.free_bytes == 0;
    }
    const MIN_FREE: u64 = 256 * 1024 * 1024;
    g.free_bytes < MIN_FREE || g.free_bytes < g.total_bytes / 20
}

/// Free disk bytes for a path (filesystem containing that path).
pub fn disk_free_bytes(path: &Path) -> Result<u64> {
    disk_usage_bytes(path).map(|(free, _)| free)
}

/// Free and optional total bytes for the filesystem of `path`.
///
/// Walks up to the nearest existing ancestor when `path` does not exist yet
/// (typical for a default model store before first install).
pub fn disk_usage_bytes(path: &Path) -> Result<(u64, Option<u64>)> {
    let p = nearest_existing_ancestor(path);
    fs_usage(p)
}

fn nearest_existing_ancestor(path: &Path) -> &Path {
    let mut p = path;
    loop {
        if p.exists() {
            return p;
        }
        match p.parent() {
            Some(parent) if parent != p => p = parent,
            _ => return path,
        }
    }
}

#[cfg(unix)]
fn fs_usage(path: &Path) -> Result<(u64, Option<u64>)> {
    // Linux: size + avail in bytes.
    let out = Command::new("df")
        .args(["-B1", "--output=size,avail"])
        .arg(path)
        .output()
        .map_err(Error::Io)?;
    if out.status.success() {
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines().skip(1) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() >= 2 {
                if let (Ok(size), Ok(avail)) = (fields[0].parse::<u64>(), fields[1].parse::<u64>())
                {
                    return Ok((avail, Some(size)));
                }
            }
        }
    }
    // macOS / fallback: df -k
    let out = Command::new("df")
        .args(["-k", path.to_str().unwrap_or(".")])
        .output()
        .map_err(Error::Io)?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().nth(1).unwrap_or("");
    let fields: Vec<&str> = line.split_whitespace().collect();
    // Filesystem 1024-blocks Used Available Capacity ...
    if fields.len() >= 4 {
        let total = fields[1].parse::<u64>().ok().map(|k| k * 1024);
        let free = fields[3].parse::<u64>().map(|k| k * 1024).map_err(|_| {
            Error::invalid(format!("could not parse disk free for {}", path.display()))
        })?;
        return Ok((free, total));
    }
    Err(Error::invalid(format!(
        "could not parse disk free for {}",
        path.display()
    )))
}

#[cfg(not(unix))]
fn fs_usage(_path: &Path) -> Result<(u64, Option<u64>)> {
    // Windows: injectable via plan fixtures; return a large placeholder.
    Ok((500 * GB, Some(1000 * GB)))
}

// ---------------------------------------------------------------------------
// CPU / SIMD / NPU / libraries
// ---------------------------------------------------------------------------

fn probe_cpu_info(physical: u32, logical: u32) -> CpuInfo {
    let architecture = std::env::consts::ARCH.to_string();
    let (vendor, model_name, family, model, stepping, flags) = read_cpu_identity();
    let threads_per_core = logical
        .checked_div(physical.max(1))
        .map(|t| t.max(1))
        .filter(|_| physical > 0);
    let generation_hint = generation_hint(vendor.as_deref(), family, model, model_name.as_deref());
    let big_little = probe_big_little();
    let (simd, isa_flags) = classify_simd_and_flags(&flags, &architecture);
    CpuInfo {
        architecture,
        vendor,
        model_name,
        family,
        model,
        stepping,
        generation_hint,
        threads_per_core,
        big_little,
        simd,
        isa_flags,
    }
}

type CpuIdentity = (
    Option<String>,
    Option<String>,
    Option<u32>,
    Option<u32>,
    Option<u32>,
    HashSet<String>,
);

fn read_cpu_identity() -> CpuIdentity {
    let mut flags = HashSet::new();
    // lscpu Flags line is richest on Linux.
    if let Ok(out) = Command::new("lscpu").output() {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            let mut vendor = None;
            let mut model_name = None;
            let mut family = None;
            let mut model = None;
            let mut stepping = None;
            for line in text.lines() {
                if let Some(v) = line.strip_prefix("Vendor ID:") {
                    vendor = Some(v.trim().to_string());
                } else if let Some(v) = line.strip_prefix("Model name:") {
                    model_name = Some(v.trim().to_string());
                } else if let Some(v) = line.strip_prefix("CPU family:") {
                    family = v.trim().parse().ok();
                } else if let Some(v) = line.strip_prefix("Model:") {
                    // Avoid "Model name"
                    if !line.contains("name") {
                        model = v.trim().parse().ok();
                    }
                } else if let Some(v) = line.strip_prefix("Stepping:") {
                    stepping = v.trim().parse().ok();
                } else if let Some(v) = line.strip_prefix("Flags:") {
                    for f in v.split_whitespace() {
                        flags.insert(f.to_ascii_lowercase());
                    }
                }
            }
            if !flags.is_empty() || model_name.is_some() {
                return (vendor, model_name, family, model, stepping, flags);
            }
        }
    }
    // /proc/cpuinfo fallback
    if let Ok(text) = std::fs::read_to_string("/proc/cpuinfo") {
        let mut vendor = None;
        let mut model_name = None;
        let mut family = None;
        let mut model = None;
        let mut stepping = None;
        for line in text.lines() {
            if let Some((k, v)) = line.split_once(':') {
                let k = k.trim();
                let v = v.trim();
                match k {
                    "vendor_id" => vendor = Some(v.to_string()),
                    "model name" => model_name = Some(v.to_string()),
                    "cpu family" => family = v.parse().ok(),
                    "model" => model = v.parse().ok(),
                    "stepping" => stepping = v.parse().ok(),
                    "flags" | "Features" => {
                        for f in v.split_whitespace() {
                            flags.insert(f.to_ascii_lowercase());
                        }
                    }
                    _ => {}
                }
            }
            // first cpu block is enough for identity
            if line.is_empty() && model_name.is_some() {
                break;
            }
        }
        return (vendor, model_name, family, model, stepping, flags);
    }
    #[cfg(target_os = "macos")]
    {
        let brand = Command::new("sysctl")
            .args(["-n", "machdep.cpu.brand_string"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty());
        let features = Command::new("sysctl")
            .args(["-n", "machdep.cpu.features"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();
        for f in features.split_whitespace() {
            flags.insert(f.to_ascii_lowercase());
        }
        let leaf7 = Command::new("sysctl")
            .args(["-n", "machdep.cpu.leaf7_features"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();
        for f in leaf7.split_whitespace() {
            flags.insert(f.to_ascii_lowercase());
        }
        return (Some("Apple".into()), brand, None, None, None, flags);
    }
    (None, None, None, None, None, flags)
}

/// Best-effort microarchitecture label for host inventory (not a perfect decoder).
fn generation_hint(
    vendor: Option<&str>,
    family: Option<u32>,
    model: Option<u32>,
    model_name: Option<&str>,
) -> Option<String> {
    let name = model_name.unwrap_or("");
    let vendor = vendor.unwrap_or("");
    if name.contains("Ryzen AI") || (name.contains("Ryzen") && name.contains("AI")) {
        if family == Some(26) {
            return Some("AMD Zen 5 (Strix Point / Ryzen AI 300 series)".into());
        }
        return Some("AMD Ryzen AI".into());
    }
    if vendor.contains("AMD") || name.contains("AMD") {
        // Rough AMD family map
        match family {
            Some(25) => return Some("AMD Zen 3/4 family 19h".into()),
            Some(26) => return Some("AMD Zen 5 family 1Ah".into()),
            Some(23) => return Some("AMD Zen/Zen+/Zen 2 family 17h".into()),
            _ => {
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
        }
    }
    if vendor.contains("Intel") || name.contains("Intel") {
        if name.contains("Ultra") {
            return Some(format!("Intel {name}"));
        }
        if let (Some(f), Some(m)) = (family, model) {
            return Some(format!("Intel family {f} model {m}"));
        }
    }
    if std::env::consts::ARCH == "aarch64" {
        if name.contains("Apple") || vendor.contains("Apple") {
            return Some("Apple Silicon".into());
        }
        return Some("AArch64".into());
    }
    if !name.is_empty() {
        return Some(name.to_string());
    }
    None
}

fn probe_big_little() -> Option<BigLittleInfo> {
    let mut capacities = BTreeSet::new();
    let mut per_cpu = HashMap::new();
    if let Ok(rd) = std::fs::read_dir("/sys/devices/system/cpu") {
        for ent in rd.flatten() {
            let name = ent.file_name().to_string_lossy().into_owned();
            if !name.starts_with("cpu") || !name[3..].chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let cap_path = ent.path().join("cpu_capacity");
            if let Ok(s) = std::fs::read_to_string(&cap_path) {
                if let Ok(c) = s.trim().parse::<u32>() {
                    capacities.insert(c);
                    per_cpu.insert(name, c);
                }
            }
        }
    }
    if capacities.is_empty() {
        return None;
    }
    let hybrid = capacities.len() > 1;
    let classes: Vec<u32> = capacities.iter().copied().collect();
    let note = if hybrid {
        format!(
            "hybrid topology: {} capacity class(es) across {} CPUs",
            classes.len(),
            per_cpu.len()
        )
    } else {
        format!(
            "uniform cpu_capacity={} on {} CPUs (no big.LITTLE capacity split)",
            classes.first().copied().unwrap_or(0),
            per_cpu.len()
        )
    };
    Some(BigLittleInfo {
        hybrid,
        capacity_classes: classes,
        note,
    })
}

fn classify_simd_and_flags(flags: &HashSet<String>, arch: &str) -> (Vec<SimdFeature>, Vec<String>) {
    // (flag key in cpu flags, display name, family, detail)
    let catalog: &[(&str, &str, &str, Option<&str>)] = &[
        ("sse2", "SSE2", "sse", None),
        ("ssse3", "SSSE3", "sse", None),
        ("sse4_1", "SSE4.1", "sse", None),
        ("sse4_2", "SSE4.2", "sse", None),
        ("avx", "AVX", "avx", None),
        ("avx2", "AVX2", "avx2", None),
        ("fma", "FMA3", "avx2", Some("fused multiply-add")),
        ("avx512f", "AVX512F", "avx512", Some("foundation")),
        ("avx512dq", "AVX512DQ", "avx512", None),
        ("avx512bw", "AVX512BW", "avx512", None),
        ("avx512vl", "AVX512VL", "avx512", None),
        ("avx512vnni", "AVX512_VNNI", "avx512", Some("vnni")),
        ("avx512_vnni", "AVX512_VNNI", "avx512", Some("vnni")),
        ("avx512_bf16", "AVX512_BF16", "avx512", Some("bf16")),
        ("avx512bf16", "AVX512_BF16", "avx512", Some("bf16")),
        ("avx512vbmi", "AVX512VBMI", "avx512", None),
        ("avx512_vbmi2", "AVX512_VBMI2", "avx512", None),
        ("avx512_bitalg", "AVX512_BITALG", "avx512", None),
        ("avx512_vpopcntdq", "AVX512_VPOPCNTDQ", "avx512", None),
        ("avx_vnni", "AVX_VNNI", "avx2", Some("vnni")),
        ("amx_tile", "AMX_TILE", "amx", None),
        ("amx_int8", "AMX_INT8", "amx", None),
        ("amx_bf16", "AMX_BF16", "amx", None),
        ("neon", "NEON", "neon", None),
        ("asimd", "ASIMD", "neon", Some("AArch64 Advanced SIMD")),
        ("sve", "SVE", "sve", None),
        ("sve2", "SVE2", "sve", None),
        ("i8mm", "I8MM", "neon", Some("int8 matrix")),
        ("bf16", "BF16", "neon", Some("bf16")),
        ("aes", "AES-NI", "crypto", None),
        ("sha_ni", "SHA_NI", "crypto", None),
        ("sha2", "SHA2", "crypto", None),
        ("sha3", "SHA3", "crypto", None),
        ("sha512", "SHA512", "crypto", None),
        ("pmull", "PMULL", "crypto", None),
    ];

    let mut simd: Vec<SimdFeature> = Vec::new();
    let mut seen_names: HashSet<String> = HashSet::new();
    for (key, name, family, detail) in catalog {
        let present = flags.contains(*key)
            || (arch == "aarch64" && *key == "neon" && flags.contains("asimd"));
        if !seen_names.insert((*name).to_string()) {
            // merge presence if alternate flag spelling
            if present {
                if let Some(s) = simd.iter_mut().find(|s| s.name == *name) {
                    s.present = true;
                }
            }
            continue;
        }
        simd.push(SimdFeature {
            name: (*name).into(),
            family: (*family).into(),
            present,
            detail: detail.map(|s| s.to_string()),
        });
    }

    // Extra interesting flags not in catalog.
    let interesting_prefixes = [
        "avx",
        "sse",
        "amx",
        "sve",
        "neon",
        "asimd",
        "fma",
        "bmi",
        "sha",
        "aes",
        "vnni",
        "bf16",
        "i8mm",
        "rdseed",
        "rdrand",
        "movbe",
        "popcnt",
        "xsaves",
        "serialize",
    ];
    let catalog_keys: HashSet<&str> = catalog.iter().map(|c| c.0).collect();
    let mut isa_flags: Vec<String> = flags
        .iter()
        .filter(|f| {
            !catalog_keys.contains(f.as_str())
                && interesting_prefixes
                    .iter()
                    .any(|p| f.starts_with(p) || f.contains(p))
        })
        .cloned()
        .collect();
    isa_flags.sort();
    (simd, isa_flags)
}

/// Discover NPUs / on-die AI accelerators.
pub fn discover_npus() -> Vec<NpuDevice> {
    let mut out = Vec::new();

    // AMD XDNA / XRT accel devices
    if let Ok(rd) = std::fs::read_dir("/sys/class/accel") {
        for ent in rd.flatten() {
            let name = ent.file_name().to_string_lossy().into_owned();
            let dev_path = format!("/dev/{name}");
            let mut kind = "accel".to_string();
            let mut details = None;
            let modalias = ent.path().join("device/modalias");
            if let Ok(m) = std::fs::read_to_string(&modalias) {
                let m = m.trim();
                if m.contains("1022") || m.to_lowercase().contains("xdna") {
                    kind = "xdna".into();
                }
                details = Some(m.to_string());
            }
            // amdxdna module symlink
            let driver = ent.path().join("device/driver");
            if let Ok(t) = std::fs::read_link(&driver) {
                let t = t.to_string_lossy();
                if t.contains("amdxdna") || t.contains("xdna") {
                    kind = "xdna".into();
                }
                details = Some(format!("{} driver={}", details.unwrap_or_default(), t));
            }
            out.push(NpuDevice {
                kind: kind.clone(),
                name: if kind == "xdna" {
                    "AMD XDNA NPU".into()
                } else {
                    format!("accel {name}")
                },
                device_path: Some(dev_path),
                details,
            });
        }
    }

    // xrt-smi examine (Ryzen AI / Vitis AI)
    if let Ok(xo) = Command::new("xrt-smi").arg("examine").output() {
        if xo.status.success() {
            let text = String::from_utf8_lossy(&xo.stdout);
            if text.contains("RyzenAI") || text.contains("NPU") || text.contains("amdxdna") {
                let fw = text
                    .lines()
                    .find(|l| l.contains("NPU Firmware"))
                    .map(|l| l.trim().to_string());
                let device_line = text
                    .lines()
                    .find(|l| l.contains("RyzenAI") || l.contains("npu"))
                    .map(|l| l.trim().to_string());
                // Avoid duplicate if accel already found
                if !out.iter().any(|n| n.kind == "xdna") {
                    out.push(NpuDevice {
                        kind: "xdna".into(),
                        name: device_line
                            .clone()
                            .unwrap_or_else(|| "AMD Ryzen AI NPU (XDNA)".into()),
                        device_path: None,
                        details: Some(format!(
                            "xrt-smi: {}",
                            fw.or(device_line).unwrap_or_default()
                        )),
                    });
                } else if let Some(n) = out.iter_mut().find(|n| n.kind == "xdna") {
                    let extra = fw.unwrap_or_default();
                    if !extra.is_empty() {
                        n.details = Some(format!(
                            "{}; {}",
                            n.details.clone().unwrap_or_default(),
                            extra
                        ));
                    }
                    if n.name == "AMD XDNA NPU" {
                        if let Some(dl) = device_line {
                            n.name = dl;
                        }
                    }
                }
            }
        }
    }

    // Intel NPU sysfs
    if let Ok(rd) = std::fs::read_dir("/sys/class/accel") {
        let _ = rd; // already scanned
    }
    if Path::new("/dev/accel").exists() {
        // covered via /sys/class/accel
    }

    // OpenVINO / QNN presence as soft NPU software stack markers
    if Command::new("which")
        .arg("openvino")
        .output()
        .ok()
        .is_some_and(|o| o.status.success())
    {
        out.push(NpuDevice {
            kind: "openvino".into(),
            name: "OpenVINO tools present".into(),
            device_path: None,
            details: None,
        });
    }

    out
}

/// Discover host shared libraries useful for inference / GPU / NPU.
pub fn discover_host_libraries() -> Vec<HostLibrary> {
    let mut found: Vec<HostLibrary> = Vec::new();
    let mut seen = HashSet::new();

    let patterns: &[(&str, &str)] = &[
        ("libcuda", "cuda"),
        ("libcudart", "cuda"),
        ("libcublas", "cuda"),
        ("libnccl", "cuda"),
        ("libamdhip64", "rocm"),
        ("libhipblas", "rocm"),
        ("librocblas", "rocm"),
        ("libvulkan", "vulkan"),
        ("libOpenCL", "opencl"),
        ("libomp", "omp"),
        ("libgomp", "omp"),
        ("libiomp", "omp"),
        ("libxrt_core", "xrt"),
        ("libxrt_coreutil", "xrt"),
        ("libxrt_driver_xdna", "xrt"),
        ("libxrt++", "xrt"),
        ("libonnxruntime", "onnx"),
        ("libopenvino", "openvino"),
        ("libmkl", "blas"),
        ("libblas", "blas"),
        ("libopenblas", "blas"),
    ];

    if let Ok(out) = Command::new("ldconfig").arg("-p").output() {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                // libfoo.so (libc6,x86-64) => /usr/lib/libfoo.so
                let lower = line.to_ascii_lowercase();
                for (pat, cat) in patterns {
                    if lower.contains(&pat.to_ascii_lowercase()) {
                        if let Some(path) = line.split("=>").nth(1).map(str::trim) {
                            let name = line.split_whitespace().next().unwrap_or(pat).to_string();
                            let key = format!("{name}|{path}");
                            if seen.insert(key) {
                                found.push(HostLibrary {
                                    name,
                                    path: path.to_string(),
                                    category: (*cat).into(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // Fallback: probe common absolute paths when ldconfig is thin.
    let fallbacks: &[(&str, &str, &str)] = &[
        ("libcuda.so.1", "/usr/lib/libcuda.so.1", "cuda"),
        ("libamdhip64.so", "/opt/rocm/lib/libamdhip64.so", "rocm"),
        (
            "libxrt_driver_xdna.so",
            "/usr/lib/libxrt_driver_xdna.so",
            "xrt",
        ),
        ("libomp.so", "/usr/lib/libomp.so", "omp"),
        ("libvulkan.so.1", "/usr/lib/libvulkan.so.1", "vulkan"),
    ];
    for (name, path, cat) in fallbacks {
        if Path::new(path).exists() {
            let key = format!("{name}|{path}");
            if seen.insert(key) {
                found.push(HostLibrary {
                    name: (*name).into(),
                    path: (*path).into(),
                    category: (*cat).into(),
                });
            }
        }
    }

    found.sort_by(|a, b| a.category.cmp(&b.category).then(a.name.cmp(&b.name)));
    found
}

// ---------------------------------------------------------------------------
// SSD cache grammar (.coli_ssd)
// ---------------------------------------------------------------------------

/// Classification of raw `.coli_ssd` bytes.
///
/// Port of `resource_plan.parse_ssd_cache` — strict grammar shared with C
/// `coli_ssd_cache_parse` (see `c/tests/fixtures/ssd_cache_vectors.txt`).
#[derive(Debug, Clone, PartialEq)]
pub enum SsdCacheParse {
    V2 { gbs: f64, st_dev: u64 },
    Legacy { gbs: f64 },
    Garbage,
}

/// Parse raw `.coli_ssd` bytes under the strict grammar.
pub fn parse_ssd_cache(data: &[u8]) -> SsdCacheParse {
    if data.is_empty() || data.len() > 64 || data.contains(&0) {
        return SsdCacheParse::Garbage;
    }
    // v2: b"v2 <gbs> <st_dev>" optional trailing \n
    // legacy: b"<gbs>" optional trailing \n
    // gbs = digits["."digits], 0 < gbs < 1000; st_dev = 1..20 digits
    static V2: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static LEGACY: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let v2 = V2.get_or_init(|| Regex::new(r"\Av2 (\d+(?:\.\d+)?) (\d{1,20})\n?\z").unwrap());
    let legacy = LEGACY.get_or_init(|| Regex::new(r"\A(\d+(?:\.\d+)?)\n?\z").unwrap());
    if let Some(c) = v2.captures(data) {
        let gbs: f64 = std::str::from_utf8(c.get(1).unwrap().as_bytes())
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(-1.0);
        let dev_s = std::str::from_utf8(c.get(2).unwrap().as_bytes()).unwrap_or("");
        // Reject values that don't fit u64 (vector: 18446744073709551616).
        let dev: Option<u64> = dev_s.parse().ok();
        if let Some(dev) = dev {
            if gbs > 0.0 && gbs < 1000.0 {
                return SsdCacheParse::V2 { gbs, st_dev: dev };
            }
        }
        return SsdCacheParse::Garbage;
    }
    if let Some(c) = legacy.captures(data) {
        let gbs: f64 = std::str::from_utf8(c.get(1).unwrap().as_bytes())
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(-1.0);
        if gbs > 0.0 && gbs < 1000.0 {
            return SsdCacheParse::Legacy { gbs };
        }
    }
    SsdCacheParse::Garbage
}

/// High-level SSD probe state for a model directory.
///
/// Port of `resource_plan.ssd_probe_state`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SsdProbeState {
    /// `ok` | `legacy` | `foreign` | `garbage` | `absent`
    pub state: String,
    /// Trusted GB/s when `state == "ok"`.
    pub gbs: Option<f64>,
}

/// Messages for non-trusted cache states (doctor/plan wording).
pub const SSD_PROBE_PENDING: &[(&str, &str)] = &[
    (
        "legacy",
        "legacy cache pending engine upgrade; re-measured on the next Metal+darwin start",
    ),
    (
        "foreign",
        "cache from another volume; the engine will re-probe here",
    ),
    ("garbage", "unreadable cache; the engine will re-probe"),
];

pub fn ssd_probe_pending(state: &str) -> Option<&'static str> {
    SSD_PROBE_PENDING
        .iter()
        .find(|(k, _)| *k == state)
        .map(|(_, v)| *v)
}

/// Read and classify `<model>/.coli_ssd` (read-only, bounded to 65 bytes).
pub fn ssd_probe_state(model_dir: &Path) -> SsdProbeState {
    let path = model_dir.join(".coli_ssd");
    let mut buf = [0u8; 65];
    let data = match std::fs::File::open(&path) {
        Ok(mut f) => {
            use std::io::Read;
            match f.read(&mut buf) {
                Ok(n) => &buf[..n],
                Err(_) => {
                    return SsdProbeState {
                        state: "absent".into(),
                        gbs: None,
                    };
                }
            }
        }
        Err(_) => {
            return SsdProbeState {
                state: "absent".into(),
                gbs: None,
            };
        }
    };
    match parse_ssd_cache(data) {
        SsdCacheParse::V2 { gbs, st_dev } => {
            let same = volume_dev(model_dir).is_some_and(|d| d == st_dev);
            if same {
                SsdProbeState {
                    state: "ok".into(),
                    gbs: Some(gbs),
                }
            } else {
                SsdProbeState {
                    state: "foreign".into(),
                    gbs: None,
                }
            }
        }
        SsdCacheParse::Legacy { .. } => SsdProbeState {
            state: "legacy".into(),
            gbs: None,
        },
        SsdCacheParse::Garbage => SsdProbeState {
            state: "garbage".into(),
            gbs: None,
        },
    }
}

#[cfg(unix)]
fn volume_dev(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| m.dev())
}

#[cfg(not(unix))]
fn volume_dev(_path: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_v2_basic() {
        let p = parse_ssd_cache(b"v2 9.742 16777233\n");
        match p {
            SsdCacheParse::V2 { gbs, st_dev } => {
                assert!((gbs - 9.742).abs() < 1e-9);
                assert_eq!(st_dev, 16777233);
            }
            _ => panic!("expected v2"),
        }
    }

    #[test]
    fn parse_legacy() {
        match parse_ssd_cache(b"22.139\n") {
            SsdCacheParse::Legacy { gbs } => assert!((gbs - 22.139).abs() < 1e-9),
            _ => panic!("expected legacy"),
        }
    }

    #[test]
    fn reject_inf_and_scientific() {
        assert_eq!(parse_ssd_cache(b"inf\n"), SsdCacheParse::Garbage);
        assert_eq!(parse_ssd_cache(b"1e3\n"), SsdCacheParse::Garbage);
        assert_eq!(parse_ssd_cache(b"+9.5\n"), SsdCacheParse::Garbage);
        assert_eq!(parse_ssd_cache(b"v2 9.5 1\r\n"), SsdCacheParse::Garbage);
    }

    #[test]
    fn reject_nul_and_oversize() {
        assert_eq!(parse_ssd_cache(b"9.5\0"), SsdCacheParse::Garbage);
        let big = vec![b'9'; 65];
        assert_eq!(parse_ssd_cache(&big), SsdCacheParse::Garbage);
    }

    #[test]
    fn machine_probe_smoke() {
        let m = MachineInfo::probe().unwrap();
        assert!(m.physical_cores >= 1);
        assert!(m.logical_cores >= m.physical_cores || m.logical_cores >= 1);
        assert!(m.sockets >= 1);
        assert!(!m.cpu.architecture.is_empty());
        assert!(!m.model_store.path.as_os_str().is_empty());
        // Real hosts with /proc (Linux) or OS APIs report non-zero total RAM.
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        {
            assert!(
                m.total_memory > 0 || m.available_memory > 0,
                "expected total_memory or available_memory > 0 on this host"
            );
        }
        // Public nested fields are reachable (compile-use + smoke).
        let _swap = (m.swap_total, m.swap_free);
        let _cores = (m.physical_cores, m.logical_cores, m.cpu.threads_per_core);
        let _id = (
            m.cpu.vendor.as_deref(),
            m.cpu.model_name.as_deref(),
            m.cpu.generation_hint.as_deref(),
            m.cpu.family,
            m.cpu.model,
            m.cpu.stepping,
        );
        let _bl = m
            .cpu
            .big_little
            .as_ref()
            .map(|b| (b.hybrid, b.capacity_classes.len()));
        let _simd_n = m.cpu.simd.len();
        let _npus = m.npus.len();
        let _libs = m.host_libraries.len();
        let _store = (
            m.model_store.free_bytes,
            m.model_store.total_bytes,
            m.model_store.source,
        );
        let _gpus = m.gpus.len();
        let _legacy = (m.disk_free, m.disk_path.as_ref());
    }

    #[test]
    fn model_store_override_path() {
        let m = MachineInfo::probe_with(&ProbeOptions {
            model_store: Some(PathBuf::from("/tmp")),
            disk_path: None,
        })
        .unwrap();
        assert_eq!(m.model_store.path, PathBuf::from("/tmp"));
        assert_eq!(
            m.model_store.source,
            crate::paths::ModelStoreSource::Override
        );
        assert!(m.model_store.free_bytes > 0 || m.model_store.total_bytes.is_some());
    }

    #[test]
    fn probe_for_config_applies_model_store() {
        let cfg = crate::ColibriConfig::default().model_store("/tmp/colibri-probe-cfg-store");
        let m = MachineInfo::probe_for_config(&cfg).unwrap();
        assert_eq!(
            m.model_store.path,
            PathBuf::from("/tmp/colibri-probe-cfg-store")
        );
        assert_eq!(
            m.model_store.source,
            crate::paths::ModelStoreSource::Override
        );
        // Free bytes walk to nearest existing ancestor (/tmp).
        assert!(m.model_store.free_bytes > 0 || m.model_store.total_bytes.is_some());

        let from_opts = MachineInfo::probe_with(&ProbeOptions::from_config(&cfg)).unwrap();
        assert_eq!(from_opts.model_store.path, m.model_store.path);
        assert_eq!(from_opts.model_store.source, m.model_store.source);
    }

    #[test]
    fn public_inventory_types_are_usable() {
        // Ensures dependents can name nested public fields without private modules.
        let m = MachineInfo::probe().unwrap();
        let store: &ModelStoreVolume = &m.model_store;
        let cpu: &CpuInfo = &m.cpu;
        let _ = store.path.as_os_str();
        let _ = &cpu.architecture;
        for s in &cpu.simd {
            let _: &SimdFeature = s;
            let _ = (
                s.name.as_str(),
                s.family.as_str(),
                s.present,
                s.detail.as_deref(),
            );
        }
        if let Some(bl) = &cpu.big_little {
            let _: &BigLittleInfo = bl;
            let _ = (bl.hybrid, bl.capacity_classes.as_slice(), bl.note.as_str());
        }
        for n in &m.npus {
            let _: &NpuDevice = n;
            let _ = (n.kind.as_str(), n.name.as_str());
        }
        for lib in &m.host_libraries {
            let _: &HostLibrary = lib;
            let _ = (lib.category.as_str(), lib.name.as_str(), lib.path.as_str());
        }
        for g in &m.gpus {
            let _: &GpuDevice = g;
            let _ = (
                g.index,
                g.name.as_str(),
                g.total_bytes,
                g.free_bytes,
                g.vendor.as_str(),
                g.source.as_str(),
                g.arch.as_deref(),
                g.integrated,
                g.gtt_total_bytes,
                g.gtt_free_bytes,
            );
        }
    }

    #[test]
    fn parse_rocm_smi_csv_gfx115x_igpu_fixture() {
        // Live-shaped CSV from Radeon 860M / gfx1152 (bytes, not MiB).
        let csv = "\
WARNING: AMD GPU device(s) is/are in a low-power state. Check power control/runtime_status

device,VRAM Total Memory (B),VRAM Total Used Memory (B),Card Series,Card Model,Card Vendor,Card SKU,Subsystem ID,Device Rev,Node ID,GUID,GFX Version
card0,4294967296,4095787008,AMD Radeon 860M Graphics,0x1114,Advanced Micro Devices Inc. [AMD/ATI],STRIXEMU,0x512f,0xd2,1,47981,gfx1152
";
        let devices = parse_rocm_smi_csv(csv);
        assert_eq!(devices.len(), 1);
        let g = &devices[0];
        assert_eq!(g.index, 0);
        assert_eq!(g.name, "AMD Radeon 860M Graphics");
        assert_eq!(g.total_bytes, 4_294_967_296);
        assert_eq!(g.free_bytes, 4_294_967_296 - 4_095_787_008);
        assert_eq!(g.vendor, "amd");
        assert_eq!(g.source, "rocm-smi");
        assert_eq!(g.arch.as_deref(), Some("gfx1152"));
        assert!(gpu_free_vram_near_zero(g));
        // Classification: APU name + small carve-out + large system RAM → integrated.
        let mut classified = devices;
        apply_gpu_memory_classification(&mut classified, 64 * GB);
        assert!(
            classified[0].integrated,
            "860M fixture must classify as integrated/UMA"
        );
    }

    #[test]
    fn apu_fixture_integrated_discrete_unchanged() {
        let apu = GpuDevice {
            index: 0,
            name: "AMD Radeon 860M Graphics".into(),
            total_bytes: 4 * GB,
            free_bytes: 200 * 1024 * 1024,
            vendor: "amd".into(),
            source: "rocm-smi".into(),
            arch: Some("gfx1152".into()),
            ..Default::default()
        };
        assert!(infer_gpu_integrated(&apu, 48 * GB));
        assert!(name_looks_like_integrated_gpu(&apu.name));

        let discrete = GpuDevice {
            index: 0,
            name: "AMD Radeon RX 7900 XTX".into(),
            total_bytes: 24 * GB,
            free_bytes: 22 * GB,
            vendor: "amd".into(),
            source: "rocm-smi".into(),
            ..Default::default()
        };
        assert!(!name_looks_like_integrated_gpu(&discrete.name));
        assert!(
            !infer_gpu_integrated(&discrete, 64 * GB),
            "large discrete VRAM must not soft-classify as UMA"
        );

        // Soft: small VRAM + large RAM + AMD, even with opaque PCI name.
        let soft = GpuDevice {
            index: 0,
            name: "AMD GPU (PCI 0x1114, drm card0)".into(),
            total_bytes: 4 * GB,
            free_bytes: GB,
            vendor: "amd".into(),
            source: "sysfs".into(),
            ..Default::default()
        };
        assert!(infer_gpu_integrated(&soft, 32 * GB));
        assert!(!infer_gpu_integrated(&soft, 8 * GB)); // system RAM too small
    }

    #[test]
    fn coli_gpu_memory_override_wins() {
        assert_eq!(parse_gpu_memory_mode("unified"), Some(true));
        assert_eq!(parse_gpu_memory_mode("UMA"), Some(true));
        assert_eq!(parse_gpu_memory_mode("discrete"), Some(false));
        assert_eq!(parse_gpu_memory_mode("dgpu"), Some(false));
        assert_eq!(parse_gpu_memory_mode("nope"), None);

        let mut apu = vec![GpuDevice {
            index: 0,
            name: "AMD Radeon 860M Graphics".into(),
            total_bytes: 4 * GB,
            free_bytes: 200 * 1024 * 1024,
            vendor: "amd".into(),
            source: "rocm-smi".into(),
            ..Default::default()
        }];
        apply_gpu_memory_classification_with(&mut apu, 64 * GB, Some(false));
        assert!(
            !apu[0].integrated,
            "override discrete must win over APU name"
        );

        let mut dgpu = vec![GpuDevice {
            index: 0,
            name: "AMD Radeon RX 7900 XTX".into(),
            total_bytes: 24 * GB,
            free_bytes: 22 * GB,
            vendor: "amd".into(),
            ..Default::default()
        }];
        apply_gpu_memory_classification_with(&mut dgpu, 64 * GB, Some(true));
        assert!(dgpu[0].integrated, "override unified must force integrated");
    }

    #[test]
    fn parse_rocm_smi_csv_empty_and_garbage() {
        assert!(parse_rocm_smi_csv("").is_empty());
        assert!(parse_rocm_smi_csv("WARNING: only\n").is_empty());
        assert!(parse_rocm_smi_csv("not,a,gpu,header\n1,2,3,4\n").is_empty());
    }

    #[test]
    fn rocm_smi_path_candidates_include_opt_rocm() {
        let c = rocm_smi_path_candidates();
        assert!(
            c.iter().any(|p| p.ends_with("rocm-smi")),
            "expected rocm-smi paths, got {c:?}"
        );
        assert!(
            c.iter().any(|p| p == Path::new("/opt/rocm/bin/rocm-smi")),
            "expected /opt/rocm/bin/rocm-smi in {c:?}"
        );
    }

    #[test]
    fn sysfs_amd_fallback_from_fixture_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let drm = tmp.path().join("drm");
        // Non-AMD card0
        let card0 = drm.join("card0/device");
        std::fs::create_dir_all(&card0).unwrap();
        std::fs::write(card0.join("vendor"), "0x10de\n").unwrap();
        // AMD card1 with VRAM
        let card1 = drm.join("card1/device");
        std::fs::create_dir_all(&card1).unwrap();
        std::fs::write(card1.join("vendor"), "0x1002\n").unwrap();
        std::fs::write(card1.join("device"), "0x1114\n").unwrap();
        std::fs::write(card1.join("uevent"), "DRIVER=amdgpu\nPCI_ID=1002:1114\n").unwrap();
        std::fs::write(card1.join("mem_info_vram_total"), "4294967296\n").unwrap();
        std::fs::write(card1.join("mem_info_vram_used"), "4000000000\n").unwrap();
        std::fs::write(card1.join("mem_info_gtt_total"), "17179869184\n").unwrap(); // 16 GiB
        std::fs::write(card1.join("mem_info_gtt_used"), "1073741824\n").unwrap();
        // Connector node must be ignored
        std::fs::create_dir_all(drm.join("card1-DP-1")).unwrap();

        let devices = discover_amd_gpus_sysfs_from(&drm);
        assert_eq!(devices.len(), 1);
        let g = &devices[0];
        assert_eq!(g.index, 0); // HIP ordinal, not DRM card number
        assert_eq!(g.vendor, "amd");
        assert_eq!(g.source, "sysfs");
        assert_eq!(g.total_bytes, 4_294_967_296);
        assert_eq!(g.free_bytes, 4_294_967_296 - 4_000_000_000);
        assert!(g.name.contains("0x1114") || g.name.contains("card1"));
        assert_eq!(g.gtt_total_bytes, Some(17_179_869_184));
        assert_eq!(g.gtt_free_bytes, Some(17_179_869_184 - 1_073_741_824));
    }

    #[test]
    fn free_vram_near_zero_thresholds() {
        let full = GpuDevice {
            total_bytes: 4 * 1024 * 1024 * 1024,
            free_bytes: 3 * 1024 * 1024 * 1024,
            ..Default::default()
        };
        assert!(!gpu_free_vram_near_zero(&full));
        let starved = GpuDevice {
            total_bytes: 4 * 1024 * 1024 * 1024,
            free_bytes: 100 * 1024 * 1024,
            ..Default::default()
        };
        assert!(gpu_free_vram_near_zero(&starved));
    }

    #[test]
    fn simd_catalog_mentions_major_families() {
        let m = MachineInfo::probe().unwrap();
        let names: Vec<_> = m.cpu.simd.iter().map(|s| s.name.as_str()).collect();
        // Catalog always lists these names; presence is host-dependent.
        assert!(
            names
                .iter()
                .any(|n| n.starts_with("AVX") || *n == "ASIMD" || *n == "NEON")
        );
        assert!(
            !m.cpu.simd.is_empty(),
            "SIMD catalog on MachineInfo.cpu.simd must not be empty"
        );
    }

    #[test]
    fn windows_memory_prefers_ull_avail_phys() {
        let v = windows_memory_available_with(|| Some(3 * GB), || Some(16 * 1024 * 1024));
        assert_eq!(v, Some(3 * GB));
    }

    #[test]
    fn windows_memory_falls_back_to_installed_total() {
        // Primary empty / zero → installed KB * 1024 (binary KiB, not plan GB=1e9).
        let v = windows_memory_available_with(|| None, || Some(16 * 1024 * 1024));
        assert_eq!(v, Some(16 * 1024 * 1024 * 1024));
        let v = windows_memory_available_with(|| Some(0), || Some(8 * 1024 * 1024));
        assert_eq!(v, Some(8 * 1024 * 1024 * 1024));
    }

    #[test]
    fn windows_memory_none_when_both_fail() {
        assert_eq!(windows_memory_available_with(|| None, || None), None);
    }

    #[cfg(windows)]
    #[test]
    fn windows_memory_available_nonzero_on_host() {
        let v = windows_memory_available().expect("Windows RAM probe should succeed");
        assert!(v > 0, "ullAvailPhys / installed total must be non-zero");
    }
}
