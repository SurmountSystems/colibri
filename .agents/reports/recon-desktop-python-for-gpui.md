# Recon: desktop + Python host → GPUI via colibri-sys

**Repo:** `/home/hunter/Projects/surmount/colibri`
**Date:** 2026-08-10
**Mode:** read-only inventory for a GPUI native desktop that embeds via `colibri-sys` (no second REST/RPC face if avoidable).

---

## 1. `desktop/` layout (Tauri)

### What ships today

| Path | Role |
|------|------|
| `/home/hunter/Projects/surmount/colibri/desktop/README.md` | Product intent: thin shell only |
| `desktop/src-tauri/` | Tauri v2 Rust host |
| `desktop/src-tauri/src/main.rs` | `colibri_desktop_lib::run()` |
| `desktop/src-tauri/src/lib.rs` | `tauri::Builder::default().run(...)` only |
| `desktop/src-tauri/tauri.conf.json` | Window + Vite/dist wiring + CSP |
| `desktop/src-tauri/capabilities/default.json` | `core:default` only (no FS/process APIs) |
| `desktop/src-tauri/Cargo.toml` | `tauri` 2.11, **no** `colibri-sys`, no plugins |

There is **no** second frontend under `desktop/`. Dev runs Vite from `../web`; release packages `../../web/dist`.

### Main window

From `tauri.conf.json`:

- **label:** `main`
- **title:** `colibrì`
- **size:** 1280×820, min 860×600, resizable, centered
- **devUrl:** `http://localhost:5173`
- **beforeDevCommand:** `npm --prefix ../web run dev`
- **beforeBuildCommand:** `npm --prefix ../web run build`
- **frontendDist:** `../../web/dist`

### Features (and non-features)

**Does:**

- One native window around the shared React app (`web/`).
- CSP `connect-src` allows `http://127.0.0.1:*` and `http://localhost:*` so the SPA can call a local OpenAI gateway.

**Does not (explicit README + code):**

- Start or supervise the C engine.
- Spawn Python / `coli serve`.
- Download or install models.
- Expose native filesystem or process permissions beyond `core:default`.
- Call `colibri-sys` or any Rust host library.

### How desktop talks to Colibri

**HTTP only, from the webview.**

1. Operator starts inference separately, e.g. `coli serve --model <dir>` (default `127.0.0.1:8000`).
2. UI default base is `http://127.0.0.1:8000/v1` (unless the page is served by the engine gateway itself; see §2).
3. Browser `fetch` → OpenAI-compatible + colibri health/experts/profile endpoints (see §3).

No Tauri `invoke`, no stdin/stdout to the engine, no embedded Python.

**Related product path (not Tauri):** `coli web` in `c/coli` (`cmd_web`) starts the same HTTP stack as `cmd_serve` and optionally opens a browser; static SPA comes from `web/dist` via `openai_server.APIHandler.serve_static`.

---

## 2. Key Python scripts and CLI entrypoints

### Console entry

| Entry | Path | Behavior |
|-------|------|----------|
| pip script `coli` | `pyproject.toml` → `colibri.cli:main` | `colibri/cli.py` → `runpy` on `c/coli` |
| source | `c/coli` | Full subcommand surface |
| engines | `c/colibri`, `c/glm`, `c/inkling`, `c/kimi_k3`, `c/deepseek_v4` | C binaries; selected by model family / `COLI_ENGINE` |

### Subcommands the **desktop user** actually needs

Desktop does not run these itself, but a “faithful” local product still depends on them (or their colibri-sys ports):

