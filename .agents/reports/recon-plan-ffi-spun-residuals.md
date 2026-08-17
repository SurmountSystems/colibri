# Recon: FFI spun residuals (visual-abi / product-default / gpu)

**Date:** 2026-08-10
**Scope:** entry points only; process remains product default.

## Snapshot

| Residual | Today |
|----------|--------|
| `open:ffi-visual-abi` | Process: stdout HWINFO/EMAP/HITS/PROF/TIERS → `ServeClient` → `EngineHandle::pump_visual` → `VisualSnapshot`. FFI: open/generate only; `pump_visual` empty; STOP = cancel flag |
| `open:ffi-product-default` | `prefer_process=true`; desktop needs `feature=ffi` + `COLIBRI_PREFER_FFI=1`; kill-switch `COLIBRI_FORCE_PROCESS` |
| `open:ffi-gpu` | Static libs CPU-only; CUDA/HIP/Metal on process binaries / DLLs, not in FFI archives |

## Critical files

| Path | Why |
|------|-----|
| `c/colibri_api.h` | C embed ABI: open/size/generate/destroy only (no visual/STOP) |
| `c/telemetry.h` | Process emit: HWINFO, EMAP, HITS (+ layout contract) |
| `c/colibri.c`, `kimi_k3.c`, `inkling.c` | CLI + `*_NO_MAIN` libs; PROF/EMAP printf on process path |
| `c/Makefile` | `libcolibri` / `libkimi_k3` / `libinkling` (NOCUDA inkling); CUDA/HIP/Metal process targets |
| `c/Makefile.deepseek-v4` | `libdeepseek-v4` for V4 static |
| `c/backend_cuda.cu`, `backend_gpu_compat.h`, `backend_metal.mm`, `backend_vulkan.c`, `backend_loader.c` | GPU backends for process / DLL, not FFI link matrix |
| `crates/colibri-sys/src/ffi/{mod,multi,v4,bindings}.rs` | `open_engine`, `FfiEngine`, generate + cooperative cancel |
| `crates/colibri-sys/build.rs` | Links four CPU `.a`; system m/gomp/pthread only |
| `crates/colibri-sys/src/engine/{mod,serve,duplex}.rs` | Process spawn; parse serve lines; `stop_request`; `pump_visual` |
| `crates/colibri-sys/src/visual.rs` | `VisualSnapshot`, maps, `Subscribe` |
| `crates/colibri-sys/src/config.rs` | `prefer_process`, `must_use_process`, `prefer_ffi_path` |
| `crates/colibri-sys/docs/ffi-phase-d.md` | SoT: opt-in closed; three spins named |
| `crates/colibri-native/src/host.rs` | Dual path: `resolve_prefer_process`, `LiveEngine`, FFI empty visual |
| `crates/colibri-native/src/main.rs` | ~500ms visual pump timer; Brain/PROF UI |
| `crates/colibri-native/docs/fidelity.md` | Matrix rows for visual/default/GPU honesty |

## Reuse (symbols)

- **Process visual:** `ServeClient` line parsers (`HWINFO`/`EMAP`/`HITS`/`PROF`/`TIERS`); `pack_expert_cell` / `ExpertMap` / `ExpertHits`; `EngineHandle::pump_visual` / `visual_snapshot`; duplex `Subscribe::ALL`
- **Process STOP:** `ServeClient::stop_request`; native `EngineSession::stop_active` (mux)
- **FFI gen/stop:** `coli_*_generate` + `ColiTokenFn` nonzero = stop; native `AtomicBool` cancel; `FfiGenerateOptions`
- **Path select:** `ColibriConfig::prefer_process(false)` / `prefer_ffi_path`; `resolve_prefer_process`, `should_try_ffi_open`, `force_process_from_env`
- **Open:** `ffi::open_engine(FfiFamily, model_dir)`; family map via `FfiFamily::from_model_family`
- **GPU process only:** `make CUDA=1` / `HIP=1` / Metal objs; Windows `cuda-dll` / `hip-dll` + `backend_loader`

## Suggested step order + red tests

1. **visual-abi (design first):** poll or push API on C side (or shared snapshot struct) producing same bytes as `telemetry.h`; red: decode fixtures → `VisualSnapshot` equality without subprocess. Wire `FfiEngine` poll + native `pump_visual` for FFI branch. STOP: token-fn cancel already; red: mid-generate cancel returns early (exist multi.rs patterns). Do not claim mux multi-slot/grammar until designed.
2. **product-default (after visual parity + isolation story):** red: `prefer_process` default false only under explicit product flag; desktop start without env still process until flip; force-process still wins. Keep process fallback.
3. **gpu (one platform):** red: Makefile/build.rs link matrix includes one backend obj (e.g. Linux CUDA static or dynamic); inkling already documents NOCUDA strip. Prefer dynamic GPU like Windows DLL over bloating all `.a`.

## Risks

- Crash isolation: in-process fault kills host (docs already warn).
- Visual ABI must match process hex layouts or Brain/heat diverge.
- OpenMP process-global; re-entrancy forbidden per handle.
- GPU link pulls cudart/hip/Metal frameworks into host binary/rpath.
- Product-default before visual-abi leaves Brain/PROF dead on “default” path.

## Docs pins

Residual: `.agents/RESIDUAL.md` OPEN three spins. Phase D closeout: `crates/colibri-sys/docs/ffi-phase-d.md`.
