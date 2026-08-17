# Implement: ROCm `ffi-hip` in-process embed (Step B)

**Date:** 2026-08-11
**Plan:** `.agents/plans/plan-rocm-unified-ddr5.md` Step B
**Scope:** Makefile static HIP + Cargo `ffi-hip` + build.rs + doctor honesty + native feature docs

## Goal (met)

Mirror closed **`ffi-cuda`** for AMD/ROCm so default native can run HIP compute when
built with an explicit feature (not permanently CPU-only on AMD).

| Bar | Status |
|-----|--------|
| `make libcolibri HIP=1` packs `backend_cuda.o` | **Yes** (same `CUDA_OBJ` path as process) |
| Cargo feature `ffi-hip` | **Yes** (implies `ffi`) |
| `build.rs` make HIP=1 + link `amdhip64` + cfg | **Yes** (`ffi_hip_linked`) |
| Mutual exclusion vs `ffi-cuda` | **Yes** (build panic if both forced) |
| Default `ffi` stays CPU-only | **Yes** |
| Doctor: not CPU-only solely because process binary missing | **Yes** (merge in-process HIP/CUDA linkage) |
| Native optional `ffi-hip` + docs matrix | **Yes** |
| CI without ROCm | **Yes** (CPU fallback + warning) |

## Feature / env / cfg matrix

| Name | Meaning |
|------|---------|
| Cargo `ffi-hip` | Opt-in HIP embed; implies `ffi` |
| Env `COLIBRI_FFI_HIP=1` | Same ask with `ffi` only |
| Env `COLIBRI_REQUIRE_FFI_HIP=1` | Hard-fail if ROCm missing (no CPU fallback) |
| rustc cfg `ffi_hip_linked` | GLM archive built with HIP + host linked `amdhip64` |
| `ffi::ffi_hip_feature_enabled()` | Feature requested |
| `ffi::ffi_hip_linked()` | Actually linked |
| `ffi::ffi_gpu_linked()` | CUDA or HIP actually linked |
| Cargo `ffi-cuda` / cfg `ffi_cuda_linked` | Unchanged NVIDIA path |

**Mutual exclusion:** `ffi-cuda` + `ffi-hip` (or both env flags) → `build.rs` panic:

```text
ffi-cuda and ffi-hip are mutually exclusive: one GPU vendor link mode per binary.
```

## How to build

### Static lib (make)

```bash
make -C c libcolibri HIP=1 LTO=0 ROCM_HOME=/opt/rocm [HIP_ARCH=native|gfxXXXX]
# optional if rocWMMA headers missing:
make -C c libcolibri HIP=1 LTO=0 COLI_HIP_NO_WMMA=1
```

- `ROCM_HOME` defaults to `ROCM_PATH` if set, else `/opt/rocm`
- `HIP_ARCH=native` uses `rocm_agent_enumerator`; override for cross / dry hosts

### Rust host

```bash
# colibri-sys
cargo build -p colibri-sys --features ffi-hip

# colibri-native (desktop)
cargo build -p colibri-native --features ffi-hip

# hard-require ROCm (no CPU fallback)
COLIBRI_REQUIRE_FFI_HIP=1 cargo build -p colibri-sys --features ffi-hip
```

Env knobs: `ROCM_HOME`, `ROCM_PATH`, `HIPCC`, `HIP_ARCH`, `COLI_HIP_NO_WMMA`.

### Honesty

| Build | GLM in-process GPU |
|-------|--------------------|
| `ffi` only | CPU kernels only |
| `ffi` + `ffi-hip` (ROCm present) | HIP + `libamdhip64` |
| `ffi` + `ffi-cuda` (CUDA present) | CUDA + `cudart` |
| process `HIP=1` engine | Process path |
| `ffi-cuda` + `ffi-hip` | **Build error** |

## Makefile (`c/Makefile`)

- Documented `libcolibri HIP=1` (same `backend_cuda.o` archive as CUDA when `CUDA_OBJ` set)
- `ROCM_HOME ?= $(or $(ROCM_PATH),/opt/rocm)`
- `COLI_HIP_NO_WMMA=1` forces `-DCOLI_HIP_NO_WMMA` (portable kernels when rocWMMA missing)
- `.build-config` stamp includes `COLI_HIP_NO_WMMA`

## `build.rs`

