# process-mop: colibri-sys

**Date:** 2026-08-09  
**Scope:** `crates/colibri-sys` only  
**Result:** all green; no code changes

## Commands and exit codes

| # | Command | Exit |
|---|---------|------|
| 1 | `cargo fmt -p colibri-sys` | **0** |
| 2 | `cargo clippy -p colibri-sys --all-targets --features install -- -D warnings` | **0** |
| 3 | `cargo test -p colibri-sys` | **0** |
| 4 | `cargo test -p colibri-sys --features install` | **0** |

## Test summary

### Default features (`cargo test -p colibri-sys`)
- lib: 26 passed
- `engine_real`: 0 passed, 1 ignored (`requires COLIBRI_TEST_ENGINE and COLIBRI_TEST_MODEL`)
- `plan_golden`: 2 passed
- `ssd_cache_vectors`: 1 passed
- doctests: 1 passed

### With `--features install`
- lib: 27 passed (includes `model::install::tests::local_install_registers`)
- integration/doctests same as default (1 ignored real-engine smoke)

## Fixes

None. fmt, clippy (`-D warnings`), and both test runs were already clean. No edits under `crates/colibri-sys` or root `Cargo.toml`.

## Git

No `git add` / `git commit` (process mop only).
