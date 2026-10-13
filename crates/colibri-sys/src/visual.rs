//! Visual / telemetry function APIs.
//!
//! Typed snapshots matching the live dashboard contracts consumed by
//! `web/src` via `/health`, `/experts`, `/profile` (see explore-visual-telemetry).
//! Binary layouts match `c/telemetry.h` (EMAP `tier<<6|heat`, HITS little-endian bits).
//! The same cell/bit packing is used by the process serve mux and the embed
//! `coli_*_visual_poll` path (no hex round-trip required for FFI).

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Decode hex-encoded map/hits payloads from the line protocol.
pub fn decode_hex_bytes(hex_str: &str) -> Result<Vec<u8>> {
    hex::decode(hex_str).map_err(|e| Error::protocol(format!("invalid hex telemetry: {e}")))
}

/// Pack EMAP cell: `byte = (tier << 6) | heat` (matches `c/telemetry.h`).
#[inline]
pub fn pack_expert_cell(tier: u8, heat: u8) -> u8 {
    ((tier & 0x3) << 6) | (heat & 0x3f)
}

/// Unpack EMAP cell into `(tier, heat)`.
#[inline]
pub fn unpack_expert_cell(byte: u8) -> (u8, u8) {
    (byte >> 6, byte & 0x3f)
}

/// Hardware snapshot from HWINFO lines / `ColiHwinfoSnap`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HwinfoSnap {
    pub cores: u32,
    pub ram_total_gb: f64,
    pub ram_avail_gb: f64,
    pub gpus: u32,
    pub vram_total_gb: f64,
    pub cpu: String,
    pub gpu: String,
}

/// Tier counts from TIERS lines / `ColiTiersSnap`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TiersSnap {
    pub vram: u32,
    pub ram: u32,
    pub disk: u32,
    pub vram_gb: f64,
    pub ram_gb: f64,
}

/// One PROF turn (engine PROF line fields / `ColiProfSnap` body).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProfileTurn {
    pub wall_s: f64,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub expert_disk_s: f64,
    pub expert_wait_s: f64,
    pub expert_matmul_s: f64,
    pub attention_s: f64,
    pub lm_head_s: f64,
    pub forwards: u64,
}

/// Expert map (cortex grid): row-major `u8` cells.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpertMap {
    pub rows: u32,
    pub cols: u32,
    /// Length `rows * cols`; each byte is `(tier << 6) | heat`.
    pub cells: Vec<u8>,
}

impl ExpertMap {
    /// Build from raw EMAP bytes (embed poll or decoded hex).
    pub fn from_cells(rows: u32, cols: u32, cells: Vec<u8>) -> Self {
        Self { rows, cols, cells }
    }

    pub fn from_hex(rows: u32, cols: u32, hex: &str) -> Result<Self> {
        let cells = decode_hex_bytes(hex)?;
        Ok(Self::from_cells(rows, cols, cells))
    }

    pub fn tier_at(&self, row: u32, col: u32) -> Option<u8> {
        let i = (row as usize) * (self.cols as usize) + (col as usize);
        self.cells.get(i).map(|b| b >> 6)
    }

    pub fn heat_at(&self, row: u32, col: u32) -> Option<u8> {
        let i = (row as usize) * (self.cols as usize) + (col as usize);
        self.cells.get(i).map(|b| b & 0x3f)
    }
}

/// Hits bitmap + sequence for Brain pulse animation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpertHits {
    pub rows: u32,
    pub cols: u32,
    /// `ceil(rows*cols/8)` bytes, little-endian bit packing.
    pub bits: Vec<u8>,
    pub seq: u64,
}

impl ExpertHits {
    /// Build from raw HITS bytes (embed poll or decoded hex).
    pub fn from_bits(rows: u32, cols: u32, bits: Vec<u8>, seq: u64) -> Self {
        Self {
            rows,
            cols,
            bits,
            seq,
        }
    }

    pub fn from_hex(rows: u32, cols: u32, hex: &str, seq: u64) -> Result<Self> {
        let bits = decode_hex_bytes(hex)?;
        Ok(Self::from_bits(rows, cols, bits, seq))
    }

    /// Whether expert index `i` (row-major) was hit since the previous emit.
    pub fn hit(&self, index: usize) -> bool {
        let byte = index >> 3;
        let bit = index & 7;
        self.bits
            .get(byte)
            .map(|b| (b >> bit) & 1 == 1)
            .unwrap_or(false)
    }
}

