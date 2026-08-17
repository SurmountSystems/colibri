//! Model inspection, family routing, registry, and supported catalog.
//!
//! Port of family routing from `c/coli` (`model_arch`, `engine_for`) and
//! geometry scan from `c/resource_plan.py` (`analyze_model`, `_tensor_sizes`).
//!
//! [`SupportedModel`] / [`supported_models`] is the **product** list of models
//! Colibri supports (README + engine families). [`ModelRegistry`] is local disk
//! inventory only (scan `config.json` leaves under store roots).

mod catalog;
mod registry;

pub use catalog::{
    SupportedModel, supported_model_by_hf_repo, supported_model_by_id, supported_models,
};
pub use registry::{ModelEntry, ModelRegistry, ModelStatus};

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

#[cfg(feature = "install")]
pub mod install;

/// Model family used for engine binary selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModelFamily {
    #[default]
    Glm,
    Inkling,
    Kimi,
    DeepseekV4,
    /// Research path; coli does not auto-route OLMoE (falls through to GLM binary).
    Olmoe,
}

impl ModelFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Glm => "glm",
            Self::Inkling => "inkling",
            Self::Kimi => "kimi",
            Self::DeepseekV4 => "deepseek_v4",
            Self::Olmoe => "olmoe",
        }
    }

    /// Default engine binary basename (without `.exe`).
    pub fn engine_basename(self) -> &'static str {
        match self {
            Self::Glm | Self::Olmoe => "colibri",
            Self::Inkling => "inkling",
            Self::Kimi => "kimi_k3",
            Self::DeepseekV4 => "deepseek_v4",
        }
    }
}

/// Classify family from `config.json` `model_type`.
///
/// Port of `coli.model_arch`.
pub fn model_arch_from_type(model_type: &str) -> ModelFamily {
    let t = model_type.to_lowercase();
    if t.contains("inkling") {
        return ModelFamily::Inkling;
    }
    if t.contains("kimi") {
        return ModelFamily::Kimi;
    }
    if t.contains("deepseek_v4") || (t.contains("deepseek") && t.contains("v4")) {
        return ModelFamily::DeepseekV4;
    }
    if t.contains("olmoe") {
        return ModelFamily::Olmoe;
    }
    ModelFamily::Glm
}

/// Read family from a model directory's `config.json`.
pub fn model_arch(model: &Path) -> ModelFamily {
    let config_path = model.join("config.json");
    match std::fs::read_to_string(&config_path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(v) => {
                let mt = v.get("model_type").and_then(|x| x.as_str()).unwrap_or("");
                model_arch_from_type(mt)
            }
            Err(_) => ModelFamily::Glm,
        },
        Err(_) => ModelFamily::Glm,
    }
}

/// Inspected model geometry and completeness (without full plan).
///
/// **Size fields (programmatic):** hosts that need “how large is this model”
/// should read [`Self::disk_bytes`] (raw weight bytes on disk). [`Self::model_bytes`]
/// is the same value (historical name from `resource_plan.analyze_model`).
/// Optional [`Self::param_count`] is filled only when `config.json` declares it.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelInfo {
    pub path: PathBuf,
    pub family: Option<ModelFamily>,
    /// Engine binary basename for this family (`colibri`, `kimi_k3`, …).
    #[serde(default)]
    pub engine_id: String,
    pub model_type: Option<String>,
    pub shards: usize,
    /// Total safetensors payload size on disk (bytes). Alias of historical name.
    pub model_bytes: u64,
    /// Total weight size on disk in bytes (same as [`Self::model_bytes`]).
    /// Prefer this field in new host code.
    #[serde(default)]
    pub disk_bytes: u64,
    /// Parameter count from config when present (`num_parameters` / `n_params` / …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param_count: Option<u64>,
    pub dense_bytes: u64,
    pub expert_bytes: u64,
    pub expert_count: usize,
    pub expert_layers: usize,
    pub typical_expert_bytes: u64,
    /// Sum of per-layer median expert sizes (one slot per sparse layer).
    pub per_cap_bytes: u64,
    pub has_config: bool,
    pub has_tokenizer: bool,
    /// Parsed `config.json` object (plan math reads selected keys).
    #[serde(skip)]
    pub config: Value,
    pub shard_names: Vec<String>,
}

impl ModelInfo {
    /// Inspect a model directory (headers only; no payload hash).
    ///
    /// Port of `resource_plan.analyze_model`.
    pub fn inspect(path: impl AsRef<Path>) -> Result<Self> {
        let model = path
            .as_ref()
            .canonicalize()
            .map_err(|e| Error::model(path.as_ref(), e.to_string()))?;
        let config_path = model.join("config.json");
        if !config_path.is_file() {
            return Err(Error::model(&model, "missing config.json"));
        }
        let config_text = std::fs::read_to_string(&config_path)?;
        let config: Value = serde_json::from_str(&config_text)?;
        if !config.is_object() {
            return Err(Error::model(&model, "config.json is not a JSON object"));
        }
        let model_type = config
            .get("model_type")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let detected_family = model_arch_from_type(model_type.as_deref().unwrap_or(""));
        let family = Some(detected_family);

        let mut shards: Vec<PathBuf> = std::fs::read_dir(&model)?
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
            return Err(Error::model(&model, "no safetensors shards"));
        }

