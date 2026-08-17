# Implement: open:ffi-gpu (one-platform Linux CUDA embed)

**Date:** 2026-08-10
**Residual:** `open:ffi-gpu` → **closed** (one-platform bar)
**Scope:** GLM embed only; default `feature=ffi` stays CPU-only

## Goal (met)

Honest opt-in path so embed can use GPU when built with an explicit feature /
env, without requiring CUDA for normal `ffi` builds or default CI.

| Bar | Status |
|-----|--------|
| Makefile flag packs CUDA into GLM static | **Yes** — `make libcolibri CUDA=1` archives `backend_cuda.o` |
| `build.rs` optional path | **Yes** — feature `ffi-cuda` / env `COLIBRI_FFI_CUDA=1` |
| Docs: CPU default, how to enable | **Yes** — `ffi-phase-d.md`, residual, fidelity, crate README |
| Smoke: no toolkit → CI green | **Yes** — CPU fallback + warning; ignored host-gated smoke |
| Product-default FFI / visual ABI unchanged | **Yes** |
| NPU still deferred | **Yes** |

## What shipped

### Makefile (`c/Makefile`)

- `libcolibri` archives `$(CUDA_OBJ)` when `CUDA=1` (Linux: `backend_cuda.o`).
- Default `CUDA=0` still produces CPU-only `libcolibri.a`.
- Comment block documents opt-in and that other families stay CPU.

### Cargo (`crates/colibri-sys`)

- Feature **`ffi-cuda`** = `["ffi"]` (opt-in; not default).
- `build.rs`:
  - Detects Linux + `nvcc` (`CUDA_HOME` / `PATH` / common roots).
  - On success: `make libcolibri CUDA=1`, link `cudart` + `stdc++` + rpath, set rustc cfg **`ffi_cuda_linked`**.
  - On missing toolkit: cargo **warning**, build **CPU-only** GLM (unless `COLIBRI_REQUIRE_FFI_CUDA=1` → panic).
  - Env alternate: `COLIBRI_FFI_CUDA=1` with `feature=ffi`.
- Public API (`ffi` module):
  - `ffi_cuda_feature_enabled()` — requested CUDA embed
  - `ffi_cuda_linked()` — actually linked CUDA + cudart

### Tests

| Test | Role |
|------|------|
| `c/tests/test_makefile_cuda_scope.py` | Dry-run: `libcolibri CUDA=1` packs backend; default CPU-only |
| `ffi::cuda_gate_tests::default_ffi_without_ffi_cuda_is_cpu_only` | Default `ffi` → not feature, not linked |
| `ffi::cuda_gate_tests::ffi_cuda_feature_reports_request_not_necessarily_link` | Feature vs link split |
| `ffi::cuda_gate_tests::ffi_cuda_linked_when_toolkit_present` | `#[ignore]` host-gated smoke |

### Docs / residual

- `crates/colibri-sys/docs/ffi-phase-d.md` — GPU section closed; how-to table
- `.agents/RESIDUAL.md` — CLOSED row; open list drops `open:ffi-gpu`
- `crates/colibri-native/docs/fidelity.md` — matrix honesty
- `crates/colibri-sys/README.md` — feature pointer

## Honesty (product claim)

| Claim | Reality |
|-------|---------|
| Default `feature=ffi` | **CPU-only** static libs (unchanged) |
| `feature=ffi-cuda` with toolkit | GLM archive includes CUDA objects; host links cudart |
| `feature=ffi-cuda` without toolkit | **CPU fallback**; `ffi_cuda_linked() == false` |
| Families | **GLM only** for CUDA embed; Kimi / Inkling / V4 stay CPU in FFI matrix |
| Platforms | **Linux CUDA only** this slice (not Windows DLL / Metal / HIP / Vulkan FFI) |
| NPU | Still deferred (`open:npu-inference`) |
| Full GPU generate golden | Not claimed; link matrix + gate tests + ignored smoke |

Process path (`make colibri CUDA=1`, Windows `cuda-dll`) is unchanged.

## Verification (this host)

```text
python3 c/tests/test_makefile_cuda_scope.py -v   # 6 passed
cargo test -p colibri-sys --features ffi --lib   # 107 passed, 1 ignored
cargo test -p colibri-sys --features ffi-cuda --lib cuda_gate
  # warning: nvcc not found → CPU fallback
  # 2 passed, 1 ignored
cargo fmt -p colibri-sys
cargo clippy -p colibri-sys --all-targets --features ffi -- -D warnings
cargo clippy -p colibri-sys --all-targets --features ffi-cuda -- -D warnings
```

This machine has **no CUDA toolkit**; full `ffi_cuda_linked` path was not
executed here. On a CUDA host:

```bash
cargo test -p colibri-sys --features ffi-cuda --lib
# expect ffi_cuda_linked() true; optional:
cargo test -p colibri-sys --features ffi-cuda --lib \
  ffi_cuda_linked_when_toolkit_present -- --ignored
# or hard-require toolkit:
COLIBRI_REQUIRE_FFI_CUDA=1 cargo build -p colibri-sys --features ffi-cuda
```

## Files touched

| Path | Change |
|------|--------|
| `c/Makefile` | `libcolibri` packs `CUDA_OBJ` when set |
| `c/tests/test_makefile_cuda_scope.py` | libcolibri CUDA/CPU dry-run asserts |
| `crates/colibri-sys/Cargo.toml` | feature `ffi-cuda` |
| `crates/colibri-sys/build.rs` | detect / make CUDA=1 / link / cfg / fallback |
| `crates/colibri-sys/src/ffi/mod.rs` | `ffi_cuda_*` + gate tests |
| `crates/colibri-sys/docs/ffi-phase-d.md` | GPU closed + how-to |
| `crates/colibri-sys/README.md` | feature note |
| `crates/colibri-native/docs/fidelity.md` | matrix row |
| `.agents/RESIDUAL.md` | close residual |
| `.agents/reports/impl-ffi-gpu-one-platform.md` | this report |

## Out of scope (intentional)

- Metal / Vulkan / HIP in FFI static matrix
- Multi-family GPU embed
- Dynamic `dlopen` Linux `.so` (static CUDA objects + cudart link, feature-gated)
- Full in-process CUDA generate golden vs process
- Product flip of library `prefer_process` (already closed separately as native-only)
- NPU inference
