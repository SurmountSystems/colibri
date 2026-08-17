# Verify: colibri-sys rich machine resource probe

**Scope:** read-only check of probe inventory vs checklist items 1–9.
**Primary SoT:** `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/probe.rs`
**Also:** `paths.rs`, `config.rs`, `lib.rs`, `docs/user-guide.md` §2, `examples/plan_probe.rs`, probe module tests.

## Status: COMPLETE

All nine checklist areas are implemented in types, filled by `MachineInfo::probe` / `probe_with`, documented in the user guide, exercised by unit tests and the `plan_probe` example. Remaining issues are platform depth (Windows disk stub, NPU version labels) rather than missing fields.

## Field matrix (1–9)

| # | Requirement | Status | Types / fields | Evidence (path:line) |
|---|-------------|--------|----------------|----------------------|
| 1 | Total system memory | **present** | `MachineInfo.total_memory: u64`; `memory_total()` | Struct `136:140:crates/colibri-sys/src/probe.rs`; fill `222:223`; Linux `MemTotal` `284:284`; API `389:392`; re-export `lib.rs:101` |
| 2 | Swap total and free | **present** | `swap_total`, `swap_free` | Struct `141:144`; Linux `SwapTotal`/`SwapFree` `286:287`; macOS `311:317`/`322:339`; Windows pagefile-derived `360:363`; example print `33:38:examples/plan_probe.rs` |
| 3 | Free storage on model-store volume | **present** | `ModelStoreVolume { free_bytes, total_bytes: Option }` | Struct `52:62`; probe fill `197:205`; `disk_usage_bytes` + ancestor walk `733:740`; Unix `df` `756:795` |
| 4 | Model path default + `Option`/`Some(path)` override | **present** | `ProbeOptions.model_store: Option<PathBuf>`; `ColibriConfig.model_store: Option<PathBuf>`; `ModelStoreSource` | `ProbeOptions` `42:45`; resolve `74:89:paths.rs`; config `136:140`/`209:218:config.rs`; test override `1522:1533:probe.rs`; docs `111:117:docs/user-guide.md` |
| 5 | Logical vs physical cores (threads) | **present** | `physical_cores`, `logical_cores`, `cpu.threads_per_core` | Struct `145:148`/`79:79`; `physical_cpu_count` `518:568`; `logical_cpu_count` `394:399`; ratio `810:814` |
| 6 | big.LITTLE if detectable | **present** (Linux `cpu_capacity`) | `cpu.big_little: Option<BigLittleInfo>` with `hybrid`, `capacity_classes`, `note` | Types `88:96`; probe `988:1028`; docs `128:128:user-guide.md`. **None** when sysfs capacities absent (typical homogeneous x86 without capacity files) |
| 7 | Architecture, generation | **present** | `cpu.architecture`, `generation_hint`, plus `vendor`/`model_name`/`family`/`model`/`stepping` | `CpuInfo` `66:77`; `std::env::consts::ARCH` `808:808`; identity `841:938`; `generation_hint` (Zen 5 Strix Point etc.) `940:986` |
| 8 | SIMD + NPU with specific ISA / kinds | **present** (NPU gen label partial) | `Vec<SimdFeature>` (AVX512*, NEON/ASIMD, SVE, AMX, …); `Vec<NpuDevice>` (`xdna`, OpenVINO soft marker) | Catalog `1031:1128` (AVX512F/DQ/BW/VL/VNNI/BF16, NEON, SVE2, AMX…); NPU `1130:1239` (`/sys/class/accel`, `xrt-smi examine`); no dedicated string **XDNA2** vs XDNA1 |
| 9 | Relevant host libraries | **present** | `Vec<HostLibrary>` (`name`, `path`, `category`) | `discover_host_libraries` `1242:1322` (`ldconfig -p` + path fallbacks: cuda, rocm, xrt/xdna, vulkan, opencl, omp, onnx, openvino, blas) |

Also present beyond the checklist (context): GPUs (`discover_gpus`), sockets, available RAM (`available_memory`), SSD cache grammar (`.coli_ssd`, not live F_NOCACHE measure).

### Docs / example / tests (supporting)

