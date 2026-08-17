# impl: colibri-sys rich probe public API complete

**Repo:** `/home/hunter/Projects/surmount/colibri`
**Crate:** `crates/colibri-sys`
**Date:** 2026-08-10

## Summary

The rich machine inventory was **already present** as public types and fields on
`MachineInfo` (re-exported from `lib.rs`). This pass closed the operator gap:
config → probe wiring was caller-owned and easy to miss; docs listed inventory
only as comments; the example truncated SIMD/libs. Work here makes the public
API **complete, one-liner wired, documented as a field table, and fully printed**.

## Already public (no type gaps)

| Requirement | Status before this pass |
|-------------|-------------------------|
| Total system memory | `MachineInfo.total_memory` |
| Swap total/free | `swap_total`, `swap_free` |
| Free storage on model store volume | `model_store.free_bytes` (+ `total_bytes`) |
| Model store path default + `Option` override | `ProbeOptions.model_store`, `ColibriConfig.model_store`, `ModelStoreSource` |
| Logical vs physical cores | `physical_cores`, `logical_cores`, `cpu.threads_per_core` |
| big.LITTLE when detectable | `cpu.big_little: Option<BigLittleInfo>` |
| Architecture + generation + CPU identity | `cpu.architecture`, `generation_hint`, vendor/model/family/model/stepping |
| SIMD and NPU with specific ISA/kinds | `cpu.simd` (AVX512*, NEON, …), `npus` (`xdna`, …) |
| Relevant host libraries | `host_libraries` |
| Re-exports | `lib.rs` already re-exported all nested inventory types |

## Fixed / added

| Change | Why |
|--------|-----|
| `ProbeOptions::from_config(&ColibriConfig)` | One-liner from config |
| `From<&ColibriConfig> for ProbeOptions` | Idiomatic conversion |
| `MachineInfo::probe_for_config(&ColibriConfig)` | Hosts cannot miss `model_store` override |
| Tests: `probe_for_config_applies_model_store`, `public_inventory_types_are_usable`; stronger `machine_probe_smoke` | Assert public fields compile-use and fill on real hosts |
| `examples/plan_probe.rs` prints **every** public inventory field | Operator-visible completeness (full SIMD catalog, all libs, legacy disk fields) |
| `docs/user-guide.md` §2 explicit field tables | Match `MachineInfo` public surface |
| Crate docs (`lib.rs`) + `README.md` inventory table | Dependents see API without hunting `probe.rs` |
| `paths.rs` module docs | Clarify config is applied via probe helpers, not inside `resolve_model_store` |

## Public type / field inventory

### `MachineInfo` (crate root)

| Field | Type |
|-------|------|
| `total_memory` | `u64` |
| `available_memory` | `u64` |
| `swap_total` | `u64` |
| `swap_free` | `u64` |
| `physical_cores` | `u32` |
| `logical_cores` | `u32` |
| `sockets` | `u32` |
| `cpu` | `CpuInfo` |
| `gpus` | `Vec<GpuDevice>` |
| `npus` | `Vec<NpuDevice>` |
| `model_store` | `ModelStoreVolume` |
| `host_libraries` | `Vec<HostLibrary>` |
| `disk_free` | `Option<u64>` (legacy) |
| `disk_path` | `Option<PathBuf>` (legacy) |

### Nested public types (all re-exported)

| Type | Fields |
|------|--------|
| `CpuInfo` | `architecture`, `vendor`, `model_name`, `family`, `model`, `stepping`, `generation_hint`, `threads_per_core`, `big_little`, `simd`, `isa_flags` |
| `BigLittleInfo` | `hybrid`, `capacity_classes`, `note` |
| `SimdFeature` | `name`, `family`, `present`, `detail` |
| `NpuDevice` | `kind`, `name`, `device_path`, `details` |
| `HostLibrary` | `name`, `path`, `category` |
| `ModelStoreVolume` | `path`, `source`, `free_bytes`, `total_bytes` |
| `GpuDevice` | `index`, `name`, `total_bytes`, `free_bytes` |
| `ProbeOptions` | `model_store`, `disk_path`; methods `from_config` |
| `ModelStoreSource` | `Override` \| `Environment` \| `PlatformDefault` |