| User goal | CLI | Implementation |
|-----------|-----|----------------|
| Load model once, chat + dashboard HTTP | `coli serve` | `cmd_serve` → `openai_server.serve` |
| Browser UI without Tauri | `coli web` | `cmd_web` → same serve + open browser on `/health` |
| One-shot / REPL without HTTP | `coli chat`, `coli run` | Engine stdin protocol (legacy or private serve) |
| Placement | `coli plan` | `resource_plan.build_plan` / `format_plan` |
| Diagnostics | `coli doctor` | `doctor.run_doctor` / `format_doctor` / `exit_code` |
| Status | `coli info` | model/RAM/disk banner |
| Stop serve + engine | `coli stop` | pidfile + `/proc` SERVE=1 hunt |
| Build engine (checkout) | `coli build` | make path |
| Quant convert (optional, heavy) | `coli convert` | Python tools under `c/tools/` |
| Autotune (advanced) | `coli tune` | `autotune.run_tune` |

**Not required for GPUI parity demo:** `bench`, `mirror`, most `tools/*` convert/oracle scripts.

### `c/openai_server.py` (HTTP face of serve)

Core types / functions:

- `Engine` — `Popen` C binary with `SERVE=1`, `SERVE_BATCH=1`, `SNAP`, `NGEN`, `KV_SLOTS`; stdout dispatcher for mux lines (`DATA`, `ACCEPT`, `DONE`, `HWINFO`, `EMAP`, `HITS`, `TIERS`, `PROF`, …).
- `GenerationScheduler` — FIFO admission queue (`--max-queue`, `--queue-timeout`).
- `render_chat` / `render_chat_kimi` / `render_chat_v4` / `render_chat_inkling` — **chat templates live here** (host-owned prompt string for SUBMIT).
- `APIServer` / `APIHandler` — stdlib `ThreadingHTTPServer`.
- `serve(model, host, port, model_id, api_key, ...)` — public entry from `coli serve`.

HTTP surface (what the web UI hits):

| Method | Path | Role |
|--------|------|------|
| GET | `/health` | liveness; full scheduler/tiers/hwinfo if authed (or no key) |
| GET | `/experts` | latest EMAP/HITS (`rows, cols, map, hits, seq`) |
| GET | `/profile` | rolling PROF turns (`seq`, `turns[]`) |
| GET | `/v1/models` | single synthetic model id |
| POST | `/v1/chat/completions` | OpenAI chat; SSE when `stream: true` |
| POST | `/v1/completions` | legacy completions |
| POST | `/v1/messages` | Anthropic Messages rewrite |
| GET | `/*` static | `web/dist` SPA (+ `experts.json` if present) |

Default bind: `127.0.0.1:8000`. Auth: optional `COLI_API_KEY` / `--api-key`. CORS for Vite/Tauri origins by default.

### `c/resource_plan.py`

Important APIs used by `coli` and ported into colibri-sys:

- `analyze_model`, `build_plan`, `environment_for_plan`, `format_plan`
- `discover_gpus`, `physical_cpu_count`, `memory_available`, `ssd_probe_state`, …

### `c/doctor.py`

- `run_doctor(...)` → structured report
- `deep_container_report`, `cuda_linkage`, `missing_shared_libraries`
- `format_doctor`, `exit_code`

### Model install / convert (desktop deferred; host still owns paths)

- Desktop README: model is external, not bundled.
- Python: `coli convert`, `c/download_fp8.py`, `c/tools/download_glm52.py`, HF via optional deps.
- colibri-sys: feature `install` → `model::install::install_model` (HF CLI / `hf-hub`; convert still may shell to Python tools).

---

## 3. Data flows (config → model → generate → visual)

### Architecture today

```
C engine (stdin/stdout mux lines)
    └── openai_server.py Engine dispatcher + HTTP + chat templates + scheduler
            ├── web SPA (Tauri webview or browser)  — fetch REST + SSE
            └── external OpenAI/Anthropic clients
```

No WebSocket. Brain/profile are **poll**, chat is **SSE**.

### Config

Product config is **process environment + model directory**, not a TOML app file (`docs/SETTINGS.md` / `docs/ENVIRONMENT.md`; colibri-sys `ColibriConfig` documents the same).

Typical keys: `COLI_MODEL` / `SNAP`, `COLI_ENGINE`, RAM/VRAM/CUDA tier vars from plan, `KV_SLOTS`, `NGEN`, `SERVE`/`SERVE_BATCH`, optional `COLI_API_KEY`.