/// Binary fragments from embed `coli_*_visual_poll` (or unit-test fixtures).
///
/// Fields that are `None` leave the corresponding snapshot slots unchanged
/// (except profile, which only appends when `profile` is `Some` with a newer seq).
#[derive(Debug, Clone, Default)]
pub struct BinaryPollParts {
    pub hwinfo: Option<HwinfoSnap>,
    pub tiers: Option<TiersSnap>,
    pub expert_map: Option<ExpertMap>,
    pub expert_hits: Option<ExpertHits>,
    /// `(engine prof seq, turn)` — append only when `seq` advances past `profile_seq`.
    pub profile: Option<(u64, ProfileTurn)>,
}

/// Aggregated visual state for function APIs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VisualSnapshot {
    pub tiers: Option<TiersSnap>,
    pub hwinfo: Option<HwinfoSnap>,
    pub expert_map: Option<ExpertMap>,
    pub expert_hits: Option<ExpertHits>,
    pub profile: Vec<ProfileTurn>,
    pub profile_seq: u64,
    pub hits_seq: u64,
}

impl VisualSnapshot {
    /// Merge binary poll / fixture parts into this snapshot (no subprocess).
    ///
    /// Shared by process-hex decode helpers and FFI `coli_*_visual_poll` mapping.
    pub fn absorb_binary_poll(&mut self, parts: BinaryPollParts) {
        if let Some(h) = parts.hwinfo {
            self.hwinfo = Some(h);
        }
        if let Some(t) = parts.tiers {
            self.tiers = Some(t);
        }
        if let Some(m) = parts.expert_map {
            if m.rows > 0 && m.cols > 0 && !m.cells.is_empty() {
                self.expert_map = Some(m);
            }
        }
        if let Some(h) = parts.expert_hits {
            if h.rows > 0 && h.cols > 0 {
                self.hits_seq = h.seq;
                self.expert_hits = Some(h);
            }
        }
        if let Some((seq, turn)) = parts.profile {
            if seq > self.profile_seq {
                self.profile.push(turn);
                if self.profile.len() > 120 {
                    let drain = self.profile.len() - 120;
                    self.profile.drain(0..drain);
                }
                self.profile_seq = seq;
            }
        }
    }

    /// Pull latest telemetry from a serve client into this snapshot.
    #[cfg(feature = "runtime")]
    pub fn absorb_from_client(&mut self, client: &crate::engine::ServeClient) {
        if let Some(t) = client.tiers() {
            self.tiers = Some(t);
        }
        if let Some(h) = client.hwinfo() {
            self.hwinfo = Some(h);
        }
        if let Some((rows, cols, hex)) = client.emap_hex() {
            if let Ok(m) = ExpertMap::from_hex(rows, cols, &hex) {
                self.expert_map = Some(m);
            }
        }
        if let Some(hex) = client.hits_hex() {
            let rows = self.expert_map.as_ref().map(|m| m.rows).unwrap_or(0);
            let cols = self.expert_map.as_ref().map(|m| m.cols).unwrap_or(0);
            let seq = client.hits_seq();
            if let Ok(h) = ExpertHits::from_hex(rows, cols, &hex, seq) {
                self.expert_hits = Some(h);
                self.hits_seq = seq;
            }
        }
        self.profile = client.profile();
        self.profile_seq = client.profile_seq();
    }
}

/// Subscribe interest bitset for duplex streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Subscribe(pub u32);

impl Subscribe {
    pub const VISUAL: Subscribe = Subscribe(1 << 0);
    pub const TOKENS: Subscribe = Subscribe(1 << 1);
    pub const PROFILE: Subscribe = Subscribe(1 << 2);
    pub const HW: Subscribe = Subscribe(1 << 3);
    pub const SCHEDULER: Subscribe = Subscribe(1 << 4);
    pub const ALL: Subscribe = Subscribe(0xffff_ffff);

    pub fn contains(self, other: Subscribe) -> bool {
        self.0 & other.0 != 0
    }

    pub fn union(self, other: Subscribe) -> Subscribe {
        Subscribe(self.0 | other.0)
    }
}

