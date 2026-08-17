# Implement: intended cold-expert overflow is a note, not a scare warning

**Date:** 2026-08-13

## Named contract

When the placement plan **intends** that cold MoE experts live on disk / SSD
cache (model much larger than the unified or RAM budget), that is not a
misconfiguration. Doctor health check and Memory plan must **not** present
that as a scare `Warning:` / `[warn]` that makes Overall look unhealthy.

It is an informational **note** (`PlacementPlan.notes` / native
`format_plan_readiness` prints notes plain). Real **warnings** stay when
something is actually wrong (RAM cannot hold one expert slot, requested GPU
missing, discrete VRAM busy, `storage.disk` under 1 GB).

This matches the operator screenshot: GLM-5.2-class ~429 GB model vs ~39.4 GB
unified budget, "Likely limit: disk I/O". That is intended overflow, not a
placement bug. Not parked.

HIP CPU-only doctor line was not touched.

## Files

| File | Change |
|------|--------|
| `crates/colibri-sys/src/plan.rs` | When `cold_bytes > 0`, push the hit-rate sentence to `notes`, not `warnings`. New test: `intended_cold_overflow_is_note_not_warning`. |
| `crates/colibri-sys/src/doctor.rs` | Product path unchanged (`placement.plan` already warns only on `plan.warnings`). New test: `intended_cold_overflow_does_not_drive_placement_plan_warn`. |
| `crates/colibri-native/src/host.rs` | Product path unchanged (`format_plan_readiness` already prints notes plain and prefixes `Warning:` only for `warnings`). New test: `intended_cold_overflow_memory_plan_is_note_not_warning`. |
| `c/resource_plan.py` | Same notes vs warnings split (parity). |
| `c/tests/test_resource_plan.py` | New test: `test_intended_cold_overflow_is_note_not_warning`. |

`c/doctor.py` unchanged: it already joins `plan["warnings"]` for
`placement.plan`. After the plan change, the health check is pass when this
sentence is the only former scare.

## TDD

### RED (before product edit)

```text
cargo test -p colibri-sys --lib intended_cold_overflow -- --nocapture
```

Failed (2 tests):

- `plan::tests::intended_cold_overflow_is_note_not_warning`
  - `intended overflow must be a note: notes=["using unified system memory budget 20.5 GB for GPU-resident experts"] warnings=["cold expert misses may reach disk; normal decode speed depends on hit rate"]`
- `doctor::tests::intended_cold_overflow_does_not_drive_placement_plan_warn`
  - same: sentence in `warnings`, not `notes`

```text
cargo test -p colibri-native --bin colibri-native intended_cold_overflow -- --nocapture
```

Failed (1 test):

- `host::tests::intended_cold_overflow_memory_plan_is_note_not_warning`
  - `Memory plan must not scare-prefix intended overflow: Memory plan: review warnings before start` plus `Warning: cold expert misses may reach disk; normal decode speed depends on hit rate` and `Likely limit: disk I/O`

```text
cd c && python3 -m unittest tests.test_resource_plan.ResourcePlanTest.test_intended_cold_overflow_is_note_not_warning -v
```

Failed: sentence not in `notes` (only the unified-budget note).

That is the operator copy on both screenshot surfaces.

### GREEN (same commands after product edit)

```text
cargo test -p colibri-sys --lib intended_cold_overflow
```

Passed (2 tests). Exit 0.

```text
cargo test -p colibri-native --bin colibri-native intended_cold_overflow
```

Passed (1 test). Exit 0.

```text
cd c && python3 -m unittest tests.test_resource_plan.ResourcePlanTest.test_intended_cold_overflow_is_note_not_warning -v
```

Passed. Exit 0.

## Post-impl verify

| Step | Command | Exit |
|------|---------|------|
| fmt | `cargo fmt -p colibri-sys` | 0 |
| fmt | `cargo fmt -p colibri-native` | 0 |
| clippy | `cargo clippy -p colibri-sys --all-targets -- -D warnings` | 0 |
| clippy | `cargo clippy -p colibri-native --all-targets -- -D warnings` | 0 |
| tests | `cargo test -p colibri-sys --lib intended_cold_overflow` | 0 (2) |
| tests | `cargo test -p colibri-sys --lib uma_` | 0 (6) |
| tests | `cargo test -p colibri-sys --lib discrete_` | 0 (6) |
| tests | `cargo test -p colibri-sys --lib plan::` | 0 (15) |
| tests | `cargo test -p colibri-sys --lib doctor::` | 0 (37) |
| tests | `cargo test -p colibri-sys --test plan_golden` | 0 (2) |
| tests | `cargo test -p colibri-native --bin colibri-native intended_cold_overflow` | 0 (1) |
| tests | `cargo test -p colibri-native --bin colibri-native uma_memory_plan` | 0 (2) |
| tests | `cargo test -p colibri-native --bin colibri-native discrete_memory_plan` | 0 (1) |
| tests | Python `ResourcePlanTest` overflow + UMA/discrete/mixed/disk-hit | 0 (5) |

Did not run `just check`.

## What the operator will see

- Health check: no `[warn] cold expert misses may reach disk; ...`.
  `placement.plan` stays pass (`tier placement has no warnings`) when this
  is the only former scare.
- Memory plan: same sentence as a **plain note**, not `Warning:`. Header
  stays `Memory plan: ready to run` when there are no real warnings.
- `Likely limit: disk I/O` stays (informational bottleneck). That is
  expected for a 429 GB model on a 39 GB unified budget.
- Doctor Overall no longer goes to warning **solely** because of this note.
  Other real warns (including HIP CPU-only, which this work did not touch)
  can still set Overall to warning.

## Real warnings left in place

- `RAM budget cannot hold one expert slot per sparse layer`
- `one or more requested GPUs were not detected`
- VRAM / unified-budget clamp warnings
- Discrete `VRAM is already in use ...`
- Doctor `storage.disk` when free space is under 1 GB
