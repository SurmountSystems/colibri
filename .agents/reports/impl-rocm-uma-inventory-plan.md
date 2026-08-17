# Implement: ROCm UMA inventory + unified DDR5 planner (Steps C+D)

**Date:** 2026-08-11
**Plan:** `.agents/plans/plan-rocm-unified-ddr5.md` steps C + D
**Scope:** inventory flags + placement budget only (not ffi-hip, not process HIP build)

## What landed

### C — Inventory (`GpuDevice` + heuristics + doctor)

**Rust** (`crates/colibri-sys/src/probe.rs`):

- `GpuDevice` gains:
  - `integrated: bool` (serde default false)
  - `gtt_total_bytes` / `gtt_free_bytes` (`Option<u64>`, from amdgpu sysfs)
- `total_bytes` / `free_bytes` remain the **VRAM carve-out** only.
- Classification:
  - Override: `COLI_GPU_MEMORY=unified|discrete` (also `uma` / `integrated` / `shared` / `dgpu` / `vram`)
  - Name patterns: Radeon `NNNM` / `NNNNS`, "… Graphics" without RX/Instinct
  - Soft: AMD + VRAM ≤ 8 GB + system RAM ≥ 16 GB (not discrete product names)
  - Supporting: substantial GTT relative to VRAM
- Helpers: `parse_gpu_memory_mode`, `gpu_memory_mode_override`, `infer_gpu_integrated`,
  `apply_gpu_memory_classification`, `name_looks_like_integrated_gpu`
- Sysfs path reads `mem_info_gtt_*`; rocm-smi path can enrich GTT from sysfs
- `discover_gpus` applies classification after inventory

**Doctor** (`doctor.rs` + Python `doctor.py`):

- Accelerator details include: `integrated`, `shared_system_memory`, carve-out
  used/total per device, system free/total RAM, optional GTT
- Linked + low free + UMA: carve-out busy note + "GPU shares system memory (UMA)"
- Linked + UMA + healthy free: "shared system memory (UMA), not discrete VRAM only"

**Python parity** (`c/resource_plan.py`, `c/doctor.py`): same fields and heuristics.

### D — Planner (unified budget)

**Rust** (`plan.rs`) + **Python** (`resource_plan.py`):

| Mode | Hot GPU budget |
|------|----------------|
| **Discrete** | `free_vram − 2 GiB` (unchanged goldens) |
| **Integrated / UMA** | Conservative share of free system RAM: 50% of (`available_memory − 4 GiB` OS headroom), maxed with discrete usable if carve-out has room |

Also:

- Subtract planned hot bytes from warm RAM cache (`expert_cache_bytes` / warm cap) so the same physical DDR pool is not double-counted (planner mirror of engine `#653`)
- Busy carve-out warning on UMA:
  `device VRAM carve-out is busy … using unified system memory budget X for GPU-resident experts`
- `environment_for_plan`: emits `COLI_CUDA=1` + non-zero `CUDA_EXPERT_GB` when UMA budget > 0 and CUDA path enabled

## Tests (red → green contracts)

| Contract | Test |
|----------|------|
| APU free VRAM ~0.2 GiB + free RAM 48 GiB + integrated → hot > 0, unified warning, env expert GB | `plan::tests::uma_apu_starved_carveout_nonzero_hot_from_system_ram` |
| Discrete free 24 GiB → usable free−2 GiB | `plan::tests::discrete_free_vram_minus_two_gib_preserved` |
| UMA warm reduced by hot vs discrete starved | `plan::tests::uma_warm_reduced_by_hot` |
| APU fixture → integrated; RX discrete unchanged; override wins | `probe::tests::apu_fixture_integrated_discrete_unchanged`, `coli_gpu_memory_override_wins`, 860M CSV classifies |
| Sysfs GTT filled | `probe::tests::sysfs_amd_fallback_from_fixture_tree` |
| Doctor UMA details | `doctor::tests::accelerator_uma_details_note_shared_memory` |

## Verify (ran green)

```text
cargo fmt -p colibri-sys
cargo clippy -p colibri-sys --all-targets -- -D warnings
cargo test -p colibri-sys --lib plan
cargo test -p colibri-sys --lib doctor
cargo test -p colibri-sys --lib probe
# python parity smoke for UMA + discrete usable
```

## Not in this slice

- `ffi-hip`, Makefile HIP static, process engine HIP build (later plan steps)
- Runtime `coli_cuda_device_integrated` confirm / smoke residency (Step E)
- Docs / residual close (Step F)

## Operator knobs

| Env | Effect |
|-----|--------|
| `COLI_GPU_MEMORY=unified` | Force UMA budget path |
| `COLI_GPU_MEMORY=discrete` | Force classic free VRAM − 2 GiB |
| Existing `vram_gb` / `CUDA_EXPERT_GB` / plan options | Still clamp requested hot size |
