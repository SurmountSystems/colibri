# colibri-native

Native **GPUI** desktop shell for [colibri-sys](../colibri-sys). One window that
reads your machine, runs readiness checks and placement plan, chats with a local
model, shows live memory placement and expert activity, and can install Hugging
Face models into the model store.

This is the **local embed** product path: no browser, no Python gateway, no
separate REST face. The Rust host library is linked in-process; the C engine
still runs as its own process.

**Visual system** matches the `web/` SPA design family: mint brand (`#4ed6a5`),
near-black teal background, left lifecycle rail, top tabs **Chat | Brain |
Profiling**, chat hero + suggested prompts, live topbar badges, proportional
tier bar, and a Profiling page with share bars / phase charts / turn table.
Locales: English and Italian. Layout density is “same product family,” not
pixel-perfect CSS spacing.

## What you get

| Surface | Behavior |
|---------|----------|
| **Shell** | Left rail (~292px) + main column; top tabs Chat / Brain / Profiling; mint design tokens |
| **Machine** | Short summary by default (memory, cores, GPU including AMD/ROCm when present); Details for SIMD / NPU inventory / store |
| **Doctor** | Quick checklist (**Run checks**) or thorough tensor/shard validation (**Deep check**) for the selected model path |
| **Plan / model** | Model path, placement summary, start engine |
| **Model registry** | Scan the model store; pick a downloaded model to fill the path |
| **Chat** | Hero empty state + suggested prompts; stream tokens; **Stop** mid-generate; Clear |
| **Topbar badges** | Live tokens, tok/s, TTFT (when available), session slot |
| **Inference controls** | Temperature, max tokens, reasoning toggle, optional GBNF grammar, multi-slot session when the engine advertises KV slots |
| **Memory placement** | Proportional VRAM / RAM / disk tier bar + live expert counts |
| **Live hardware** | Engine HWINFO strip (RAM free/total, cores, CPU/GPU names, VRAM) while running |
| **Profiling** | Full page: metric tiles, phase share bars, tok/s + stacked phase columns, reverse turn table (web phase model) |
| **Brain** | Full-page expert grid with heat/hits pulse; hover atlas tips (`experts.json`); default sample ≤2048 with **Full grid** toggle / `COLIBRI_BRAIN_FULL` |
| **i18n** | English + Italian (locale toggle in rail footer) |
| **HF install** (feature `install`, default on) | Download a model into the store; **Cancel** stops the job; min free GB gate before download |

Optional **About** in the rail footer (off by default). Engineering host details
stay out of the main chrome.

### AMD / ROCm and NPU

Probe and doctor surface **AMD GPUs** (via `rocm-smi` / sysfs) and HIP linkage
on the engine binary the same way they surface NVIDIA/CUDA. Machine Details may
list **NPU inventory** (for example AMD XDNA / Ryzen AI) when the host detects
it. **NPU inference is not wired** in this app: inventory and probe only.

## Relation to `desktop/` (Tauri)

| Path | Role |
|------|------|
| [`desktop/`](../../desktop/) | Thin Tauri v2 shell around `web/`. Talks HTTP to a separately started gateway. Does not link colibri-sys. |
| **This crate** | Native GPUI (`colibri-native`). Embeds colibri-sys. Spawns the engine itself. |

Both can coexist. Tauri remains the full SPA dashboard against a running server.
This crate is the one-window local app path with the same visual design system
and layout density as `web/`, plus native lifecycle (Machine, Doctor, Plan,
Start, Install) that the HTTP SPA does not own.

## How to run

From the repository root:

```bash
cargo run -p colibri-native
```

The main window opens **fullscreen** by default (GPUI `WindowBounds::Fullscreen`
with a 1280×820 restore size). For a centered windowed launch while developing:

```bash
COLIBRI_WINDOWED=1 cargo run -p colibri-native
```

(`true`, `yes`, or `windowed` also work.)

Default features are `install` + `ffi` (HF download UI and in-process CPU
static engines). Process-only: `--no-default-features --features install`, or
runtime kill-switch `COLIBRI_FORCE_PROCESS=1`. C static libs follow existing FFI
docs (`colibri-sys` Phase D).

**GPU in-process (opt-in):** default `ffi` is **CPU-only** for GPU kernels.
On an AMD/ROCm machine, build with HIP embed:

```bash
# Optional: ROCM_HOME=/opt/rocm HIP_ARCH=native (or gfxXXXX)
cargo build -p colibri-native --features ffi-hip
# After link, verify: ldd target/debug/colibri-native | grep amdhip64
```