**Web UI persistence** (`web/src/lib/storage.ts`):

- localStorage: `colibri.baseUrl`, `colibri.model`
- API key: **memory only**; legacy `colibri.apiKey` stripped

Default base (`App.tsx`):

- If page is not Vite (`port !== 5173`) and is http(s) → `window.location.origin + /v1` (served-by-engine path).
- Else → `http://127.0.0.1:8000/v1`.

### Model pick

1. User sets endpoint + optional API key in sidebar.
2. **Probe server** → `listModels` → `GET {base}/models` (`web/src/lib/api.ts` `listModels`).
3. Selected model id is the **server’s advertised id** (from serve flags / defaults like `glm-5.2-colibri`), not a filesystem path. The path lives only on the server process (`SNAP`).

### Generate (chat)

`streamChat` in `web/src/lib/api.ts`:

- `POST {baseUrl}/chat/completions` with `stream: true`, `stream_options.include_usage`, messages, temperature, `max_completion_tokens`, `enable_thinking`, optional **`cache_slot`** when `supportsCacheSlots(health)` (`web/src/lib/runtime.ts`).
- Body is OpenAI JSON; gateway runs **chat template** (`render_chat*`) then engine **SUBMIT** with rendered prompt.
- SSE: `data: {...}` deltas; `extractSSE`; finish_reason + usage; headers `x-request-id`, `x-colibri-queue-wait-ms`.

Server-side mux (spec: `docs/serve_protocol.md`):

- Handshake: `\x01\x01READY\x01\x01`
- `SUBMIT id slot nbytes max_tokens temp top_p\n<payload>\n`
- Stream: `DATA`, `ACCEPT`, `DONE STAT ...`, telemetry lines
- Multi-slot: conversation sticky via `conversation_cache_slot` / client `cache_slot`

### Visual / telemetry

| UI | Poll | Interval | Backend |
|----|------|----------|---------|
| Sidebar runtime | `GET /health` | 5s | scheduler + optional tiers/hwinfo |
| Brain | `GET /experts` | 1.5s | EMAP hex + HITS hex + seq |
| Profiling | `GET /profile` | 2s (`Profiling.tsx`) | PROF window |
| Atlas hover | `GET /experts.json` | once | static file next to SPA |

Binary packing: EMAP byte = `(tier<<6)|heat`; HITS bitmaps LE — `c/telemetry.h`, mirrored in colibri-sys `visual.rs` and `web/src/Brain.tsx`.

Deeper drift notes (doc vs code): `.agents/reports/explore-visual-telemetry.md` (e.g. HITS timing, missing ENTROPY/GPUS on HTTP path).

---

## 4. colibri-sys: covered vs missing for “faithful embed”

Crate: `/home/hunter/Projects/surmount/colibri/crates/colibri-sys`
Docs: `docs/user-guide.md`, `README.md`, examples `plan_probe`, `embed_chat`.

### Covered (use these for GPUI)

| Capability | Module / API | Python origin |
|------------|--------------|---------------|
| Typed config + env map | `config::ColibriConfig`, `EnvMap`, `Policy` | `coli` env / `environment_for_plan` |
| Machine probe | `probe::MachineInfo::probe*`, GPUs/NPUs/store | `resource_plan` discover + host facts |
| Placement plan | `plan::PlacementPlan::build*`, `environment_for_plan` | `resource_plan.build_plan` |
| Model inspect / family | `model::ModelInfo`, `model_arch`, `ModelFamily` | `coli` / server `model_arch` |
| Model inventory | `ModelRegistry` | host-side (no multi-model DB upstream) |
| Doctor | `doctor::run_doctor`, `exit_code` | `doctor.py` |
| Locate + spawn engine | `engine::locate_engine`, `ServeClient::spawn` | `coli` `engine_for` + `openai_server.Engine` |
| High-level process handle | `EngineHandle::start_with_plan` / `start_blocking`, `generate`, `stop` | supervised serve process |
| Mux client | `GenerateRequest`, `GenerateResult`, `DoneStats` | SUBMIT/DATA/DONE |
| Visual snapshots | `VisualSnapshot`, `expert_map`, `expert_hits`, `tiers`, `hwinfo`, `profile_window` | EMAP/HITS/TIERS/HWINFO/PROF |
| rkyv duplex frames | `stream::{ClientFrame, ServerFrame, encode_frame, decode_frame*}` | designed host↔app protocol (not live engine wire) |
| Async duplex helpers | `DuplexSession`, `duplex_pair` (features `stream`+`tokio`) | tests / custom transport glue |
| Model install (opt) | `model::install` feature `install` | HF download orchestration |

