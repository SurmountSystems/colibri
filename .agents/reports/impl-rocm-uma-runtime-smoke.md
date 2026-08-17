# Implement: ROCm UMA runtime alignment + smoke notes (Step E)

**Date:** 2026-08-11
**Plan:** `.agents/plans/plan-rocm-unified-ddr5.md` Step E
**Scope:** operator smoke checklist, plan vs runtime integrated honesty. No large C engine work.

## Summary

Product code for process HIP, `ffi-hip`, UMA inventory, and unified planner already
landed in earlier steps. This step documents how the operator verifies both HIP
paths on a ROCm APU host, and how `coli_cuda_device_integrated` relates to the
host plan. Live generate with a full model remains **operator-gated**.

## Integrated: plan vs runtime

| Layer | Source of truth | Role |
|-------|-----------------|------|
| **Host plan / doctor** | `GpuDevice.integrated` via heuristics + `COLI_GPU_MEMORY` + optional GTT | UMA hot budget from system free RAM; doctor shared-memory details |
| **Runtime (HIP/CUDA up)** | `coli_cuda_device_integrated(device)` in `c/backend_cuda.cu` | `cudaDeviceProp.integrated` (HIP maps the same surface) |
| **Runtime #653** | `c/colibri.c` after GPU expert place | If integrated, shrink `g_mem_avail_boot` by placed GPU expert bytes so warm RAM does not double-count the same DDR |

**Already wired (no new C work this step):**

- `coli_cuda_device_integrated` exported from CUDA/HIP backend and optional loader resolve.
- Engine applies #653 when the live device reports integrated.
- Planner mirrors the double-count policy before start (subtract hot from warm).

**Honesty if plan and runtime disagree:**

