//! Profiling page data model (web SPA phase model port).
//!
//! Pure helpers for share bars, throughput columns, and table rows. GPUI chrome
//! lives in `main.rs`; this module stays unit-testable without a display.

use colibri_sys::ProfileTurn;

use crate::theme::ThemePalette;

/// How many recent turns the Profiling page charts (web uses ~40).
pub const PROF_CHART_N: usize = 40;

/// Phase slice matching `web/src/Profiling.tsx` PHASES.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProfilePhase {
    IoWait,
    ExpertMatmul,
    Attention,
    LmHead,
    Other,
}

impl ProfilePhase {
    pub const ALL: [ProfilePhase; 5] = [
        ProfilePhase::IoWait,
        ProfilePhase::ExpertMatmul,
        ProfilePhase::Attention,
        ProfilePhase::LmHead,
        ProfilePhase::Other,
    ];

    pub fn i18n_key(self) -> &'static str {
        match self {
            ProfilePhase::IoWait => "profile.ioWait",
            ProfilePhase::ExpertMatmul => "profile.expertMatmul",
            ProfilePhase::Attention => "profile.attention",
            ProfilePhase::LmHead => "profile.lmHead",
            ProfilePhase::Other => "profile.other",
        }
    }

    /// Phase color from the active theme palette (DOGE-safe when palette is DOGE).
    pub fn color_in(self, p: &ThemePalette) -> u32 {
        match self {
            ProfilePhase::IoWait => p.phase_io_wait,
            ProfilePhase::ExpertMatmul => p.phase_matmul,
            ProfilePhase::Attention => p.phase_attention,
            ProfilePhase::LmHead => p.phase_lm_head,
            ProfilePhase::Other => p.phase_other,
        }
    }
}

/// Derived turn metrics (web `derive`).
#[derive(Debug, Clone, PartialEq)]
pub struct DerivedTurn {
    pub wall_s: f64,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub expert_disk_s: f64,
    pub expert_wait_s: f64,
    pub expert_matmul_s: f64,
    pub attention_s: f64,
    pub lm_head_s: f64,
    pub other_s: f64,
    pub forwards: u64,
    pub toks: f64,
}

impl DerivedTurn {
    pub fn from_profile(turn: &ProfileTurn) -> Self {
        let other_s = (turn.wall_s
            - turn.expert_wait_s
            - turn.expert_matmul_s
            - turn.attention_s
            - turn.lm_head_s)
            .max(0.0);
        let toks = if turn.wall_s > 0.0 {
            turn.completion_tokens as f64 / turn.wall_s
        } else {
            0.0
        };
        Self {
            wall_s: turn.wall_s,
            prompt_tokens: turn.prompt_tokens,
            completion_tokens: turn.completion_tokens,
            expert_disk_s: turn.expert_disk_s,
            expert_wait_s: turn.expert_wait_s,
            expert_matmul_s: turn.expert_matmul_s,
            attention_s: turn.attention_s,
            lm_head_s: turn.lm_head_s,
            other_s,
            forwards: turn.forwards,
            toks,
        }
    }

    pub fn phase_s(&self, phase: ProfilePhase) -> f64 {
        match phase {
            ProfilePhase::IoWait => self.expert_wait_s,
            ProfilePhase::ExpertMatmul => self.expert_matmul_s,
            ProfilePhase::Attention => self.attention_s,
            ProfilePhase::LmHead => self.lm_head_s,
            ProfilePhase::Other => self.other_s,
        }
    }

    pub fn tokens_per_forward(&self) -> Option<f64> {
        if self.forwards > 0 {
            Some(self.completion_tokens as f64 / self.forwards as f64)
        } else {
            None
        }
    }
}

/// Format seconds like web: ≥10 → one decimal, else two.
pub fn format_seconds(value: f64) -> String {
    if value >= 10.0 {
        format!("{value:.1}s")
    } else {
        format!("{value:.2}s")
    }
}

/// One share-bar segment (fraction of total wall, plus absolute seconds).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShareSegment {
    pub phase: ProfilePhase,
    pub seconds: f64,
    /// 0..1 share of total wall; 0 means omit from bar.
    pub share: f64,
}

