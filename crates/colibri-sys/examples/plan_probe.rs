//! Probe the machine (full inventory) and optionally plan placement for a model.
//!
//! Prints **every public inventory field** on [`MachineInfo`] and nested types so
//! hosts can see the complete API surface.
//!
//! ```bash
//! cargo run -p colibri-sys --example plan_probe
//! cargo run -p colibri-sys --example plan_probe -- /path/to/model
//! COLIBRI_MODEL_STORE=/data/models cargo run -p colibri-sys --example plan_probe
//! COLIBRI_PROBE_MODEL_STORE=/tmp cargo run -p colibri-sys --example plan_probe
//! ```

use colibri_sys::{ColibriConfig, GB, MachineInfo, PlacementPlan, PlanOptions, ProbeOptions};

fn main() {
    let model = std::env::args().nth(1);
    let store_override = std::env::var_os("COLIBRI_PROBE_MODEL_STORE").map(Into::into);

    // Prefer ProbeOptions (or MachineInfo::probe_for_config) so config/model_store
    // overrides are one-liner trivial for embedders.
    let opts = if let Some(path) = store_override {
        ProbeOptions {
            model_store: Some(path),
            disk_path: None,
        }
    } else if let Ok(path) = std::env::var("COLIBRI_CONFIG_MODEL_STORE") {
        // Demo of config → probe wiring (same as probe_for_config).
        let cfg = ColibriConfig::default().model_store(path);
        ProbeOptions::from_config(&cfg)
    } else {
        ProbeOptions::default()
    };

    match MachineInfo::probe_with(&opts) {
        Ok(m) => {
            print_full_inventory(&m);

            if let Some(path) = model {
                println!("=== placement plan ===");
                let plan_opts = PlanOptions {
                    available_memory: Some(m.available_memory),
                    gpus: Some(m.gpus.clone()),
                    physical_cpus: Some(m.physical_cores),
                    cpu_sockets: Some(m.sockets),
                    available_disk: Some(m.model_store.free_bytes),
                    ..Default::default()
                };
                match PlacementPlan::build(&path, &plan_opts) {
                    Ok(plan) => {
                        println!(
                            "plan version={} policy={} hit={:.2}% bottleneck={}",
                            plan.version,
                            plan.policy.name,
                            plan.projected_hit_rate * 100.0,
                            plan.bottleneck_class
                        );
                        println!(
                            "  ram_budget_gb={:.3} cap/layer={}",
                            plan.tiers.ram.budget_bytes as f64 / 1e9,
                            plan.tiers.ram.cache_slots_per_layer
                        );
                        for w in &plan.warnings {
                            println!("  warn: {w}");
                        }
                    }
                    Err(e) => eprintln!("plan error: {e}"),
                }
            }
        }
        Err(e) => {
            eprintln!("probe failed: {e}");
            std::process::exit(1);
        }
    }
}

/// Dump every public field on `MachineInfo` and nested inventory types.
fn print_full_inventory(m: &MachineInfo) {
    println!("=== memory ===");
    println!(
        "total_memory_bytes={} ({:.2} GB)",
        m.total_memory,
        m.total_memory as f64 / GB as f64
    );
    println!(
        "available_memory_bytes={} ({:.2} GB)",
        m.available_memory,
        m.available_memory as f64 / GB as f64
    );
    println!(
        "swap_total_bytes={} ({:.2} GB)",
        m.swap_total,
        m.swap_total as f64 / GB as f64
    );
    println!(
        "swap_free_bytes={} ({:.2} GB)",
        m.swap_free,
        m.swap_free as f64 / GB as f64
    );

    println!("=== cpu topology ===");
    println!("physical_cores={}", m.physical_cores);
    println!("logical_cores={}", m.logical_cores);
    println!("sockets={}", m.sockets);
    println!("threads_per_core={:?}", m.cpu.threads_per_core);

    println!("=== cpu identity / generation ===");
    println!("architecture={}", m.cpu.architecture);
    println!("vendor={:?}", m.cpu.vendor);
    println!("model_name={:?}", m.cpu.model_name);
    println!("family={:?}", m.cpu.family);
    println!("model_id={:?}", m.cpu.model);
    println!("stepping={:?}", m.cpu.stepping);
    println!("generation_hint={:?}", m.cpu.generation_hint);

    println!("=== big.LITTLE / hybrid ===");
    match &m.cpu.big_little {
        Some(bl) => {
            println!("big_little.hybrid={}", bl.hybrid);
            println!("big_little.capacity_classes={:?}", bl.capacity_classes);
            println!("big_little.note={}", bl.note);
        }
        None => println!("big_little=None (not detected on this host)"),
    }

    println!("=== simd / isa ({} catalog entries) ===", m.cpu.simd.len());
    for s in &m.cpu.simd {
        match &s.detail {
            Some(d) => println!(
                "  simd name={} family={} present={} detail={}",
                s.name, s.family, s.present, d
            ),
            None => println!(
                "  simd name={} family={} present={}",
                s.name, s.family, s.present
            ),
        }
    }
    if m.cpu.isa_flags.is_empty() {
        println!("isa_flags=(none extra)");
    } else {
        println!("isa_flags={}", m.cpu.isa_flags.join(" "));
    }

    println!("=== model store volume ===");
    println!("model_store.path={}", m.model_store.path.display());
    println!("model_store.source={:?}", m.model_store.source);
    println!(
        "model_store.free_bytes={} ({:.2} GB)",
        m.model_store.free_bytes,
        m.model_store.free_bytes as f64 / GB as f64
    );
    match m.model_store.total_bytes {
        Some(t) => println!(
            "model_store.total_bytes={} ({:.2} GB)",
            t,
            t as f64 / GB as f64
        ),
        None => println!("model_store.total_bytes=None"),
    }

    println!("=== gpus ({}) ===", m.gpus.len());
    if m.gpus.is_empty() {
        println!("  (none)");
    }
    for g in &m.gpus {
        println!(
            "  gpu index={} name={} total_bytes={} free_bytes={} (total_mib={} free_mib={})",
            g.index,
            g.name,
            g.total_bytes,
            g.free_bytes,
            g.total_bytes / (1024 * 1024),
            g.free_bytes / (1024 * 1024)
        );
    }

    println!("=== npus ({}) ===", m.npus.len());
    if m.npus.is_empty() {
        println!("  (none)");
    }
    for n in &m.npus {
        println!("  npu kind={}", n.kind);
        println!("  npu name={}", n.name);
        println!("  npu device_path={:?}", n.device_path);
        println!("  npu details={:?}", n.details);
    }

    println!("=== host libraries ({}) ===", m.host_libraries.len());
    if m.host_libraries.is_empty() {
        println!("  (none)");
    }
    for lib in &m.host_libraries {
        println!(
            "  library category={} name={} path={}",
            lib.category, lib.name, lib.path
        );
    }

    println!("=== legacy disk fields ===");
    println!("disk_free={:?}", m.disk_free);
    println!("disk_path={:?}", m.disk_path);
}