| Artifact | What it covers |
|----------|----------------|
| `docs/user-guide.md` §2 (~87–136) | Full inventory field list, model-store precedence, detection table |
| `examples/plan_probe.rs` | Prints memory/swap, cores, arch/generation, big.LITTLE, SIMD, model store free, NPUs, host libs |
| `probe.rs` tests | `machine_probe_smoke`, `model_store_override_path`, `simd_catalog_mentions_major_families`, Windows mem selection tests |
| `paths.rs` tests | platform default shape, override wins |
| `lib.rs` | Re-exports `MachineInfo`, `ProbeOptions`, `CpuInfo`, `BigLittleInfo`, `SimdFeature`, `NpuDevice`, `HostLibrary`, `ModelStoreVolume`, helpers |

## Config override for model_store: how it works

**Precedence** (first wins), from `paths` module docs and `resolve_model_store`:

1. Explicit API: `ProbeOptions::model_store = Some(path)` (or legacy `disk_path` alone folded in) → `ModelStoreSource::Override`
2. Env: `COLIBRI_MODEL_STORE` then `COLI_MODEL_STORE` → `Environment`
3. Platform: `$XDG_DATA_HOME/colibri/models` or `~/.local/share/colibri/models` (Windows: `%LOCALAPPDATA%\colibri\models`) → `PlatformDefault`

**Probe path:** `MachineInfo::probe()` uses default options (no explicit path). `probe_with(&ProbeOptions { model_store: Some(...), .. })` overrides. Free/total bytes come from `disk_usage_bytes` on that path (nearest existing ancestor if the dir is not created yet).

**Config path:** `ColibriConfig.model_store: Option<PathBuf>` defaults to `None`. Builder `model_store(path)` sets `Some`. `resolved_model_store()` returns override or `default_model_store_path()` (env + platform). **Note:** `MachineInfo::probe` does **not** auto-read `ColibriConfig`; hosts must pass `cfg.model_store` into `ProbeOptions` (or rely on env/platform). User guide shows both patterns side by side.

**Example quirk:** `plan_probe` uses env `COLIBRI_PROBE_MODEL_STORE` for the `ProbeOptions` override; bare `COLIBRI_MODEL_STORE` still applies when that is unset, via `resolve_model_store(None)`.

## Gaps if any (concrete)

None of the nine checklist items are missing as product fields. Nuances:

1. **Windows disk free is a stub** — `fs_usage` non-unix returns fixed `(500 * GB, Some(1000 * GB))` (`probe.rs:797-800`). Not real volume free space on Windows.
2. **XDNA generation not versioned** — NPU kind is `xdna` / name from sysfs or `xrt-smi`; there is no explicit **XDNA2** (vs XDNA1) ISA/version field. Presence of Ryzen AI stack is still detectable.
3. **Intel NPU** — only soft path via accel class / OpenVINO tools marker; no rich Intel NPU firmware identity beyond that.
4. **big.LITTLE detail** — reports capacity class values and hybrid flag, not “N performance / M efficiency cores” counts.
5. **Config ↔ probe wiring** — `ColibriConfig.model_store` is not automatically applied by `MachineInfo::probe()`; integration is caller-owned.
6. **macOS / Windows** — physical cores / sockets / flags / swap are best-effort; richest path is Linux (`/proc`, `lscpu`, sysfs).

## Suggested minimal diffs if incomplete

Status is **COMPLETE** for the checklist; optional polish only:

1. **Windows real disk free:** replace non-unix `fs_usage` stub with `GetDiskFreeSpaceExW` (or equivalent) on the resolved store path.
2. **XDNA2 label (if desired):** when `xrt-smi` / sysfs text mentions XDNA2 / Strix Halo generation, set `NpuDevice.details` or `kind` to something like `xdna2` rather than only `xdna`.
3. **Wire config into probe (optional API):** e.g. `MachineInfo::probe_for_config(&ColibriConfig)` that sets `ProbeOptions.model_store = cfg.model_store.clone()` so callers do not hand-plumb twice.
4. **big.LITTLE core counts:** when multiple `cpu_capacity` values exist, count CPUs per class in `BigLittleInfo` (new fields) if plans need them.

No product edits made in this verification pass.
