# Implement: rust-nextest HIP archive / just check linker fail

**Date:** 2026-08-12
**Observed red:** `just rust-nextest` / `just check` linker error
`Undefined symbol __hipUnregisterFatBinary` from `c/libcolibri.a` (`backend_cuda.o`)
when compiling `colibri-sys` lib tests under `--features install,ffi`.

## Contracts

1. **`just rust-nextest` / `just check`:** GPU feature selection matches
   `rust-clippy` on the same machine. `hipcc` on PATH → nextest uses `ffi-hip`.
   Else `nvcc` on PATH → `ffi-cuda`. Else `install,ffi` only. Same warn-and-skip
   as clippy (missing toolkit does not fail the gate). Never both vendors.
2. **`build.rs`:** A CPU-only `feature=ffi` build (no `ffi-cuda` / `ffi-hip`)
   must not link an archive that still contains HIP or CUDA objects. Detect
   leftover `c/libcolibri.a` (or `COLIBRI_GLM_STATIC_LIB`) via unique fatbin
   symbols (`__hipUnregisterFatBinary` / `__cudaUnregisterFatBinary` and
   register siblings). In-tree leftover: delete and `make libcolibri HIP=0
   CUDA=0`. Prebuilt override that is still GPU-flavored: hard rustc error.
   Do not paper over by linking `amdhip64` / `cudart` on CPU-only ffi.
3. Do **not** loosen nextest to skip `colibri-sys` tests.

## What changed

### `justfile`

- Extracted private `_gpu-ffi-compilers` (same hipcc / nvcc / hardware probe
  and warn-and-skip rules as clippy). stdout is `HIP=0|1` and `CUDA=0|1`.
- `rust-clippy` evals that probe instead of duplicating the bash.
- `rust-nextest` now runs
  `cargo nextest run --workspace --all-targets --features <set>`
  where `<set>` is `install,ffi,ffi-hip` or `install,ffi,ffi-cuda` or
  `install,ffi`. HIP wins if both compilers exist. Never both vendor features.

### `crates/colibri-sys/src/archive_gpu_flavor.rs` (new)

Unit-testable helper included from `build.rs` (`#[path]`) and from crate tests
(`#[cfg(test)]` module). Parser is a byte scan of unique fatbin names, so tests
use canned `nm` text / empty files, not binary `.a` fixtures.

| Test | Contract |
|------|----------|
| `missing_file_is_none` | missing path → none |
| `empty_bytes_are_none` / `empty_file_is_none` | empty → none |
| `cpu_archive_text_is_none` | CPU `nm` listing → none |
| `hip_unregister_from_nm_lines` | `__hipUnregisterFatBinary` → HIP |
| `cuda_unregister_from_nm_lines` | `__cudaUnregisterFatBinary` → CUDA |
| `hip_wins_when_both_vendor_markers_present` | HIP CUDA-compat blobs stay HIP |

### `crates/colibri-sys/build.rs`

- CPU-only GLM path calls `ensure_cpu_only_libcolibri`: if the existing
  archive is HIP/CUDA, warn, delete it, remake with explicit `HIP=0 CUDA=0`,
  then refuse if uniques remain.
- `COLIBRI_GLM_STATIC_LIB` on CPU-only ffi: refuse GPU-flavored override
  with a clear panic (operator must rebuild CPU-only or enable the matching
  feature).

### `c/Makefile` (`libcolibri`)

`ar rcs` updates members but does **not** drop leftover `backend_cuda.o`.
The archive recipe now `$(RM) $@` then `$(AR) rcs` so a CPU remake only packs
the listed objects.

## Commands + exit codes

Host: `hipcc` present, `nvcc` absent. `just _gpu-ffi-compilers` → `HIP=1` `CUDA=0`.

| Command | Exit | Notes |
|---------|------|--------|
| `cargo fmt -p colibri-sys` | 0 | after Rust edits |
| `cargo test -p colibri-sys --lib archive_gpu_flavor` | 0 | 8 passed |
| `cargo test -p colibri-sys --lib --features install,ffi-hip --no-run` | 0 | dirty HIP archive |
| `ar t c/libcolibri.a` after dirty | 0 | `colibri.lib.o` + `backend_cuda.o`; `nm` shows `U __hipUnregisterFatBinary` |
| `cargo test -p colibri-sys --lib --features install,ffi --no-run` | 0 | warning: existing archive is HIP; rebuilt CPU-only |
| `ar t c/libcolibri.a` after remake | 0 | `colibri.lib.o` only; no hip/cuda fatbin uniques |
| `cargo test -p colibri-sys --lib --features install,ffi` | 0 | 207 passed, 3 ignored; no `__hipUnregisterFatBinary` |
| `cargo clippy -p colibri-sys --all-targets --features install,ffi -- -D warnings` | 0 | |
| `python3 c/tests/test_makefile_cuda_scope.py -v` | 0 | 8 passed (Makefile dry-run still CPU-only / HIP packs backend) |
| `just rust-nextest` | 0 | `--features install,ffi,ffi-hip`; 495 passed, 4 skipped |

Dirty-HIP then CPU-only ffi compile is the hygiene contract. Nextest on this
host uses `ffi-hip` so it links `amdhip64` and does not hit the leftover-archive
path. The explicit `--features install,ffi` compile after a HIP-built
`c/libcolibri.a` is what proves CPU-only ffi rebuilds instead of failing the
linker.

## Not done / not loosened

- Did not skip `colibri-sys` tests in nextest.
- Did not link `amdhip64` on CPU-only `ffi`.
- Did not enable `ffi-cuda` and `ffi-hip` together.
