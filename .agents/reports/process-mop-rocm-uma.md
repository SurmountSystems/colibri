# Process mop: ROCm / UMA / HIP / install-pause

**Role:** `[process-mop]` L2 after parallel implementers
**Tree:** `/home/hunter/Projects/surmount/colibri`
**Date:** 2026-08-11
**Scope:** fmt → clippy → relevant tests on `colibri-sys` and `colibri-native` only. No product redesign.

## Summary

All commanded checks **passed**. **No code fixes** were required (no fmt drift, clippy `-D warnings` failures, or test failures).

Note: `cargo test` accepts a single `TESTNAME` filter. Multi-name filters were run as separate invocations.

---

## Commands and exit codes

| # | Command | Exit |
|---|---------|------|
| 1 | `cargo fmt -p colibri-sys -p colibri-native` | **0** |
| 2 | `cargo clippy -p colibri-sys --all-targets -- -D warnings` | **0** |
| 3 | `cargo clippy -p colibri-sys --all-targets --features ffi -- -D warnings` | **0** |
| 4 | `cargo clippy -p colibri-sys --all-targets --features ffi-hip -- -D warnings` | **0** |
| 5 | `cargo clippy -p colibri-native --all-targets --features install -- -D warnings` | **0** |
| 6 | `cargo clippy -p colibri-native --all-targets -- -D warnings` | **0** |
| 7 | `cargo test -p colibri-sys --lib plan` | **0** (9 passed) |
| 8 | `cargo test -p colibri-sys --lib doctor` | **0** (35 passed) |
| 9 | `cargo test -p colibri-sys --lib probe` | **0** (20 passed) |
| 10 | `cargo test -p colibri-sys --lib linkage` | **0** (9 passed) |
| 11 | `cargo test -p colibri-sys --lib engine::locate` | **0** (5 passed) |
| 12 | `cargo test -p colibri-sys --lib --features ffi` | **0** (166 passed, 2 ignored) |
| 13 | `cargo test -p colibri-native --features install` | **0** (285 passed) |
| 14 | `python3 c/tests/test_makefile_cuda_scope.py` | **0** (8 tests OK) |

### Clippy notes (non-failing)

- **ffi-hip:** build script warning only: `rocWMMA headers not found under /opt/rocm; building portable HIP kernels (COLI_HIP_NO_WMMA=1)`. Clippy finished clean under `-D warnings`.
- **colibri-native:** Cargo future-incompat note for `proc-macro-error2 v2.0.1` (dependency; not a clippy `-D warnings` failure).

### ffi test ignores (expected)

- `ffi::cuda_gate_tests::ffi_cuda_linked_when_toolkit_present` — needs `ffi-cuda` + CUDA toolkit / `COLIBRI_REQUIRE_FFI_CUDA=1`
- `ffi::cuda_gate_tests::ffi_hip_linked_when_toolkit_present` — needs `ffi-hip` + ROCm/hipcc / `COLIBRI_REQUIRE_FFI_HIP=1`

Relevant doctor merge / cuda_gate / hip tests that do run under `--features ffi` all passed (including `merge_in_process_hip_*`, `ffi_hip_feature_reports_request_not_necessarily_link`, UMA plan tests under `plan`).

---

## What was fixed

**Nothing.** Tree was already consistent after implementers; mop observed green only.

---

## Out of scope (per job)

- No UMA/HIP redesign
- No git commit / stage
- No product residual edits
