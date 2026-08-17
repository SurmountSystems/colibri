//! Optional integration against a real C engine binary.
//!
//! ```bash
//! COLIBRI_TEST_ENGINE=./c/colibri COLIBRI_TEST_MODEL=./c/glm_tiny \
//!   cargo test -p colibri-sys --test engine_real -- --ignored
//! ```

#![cfg(feature = "runtime")]

use colibri_sys::{ColibriConfig, EngineHandle, GenerateRequest};

#[test]
#[ignore = "requires COLIBRI_TEST_ENGINE and COLIBRI_TEST_MODEL"]
fn real_engine_generate_smoke() {
    let engine = std::env::var("COLIBRI_TEST_ENGINE").expect("COLIBRI_TEST_ENGINE");
    let model = std::env::var("COLIBRI_TEST_MODEL").expect("COLIBRI_TEST_MODEL");
    let cfg = ColibriConfig::default()
        .model(model)
        .engine(engine)
        .max_tokens(8);
    let handle = EngineHandle::start_blocking(cfg).expect("start engine");
    let result = handle
        .generate(GenerateRequest {
            prompt: "hi".into(),
            max_tokens: 4,
            temperature: 0.0,
            top_p: 1.0,
            cache_slot: 0,
            grammar: None,
            request_id: None,
        })
        .expect("generate");
    // Tiny models may emit empty or short text; success is DONE without error.
    let _ = result.text;
    let _ = handle.stop();
}
