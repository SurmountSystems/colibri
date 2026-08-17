//! Typed Colibrì configuration and process environment maps.
//!
//! Port of host config surface documented in `docs/SETTINGS.md` and
//! `docs/ENVIRONMENT.md`, and applied by `c/coli` / `c/resource_plan.py`
//! (`environment_for_plan` setdefault semantics).
//!
//! **Precedence (highest wins):**
//! 1. Explicit fields on [`ColibriConfig`] the host already set on the process env
//! 2. Values already present in the parent environment (when merging)
//! 3. Placement plan [`crate::plan::environment_for_plan`] via setdefault
//! 4. Engine built-in defaults
//!
//! The product has no TOML/YAML app config; configuration is the process
//! environment plus a model directory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::paths;
use crate::plan::PlacementPlan;

/// Ordered environment map used when spawning engines.
///
/// Keys match the engine / `coli` env surface (`SNAP`, `RAM_GB`, `COLI_CUDA`, …).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvMap {
    /// Key → value pairs. Insertion order is not preserved; BTree for stable tests.
    pub vars: BTreeMap<String, String>,
}

impl EnvMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or overwrite.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.vars.insert(key.into(), value.into());
    }

    /// Insert only if the key is absent (plan / tune setdefault semantics).
    ///
    /// Port of `dict.setdefault` used by `resource_plan.environment_for_plan`.
    pub fn setdefault(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.vars.entry(key.into()).or_insert_with(|| value.into());
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(String::as_str)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.vars.contains_key(key)
    }

    /// Merge another map with setdefault semantics (other does not overwrite).
    pub fn merge_setdefault(&mut self, other: &EnvMap) {
        for (k, v) in &other.vars {
            self.setdefault(k.clone(), v.clone());
        }
    }

    /// Overlay: `other` overwrites matching keys.
    pub fn merge_overwrite(&mut self, other: &EnvMap) {
        for (k, v) in &other.vars {
            self.set(k.clone(), v.clone());
        }
    }

    /// Build from the current process environment.
    pub fn from_process() -> Self {
        let mut m = Self::new();
        for (k, v) in std::env::vars() {
            m.set(k, v);
        }
        m
    }

    /// Apply into a `std::process::Command` env (clears inherited? no — only sets).
    pub fn apply_to_command(&self, cmd: &mut std::process::Command) {
        for (k, v) in &self.vars {
            cmd.env(k, v);
        }
    }

    #[cfg(feature = "tokio")]
    pub fn apply_to_tokio_command(&self, cmd: &mut tokio::process::Command) {
        for (k, v) in &self.vars {
            cmd.env(k, v);
        }
    }
}

/// Placement policy names aligned with `resource_plan.POLICIES`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Policy {
    #[default]
    Quality,
    Balanced,
    ExperimentalFast,
}

impl Policy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Quality => "quality",
            Self::Balanced => "balanced",
            Self::ExperimentalFast => "experimental-fast",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "quality" => Some(Self::Quality),
            "balanced" => Some(Self::Balanced),
            "experimental-fast" => Some(Self::ExperimentalFast),
            _ => None,
        }
    }
}