Process HIP remains available via `make -C c colibri HIP=1` if you force the
process engine. Do not enable `ffi-cuda` and `ffi-hip` on the same binary.

**UMA / APU:** probe and plan treat integrated AMD as shared system memory (not
carve-out VRAM alone). Override with `COLI_GPU_MEMORY=unified` or `discrete`.
See [GPU_BACKENDS.md](../../GPU_BACKENDS.md). Live generate on a ROCm host is
operator-gated (model + HIP link); unit plan/doctor contracts cover the UMA
budget without a live GPU.

Runnable **without a model**: Machine and Doctor
panels fill on first paint. Chat stays empty until you set a model path and
start the engine.

### With a model (chat)

```bash
export COLIBRI_MODEL=/path/to/model   # or COLI_MODEL
export COLI_ENGINE=/path/to/colibri   # optional if binary is on PATH / discoverable
cargo run -p colibri-native
```

Then **Start engine** (or Send, which auto-starts when the path is a directory).
While generating, **Stop** cancels the active request.

### Env vars

| Variable | Purpose |
|----------|---------|
| `COLIBRI_MODEL` / `COLI_MODEL` | Model directory (`SNAP`) |
| `COLI_ENGINE` / `COLIBRI_ENGINE` | Override C engine binary path |
| `COLIBRI_MODEL_STORE` / `COLI_MODEL_STORE` | Model store root (probe free space / installs) |
| `COLIBRI_KV_SLOTS` / `KV_SLOTS` | Preferred KV session slots when starting the engine |
| `COLIBRI_PREFER_FFI` | Truthy prefer in-process FFI when linked. Under feature `ffi` the native host already defaults to FFI-first, so this env is mostly redundant; kept for explicit opt-in and builds without the feature |
| `COLIBRI_FORCE_PROCESS` | When truthy, always use the engine process (overrides FFI default / prefer-FFI) |

Other placement and serve env keys follow colibri-sys / `docs/ENVIRONMENT.md`.

## UI preferences

Shell prefs (theme, locale, first-run, last model path) live at
`~/.config/colibri/native-ui.toml` (or `%LOCALAPPDATA%\colibri\native-ui.toml`
on Windows). Load prefers that TOML file; if it is missing or invalid, a
sibling `native-ui.json` with the same fields is accepted for existing users.
Saves always write TOML and leave any JSON file in place. `COLIBRI_THEME` and
`COLIBRI_SKIP_WIZARD` still override after load.

## Features

| Cargo feature | Default | Effect |
|---------------|---------|--------|
| `install` | yes | Enables HF install form + `colibri-sys/install` |
| `ffi` | yes | Links multi-family **CPU-only** static engines (`colibri-sys/ffi`). Native host defaults to try FFI first (process fallback on open failure). Kill-switch: `COLIBRI_FORCE_PROCESS=1`. Process-only: `--no-default-features --features install`. |
| `ffi-hip` | no | Implies `ffi`; Linux HIP/ROCm GLM embed (links `amdhip64`). Mutually exclusive with `ffi-cuda`. |
| `ffi-cuda` | no | Implies `ffi`; Linux CUDA GLM embed. Mutually exclusive with `ffi-hip`. |

## Build / runtime needs

```bash
cargo build -p colibri-native
# Default = install + ffi (needs C static engines via colibri-sys build.rs / make).
# Process-only: cargo build -p colibri-native --no-default-features --features install
cargo test -p colibri-native
cargo clippy -p colibri-native --all-targets -- -D warnings
```

**Compile** needs the usual Linux GPUI stack (Vulkan/OpenGL capable GPU driver
headers as required by `gpui` 0.2 / blade). **Run** needs a display (X11 or
Wayland). Headless CI can still `cargo build` / `cargo test` the host helpers;
the window itself will fail without a display server.

## Architecture note (developers)

App chrome is product-facing. Host wiring uses colibri-sys in-process for probe,
plan, doctor, and duplex. Without Cargo feature `ffi`, the **C engine is a
subprocess** on the serve mux. With `feature = "ffi"`, start tries multi-family
CPU static open first and falls back to process on failure; kill-switch
`COLIBRI_FORCE_PROCESS=1`. Library `ColibriConfig.prefer_process` stays
process-prefer for embeds. See [docs/fidelity.md](docs/fidelity.md) and
colibri-sys [Phase D](../colibri-sys/docs/ffi-phase-d.md).

```
GPUI → colibri-sys (in-process host)
         ├── feature=ffi open_engine              [native default when linked]
         │     └── fallback → ServeClient mux → C engine process
         └── ServeClient mux → C engine process   [no feature=ffi, or FORCE_PROCESS]
```

## License

MIT (same as colibri-sys).