        let expert_re = Regex::new(r"model\.layers\.(\d+)\.mlp\.experts\.(\d+)\.").unwrap();
        let mut dense_bytes: u64 = 0;
        let mut expert_groups: HashMap<(i32, i32), u64> = HashMap::new();
        let mut model_bytes: u64 = 0;
        let mut shard_names = Vec::new();

        for shard in &shards {
            model_bytes += std::fs::metadata(shard)?.len();
            shard_names.push(
                shard
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            );
            for (name, size) in tensor_sizes(shard)? {
                if let Some(c) = expert_re.captures(&name) {
                    let layer: i32 = c[1].parse().unwrap_or(0);
                    let expert: i32 = c[2].parse().unwrap_or(0);
                    *expert_groups.entry((layer, expert)).or_insert(0) += size;
                } else {
                    dense_bytes += size;
                }
            }
        }

        let mut layer_sizes: HashMap<i32, Vec<u64>> = HashMap::new();
        for ((layer, _), size) in &expert_groups {
            layer_sizes.entry(*layer).or_default().push(*size);
        }
        let mut per_layer: HashMap<i32, u64> = HashMap::new();
        for (layer, sizes) in &layer_sizes {
            per_layer.insert(*layer, median_u64(sizes));
        }
        let per_cap_bytes: u64 = per_layer.values().sum();
        let typical_expert_bytes = if per_layer.is_empty() {
            0
        } else {
            let vals: Vec<u64> = per_layer.values().copied().collect();
            median_u64(&vals)
        };
        let expert_bytes: u64 = expert_groups.values().sum();
        let has_tokenizer = model.join("tokenizer.json").is_file();
        let engine_id = detected_family.engine_basename().to_string();
        let param_count = param_count_from_config(&config);

        Ok(Self {
            path: model,
            family,
            engine_id,
            model_type,
            shards: shards.len(),
            model_bytes,
            disk_bytes: model_bytes,
            param_count,
            dense_bytes,
            expert_bytes,
            expert_count: expert_groups.len(),
            expert_layers: per_layer.len(),
            typical_expert_bytes,
            per_cap_bytes,
            has_config: true,
            has_tokenizer,
            config,
            shard_names,
        })
    }

    /// Completeness for registry status (no deep tensor audit).
    pub fn is_complete(&self) -> bool {
        self.has_config && self.has_tokenizer && self.shards > 0
    }

    /// Weight size on disk in gibibytes (binary GiB = 1024³).
    pub fn disk_gib(&self) -> f64 {
        self.disk_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
    }

    /// Compact size snapshot for hosts / FFI / install summaries.
    pub fn size_info(&self) -> ModelSizeInfo {
        ModelSizeInfo {
            path: self.path.clone(),
            family: self.family,
            engine_id: if self.engine_id.is_empty() {
                self.family
                    .unwrap_or(ModelFamily::Glm)
                    .engine_basename()
                    .to_string()
            } else {
                self.engine_id.clone()
            },
            disk_bytes: self.disk_bytes,
            model_bytes: self.model_bytes,
            dense_bytes: self.dense_bytes,
            expert_bytes: self.expert_bytes,
            param_count: self.param_count,
            shards: self.shards,
            tier_vram_bytes: None,
            tier_ram_bytes: None,
            tier_disk_bytes: None,
        }
    }
}

/// Public model size snapshot (raw bytes for programmatic use).
///
/// Used by `ModelInfo::size_info`, install summaries, plan attach, and FFI open.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ModelSizeInfo {
    pub path: PathBuf,
    pub family: Option<ModelFamily>,
    /// Engine id string (`colibri`, `deepseek_v4`, `kimi_k3`, …).
    pub engine_id: String,
    /// Total weight bytes on disk.
    pub disk_bytes: u64,
    /// Historical alias of [`Self::disk_bytes`].
    pub model_bytes: u64,
    pub dense_bytes: u64,
    pub expert_bytes: u64,
    pub param_count: Option<u64>,
    pub shards: usize,
    /// When known from a placement plan: VRAM hot-tier budget (bytes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier_vram_bytes: Option<u64>,
    /// When known from a placement plan: RAM budget (bytes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier_ram_bytes: Option<u64>,
    /// When known from a placement plan: disk tier model/cold budget (bytes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier_disk_bytes: Option<u64>,
}

impl ModelSizeInfo {
    pub fn disk_gib(&self) -> f64 {
        self.disk_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
    }

    /// Attach tier placement sizes from a plan (does not change disk_bytes).
    pub fn with_plan_tiers(mut self, plan: &crate::plan::PlacementPlan) -> Self {
        self.tier_vram_bytes = Some(plan.tiers.vram.budget_bytes);
        self.tier_ram_bytes = Some(plan.tiers.ram.budget_bytes);
        self.tier_disk_bytes = Some(
            plan.tiers
                .disk
                .model_bytes
                .saturating_add(plan.tiers.disk.cold_expert_bytes),
        );
        self
    }
}

