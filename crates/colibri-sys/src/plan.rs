//! Placement plan v2 (disk / RAM / VRAM tiers).
//!
//! Port of host logic from `c/resource_plan.py` (functions: `analyze_model`,
//! `build_plan`, `environment_for_plan`, `_auto_tune`, `POLICIES`).
//!
//! GLM-shaped MoE math first; other families still get inspect + env apply
//! but may warn. Quality doctrine: placement changes speed, not answers
//! (except `experimental-fast`).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::EnvMap;
use crate::error::{Error, Result};
use crate::model::ModelInfo;
use crate::probe::{
    GB, GpuDevice, apply_gpu_memory_classification, cpu_socket_count, discover_gpus,
    disk_free_bytes, memory_available, memory_total, physical_cpu_count, ssd_probe_state,
};

/// Discrete VRAM reserve (headroom for dense + runtime on device).
const VRAM_RESERVE_BYTES: u64 = 2 * GB;
/// OS / display headroom when budgeting hot experts from system RAM on UMA.
const UMA_OS_HEADROOM_BYTES: u64 = 4 * GB;
/// Conservative fraction of free system RAM (after headroom) for hot GPU experts.
const UMA_HOT_FRACTION: f64 = 0.5;

/// Options for [`PlacementPlan::build`].
#[derive(Debug, Clone)]
pub struct PlanOptions {
    pub policy: String,
    pub ram_gb: f64,
    pub context: u32,
    pub gpu_indices: Option<Vec<u32>>,
    pub vram_gb: f64,
    /// Injected fixtures (tests / hosts that already probed).
    pub available_memory: Option<u64>,
    pub available_disk: Option<u64>,
    pub gpus: Option<Vec<GpuDevice>>,
    pub physical_cpus: Option<u32>,
    pub cpu_sockets: Option<u32>,
}

