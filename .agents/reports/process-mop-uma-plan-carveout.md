# Process mop: UMA Memory plan carve-out notes

**Role:** `[process-mop]` after implementer `uma-plan-carveout`
**Tree:** `/home/hunter/Projects/surmount/colibri`
**Date:** 2026-08-12
**Scope:** fmt → clippy → targeted tests on packages the implementer touched. No product redesign.

Touched surface from implementer summary:

- `colibri-sys` (`crates/colibri-sys/src/plan.rs`, `config.rs`)
- `colibri-native` (`crates/colibri-native/src/host.rs`)
- Python `c/resource_plan.py` + `c/tests/test_resource_plan.py`

## Summary

All commanded checks **passed**. **No code fixes** were required. There was no fmt drift, no clippy `-D warnings` failure, and no test failure.

Implementer-claimed green was confirmed on a fresh mop pass.

---

## Commands and exit codes

| # | Command | Exit |
|---|---------|------|
| 1 | `cargo fmt -p colibri-sys -p colibri-native` | **0** |
| 2 | `cargo clippy -p colibri-sys --all-targets -- -D warnings` | **0** |
| 3 | `cargo clippy -p colibri-native --all-targets -- -D warnings` | **0** |
| 4 | `cargo test -p colibri-sys --lib uma_` | **0** (6 passed, 157 filtered) |
| 5 | `cargo test -p colibri-sys --lib discrete_` | **0** (5 passed, 158 filtered) |
| 6 | `cargo test -p colibri-sys --lib plan::` | **0** (13 passed, 150 filtered) |
| 7 | `cargo test -p colibri-sys --lib doctor::` | **0** (36 passed, 127 filtered) |
| 8 | `cargo test -p colibri-sys --lib config::` | **0** (6 passed, 157 filtered) |
| 9 | `cargo test -p colibri-sys --test plan_golden` | **0** (2 passed) |
| 10 | `cargo test -p colibri-native memory_plan_ui` | **0** (2 passed, 285 filtered) |
| 11 | `cd /home/hunter/Projects/surmount/colibri/c && python3 -m unittest tests.test_resource_plan` | **0** (40 tests OK, 6.230s) |

### Clippy notes (non-failing)

- **colibri-native:** Cargo future-incompat note for `proc-macro-error2 v2.0.1` (dependency; not a clippy `-D warnings` failure).

### Native test note (non-failing)

- `cargo test -p colibri-native memory_plan_ui` printed the existing build-script warning: `existing .../c/libcolibri.a is a HIP archive; rebuilding CPU-only libcolibri (no HIP=1/CUDA=1) for feature=ffi`. Tests still passed.

### Tests that specifically cover the named contract

- `plan::tests::uma_memory_plan_ui_does_not_warn_carveout_busy`
- `plan::tests::uma_busy_carveout_does_not_warn_as_discrete_vram`
- `plan::tests::uma_apu_starved_carveout_nonzero_hot_from_system_ram`
- `plan::tests::discrete_busy_vram_still_warns`
- `doctor::tests::uma_busy_carveout_does_not_drive_accelerator_or_plan_warn`
- `host::tests::uma_memory_plan_ui_does_not_warn_carveout_busy`
- `host::tests::discrete_memory_plan_ui_still_warns_vram_busy`

---

## What was fixed

**Nothing.** The tree was already consistent after the implementer. This mop only observed green.

---

## Out of scope (per job)

- No product edits
- No git add / commit / stage
- No residual close
