# Recon: four residual gaps (Brain/PROF/tiers, HF install, stop/cancel, FFI Phase D)

**Date:** 2026-08-10
**Repo:** `/home/hunter/Projects/surmount/colibri`
**Mode:** read-only recon for planning. No product code edits.

Sources: `crates/colibri-sys`, `crates/colibri-desktop-gpui`, `web/`, `docs/serve_protocol.md`, `crates/colibri-sys/docs/ffi-phase-d.md`, fidelity matrix, prior explores.

---

## Shared architecture (so the four items make sense)

```
GPUI / embed binary
  └── colibri-sys (Rust, in-process)
        ├── probe / plan / doctor / registry / optional install
        ├── EngineDuplex (rkyv ClientFrame ↔ ServerFrame)   [app ↔ host]
        └── ServeClient (line protocol on engine stdin/stdout)
              └── C engine process (colibri / inkling / …, SERVE_BATCH=1)
```

| Layer | What it is | What it is not |
|-------|------------|----------------|
| **colibri-sys** | Host crate: config, plan, spawn, mux client, visual types, rkyv duplex | Not the CUDA kernels |
| **EngineDuplex + rkyv** | Typed frames between UI code and the host crate | Not REST, not gRPC, not true FFI |
| **Serve mux** | Text lines: `SUBMIT` / `DATA` / `DONE` / `STOP` / `CANCEL` / `EMAP` / `PROF` / … | Same wire the Python gateway uses |
| **Python `openai_server.py` + `web/`** | HTTP OpenAI face + SPA dashboard | Not required when embedding colibri-sys |
| **libcolibri Phase D** | Future **in-process** C library (no subprocess) | Not the same as process mux + rkyv |

Fidelity status for the native shell:
`/home/hunter/Projects/surmount/colibri/crates/colibri-desktop-gpui/docs/fidelity.md`

---

## 1. Brain / PROF / live tiers UI

### What each term means (plain English)

**Brain**
A live “cortex” view of a **Mixture-of-Experts (MoE)** model. Each sparse expert is one cell on a grid (e.g. ~76 layers × 256 experts for large GLM-class models, thousands of cells). The product pitch in `README.md` / `docs/api.md`: color = **storage tier**, brightness = **how hot** that expert has been (routing heat), and experts used on the latest turn **flash** then fade.

- **Expert map (EMAP):** for every expert, one byte: high 2 bits = tier (0 disk / 1 RAM / 2 VRAM), low 6 bits = heat bucket. Packed as hex on the wire, decoded to `ExpertMap.cells`.
- **Expert hits (HITS):** a **bitmap** of which experts were actually routed since the last hits emit, plus a **sequence number** so the UI can pulse only when the set changes.

**PROF (profiling turns)**
After a generation turn finishes, the engine emits a **PROF** line: wall time, prompt/completion token counts, and how many seconds were spent in expert disk I/O, expert wait, expert matmul, attention, LM head, plus forward count. The host keeps a rolling window (~120 turns). The web **Profiling** tab charts those phases (stacked bars, tok/s). Note: `docs/serve_protocol.md` still mentions a `PERF` line name; the live path and Rust parser use **`PROF`**.

**Tiers (placement tiers)**
Where expert weights live so the model fits:

| Tier | Meaning |
|------|---------|
| **VRAM** | On GPU memory (fastest) |
| **RAM** | In system memory (hot cache) |
| **Disk** | Cold on SSD/HDD (slow misses) |

Two related but different snapshots:

1. **Placement plan tiers** (`PlacementPlan` / `PlanTiers` in `plan.rs`): **before** start, “how we intend to pin budget.”
2. **Live TIERS** (`TiersSnap` from engine `TIERS` lines): **right now**, expert counts in VRAM/RAM/disk and resident GB.

The web sidebar “tier bar” is the **live** counts from `/health`. GPUI today only shows plan summary numbers, not a live bar.

### Why it matters

Operators use Brain and tiers to see whether the machine is thrashing on disk experts, whether re-pin is working, and whether generation is I/O-bound vs compute-bound (PROF). Without them, native GPUI is “chat only” and loses the product’s signature observability.

### What already exists

**colibri-sys visual + engine APIs**