/// Host-side Colibrì settings (CLI-equivalent subset used for serve/chat embed).
///
/// See `docs/SETTINGS.md` for the full CLI surface; this struct is the Rust
/// typed subset needed to spawn and size an engine process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColibriConfig {
    /// Model directory (`SNAP` / `COLI_MODEL` / `--model`).
    pub model: Option<PathBuf>,
    /// Root where models are installed / inventoried by default.
    ///
    /// `None` = discoverable default ([`paths::default_model_store_path`]):
    /// env `COLIBRI_MODEL_STORE` / `COLI_MODEL_STORE`, else
    /// `$XDG_DATA_HOME/colibri/models` (or `~/.local/share/colibri/models`).
    /// Set to `Some(path)` to force a volume for free-space checks and installs.
    pub model_store: Option<PathBuf>,
    /// Override engine binary (`COLI_ENGINE`).
    pub engine: Option<PathBuf>,
    /// Policy name for plan + `COLI_POLICY`.
    pub policy: Policy,
    /// RAM budget in GB (`RAM_GB` / `--ram`). 0 = plan from available.
    pub ram_gb: f64,
    /// Context length (`CTX` / `--ctx`).
    pub ctx: u32,
    /// Expert cache slots per layer (`cap` argv / plan). `None` = engine auto.
    pub cap: Option<u32>,
    /// Max generated tokens (`NGEN`).
    pub max_tokens: u32,
    /// Sampling temperature (`COLI_TEMP`).
    pub temperature: f32,
    /// Top-p (`COLI_TOP_P` if set by host; serve header uses per-request).
    pub top_p: f32,
    /// GPU indices (`COLI_GPU` / `COLI_GPUS`). `None` = all discovered.
    pub gpu_indices: Option<Vec<u32>>,
    /// Explicit VRAM hot-tier budget GB (`CUDA_EXPERT_GB` / `--vram`). 0 = plan free.
    pub vram_gb: f64,
    /// Enable CUDA path in plan apply (`COLI_CUDA`).
    pub cuda_enabled: bool,
    /// KV slots for mux (`KV_SLOTS`, 1–16).
    pub kv_slots: u32,
    /// Mirror directory (`COLI_MODEL_MIRROR`).
    pub mirror: Option<PathBuf>,
    /// Extra env the host insists on (always overwrite).
    pub extra_env: EnvMap,
    /// Whether to set `COLI_NO_OMP_TUNE=1` for library-managed children.
    /// Recommended for embed so the engine does not re-exec for OMP tuning.
    pub no_omp_tune: bool,
    /// Prefer the C engine **subprocess** (serve mux) over experimental
    /// in-process FFI when both are available.
    ///
    /// Defaults to **`true`**: process serve remains the product path.
    /// Set `false` only when `feature = "ffi"` is linked and the host opts into
    /// DeepSeek V4 in-process open. Env `COLIBRI_FORCE_PROCESS=1` always wins
    /// (forces process regardless of this flag). See crate `docs/ffi-phase-d.md`.
    pub prefer_process: bool,
}

/// Env key: truthy value forces subprocess engine path (FFI kill-switch).
pub const FORCE_PROCESS_ENV: &str = "COLIBRI_FORCE_PROCESS";

/// Parse `COLIBRI_FORCE_PROCESS` / synthetic values.
///
/// Unset, empty, `0`, `false`, `no`, `off` (case-insensitive) → false.
/// Any other non-empty value → true.
pub fn env_force_process(value: Option<impl AsRef<std::ffi::OsStr>>) -> bool {
    let Some(v) = value else {
        return false;
    };
    let s = v.as_ref().to_string_lossy();
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    !matches!(
        t.to_ascii_lowercase().as_str(),
        "0" | "false" | "no" | "off"
    )
}

/// True when `COLIBRI_FORCE_PROCESS` is set truthy in the process environment.
pub fn force_process_from_env() -> bool {
    env_force_process(std::env::var_os(FORCE_PROCESS_ENV))
}

impl Default for ColibriConfig {
    fn default() -> Self {
        Self {
            model: None,
            model_store: None,
            engine: None,
            policy: Policy::Quality,
            ram_gb: 0.0,
            ctx: 4096,
            cap: None,
            max_tokens: 1024,
            temperature: 1.0,
            top_p: 1.0,
            gpu_indices: None,
            vram_gb: 0.0,
            cuda_enabled: true,
            kv_slots: 1,
            mirror: None,
            extra_env: EnvMap::new(),
            no_omp_tune: true,
            prefer_process: true,
        }
    }
}

