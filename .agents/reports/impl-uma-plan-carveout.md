# Implement: UMA Memory plan must not Warning the BIOS VRAM carve-out

**Date:** 2026-08-12
**Crates:** `colibri-sys`, `colibri-native` (Python `c/resource_plan.py` kept in parity)

## Named contract

On an integrated AMD / UMA placement plan:

1. Memory plan must not emit a Warning (or a line the native UI prefixes as `Warning:`) about device VRAM carve-out being busy.
2. The plan may still use the unified system-memory budget and may show informational notes that are not warnings.
3. On a discrete GPU, a real VRAM-busy warning must stay.
4. Operational English only. No invented marketing copy.

## What was actually leftover

The prior doctor wave already omitted the scare string from `plan.warnings` when `any_uma` was true. Native Memory plan still prefixes **every** `plan.warnings` line as `Warning:`. That wave also **deleted** the unified-budget sentence instead of demoting it to information. After a large-model UMA plan the UI therefore had no honest unified-budget line, and any leftover warning still became `Warning:`.

The screenshot string (`device VRAM carve-out is busy (only 0.4 GB of 4.3 GB free); using unified system memory budget …`) was gone from source. `target/release/colibri-native` (mtime 2026-08-12 14:07) still contained that format string. A rebuild of native after this change is what the operator will run.

## Files

| File | Change |
|------|--------|
| `crates/colibri-sys/src/plan.rs` | Added `PlacementPlan.notes`. On UMA + busy BIOS carve-out, push an informational note (`using unified system memory budget X for GPU-resident experts`). Discrete still warns `VRAM is already in use`. New test `uma_memory_plan_ui_does_not_warn_carveout_busy`. Starved-APU golden now expects the unified-budget sentence in **notes**, not warnings. |
| `crates/colibri-sys/src/config.rs` | `stub_plan` initializes `notes`. |
| `crates/colibri-native/src/host.rs` | `format_plan_readiness` prints `notes` as plain lines (no `Warning:` prefix). User-visible tests: `uma_memory_plan_ui_does_not_warn_carveout_busy`, `discrete_memory_plan_ui_still_warns_vram_busy`. Probe-shaped fixture starts `integrated: false` (rocm-smi default). |
| `c/resource_plan.py` | Same notes vs warnings split. `format_plan` prints notes without a `warn` prefix. |
| `c/tests/test_resource_plan.py` | `test_uma_busy_carveout_is_note_not_warning`, `test_discrete_busy_vram_still_warns`. |

Doctor still keys `placement.plan` off `warnings` only. Notes do not drive Overall warning.

Copy for the note is the second half of the original operational sentence (unified budget for GPU-resident experts). The scare prefix `device VRAM carve-out is busy (only X of Y free)` is not used.

## TDD

### RED (before product edit)

```text
cargo test -p colibri-sys --lib uma_memory_plan_ui_does_not_warn_carveout_busy -- --nocapture
```

Failed:

- `plan::tests::uma_memory_plan_ui_does_not_warn_carveout_busy`
  - `UMA plan should mention the unified system memory budget as information: "Warning: cold expert misses may reach disk; normal decode speed depends on hit rate\n" warnings=["cold expert misses may reach disk; normal decode speed depends on hit rate"]`

Carve-out-busy was already absent from warnings. The user-visible leftover was: no informational unified-budget line, and the only Memory plan line was a `Warning:` (cold misses, allowed).

### GREEN (after product edit; same test body / same named asserts)

```text
cargo test -p colibri-sys --lib uma_memory_plan_ui_does_not_warn_carveout_busy
```

Passed. Same filter. Product fix: `notes` + UMA informational line; test still forbids `Warning:` carve-out-busy and `Warning:` discrete VRAM-busy on UMA.

After `notes` existed, the test's UI simulator also prints `notes` without a `Warning:` prefix (same as native). Named asserts were not rewritten.

`uma_apu_starved_carveout_nonzero_hot_from_system_ram` now expects the unified-budget sentence in `notes`, not `warnings`. Named contract: that sentence must not be a Warning. Stronger check: notes have it; warnings must not contain `carve-out is busy` or `using unified system memory budget`.

## Post-impl verify

| Command | Exit |
|---------|------|
| `cargo fmt -p colibri-sys -p colibri-native` | 0 |
| `cargo clippy -p colibri-sys --all-targets -- -D warnings` | 0 |
| `cargo clippy -p colibri-native --all-targets -- -D warnings` | 0 |
| `cargo test -p colibri-sys --lib uma_` | 0 (6 passed) |
| `cargo test -p colibri-sys --lib discrete_` | 0 (5 passed) |
| `cargo test -p colibri-sys --lib plan::` | 0 (13 passed) |
| `cargo test -p colibri-sys --lib doctor::` | 0 (36 passed) |
| `cargo test -p colibri-sys --lib config::` | 0 (6 passed) |
| `cargo test -p colibri-sys --test plan_golden` | 0 (2 passed) |
| `cargo test -p colibri-native memory_plan_ui` | 0 (2 passed) |
| `cd c && python3 -m unittest tests.test_resource_plan` | 0 (40 tests OK) |

## Residual

This leftover was not listed as open in `.agents/RESIDUAL.md`. No residual close invented.

## Review round

All five open review items landed.

1. Per-device 75% busy check (Rust + Python). Mixed 860M + RX 7900 XTX still warns `VRAM is already in use` for the discrete card and may note the unified budget for the APU.
   - RED: `cargo test -p colibri-sys --lib mixed_amd_igpu_and_discrete_still_warns_vram_busy` failed (`any_uma` note-only path).
   - GREEN: same filter after the split.
2. UI tests require a note that starts with `using unified system memory budget`. They no longer ban every Warning line that contains that substring (UMA clamp).
3. Native `uma_memory_plan_notes_only_stays_ready`: 50 GB experts, `Memory plan: ready to run`, no review-warnings header.
4. Discrete native UI asserts the UMA unified-budget sentence is absent.
5. Python UMA/discrete/mixed tests pop `COLI_GPU_MEMORY` in try/finally. Discrete `format_plan` must print `warn   ` plus `VRAM is already in use`.

### Review-round verify

| Command | Exit |
|---------|------|
| `cargo fmt -p colibri-sys -p colibri-native` | 0 |
| `cargo clippy -p colibri-sys --all-targets -- -D warnings` | 0 |
| `cargo clippy -p colibri-native --all-targets -- -D warnings` | 0 |
| `cargo test -p colibri-sys --lib uma_` | 0 (6 passed) |
| `cargo test -p colibri-sys --lib discrete_` | 0 (6 passed, includes mixed) |
| `cargo test -p colibri-sys --lib plan::` | 0 (14 passed) |
| `cargo test -p colibri-native memory_plan` | 0 (3 passed) |
| `cd c && python3 -m unittest tests.test_resource_plan` | 0 (41 tests OK) |
