# Plan: ROCm HIP (process + FFI) + unified DDR5 (APU) memory plan

## Context

Operator host (verified probe):

- ROCm **7.2.4** at `/opt/rocm` (`rocminfo`, `hipcc`, `rocm-smi`)
- APU: Ryzen AI 7 PRO 350 + **Radeon 860M** (Krackan iGPU; shared system memory)
- ~**89 Gi** system RAM
- Doctor today: **AMD GPU detected but the engine is CPU-only (build with HIP=1)**; **Memory on GPU: 0.0 GB**; carved VRAM ~4.3 GB almost full; RAM expert-slot warnings on large models

Recon (authoritative):

- `.agents/reports/recon-rocm-unified-ddr5.md`
- `.agents/reports/recon-host-rocm-presence.md`
- Prior: Phase B AMD detection (`impl-phase-b-amd-rocm`); `open:ffi-gpu` closed **CUDA-only** (no `ffi-hip` yet)

## Operator revise (this plan)

**`ffi-hip` is not a non-goal.** In-process HIP embed is an **important product goal**, parallel to process `HIP=1`, not deferred residual theater. Default native already prefers FFI for CPU; AMD GPU must also be reachable via HIP-linked static engines when the operator builds with ROCm.

## Goals

1. **Use installed ROCm for real inference** on this APU host class (and discrete AMD when present).
2. **HIP process engines:** build/locate `make … HIP=1` binaries so doctor is not stuck on CPU-only for the process path.
3. **`ffi-hip` in-process path (required):** mirror `ffi-cuda` for AMD: Cargo feature + `build.rs` + Makefile static libs with `HIP=1`, link `libamdhip64`, so **default native FFI** can run HIP compute when built with the feature (not permanently CPU-only on AMD).
4. **Plan and doctor treat UMA/APU memory honestly:** shared DDR5 is the working pool; the small carved VRAM window is not the whole story.
5. **No double-count** of the same physical pool as both “VRAM hot” and “RAM warm.”
6. **Discrete AMD dGPU path stays correct** (free VRAM − reserve) once HIP-built.
7. Native UI/doctor copy stays plain operational English (no invented marketing).

## Non-goals (this plan)

- Renaming wire env (`COLI_CUDA`, `CUDA_EXPERT_GB`) away from CUDA-shaped names (documented quirk; keep for engine compatibility).
- NPU inference (still deferred).
- Vulkan as primary accelerator (optional later honesty; do not block HIP path).
- Guaranteeing DeepSeek-V4-Flash full hot residency on ~89 Gi alone (model may still need aggressive tiering / disk; plan must not *lie* that only 0.2 Gi VRAM exists).
- Shipping prebuilt HIP binaries in CI artifacts for every host gfx (operator builds against local ROCm).

## Recommended path (one)

**Both product runners get HIP, plus UMA-aware placement.**

| Layer | Product meaning |
|-------|-----------------|
| Host ROCm | Already present on operator machine |
| Process HIP | `make -C c <engine> HIP=1`; native can locate / force process when needed |
| **FFI HIP** | `feature = "ffi-hip"` (or equivalent) builds static engines with HIP and links `amdhip64`; native can enable for AMD product default GPU |
| Placement | UMA: budget hot experts from **shared system free RAM** (minus headroom + double-count fix); discrete: free VRAM − 2 GiB |

Not: process-only forever. Not: treat 4 GiB carve-out as discrete HBM. Not: claim default CPU `ffi` is GPU-capable without `ffi-hip`.

**Build order preference:** land UMA inventory + planner early (works for both runners), process HIP path and **ffi-hip** in parallel once Makefile HIP static targets exist, then native feature wiring + doctor honesty, then smoke.

## Approach

### Step A — HIP process engine product path (P0)

**Build / locate**

- Document and exercise host build for family binaries used by native (at least GLM/`colibri` and DeepSeek `deepseek_v4`):
  - `make -C c <target> HIP=1` with `ROCM_HOME`/`ROCM_PATH` (default `/opt/rocm`)
  - `HIP_ARCH=native` or explicit gfx (860M: rocminfo/smi may report gfx1102 vs gfx1152; override must be documented)