/// Read optional parameter count from `config.json` object.
pub fn param_count_from_config(config: &Value) -> Option<u64> {
    const KEYS: &[&str] = &[
        "num_parameters",
        "n_params",
        "total_params",
        "n_parameters",
        "params",
    ];
    for key in KEYS {
        if let Some(v) = config.get(*key) {
            if let Some(n) = v.as_u64() {
                return Some(n);
            }
            if let Some(n) = v.as_i64() {
                if n >= 0 {
                    return Some(n as u64);
                }
            }
            if let Some(f) = v.as_f64() {
                if f.is_finite() && f >= 0.0 {
                    return Some(f as u64);
                }
            }
            if let Some(s) = v.as_str() {
                if let Ok(n) = s.replace('_', "").parse::<u64>() {
                    return Some(n);
                }
            }
        }
    }
    None
}

fn median_u64(sizes: &[u64]) -> u64 {
    if sizes.is_empty() {
        return 0;
    }
    let mut v = sizes.to_vec();
    v.sort_unstable();
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        // Python statistics.median for even: average of two middle (may be float).
        // For bytes we use the lower of the two middle for stability (or average).
        // Python 3 statistics.median: mean of two middle as float then used as int later
        // via int(statistics.median(...)). Match int() of mean.
        let a = v[n / 2 - 1] as u128;
        let b = v[n / 2] as u128;
        ((a + b) / 2) as u64
    }
}

/// Yield `(tensor_name, byte_size)` from a safetensors header (no payload read).
///
/// Port of `resource_plan._tensor_sizes`.
pub fn tensor_sizes(path: &Path) -> Result<Vec<(String, u64)>> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let file_size = f.metadata()?.len();
    let mut len_buf = [0u8; 8];
    f.read_exact(&mut len_buf)?;
    let length = u64::from_le_bytes(len_buf);
    if length < 2 || length > file_size.saturating_sub(8) {
        return Err(Error::model(path, "invalid safetensors header length"));
    }
    let mut header_bytes = vec![0u8; length as usize];
    f.read_exact(&mut header_bytes)?;
    let header: Value = serde_json::from_slice(&header_bytes)?;
    let obj = header
        .as_object()
        .ok_or_else(|| Error::model(path, "safetensors header is not an object"))?;
    let mut out = Vec::new();
    for (name, meta) in obj {
        if name == "__metadata__" {
            continue;
        }
        let offsets = meta
            .get("data_offsets")
            .and_then(|v| v.as_array())
            .ok_or_else(|| Error::model(path, format!("missing data_offsets for {name}")))?;
        if offsets.len() != 2 {
            return Err(Error::model(path, format!("bad data_offsets for {name}")));
        }
        let start = offsets[0].as_u64().unwrap_or(u64::MAX);
        let end = offsets[1].as_u64().unwrap_or(u64::MAX);
        let max = file_size.saturating_sub(8).saturating_sub(length);
        if start > end || end > max {
            return Err(Error::model(
                path,
                format!("invalid tensor offsets for {name}"),
            ));
        }
        out.push((name.clone(), end - start));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_routing() {
        assert_eq!(model_arch_from_type("glm_moe_dsa"), ModelFamily::Glm);
        assert_eq!(model_arch_from_type("inkling-moe"), ModelFamily::Inkling);
        assert_eq!(model_arch_from_type("kimi_k3"), ModelFamily::Kimi);
        assert_eq!(
            model_arch_from_type("deepseek_v4_flash"),
            ModelFamily::DeepseekV4
        );
        assert_eq!(model_arch_from_type("deepseek-v4"), ModelFamily::DeepseekV4);
    }

    #[test]
    fn median_even() {
        assert_eq!(median_u64(&[1, 2, 3, 4]), 2);
        assert_eq!(median_u64(&[10]), 10);
        assert_eq!(median_u64(&[1, 3]), 2);
    }

    #[test]
    fn param_count_from_config_keys() {
        let v = serde_json::json!({"num_parameters": 1_234_567u64});
        assert_eq!(param_count_from_config(&v), Some(1_234_567));
        let v = serde_json::json!({"n_params": "2_000"});
        assert_eq!(param_count_from_config(&v), Some(2000));
        let v = serde_json::json!({"hidden_size": 128});
        assert_eq!(param_count_from_config(&v), None);
    }

    #[test]
    fn inspect_glm_tiny_has_disk_bytes() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../c/glm_tiny");
        if !root.join("model.safetensors").is_file() {
            return;
        }
        let info = ModelInfo::inspect(&root).expect("glm_tiny inspect");
        assert!(info.disk_bytes > 0, "disk_bytes must be set");
        assert_eq!(info.disk_bytes, info.model_bytes);
        assert_eq!(info.family, Some(ModelFamily::Glm));
        assert_eq!(info.engine_id, "colibri");
        let size = info.size_info();
        assert_eq!(size.disk_bytes, info.disk_bytes);
        assert_eq!(size.engine_id, "colibri");
    }
}