- Host may mark UMA (name / carve-out / override) while HIP reports `integrated=0`, or the reverse.
- Plan still emits non-zero `CUDA_EXPERT_GB` / `COLI_CUDA=1` from host inventory when UMA budget allows.
- Runtime #653 only runs when the **device property** says integrated.
- Product does **not** hard-fail on mismatch. One detail note is enough (doctor already notes UMA from inventory; engine may log the #653 line on stderr when it fires).
- Operator force: `COLI_GPU_MEMORY=unified` or `discrete` for plan path only.

No new host “confirm integrated from live HIP props” API was added. That would be
FFI/process probe surface work; residual only if a future product wants doctor to
cross-check HIP props after engine start.

## Operator smoke checklist (ROCm APU host)

Prerequisites: ROCm at `/opt/rocm` (or `ROCM_HOME` / `ROCM_PATH`), `hipcc`,
`libamdhip64`, AMD GPU visible. Model directory optional until generate steps.

### 1. Process HIP binary

```bash
cd /path/to/colibri
make -C c colibri HIP=1
# If arch is wrong (e.g. 860M-class): HIP_ARCH=gfx1152 make -C c colibri HIP=1
ldd c/colibri | grep libamdhip64
# expect: libamdhip64.so.* => /opt/rocm/lib/...
export COLI_ENGINE=$PWD/c/colibri   # or COLIBRI_ENGINE
```

**Pass:** `ldd` shows `libamdhip64`.
**Fail:** CPU-only binary (no amdhip64) → doctor will warn CPU-only + rebuild hint.

### 2. Native / colibri-sys with ffi-hip

```bash
cargo build -p colibri-sys --features ffi-hip
cargo build -p colibri-native --features ffi-hip
# rpath/link check (path may be target/debug/deps/libcolibri_sys-*.so or binary):
ldd target/debug/colibri-native | grep amdhip64
# expect: libamdhip64.so.*
```

Optional hard-require (no CPU fallback if ROCm missing):

```bash
COLIBRI_REQUIRE_FFI_HIP=1 cargo build -p colibri-sys --features ffi-hip
```

**Pass:** link shows amdhip64; `ffi_hip_linked` cfg when toolkit present.
**Default `ffi` without `ffi-hip`:** CPU kernels only (by design).

### 3. Doctor: not CPU-only

With AMD inventory and either process HIP binary **or** `ffi-hip` linked host:

```bash
# process path
COLI_ENGINE=$PWD/c/colibri cargo run -p colibri-sys --example plan_probe -- /path/to/model
# or native UI Doctor / Thorough check after build with ffi-hip
```

**Pass:** `accelerator.cuda` is not “CPU-only (build with HIP=1)” solely because a
process engine is missing when in-process HIP is linked; HIP-linked process
engine is pass or UMA carve-out warn, not CPU-only.
**Fail:** AMD + CPU-only process + no ffi-hip link → warn + next-step rebuild.

### 4. UMA plan: non-zero expert budget on APU

Unit contracts (no live GPU):

```bash
cargo test -p colibri-sys --lib plan::tests::uma_apu_starved_carveout_nonzero_hot_from_system_ram
cargo test -p colibri-sys --lib plan::tests::discrete_free_vram_minus_two_gib_preserved
cargo test -p colibri-sys --lib probe doctor
```

On live APU (busy carve-out, large free RAM):

```bash
# Force UMA if heuristics miss:
export COLI_GPU_MEMORY=unified
cargo run -p colibri-sys --example plan_probe -- /path/to/model
# Expect plan env / placement: COLI_CUDA=1 and non-zero CUDA_EXPERT_GB (or equivalent
# expert GB) when GPU path enabled; warm RAM reduced by hot; not “0 GB GPU only”.
```

**Pass:** hot tier budget > 0 from system RAM share when integrated.
**Override check:** `COLI_GPU_MEMORY=discrete` restores free-VRAM − 2 GiB path.

### 5. Optional generate (operator-gated)

Only when a suitable model is installed and HIP link is green:

```bash
# Process: force process if native prefers FFI
export COLIBRI_FORCE_PROCESS=1
export COLI_ENGINE=$PWD/c/colibri
export COLIBRI_MODEL=/path/to/model
# Plan-emitted env (or native Plan → Start) should set COLI_CUDA=1 + expert GB
cargo run -p colibri-native --features ffi-hip
# Or FFI path: unset COLIBRI_FORCE_PROCESS so native uses in-process HIP when linked
```

**Pass criteria (honest):**

- Engine starts without CPU-only GPU failure when HIP is linked.
- TIERS / Memory on GPU / stderr hot expert tier show non-zero GPU experts **or**
  documented ROCm/arch failure (wrong `HIP_ARCH`, missing rocWMMA portable path, OOM).
- If integrated and #653 fires: stderr may show RAM budget snapshot reduced by
  GPU expert tier size.

**Not claimed by CI:** full multi-GB model generate on every host.

### 6. Mutual exclusion / gfx notes

```bash
# Must fail build:
cargo build -p colibri-sys --features ffi-cuda,ffi-hip
```

If hipcc/`native` picks wrong gfx: set `HIP_ARCH=gfxNNNN` for process and FFI
builds. Portable kernels: `COLI_HIP_NO_WMMA=1` when rocWMMA headers are missing
(build.rs may set this automatically with a warning).

## Unit tests (this slice)

No new tests required. Existing green contracts cover UMA plan, discrete goldens,
probe integrated fixtures, doctor UMA details, linkage parsers, and ignored
`ffi_hip_linked` host smoke. Prefer those over live GPU probes in CI.

## Tiny code

None. Runtime integrated API and #653 already exist; plan/doctor inventory already
classify UMA. Step E is documentation + residual honesty only.

## Related reports

| Report | Step |
|--------|------|
| `impl-rocm-hip-process-path.md` | A process HIP |
| `impl-rocm-ffi-hip.md` | B ffi-hip |
| `impl-rocm-uma-inventory-plan.md` | C+D UMA inventory + plan |
| `impl-rocm-uma-docs-residual.md` | F docs + residual |
