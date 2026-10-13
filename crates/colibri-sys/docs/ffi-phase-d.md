# Phase D: multi-family in-process FFI (CPU static)

**Status (2026-08-10):** Multi-family CPU FFI complete. **Native host**
(`colibri-native` with `feature = "ffi"`) defaults to try FFI first; crate
`ColibriConfig.prefer_process` stays process-prefer for library embeds.
Visual poll ABI shipped for GLM (`open:ffi-visual-abi` closed). Product-default
flip closed as **native-host only** (`open:ffi-product-default` closed).
GPU embed: opt-in Linux CUDA for GLM (`open:ffi-gpu` closed; feature `ffi-cuda`)
and opt-in Linux HIP/ROCm for GLM (feature `ffi-hip`; one vendor per binary).
Default `ffi` stays CPU-only.

Multi-family CPU static libs + Rust `feature = "ffi"` are implemented for product
engines GLM, Kimi K3, Inkling, and DeepSeek V4. Size metadata, kill-switch,
tiny golden process↔FFI token parity (GLM tiny), desktop FFI-first with process
fallback, and embed visual poll (GLM full fill) are in tree.

| Product path | Status |
|--------------|--------|
| Process serve mux | **Default for library embeds** (`ColibriConfig.prefer_process = true`); also native path without `feature=ffi`, under `COLIBRI_FORCE_PROCESS`, or after FFI open failure |
| Multi-family CPU static FFI | **Shipped**; **colibri-native** defaults FFI-first when built with `feature = "ffi"` |
| Brain / live PROF / HWINFO in-process (GLM) | **Shipped** via `coli_glm_visual_poll` + `FfiEngine::pump_visual` (`open:ffi-visual-abi` closed) |
| Kimi / Inkling visual fill | **Stub** (empty success); V4 **empty** until symbols exist |
| Mid-generate cancel (FFI) | **Cooperative** token callback plus embed stop checked in `spec_decode` and chunked prefill; mux multi-slot STOP stays process-only |
| Product-default in-process engine | **Closed** as native-host default under `feature=ffi` (`open:ffi-product-default`); crate config stays process-prefer |
| GPU in static/dynamic link | **Opt-in Linux CUDA** (`ffi-cuda`) and **opt-in Linux HIP** (`ffi-hip`) for GLM; default `ffi` CPU-only; mutually exclusive vendors |
| NPU inference | **Deferred** → residual `open:npu-inference` |

**Honesty for native desktop:** the GPUI host embeds **colibri-sys in-process**
(probe, plan, doctor, duplex, optional install). When built with
`feature = "ffi"`, start **tries in-process open first** and falls back to the
serve mux child on failure. Without that feature, or with
`COLIBRI_FORCE_PROCESS=1`, the engine is a **separate C process**. Library
embeds still default `prefer_process = true` until they opt out.

## What shipped (opt-in complete)

| Piece | Path |
|-------|------|
| GLM static lib (no CLI `main`) | `make -C c libcolibri` → `c/libcolibri.a` (`COLIBRI_NO_MAIN`) |
| Kimi K3 static lib | `make -C c libkimi_k3` → `c/libkimi_k3.a` (`KIMI_NO_MAIN`) |
| Inkling static lib (CPU) | `make -C c libinkling` → `c/libinkling.a` (`INKLING_NO_MAIN`) |
| DeepSeek V4 static lib | `make -f c/Makefile.deepseek-v4 libdeepseek-v4` → `c/libdeepseek_v4.a` |
| Shared C size / open API | `c/colibri_api.h`, `c/coli_model_size.c` |
| V4 public C API | `c/deepseek_v4.h` (`coli_v4_engine_*`, `coli_v4_session_*`) |
| Rust link | `crates/colibri-sys/build.rs` when `feature = "ffi"` (all four libs) |
| Family-selected open | `colibri_sys::ffi::{FfiFamily, FfiEngine, open_engine}` |
| Per-family wrappers | `GlmEngine`, `KimiEngine`, `InkEngine`, `V4Engine` / `V4Session` |
| Size metadata | `ModelInfo::{disk_bytes, engine_id, param_count}`, `ModelSizeInfo`, `FfiEngine::size_info` |
| Kill-switch | `COLIBRI_FORCE_PROCESS`, `ColibriConfig::prefer_process` (crate default true) |
| Tiny golden process↔FFI | `glm_tiny_process_ffi_token_parity` (greedy token ids on `c/glm_tiny`) |
| Desktop FFI-first + fallback | `colibri-native` feature `ffi` defaults try FFI; open/generate fall back to process; `COLIBRI_PREFER_FFI` kept (redundant under that default) |

