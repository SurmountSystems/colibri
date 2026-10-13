//! Placement plan golden-style fixture tests (fixed machine geometry).

use colibri_sys::{GB, GpuDevice, ModelInfo, PlacementPlan, PlanOptions, environment_for_plan};
use serde_json::json;

fn stub_info() -> ModelInfo {
    ModelInfo {
        path: std::path::PathBuf::from("/tmp/colibri-sys-golden-model"),
        family: Some(colibri_sys::ModelFamily::Glm),
        engine_id: "colibri".into(),
        model_type: Some("glm_moe_dsa".into()),
        shards: 2,
        model_bytes: 100 * GB,
        disk_bytes: 100 * GB,
        param_count: None,
        dense_bytes: 10 * GB,
        expert_bytes: 80 * GB,
        expert_count: 200,
        expert_layers: 20,
        typical_expert_bytes: 100_000_000,
        per_cap_bytes: 2_000_000_000,
        has_config: true,
        has_tokenizer: true,
        config: json!({
            "num_hidden_layers": 20,
            "n_routed_experts": 128,
            "kv_lora_rank": 512,
            "qk_rope_head_dim": 64,
            "num_attention_heads": 64,
            "qk_nope_head_dim": 128,
            "v_head_dim": 128,
        }),
        shard_names: vec!["a.safetensors".into(), "b.safetensors".into()],
    }
}

#[test]
fn plan_v2_shape_and_env_stable() {
    let info = stub_info();
    let opts = PlanOptions {
        policy: "balanced".into(),
        ram_gb: 64.0,
        context: 8192,
        available_memory: Some(128 * GB),
        available_disk: Some(2_000 * GB),
        gpus: Some(vec![GpuDevice {
            index: 0,
            name: "FixtureGPU".into(),
            total_bytes: 48 * GB,
            free_bytes: 40 * GB,
            ..Default::default()
        }]),
        physical_cpus: Some(32),
        cpu_sockets: Some(2),
        ..Default::default()
    };
    let plan = PlacementPlan::build_from_info(&info, &opts).unwrap();
    assert_eq!(plan.version, 2);
    assert_eq!(plan.policy.name, "balanced");
    assert!(plan.policy.quality_preserving);
    assert_eq!(plan.cpu.physical_cores, 32);
    assert_eq!(plan.cpu.sockets, 2);
    assert_eq!(plan.tiers.ram.budget_bytes, 64 * GB);
    assert!(!plan.tiers.vram.devices.is_empty());
    assert!(plan.projected_hit_rate >= 0.0 && plan.projected_hit_rate <= 1.0);

    let env = environment_for_plan(&plan, None, true);
    assert_eq!(env.get("COLI_POLICY"), Some("balanced"));
    assert_eq!(env.get("OMP_NUM_THREADS"), Some("32"));
    assert_eq!(env.get("REPIN"), Some("64"));
    assert_eq!(env.get("COLI_CUDA"), Some("1"));
    assert_eq!(env.get("COLI_GPU"), Some("0"));
    assert!(env.get("RAM_GB").is_some());
    assert!(env.get("CUDA_EXPERT_GB").is_some());
    // Must not set OMP_PROC_BIND (Python plan contract #325).
    assert!(env.get("OMP_PROC_BIND").is_none());
}

#[test]
fn experimental_fast_not_quality_preserving() {
    let info = stub_info();
    let opts = PlanOptions {
        policy: "experimental-fast".into(),
        available_memory: Some(64 * GB),
        available_disk: Some(500 * GB),
        gpus: Some(vec![]),
        physical_cpus: Some(8),
        ..Default::default()
    };
    let plan = PlacementPlan::build_from_info(&info, &opts).unwrap();
    assert!(!plan.policy.quality_preserving);
    assert!(!plan.policy.preserve_quantization);
}