impl ColibriConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn model(mut self, path: impl Into<PathBuf>) -> Self {
        self.model = Some(path.into());
        self
    }

    /// Override default model store root (`Some(path)`), or clear with `None` via field set.
    pub fn model_store(mut self, path: impl Into<PathBuf>) -> Self {
        self.model_store = Some(path.into());
        self
    }

    /// Resolved model store path (config override, else discoverable default).
    pub fn resolved_model_store(&self) -> PathBuf {
        self.model_store
            .clone()
            .unwrap_or_else(paths::default_model_store_path)
    }

    pub fn engine(mut self, path: impl Into<PathBuf>) -> Self {
        self.engine = Some(path.into());
        self
    }

    pub fn policy(mut self, policy: Policy) -> Self {
        self.policy = policy;
        self
    }

    pub fn ram_gb(mut self, gb: f64) -> Self {
        self.ram_gb = gb;
        self
    }

    pub fn ctx(mut self, ctx: u32) -> Self {
        self.ctx = ctx;
        self
    }

    pub fn max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = n;
        self
    }

    pub fn kv_slots(mut self, n: u32) -> Self {
        self.kv_slots = n.clamp(1, 16);
        self
    }

    /// Prefer subprocess (`true`, default) or allow experimental FFI when linked.
    pub fn prefer_process(mut self, prefer: bool) -> Self {
        self.prefer_process = prefer;
        self
    }

    /// Apply a placement plan with setdefault semantics, then layer explicit
    /// config fields and `extra_env`.
    ///
    /// Port of `environment_for_plan` + `coli` flag→env mapping.
    pub fn apply_plan(&self, plan: &PlacementPlan) -> EnvMap {
        let mut env = crate::plan::environment_for_plan(plan, None, self.cuda_enabled);
        // Explicit config overwrites plan setdefaults.
        if let Some(ref model) = self.model {
            env.set("SNAP", model.display().to_string());
            env.set("COLI_MODEL", model.display().to_string());
        }
        if let Some(ref eng) = self.engine {
            env.set("COLI_ENGINE", eng.display().to_string());
        }
        if self.ctx > 0 {
            env.set("CTX", self.ctx.to_string());
        }
        if self.max_tokens > 0 {
            env.set("NGEN", self.max_tokens.to_string());
        }
        env.set("COLI_TEMP", format!("{:.8}", self.temperature));
        env.set("KV_SLOTS", self.kv_slots.clamp(1, 16).to_string());
        if let Some(ref mirror) = self.mirror {
            env.set("COLI_MODEL_MIRROR", mirror.display().to_string());
        }
        if self.no_omp_tune {
            env.set("COLI_NO_OMP_TUNE", "1");
        }
        if !self.cuda_enabled {
            env.set("COLI_CUDA", "0");
        }
        env.merge_overwrite(&self.extra_env);
        env
    }

    /// Build spawn env for serve mode without a plan (minimal SERVE flags).
    pub fn serve_env(&self) -> Result<EnvMap> {
        let model = self
            .model
            .as_ref()
            .ok_or_else(|| crate::Error::invalid("ColibriConfig.model is required"))?;
        let mut env = EnvMap::from_process();
        env.set("SNAP", model.display().to_string());
        env.set("COLI_MODEL", model.display().to_string());
        env.set("SERVE", "1");
        env.set("SERVE_BATCH", "1");
        env.set("NGEN", self.max_tokens.to_string());
        env.set("KV_SLOTS", self.kv_slots.clamp(1, 16).to_string());
        env.set("CTX", self.ctx.to_string());
        if self.no_omp_tune {
            env.set("COLI_NO_OMP_TUNE", "1");
        }
        if let Some(ref eng) = self.engine {
            env.set("COLI_ENGINE", eng.display().to_string());
        }
        env.merge_overwrite(&self.extra_env);
        Ok(env)
    }

    /// Apply plan into serve env (plan setdefault, then serve flags, then extra).
    pub fn serve_env_with_plan(&self, plan: &PlacementPlan) -> Result<EnvMap> {
        let mut env = self.apply_plan(plan);
        let model = self
            .model
            .as_ref()
            .ok_or_else(|| crate::Error::invalid("ColibriConfig.model is required"))?;
        env.set("SNAP", model.display().to_string());
        env.set("COLI_MODEL", model.display().to_string());
        env.set("SERVE", "1");
        env.set("SERVE_BATCH", "1");
        Ok(env)
    }

    pub fn model_path(&self) -> Option<&Path> {
        self.model.as_deref()
    }

    /// True when the host must use the process serve path (kill-switch).
    ///
    /// Order: `COLIBRI_FORCE_PROCESS` env → `prefer_process` → lack of linked FFI.
    pub fn must_use_process(&self) -> bool {
        if force_process_from_env() {
            return true;
        }
        if self.prefer_process {
            return true;
        }
        #[cfg(feature = "ffi")]
        {
            !crate::ffi::ffi_link_available()
        }
        #[cfg(not(feature = "ffi"))]
        {
            true
        }
    }

    /// True when experimental in-process FFI is allowed by config/env.
    #[inline]
    pub fn prefer_ffi_path(&self) -> bool {
        !self.must_use_process()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{PlacementPlan, PlanCpu, PlanPolicy, PlanTiers, TierDisk, TierRam, TierVram};

    #[test]
    fn force_process_env_truthy_matrix() {
        assert!(!env_force_process(None::<&str>));
        assert!(!env_force_process(Some("")));
        assert!(!env_force_process(Some("0")));
        assert!(!env_force_process(Some("false")));
        assert!(!env_force_process(Some("NO")));
        assert!(!env_force_process(Some("off")));
        assert!(env_force_process(Some("1")));
        assert!(env_force_process(Some("true")));
        assert!(env_force_process(Some("yes")));
    }

    #[test]
    fn prefer_process_default_forces_process_path() {
        let cfg = ColibriConfig::default();
        assert!(cfg.prefer_process);
        assert!(cfg.must_use_process());
        assert!(!cfg.prefer_ffi_path());
    }

    #[test]
    fn prefer_process_false_allows_ffi_only_when_linked() {
        let cfg = ColibriConfig::default().prefer_process(false);
        assert!(!cfg.prefer_process);
        if force_process_from_env() {
            assert!(cfg.must_use_process());
            return;
        }
        #[cfg(feature = "ffi")]
        {
            assert!(!cfg.must_use_process());
            assert!(cfg.prefer_ffi_path());
        }
        #[cfg(not(feature = "ffi"))]
        {
            assert!(cfg.must_use_process());
            assert!(!cfg.prefer_ffi_path());
        }
    }

    fn stub_plan() -> PlacementPlan {
        PlacementPlan {
            version: 2,
            policy: PlanPolicy {
                name: "balanced".into(),
                preserve_quantization: true,
                preserve_router: true,
                quality_preserving: true,
            },
            model: Default::default(),
            cpu: PlanCpu {
                physical_cores: 8,
                sockets: 1,
                thread_policy: "physical-cores".into(),
            },
            tiers: PlanTiers {
                disk: TierDisk {
                    role: "cold-backing".into(),
                    model_bytes: 0,
                    available_bytes: 100 * GB,
                    cold_expert_bytes: 0,
                },
                ram: TierRam {
                    role: "resident+warm-experts".into(),
                    available_bytes: 32 * GB,
                    budget_bytes: 28 * GB,
                    dense_bytes: 0,
                    runtime_bytes: 0,
                    expert_cache_bytes: 0,
                    warm_expert_bytes: 0,
                    cache_slots_per_layer: 4,
                },
                vram: TierVram {
                    role: "hot-experts".into(),
                    devices: vec![],
                    budget_bytes: 0,
                    hot_expert_bytes: 0,
                    expert_capacity: 0,
                    requires_host_backing: false,
                },
            },
            expected_bottleneck: "CPU".into(),
            bottleneck_class: "memory".into(),
            projected_hit_rate: 1.0,
            tune: Default::default(),
            decisions: vec![],
            warnings: vec![],
            notes: vec![],
            ssd_probe_gbs: None,
            ssd_probe_state: "absent".into(),
        }
    }

    const GB: u64 = 1_000_000_000;

    #[test]
    fn setdefault_does_not_overwrite() {
        let mut env = EnvMap::new();
        env.set("OMP_NUM_THREADS", "4");
        env.setdefault("OMP_NUM_THREADS", "8");
        assert_eq!(env.get("OMP_NUM_THREADS"), Some("4"));
        env.setdefault("RAM_GB", "16");
        assert_eq!(env.get("RAM_GB"), Some("16"));
    }

    #[test]
    fn apply_plan_sets_policy_and_omp() {
        let plan = stub_plan();
        let cfg = ColibriConfig::default()
            .model("/tmp/m")
            .policy(Policy::Balanced);
        // Force balanced name on plan
        let mut plan = plan;
        plan.policy.name = "balanced".into();
        let env = cfg.apply_plan(&plan);
        assert_eq!(env.get("COLI_POLICY"), Some("balanced"));
        assert_eq!(env.get("OMP_NUM_THREADS"), Some("8"));
        assert_eq!(env.get("REPIN"), Some("64"));
        assert!(env.get("RAM_GB").is_some());
        assert_eq!(env.get("COLI_NO_OMP_TUNE"), Some("1"));
        assert_eq!(env.get("SNAP"), Some("/tmp/m"));
    }

    #[test]
    fn explicit_extra_env_wins() {
        let plan = stub_plan();
        let mut cfg = ColibriConfig::default().model("/tmp/m");
        cfg.extra_env.set("OMP_NUM_THREADS", "2");
        let env = cfg.apply_plan(&plan);
        assert_eq!(env.get("OMP_NUM_THREADS"), Some("2"));
    }
}