| Type / API | Path | Role |
|------------|------|------|
| `ExpertMap`, `ExpertHits`, `ProfileTurn`, `TiersSnap`, `HwinfoSnap` | `crates/colibri-sys/src/visual.rs` | Typed snapshots; packing matches `c/telemetry.h` |
| `VisualSnapshot` + `absorb_from_client` | same | Aggregate after generate / pump |
| `Subscribe` bitset (`VISUAL`, `TOKENS`, `PROFILE`, `HW`, …) | same | Interest mask for duplex emits |
| `ServerFrame::{ExpertMap, ExpertHits, ProfTurn, Tiers, Hwinfo}` | `crates/colibri-sys/src/stream/frame.rs` | rkyv wire to UI |
| `ClientFrame::Subscribe` | same | Ask duplex to emit visual snapshot |
| `EngineDuplex::pump_visual` / `emit_visual_snapshot` | `crates/colibri-sys/src/engine/duplex.rs` | Pull telemetry → frames |
| `ServeClient` dispatcher parses `HWINFO` / `TIERS` / `EMAP` / `HITS` / `PROF` | `crates/colibri-sys/src/engine/serve.rs` | Fills telemetry from stdout |
| `EngineHandle::{tiers, expert_map, expert_hits, profile_window, pump_visual}` | `crates/colibri-sys/src/engine/mod.rs` | Host convenience getters |

**Web (reference implementation, HTTP poll)**

| UI | File | How it gets data |
|----|------|------------------|
| Brain canvas | `web/src/Brain.tsx` | Poll `GET /experts` every 1.5s; hex map + hits; optional static `/experts.json` atlas |
| Profiling charts | `web/src/Profiling.tsx` | Poll `GET /profile` (~2s) |
| Tier bar + HW | `web/src/App.tsx` | Poll `GET /health` |
| Types | `web/src/lib/api.ts` | `ProfileTurn`, health tiers/hwinfo |

Python gateway + poll architecture: prior note
`.agents/reports/explore-visual-telemetry.md`
Wire: `docs/serve_protocol.md`.

**GPUI today** (`crates/colibri-desktop-gpui/`)

| Surface | Status |
|---------|--------|
| Chat via `EngineDuplex` + tokens | done |
| Machine probe / doctor / plan text | done |
| Live Brain / EMAP / HITS panel | **missing** (API ready) |
| Profiling / PROF charts | **missing** (API ready) |
| Live TIERS bar | **partial** (plan text only; not `ServerFrame::Tiers` graph) |
| Live HWINFO strip from engine | **partial** (static probe, not engine HWINFO) |

`host.rs` only maps `Token` / `Done` / `Error` from generate; it never calls `pump_visual` or handles visual frames. Cargo features: `runtime`, `stream`, `tokio` only (no install).

### What’s missing

1. **GPUI panels** (or a minimal strip): Brain grid, tier bar, PROF history. Port behavior from `web/src/Brain.tsx` and `Profiling.tsx`, but feed from `ServerFrame` / `EngineHandle` instead of HTTP.
2. **Background visual pump** while idle and mid-turn: timer or thread that calls `duplex.pump_visual()` / `ClientFrame::Subscribe` and updates UI state. Today telemetry is mainly refreshed after a generate completes (`absorb_from_client` in `EngineHandle::generate_stream`).
3. **Lock / session design:** generate currently **takes** `EngineSession` out of the mutex for the whole stream (`host.rs` `generate_async`). A visual pump needs concurrent access to the same client (split lock or shared `ServeClient` telemetry read path) or must pump only between turns.
4. Optional: expert atlas (`experts.json`) hover data; web loads it statically; not required for first native Brain.

### Effort / risk

| | Estimate |
|--|----------|
| **Effort** | Medium–large UI: 2–5 focused implement slices (tier bar + PROF first; Brain canvas last). Sys API mostly green. |
| **Risk** | Medium. Canvas/perf for 10k+ cells in GPUI; concurrent generate + pump; packing must stay aligned with `c/telemetry.h`. Low protocol risk (already parsed). |
| **Value** | High product differentiation for native desktop. |

---

## 2. HF install picker

### What it is

A desktop UI to **download a model** from Hugging Face (or register a local folder) into the model store, with free-space check and progress, then point the engine at that directory.

### Why it matters

Today GPUI expects a **model path** (`COLIBRI_MODEL` / field). First-run users without a pre-downloaded tree need a CLI or manual `hf download`. Install is the gap between “I have a machine” and “I can chat.”