### Build static libraries

```bash
cd c
make libcolibri LTO=0
make libkimi_k3 LTO=0
make libinkling LTO=0
make -f Makefile.deepseek-v4 libdeepseek-v4 LTO=0
# artifacts: libcolibri.a, libkimi_k3.a, libinkling.a, libdeepseek_v4.a  (no `main` in archives)
```

Optional env overrides (skip make for that lib in `build.rs`):

| Env | Artifact |
|-----|----------|
| `COLIBRI_V4_STATIC_LIB` | `libdeepseek_v4.a` |
| `COLIBRI_GLM_STATIC_LIB` | `libcolibri.a` |
| `COLIBRI_KIMI_STATIC_LIB` | `libkimi_k3.a` |
| `COLIBRI_INKLING_STATIC_LIB` | `libinkling.a` |

### Cargo features

```toml
# CPU-only multi-family embed (product default for FFI)
colibri-sys = { path = "...", features = ["ffi"] }

# Opt-in Linux CUDA for GLM embed only (implies ffi)
colibri-sys = { path = "...", features = ["ffi-cuda"] }

# Opt-in Linux HIP/ROCm for GLM embed only (implies ffi)
colibri-sys = { path = "...", features = ["ffi-hip"] }
```

Default builds **do not** compile or link C. Enabling `ffi` builds (or uses
prebuilt paths for) all four archives and links them with `-lm -lgomp -lpthread`.

**CPU is the default** for `feature = "ffi"`. GPU is **opt-in** (one vendor):

| Flag | Effect |
|------|--------|
| `feature = "ffi-cuda"` | Implies `ffi`; `build.rs` runs `make libcolibri CUDA=1` when `nvcc` is found |
| Env `COLIBRI_FFI_CUDA=1` | Same ask while building with `ffi` (no need for the Cargo feature name) |
| Env `COLIBRI_REQUIRE_FFI_CUDA=1` | Hard-fail if CUDA toolkit missing (no CPU fallback) |
| Make `libcolibri CUDA=1` | Packs `backend_cuda.o` into `libcolibri.a` (host must still link cudart) |
| `feature = "ffi-hip"` | Implies `ffi`; `build.rs` runs `make libcolibri HIP=1` when `hipcc` + `libamdhip64` found |
| Env `COLIBRI_FFI_HIP=1` | Same ask while building with `ffi` |
| Env `COLIBRI_REQUIRE_FFI_HIP=1` | Hard-fail if ROCm missing (no CPU fallback) |
| Make `libcolibri HIP=1` | Packs HIP-built `backend_cuda.o` (host must still link amdhip64) |
| `ROCM_HOME` / `ROCM_PATH` | ROCm root (default `/opt/rocm`) |
| `HIP_ARCH` | `native` or explicit `gfxXXXX` for hipcc offload |

Without a matching toolkit, `ffi-cuda` / `ffi-hip` **fall back to CPU-only** GLM
(cargo warning) so default CI stays green. When the toolkit is present:

- rustc cfg `ffi_cuda_linked` + `ffi::ffi_cuda_linked()` for CUDA + cudart
- rustc cfg `ffi_hip_linked` + `ffi::ffi_hip_linked()` for HIP + amdhip64

