# Phase B: AMD / ROCm detection honesty

**Repo:** `/home/hunter/Projects/surmount/colibri`
**Date:** 2026-08-10
**Scope:** colibri-sys probe/doctor (+ native display), surgical Python host parity
**NPU:** deferred for inference; inventory unchanged

---

## Summary

AMD hosts no longer fail closed to “no NVIDIA GPU” when ROCm lives under `/opt/rocm` but is off PATH. Doctor and plan copy are vendor-aware (CUDA vs HIP). Free VRAM near zero surfaces as a warn. Sysfs is a documented best-effort fallback only.

---

## Changes

### 1. `GpuDevice` inventory fields (`crates/colibri-sys/src/probe.rs`)

| Field | Meaning |
|-------|---------|
| `vendor` | `nvidia` / `amd` (empty when fixture omits) |
| `source` | `nvidia-smi` / `rocm-smi` / `sysfs` |
| `arch` | optional, e.g. `gfx1152` from rocm-smi |

Serde defaults keep older fixtures valid. `Default` derived for `..Default::default()` in tests.

### 2. PATH-resilient `rocm-smi`

Order:

1. bare `rocm-smi` (PATH)
2. `$ROCM_PATH/bin/rocm-smi`
3. `$ROCM_HOME/bin/rocm-smi`
4. `$HIP_PATH/bin/rocm-smi`
5. `/opt/rocm/bin/rocm-smi`

Helpers: `rocm_smi_path_candidates()`, `parse_rocm_smi_csv()` (public, fixture-tested). CSV parser skips ROCm warning lines before the header and reads GFX Version when present.

### 3. AMD sysfs fallback

`discover_amd_gpus_sysfs()` / `discover_amd_gpus_sysfs_from(drm_root)` when rocm-smi is missing or empty.

- Scans `/sys/class/drm/cardN` only (not connectors / `renderD*`)
- PCI vendor `0x1002`, prefers `DRIVER=amdgpu`
- Reads `mem_info_vram_total` / `mem_info_vram_used`
- Index is **0..N-1 of accepted AMD cards**, not the DRM card number (documented limit: may still differ from HIP ordinals)

**Limits (code comments + this report):** no product series name in many trees (PCI id only); free VRAM approximate under display load; prefer rocm-smi.

### 4. Free VRAM signal

`gpu_free_vram_near_zero()`: free &lt; 256 MiB or free &lt; 5% of total. Doctor uses this on the accelerator check.

### 5. Doctor vendor-aware messages (`crates/colibri-sys/src/doctor.rs`)

- Check id stays **`accelerator.cuda`** (schema stability).
- Messages no longer hardcode “NVIDIA” for AMD devices.
- `AcceleratorLinkage { linked, missing, kind }` with `kind` = `cuda` | `hip` | empty.
- Linux: `ldd` still matches `libcudart` / `libamdhip64`; kind set from which appears.
- **Windows:** `coli_hip.dll` beside the engine → HIP linked; else `coli_cuda.dll` + optional `[CUDA] mode: routed experts` marker (was always `(false, false)` in Rust).
- Injectables: `DoctorOptions.gpus`, `DoctorOptions.linkage` for tests and pre-probed hosts.
- Near-zero free VRAM: status `warn` even when HIP/CUDA is linked, summary notes display compositor.

Example summaries:

| Situation | Summary |
|-----------|---------|
| AMD + CPU engine | `AMD GPU detected but the engine is CPU-only (build with HIP=1)` |
| AMD + HIP linked | `HIP engine and AMD device(s) are available` |
| AMD + HIP + starved VRAM | same + `; free VRAM is near zero (display compositor may own most of it)` |
| no devices | `no GPU detected; CPU path is available` |
| missing HIP | `HIP runtime library (libamdhip64) is missing` |

### 6. Plan empty-GPU copy

Python `format_plan`: `VRAM   no GPU device detected · CPU path` (was “no NVIDIA…”).

### 7. Native display (`crates/colibri-native/src/host.rs`)

Summary shows arch/vendor when present; details show discovery `source`.

### 8. Python host parity (surgical)

- `c/resource_plan.py`: PATH-resilient rocm-smi, CSV parse, sysfs fallback, vendor/source/arch on devices, empty-VRAM plan wording.
- `c/doctor.py`: same vendor-aware accelerator messaging + Windows `coli_hip.dll`; linkage dict includes `kind`.

---

## Tests (no live GPU required)

**Rust (`cargo test -p colibri-sys --lib`):** 81 passed, including:

- `parse_rocm_smi_csv_gfx115x_igpu_fixture` (live-shaped 860M / gfx1152 CSV)
- `sysfs_amd_fallback_from_fixture_tree`
- `rocm_smi_path_candidates_include_opt_rocm`
- `accelerator_amd_cpu_engine_warns_without_nvidia_wording`
- `accelerator_amd_hip_pass_and_low_vram_warn`
- `accelerator_no_gpu_skips_without_nvidia_wording`
- `accelerator_missing_hip_runtime_message`

**Also:** `cargo test -p colibri-sys --test plan_golden`, `cargo test -p colibri-native`, Python `unittest tests.test_doctor tests.test_resource_plan` (65 OK).

**Verify commands:**

```
cargo fmt -p colibri-sys
cargo clippy -p colibri-sys --all-targets -- -D warnings
cargo test -p colibri-sys --lib
```

All green. Native: `cargo clippy -p colibri-native --all-targets -- -D warnings` green.

---

## Residual / not in this phase

| Item | Status |
|------|--------|
| NPU / XDNA for inference | Deferred (operator pin); inventory only |
| Multi-vendor merge (NVIDIA **and** AMD in one list) | Still NVIDIA-first exclusive |
| Vulkan as `accelerator.vulkan` doctor check | Not added |
| HIP runtime API (`hipGetDeviceCount`) tertiary probe | Not added |
| Windows full HIP discover via rocm-smi tools | Linkage for `coli_hip.dll` only; no Windows-specific smi PATH matrix beyond shared logic |
| COLI_CUDA wire rename | Unchanged by design (env ABI) |
| Live host integration test requiring GPU | Not required; fixtures cover honesty |

---

## Key paths

| Path | Role |
|------|------|
| `crates/colibri-sys/src/probe.rs` | discover, parse, sysfs, GpuDevice fields |
| `crates/colibri-sys/src/doctor.rs` | accelerator check, linkage, inject options |
| `crates/colibri-sys/src/lib.rs` | re-exports |
| `crates/colibri-native/src/host.rs` | display tags |
| `c/resource_plan.py` | Python discover + format_plan |
| `c/doctor.py` | Python doctor messages |
| `c/tests/test_doctor.py` | AMD honesty cases |

---

## Bottom line

Phase B delivers honest AMD/ROCm detection and reporting: find `rocm-smi` even when only under `/opt/rocm`, fall back to amdgpu sysfs with documented limits, and stop telling AMD operators they have no NVIDIA GPU.