### What already exists

**colibri-sys feature `install`** (optional, off by default)

| Piece | Path |
|-------|------|
| `InstallSource::{HuggingFace, LocalPath}` | `crates/colibri-sys/src/model/install.rs` |
| `InstallOptions` (`dest`, `prefer_cli`, `min_free_bytes`, `inspect_after`, `register`) | same |
| `InstallProgress` (phase, message, bytes, file index) | same |
| `install_model` / `install_model_with` | same |
| `ensure_space` via `disk_free_bytes` | same + `probe.rs` |
| Prefer **`hf` CLI** on PATH (`SystemHfCli`); else **hf-hub 1.x** blocking (`list_tree` + `download_file`) | same; ported per `.agents/reports/impl-hf-hub-1.md` |
| Incomplete download detection; optional `ModelInfo::inspect` | same |
| `ModelRegistry` | `crates/colibri-sys/src/model/registry.rs` |
| Docs | `crates/colibri-sys/docs/user-guide.md` §9 |

Cargo: `install = ["dep:hf-hub", "dep:indicatif"]` in `crates/colibri-sys/Cargo.toml`.

**Not linked** into GPUI:
`colibri-desktop-gpui/Cargo.toml` uses `features = ["runtime", "stream", "tokio"]` only. Fidelity: install + registry **missing**.

### What a desktop picker needs

| UI field / behavior | Backed by |
|---------------------|-----------|
| Repo id (`org/name`) | `InstallSource::HuggingFace.repo_id` |
| Optional revision / allow patterns | same |
| Destination under model store | `InstallOptions.dest`; store root from probe / `COLIBRI_MODEL_STORE` |
| Free space gate | `min_free_bytes` + `MachineInfo.model_store.free_bytes` |
| Progress bar / log | `InstallProgress` callback → channel → UI |
| Cancel? | **Not first-class** in install API today (CLI/hf-hub run to completion or fail) |
| After success | set model path, optional `ModelRegistry` register, doctor/plan, Start engine |
| Prefer-cli vs library | keep default `prefer_cli: true` (operators with `hf` get better progress UX from CLI; pure-Rust fallback for machines without it) |

### Prefer-cli vs hf-hub 1.x (honest)

| Path | Pros | Cons |
|------|------|------|
| **`hf` CLI** | Mature progress, auth tokens, operator-familiar | Requires install on PATH; harder to unit-test |
| **hf-hub 1.x** | In-tree dependency, no external binary | Sync blocking; progress is host-driven; still needs network/auth env |

Recommendation: **keep prefer-cli default**; enable `install` feature on the desktop binary; do not re-litigate the 1.x port.

### What’s missing

1. Enable `install` on `colibri-desktop-gpui` (and re-export or thin host wrapper).
2. Modal/form: repo id, dest, free-space display, progress, error notes.
3. Background thread for `install_model` (blocking API).
4. Registry scan UI for already-downloaded models (fidelity: missing).
5. Optional cancel/kill of download process (new sys work if required).

### Effort / risk

| | Estimate |
|--|----------|
| **Effort** | Small–medium if sys install stays as-is (1–2 slices for MVP form + progress). |
| **Risk** | Medium operational: network, HF auth, multi-shard disk, incomplete partials. Low API risk (tested with feature `install`). |
| **Value** | High for first-run; does not block chat if path is pre-set. |

---

## 3. Stop / cancel mid-generate UI

### What it is

While tokens stream, the user presses **Stop** so generation ends without waiting for `max_tokens`. Related: **Cancel** when the client goes away (discard path vs graceful stop).

### Protocol (engine serve mux)

From `docs/serve_protocol.md`:

| Command | Intent |
|---------|--------|
| `STOP <id>` | End generation through the normal successful **DONE** path (stats + KV kept). Used when a stop sequence matches. |
| `CANCEL <id>` | Abort; engine returns `ERROR <id> CANCELLED`. Used on client disconnect. |

Python gateway (`c/openai_server.py`) sends **STOP** when a stop-sequence filter trips, and **CANCEL** when the HTTP client disconnects.

### What colibri-sys has today