- Ensure `locate_engine` / `COLIBRI_ENGINE` / doctor engine resolution can pick the HIP binary.
- Acceptance: `ldd <engine>` shows `libamdhip64`; doctor does **not** say CPU-only for that path.

**Native / doctor UX (operational)**

- When AMD + ROCm present + resolved process engine is CPU-only: keep HIP=1 warning + one-line next step (rebuild process with HIP=1 **or** rebuild native with `ffi-hip` when that feature is available).
- When process engine is HIP-linked: Phase B linkage path (pass or carve-out warn, not CPU-only).

### Step B — `ffi-hip` in-process embed (P0, required)

Mirror the closed `ffi-cuda` vertical for AMD/ROCm.

**Makefile / C**

- Ensure static lib targets used by Rust FFI (`libcolibri.a`, and multi-family archives as today: inkling, kimi, deepseek_v4 as applicable) can build with **`HIP=1`** (same `backend_cuda.cu` + hipcc path as process).
- Rpath / link flags for `libamdhip64` consistent with process HIP.
- Document `ROCM_HOME`, `HIP_ARCH` for static builds.

**Rust `colibri-sys`**

- Cargo feature e.g. **`ffi-hip`** (name final in impl; must be explicit and documented):
  - Depends on `ffi` (or co-enables multi-family static link).
  - `build.rs`: when `ffi-hip` / `COLIBRI_FFI_HIP=1`, invoke make with `HIP=1`, link `amdhip64`, set cfg like existing `ffi_cuda_linked` pattern (e.g. `ffi_hip_linked`).
  - Mutual exclusion / clarity vs `ffi-cuda` (one GPU vendor link mode per binary; document; fail or prefer clear error if both forced).
- Doctor / host `in_process` + accelerator: when `ffi_hip_linked`, AMD path is not “CPU-only” solely because process binary is missing.
- Tests: feature flag / build.rs unit tests without live GPU where possible; CI may skip link if no ROCm (cfg + docs); operator host verifies `ldd` on the native binary shows `libamdhip64`.

**Native**

- Optional or documented default for AMD hosts: e.g. enable `ffi-hip` in addition to `ffi` when building on ROCm machines (document; do not break CUDA-only machines).
- Prefer in-process HIP when linked; process HIP remains fallback.

**Honesty**

- Default `feature=ffi` **without** `ffi-hip` remains **CPU-only** for GPU kernels. Docs and residual must say so until operator builds with `ffi-hip`.
- Closing residual: mark HIP FFI as landed for the product bar when link + UMA plan + doctor pass; do not claim multi-vendor simultaneous CUDA+HIP in one binary.

### Step C — Inventory: mark unified / UMA (P1, supports plan)

**Files:** `crates/colibri-sys/src/probe.rs`, plan types, Python `resource_plan.py` parity, doctor details.

- Extend `GpuDevice` (serde defaults): `integrated: bool` (or `memory_kind: discrete | unified | unknown`), optional `gtt_total_bytes` / `gtt_free_bytes` from amdgpu sysfs.
- Keep `total_bytes` / `free_bytes` as **device VRAM carve-out**.
- Heuristics (override wins always):
  1. Env/config: e.g. `COLI_GPU_MEMORY=unified|discrete`.
  2. Soft: small VRAM (≤ 8 GiB) + large system RAM (≥ 16 GiB) + AMD iGPU name patterns.
  3. Supporting: substantial GTT relative to VRAM on sysfs.
- Doctor details on UMA: carve-out used/total **and** system free/total; note shared system memory when integrated.
- Tests: APU fixture → integrated; discrete fixture unchanged.

### Step D — Planner: unified DDR5 budget (P0 core)

**Files:** `crates/colibri-sys/src/plan.rs`, `c/resource_plan.py`, env emission for serve **and** FFI start paths.

When **unified/integrated**:

1. Do **not** set hot GPU expert budget to `max(0, free_vram − 2 GiB)` alone.
2. Shared-pool budget from **system free RAM** (conservative default; clamp by model + overrides).
3. Subtract planned hot bytes from warm RAM budget (mirror `#653` in the planner).
4. Warnings: carve-out busy → “using unified system memory budget X GiB…” not “no GPU memory.”
5. Emit `COLI_CUDA=1` and non-zero expert GB when budget allows and GPU path is enabled (process or FFI HIP).

When **discrete**: keep free VRAM − 2 GiB (goldens preserved).

**Golden tests (red → green)**

| Fixture | Expect |
|---------|--------|
| APU: free VRAM ~0.2 GiB, free RAM ~48 GiB, integrated | Non-zero hot tier; warm reduced by hot |
| Discrete: free VRAM 24 GiB | Classic free−2GiB |
| Override discrete on APU-shaped hardware | Classic VRAM path |
| Explicit unified + large free RAM | Shared-pool path |

### Step E — Runtime alignment + smoke (P1)

- Confirm `coli_cuda_device_integrated` when HIP is up; plan vs runtime mismatch → detail once, not hard fail.
- Smoke (operator host): **process HIP** and **ffi-hip native** each with plan env + model that fits → TIERS / Memory on GPU non-zero **or** documented ROCm/arch failure.
- Planner UMA and `#653` do not both over-subtract.

### Step F — Docs + residual honesty

- GPU_BACKENDS / ENVIRONMENT / native README: process HIP, **ffi-hip** feature matrix, UMA plan, overrides, ROCM_HOME / HIP_ARCH.
- Residual: Vulkan doctor optional; NPU deferred; **ffi-hip is in-scope here** (not “later”).
- Clear matrix:

| Build | GPU on AMD |
|-------|------------|
| `ffi` only | CPU kernels only |
| `ffi` + `ffi-hip` | HIP in-process |
| process `HIP=1` engine | HIP process |
| `ffi-cuda` | NVIDIA path (unchanged); not for this APU |

## Critical files

| Path | Role |
|------|------|
| `c/Makefile` | `HIP=1` process + static libs for FFI |
| `c/backend_cuda.cu` / `backend_gpu_compat.h` | HIP device props, mem_info, integrated |
| `c/colibri.c` | Expert placement, `#653` |
| `c/resource_plan.py` | Plan + env parity |
| `crates/colibri-sys/build.rs` | **`ffi-hip`**: make HIP=1, link amdhip64, cfg |
| `crates/colibri-sys/Cargo.toml` | `ffi-hip` feature |
| `crates/colibri-sys/src/probe.rs` | UMA/GTT inventory |
| `crates/colibri-sys/src/plan.rs` | Unified vs discrete budget |
| `crates/colibri-sys/src/doctor.rs` | Accelerator + UMA + HIP-linked FFI |
| `crates/colibri-sys/src/engine/locate.rs` | Process engine path |
| `crates/colibri-sys/src/ffi/` | Multi-family FFI + GPU link awareness |
| `crates/colibri-native/Cargo.toml` | Feature wiring for `ffi-hip` |
| `crates/colibri-native/src/host.rs` | Doctor/plan UI, prefer FFI HIP when linked |
| `GPU_BACKENDS.md`, `docs/ENVIRONMENT.md` | Build contracts |

## Reuse

- Phase B AMD detection and doctor linkage messages
- **`ffi-cuda` / `impl-ffi-gpu-one-platform`** as the template for `ffi-hip` (build.rs, feature, smoke)
- Existing `HIP=1` C backend (no new GPU stack)
- NVIDIA unified / Metal as plan policy inspiration only
- `#653` integrated double-count as runtime precedent

## Steps (implement order)

| Id | Work | Size |
|----|------|------|
| `impl:rocm-uma-inventory` | GpuDevice integrated/GTT, heuristics, override, doctor details, tests | 2 |
| `impl:rocm-uma-plan` | Unified budget + no double-count + env emission; discrete goldens | 2 |
| `impl:rocm-hip-process-path` | Process HIP build docs + locate/doctor next-step; linkage acceptance | 2 |
| `impl:rocm-ffi-hip` | Makefile static HIP + Cargo `ffi-hip` + build.rs link + doctor/native awareness | 2 |
| `impl:rocm-uma-runtime-smoke` | Runtime integrated confirm; process + FFI HIP smoke notes/tests | 2 |
| `impl:rocm-uma-docs-residual` | Docs + residual close for HIP process and ffi-hip | 1 |