Query helpers:

- `ffi::ffi_cuda_feature_enabled()` / `ffi::ffi_cuda_linked()`
- `ffi::ffi_hip_feature_enabled()` / `ffi::ffi_hip_linked()`
- `ffi::ffi_gpu_linked()` — either vendor actually linked

**Mutual exclusion:** `ffi-cuda` and `ffi-hip` (or both env flags) cannot be
combined; `build.rs` panics with a clear error. One GPU vendor link mode per binary.

**Scope honesty:** GPU embed is **Linux**, **GLM only**. Kimi / Inkling / V4 stay
CPU in the FFI matrix. Metal, Vulkan, and NPU are **not** in the FFI static
matrix (`open:npu-inference` remains deferred).

### Feature matrix (AMD vs NVIDIA)

| Build | GPU kernels (GLM in-process) |
|-------|------------------------------|
| `ffi` only | CPU only |
| `ffi` + `ffi-hip` (ROCm present) | HIP + `libamdhip64` |
| `ffi` + `ffi-cuda` (CUDA present) | CUDA + `cudart` |
| process `HIP=1` / `CUDA=1` engine | Process path (not FFI) |
| `ffi-cuda` + `ffi-hip` | **Build error** (mutual exclusion) |

## Availability split

| API | Meaning |
|-----|---------|
| `ffi::ffi_link_available()` | Linked static engines (always true under `feature = "ffi"`). |
| `ffi::linked_families()` | `Glm`, `Kimi`, `Inkling`, `DeepseekV4` in this build. |
| `ffi::ffi_available()` | Link available **and** `COLIBRI_FORCE_PROCESS` is not forcing process. Does **not** open weights. |
| `ffi::ffi_family_available(family)` | Same, for one family. |
| `ColibriConfig::must_use_process()` | Env force **or** `prefer_process` **or** no FFI link. |
| `ColibriConfig::prefer_ffi_path()` | Inverse of `must_use_process`. |
| `ffi::open_engine(family, model_dir)` | Real load by family; may fail (missing weights, bad config, OOM). Refuses when env kill-switch is on. |

`EngineHandle::start_blocking` **always** spawns a subprocess. Hosts choose
process vs FFI **before** calling start; they do not get automatic dual-path
inside `EngineHandle`. Desktop `colibri-native` implements dual-path +
fallback at the host layer (FFI-first when `feature = "ffi"`).

## Kill-switch (mandatory)

1. Env: `COLIBRI_FORCE_PROCESS=1` → treat process as required; open/generate
   refuse; `ffi_available()` is false.
2. Config: `ColibriConfig::prefer_process` defaults **true** for library embeds
   until a host opts out. **Native host** sets `prefer_process = false` at start
   when built with `feature = "ffi"` (unless force-process).
3. On FFI open failure: host should fall back to process embed (do not abort).

Falsy env values: unset, empty, `0`, `false`, `no`, `off` (case-insensitive).

Desktop still accepts `COLIBRI_PREFER_FFI=1` (redundant under native `feature=ffi`
default; still useful documentation / explicit opt-in). Always loses to
`COLIBRI_FORCE_PROCESS`.

## Model size metadata (mandatory)

Hosts need **raw bytes** on public types for “how large is this model,” not
only human strings.

| Type | Always / when | Fields |
|------|---------------|--------|
| `ModelInfo` | `ModelInfo::inspect` (no `ffi` feature required) | `disk_bytes`, `model_bytes` (same), `engine_id`, `family`, optional `param_count` |
| `ModelSizeInfo` | `ModelInfo::size_info()`, plan overlay, FFI open | `disk_bytes`, `family`, `engine_id`, optional `param_count`, optional `tier_*_bytes` when plan known |
| `PlacementPlan` model summary | After plan build | `disk_bytes`, `param_count` where known |
| C `ColiModelSizeSummary` | Open / `coli_model_size_probe` | `disk_bytes`, dense/expert when known, `param_count` + family/engine strings |
| `FfiEngine::size_info()` | After open under `ffi` | Prefer Rust inspect; overlay C size where useful |