| Piece | Behavior |
|-------|----------|
| `ClientFrame::Stop { req_id }` / `Cancel { req_id }` | Defined in `stream/frame.rs` |
| `EngineDuplex::handle` for both | **Both** call `ServeClient::stop_request` only → writes **`STOP {id}`** only (`duplex.rs` ~122–124). **Cancel is not distinct.** |
| `ServeClient::stop_request` | Only STOP; **no `cancel_request`** (`serve.rs` ~318–324) |
| `ServeClient::generate_stream` | Allocates **its own** mux id from `next_id` (starts at 1); does **not** use `ClientFrame`’s `req_id` on the `SUBMIT` line |
| Engine request id vs UI `req_id` | Server frames stamp the **UI** `req_id`; STOP is sent with that same number. Works only if UI ids stay in lockstep with mux `next_id` (both start at 1). Accidental, not mapped. |

**Concurrency gap (important):**

- `EngineHandle::generate_stream` holds `inner` **Mutex for the entire generation**.
- `with_client` (used by Stop) needs the **same** lock → **cannot stop from another thread** while generate holds it.
- `ServeClient` itself could write STOP on `stdin` while the dispatcher thread feeds `DATA` (stdin is a separate mutex), but the **handle-level lock** blocks that path.
- GPUI `generate_async` **moves** `EngineSession` out of the slot for the whole job → UI has **no live session** to send Stop to.

**Web today:** stop button aborts the **HTTP fetch** (`AbortController` in `web/src/App.tsx`). Gateway maps disconnect → mux `CANCEL`. That path does not exist in GPUI.

### What the UI needs

1. **Stop button** while `generating == true` (mirror web’s destructive stop control).
2. **Shared generate session** that still accepts control frames: e.g. keep `Arc<EngineSession>` or a stop channel that holds a clone of stdin/stop capability without taking the generate lock.
3. **Sys fixes before or with UI:**
   - Map `ClientFrame` `req_id` ↔ mux SUBMIT id (or pass UI id into SUBMIT if the engine accepts that id).
   - Implement `cancel_request` → `CANCEL`, and decide Stop vs Cancel semantics for the button (product: Stop ≈ graceful DONE is usually better for chat).
   - Unlock stop during generate (split locks: do not hold `EngineHandle` mutex across `rx.recv()`).
4. Wire GPUI: on stop, send `ClientFrame::Stop { req_id }` (or direct `stop_request` with **mux** id), clear generating flag on Done/Error.

### What’s missing

- UI button and control path.
- Real cancel line.
- Concurrent stop during generate.
- Explicit id mapping.
- Tests that STOP mid-stream yields Done (or Cancelled) without deadlock.

### Effort / risk

| | Estimate |
|--|----------|
| **Effort** | Small UI once sys is fixed; **sys lock + id mapping is the real work** (1–2 slices). |
| **Risk** | Medium: deadlocks, wrong id → silent no-op, half-written STOP races. Engine protocol itself is mature. |
| **Value** | High everyday UX; web already trains users to expect it. |

---

## 4. libcolibri true FFI (Phase D)

### Why engines are processes with `main` today

C engines (`c/colibri.c`, `inkling.c`, `kimi_k3.c`, `deepseek_v4.c`) are **CLI programs**: process lifetime, signals, GPU device ownership, env knobs, and optional OpenMP re-exec. Host recovery is simple: kill the child, spawn again. That is intentional product architecture for now.

colibri-sys documents this clearly: host configures/spawns/streams; **kernels stay in C binaries**. Feature `ffi` is a **stub** (`ffi_available() == false`). Design only:
`/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/ffi-phase-d.md`

### What true in-process embedding would require

From the Phase D spike (proposed, **not** operator-accepted architecture):

| Requirement | Why |
|-------------|-----|
| **No-`main` library target** (`COLIBRI_NO_MAIN=1` or `libcolibri`) | Host supplies process entry; engine is a library |
| **Stable C ABI** | `coli_model_open` / `session_generate` / `coli_cancel` / optional `coli_visual_poll` (names illustrative) |
| **All families or a staged set** | V4 has experimental shapes in `c/deepseek_v4.h`; not a full product ABI |
| **Device / thread ownership model** | CUDA/Metal/Vulkan assume process-wide control |
| **Kill-switches** | `COLIBRI_FORCE_PROCESS=1`, config prefer-process, init-failure fallback to subprocess |
| **Crash isolation tradeoff** | In-process fault can take down the host; process embed cannot |
| **CI golden generate** | e.g. `c/glm_tiny` matches process-embed quality doctrine |