/// Build share-bar segments for one or more turns (web ShareBar).
pub fn share_segments(turns: &[DerivedTurn]) -> (f64, Vec<ShareSegment>) {
    let total: f64 = turns.iter().map(|t| t.wall_s).sum();
    let mut segs = Vec::with_capacity(5);
    for phase in ProfilePhase::ALL {
        let value: f64 = turns.iter().map(|t| t.phase_s(phase)).sum();
        let share = if total > 0.0 { value / total } else { 0.0 };
        segs.push(ShareSegment {
            phase,
            seconds: value,
            share,
        });
    }
    (total, segs)
}

/// Column heights for throughput chart (0..1 of peak).
pub fn throughput_heights(turns: &[DerivedTurn]) -> (f64, Vec<f64>) {
    let peak = turns.iter().map(|t| t.toks).fold(1e-9_f64, f64::max);
    let heights = turns
        .iter()
        .map(|t| (t.toks / peak).clamp(0.0, 1.0))
        .collect();
    (peak, heights)
}

/// Stacked phase heights per turn (each phase 0..1 of peak wall).
pub fn phase_stack_heights(turns: &[DerivedTurn]) -> (f64, Vec<[f64; 5]>) {
    let peak = turns.iter().map(|t| t.wall_s).fold(1e-9_f64, f64::max);
    let stacks = turns
        .iter()
        .map(|t| {
            let mut h = [0.0_f64; 5];
            for (i, phase) in ProfilePhase::ALL.iter().enumerate() {
                h[i] = (t.phase_s(*phase) / peak).clamp(0.0, 1.0);
            }
            h
        })
        .collect();
    (peak, stacks)
}

/// Last N derived turns (oldest → newest), web recent window.
pub fn recent_turns(raw: &[ProfileTurn], last_n: usize) -> Vec<DerivedTurn> {
    if last_n == 0 || raw.is_empty() {
        return Vec::new();
    }
    let start = raw.len().saturating_sub(last_n);
    raw[start..].iter().map(DerivedTurn::from_profile).collect()
}

/// Badge helpers (topbar live metrics).
pub fn format_badge_tokens(n: u64) -> String {
    format!("{n} tokens")
}

pub fn format_badge_tok_per_sec(n: f64) -> String {
    format!("{n:.1} tok/s")
}

pub fn format_badge_ttft_ms(ms: f64) -> String {
    format!("TTFT {ms:.0} ms")
}

