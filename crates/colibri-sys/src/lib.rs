//! # colibri-sys
//!
//! Embeddable host crate for [Colibrì](https://github.com/SurmountSystems/colibri).
//!
//! **Not published to crates.io yet.** Depend on this package with a **path** or
//! **git** dependency until a registry release exists. Local rustdoc:
//! `cargo doc -p colibri-sys --open`. Human docs: crate `docs/user-guide.md` and
//! `docs/README.md`.
//!
//! Inference stays in **C engine subprocesses** (`colibri`, `inkling`, `kimi_k3`,
//! `deepseek_v4`). This crate owns the Rust-side host surface:
//!
//! - typed config + env maps (repo `docs/SETTINGS.md` / `docs/ENVIRONMENT.md`)
//! - machine probe + placement plan v2 (port of `c/resource_plan.py`)
//! - model inspect / registry (family routing from `c/coli`)
//! - doctor standard + deep paths (port of `c/doctor.py`)
//! - serve mux client (repo `docs/serve_protocol.md` / `c/openai_server.py`)
//! - rkyv duplex bridge (`EngineDuplex`) mapping frames onto that mux
//! - minimal chat templates (`render_chat`) so hosts need no Python
//! - visual snapshots (EMAP/HITS/TIERS/HWINFO/PROF)
//! - optional rkyv frame codec (trusted + bytecheck decode) + HF install
//!
//! ## Features
//!
//! | Feature | Default | Purpose |
//! |---------|---------|---------|
//! | `runtime` | on | process spawn + serve mux |
//! | `stream` | on | rkyv duplex frames |
//! | `tokio` | on | async duplex session |
//! | `install` | off | HF multi-shard snapshot orchestration |
//! | `ffi` | off | optional multi-family CPU static link (`libcolibri.a`, `libkimi_k3.a`, `libinkling.a`, `libdeepseek_v4.a`); process serve remains default (see crate `docs/ffi-phase-d.md`) |
//!
//! ## Quick start
//!
//! ```no_run
//! use colibri_sys::{ColibriConfig, MachineInfo, ModelInfo, PlacementPlan, PlanOptions};
//!
//! let machine = MachineInfo::probe()?;
//! let model = ModelInfo::inspect("/path/to/model")?;
//! let plan = PlacementPlan::build_from_info(&model, &PlanOptions {
//!     available_memory: Some(machine.available_memory),
//!     gpus: Some(machine.gpus.clone()),
//!     physical_cpus: Some(machine.physical_cores),
//!     cpu_sockets: Some(machine.sockets),
//!     ..Default::default()
//! })?;
//! let cfg = ColibriConfig::default().model(model.path.clone());
//! let env = cfg.apply_plan(&plan);
//! # Ok::<(), colibri_sys::Error>(())
//! ```
//!
//! ## Public machine inventory (`MachineInfo`)
//!
//! [`MachineInfo::probe`] / [`MachineInfo::probe_with`] / [`MachineInfo::probe_for_config`]
//! fill a public tree re-exported from this crate. Dependents use public fields
//! directly (no private modules).
//!
//! | Area | Public fields / types |
//! |------|------------------------|
//! | RAM | `total_memory`, `available_memory` |
//! | Swap | `swap_total`, `swap_free` |
//! | Cores | `physical_cores`, `logical_cores`, `sockets`, `cpu.threads_per_core` |
//! | CPU identity | `cpu.architecture`, `vendor`, `model_name`, `family`, `model`, `stepping`, `generation_hint` |
//! | big.LITTLE | `cpu.big_little: Option<BigLittleInfo>` (`hybrid`, `capacity_classes`, `note`) |
//! | SIMD / ISA | `cpu.simd: Vec<SimdFeature>`, `cpu.isa_flags` (AVX512*, NEON, SVE, AMX, …) |
//! | GPUs | `gpus: Vec<GpuDevice>` |
//! | NPUs | `npus: Vec<NpuDevice>` (e.g. XDNA / Ryzen AI) |
//! | Model store | `model_store: ModelStoreVolume` (`path`, `source`, `free_bytes`, `total_bytes`) |
//! | Host libs | `host_libraries: Vec<HostLibrary>` |
//!
//! Model-store path: discoverable default, or `ProbeOptions.model_store = Some(path)`,
//! or `ColibriConfig.model_store` via [`MachineInfo::probe_for_config`] /
//! [`ProbeOptions::from_config`].
//!
//! ## Python origins
//!
//! Modules that port Python host logic include a crate-level or module-level
//! comment naming the original file and functions. C engines are never rewritten
//! here; they are located and spawned as subprocesses.

#[cfg(test)]
mod archive_gpu_flavor;