Unit tests use tiny fixture dirs (not multi-GB weights).

```rust
// no ffi feature required
let info = colibri_sys::ModelInfo::inspect("/path/to/model")?;
assert!(info.disk_bytes > 0);
let size = info.size_info(); // ModelSizeInfo

// feature = "ffi"
let eng = colibri_sys::ffi::open_engine(
    colibri_sys::ffi::FfiFamily::Glm,
    "/path/to/model",
)?;
let s = eng.size_info();
assert_eq!(s.family, "glm");
assert!(s.disk_bytes > 0);
```

## Rust surface (`feature = "ffi"`)

```text
pub mod ffi {
    pub fn ffi_link_available() -> bool;
    pub fn ffi_available() -> bool;
    pub fn ffi_family_available(FfiFamily) -> bool;
    pub fn linked_families() -> &'static [FfiFamily];
    pub enum FfiFamily { Glm, Kimi, Inkling, DeepseekV4 }
    pub enum FfiEngine { Glm(GlmEngine), Kimi(KimiEngine), Inkling(InkEngine), DeepseekV4(V4Engine) }
    pub fn open_engine(family, model_dir) -> Result<FfiEngine>;
    pub struct V4Engine { ... }   // also open via V4Engine::open
    pub struct V4Session<'a> { ... }
}
```

Without the feature, the `ffi` module is not compiled. Kill-switch helpers on
`ColibriConfig` and `force_process_from_env` are always available. Size fields
on `ModelInfo` / `ModelSizeInfo` are always available.

## Visual / stop (embed poll) — **shipped** (`open:ffi-visual-abi` closed)

C embed poll (`coli_*_visual_poll`) + Rust `FfiEngine::pump_visual` + native
`LiveEngine::Ffi` pump are in tree. **GLM** has full fill; **Kimi/Inkling** stubs
return empty success until family fill lands; **DeepSeek V4** has no poll
symbols yet (`pump_visual` → empty snapshot).

Closing `open:ffi-visual-abi` means Brain / live PROF / HWINFO / TIERS work on
**pure FFI for GLM** without a SERVE child. It does **not** mean full
multi-family visual fill, V4 poll symbols, or mux multi-slot STOP in-process.
(Native product-default FFI and one-platform GPU are separate closed residuals
below.)

| Capability | Process path (library default / fallback) | CPU FFI (native default under `feature=ffi`) |
|------------|-------------------------------------------|-----------------------------------------------|
| Expert map / Brain (EMAP, HITS) | Serve mux visual frames | **GLM:** `coli_glm_visual_poll` → `VisualSnapshot`. Kimi/Inkling: stub empty. V4: empty |
| Live PROF turns | Serve mux profile window | **GLM:** last embed generate PROF (`valid`/`seq`). Stubs/V4: empty |
| HWINFO / TIERS strip | Serve mux | **GLM:** same fields from poll. Stubs/V4: empty |
| Mid-generate STOP | Mux `STOP` with `req_id` | Cooperative cancel via token callback only (no mux STOP) |
| Multi-slot KV / grammar on pure FFI | Mux | Process path only |

**`open:ffi-product-default` closed:** native host defaults to FFI under
`feature = "ffi"`; crate config remains process-prefer; isolation policy below.

## GPU link — Linux CUDA and HIP for GLM

Residual id: **`open:ffi-gpu`** — **closed** for CUDA one-platform bar (2026-08-10).
HIP in-process embed: feature **`ffi-hip`** — **landed** (plan `plan-rocm-unified-ddr5`
Steps A–B; host live generate remains operator-gated smoke).

