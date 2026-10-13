//! rkyv ClientFrame / ServerFrame enums for the duplex stream.

use rkyv::{Archive, Deserialize, Serialize};

/// Wire protocol version carried in [`ServerFrame::Hello`].
///
/// Bumped to 2 when optional GBNF `grammar` was added on [`ClientFrame::Submit`].
pub const PROTOCOL_VERSION: u16 = 2;

/// Host → server frames.
#[derive(Debug, Clone, Archive, Serialize, Deserialize)]
#[rkyv(derive(Debug))]
pub enum ClientFrame {
    Submit {
        req_id: u64,
        slot: u32,
        max_tokens: u32,
        temperature: f32,
        top_p: f32,
        prompt: String,
        /// Optional GBNF grammar payload for the mux SUBMIT (7th header length field).
        grammar: Option<String>,
    },
    Stop {
        req_id: u64,
    },
    Cancel {
        req_id: u64,
    },
    Subscribe {
        /// Bitset; see [`crate::visual::Subscribe`].
        mask: u32,
    },
    Ping {
        nonce: u64,
    },
}

/// Server → host frames.
#[derive(Debug, Clone, Archive, Serialize, Deserialize)]
#[rkyv(derive(Debug))]
pub enum ServerFrame {
    Hello {
        protocol_version: u16,
        model_id: String,
        engine_name: String,
        kv_slots: u32,
    },
    Hwinfo {
        cores: u32,
        ram_total_gb: f32,
        ram_avail_gb: f32,
        gpus: u32,
        vram_total_gb: f32,
        cpu: String,
        gpu: String,
    },
    Tiers {
        vram_n: u32,
        ram_n: u32,
        disk_n: u32,
        vram_gb: f32,
        ram_gb: f32,
    },
    ExpertMap {
        rows: u16,
        cols: u16,
        cells: Vec<u8>,
    },
    ExpertHits {
        rows: u16,
        cols: u16,
        bits: Vec<u8>,
        seq: u64,
    },
    ProfTurn {
        seq: u64,
        wall_s: f32,
        prompt_tokens: u32,
        completion_tokens: u32,
        expert_disk_s: f32,
        expert_wait_s: f32,
        expert_matmul_s: f32,
        attention_s: f32,
        lm_head_s: f32,
        forwards: u64,
    },
    Token {
        req_id: u64,
        utf8: Vec<u8>,
    },
    Accept {
        req_id: u64,
        prompt_tokens: u32,
    },
    Done {
        req_id: u64,
        completion_tokens: u64,
        tokens_per_second: f32,
        cache_hit_percent: f32,
        rss_gb: f32,
        prompt_tokens: u64,
        length_limited: bool,
    },
    Scheduler {
        active: u32,
        queued: u32,
        capacity: u32,
        max_queue: u32,
        admitted: u32,
        completed: u32,
        rejected: u32,
        timed_out: u32,
        cancelled: u32,
        queue_timeout_s: f32,
    },
    Error {
        req_id: u64,
        code: String,
        message: String,
    },
    Pong {
        nonce: u64,
    },
}