pub mod chat;
pub mod config;
pub mod doctor;
pub mod error;
pub mod linkage;
pub mod model;
pub mod native_log;
pub mod paths;
pub mod plan;
pub mod probe;
pub mod process_priority;
pub mod visual;

#[cfg(feature = "runtime")]
pub mod engine;

#[cfg(feature = "stream")]
pub mod stream;

#[cfg(feature = "ffi")]
pub mod ffi;

pub use chat::{ChatMessage, ChatRenderOptions, ChatRole, render_chat, render_chat_simple};
pub use config::{
    ColibriConfig, EnvMap, FORCE_PROCESS_ENV, Policy, env_force_process, force_process_from_env,
};
pub use doctor::{
    AcceleratorLinkage, DoctorCheck, DoctorOptions, DoctorReport, cuda_linkage, exit_code,
    run_doctor,
};
pub use error::{Error, Result};
pub use linkage::{
    ProcessGpuLinkage, bytes_mention_cuda_runtime, bytes_mention_hip_runtime,
    hip_process_rebuild_next_step, parse_bytes_gpu_markers, parse_ldd_gpu_linkage,
    probe_process_gpu_linkage,
};
pub use model::{
    ModelEntry, ModelFamily, ModelInfo, ModelRegistry, ModelSizeInfo, ModelStatus, SupportedModel,
    model_arch, model_arch_from_type, param_count_from_config, supported_model_by_hf_repo,
    supported_model_by_id, supported_models,
};
pub use native_log::{
    COLIBRI_LOG_ENV, DEFAULT_NATIVE_LOG_FILTER, ProcStatusVm, SessionIdentity, cgroup_leaf,
    format_engine_start_log, format_generate_log, format_session_heartbeat_line,
    linked_engine_flavor, native_log_enabled, native_log_enabled_from, native_log_filter_from,
    parse_proc_status_vm, sanitize_log_text, session_identity_now,
};
pub use paths::{
    EnsureModelDir, MODEL_STORE_ENV_KEYS, ModelStoreSource, NATIVE_LOG_FILE_NAME, default_log_dir,
    default_log_dir_from, default_model_store_path, default_native_log_path,
    default_native_log_path_from, ensure_default_model_store, ensure_log_directory,
    ensure_model_directory, expand_user_path, platform_data_dir, platform_data_dir_from,
    platform_default_model_store, resolve_model_store,
};
pub use plan::{
    ClampExpertCapInput, ExpertCapDecision, PlacementPlan, PlanOptions, PlanPolicy, PlanTiers,
    clamp_expert_cap_for_ram, embed_decode_should_stop, environment_for_plan,
    plan_cannot_hold_one_expert_slot, ram_overcommit_from, ram_overcommit_from_env,
};
pub use probe::{
    BigLittleInfo, CpuInfo, GB, GpuDevice, HostLibrary, MachineInfo, ModelStoreVolume, NpuDevice,
    ProbeOptions, SimdFeature, SsdCacheParse, SsdProbeState, apply_gpu_memory_classification,
    apply_gpu_memory_classification_with, discover_amd_gpus_sysfs, discover_amd_gpus_sysfs_from,
    discover_gpus, discover_host_libraries, discover_npus, disk_free_bytes, disk_usage_bytes,
    gpu_free_vram_near_zero, gpu_memory_mode_override, infer_gpu_integrated, logical_cpu_count,
    memory_available, memory_total, name_looks_like_integrated_gpu, parse_gpu_memory_mode,
    parse_rocm_smi_csv, parse_ssd_cache, physical_cpu_count, rocm_smi_path_candidates,
    ssd_probe_state,
};
pub use process_priority::{
    ENGINE_CHILD_NICE, apply_low_compute_priority, engine_child_nice,
    engine_child_nice_is_elevated, set_current_thread_nice,
};
pub use visual::{
    BinaryPollParts, ExpertHits, ExpertMap, HwinfoSnap, ProfileTurn, Subscribe, TiersSnap,
    VisualSnapshot, decode_hex_bytes, pack_expert_cell, unpack_expert_cell,
};

#[cfg(feature = "runtime")]
pub use engine::{
    DoneStats, EngineHandle, EngineLocate, GenerateRequest, GenerateResult, InFlightGenerate,
    ServeClient, ServeEvent, engine_override_from_env, locate_engine,
};

#[cfg(all(feature = "runtime", feature = "stream"))]
pub use engine::EngineDuplex;

#[cfg(feature = "stream")]
pub use stream::{
    ClientFrame, PROTOCOL_VERSION, ServerFrame, decode_frame, decode_frame_checked, encode_frame,
};

#[cfg(all(feature = "stream", feature = "tokio"))]
pub use stream::{DuplexSession, duplex_pair};

#[cfg(feature = "install")]
pub use model::install;

/// Crate version string.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
