# Implement: ROCm UMA docs + residual honesty (Step F)

**Date:** 2026-08-11
**Plan:** `.agents/plans/plan-rocm-unified-ddr5.md` Step F
**Scope:** product docs + `.agents/RESIDUAL.md` open body; no marketing.

## Residual before → after

### Before (stale claims)

| Claim in residual / docs | Problem |
|--------------------------|---------|
| GPU embed closed CUDA-only; “Not claimed: … HIP FFI static” | False after Step B: `ffi-hip` landed |
| Architecture diagram: `feature=ffi` CPU only, no HIP branch | Incomplete |
| No ROCm UMA plan closeout in residual MVP body | Plan C–D landed but residual silent |
| ENVIRONMENT.md: no `COLI_GPU_MEMORY` / FFI HIP build knobs | Host planner knobs undocumented |
| GPU_BACKENDS: process/FFI HIP present; no UMA plan section | UMA budget path not operator-visible |
| user-guide residual: “GPU/NPU out” under FFI | GPU CUDA/HIP opt-in already in tree |

### After (honest)

| Item | Status |
|------|--------|
| Process HIP | Landed (build/locate/doctor); host generate smoke operator-gated |
| `ffi-hip` | Landed (feature matrix, build.rs, doctor merge linkage) |
| UMA inventory + plan | Landed (integrated/GTT, unified hot budget, #653 planner mirror) |
| Default `ffi` without `ffi-hip` | **CPU-only** for GPU kernels |
| Live ROCm generate / full TIERS | Operator-gated (model + local ROCm) |
| NPU inference | Still deferred (`open:npu-inference`) |
| Vulkan primary accelerator | Not this campaign; optional later honesty |
| Metal / multi-family GPU FFI | Not claimed |

Disk pin: `.agents/RESIDUAL.md` MVP + architecture sections updated same turn.

## Product doc paths touched

| Path | Change |
|------|--------|
| `GPU_BACKENDS.md` | New **Unified / UMA memory plan** section; `COLI_GPU_MEMORY`; #653 / integrated note; smoke report pointer |
| `docs/ENVIRONMENT.md` | Host planner/inventory section: `COLI_GPU_MEMORY`, `COLIBRI_FFI_HIP`, require flags, CUDA twins |
| `crates/colibri-native/README.md` | UMA/APU note; feature table `ffi-hip` / `ffi-cuda`; CPU-only default clarity |
| `crates/colibri-sys/README.md` | CPU-only default wording; GPU_BACKENDS pointer for UMA/process HIP |
| `crates/colibri-sys/docs/user-guide.md` | Features `ffi-cuda`/`ffi-hip`; doctor AMD/UMA paragraph; residual table honesty |
| `crates/colibri-sys/docs/ffi-phase-d.md` | ffi-hip landed + operator smoke pointer; “not claimed” list |
| `.agents/RESIDUAL.md` | ROCm HIP + UMA closed for product bar; smoke operator-gated; arch diagram |

## Clear matrix (docs + residual agree)

| Build | GPU on AMD |
|-------|------------|
| `ffi` only | CPU kernels only |
| `ffi` + `ffi-hip` | HIP in-process when ROCm present |
| process `HIP=1` engine | HIP process |
| `ffi-cuda` | NVIDIA path; not for this APU |

## Smoke report

Operator checklist (process + FFI + doctor + UMA plan + optional generate):

`.agents/reports/impl-rocm-uma-runtime-smoke.md`

## Verification (docs-only slice)

No product Rust/C edits in Step F. Prior steps already ran fmt/clippy/plan/doctor/probe
tests green. This slice: residual disk pin + doc fidelity only.

## Non-goals (unchanged)

- Rename CUDA-shaped wire env under HIP
- NPU inference
- Vulkan as primary accelerator
- Prebuilt HIP binaries for every gfx in CI
- Guaranteeing full large MoE hot residency on ~89 Gi alone