/// Proportional tier bar fractions (VRAM, RAM, disk) summing to 1 when total > 0.
pub fn tier_share_fractions(vram: u32, ram: u32, disk: u32) -> (f32, f32, f32) {
    let total = vram.saturating_add(ram).saturating_add(disk);
    if total == 0 {
        return (0.0, 0.0, 0.0);
    }
    let t = total as f32;
    (vram as f32 / t, ram as f32 / t, disk as f32 / t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(completion: u32, wall: f64) -> ProfileTurn {
        ProfileTurn {
            wall_s: wall,
            prompt_tokens: 10,
            completion_tokens: completion,
            expert_disk_s: 0.1,
            expert_wait_s: 0.2,
            expert_matmul_s: 0.5,
            attention_s: 0.2,
            lm_head_s: 0.05,
            forwards: 30,
        }
    }

    #[test]
    fn derive_other_and_toks() {
        let t = DerivedTurn::from_profile(&sample(20, 1.0));
        // 1.0 - 0.2 - 0.5 - 0.2 - 0.05 = 0.05
        assert!((t.other_s - 0.05).abs() < 1e-9, "other={}", t.other_s);
        assert!((t.toks - 20.0).abs() < 1e-9, "toks={}", t.toks);
        assert!((t.tokens_per_forward().unwrap() - 20.0 / 30.0).abs() < 1e-9);
    }

    #[test]
    fn derive_zero_wall() {
        let mut p = sample(10, 0.0);
        p.wall_s = 0.0;
        let t = DerivedTurn::from_profile(&p);
        assert_eq!(t.toks, 0.0);
    }

    #[test]
    fn format_seconds_threshold() {
        assert_eq!(format_seconds(9.99), "9.99s");
        assert_eq!(format_seconds(10.0), "10.0s");
        assert_eq!(format_seconds(0.05), "0.05s");
    }

    #[test]
    fn share_segments_sum_to_one() {
        let turns = vec![DerivedTurn::from_profile(&sample(20, 1.0))];
        let (total, segs) = share_segments(&turns);
        assert!((total - 1.0).abs() < 1e-9);
        let sum: f64 = segs.iter().map(|s| s.share).sum();
        assert!((sum - 1.0).abs() < 1e-6, "sum={sum}");
        assert_eq!(segs.len(), 5);
        assert_eq!(segs[0].phase, ProfilePhase::IoWait);
    }

    #[test]
    fn multi_turn_window_share() {
        let turns = vec![
            DerivedTurn::from_profile(&sample(10, 1.0)),
            DerivedTurn::from_profile(&sample(20, 2.0)),
        ];
        let (total, _) = share_segments(&turns);
        assert!((total - 3.0).abs() < 1e-9);
    }

    #[test]
    fn throughput_heights_peak() {
        let turns = vec![
            DerivedTurn::from_profile(&sample(10, 1.0)), // 10 tok/s
            DerivedTurn::from_profile(&sample(20, 1.0)), // 20 tok/s
        ];
        let (peak, heights) = throughput_heights(&turns);
        assert!((peak - 20.0).abs() < 1e-9);
        assert!((heights[0] - 0.5).abs() < 1e-9);
        assert!((heights[1] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn phase_stack_heights_len() {
        let turns = vec![DerivedTurn::from_profile(&sample(20, 1.0))];
        let (peak, stacks) = phase_stack_heights(&turns);
        assert!((peak - 1.0).abs() < 1e-9);
        assert_eq!(stacks.len(), 1);
        assert_eq!(stacks[0].len(), 5);
    }

    #[test]
    fn recent_turns_window() {
        let raw: Vec<_> = (0..5).map(|i| sample(10 + i, 1.0)).collect();
        let r = recent_turns(&raw, 2);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].completion_tokens, 13);
        assert_eq!(r[1].completion_tokens, 14);
        assert!(recent_turns(&raw, 0).is_empty());
    }

    #[test]
    fn badge_formatters() {
        assert_eq!(format_badge_tokens(42), "42 tokens");
        assert_eq!(format_badge_tok_per_sec(12.34), "12.3 tok/s");
        assert_eq!(format_badge_ttft_ms(85.4), "TTFT 85 ms");
    }

    #[test]
    fn tier_shares() {
        let (v, r, d) = tier_share_fractions(10, 20, 70);
        assert!((v - 0.1).abs() < 1e-6);
        assert!((r - 0.2).abs() < 1e-6);
        assert!((d - 0.7).abs() < 1e-6);
        assert_eq!(tier_share_fractions(0, 0, 0), (0.0, 0.0, 0.0));
    }

    #[test]
    fn phase_colors_match_web() {
        let mint = crate::theme::mint_palette();
        assert_eq!(ProfilePhase::IoWait.color_in(&mint), 0x3987e5);
        assert_eq!(ProfilePhase::ExpertMatmul.color_in(&mint), 0x199e70);
        assert_eq!(ProfilePhase::Attention.color_in(&mint), 0xc98500);
        assert_eq!(ProfilePhase::LmHead.color_in(&mint), 0x008300);
        assert_eq!(ProfilePhase::Other.color_in(&mint), 0x9085e9);
    }

    #[test]
    fn phase_colors_doge_are_pure_eight() {
        use crate::theme::DOGE_EIGHT;
        let doge = crate::theme::doge_palette();
        for ph in ProfilePhase::ALL {
            let c = ph.color_in(&doge);
            assert!(
                DOGE_EIGHT.contains(&c),
                "phase {ph:?} color 0x{c:06X} not in DOGE eight"
            );
        }
    }
}