**Process model (important):** inference stays a **C subprocess**. `ffi` feature is a stub (`ffi_available() == false` until Phase D `libcolibri`). GPUI does not get in-process kernels day one.

**`embed_chat` example** is the closest reference binary: plan → `EngineHandle::start_with_plan` → `generate` → print text + tiers.

### Missing or only partial (faithful desktop still owes)

| Gap | Why it matters | Today’s owner |
|-----|----------------|---------------|
| **Chat templates** | Mux payload is a **rendered** prompt, not OpenAI messages | `openai_server.render_chat*` — **no** Rust port in colibri-sys |
| **OpenAI/Anthropic HTTP + SSE** | Web/Tauri clients depend on it | `openai_server` only; sys is not an HTTP server |
| **GenerationScheduler / queue / 429** | Concurrent HTTP + capacity UX | Python only; EngineHandle generates against mux (serialize / `kv_slots`) |
| **Token streaming callback** | UI wants progressive tokens | `ServeClient` events exist; `EngineHandle::generate` is **blocking full result** |
| **Stop sequences / tools / thinking split** | Product chat quality | Python gateway |
| **Inkling audio / multimodal** | Optional family features | Python only |
| **Static SPA + CORS + Host allowlist** | `coli web` / Tauri remote | Python / Tauri shell |
| **Live ServerFrame push from engine** | User-guide: host glue maps snapshots → frames | rkyv is codec + session, not auto-bridged to mux |
| **Multi-model “served id” catalog** | UI model dropdown | Single model id on Python serve; registry is filesystem roots |
| **convert / quant pipeline** | First-time model prep | Python tools; install may shell out |

“Faithful embed” for **native GPUI** means: **skip HTTP** and own the host responsibilities Python currently owns (template, queue policy, streaming UI), while reusing sys for probe/plan/doctor/spawn/mux/visual.

---

## 5. Recommendation: minimum GPUI surface for -sys parity (no REST/RPC)

Goal: prove colibri-sys can replace the **desktop-relevant** host path without `openai_server` and without inventing a second network protocol. Prefer **in-process calls** to `EngineHandle` + optional **rkyv duplex** only if you want a clean actor boundary inside the app (UI thread ↔ host actor).

### Screens / actions (MVP)

| Screen | Actions | colibri-sys API |
|--------|---------|-----------------|
| **1. Machine** | Probe hardware + model store free space | `MachineInfo::probe` / `probe_for_config` |
| **2. Models** | List roots, inspect path, optional install | `ModelRegistry`, `ModelInfo::inspect`, feature `install` |
| **3. Plan + doctor** | Build plan, show tiers, run doctor before start | `PlacementPlan::build_from_info`, `run_doctor` |
| **4. Engine** | Start / stop one supervised process | `ColibriConfig` + `EngineHandle::start_with_plan` / `stop` |
| **5. Chat** | Multi-turn text; show tok/s, cache hit, RSS from DONE | **Template in GPUI or small Rust helper** → `generate` / streaming over `ServeClient` events |
| **6. Runtime strip** | Cores/RAM/GPU + tier counts | `hwinfo()`, `tiers()` after turn / poll pump |
| **7. Brain** | Grid from EMAP + pulse from HITS | `expert_map()`, `expert_hits()` (same packing as `Brain.tsx`) |
| **8. Profiling** | Sparkline / table of PROF turns | `profile_window()` |