impl Default for PlanOptions {
    fn default() -> Self {
        Self {
            policy: "quality".into(),
            ram_gb: 0.0,
            context: 4096,
            gpu_indices: None,
            vram_gb: 0.0,
            available_memory: None,
            available_disk: None,
            gpus: None,
            physical_cpus: None,
            cpu_sockets: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanPolicy {
    pub name: String,
    pub preserve_quantization: bool,
    pub preserve_router: bool,
    pub quality_preserving: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlanModelSummary {
    pub path: String,
    /// Family string (`glm`, `kimi`, `deepseek_v4`, …) when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// Engine binary basename (`colibri`, `kimi_k3`, …).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub engine_id: String,
    pub shards: usize,
    /// Weight size on disk (bytes). Same as [`Self::disk_bytes`].
    pub model_bytes: u64,
    /// Weight size on disk (bytes). Prefer this name in new hosts.
    #[serde(default)]
    pub disk_bytes: u64,
    /// Parameter count when declared in model config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param_count: Option<u64>,
    pub dense_bytes: u64,
    pub expert_bytes: u64,
    pub expert_count: usize,
    pub expert_layers: usize,
    pub typical_expert_bytes: u64,
    pub per_cap_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanCpu {
    pub physical_cores: u32,
    pub sockets: u32,
    pub thread_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TierDisk {
    pub role: String,
    pub model_bytes: u64,
    pub available_bytes: u64,
    pub cold_expert_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TierRam {
    pub role: String,
    pub available_bytes: u64,
    pub budget_bytes: u64,
    pub dense_bytes: u64,
    pub runtime_bytes: u64,
    pub expert_cache_bytes: u64,
    pub warm_expert_bytes: u64,
    pub cache_slots_per_layer: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierVram {
    pub role: String,
    pub devices: Vec<GpuPlanDevice>,
    pub budget_bytes: u64,
    pub hot_expert_bytes: u64,
    pub expert_capacity: u64,
    pub requires_host_backing: bool,
}

impl Default for TierVram {
    fn default() -> Self {
        Self {
            role: "hot-experts".into(),
            devices: vec![],
            budget_bytes: 0,
            hot_expert_bytes: 0,
            expert_capacity: 0,
            requires_host_backing: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuPlanDevice {
    pub index: u32,
    pub name: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub reserve_bytes: u64,
    pub usable_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlanTiers {
    pub disk: TierDisk,
    pub ram: TierRam,
    pub vram: TierVram,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuneEntry {
    pub value: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanDecision {
    pub target: String,
    pub reason: String,
}

/// Placement plan version 2 JSON (compatible with `coli plan --json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementPlan {
    pub version: u32,
    pub policy: PlanPolicy,
    pub model: PlanModelSummary,
    pub cpu: PlanCpu,
    pub tiers: PlanTiers,
    pub expected_bottleneck: String,
    pub bottleneck_class: String,
    pub projected_hit_rate: f64,
    pub tune: BTreeMap<String, TuneEntry>,
    pub decisions: Vec<PlanDecision>,
    pub warnings: Vec<String>,
    /// Informational plan notes (not warnings). Native Memory plan must not
    /// prefix these as `Warning:`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    pub ssd_probe_gbs: Option<f64>,
    pub ssd_probe_state: String,
}

fn policy_flags(name: &str) -> Result<(bool, bool, bool)> {
    match name {
        "quality" | "balanced" => Ok((true, true, true)),
        "experimental-fast" => Ok((false, false, false)),
        _ => Err(Error::Plan(format!("unknown policy: {name}"))),
    }
}

impl PlacementPlan {
    /// Build a plan from an already-inspected model + options.
    pub fn build_from_info(info: &ModelInfo, opts: &PlanOptions) -> Result<Self> {
        let (pq, pr, qp) = policy_flags(&opts.policy)?;
        let quality_preserving = opts.policy != "experimental-fast" && qp;

        let physical_cpus = opts.physical_cpus.unwrap_or_else(physical_cpu_count);
        let cpu_sockets = opts.cpu_sockets.unwrap_or_else(cpu_socket_count);
        let available_memory = opts.available_memory.unwrap_or_else(memory_available);
        let available_disk = match opts.available_disk {
            Some(d) => d,
            None => disk_free_bytes(&info.path).unwrap_or(500 * GB),
        };
        let mut gpus = opts.gpus.clone().unwrap_or_else(discover_gpus);
        // Re-apply override/heuristics so COLI_GPU_MEMORY wins even on fixtures,
        // and injected inventories without integrated flags still classify.
        apply_gpu_memory_classification(&mut gpus, available_memory.max(memory_total()));
        if let Some(ref indices) = opts.gpu_indices {
            let wanted: std::collections::HashSet<u32> = indices.iter().copied().collect();
            gpus.retain(|g| wanted.contains(&g.index));
        }

        let ram_budget = if opts.ram_gb > 0.0 {
            (opts.ram_gb * GB as f64) as u64
        } else {
            (available_memory as f64 * 0.88) as u64
        };
        let ram_budget = if ram_budget < 4 * GB {
            8 * GB
        } else {
            ram_budget
        };

        let cfg = &info.config;
        let typical = info.typical_expert_bytes;
        let layers = cfg_u64(cfg, "num_hidden_layers") + 1;
        let context = opts.context as u64;
        let kv_bytes = layers
            * context
            * (cfg_u64(cfg, "kv_lora_rank") + cfg_u64(cfg, "qk_rope_head_dim"))
            * 4;
        let kv_buffer = context
            * cfg_u64(cfg, "num_attention_heads")
            * (cfg_u64(cfg, "qk_nope_head_dim") + cfg_u64(cfg, "v_head_dim"))
            * 4;
        let runtime_bytes =
            (1.2 * GB as f64 + 2.5 * GB as f64) as u64 + 64 * typical + kv_bytes + kv_buffer;
        let cache_bytes = ram_budget
            .saturating_sub(info.dense_bytes)
            .saturating_sub(runtime_bytes);
        let per_cap = info.per_cap_bytes;
        let configured_experts = cfg_u64(cfg, "n_routed_experts");

        let any_uma = gpus.iter().any(|g| g.integrated);
        let n_integrated = gpus.iter().filter(|g| g.integrated).count().max(1) as u64;
        // Shared-pool hot budget from free system RAM (conservative). Not the
        // tiny VRAM carve-out free − 2 GiB path used for discrete GPUs.
        let uma_shared_hot = if any_uma {
            let free_after_headroom = available_memory.saturating_sub(UMA_OS_HEADROOM_BYTES);
            ((free_after_headroom as f64) * UMA_HOT_FRACTION) as u64
        } else {
            0
        };

        let mut gpu_plan = Vec::new();
        let mut safe_vram: u64 = 0;
        for gpu in &gpus {
            let discrete_usable = gpu.free_bytes.saturating_sub(VRAM_RESERVE_BYTES);
            let usable = if gpu.integrated {
                // Per-device share of the UMA pool; never less than discrete usable
                // if the carve-out happens to have room.
                let share = uma_shared_hot / n_integrated;
                share.max(discrete_usable)
            } else {
                discrete_usable
            };
            safe_vram += usable;
            gpu_plan.push(GpuPlanDevice {
                index: gpu.index,
                name: gpu.name.clone(),
                total_bytes: gpu.total_bytes,
                free_bytes: gpu.free_bytes,
                reserve_bytes: VRAM_RESERVE_BYTES,
                usable_bytes: usable,
            });
        }
        let requested_vram = if opts.vram_gb > 0.0 {
            (opts.vram_gb * GB as f64) as u64
        } else {
            safe_vram
        };
        let vram_budget = requested_vram.min(safe_vram).min(info.expert_bytes);
        let vram_experts = vram_budget.checked_div(typical).unwrap_or(0);
        let hot_bytes = info.expert_bytes.min(vram_experts * typical);
        // UMA double-count fix (planner mirror of engine #653): hot experts live
        // in the same physical DDR pool as warm RAM cache.
        let warm_cap = if any_uma {
            cache_bytes.saturating_sub(hot_bytes)
        } else {
            cache_bytes
        };
        let mut cap = warm_cap.checked_div(per_cap).unwrap_or(0);
        if configured_experts > 0 {
            cap = cap.min(configured_experts);
        }
        let warm_bytes = info.expert_bytes.saturating_sub(hot_bytes).min(warm_cap);
        let cold_bytes = info
            .expert_bytes
            .saturating_sub(hot_bytes)
            .saturating_sub(warm_bytes);

        let mut warnings = Vec::new();
        let mut notes = Vec::new();
        if cap < 1 {
            warnings.push("RAM budget cannot hold one expert slot per sparse layer".into());
        }
        if let Some(ref indices) = opts.gpu_indices {
            let unique: std::collections::HashSet<u32> = indices.iter().copied().collect();
            if gpus.len() != unique.len() {
                warnings.push("one or more requested GPUs were not detected".into());
            }
        }
        if !gpus.is_empty() && vram_budget < requested_vram {
            if any_uma {
                warnings.push(
                    "hot expert tier was clamped by unified system memory budget or model expert size"
                        .into(),
                );
            } else {
                warnings.push("VRAM tier was clamped by free VRAM or model expert size".into());
            }
        }
        // Per device: a busy BIOS window on an integrated GPU is not discrete
        // VRAM headroom (note the unified budget). A busy discrete card still
        // warns. Mixed AMD iGPU + RX must do both.
        let mut uma_carveout_busy = false;
        let mut disc_busy_total: u64 = 0;
        let mut disc_busy_free: u64 = 0;
        for gpu in &gpus {
            if gpu.total_bytes == 0 {
                continue;
            }
            let busy = gpu.free_bytes < (gpu.total_bytes as f64 * 0.75) as u64;
            if !busy {
                continue;
            }
            if gpu.integrated {
                uma_carveout_busy = true;
            } else {
                disc_busy_total += gpu.total_bytes;
                disc_busy_free += gpu.free_bytes;
            }
        }
        if uma_carveout_busy {
            notes.push(format!(
                "using unified system memory budget {} for GPU-resident experts",
                format_bytes(vram_budget)
            ));
        }
        if disc_busy_total > 0 {
            warnings.push(format!(
                "{} of VRAM is already in use (only {} of {} free): this plan plans against the remainder. Stop the running engine for a representative plan.",
                format_bytes(disc_busy_total - disc_busy_free),
                format_bytes(disc_busy_free),
                format_bytes(disc_busy_total)
            ));
        }
        if cold_bytes > 0 {
            // Model larger than RAM/unified budget: cold experts on the
            // existing store/SSD is intended overflow, not a misconfig.
            // Native Memory plan prints notes plain; doctor placement.plan
            // only warns on `warnings`. Real capacity fails stay warnings
            // (RAM slot, missing GPU, storage.disk < 1 GB).
            notes.push(
                "cold expert misses may reach disk; normal decode speed depends on hit rate".into(),
            );
        }

        let total_expert = info.expert_bytes;
        let resident_expert = hot_bytes + warm_bytes;
        let projected_hit = if total_expert > 0 {
            resident_expert as f64 / total_expert as f64
        } else {
            1.0
        };

        let (bottleneck, bottleneck_class): (String, String) = if cold_bytes > 0 {
            ("disk expert misses".to_string(), "disk".to_string())
        } else if warm_bytes > 0 && !gpus.is_empty() {
            (
                "CPU expert tail and GPU compute".to_string(),
                "mixed".to_string(),
            )
        } else if projected_hit >= 0.99 {
            if !gpus.is_empty() {
                (
                    "GPU compute and interconnect".to_string(),
                    "compute".to_string(),
                )
            } else {
                (
                    "CPU expert compute (fully resident)".to_string(),
                    "compute".to_string(),
                )
            }
        } else {
            (
                "CPU expert compute and RAM bandwidth".to_string(),
                "memory".to_string(),
            )
        };

        let tune = auto_tune(
            &bottleneck_class,
            projected_hit,
            &gpus,
            resolve_sockets(cpu_sockets),
            false,
        );
        let probe = ssd_probe_state(&info.path);

        Ok(Self {
            version: 2,
            policy: PlanPolicy {
                name: opts.policy.clone(),
                preserve_quantization: pq,
                preserve_router: pr,
                quality_preserving,
            },
            model: PlanModelSummary {
                path: info.path.display().to_string(),
                family: info.family.map(|f| f.as_str().to_string()),
                engine_id: if info.engine_id.is_empty() {
                    info.family
                        .unwrap_or(crate::model::ModelFamily::Glm)
                        .engine_basename()
                        .to_string()
                } else {
                    info.engine_id.clone()
                },
                shards: info.shards,
                model_bytes: info.model_bytes,
                disk_bytes: if info.disk_bytes > 0 {
                    info.disk_bytes
                } else {
                    info.model_bytes
                },
                param_count: info.param_count,
                dense_bytes: info.dense_bytes,
                expert_bytes: info.expert_bytes,
                expert_count: info.expert_count,
                expert_layers: info.expert_layers,
                typical_expert_bytes: info.typical_expert_bytes,
                per_cap_bytes: info.per_cap_bytes,
            },
            cpu: PlanCpu {
                physical_cores: resolve_physical_cores(physical_cpus),
                sockets: resolve_sockets(cpu_sockets),
                thread_policy: "physical-cores".into(),
            },
            tiers: PlanTiers {
                disk: TierDisk {
                    role: "cold-backing".into(),
                    model_bytes: info.model_bytes,
                    available_bytes: available_disk,
                    cold_expert_bytes: cold_bytes,
                },
                ram: TierRam {
                    role: "resident+warm-experts".into(),
                    available_bytes: available_memory,
                    budget_bytes: ram_budget,
                    dense_bytes: info.dense_bytes,
                    runtime_bytes,
                    // On UMA, warm cache is reduced by hot (same physical pool).
                    expert_cache_bytes: warm_cap,
                    warm_expert_bytes: warm_bytes,
                    cache_slots_per_layer: cap,
                },
                vram: TierVram {
                    role: "hot-experts".into(),
                    devices: gpu_plan,
                    budget_bytes: vram_budget,
                    hot_expert_bytes: hot_bytes,
                    expert_capacity: vram_experts,
                    requires_host_backing: false,
                },
            },
            expected_bottleneck: bottleneck,
            bottleneck_class,
            projected_hit_rate: (projected_hit * 10000.0).round() / 10000.0,
            tune,
            decisions: vec![
                PlanDecision {
                    target: "VRAM".into(),
                    reason: "profile-ranked hot experts".into(),
                },
                PlanDecision {
                    target: "RAM".into(),
                    reason: "warm experts execute on CPU without quality loss".into(),
                },
                PlanDecision {
                    target: "Disk".into(),
                    reason: "immutable recovery source for cold experts".into(),
                },
            ],
            warnings,
            notes,
            ssd_probe_gbs: probe.gbs,
            ssd_probe_state: probe.state,
        })
    }

    /// Inspect model then build plan.
    /// Model + tier size snapshot for hosts (raw bytes).
    pub fn model_size_info(&self) -> crate::model::ModelSizeInfo {
        use crate::model::{ModelFamily, ModelSizeInfo};
        let family = self.model.family.as_deref().and_then(|s| match s {
            "glm" => Some(ModelFamily::Glm),
            "inkling" => Some(ModelFamily::Inkling),
            "kimi" => Some(ModelFamily::Kimi),
            "deepseek_v4" => Some(ModelFamily::DeepseekV4),
            "olmoe" => Some(ModelFamily::Olmoe),
            _ => None,
        });
        ModelSizeInfo {
            path: Path::new(&self.model.path).to_path_buf(),
            family,
            engine_id: self.model.engine_id.clone(),
            disk_bytes: if self.model.disk_bytes > 0 {
                self.model.disk_bytes
            } else {
                self.model.model_bytes
            },
            model_bytes: self.model.model_bytes,
            dense_bytes: self.model.dense_bytes,
            expert_bytes: self.model.expert_bytes,
            param_count: self.model.param_count,
            shards: self.model.shards,
            tier_vram_bytes: Some(self.tiers.vram.budget_bytes),
            tier_ram_bytes: Some(self.tiers.ram.budget_bytes),
            tier_disk_bytes: Some(
                self.tiers
                    .disk
                    .model_bytes
                    .saturating_add(self.tiers.disk.cold_expert_bytes),
            ),
        }
    }

    pub fn build(model: impl AsRef<Path>, opts: &PlanOptions) -> Result<Self> {
        let info = ModelInfo::inspect(model)?;
        Self::build_from_info(&info, opts)
    }
}

fn cfg_u64(cfg: &serde_json::Value, key: &str) -> u64 {
    cfg.get(key)
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_i64().map(|i| i.max(0) as u64))
                .or_else(|| v.as_f64().map(|f| f.max(0.0) as u64))
        })
        .unwrap_or(0)
}

fn resolve_physical_cores(physical_cpus: u32) -> u32 {
    if physical_cpus < 1 {
        tracing::warn!("physical core count resolved to 0; defaulting to 1");
        1
    } else {
        physical_cpus
    }
}

fn resolve_sockets(n: u32) -> u32 {
    n.max(1)
}

/// Port of `resource_plan._auto_tune`.
fn auto_tune(
    bottleneck_class: &str,
    projected_hit: f64,
    gpus: &[GpuDevice],
    cpu_sockets: u32,
    plan_has_metal: bool,
) -> BTreeMap<String, TuneEntry> {
    let mut tune = BTreeMap::new();
    let has_gpu = !gpus.is_empty();
    let n_gpu = gpus.len();

    if std::env::var("COLI_CUDA_MTP").ok().as_deref() == Some("1") {
        // leave DRAFT to engine
    } else if bottleneck_class == "compute" {
        tune.insert(
            "DRAFT".into(),
            TuneEntry {
                value: "0".into(),
                reason: "compute-bound: MTP batch overhead exceeds yield".into(),
            },
        );
    } else if bottleneck_class == "disk" && projected_hit < 0.90 {
        tune.insert(
            "DRAFT".into(),
            TuneEntry {
                value: "0".into(),
                reason: "low hit rate: MTP widens expert union, adds disk reads".into(),
            },
        );
    }

    if has_gpu && n_gpu == 1 {
        tune.insert(
            "COLI_CUDA_PIPE".into(),
            TuneEntry {
                value: "1".into(),
                reason: "single GPU: S=1 pipeline gate".into(),
            },
        );
    } else if has_gpu && n_gpu > 1 {
        tune.insert(
            "COLI_CUDA_PIPE".into(),
            TuneEntry {
                value: "2".into(),
                reason: "multi-GPU: residual stays on-device across layers".into(),
            },
        );
    } else if !has_gpu && bottleneck_class == "disk" {
        tune.insert(
            "PIPE".into(),
            TuneEntry {
                value: "1".into(),
                reason: "overlap disk reads with resident expert compute".into(),
            },
        );
    }

    if cpu_sockets > 1 && has_gpu {
        tune.insert(
            "COLI_NUMA".into(),
            TuneEntry {
                value: "1".into(),
                reason: "multi-socket + GPU: interleave expert slabs, protect DMA buffers".into(),
            },
        );
    } else if cpu_sockets > 1 && !has_gpu {
        tune.insert(
            "COLI_NUMA".into(),
            TuneEntry {
                value: "1".into(),
                reason: "multi-socket CPU-only: interleave expert slabs across nodes".into(),
            },
        );
    }

    if plan_has_metal {
        tune.insert(
            "COLI_NO_OMP_TUNE".into(),
            TuneEntry {
                value: "1".into(),
                reason: "Metal: OMP spin-wait steals GPU power budget".into(),
            },
        );
    }

    if projected_hit >= 0.99 && !has_gpu {
        tune.insert(
            "PIN_GB".into(),
            TuneEntry {
                value: "all".into(),
                reason: "enough RAM for full expert residency".into(),
            },
        );
    }

    tune
}

/// Apply plan with setdefault semantics.
///
/// Port of `resource_plan.environment_for_plan`.
pub fn environment_for_plan(
    plan: &PlacementPlan,
    base: Option<&EnvMap>,
    cuda_enabled: bool,
) -> EnvMap {
    let mut result = base.cloned().unwrap_or_default();
    result.setdefault("COLI_POLICY", plan.policy.name.clone());
    result.setdefault("OMP_NUM_THREADS", plan.cpu.physical_cores.to_string());
    // Intentionally do NOT set OMP_PROC_BIND / OMP_PLACES (see Python note #325).
    for (key, entry) in &plan.tune {
        if key.starts_with('_') {
            continue;
        }
        result.setdefault(key.clone(), entry.value.clone());
    }
    if plan.policy.name == "balanced" {
        result.setdefault("REPIN", "64");
    }
    let ram = &plan.tiers.ram;
    result.setdefault(
        "RAM_GB",
        format!("{:.3}", ram.budget_bytes as f64 / GB as f64),
    );

    let vram = &plan.tiers.vram;
    let devices: Vec<u32> = vram.devices.iter().map(|d| d.index).collect();
    if !cuda_enabled || devices.is_empty() || vram.budget_bytes == 0 {
        return result;
    }
    if result.get("COLI_CUDA") == Some("0") {
        return result;
    }
    result.setdefault("COLI_CUDA", "1");
    if !result.contains("COLI_GPU") && !result.contains("COLI_GPUS") {
        let key = if devices.len() == 1 {
            "COLI_GPU"
        } else {
            "COLI_GPUS"
        };
        let joined = devices
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        result.set(key, joined);
    }
    result.setdefault(
        "CUDA_EXPERT_GB",
        format!("{:.3}", vram.budget_bytes as f64 / GB as f64),
    );
    if result.contains("PIN") {
        result.setdefault(
            "PIN_GB",
            format!("{:.3}", vram.budget_bytes as f64 / GB as f64),
        );
    }
    result
}

pub fn format_bytes(value: u64) -> String {
    format!("{:.1} GB", value as f64 / GB as f64)
}

/// Inputs for the CLI-shaped expert-cache clamp (`cap_for_ram` math).
#[derive(Debug, Clone, Copy)]
pub struct ClampExpertCapInput {
    pub requested_cap: u32,
    pub available_bytes: u64,
    pub resident_bytes: u64,
    pub sparse_rows: u32,
    pub expert_bytes: u64,
    pub slack_bytes: u64,
    pub overcommit: bool,
}

/// Result of [`clamp_expert_cap_for_ram`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpertCapDecision {
    Clamped { cap: u32 },
    Refused { projected_peak_bytes: u64 },
}

/// Same idea as C `cap_for_ram`: budget is 88% of available RAM; floor cap is 1.
///
/// When even one slot's projected peak exceeds `available_bytes` and
/// `overcommit` is false, returns [`ExpertCapDecision::Refused`].
pub fn clamp_expert_cap_for_ram(input: ClampExpertCapInput) -> ExpertCapDecision {
    let budget = ((input.available_bytes as f64) * 0.88) as u64;
    let slot = input.sparse_rows as u64 * input.expert_bytes;
    let avail = budget
        .saturating_sub(input.resident_bytes)
        .saturating_sub(input.slack_bytes);
    let mut capmax = avail.checked_div(slot).unwrap_or(0) as u32;
    let floored = capmax < 1;
    if capmax < 1 {
        capmax = 1;
    }
    if floored {
        let peak = input
            .resident_bytes
            .saturating_add(capmax as u64 * slot)
            .saturating_add(input.slack_bytes);
        if peak > input.available_bytes && !input.overcommit {
            return ExpertCapDecision::Refused {
                projected_peak_bytes: peak,
            };
        }
    }
    if capmax < input.requested_cap {
        ExpertCapDecision::Clamped { cap: capmax }
    } else {
        ExpertCapDecision::Clamped {
            cap: input.requested_cap.max(1),
        }
    }
}

/// `COLI_RAM_OVERCOMMIT=1` (C `atoi` non-zero). Isolated tests should pass a bool.
pub fn ram_overcommit_from(raw: Option<&str>) -> bool {
    raw.is_some_and(|v| atoi_i32(v) != 0)
}

/// C `atoi`: leading whitespace, optional sign, then digits. `"1foo"` is 1.
fn atoi_i32(s: &str) -> i32 {
    let s = s.trim_start();
    let (sign, rest) = if let Some(r) = s.strip_prefix('-') {
        (-1, r)
    } else if let Some(r) = s.strip_prefix('+') {
        (1, r)
    } else {
        (1, s)
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return 0;
    }
    digits.parse::<i32>().unwrap_or(0).saturating_mul(sign)
}

/// Process env for [`ram_overcommit_from`].
pub fn ram_overcommit_from_env() -> bool {
    ram_overcommit_from(std::env::var("COLI_RAM_OVERCOMMIT").ok().as_deref())
}

/// True when decode/prefill should leave the loop (embed stop or serve mux / SIGINT).
pub fn embed_decode_should_stop(
    embed_stop: bool,
    mux_stop: bool,
    mux_cancel: bool,
    interrupted: bool,
) -> bool {
    embed_stop || mux_stop || mux_cancel || interrupted
}

/// Whether a placement plan says even one expert slot cannot fit.
pub fn plan_cannot_hold_one_expert_slot(plan: &PlacementPlan) -> bool {
    plan.warnings
        .iter()
        .any(|w| w.contains("cannot hold one expert slot"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stub_info() -> ModelInfo {
        ModelInfo {
            path: Path::new("/tmp/fake-model").to_path_buf(),
            family: Some(crate::model::ModelFamily::Glm),
            engine_id: "colibri".into(),
            model_type: Some("glm_moe_dsa".into()),
            shards: 1,
            model_bytes: 10 * GB,
            disk_bytes: 10 * GB,
            param_count: Some(1_000_000),
            dense_bytes: 2 * GB,
            expert_bytes: 6 * GB,
            expert_count: 100,
            expert_layers: 10,
            typical_expert_bytes: 60_000_000,
            per_cap_bytes: 600_000_000,
            has_config: true,
            has_tokenizer: true,
            config: json!({
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

    #[test]
    fn cap_for_ram_injected_small_mem_clamps_below_default_64() {
        // 2 GiB available, huge expert slots: cap must not stay 64.
        let decision = clamp_expert_cap_for_ram(ClampExpertCapInput {
            requested_cap: 64,
            available_bytes: 2 * GB,
            resident_bytes: 512 * 1024 * 1024,
            sparse_rows: 80,
            expert_bytes: 38 * 1024 * 1024,
            slack_bytes: 4 * GB,
            overcommit: true,
        });
        match decision {
            ExpertCapDecision::Clamped { cap } => {
                assert!(cap < 64, "small RAM must clamp default cap=64, got {cap}");
                assert!(cap >= 1, "clamp keeps at least the floor slot");
            }
            other => panic!("expected clamp, got {other:?}"),
        }
    }

    #[test]
    fn cap_for_ram_floor_peak_above_ram_refuses_without_overcommit() {
        let decision = clamp_expert_cap_for_ram(ClampExpertCapInput {
            requested_cap: 64,
            available_bytes: 512 * 1024 * 1024,
            resident_bytes: 400 * 1024 * 1024,
            sparse_rows: 80,
            expert_bytes: 38 * 1024 * 1024,
            slack_bytes: 4 * GB,
            overcommit: false,
        });
        match decision {
            ExpertCapDecision::Refused {
                projected_peak_bytes,
            } => {
                assert!(
                    projected_peak_bytes > 512 * 1024 * 1024,
                    "peak {projected_peak_bytes} should exceed available"
                );
            }
            other => panic!("expected refuse, got {other:?}"),
        }
    }

    #[test]
    fn cap_for_ram_mid_range_clamps_to_88_percent_budget() {
        let available = 32 * GB;
        let resident = 2 * GB;
        let slack = 4 * GB;
        let sparse_rows = 10u32;
        let expert_bytes = GB / 10;
        let slot = sparse_rows as u64 * expert_bytes;
        let expected = ((0.88 * available as f64) as u64)
            .saturating_sub(resident)
            .saturating_sub(slack)
            / slot;
        assert!(
            expected > 1 && expected < 64,
            "fixture must land mid-range, got {expected}"
        );
        let decision = clamp_expert_cap_for_ram(ClampExpertCapInput {
            requested_cap: 64,
            available_bytes: available,
            resident_bytes: resident,
            sparse_rows,
            expert_bytes,
            slack_bytes: slack,
            overcommit: false,
        });
        match decision {
            ExpertCapDecision::Clamped { cap } => {
                assert_eq!(cap, expected as u32, "88% budget clamp");
            }
            other => panic!("expected mid-range clamp, got {other:?}"),
        }
    }

    #[test]
    fn ram_overcommit_from_matches_atoi_nonzero() {
        assert!(!ram_overcommit_from(None));
        assert!(!ram_overcommit_from(Some("")));
        assert!(!ram_overcommit_from(Some("0")));
        assert!(!ram_overcommit_from(Some("no")));
        assert!(ram_overcommit_from(Some("1")));
        assert!(ram_overcommit_from(Some("2")));
        assert!(ram_overcommit_from(Some(" 1 ")));
        // C atoi("1foo") == 1. Product matches that, not Rust i32 parse.
        assert!(ram_overcommit_from(Some("1foo")));
    }

    #[test]
    fn cap_for_ram_overcommit_allows_floor() {
        let decision = clamp_expert_cap_for_ram(ClampExpertCapInput {
            requested_cap: 64,
            available_bytes: 512 * 1024 * 1024,
            resident_bytes: 400 * 1024 * 1024,
            sparse_rows: 80,
            expert_bytes: 38 * 1024 * 1024,
            slack_bytes: 4 * GB,
            overcommit: true,
        });
        match decision {
            ExpertCapDecision::Clamped { cap } => assert_eq!(cap, 1),
            other => panic!("overcommit must clamp to floor, got {other:?}"),
        }
    }

    #[test]
    fn embed_decode_should_stop_when_flag_set() {
        assert!(!embed_decode_should_stop(false, false, false, false));
        assert!(embed_decode_should_stop(true, false, false, false));
        assert!(embed_decode_should_stop(false, true, false, false));
        assert!(embed_decode_should_stop(false, false, true, false));
        assert!(embed_decode_should_stop(false, false, false, true));
    }

    #[test]
    fn plan_model_summary_carries_disk_bytes_and_family() {
        let plan = PlacementPlan::build_from_info(
            &stub_info(),
            &PlanOptions {
                policy: "quality".into(),
                ram_gb: 32.0,
                context: 4096,
                available_memory: Some(64 * GB),
                available_disk: Some(500 * GB),
                gpus: Some(vec![]),
                physical_cpus: Some(8),
                cpu_sockets: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(plan.model.disk_bytes, 10 * GB);
        assert_eq!(plan.model.model_bytes, 10 * GB);
        assert_eq!(plan.model.family.as_deref(), Some("glm"));
        assert_eq!(plan.model.engine_id, "colibri");
        assert_eq!(plan.model.param_count, Some(1_000_000));
        let size = plan.model_size_info();
        assert_eq!(size.disk_bytes, 10 * GB);
        assert!(size.tier_ram_bytes.is_some());
    }

    #[test]
    fn build_plan_cpu_only_fixture() {
        let info = stub_info();
        let opts = PlanOptions {
            policy: "quality".into(),
            ram_gb: 32.0,
            context: 4096,
            available_memory: Some(64 * GB),
            available_disk: Some(500 * GB),
            gpus: Some(vec![]),
            physical_cpus: Some(16),
            cpu_sockets: Some(1),
            ..Default::default()
        };
        let plan = PlacementPlan::build_from_info(&info, &opts).unwrap();
        assert_eq!(plan.version, 2);
        assert_eq!(plan.cpu.physical_cores, 16);
        assert_eq!(plan.policy.name, "quality");
        assert!(plan.policy.quality_preserving);
        assert_eq!(plan.tiers.vram.devices.len(), 0);
        assert!(plan.tiers.ram.budget_bytes >= 32 * GB - 1);
        let env = environment_for_plan(&plan, None, true);
        assert_eq!(env.get("OMP_NUM_THREADS"), Some("16"));
        assert_eq!(env.get("COLI_POLICY"), Some("quality"));
        assert!(env.get("RAM_GB").is_some());
        // no CUDA keys without devices
        assert!(env.get("COLI_CUDA").is_none());
    }

    #[test]
    fn build_plan_with_gpu() {
        let info = stub_info();
        // Hold COLI_GPU_MEMORY lock so override goldens cannot race classification.
        let plan = with_coli_gpu_memory(None, || {
            let opts = PlanOptions {
                policy: "balanced".into(),
                available_memory: Some(64 * GB),
                available_disk: Some(500 * GB),
                gpus: Some(vec![GpuDevice {
                    index: 0,
                    name: "TestGPU".into(),
                    total_bytes: 48 * GB,
                    free_bytes: 40 * GB,
                    ..Default::default()
                }]),
                physical_cpus: Some(8),
                cpu_sockets: Some(1),
                ..Default::default()
            };
            PlacementPlan::build_from_info(&info, &opts).unwrap()
        });
        assert!(plan.tiers.vram.budget_bytes > 0);
        let env = environment_for_plan(&plan, None, true);
        assert_eq!(env.get("COLI_CUDA"), Some("1"));
        assert_eq!(env.get("COLI_GPU"), Some("0"));
        assert_eq!(env.get("REPIN"), Some("64"));
    }

    #[test]
    fn unknown_policy_errors() {
        let info = stub_info();
        let opts = PlanOptions {
            policy: "turbo".into(),
            ..Default::default()
        };
        assert!(PlacementPlan::build_from_info(&info, &opts).is_err());
    }

    #[test]
    fn setdefault_env_respects_base() {
        let info = stub_info();
        let opts = PlanOptions {
            available_memory: Some(64 * GB),
            available_disk: Some(500 * GB),
            gpus: Some(vec![]),
            physical_cpus: Some(4),
            ..Default::default()
        };
        let plan = PlacementPlan::build_from_info(&info, &opts).unwrap();
        let mut base = EnvMap::new();
        base.set("OMP_NUM_THREADS", "2");
        let env = environment_for_plan(&plan, Some(&base), false);
        assert_eq!(env.get("OMP_NUM_THREADS"), Some("2"));
    }

    #[test]
    fn uma_apu_starved_carveout_nonzero_hot_from_system_ram() {
        // APU: free VRAM ~0.2 GiB (carve-out busy) + free RAM 48 GiB + integrated
        // → hot usable = 50% of (available − 4 GiB OS headroom) = 22 GiB;
        // budget = min(usable, expert_bytes).
        let info = stub_info();
        let available = 48 * GB;
        let free_vram = 200 * 1024 * 1024; // ~0.2 GiB
        let plan = with_coli_gpu_memory(None, || {
            let opts = PlanOptions {
                policy: "quality".into(),
                available_memory: Some(available),
                available_disk: Some(500 * GB),
                gpus: Some(vec![GpuDevice {
                    index: 0,
                    name: "AMD Radeon 860M Graphics".into(),
                    total_bytes: 4 * GB,
                    free_bytes: free_vram,
                    vendor: "amd".into(),
                    source: "rocm-smi".into(),
                    arch: Some("gfx1152".into()),
                    integrated: true,
                    ..Default::default()
                }]),
                physical_cpus: Some(8),
                cpu_sockets: Some(1),
                ..Default::default()
            };
            PlacementPlan::build_from_info(&info, &opts).unwrap()
        });
        // Exact UMA formula: 0.5 * (free_system_RAM − 4 GiB).
        let expected_uma_usable =
            ((available.saturating_sub(UMA_OS_HEADROOM_BYTES) as f64) * UMA_HOT_FRACTION) as u64;
        assert_eq!(
            expected_uma_usable,
            22 * GB,
            "fixture sanity: 50% of (48−4) GiB must be 22 GiB"
        );
        // Discrete carve-out usable is 0 (0.2 GiB free − 2 GiB reserve); UMA share wins.
        assert_eq!(
            plan.tiers.vram.devices[0].usable_bytes, expected_uma_usable,
            "UMA usable_bytes must be half free RAM after OS headroom"
        );
        let expected_budget = expected_uma_usable.min(info.expert_bytes);
        assert_eq!(
            plan.tiers.vram.budget_bytes, expected_budget,
            "budget must be min(UMA usable, expert_bytes); expert_bytes={}",
            info.expert_bytes
        );
        assert!(
            plan.tiers.vram.hot_expert_bytes > 0,
            "hot_expert_bytes must be non-zero when budget > 0"
        );
        assert!(
            plan.tiers.vram.hot_expert_bytes <= plan.tiers.vram.budget_bytes,
            "hot must not exceed budget"
        );
        // Warm cache must leave room for hot on the same physical pool.
        let hot = plan.tiers.vram.hot_expert_bytes;
        let cache = plan.tiers.ram.expert_cache_bytes;
        let pre_hot_cache = plan
            .tiers
            .ram
            .budget_bytes
            .saturating_sub(plan.tiers.ram.dense_bytes)
            .saturating_sub(plan.tiers.ram.runtime_bytes);
        assert_eq!(
            cache + hot,
            pre_hot_cache,
            "UMA expert_cache + hot must equal pre-hot cache (warm_cap = cache − hot)"
        );
        assert!(
            plan.notes
                .iter()
                .any(|n| n.contains("unified system memory budget")),
            "busy carve-out should mention unified budget as information: notes={:?} warnings={:?}",
            plan.notes,
            plan.warnings
        );
        assert!(
            !plan.warnings.iter().any(|w| w.contains("carve-out is busy")
                || w.contains("using unified system memory budget")),
            "busy carve-out must not be a warning: {:?}",
            plan.warnings
        );
        let env = environment_for_plan(&plan, None, true);
        assert_eq!(env.get("COLI_CUDA"), Some("1"));
        let expert_gb: f64 = env
            .get("CUDA_EXPERT_GB")
            .unwrap()
            .parse()
            .expect("CUDA_EXPERT_GB");
        assert!(
            expert_gb > 0.0,
            "CUDA_EXPERT_GB must be positive when UMA hot > 0; got {expert_gb}"
        );
        // Named expected scale: budget is 6 GiB on this stub → expert GB ~6.
        assert!(
            (expert_gb - (expected_budget as f64 / GB as f64)).abs() < 0.1,
            "CUDA_EXPERT_GB={expert_gb} should track budget_bytes={} GiB",
            expected_budget as f64 / GB as f64
        );
    }

    /// Integrated AMD + busy BIOS carve-out + large system RAM must budget
    /// from unified RAM and must not warn as if the carve-out were discrete VRAM.
    #[test]
    fn uma_busy_carveout_does_not_warn_as_discrete_vram() {
        let mut info = stub_info();
        // Large expert pool so the unified budget is not clamped to the stub 6 GB.
        info.expert_bytes = 80 * GB;
        info.typical_expert_bytes = 100_000_000;
        let available = 72 * GB;
        // Operator-shaped APU: 0.4 GB free of 4.3 GB BIOS carve-out.
        let carve_total = (4.3 * GB as f64) as u64;
        let carve_free = (0.4 * GB as f64) as u64;
        let plan = with_coli_gpu_memory(None, || {
            let opts = PlanOptions {
                policy: "quality".into(),
                available_memory: Some(available),
                available_disk: Some(500 * GB),
                gpus: Some(vec![GpuDevice {
                    index: 0,
                    name: "AMD Radeon 860M Graphics".into(),
                    total_bytes: carve_total,
                    free_bytes: carve_free,
                    vendor: "amd".into(),
                    source: "rocm-smi".into(),
                    arch: Some("gfx1152".into()),
                    integrated: true,
                    ..Default::default()
                }]),
                physical_cpus: Some(8),
                cpu_sockets: Some(1),
                ..Default::default()
            };
            PlacementPlan::build_from_info(&info, &opts).unwrap()
        });
        let expected_uma_usable =
            ((available.saturating_sub(UMA_OS_HEADROOM_BYTES) as f64) * UMA_HOT_FRACTION) as u64;
        // 0.5 * (72 − 4) GiB = 34 GiB, tens of GB, not the 4.3 GB carve-out.
        assert_eq!(expected_uma_usable, 34 * GB);
        assert_eq!(
            plan.tiers.vram.devices[0].usable_bytes, expected_uma_usable,
            "UMA usable must be the unified RAM share, not carve-out free"
        );
        let expected_budget = expected_uma_usable.min(info.expert_bytes);
        assert_eq!(plan.tiers.vram.budget_bytes, expected_budget);
        assert!(
            plan.tiers.vram.budget_bytes > carve_total,
            "unified budget {} must exceed carve-out total {}",
            plan.tiers.vram.budget_bytes,
            carve_total
        );
        assert!(
            !plan.warnings.iter().any(|w| w.contains("carve-out is busy")
                || (w.contains("carve-out") && w.contains("only") && w.contains("free"))),
            "UMA must not warn that the BIOS carve-out is busy as if it were discrete VRAM: {:?}",
            plan.warnings
        );
    }

    /// User-visible Memory plan contract (native prefixes `warnings` as `Warning:`).
    ///
    /// Integrated AMD + busy BIOS carve-out must not emit a Warning-prefixed
    /// carve-out-busy line. Unified budget may appear as information only.
    #[test]
    fn uma_memory_plan_ui_does_not_warn_carveout_busy() {
        let mut info = stub_info();
        info.expert_bytes = 80 * GB;
        info.typical_expert_bytes = 100_000_000;
        let available = (81.2 * GB as f64) as u64;
        let carve_total = (4.3 * GB as f64) as u64;
        let carve_free = (0.4 * GB as f64) as u64;
        let plan = with_coli_gpu_memory(None, || {
            let opts = PlanOptions {
                policy: "quality".into(),
                available_memory: Some(available),
                available_disk: Some(500 * GB),
                gpus: Some(vec![GpuDevice {
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
            };
            PlacementPlan::build_from_info(&info, &opts).unwrap()
        });
        // Native Memory plan prefixes every `warnings` entry as `Warning:`.
        // Informational `notes` are printed without that prefix.
        let mut ui = String::new();
        for n in &plan.notes {
            ui.push_str(&format!("{n}\n"));
        }
        for w in &plan.warnings {
            ui.push_str(&format!("Warning: {w}\n"));
        }
        assert!(
            !ui.contains("Warning: device VRAM carve-out is busy"),
            "Memory plan must not show the carve-out-busy Warning: {ui}"
        );
        for line in ui.lines() {
            if line.starts_with("Warning:") {
                assert!(
                    !line.contains("carve-out is busy")
                        && !(line.contains("carve-out")
                            && line.contains("only")
                            && line.contains("free")),
                    "UMA must not prefix carve-out-busy as Warning: {ui}"
                );
                assert!(
                    !line.contains("VRAM is already in use"),
                    "UMA must not use the discrete VRAM-busy Warning: {ui}"
                );
            }
        }
        assert!(
            plan.notes
                .iter()
                .any(|n| n.starts_with("using unified system memory budget")),
            "UMA plan should mention the unified system memory budget as a note: notes={:?} warnings={:?}",
            plan.notes,
            plan.warnings
        );
        assert!(
            ui.lines().any(|l| {
                l.starts_with("using unified system memory budget") && !l.starts_with("Warning:")
            }),
            "unified-budget note must not be Warning-prefixed: {ui}"
        );
    }

    /// Operator screenshot contract: a 400+ GB MoE vs a ~39 GB unified budget
    /// is intended SSD overflow, not a misconfiguration. Cold-miss copy must
    /// be a plan note (native Memory plan prints notes plain). Keep real
    /// capacity fails (RAM slot, missing GPU, no disk room) as warnings.
    #[test]
    fn intended_cold_overflow_is_note_not_warning() {
        let mut info = stub_info();
        // Operator-shaped: GLM-5.2-class MoE (~429 GB) vs ~39 GB unified budget.
        info.expert_bytes = 400 * GB;
        info.typical_expert_bytes = 100_000_000;
        info.model_bytes = 429 * GB;
        info.disk_bytes = 429 * GB;
        let available = (45.0 * GB as f64) as u64;
        let plan = with_coli_gpu_memory(None, || {
            let opts = PlanOptions {
                policy: "quality".into(),
                available_memory: Some(available),
                available_disk: Some(500 * GB),
                gpus: Some(vec![GpuDevice {
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
            };
            PlacementPlan::build_from_info(&info, &opts).unwrap()
        });
        const COLD_MISS: &str =
            "cold expert misses may reach disk; normal decode speed depends on hit rate";
        assert!(
            plan.tiers.disk.cold_expert_bytes > 0,
            "fixture must intend overflow: cold={} hot={} warm={}",
            plan.tiers.disk.cold_expert_bytes,
            plan.tiers.vram.hot_expert_bytes,
            plan.tiers.ram.warm_expert_bytes
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
            "intended overflow must not be a scare warning: {:?}",
            plan.warnings
        );
        // Disk can hold the overflow; do not invent a capacity fail.
        assert!(
            plan.tiers.disk.available_bytes >= plan.tiers.disk.cold_expert_bytes,
            "fixture must have disk room for overflow"
        );
    }

    /// Mixed AMD iGPU + discrete dGPU: the discrete card still warns when its
    /// own VRAM is busy. The APU may still get a unified-budget note.
    #[test]
    fn mixed_amd_igpu_and_discrete_still_warns_vram_busy() {
        let mut info = stub_info();
        info.expert_bytes = 80 * GB;
        info.typical_expert_bytes = 100_000_000;
        let plan = with_coli_gpu_memory(None, || {
            let opts = PlanOptions {
                policy: "quality".into(),
                available_memory: Some((81.2 * GB as f64) as u64),
                available_disk: Some(500 * GB),
                gpus: Some(vec![
                    GpuDevice {
                        index: 0,
                        name: "AMD Radeon 860M Graphics".into(),
                        total_bytes: (4.3 * GB as f64) as u64,
                        free_bytes: (0.4 * GB as f64) as u64,
                        vendor: "amd".into(),
                        source: "rocm-smi".into(),
                        arch: Some("gfx1152".into()),
                        integrated: false,
                        ..Default::default()
                    },
                    GpuDevice {
                        index: 1,
                        name: "AMD Radeon RX 7900 XTX".into(),
                        total_bytes: 24 * GB,
                        free_bytes: 6 * GB,
                        vendor: "amd".into(),
                        source: "rocm-smi".into(),
                        integrated: false,
                        ..Default::default()
                    },
                ]),
                physical_cpus: Some(16),
                cpu_sockets: Some(1),
                ..Default::default()
            };
            PlacementPlan::build_from_info(&info, &opts).unwrap()
        });
        assert!(
            plan.warnings
                .iter()
                .any(|w| w.contains("VRAM is already in use")),
            "mixed AMD list must still warn that discrete VRAM is already in use: warnings={:?} notes={:?}",
            plan.warnings,
            plan.notes
        );
        assert!(
            plan.notes
                .iter()
                .any(|n| n.starts_with("using unified system memory budget")),
            "APU carve-out may still mention unified budget as a note: notes={:?}",
            plan.notes
        );
        assert!(
            !plan
                .warnings
                .iter()
                .any(|w| w.contains("carve-out is busy")),
            "mixed list must not use the carve-out-busy scare: {:?}",
            plan.warnings
        );
    }

    /// Discrete GPUs keep the busy-VRAM warning (do not loosen this golden).
    #[test]
    fn discrete_busy_vram_still_warns() {
        let info = stub_info();
        let plan = with_coli_gpu_memory(None, || {
            let opts = PlanOptions {
                available_memory: Some(64 * GB),
                available_disk: Some(500 * GB),
                gpus: Some(vec![GpuDevice {
                    index: 0,
                    name: "AMD Radeon RX 7900 XTX".into(),
                    total_bytes: 24 * GB,
                    free_bytes: 6 * GB, // 25% free → below 75% threshold
                    vendor: "amd".into(),
                    source: "rocm-smi".into(),
                    integrated: false,
                    ..Default::default()
                }]),
                physical_cpus: Some(16),
                cpu_sockets: Some(1),
                ..Default::default()
            };
            PlacementPlan::build_from_info(&info, &opts).unwrap()
        });
        assert!(
            plan.warnings.iter().any(|w| {
                w.contains("VRAM is already in use") && w.contains("only") && w.contains("free")
            }),
            "discrete busy VRAM must still warn: {:?}",
            plan.warnings
        );
        assert!(
            !plan
                .warnings
                .iter()
                .any(|w| w.contains("carve-out is busy")),
            "discrete warning must stay the VRAM-in-use wording: {:?}",
            plan.warnings
        );
    }

    #[test]
    fn discrete_free_vram_minus_two_gib_preserved() {
        let info = stub_info();
        let free = 24 * GB;
        let plan = with_coli_gpu_memory(None, || {
            let opts = PlanOptions {
                available_memory: Some(64 * GB),
                available_disk: Some(500 * GB),
                gpus: Some(vec![GpuDevice {
                    index: 0,
                    name: "AMD Radeon RX 7900 XTX".into(),
                    total_bytes: 24 * GB,
                    free_bytes: free,
                    vendor: "amd".into(),
                    source: "rocm-smi".into(),
                    integrated: false,
                    ..Default::default()
                }]),
                physical_cpus: Some(16),
                cpu_sockets: Some(1),
                ..Default::default()
            };
            PlacementPlan::build_from_info(&info, &opts).unwrap()
        });
        let expected_usable = free.saturating_sub(2 * GB);
        assert_eq!(plan.tiers.vram.devices[0].usable_bytes, expected_usable);
        // Budget is min(usable, expert_bytes).
        assert_eq!(
            plan.tiers.vram.budget_bytes,
            expected_usable.min(info.expert_bytes)
        );
    }

    #[test]
    fn uma_warm_reduced_by_hot() {
        let info = stub_info();
        let (plan_uma, plan_disc) = with_coli_gpu_memory(None, || {
            // Large free RAM, integrated, lots of expert bytes.
            let opts = PlanOptions {
                ram_gb: 40.0,
                available_memory: Some(48 * GB),
                available_disk: Some(500 * GB),
                gpus: Some(vec![GpuDevice {
                    index: 0,
                    name: "AMD Radeon 860M Graphics".into(),
                    total_bytes: 4 * GB,
                    free_bytes: 200 * 1024 * 1024,
                    vendor: "amd".into(),
                    integrated: true,
                    ..Default::default()
                }]),
                physical_cpus: Some(8),
                ..Default::default()
            };
            let plan_uma = PlacementPlan::build_from_info(&info, &opts).unwrap();
            // Same machine forced discrete (via integrated false + discrete name).
            let opts_disc = PlanOptions {
                ram_gb: 40.0,
                available_memory: Some(48 * GB),
                available_disk: Some(500 * GB),
                gpus: Some(vec![GpuDevice {
                    index: 0,
                    name: "AMD Radeon RX 7900 XTX".into(),
                    total_bytes: 4 * GB,
                    free_bytes: 200 * 1024 * 1024,
                    vendor: "amd".into(),
                    integrated: false,
                    ..Default::default()
                }]),
                physical_cpus: Some(8),
                ..Default::default()
            };
            let plan_disc = PlacementPlan::build_from_info(&info, &opts_disc).unwrap();
            (plan_uma, plan_disc)
        });
        let uma_hot = plan_uma.tiers.vram.hot_expert_bytes;
        let disc_hot = plan_disc.tiers.vram.hot_expert_bytes;
        assert!(
            uma_hot > disc_hot,
            "UMA hot ({uma_hot}) must exceed discrete-starved hot ({disc_hot})"
        );
        assert_eq!(
            disc_hot, 0,
            "starved discrete carve-out (0.2 GiB free − 2 GiB reserve) must yield zero hot"
        );
        // Strong contract: warm_cap_UMA + hot_UMA == pre-hot cache == discrete
        // expert_cache when discrete hot is zero (same ram_gb / dense / runtime).
        assert_eq!(
            plan_uma.tiers.ram.expert_cache_bytes + uma_hot,
            plan_disc.tiers.ram.expert_cache_bytes,
            "UMA expert_cache + hot must equal discrete expert_cache when disc hot is 0"
        );
        assert!(
            plan_uma.tiers.ram.expert_cache_bytes < plan_disc.tiers.ram.expert_cache_bytes,
            "UMA warm cache must be strictly smaller than discrete warm cache when hot > 0"
        );
    }

    /// Serialize env mutation for plan-level `COLI_GPU_MEMORY` goldens.
    ///
    /// All plan tests that call [`PlacementPlan::build_from_info`] with GPUs
    /// must hold this lock (pass `None` to clear) so override tests cannot
    /// race parallel discrete/UMA goldens.
    fn with_coli_gpu_memory<T>(value: Option<&str>, f: impl FnOnce() -> T) -> T {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let _guard = LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        // SAFETY: test-only env mutation; held under LOCK so plan tests do not race.
        unsafe {
            match value {
                Some(v) => std::env::set_var("COLI_GPU_MEMORY", v),
                None => std::env::remove_var("COLI_GPU_MEMORY"),
            }
        }
        let out = f();
        unsafe {
            std::env::remove_var("COLI_GPU_MEMORY");
        }
        out
    }

    #[test]
    fn coli_gpu_memory_discrete_override_forces_vram_minus_two_on_apu_name() {
        // APU-shaped free VRAM with explicit discrete → classic free − 2 GiB
        // (near-zero usable), not the 50%×(RAM−4 GiB) shared pool.
        let info = stub_info();
        let free_vram = 200 * 1024 * 1024; // ~0.2 GiB
        let plan = with_coli_gpu_memory(Some("discrete"), || {
            let opts = PlanOptions {
                available_memory: Some(48 * GB),
                available_disk: Some(500 * GB),
                gpus: Some(vec![GpuDevice {
                    index: 0,
                    name: "AMD Radeon 860M Graphics".into(),
                    total_bytes: 4 * GB,
                    free_bytes: free_vram,
                    vendor: "amd".into(),
                    source: "rocm-smi".into(),
                    arch: Some("gfx1152".into()),
                    integrated: true, // will be forced false by override
                    ..Default::default()
                }]),
                physical_cpus: Some(8),
                ..Default::default()
            };
            PlacementPlan::build_from_info(&info, &opts).unwrap()
        });
        let expected_usable = free_vram.saturating_sub(VRAM_RESERVE_BYTES);
        assert_eq!(
            expected_usable, 0,
            "fixture: 0.2 GiB free − 2 GiB reserve → 0 usable"
        );
        assert_eq!(
            plan.tiers.vram.devices[0].usable_bytes, expected_usable,
            "COLI_GPU_MEMORY=discrete must use free−2 GiB, not UMA shared pool"
        );
        assert_eq!(plan.tiers.vram.budget_bytes, 0);
        assert_eq!(plan.tiers.vram.hot_expert_bytes, 0);
        assert!(
            !plan
                .warnings
                .iter()
                .any(|w| w.contains("using unified system memory budget"))
                && !plan
                    .notes
                    .iter()
                    .any(|n| n.contains("using unified system memory budget")),
            "discrete override must not emit UMA busy-carve-out budget string: warnings={:?} notes={:?}",
            plan.warnings,
            plan.notes
        );
    }

    #[test]
    fn coli_gpu_memory_unified_override_forces_shared_pool_on_rx_name() {
        // Discrete-named GPU with modest free VRAM, COLI_GPU_MEMORY=unified →
        // shared-pool usable 50%×(RAM−4 GiB) must beat discrete free−2 alone.
        let info = stub_info();
        let free_vram = 6 * GB; // discrete free−2 = 4 GiB
        let available = 48 * GB; // UMA share = 22 GiB
        let plan = with_coli_gpu_memory(Some("unified"), || {
            let opts = PlanOptions {
                available_memory: Some(available),
                available_disk: Some(500 * GB),
                gpus: Some(vec![GpuDevice {
                    index: 0,
                    name: "AMD Radeon RX 7900 XTX".into(),
                    total_bytes: 24 * GB,
                    free_bytes: free_vram,
                    vendor: "amd".into(),
                    source: "rocm-smi".into(),
                    integrated: false, // forced true by override
                    ..Default::default()
                }]),
                physical_cpus: Some(16),
                ..Default::default()
            };
            PlacementPlan::build_from_info(&info, &opts).unwrap()
        });
        let uma_share =
            ((available.saturating_sub(UMA_OS_HEADROOM_BYTES) as f64) * UMA_HOT_FRACTION) as u64;
        let discrete_usable = free_vram.saturating_sub(VRAM_RESERVE_BYTES);
        assert_eq!(uma_share, 22 * GB);
        assert_eq!(discrete_usable, 4 * GB);
        let expected_usable = uma_share.max(discrete_usable);
        assert_eq!(
            plan.tiers.vram.devices[0].usable_bytes, expected_usable,
            "unified override must take UMA share when larger than free−2"
        );
        assert!(
            plan.tiers.vram.devices[0].usable_bytes > discrete_usable,
            "must exceed pure discrete free−2 on this fixture"
        );
        assert_eq!(
            plan.tiers.vram.budget_bytes,
            expected_usable.min(info.expert_bytes)
        );
        assert!(plan.tiers.vram.hot_expert_bytes > 0);
        let env = environment_for_plan(&plan, None, true);
        assert_eq!(env.get("COLI_CUDA"), Some("1"));
    }
}
