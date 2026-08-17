# Implement: UMA VRAM carve-out must not warn as discrete VRAM

**Date:** 2026-08-12
**Crate:** `colibri-sys` (Python `c/resource_plan.py` + `c/doctor.py` kept in parity)

## Named contract

On **integrated / UMA** AMD (probe already has `integrated` / GTT / unified budget):

- The GPU memory budget is **unified system RAM** (existing reserve / no-double-count rules). It is **not** the BIOS/carve-out VRAM number (operator: 4.3 GB).
- Doctor and Memory plan must **not** emit a **warn** (or fail) that the **carve-out is busy / only X of Y GB free** as if that were discrete VRAM headroom.
- Do **not** tell the user they are constrained to ~4 GB VRAM when the plan already uses tens of GB of unified RAM.
- Carve-out stats may stay as **detail** (`vram_carve_out`, `low_free_vram`). They must **not** drive Overall warning by themselves on UMA.
- Discrete (non-integrated) GPUs: keep the existing VRAM-busy / VRAM-too-small warnings.

**Out of scope (untouched):** `AMD GPU detected but the engine is CPU-only (build with HIP=1)`. If Overall stays warning solely because of that line, that is OK.

## Files

| File | Change |
|------|--------|
| `crates/colibri-sys/src/plan.rs` | On UMA, omit the "device VRAM carve-out is busy …" warning. Discrete still warns when free &lt; 75% of VRAM. New tests: `uma_busy_carveout_does_not_warn_as_discrete_vram`, `discrete_busy_vram_still_warns`. |
| `crates/colibri-sys/src/doctor.rs` | HIP-linked + UMA + low carve-out: **pass** with the existing UMA info note (`shared system memory (UMA), not discrete VRAM only`). Discrete + low free still **warn**. New test: `uma_busy_carveout_does_not_drive_accelerator_or_plan_warn`. Updated `accelerator_uma_details_note_shared_memory` (pass, not warn) and `accelerator_amd_hip_pass_and_low_vram_warn` (near-zero case is discrete RX). |
| `c/resource_plan.py` | Same UMA omit / discrete keep. |
| `c/doctor.py` | Same UMA pass / discrete low-free warn. |

CPU-only accelerator wording was not changed.

## TDD

### Red (before product edit)

```text
cargo test -p colibri-sys --lib uma_busy_carveout -- --nocapture
```

Failed (2 tests):

- `plan::tests::uma_busy_carveout_does_not_warn_as_discrete_vram`
  - `UMA must not warn that the BIOS carve-out is busy as if it were discrete VRAM: ["device VRAM carve-out is busy (only 0.4 GB of 4.3 GB free); using unified system memory budget 34.0 GB for GPU-resident experts", "cold expert misses may reach disk; normal decode speed depends on hit rate"]`
- `doctor::tests::uma_busy_carveout_does_not_drive_accelerator_or_plan_warn`
  - `placement.plan must not warn that the BIOS carve-out is busy: … device VRAM carve-out is busy (only 0.4 GB of 4.3 GB free); using unified system memory budget 0.0 GB for GPU-resident experts`

That is the operator string (0.4 of 4.3 GB, unified tens of GB).

### Green (after product edit)

```text
cargo test -p colibri-sys --lib uma_busy_carveout
```

Passed (2 tests).

Nearby goldens also passed:

```text
cargo test -p colibri-sys --lib uma_
cargo test -p colibri-sys --lib discrete_
cargo test -p colibri-sys --lib accelerator_
cargo test -p colibri-sys --lib coli_gpu_memory
cargo test -p colibri-sys --lib plan::
cargo test -p colibri-sys --lib doctor::
cargo test -p colibri-sys --test plan_golden
cargo fmt -p colibri-sys
cargo clippy -p colibri-sys --all-targets -- -D warnings
```

All green. Clippy `-D warnings` clean.

## Overall can still warn from the CPU-only HIP line

Yes. `accelerator_check` still returns **warn** when an AMD device is present and the engine is not GPU-linked:

`AMD GPU detected but the engine is CPU-only (build with HIP=1); GPU shares system memory (UMA)`

That is enough for doctor Overall **warning**. The carve-out-busy line is no longer a warn on UMA, so it no longer appears on Memory plan or `placement.plan`.

Other legitimate plan warns can still appear (cold expert misses; unified budget clamped by model size). Discrete busy-VRAM wording is unchanged.
