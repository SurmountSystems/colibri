# Implement: ROCm HIP process engine product path (Step A)

**Date:** 2026-08-11
**Plan:** `.agents/plans/plan-rocm-unified-ddr5.md` Step A
**Scope:** process engines only (`HIP=1` build/locate/doctor). Did not rework UMA plan/probe. Left Cargo `ffi-hip` / `build.rs` ownership to the parallel ffi-hip agent (already co-landed in tree).

## What landed

### Linkage module (`crates/colibri-sys/src/linkage.rs`)

- `ProcessGpuLinkage` + pure parsers:
  - `parse_ldd_gpu_linkage` (unit-testable without live GPU)
  - `parse_bytes_gpu_markers` / `bytes_mention_hip_runtime` (ELF/string marker for `libamdhip64`)
  - `probe_process_gpu_linkage` (Unix `ldd`, bytes fallback; Windows DLL siblings)
- `hip_process_rebuild_next_step(basename)`: operational next step for AMD CPU-only process engines

Doctor `cuda_linkage` / `probe_cuda_linkage` now delegate to this module.

### Locate / process spawn

- `engine_override_from_env()`: `COLI_ENGINE` then `COLIBRI_ENGINE` (matches native host)
- `EngineHandle::start_*` and doctor resolution use that dual name
- Miss errors mention `COLIBRI_ENGINE` and `HIP=1`
- Docs in `locate.rs` for HIP process basenames (same names as CPU; no separate HIP filename)

### Doctor process-engine UX

- AMD + process engine **not** GPU-linked → warn CPU-only **and** details `hint` with:
  - `make -C c <engine> HIP=1`
  - `ROCM_HOME` / `HIP_ARCH`
  - `COLI_ENGINE` / `COLIBRI_ENGINE`
  - optional alternate Cargo `ffi-hip` (in tree; listed after process HIP)
- HIP-linked process engine → pass / UMA carve-out warn path; **not** CPU-only; no rebuild hint
- Doctor still resolves engine via override → env → `locate_engine` → basename

### Docs / Makefile

- `GPU_BACKENDS.md`: Linux process engines section (build, accept `ldd`, env, family notes)
- `docs/ENVIRONMENT.md`: process engine path vars + note that runtime knobs stay CUDA-shaped under HIP
- `c/Makefile`: process HIP build comment block above `HIP=1` (colibri/glm, inkling; deepseek_v4 CPU-only for GPU experts)

## How to build HIP process engines (operator host)

```sh
# GLM (native default process basename)
make -C c colibri HIP=1
# optional: ROCM_HOME=/opt/rocm HIP_ARCH=gfx1152 make -C c colibri HIP=1

ldd c/colibri | grep libamdhip64   # acceptance
export COLI_ENGINE=$PWD/c/colibri  # or COLIBRI_ENGINE
# doctor / native locate also pick c/colibri when cwd/repo layout matches
```

| Knob | Default / note |
|------|----------------|
| `ROCM_HOME` / `ROCM_PATH` | `/opt/rocm` |
| `HIP_ARCH` | `native` via `rocm_agent_enumerator`, or explicit `gfxNNNN` |
| Targets with HIP process GPU object | `colibri` / `glm`, `inkling` |
| No HIP expert backend today | `deepseek_v4` (CPU process), `kimi_k3` (Vulkan path) |

## Doctor behavior (process path)

| Situation | `accelerator.cuda` |
|-----------|--------------------|
| AMD + CPU-only process binary | **warn** CPU-only + details `hint` rebuild HIP=1 |
| AMD + `libamdhip64` linked | **pass** (or low free-VRAM / UMA warn); not CPU-only |
| HIP runtime missing on linked binary | **fail** missing libamdhip64 (+ hint) |

## Tests (green)

```text
cargo fmt -p colibri-sys
cargo clippy -p colibri-sys --all-targets -- -D warnings
cargo test -p colibri-sys --lib doctor
cargo test -p colibri-sys --lib linkage
cargo test -p colibri-sys --lib engine::locate
```

Contracts covered without live GPU:

- ldd text → hip/cuda/missing/cpu-only
- fixture blob with `libamdhip64` marker → probe hip
- doctor AMD CPU-only → hint HIP=1 primary before ffi-hip
- doctor HIP-linked → not CPU-only, no rebuild hint
- locate override + COLI/COLIBRI env dual name

## Residual for Step E host smoke

1. On operator ROCm host: `make -C c colibri HIP=1` and confirm `ldd` shows `libamdhip64`.
2. Doctor against that binary + AMD inventory: not CPU-only; UMA planner still from Steps C+D.
3. Process serve smoke with plan env (`COLI_CUDA=1`, expert GB) and model that fits.
4. gfx mismatch (1102 vs 1152): override `HIP_ARCH` if hipcc/`native` fails.
5. ffi-hip native path is separate Step B smoke (parallel agent).

## Not in this slice

- UMA inventory/plan (already landed)
- Owning further `ffi-hip` / static Makefile changes beyond process docs
- Live GPU inference claim
