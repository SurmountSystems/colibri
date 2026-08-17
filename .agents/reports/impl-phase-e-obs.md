# Phase E: observability polish (implement report)

**Date:** 2026-08-10
**Scope:** colibri-native live HWINFO strip, Brain pulse multi-frame decay + heat fidelity confirmation, PROF text polish.
**Architecture (unchanged):** GPUI → colibri-sys (in-process host) → ServeClient mux → C engine process.

---

## What landed

### 1. Live HWINFO strip (`open:live-hwinfo-strip` → **closed**)

| Piece | Detail |
|-------|--------|
| Formatter | `format_live_hwinfo(&HwinfoSnap)` — plain labels: RAM free/total GB, cores, CPU name, GPU name + VRAM; omits empty names; no raw field dump |
| Idle copy | `LiveHwinfoIdle` + `live_hwinfo_idle_message` (start / stopped / waiting) |
| UI | Second top strip under memory-placement (cyan-tint text), updates on each `apply_visual_snapshot` / visual pump |
| Clear on stop | Pump exit sets EngineStopped idle for both tiers and HWINFO |

### 2. Brain / PROF MVP progress

| Item | Status |
|------|--------|
| Heat scaling `heat/24` | Already correct pre-Phase E (`brain_cell_rgb`); tests reaffirm saturation at 24 |
| Pulse multi-frame decay (`open:brain-pulse-decay` → **closed**) | Web RAF `*= 0.94` via `brain_pulse_after_decay` / `apply_brain_pulse_decay`; steps from `brain_pulse_decay_steps_for_ms(VISUAL_PUMP_MS)` (~31 steps at 500 ms) so feel tracks SPA, not one-shot on seq change |
| Full atlas (`open:brain-full-atlas`) | **Still open** — no hover tooltips / experts.json affinities / full-res grid; sample budget stays 2048 |
| PROF strip | Column header + aligned rows (wall, prompt, out, tok/s, disk, wait, matmul, attn); last-N wording; not SPA charts |

### 3. Docs / residual

- `.agents/RESIDUAL.md` — closed HWINFO + pulse decay; left full atlas + deep doctor + strategic items honest
- `crates/colibri-native/docs/fidelity.md` — HWINFO **done**; Brain notes decay + heat/24; PROF columns; atlas residual called out

---

## Files touched

| Path | Change |
|------|--------|
| `crates/colibri-native/src/host.rs` | `format_live_hwinfo`, idle enum, pulse decay helpers, PROF columns, unit tests |
| `crates/colibri-native/src/main.rs` | `live_hwinfo_text` state, strip UI, pump/snapshot wire, `apply_brain_pulse_decay` on brain rebuild |
| `crates/colibri-native/docs/fidelity.md` | Matrix + Brain limits |
| `.agents/RESIDUAL.md` | Closed/open honesty |

No colibri-sys product API changes (snapshot already absorbed HWINFO).

---

## APIs (host helpers)

```text
format_live_hwinfo(h: &HwinfoSnap) -> String
live_hwinfo_idle_message(LiveHwinfoIdle) -> &'static str

BRAIN_PULSE_RAF_FACTOR = 0.94
BRAIN_PULSE_FLOOR = 0.01
BRAIN_PULSE_RAF_MS = 16
brain_pulse_decay_steps_for_ms(elapsed_ms) -> u32
brain_pulse_after_decay(pulse, steps) -> f32
apply_brain_pulse_decay(view: &mut BrainView, prev: &BrainView, decay_steps)
```

`format_profile_turns` still takes `&[ProfileTurn], last_n`; output shape is labeled columns.

---

## Tests (new / updated)

- `format_live_hwinfo_plain_labels`
- `format_live_hwinfo_omits_empty_names`
- `live_hwinfo_idle_messages`
- `brain_pulse_decay_math_matches_web_raf`
- `brain_pulse_decay_steps_for_ms_maps_pump_cadence`
- `apply_brain_pulse_decay_carries_and_preserves_fresh_hits`
- `format_profile_*` updated for column labels (no `c=` / `mm=` wire style)

---

## Residual closed vs still open

| Id | Result |
|----|--------|
| `open:live-hwinfo-strip` | **Closed** |
| `open:brain-pulse-decay` | **Closed** |
| Heat `/24` fidelity | Confirmed (already done; kept tested) |
| `open:brain-full-atlas` | **Open** (M–L; SPA leads) |
| `open:deep-doctor-ui` | Open |
| `open:tauri-parity` / charts | Deferred |
| `open:visual-pump-idle-stop` | Open (process polish) |

---

## Verify commands + exit codes

| Command | Exit |
|---------|------|
| `cargo fmt -p colibri-sys -p colibri-native` | 0 |
| `cargo clippy -p colibri-sys --all-targets -- -D warnings` | 0 |
| `cargo clippy -p colibri-native --all-targets -- -D warnings` | 0 (after unused-mut fix in test) |
| `cargo test -p colibri-sys --lib` | 0 — **85 passed** |
| `cargo test -p colibri-native` | 0 — **46 passed** |

---

## MVP vertical complete?

**Yes** for Phase E observability MVP: live HWINFO strip + pulse decay + heat fidelity + usable PROF text. Full Brain atlas remains residual by design.