## Risks

| Risk | Mitigation |
|------|------------|
| gfx arch mismatch (1102 vs 1152) | `HIP_ARCH` override; fail loud on hipcc errors |
| ffi-cuda vs ffi-hip both requested | Document exclusive GPU link; clear build error |
| Static HIP archives fail in CI without ROCm | Feature optional; CI CPU `ffi`; operator/local ROCm for green HIP |
| Over-aggressive UMA hot tier OOMs | Conservative defaults; overrides |
| Double-count RAM | Planner subtract hot from warm; tests |
| Larger link / load time for HIP FFI | Acceptable; document |

## Verification

```text
cargo fmt -p colibri-sys -p colibri-native
cargo clippy -p colibri-sys -p colibri-native --all-targets -- -D warnings
cargo test -p colibri-sys --lib plan
cargo test -p colibri-sys --lib doctor
cargo test -p colibri-sys --lib probe
# UMA goldens + feature docs tests
# On host with ROCm (manual / local):
# cargo build -p colibri-native --features ffi,ffi-hip
# ldd target/.../colibri-native | rg amdhip64
```

**Red → green contracts (named):**

1. APU fixture free VRAM 0.2 GiB + free RAM 48 GiB + integrated → plan hot budget **&gt; 0** and warm RAM reduced by hot.
2. Discrete free VRAM 24 GiB → free−2GiB preserved.
3. Doctor: CPU process engine + AMD → warns HIP=1; HIP-linked process **or** `ffi_hip_linked` injection → not CPU-only solely for missing process binary.
4. Integrated flag true on APU-shaped inventory without requiring HIP link.
5. `ffi-hip` feature / build.rs contract: when feature on and ROCm available, HIP link path selected (or skip with documented cfg when ROCm absent in CI).

**Operator host (manual after green):**

```text
# Process
make -C c deepseek_v4 HIP=1 ROCM_HOME=/opt/rocm
# FFI native
cargo run -p colibri-native --features ffi,ffi-hip
# Doctor: no CPU-only when HIP linked; plan unified budget; start with GPU env
```

## Open questions (non-blocking; plan defaults)

- **UMA hot fraction:** conservative shared-pool split; tunable via existing vram/env overrides.
- **Native default features on AMD:** document `ffi` + `ffi-hip` for ROCm product builds; do not force `ffi-hip` on every default build if that breaks non-ROCm CI (prefer explicit feature + docs, or detect at build time only when `COLIBRI_FFI_HIP=1`).
- **Vulkan:** later; not required for this vertical.

## Assumptions

- Operator wants **full product GPU on AMD**: process HIP **and** in-process **ffi-hip**, plus UMA plan truth.
- Large MoE models may still warn on RAM slots; UMA fix addresses **false “no GPU memory”**, not free lunch for 160+ GB footprints.
- Wire names stay CUDA-shaped for engine compatibility.

### Critical Files for Implementation
- `crates/colibri-sys/build.rs` — ffi-hip make/link (mirror ffi-cuda)
- `crates/colibri-sys/Cargo.toml` — ffi-hip feature
- `crates/colibri-sys/src/plan.rs` — UMA vs discrete budget
- `crates/colibri-sys/src/probe.rs` — integrated/GTT inventory
- `crates/colibri-sys/src/doctor.rs` — accelerator + UMA + HIP-linked FFI
- `c/Makefile` — HIP=1 process and static libs
- `c/resource_plan.py` — plan parity
- `crates/colibri-native/Cargo.toml` — feature wiring
- `crates/colibri-native/src/host.rs` — doctor/plan UI, FFI HIP prefer
- `GPU_BACKENDS.md` — operator build contract
