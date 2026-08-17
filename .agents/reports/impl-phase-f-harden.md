# Phase F: harden + docs + residual closeout (implement report)

**Date:** 2026-08-10
**Scope:** colibri-native deep doctor UI, production docs, residual honesty, light harden note.
**Architecture (unchanged):** GPUI → colibri-sys (in-process host) → ServeClient mux → C engine process.

---

## Campaign summary: production MVP vs deferred

| Journey | Status |
|---------|--------|
| **Local-embed production MVP (Phases A–F)** | **Complete** for the one-window path: product chrome, AMD/ROCm probe + doctor, registry + install cancel + min free, inference controls (temperature / max tokens / reasoning / grammar / multi-slot), live tiers + HWINFO + PROF + Brain pulse, deep doctor UI, production-facing docs |
| **NPU inference** | **Deferred** (`open:npu-inference`). Inventory/probe may list NPUs; no NPU decode path |
| **True Phase D FFI** | **Deferred** (`open:ffi-phase-d`). Process mux remains the product path; `ffi_available() == false` |
| **Full SPA / Tauri parity** | **Deferred** (`open:tauri-parity`). Not a full SPA clone; fidelity demo + embed MVP only |
| **Full Brain atlas** | **Still open** (`open:brain-full-atlas`). MVP heat/pulse/sample only |

Do **not** claim full SPA clone or NPU inference.

---

## What landed

### 1. Deep doctor UI (`open:deep-doctor-ui` → **closed**)

| Piece | Detail |
|-------|--------|
| Host API | `run_doctor_checks(model, machine, deep)`, `run_deep_doctor` (thin wrapper); `run_shallow_doctor` kept |
| Checklist | Plain **Depth: quick** / **Depth: thorough (tensor headers and shards)** (no raw `mode=` dump) |
| UI | Doctor panel: **Run checks** (quick) + **Deep check** (thorough); status lines "Checks finished" / "Thorough checks finished" |
| Sys | Unchanged; already had `DoctorOptions.deep` + deep container checks |

### 2. Docs

| Doc | Change |
|-----|--------|
| `crates/colibri-native/README.md` | Production capability table: doctor deep, registry, inference controls, live HW, AMD/ROCm, NPU inventory-only honesty, install cancel + min free, `COLIBRI_KV_SLOTS` |
| `crates/colibri-native/docs/fidelity.md` | Doctor row: Deep check exposed |
| `crates/colibri-sys/docs/user-guide.md` | AMD/ROCm GPU fields + detection; NPU inventory-only; doctor HIP note; install cancel + min free; grammar/slot on generate + ClientFrame Submit examples |
| Root `README.md` | Already points at `colibri-native`; no edit |

### 3. Residual honesty (`.agents/RESIDUAL.md`)

- Closed: deep doctor UI, production docs for MVP.
- Open kept: full Brain atlas, NPU inference (labeled deferred), FFI Phase D, Tauri parity, OpenAI REST, visual-pump Join polish.
- Added **Production MVP status** section (complete vs deferred).

### 4. Harden polish (`open:visual-pump-idle-stop`)

**Left open.** Pump already stops when the engine session is cleared (`visual_pump_running = false` + loop break). Explicit Join/cancel-on-drop needs storing a GPUI task handle and a small redesign; not done as a drive-by.

---

## Files touched

| Path | Change |
|------|--------|
| `crates/colibri-native/src/host.rs` | `run_doctor_checks` / `run_deep_doctor`, depth labels, tests |
| `crates/colibri-native/src/main.rs` | **Deep check** button + `run_deep_doctor` handler |
| `crates/colibri-native/README.md` | Production-facing rewrite of capability table |
| `crates/colibri-native/docs/fidelity.md` | Doctor deep row |
| `crates/colibri-sys/docs/user-guide.md` | AMD/ROCm, install cancel, grammar/slot |
| `.agents/RESIDUAL.md` | Phase F closeout + MVP status |

No colibri-sys product code changes this phase (API already sufficient).

---

## Verify

| Step | Result |
|------|--------|
| `cargo fmt -p colibri-native -p colibri-sys` | ok |
| `cargo clippy -p colibri-sys --all-targets -- -D warnings` | ok |
| `cargo clippy -p colibri-native --all-targets --features install -- -D warnings` | ok |
| `cargo test -p colibri-sys --lib` | **85 passed** |
| `cargo test -p colibri-native` | **47 passed** (includes `format_doctor_checklist_deep_depth_label`) |

---

## Residual closed vs still open (this phase)

| Id | Status |
|----|--------|
| `open:deep-doctor-ui` | **Closed** |
| Production docs A–F | **Closed** |
| `open:visual-pump-idle-stop` | **Open** (pump exits on engine clear; no Join handle) |
| `open:brain-full-atlas` | Open |
| `open:npu-inference` | Open (deferred) |
| `open:ffi-phase-d` | Open (strategic) |
| `open:tauri-parity` | Open (strategic) |
| `open:openai-rest` | Open (intentionally absent) |
