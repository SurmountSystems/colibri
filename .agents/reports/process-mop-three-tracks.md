# Process mop — three tracks (FFI multi-family, SPA pixel, Brain atlas)

**Date:** 2026-08-10
**Scope:** `/home/hunter/Projects/surmount/colibri` — `colibri-sys` + `colibri-native` only
**Product edits:** none (no compile/fmt/clippy/test fallout)

## Commands and exit codes

| # | Command | Exit |
|---|---------|------|
| 1 | `cargo fmt -p colibri-sys -p colibri-native` | **0** |
| 2 | `cargo clippy -p colibri-sys --all-targets -- -D warnings` | **0** |
| 3 | `cargo clippy -p colibri-sys --all-targets --features ffi -- -D warnings` | **0** |
| 4 | `cargo clippy -p colibri-native --all-targets --features install -- -D warnings` | **0** |
| 5 | `cargo test -p colibri-sys --lib` | **0** (91 passed) |
| 6 | `cargo test -p colibri-sys --lib --features ffi` | **0** (99 passed) |
| 7 | `cargo test -p colibri-native` | **0** (74 passed) |

## Notes

- Clippy on `colibri-native` emitted an upstream future-incompat note for `proc-macro-error2 v2.0.1` only; not a warning-as-error failure, no local fix required for this mop.
- No merge-conflict or atlas/SPA compile fallout observed.
- No product source changes in this mop.

## Result

**Clean.** All seven commands exit 0. Ready for parent closeout.