Acceptance checklist is in `ffi-phase-d.md` § Acceptance criteria. Stub module: `crates/colibri-sys/src/lib.rs` `pub mod ffi`.

### Honest relationship to process serve mux + rkyv

| Mechanism | In-process? | Role |
|-----------|-------------|------|
| **rkyv EngineDuplex** | Frames live in the **Rust host process** | App ↔ host control plane |
| **ServeClient + C subprocess** | Engine is **out-of-process** | Actual inference |
| **True libcolibri FFI** | Engine would be **in-process** | Different product path |

Ship native desktop **without** Phase D. Duplex + mux is the supported embed path. Claiming “in-process engine” today is wrong; claiming “in-process **host**” is correct.

### What’s missing

Everything for a real link: no-main build, ABI, kill-switch tests, `build.rs` search, operator acceptance. Large C refactor; **out of scope** for ordinary GPUI residual.

### Effort / risk

| | Estimate |
|--|----------|
| **Effort** | Very large (multi-week C + build + ABI + multi-backend). Design spike only is done. |
| **Risk** | High: GPU init, undefined behavior surface, crash domain, multi-family drift. |
| **Value** | Latency/control for advanced embeds; **low operator value** vs Brain/stop/install for desktop residual. |

---

## Recommended implementation order

| Order | Item | Why first / later |
|-------|------|-------------------|
| **1** | **Stop / cancel (sys + UI)** | Smallest high-value UX; chat is already live; unblocks trust. Fix **lock + id + STOP path** in colibri-sys, then one GPUI button. Prefer Stop→DONE semantics for chat; add real CANCEL later if needed for multi-client. |
| **2** | **Live tiers + PROF strip** | Sys frames already exist; no canvas. Shows placement health and bottlenecks next to chat. Reuses `pump_visual` once session locking is fixed for stop (same concurrency design). |
| **3** | **Brain panel** | Same data plane as (2); heavier GPUI/canvas work and atlas optional. Do after visual pump is reliable between/during turns. |
| **4** | **HF install picker** | Independent of engine frames; enable `install` + form. Can parallelize with (2)–(3) if two tracks, but **after** stop if only one writer on the desktop crate. Slight dependency: model store path / free space already on probe panel. |
| **5** | **Phase D FFI** | Last / separate campaign. No dependency from 1–4; process mux remains the product path. Do not block desktop residual on libcolibri. |

### Dependency sketch

```
[Stop: unlock generate + mux id mapping]
        │
        ├─► Stop button (GPUI)
        │
        └─► Concurrent pump_visual
                │
                ├─► Tiers + PROF UI
                └─► Brain UI

[HF install] ── parallel after stop, or anytime if separate binary feature work

[Phase D] ── independent, defer
```

### Risk ordering

1. Stop concurrency/id bugs (correctness).
2. Visual pump vs generate lock (same root as stop).
3. Install network/disk (ops).
4. Brain rendering scale (polish).
5. FFI (strategic, high cost).

---

## File index (absolute paths)

| Topic | Paths |
|-------|--------|
| Visual types | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/visual.rs` |
| Frames | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/stream/frame.rs` |
| Duplex | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/engine/duplex.rs` |
| Serve mux client | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/engine/serve.rs` |
| Engine handle | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/engine/mod.rs` |
| Install | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/model/install.rs` |
| FFI design | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/ffi-phase-d.md` |
| Protocol | `/home/hunter/Projects/surmount/colibri/docs/serve_protocol.md` |
| Fidelity | `/home/hunter/Projects/surmount/colibri/crates/colibri-desktop-gpui/docs/fidelity.md` |
| GPUI host | `/home/hunter/Projects/surmount/colibri/crates/colibri-desktop-gpui/src/host.rs` |
| Web Brain / Profiling | `/home/hunter/Projects/surmount/colibri/web/src/Brain.tsx`, `Profiling.tsx`, `App.tsx` |
| Placement tiers | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/plan.rs` |

---

## Bottom line

- **Sys already speaks Brain, PROF, and live tiers** over the process serve mux and rkyv; **GPUI does not paint them yet**.
- **HF install is a finished optional library feature**; desktop needs the feature flag and a form.
- **Stop frames exist on paper but do not work under load** (handle lock, id coupling, Cancel=STOP). Fix host concurrency, then add the button.
- **True libcolibri is a different product phase**; process embed + duplex is the residual path for 1–4.