| Piece | Detail |
|-------|--------|
| Default `feature = "ffi"` | **CPU-only** archives (unchanged) |
| Opt-in CUDA | Cargo `ffi-cuda` and/or env `COLIBRI_FFI_CUDA=1`; Makefile `libcolibri CUDA=1` |
| Opt-in HIP | Cargo `ffi-hip` and/or env `COLIBRI_FFI_HIP=1`; Makefile `libcolibri HIP=1` |
| Platform / family | **Linux**, **GLM** only (`backend_cuda.o` + cudart **or** amdhip64) |
| Mutual exclusion | One vendor per binary; both features/envs → `build.rs` panic |
| No toolkit | build.rs CPU fallback + warning; optional `COLIBRI_REQUIRE_FFI_CUDA=1` / `COLIBRI_REQUIRE_FFI_HIP=1` hard fail |
| Smoke | `ffi::ffi_cuda_*` / `ffi::ffi_hip_*` unit tests; ignored host-gated link smokes; full generate: operator checklist in `.agents/reports/impl-rocm-uma-runtime-smoke.md` |
| Not claimed here | Metal / Vulkan FFI static; multi-family GPU; NPU (`open:npu-inference`) |

Process binaries still use `make colibri CUDA=1` / `HIP=1` / Windows DLL paths
as before. FFI GPU does **not** change native FFI-first vs process kill-switch
policy.

## Still out of scope / spun residuals

| Residual | Gap |
|----------|-----|
| `open:npu-inference` | NPU inference (deferred) |

**Closed:** `open:ffi-visual-abi` (GLM visual poll + cooperative cancel;
Kimi/Inkling stub; V4 empty; mux multi-slot STOP remains process-only);
`open:ffi-product-default` (native-host FFI-first under `feature=ffi`; crate
`prefer_process` stays true; process fallback + `COLIBRI_FORCE_PROCESS`);
`open:ffi-gpu` (Linux CUDA GLM embed opt-in; default `ffi` stays CPU-only).

Also not claimed:

- Golden token parity vs process on full production weights for every family
  (GLM tiny covered where fixtures exist)
- Full UTF-8 detokenize stream on multi-family CPU FFI (token-id API dominant)
- Inkling audio path in embed generate (text-only open/generate)
- Full Kimi/Inkling visual fill or V4 poll symbols (stubs/empty are intentional)

## Current process embed (library default / fallback)

| Binary | Family | Locate |
|--------|--------|--------|
| `colibri` | GLM / default | `COLI_ENGINE`, libexec, in-tree `c/` |
| `inkling` | Inkling | same |
| `kimi_k3` | Kimi | same |
| `deepseek_v4` | DeepSeek V4 CLI | same (subprocess still valid) |

Host owns: probe, plan, doctor, model registry, serve mux client, visual
snapshots, rkyv duplex. Engines own: weights, decode, SERVE/SERVE_BATCH.

## Thread and device ownership (CPU static)

- Generate is not re-entrant per session/engine; one owner thread per handle.
- OpenMP team is process-global; host should not nest conflicting OpenMP runtimes.

### Isolation policy (accepted for native `feature=ffi` default)

In-process FFI shares the host address space. A fault in engine code (segfault,
abort, runaway OpenMP) can **kill the whole GPUI / host process**. There is no
crash isolation comparable to a SERVE child.

**Crash isolation is not oomd isolation.** A SERVE child in the same user slice
can still fill RAM and trip systemd-oomd. Embed samples MemAvailable before
`model_init` (same as the CLI), then runs the CLI `cap_for_ram` clamp. A
refuse returns an error (does not `exit(2)`) and tears down the Model so
weight slabs do not stay in the host. Native Start preflight refuses before
`coli_glm_engine_open` when one slot cannot fit. Inspect failure fails closed
unless overcommit is on. A C RAM open error or a cooperative `stopped`
generate does not start a process fallback. `COLI_RAM_OVERCOMMIT=1` remains
the override. Process mode still isolates SIGSEGV only. There is no cgroup
`MemoryMax` on the app scope yet. HIP kernels are not niced. Default load is
still pread plus malloc (`COLI_MMAP` is not enabled). Prefill Stop is checked
between layers on the default path; leftover `layers_forward` after a chunk
break is skipped.