### Probe entry points

| API | Role |
|-----|------|
| `MachineInfo::probe()` | Default store path (env / platform) |
| `MachineInfo::probe_with(&ProbeOptions)` | Explicit options |
| `MachineInfo::probe_for_config(&ColibriConfig)` | **new** — uses `cfg.model_store` |
| `ProbeOptions::from_config(&cfg)` | **new** — config → options |

## Commands run + exit codes

| Command | Exit |
|---------|------|
| `cargo fmt -p colibri-sys` | 0 |
| `cargo clippy -p colibri-sys --all-targets -- -D warnings` | 0 |
| `cargo test -p colibri-sys --lib probe` | 0 (12 passed) |
| `cargo run -p colibri-sys --example plan_probe` | 0 |

Test names that passed under the filter: `machine_probe_smoke`,
`model_store_override_path`, `probe_for_config_applies_model_store`,
`public_inventory_types_are_usable`, `simd_catalog_mentions_major_families`,
SSD parse tests, Windows memory selection tests.

## Excerpt: `plan_probe` output (this host)

```
=== memory ===
total_memory_bytes=96630894592 (96.63 GB)
available_memory_bytes=52696702976 (52.70 GB)
swap_total_bytes=198275543040 (198.28 GB)
swap_free_bytes=170908938240 (170.91 GB)
=== cpu topology ===
physical_cores=8
logical_cores=16
sockets=1
threads_per_core=Some(2)
=== cpu identity / generation ===
architecture=x86_64
vendor=Some("AuthenticAMD")
model_name=Some("AMD Ryzen AI 7 PRO 350 w/ Radeon 860M")
generation_hint=Some("AMD Zen 5 (Strix Point / Ryzen AI 300 series)")
=== big.LITTLE / hybrid ===
big_little.hybrid=false
big_little.capacity_classes=[1024]
=== simd / isa (33 catalog entries) ===
  simd name=AVX512F family=avx512 present=true detail=foundation
  simd name=AVX512_VNNI family=avx512 present=true detail=vnni
  ...
=== model store volume ===
model_store.path=/home/hunter/.local/share/colibri/models
model_store.source=PlatformDefault
model_store.free_bytes=1469025640448 (1469.03 GB)
model_store.total_bytes=7899597209600 (7899.60 GB)
=== npus (1) ===
  npu kind=xdna
  npu name=|[0000:c6:00.1]  |RyzenAI-npu6  |
  npu device_path=Some("/dev/accel0")
=== host libraries (41) ===
  library category=xrt name=libxrt_core.so path=/usr/lib/libxrt_core.so
  library category=xrt name=libxrt_driver_xdna.so.2 path=/usr/lib/libxrt_driver_xdna.so.2
  ...
```

Fields present in full output: total mem, swap total/free, store free/path,
cores, arch/generation, full SIMD catalog (present + absent), NPU XDNA, all
host libraries, GPUs, legacy disk fields.

## Remaining platform stubs (honest)

| Gap | Notes |
|-----|--------|
| **Windows disk free** | Non-unix `fs_usage` returns fixed `(500 * GB, Some(1000 * GB))`, not real volume free space. RAM/swap via Win32 APIs are real. |
| **XDNA generation label** | Kind is `xdna`; no dedicated `xdna2` vs `xdna1` field (firmware/details text may still help). |
| **big.LITTLE counts** | Reports hybrid flag + capacity class values, not N performance / M efficiency core counts. |
| **macOS / Windows depth** | Richest path is Linux (`/proc`, `lscpu`, sysfs). Physical cores / sockets / flags are best-effort elsewhere. |
| **Intel NPU** | Soft accel/OpenVINO markers; not a full firmware identity path. |

No desktop wiring was added. No git commit/stage/push.
