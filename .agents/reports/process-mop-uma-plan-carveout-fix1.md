# Process mop: UMA Memory plan carve-out (fix round 1)

**Date:** 2026-08-12
**Role:** process mop (floor sweeper). fmt, clippy, targeted tests only. No product-scope expansion.
**Isolation:** none
**Packages:** `colibri-sys`, `colibri-native`, Python `c/resource_plan.py` + `c/tests/test_resource_plan.py`

Sources: `/tmp/grok-1000/grok-review-uma-plan-carveout.md` (Implementation Summary), `/tmp/grok-1000/grok-impl-summary-uma-plan-carveout.md`.

## Result

Clean. Every required command exited 0. No compile, lint, or test fallout. No product files were edited.

## Commands

| Command | Exit |
|---------|------|
| `cargo fmt -p colibri-sys -p colibri-native` | 0 |
| `cargo clippy -p colibri-sys --all-targets -- -D warnings` | 0 |
| `cargo clippy -p colibri-native --all-targets -- -D warnings` | 0 |
| `cargo test -p colibri-sys --lib uma_` | 0 (6 passed) |
| `cargo test -p colibri-sys --lib discrete_` | 0 (6 passed) |
| `cargo test -p colibri-sys --lib mixed_` | 0 (1 passed) |
| `cargo test -p colibri-sys --lib plan::` | 0 (14 passed) |
| `cargo test -p colibri-sys --test plan_golden` | 0 (2 passed) |
| `cargo test -p colibri-native memory_plan` | 0 (3 passed) |
| `cd /home/hunter/Projects/surmount/colibri/c && python3 -m unittest tests.test_resource_plan` | 0 (41 tests OK) |

## Notes (not failures)

- `cargo clippy -p colibri-native` and `cargo test -p colibri-native` printed the existing `proc-macro-error2 v2.0.1` future-incompat note. That is a dependency warning, not a clippy `-D warnings` failure. Exit was still 0.
- Native test compile rebuilt `colibri-sys` with a CPU-only `libcolibri` (no HIP/CUDA) because an existing HIP archive was present. Tests still passed.

## Test counts (this mop)

- `uma_`: 6 passed (plan + doctor UMA tests)
- `discrete_`: 6 passed (includes `mixed_amd_igpu_and_discrete_still_warns_vram_busy` via the `discrete` substring)
- `mixed_`: 1 passed (`mixed_amd_igpu_and_discrete_still_warns_vram_busy`)
- `plan::`: 14 passed
- `plan_golden`: 2 passed
- native `memory_plan`: 3 passed (`uma_memory_plan_ui_does_not_warn_carveout_busy`, `uma_memory_plan_notes_only_stays_ready`, `discrete_memory_plan_ui_still_warns_vram_busy`)
- Python `tests.test_resource_plan`: 41 tests OK in 5.355s

## Mop

None. Tree left as the implementer left it.

## Copy

Also written to `/tmp/grok-1000/grok-process-mop-uma-plan-carveout-fix1.md`.
