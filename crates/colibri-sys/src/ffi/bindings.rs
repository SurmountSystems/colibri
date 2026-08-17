//! Raw `extern "C"` surfaces for multi-family embed APIs.

#![allow(non_camel_case_types, dead_code)]

use std::os::raw::{c_char, c_double, c_float, c_int, c_void};

/* ---- DeepSeek V4 (deepseek_v4.h) ---- */

#[repr(C)]
pub struct ColiV4Engine {
    _private: [u8; 0],
}

#[repr(C)]
pub struct ColiV4Session {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ColiV4EngineOpenOptions {
    pub target_model_dir: *const c_char,
    pub memory_limit_bytes: u64,
    pub context_tokens: c_int,
    pub pin_slots_per_layer: c_int,
    pub repin_interval: u64,
    pub no_dspark: c_int,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ColiV4EngineMemorySummary {
    pub projected_bytes: u64,
    pub expert_cache_bytes: u64,
    pub slots_per_layer: c_int,
    pub dense_resident: c_int,
    pub head_resident: c_int,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ColiV4SessionCreateOptions {
    pub max_prompt_tokens: c_int,
    pub max_new_tokens_cap: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ColiV4SessionGenerateOptions {
    pub max_new_tokens: c_int,
    pub stop_at_sentence: c_int,
    pub no_dspark: c_int,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ColiV4SessionGenerateStats {
    pub prompt_tokens: c_int,
    pub generated_tokens: c_int,
    pub eos_stopped: c_int,
    pub time_to_first_token_sec: c_double,
    pub decode_sec: c_double,
    pub speculative_drafted: u64,
    pub speculative_accepted: u64,
}

pub type ColiV4SessionTokenFn = Option<
    unsafe extern "C" fn(
        user_data: *mut c_void,
        token: c_int,
        logit: c_float,
        position: c_int,
        ordinal: c_int,
    ) -> c_int,
>;

/* ---- Shared size + token CB (colibri_api.h) ---- */

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ColiModelSizeSummary {
    pub disk_bytes: u64,
    pub dense_bytes: u64,
    pub expert_bytes: u64,
    pub param_count: u64,
    pub has_param_count: c_int,
    pub family: [c_char; 32],
    pub engine_id: [c_char; 32],
}

pub type ColiTokenFn = Option<
    unsafe extern "C" fn(
        user_data: *mut c_void,
        token: c_int,
        logit: c_float,
        position: c_int,
        ordinal: c_int,
    ) -> c_int,
>;

#[repr(C)]
pub struct ColiGlmEngine {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ColiGlmOpenOptions {
    pub model_dir: *const c_char,
    pub cap: c_int,
    pub expert_bits: c_int,
    pub dense_bits: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ColiGlmGenerateOptions {
    pub max_new_tokens: c_int,
}

#[repr(C)]
pub struct ColiKimiEngine {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ColiKimiOpenOptions {
    pub model_dir: *const c_char,
    pub n_layers: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ColiKimiGenerateOptions {
    pub max_new_tokens: c_int,
}

#[repr(C)]
pub struct ColiInkEngine {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ColiInkOpenOptions {
    pub model_dir: *const c_char,
    pub cap: c_int,
    pub bits: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ColiInkGenerateOptions {
    pub max_new_tokens: c_int,
}

/* ---- Visual poll (colibri_api.h; matches c/telemetry.h layouts) ---- */

/// Unix niceness for in-process compute / OpenMP team (`COLI_COMPUTE_NICE`).
pub const COLI_COMPUTE_NICE: i32 = 10;

pub const COLI_VISUAL_HWINFO: u32 = 1 << 0;
pub const COLI_VISUAL_TIERS: u32 = 1 << 1;
pub const COLI_VISUAL_EMAP: u32 = 1 << 2;
pub const COLI_VISUAL_HITS: u32 = 1 << 3;
pub const COLI_VISUAL_PROF: u32 = 1 << 4;
pub const COLI_VISUAL_ALL: u32 = 0xffff_ffff;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ColiHwinfoSnap {
    pub cores: u32,
    pub ram_total_gb: c_double,
    pub ram_avail_gb: c_double,
    pub gpus: u32,
    pub vram_total_gb: c_double,
    pub cpu: [c_char; 128],
    pub gpu: [c_char; 128],
}

impl Default for ColiHwinfoSnap {
    fn default() -> Self {
        Self {
            cores: 0,
            ram_total_gb: 0.0,
            ram_avail_gb: 0.0,
            gpus: 0,
            vram_total_gb: 0.0,
            cpu: [0; 128],
            gpu: [0; 128],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ColiTiersSnap {
    pub vram_experts: u32,
    pub ram_experts: u32,
    pub disk_experts: u32,
    pub vram_gb: c_double,
    pub ram_gb: c_double,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ColiExpertGridDims {
    pub rows: u32,
    pub cols: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ColiProfSnap {
    pub wall_s: c_double,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub expert_disk_s: c_double,
    pub expert_wait_s: c_double,
    pub expert_matmul_s: c_double,
    pub attention_s: c_double,
    pub lm_head_s: c_double,
    pub forwards: u64,
    pub seq: u64,
    pub valid: c_int,
}

unsafe extern "C" {
    /* V4 */
    pub fn coli_v4_engine_open(
        engine: *mut *mut ColiV4Engine,
        options: *const ColiV4EngineOpenOptions,
        error: *mut c_char,
        error_size: usize,
    ) -> c_int;
    pub fn coli_v4_engine_destroy(engine: *mut ColiV4Engine);
    pub fn coli_v4_engine_target_model_dir(engine: *const ColiV4Engine) -> *const c_char;
    pub fn coli_v4_engine_memory_summary(
        engine: *const ColiV4Engine,
        summary: *mut ColiV4EngineMemorySummary,
    );
    pub fn coli_v4_session_create(
        session: *mut *mut ColiV4Session,
        engine: *mut ColiV4Engine,
        options: *const ColiV4SessionCreateOptions,
        error: *mut c_char,
        error_size: usize,
    ) -> c_int;
    pub fn coli_v4_session_destroy(session: *mut ColiV4Session);
    pub fn coli_v4_session_generate(
        session: *mut ColiV4Session,
        prompt: *const c_char,
        prompt_length: usize,
        options: *const ColiV4SessionGenerateOptions,
        on_token: ColiV4SessionTokenFn,
        user_data: *mut c_void,
        stats: *mut ColiV4SessionGenerateStats,
        error: *mut c_char,
        error_size: usize,
    ) -> c_int;
    pub fn coli_v4_session_generated_text(
        session: *const ColiV4Session,
        buffer: *mut c_char,
        buffer_size: usize,
        out_length: *mut usize,
    ) -> c_int;

    pub fn coli_nice_compute_threads(nice: c_int) -> c_int;
    pub fn coli_openmp_team_all_at_nice(nice: c_int) -> c_int;
    pub fn coli_embed_request_stop();
    pub fn coli_embed_clear_stop();
    pub fn coli_embed_should_stop() -> c_int;
    pub fn coli_prefill_should_run_leftover(remaining_tokens: c_int) -> c_int;

    /* GLM */
    pub fn coli_glm_engine_open(
        engine: *mut *mut ColiGlmEngine,
        options: *const ColiGlmOpenOptions,
        error: *mut c_char,
        error_size: usize,
    ) -> c_int;
    pub fn coli_glm_engine_destroy(engine: *mut ColiGlmEngine);
    pub fn coli_glm_engine_size(engine: *const ColiGlmEngine, out: *mut ColiModelSizeSummary);
    pub fn coli_glm_generate(
        engine: *mut ColiGlmEngine,
        prompt: *const c_char,
        prompt_len: usize,
        options: *const ColiGlmGenerateOptions,
        on_token: ColiTokenFn,
        user_data: *mut c_void,
        error: *mut c_char,
        error_size: usize,
    ) -> c_int;
    pub fn coli_glm_generate_ids(
        engine: *mut ColiGlmEngine,
        prompt_ids: *const c_int,
        n_prompt: c_int,
        options: *const ColiGlmGenerateOptions,
        on_token: ColiTokenFn,
        user_data: *mut c_void,
        error: *mut c_char,
        error_size: usize,
    ) -> c_int;
    pub fn coli_glm_visual_poll(
        engine: *mut ColiGlmEngine,
        want: u32,
        hwinfo: *mut ColiHwinfoSnap,
        tiers: *mut ColiTiersSnap,
        emap_dims: *mut ColiExpertGridDims,
        emap_cells: *mut u8,
        emap_cells_cap: usize,
        emap_cells_len: *mut usize,
        hits_dims: *mut ColiExpertGridDims,
        hits_bits: *mut u8,
        hits_bits_cap: usize,
        hits_bits_len: *mut usize,
        hits_seq: *mut u64,
        prof: *mut ColiProfSnap,
        error: *mut c_char,
        error_size: usize,
    ) -> c_int;

    /* Kimi */
    pub fn coli_kimi_engine_open(
        engine: *mut *mut ColiKimiEngine,
        options: *const ColiKimiOpenOptions,
        error: *mut c_char,
        error_size: usize,
    ) -> c_int;
    pub fn coli_kimi_engine_destroy(engine: *mut ColiKimiEngine);
    pub fn coli_kimi_engine_size(engine: *const ColiKimiEngine, out: *mut ColiModelSizeSummary);
    pub fn coli_kimi_generate(
        engine: *mut ColiKimiEngine,
        prompt: *const c_char,
        prompt_len: usize,
        options: *const ColiKimiGenerateOptions,
        on_token: ColiTokenFn,
        user_data: *mut c_void,
        error: *mut c_char,
        error_size: usize,
    ) -> c_int;
    pub fn coli_kimi_visual_poll(
        engine: *mut ColiKimiEngine,
        want: u32,
        hwinfo: *mut ColiHwinfoSnap,
        tiers: *mut ColiTiersSnap,
        emap_dims: *mut ColiExpertGridDims,
        emap_cells: *mut u8,
        emap_cells_cap: usize,
        emap_cells_len: *mut usize,
        hits_dims: *mut ColiExpertGridDims,
        hits_bits: *mut u8,
        hits_bits_cap: usize,
        hits_bits_len: *mut usize,
        hits_seq: *mut u64,
        prof: *mut ColiProfSnap,
        error: *mut c_char,
        error_size: usize,
    ) -> c_int;

    /* Inkling */
    pub fn coli_ink_engine_open(
        engine: *mut *mut ColiInkEngine,
        options: *const ColiInkOpenOptions,
        error: *mut c_char,
        error_size: usize,
    ) -> c_int;
    pub fn coli_ink_engine_destroy(engine: *mut ColiInkEngine);
    pub fn coli_ink_engine_size(engine: *const ColiInkEngine, out: *mut ColiModelSizeSummary);
    pub fn coli_ink_generate(
        engine: *mut ColiInkEngine,
        prompt: *const c_char,
        prompt_len: usize,
        options: *const ColiInkGenerateOptions,
        on_token: ColiTokenFn,
        user_data: *mut c_void,
        error: *mut c_char,
        error_size: usize,
    ) -> c_int;
    pub fn coli_ink_visual_poll(
        engine: *mut ColiInkEngine,
        want: u32,
        hwinfo: *mut ColiHwinfoSnap,
        tiers: *mut ColiTiersSnap,
        emap_dims: *mut ColiExpertGridDims,
        emap_cells: *mut u8,
        emap_cells_cap: usize,
        emap_cells_len: *mut usize,
        hits_dims: *mut ColiExpertGridDims,
        hits_bits: *mut u8,
        hits_bits_cap: usize,
        hits_bits_len: *mut usize,
        hits_seq: *mut u64,
        prof: *mut ColiProfSnap,
        error: *mut c_char,
        error_size: usize,
    ) -> c_int;
}