**Product accept (plan Step 4):** native desktop built with `feature = "ffi"`
defaults to try FFI first. Operators who need process isolation set
`COLIBRI_FORCE_PROCESS=1` or build without `feature = "ffi"`. Prefer process for
long-running or untrusted workloads. Library embeds keep
`ColibriConfig.prefer_process = true` until they explicitly opt into FFI.

| Control | Role |
|---------|------|
| Crate `prefer_process = true` | Library embeds stay process-prefer by default |
| Native host + `feature = "ffi"` | Try in-process open first; fall back to process on open failure |
| `COLIBRI_FORCE_PROCESS=1` | Kill-switch: refuse FFI open/generate; force process |
| Build without `feature = "ffi"` | No static link; process only |

**`open:ffi-product-default` closed** with this native-host-only flip and the
isolation story above. Crate-wide `prefer_process` default is **not** flipped.

## Acceptance (opt-in complete bar)

1. `make -C c libcolibri`, `make -C c libkimi_k3`, `make -C c libinkling`, `make -f Makefile.deepseek-v4 libdeepseek-v4` build no-`main` archives (CPU Linux).
2. `cargo test -p colibri-sys --lib --features ffi` links multi-family + size + kill-switch + tiny golden + visual poll contracts where fixtures exist.
3. Default `cargo test -p colibri-sys --lib` (no ffi) still passes; size fields on `ModelInfo` covered.
4. Kill-switch tested: env matrix + `prefer_process` crate default.
5. Desktop: `colibri-native` feature `ffi` defaults FFI-first + process fallback; FFI `pump_visual` for GLM.
6. Docs honest: native FFI-first under `feature=ffi`; crate process-prefer; `open:ffi-visual-abi` + `open:ffi-product-default` + `open:ffi-gpu` closed; NPU deferred.
7. GPU opt-in: `cargo test -p colibri-sys --lib --features ffi` stays CPU-green; `ffi-cuda` without toolkit still builds (CPU fallback).

## References

- Process path: `crates/colibri-sys/src/engine/`
- Serve mux: `docs/serve_protocol.md`, `c/openai_server.py`
- GLM/Kimi/Inkling: `c/colibri.c`, `c/kimi_k3.c`, `c/inkling.c`, `c/colibri_api.h`, `c/Makefile`
- V4: `c/deepseek_v4.c`, `c/deepseek_v4.h`, `c/Makefile.deepseek-v4`
- Desktop FFI-first: `crates/colibri-native` feature `ffi`, `host.rs` (`resolve_prefer_process`)
- Visual poll: `c/colibri_api.h` (`coli_*_visual_poll`), `crates/colibri-sys/src/visual.rs`, `ffi/multi.rs`
- GPU embed: `crates/colibri-sys/build.rs` (`ffi-cuda`), `c/Makefile` (`libcolibri CUDA=1`)
- Reports: `.agents/reports/impl-track-ffi-libcolibri.md`, `impl-ffi-d0-d1-inkling.md`, `impl-ffi-d2-golden.md`, `impl-ffi-d3-desktop.md`, `impl-ffi-d6-closeout.md`, `impl-ffi-visual-c-api.md`, `impl-ffi-visual-rust-native.md`, `impl-ffi-visual-docs-residual.md`, `impl-ffi-product-default.md`, `impl-ffi-gpu-one-platform.md`
- Residual: `.agents/RESIDUAL.md` (`open:ffi-phase-d` + `open:ffi-visual-abi` + `open:ffi-product-default` + `open:ffi-gpu` closed; NPU deferred)
