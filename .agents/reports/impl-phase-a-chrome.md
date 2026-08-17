# Implement report: Phase A product chrome scrub (`colibri-native`)

**Date:** 2026-08-10
**Repo:** `/home/hunter/Projects/surmount/colibri`
**Scope:** Phase A only (product chrome). No AMD detection / Phase B.

## Summary

Scrubbed lab chrome on the native GPUI shell so the default UI reads as a local
product window, not a host/engine debug dump. Engineering honesty remains in
README developer notes and `docs/fidelity.md`, not in the main chrome.

## Changes

### 1. Honesty strip removed from default chrome

- **File:** `crates/colibri-native/src/main.rs`
- Removed constant strip
  `Host: colibri-sys in-process · Engine: serve mux process · Frames: rkyv · Not REST · FFI: no`
- Optional **About** control in the title bar (default **off**). About text is
  product-minimal: *colibrì native desktop shell. Runs models on this machine
  without a browser.*

### 2. Empty states in plain English

| Surface | Before (lab) | After (product) |
|---------|--------------|-----------------|
| Chat bootstrap | System monologue about probe/doctor / rkyv | Empty transcript + centered “Start a conversation…” |
| Engine start | Chat system line about EngineDuplex / rkyv | Status line only: “Engine ready” |
| Missing model | System chat line with env jargon | Status: set a model folder, then start |
| Live placement idle | `live tiers: (start engine)` / `TIERS` | `Memory placement: start the engine…` |
| Profiling idle | `PROF: (start engine)` | Plain “Start the engine, then generate…” |
| Brain idle | `no EMAP yet` | “Start the engine and send a message…” |
| Plan empty | fine already | “Choose a model folder, then run Plan.” |

### 3. Machine: short summary + Details expand

- **File:** `crates/colibri-native/src/host.rs`
- `format_machine_summary`: memory free/total, CPU cores (+ model name), GPU name(s)
- `format_machine_details`: swap, generation, SIMD, model store, NPU, GPU free/total
- `format_machine(m, expanded)` combines them for the panel
- UI: **Details** / **Hide details** + **Refresh** (was Re-probe)

### 4. Doctor: checklist, not CLI dump

- `format_doctor_checklist`: `Overall: Pass|Warning|Fail`, model line, then
  `[pass|warn|fail|skip] <summary>` per check
- No more `status=error mode=standard model=…` header
- Button label: **Run checks**

### 5. Chat transcript keeps engineering notes out

- Initial `chat_log` is empty (no bootstrap system bubble)
- Engine start success/fail stays on the status line; no system monologue about
  frames / mux / rkyv
- Residual role label renamed from “system” to “note” if any note rows appear

### 6. Title

- Window titlebar: **`colibrì`**
- In-window brand: **`colibrì (native)`** (no agent slogans)
- Engine subtitle: `Engine not started` / `Ready · <model> · <family>`

### 7. Live memory-placement copy

- Live line: `Experts in memory · GPU N (…) · System RAM N (…) · Disk N`
- Idle helpers: `live_tiers_idle_message(LiveTiersIdle::{StartEngine,EngineStopped,Waiting})`

### 8. README tone

- **File:** `crates/colibri-native/README.md`
- Positions crate as local embed product shell
- Surfaces table matches new chrome
- Architecture honesty moved to a short developer section + fidelity doc link

## Tests added / updated

| Test | Contract |
|------|----------|
| `format_machine_summary_is_short` | Summary has Memory/CPU/GPU; no SIMD/store/NPU |
| `format_machine_details_includes_advanced` | Details has SIMD, store, NPU |
| `format_machine_expanded_combines_summary_and_details` | expanded true includes Details + SIMD |
| `format_doctor_checklist_is_not_cli_dump` | Overall Fail + marks; no `status=` / `mode=` |
| `live_tiers_idle_messages_are_plain` | No TIERS / mux jargon |
| `format_live_tiers_line` | GPU / System RAM / Disk wording |
| `format_profile_empty_and_nonempty` | Empty copy no longer says “no turns” only |

## Verify (ran)

```text
cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets -- -D warnings   # exit 0
cargo test -p colibri-native                                   # 27 passed
```

## Out of scope (by plan)

- Phase B AMD / ROCm detection
- Model registry UI, install cancel, multi-slot, deep doctor UI, Brain atlas
- SPA pixel parity / i18n

## Files touched

- `crates/colibri-native/src/host.rs`
- `crates/colibri-native/src/main.rs`
- `crates/colibri-native/README.md`
- `.agents/reports/impl-phase-a-chrome.md` (this report)