Defer for later (not needed to prove -sys parity): Anthropic bridge, tool calling, CORS, static file server, Tauri packaging, HF convert.

### Internal wiring (no REST)

```
GPUI UI thread
    │  messages / draw commands
    ▼
Host actor (std thread or tokio)
    ├── MachineInfo / PlacementPlan / Doctor / Registry
    ├── EngineHandle  ──spawn──► C engine (SERVE mux stdin/stdout)
    │       generate / cancel via ServeClient
    │       visual_snapshot after turns
    └── optional: DuplexSession
            ClientFrame::Submit/Stop/Subscribe
            ServerFrame::Token/Done/ExpertMap/...
            (encode from handle events; UI only speaks frames)
```

**Do not** reimplement mux line parsing outside colibri-sys.
**Do** implement (thin) chat templating outside or next to the UI until ported.
**Do** keep one long-lived `EngineHandle` (cold start is expensive); serialize generates when `kv_slots == 1`.

### Acceptance checks (parity without HTTP)

1. Probe shows RAM/GPU/store consistent with `coli plan` / machine facts.
2. Plan env applied → engine READY.
3. Doctor report exit semantics match `doctor.exit_code`.
4. One generate on a tiny model (`c/glm_tiny` or env `COLIBRI_TEST_*`) returns text + `DoneStats`.
5. After a turn, Brain packing matches `ExpertMap::tier_at` / heat and web hex decode rules.
6. Profile window non-empty when engine emits PROF.
7. Optional: round-trip `ClientFrame`/`ServerFrame` through `encode_frame` / `decode_frame_checked` in the host actor (proves duplex path without network).

### Explicit non-goals for this MVP

- Replacing Tauri packaging or the React SPA 1:1.
- Shipping OpenAI-compatible port 8000 from GPUI.
- Linking `libcolibri` FFI (`docs/ffi-phase-d.md` is design-only).
- Bundling 744B weights.

---

## 6. Concrete file index (quick nav)

| Concern | Paths |
|---------|--------|
| Tauri shell | `desktop/src-tauri/src/lib.rs`, `tauri.conf.json` |
| SPA shell | `web/src/App.tsx`, `Brain.tsx`, `Profiling.tsx` |
| SPA HTTP client | `web/src/lib/api.ts`, `storage.ts`, `runtime.ts` |
| CLI | `c/coli` (`cmd_serve`, `cmd_web`, `cmd_doctor`, `cmd_plan`, …) |
| HTTP + mux host | `c/openai_server.py` (`Engine`, `APIHandler`, `serve`) |
| Plan | `c/resource_plan.py` |
| Doctor | `c/doctor.py` |
| Mux spec | `docs/serve_protocol.md`, `docs/api.md` |
| Rust host | `crates/colibri-sys/src/{lib,config,probe,plan,doctor,visual}.rs` |
| Engine embed | `crates/colibri-sys/src/engine/{mod,serve,locate}.rs` |
| Duplex | `crates/colibri-sys/src/stream/{frame,session,codec}.rs` |
| Example embed | `crates/colibri-sys/examples/embed_chat.rs` |
| Prior visual recon | `.agents/reports/explore-visual-telemetry.md` |

---

## 7. Bottom line

- **Today’s desktop** is a **webview around the React OpenAI client**. All real product behavior for inference is **outside** Tauri: Python `coli serve` + C engine + mux protocol.
- **colibri-sys already owns** the hard host pieces (probe, plan, doctor, spawn, mux client, visual types, optional install, rkyv frame codec).
- **Biggest hole for faithful native chat:** chat templates and progressive streaming UI policy still sit in Python; HTTP is optional once GPUI calls `EngineHandle` directly.
- **Minimum GPUI proof** is eight surfaces above: machine → models → plan/doctor → engine lifecycle → chat → health strip → brain → profiling, all through colibri-sys, zero REST.
