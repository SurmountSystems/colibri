//! Embed a C engine via serve mux (requires a built engine + model).
//!
//! ```bash
//! # Mock-friendly path is covered by unit tests. For a real engine:
//! export COLIBRI_TEST_ENGINE=./c/colibri
//! export COLIBRI_TEST_MODEL=./c/glm_tiny
//! cargo run -p colibri-sys --example embed_chat --features runtime,tokio
//! ```
//!
//! Without env vars this example prints usage and exits 0 (so CI stays green).

use colibri_sys::{ColibriConfig, EngineHandle, GenerateRequest, PlacementPlan, PlanOptions};

fn main() {
    let engine = std::env::var("COLIBRI_TEST_ENGINE")
        .or_else(|_| std::env::var("COLI_ENGINE"))
        .ok();
    let model = std::env::var("COLIBRI_TEST_MODEL")
        .or_else(|_| std::env::var("COLI_MODEL"))
        .ok();

    let (Some(engine), Some(model)) = (engine, model) else {
        eprintln!(
            "embed_chat: set COLIBRI_TEST_ENGINE and COLIBRI_TEST_MODEL (or COLI_ENGINE / COLI_MODEL)\n\
             unit tests cover the mux client with a mock peer; this example talks to a real engine."
        );
        return;
    };

    let cfg = ColibriConfig::default()
        .model(&model)
        .engine(&engine)
        .max_tokens(32)
        .kv_slots(1);

    let plan = PlacementPlan::build(
        &model,
        &PlanOptions {
            context: cfg.ctx,
            ..Default::default()
        },
    );
    let handle = match plan {
        Ok(p) => EngineHandle::start_with_plan(cfg, &p),
        Err(e) => {
            eprintln!("plan warning (starting without plan): {e}");
            EngineHandle::start_blocking(cfg)
        }
    };

    let handle = match handle {
        Ok(h) => h,
        Err(e) => {
            eprintln!("failed to start engine: {e}");
            std::process::exit(1);
        }
    };

    match handle.generate(GenerateRequest {
        prompt: "Hello".into(),
        max_tokens: 16,
        temperature: 0.8,
        top_p: 0.95,
        cache_slot: 0,
        grammar: None,
        request_id: None,
    }) {
        Ok(r) => {
            println!("text: {}", r.text);
            println!(
                "stats: completion={} tok/s={:.2} hit%={:.1}",
                r.stats.completion_tokens, r.stats.tokens_per_second, r.stats.cache_hit_percent
            );
            if let Some(t) = handle.tiers() {
                println!("tiers: vram={} ram={} disk={}", t.vram, t.ram, t.disk);
            }
        }
        Err(e) => {
            eprintln!("generate failed: {e}");
            std::process::exit(1);
        }
    }

    let _ = handle.stop();
}