impl std::ops::BitOr for Subscribe {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expert_map_from_cells() {
        let cells = vec![pack_expert_cell(1, 3), pack_expert_cell(2, 0)];
        assert_eq!(cells, vec![0x43, 0x80]);
        let hex = hex::encode(&cells);
        assert_eq!(hex, "4380");
        let map = ExpertMap::from_hex(1, 2, &hex).unwrap();
        assert_eq!(map.tier_at(0, 0), Some(1));
        assert_eq!(map.heat_at(0, 0), Some(3));
        assert_eq!(map.tier_at(0, 1), Some(2));
        let from_raw = ExpertMap::from_cells(1, 2, cells);
        assert_eq!(from_raw.tier_at(0, 0), Some(1));
        assert_eq!(unpack_expert_cell(0x43), (1, 3));
        assert_eq!(unpack_expert_cell(0x80), (2, 0));
    }

    #[test]
    fn hits_bit_packing() {
        // expert 0 and 3 set in first byte (fixture from visual poll C report)
        let bits = vec![0x09];
        let hex = hex::encode(&bits);
        assert_eq!(hex, "09");
        let hits = ExpertHits::from_hex(1, 8, &hex, 7).unwrap();
        assert!(hits.hit(0));
        assert!(!hits.hit(1));
        assert!(hits.hit(3));
        assert_eq!(hits.seq, 7);
        let from_raw = ExpertHits::from_bits(1, 8, bits, 7);
        assert!(from_raw.hit(0) && from_raw.hit(3));
    }

    /// Fixed binary fixtures → non-empty `VisualSnapshot` without a subprocess.
    ///
    /// Matches C visual-poll packing oracle (`0x43`/`0x80` EMAP, `0x09` HITS).
    #[test]
    fn visual_snapshot_from_fixed_binary_fixtures() {
        let mut snap = VisualSnapshot::default();
        assert!(snap.expert_map.is_none());
        assert!(snap.profile.is_empty());

        snap.absorb_binary_poll(BinaryPollParts {
            hwinfo: Some(HwinfoSnap {
                cores: 8,
                ram_total_gb: 32.0,
                ram_avail_gb: 16.0,
                gpus: 1,
                vram_total_gb: 24.0,
                cpu: "TestCPU".into(),
                gpu: "TestGPU".into(),
            }),
            tiers: Some(TiersSnap {
                vram: 2,
                ram: 4,
                disk: 10,
                vram_gb: 1.5,
                ram_gb: 3.0,
            }),
            expert_map: Some(ExpertMap::from_cells(1, 2, vec![0x43, 0x80])),
            expert_hits: Some(ExpertHits::from_bits(1, 8, vec![0x09], 1)),
            profile: Some((
                1,
                ProfileTurn {
                    wall_s: 0.5,
                    prompt_tokens: 4,
                    completion_tokens: 2,
                    expert_disk_s: 0.01,
                    expert_wait_s: 0.0,
                    expert_matmul_s: 0.1,
                    attention_s: 0.05,
                    lm_head_s: 0.02,
                    forwards: 6,
                },
            )),
        });

        assert!(snap.hwinfo.is_some());
        assert_eq!(snap.hwinfo.as_ref().unwrap().cores, 8);
        assert_eq!(snap.tiers.as_ref().unwrap().vram, 2);
        let map = snap.expert_map.as_ref().expect("emap from fixture");
        assert_eq!(map.tier_at(0, 0), Some(1));
        assert_eq!(map.heat_at(0, 0), Some(3));
        assert_eq!(map.tier_at(0, 1), Some(2));
        let hits = snap.expert_hits.as_ref().expect("hits from fixture");
        assert!(hits.hit(0));
        assert!(hits.hit(3));
        assert_eq!(snap.hits_seq, 1);
        assert_eq!(snap.profile.len(), 1);
        assert_eq!(snap.profile_seq, 1);
        assert_eq!(snap.profile[0].completion_tokens, 2);

        // Same prof seq must not duplicate turns.
        snap.absorb_binary_poll(BinaryPollParts {
            profile: Some((1, ProfileTurn::default())),
            ..Default::default()
        });
        assert_eq!(snap.profile.len(), 1);

        // Newer seq appends.
        snap.absorb_binary_poll(BinaryPollParts {
            profile: Some((
                2,
                ProfileTurn {
                    completion_tokens: 3,
                    ..Default::default()
                },
            )),
            ..Default::default()
        });
        assert_eq!(snap.profile.len(), 2);
        assert_eq!(snap.profile_seq, 2);
    }
}