- Detect Linux + `hipcc` + `libamdhip64`
- `make libcolibri HIP=1 ROCM_HOME=... [HIP_ARCH=...] [HIPCC=...]`
- Link search + rpath on ROCm lib dir; `-lamdhip64 -lstdc++`
- Set `cargo:rustc-cfg=ffi_hip_linked`
- Missing toolkit → cargo warning + CPU `libcolibri` (unless `COLIBRI_REQUIRE_FFI_HIP=1`)
- Missing `rocwmma.hpp` → auto `COLI_HIP_NO_WMMA=1` + warning (this host)

## Doctor

- `merge_in_process_gpu_linkage(process, ffi_hip, ffi_cuda)`: if process not GPU-linked,
  in-process HIP/CUDA still counts as linked for `accelerator.cuda`
- `resolve_doctor_linkage` uses process `ldd` first, then merge when `feature=ffi`

## Native

- Feature `ffi-hip = ["colibri-sys/ffi-hip", "ffi"]` (not default)
- README: how to enable + `ldd … | grep amdhip64`

## Tests

| Test | Role |
|------|------|
| `c/tests/test_makefile_cuda_scope.py` HIP class | Dry-run: `libcolibri HIP=1` packs backend; CUDA+HIP exclusive |
| `ffi::cuda_gate_tests::*` | Default CPU-only; feature vs link; ignored HIP/CUDA smokes |
| `doctor::tests::merge_in_process_*` | Pure merge without live GPU |
| `doctor::tests::accelerator_amd_with_hip_linkage_not_cpu_only_warn` | Pass not CPU-only when HIP linked |

## Verification (this host)

Host: ROCm 7.x at `/opt/rocm`, `hipcc` present, **no** rocWMMA headers, arch **gfx1102**.

```text
python3 c/tests/test_makefile_cuda_scope.py -v   # 8 passed (CUDA + HIP dry-run)
cargo fmt -p colibri-sys
cargo clippy -p colibri-sys --all-targets --features ffi -- -D warnings
cargo clippy -p colibri-sys --all-targets --features ffi-hip -- -D warnings
cargo test -p colibri-sys --lib --features ffi cuda_gate doctor::tests::merge
cargo test -p colibri-sys --lib --features ffi-hip cuda_gate
cargo test -p colibri-sys --lib --features ffi-hip ffi_hip_linked_when_toolkit_present -- --ignored  # ok
cargo build -p colibri-sys --features ffi-cuda,ffi-hip  # panics mutual exclusion
ldd target/debug/deps/colibri_sys-… | grep amdhip64  # libamdhip64.so.7
```

## Operator: verify native binary on ROCm host

```bash
cargo build -p colibri-native --features ffi-hip
ldd target/debug/colibri-native | grep amdhip64
# expect: libamdhip64.so.* => /opt/rocm/lib/...

# optional full HIP with rocWMMA tensor cores:
# install rocwmma-dev (or distro equivalent), then rebuild without COLI_HIP_NO_WMMA
```

Doctor with AMD GPU + `ffi-hip` linked should **not** say “CPU-only (build with HIP=1)”
solely because the process engine is missing or CPU-built.

## Files touched

| Path | Change |
|------|--------|
| `c/Makefile` | libcolibri HIP docs; ROCM_PATH; COLI_HIP_NO_WMMA; stamp |
| `c/tests/test_makefile_cuda_scope.py` | HIP FFI dry-run + mutual exclusion |
| `crates/colibri-sys/Cargo.toml` | feature `ffi-hip` |
| `crates/colibri-sys/build.rs` | HIP resolve/make/link/cfg/exclusion/WMMA fallback |
| `crates/colibri-sys/src/ffi/mod.rs` | `ffi_hip_*`, `ffi_gpu_linked`, gate tests |
| `crates/colibri-sys/src/doctor.rs` | merge in-process GPU linkage + tests |
| `crates/colibri-native/Cargo.toml` | feature `ffi-hip` |
| `crates/colibri-native/README.md` | enable + ldd note |
| `crates/colibri-sys/README.md`, `docs/ffi-phase-d.md` | matrix |
| `GPU_BACKENDS.md` | FFI CUDA/HIP table |
| `crates/colibri-native/docs/fidelity.md` | GPU row honesty |

## Residual / follow-ups (not this slice)

- Install **rocwmma-dev** on operator host for full tensor-core HIP kernels (build already green with portable path)
- Full generate smoke with model + UMA plan (plan Step E)
- Process HIP path agent owns process locate/doctor next-step copy
- Docs residual close (plan Step F) may mark ffi-hip residual closed when campaign ranks it

## Product claim

**Default `feature=ffi` without `ffi-hip` remains CPU-only for GPU kernels.**
**`ffi-hip` is landed** for the link + doctor honesty bar; live inference smoke is Step E.
