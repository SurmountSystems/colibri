# Process mop: colibri-sys (follow-ups)

Date: 2026-08-10
Scope: `colibri-sys` only (post-followups land). No product edits. No git.

## Commands and exit codes

| Step | Command | Exit |
|------|---------|------|
| 1. fmt | `cargo fmt -p colibri-sys` | **0** |
| 2. clippy | `cargo clippy -p colibri-sys --all-targets --features install -- -D warnings` | **0** |
| 3. test (default) | `cargo test -p colibri-sys` | **0** |
| 4. test (install) | `cargo test -p colibri-sys --features install` | **0** |

## Results

### fmt
- Exit 0. No formatting changes needed.

### clippy
- Exit 0. Already finished clean; zero warnings under `-D warnings` with `--features install`.

### cargo test -p colibri-sys
- Lib: **41 passed**, 0 failed
- `engine_real`: 0 passed, **1 ignored** (`COLIBRI_TEST_ENGINE` / `COLIBRI_TEST_MODEL`)
- `plan_golden`: **2 passed**
- `ssd_cache_vectors`: **1 passed**
- Doc-tests: **1 passed**

### cargo test -p colibri-sys --features install
- Lib: **48 passed**, 0 failed, **1 ignored** (`model::install::tests::live_hf_snapshot_tiny` — live network HF hub)
- `engine_real`: 0 passed, **1 ignored**
- `plan_golden`: **2 passed**
- `ssd_cache_vectors`: **1 passed**
- Doc-tests: **1 passed**

## Fixes

None. Tree was already clean under `crates/colibri-sys`.

## Verdict

**Green.** fmt, clippy (-D warnings, install features), default tests, and install-feature tests all exit 0.
