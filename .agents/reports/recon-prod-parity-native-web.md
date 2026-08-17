# Recon: production parity (web / Tauri SPA ↔ colibri-native ↔ colibri-sys)

**Date:** 2026-08-10
**Repo:** `/home/hunter/Projects/surmount/colibri`
**Mode:** read-only recon for production parity planning. No product edits.
**Write path:** `.agents/reports/recon-prod-parity-native-web.md`

**Sources:** `web/src/*`, `desktop/src-tauri/*`, `crates/colibri-native/*`, `crates/colibri-sys/*`, `crates/colibri-native/docs/fidelity.md`, `.agents/RESIDUAL.md`, prior recon `recon-plan-four-gaps.md`.

---

## Plain architecture (read once)

Three product-ish shells exist. They are not the same stack.

| Path | What it is | How it talks to the engine |
|------|------------|----------------------------|
| **`web/`** | React/Vite SPA: Chat, Brain, Profiling + sidebar | HTTP OpenAI-compatible API (`/v1/chat/completions` SSE) + `/health`, `/profile`, `/experts` |
| **`desktop/`** | Thin Tauri v2 shell | Packages `web/` in a window. Does **not** start the engine, download models, or link Rust host code. Connects to whatever URL the user probes. |
| **`crates/colibri-native`** | GPUI native window (fidelity demo) | Links **colibri-sys in-process**. Spawns the **C engine as a subprocess** over the serve mux (stdin/stdout). App↔host frames are rkyv `ClientFrame` / `ServerFrame` via `EngineDuplex`. **No REST.** |
| **`crates/colibri-sys`** | Embeddable Rust host library | Probe, plan, doctor, registry, chat templates, optional HF install, mux client, visual snapshots. Inference kernels stay in C binaries. |

**Host in-process** means the Rust host library is linked into the GPUI binary.
**Engine process** means decode still runs in a separate C binary (`colibri` / `inkling` / …).
**Not true FFI:** `ffi_available() == false`; Phase D `libcolibri` is design-only.

```
web (React) ──HTTP──► Python openai_server ──mux──► C engine
desktop (Tauri) ──loads web──► same HTTP path

colibri-native (GPUI)
  └── colibri-sys (in-process)
        └── EngineDuplex (rkyv) → ServeClient → C engine process
```

Parity planning must pick a target: **embed path product** (native + sys, no HTTP) vs **full SPA chrome in GPUI** (pixel/feature clone of `web/`). Fidelity docs and residual already mark full SPA parity as strategic / out of scope for the demo.

---

## 1. Web / Tauri product surface

### 1.1 Layout and navigation

**Files:** `web/src/App.tsx`, `web/src/Brain.tsx`, `web/src/Profiling.tsx`, `web/src/main.tsx`, `web/src/i18n/*`, `web/src/lib/{api,runtime,storage}.ts`
**Shell:** `desktop/src-tauri` is a minimal Tauri app (`lib.rs` only runs the builder). Window defaults: 1280×820, CSP allows localhost HTTP to the engine. No native FS/process permissions, no engine lifecycle.

**Views (top tabs):**

| View | Role |
|------|------|
| **Chat** | Multi-turn stream; empty-state hero + suggested prompts; composer Send / Stop |
| **Brain** | Full expert cortex canvas (MoE map) |
| **Profiling** | Charts of per-turn wall-time phases |

**Sidebar (not a separate “Settings” route; settings live here):**

- **Connection:** API endpoint, optional API key (memory only), Probe server
- **Runtime:** live HW strip (CPU/GPU/RAM/cores from `/health`), scheduler dashboard (active/queued/completed/failures), **tier bar** (VRAM/RAM/disk expert counts + GB), session token totals
- **Inference:** model select (from `GET /models`), **KV session / cache_slot** when `kv_slots > 1`, temperature, max tokens, **Reasoning** toggle (`enable_thinking`)
- **Locale switcher** + transport footnote (“OpenAI-compatible transport”)

**Top bar:** active model, live token / tok/s / TTFT badges, usage, truncated warning, queue wait, current slot, Clear conversation.

There is no separate settings page, install UI, doctor, placement plan, or machine probe beyond what `/health` returns after connect.

### 1.2 Connect → model → engine flow (web)

1. User sets **base URL** (default `http://127.0.0.1:8000/v1`, or same-origin `/v1` when served by the engine port).
2. Optional **API key** (not persisted; legacy key purged from storage).
3. **Probe server** → `listModels` + optional `getHealth` → `connected`.
4. When the page is served by the engine (not Vite `:5173`), auto-connect runs once.
5. Chat: `streamChat` → `POST …/chat/completions` with SSE deltas; `cache_slot` only if health advertised `kv_slots`; AbortController stops generation (HTTP abort, not mux STOP from the browser).
6. Brain polls `GET /experts` (~1.5s); Profiling polls `GET /profile` (~2s); health polls every 5s while connected.

**Engine lifecycle is external.** Web/Tauri never spawn the C binary, never run `coli plan` / doctor, never install models. Operator starts `coli serve` / OpenAI gateway separately (hundreds of GB models stay user-owned).

### 1.3 Chat, Brain, Profiling (web depth)

**Chat**

- Roles: system (template only via server), user, assistant.
- Multi-conversation keyed by **cache slot** (`conversations[slot]`).
- Streaming deltas, stop via abort, finish_reason / usage / queue-wait headers.
- Controls: temperature 0–2, max tokens 1–32768, reasoning toggle.

**Brain** (`Brain.tsx`)

- Full-resolution canvas (cell size from container).
- Tier color + heat brightness (`min(heat/24, 1)`).
- Hits pulse with **RAF multi-frame decay** (`*= 0.94`).
- Hover tooltips; optional static **atlas** from `/experts.json` (affinity / specialist labels / layer depth roles).
- Layer role copy (early / middle / late / MTP) via i18n.

**Profiling** (`Profiling.tsx`)

- Last ~40 turns; share bars + stacked phase columns + tok/s columns.
- Phases: I/O wait, expert matmul, attention, LM head, other.
- Disk service note (overlap with compute).

### 1.4 i18n tone

**Locales:** English, Simplified Chinese, Traditional Chinese, Italian (`web/src/i18n/{en,zh-CN,zh-TW,it}.ts`).

**Tone:** product marketing + operator clarity mixed:

- Brand: “local giant, tiny footprint”; hero “Ask the giant. Keep the machine yours.”
- UI copy is plain for connection/runtime; Brain is educational (layer depth explanations, specialist/generalist).
- Keys cover nav, sidebar, topbar, chat, brain, profiling, error boundary.

**Native GPUI has no i18n** (English hard-coded labels).

### 1.5 Tauri vs “native product”

| | Tauri + web | colibri-native |
|--|-------------|----------------|
| UI chrome | Full SPA | Lab panels + chat |
| Start engine | No | Yes (sys spawn) |
| Install model | No | Yes (`install` feature) |
| Doctor / plan | No (server-side only if CLI used) | Yes (sys APIs) |
| Auth / HTTP | Optional API key to gateway | N/A |
| Positioning (repo docs) | Product shell for SPA | Fidelity / embed proof path |

---

## 2. colibri-native current surface

**Package:** `crates/colibri-native`
**Entry:** `src/main.rs` (GPUI `DesktopApp`) + `src/host.rs` (sys glue) + `src/text_input.rs`
**Self-description:** fidelity demo, not a pixel clone of the React SPA (`README.md`, honesty strip).

### 2.1 Window chrome and demo honesty

- Title: **colibrì (native)** + engine label + status string.
- **Honesty strip** (constant):
  `Host: colibri-sys in-process · Engine: serve mux process · Frames: rkyv · Not REST · FFI: no`
- **Live tiers strip** under honesty (VRAM/RAM/disk expert counts from visual pump).
- **System messages** in the chat log (role `system`, amber label): bootstrap copy, “Engine started…”, start failures, install / generate status notes. Not a user-editable system prompt UI; host injects a fixed system ChatMessage in `messages_from_turns`.

This honesty/lab chrome is intentional and should stay visible until the product is no longer a demo, or be behind a “developer” toggle if production-native ships.

### 2.2 Panels (what you see)

Left column (~440px) stacked:

| Panel | Behavior |
|-------|----------|
| **Machine** | Probe on bootstrap; RAM/swap/cores/SIMD/store free/GPU·NPU; Re-probe |
| **Doctor** | **Shallow** only (`deep: false`); auto + Run doctor |
| **Plan / model** | Model path field, Plan button, Start engine, plan summary text |
| **Profiling** | Text strip of last N PROF turns (wall, tok, tok/s, phase times) — not charts |
| **Brain** | Tier/heat cells + hits pulse; **stride-sampled ≤2048 cells**; no tooltips/atlas |
| **HF install** (feature default on) | repo / revision / dest; free space; progress; prefer-cli; **no cancel** |

Right: **Chat** message list + input + **Send** + **Stop**.

### 2.3 Product vs lab

| Product-shaped (keep for real users) | Lab / demo-shaped (strip or hide for production) |
|--------------------------------------|--------------------------------------------------|
| Machine inventory | Honesty strip + “FFI: no” product line |
| Shallow doctor (at least) | Dump-style multi-line probe/doctor text panels |
| Placement plan summary before start | System messages explaining rkyv/mux internals |
| Start engine from model path | Hard-coded system prompt string |
| Chat stream + Stop | Fixed temp/max_tokens in host (not full sidebar controls) |
| Live tiers + usable Brain MVP | Brain cap 2048 + no atlas (acceptable MVP, not lab-only) |
| HF install into model store | Install form with no cancel (lab friction) |
| PROF summary | Text-only PROF vs SPA charts |

Native **does more host lifecycle** than web (probe/plan/doctor/start/install). Web **does more polish UX** (i18n, charts, full brain atlas, multi-slot UI, inference sliders, scheduler dashboard).

### 2.4 Fixed / partial behaviors (important for parity)

- Chat uses `render_chat` / templates; tools and Inkling audio **not** ported.
- Generate via `EngineDuplex`; **always slot 0**.
- Stop → mux `STOP` with active `req_id` (done; residual closed).
- Visual pump ~500ms while engine up.
- Install: path sandbox under model store; progress channel; no mid-download cancel.
- No locale, no API key, no REST gateway.

---

## 3. colibri-sys: what product needs vs still missing

Sys is largely **ahead of native UI** for several items. Gaps split into **sys API missing** vs **sys ready / UI missing**.

### 3.1 Product-needed capabilities already in sys (UI or wire incomplete)

| Capability | Sys status | Native use | Notes |
|------------|------------|------------|-------|
| Machine probe | Done (`MachineInfo::probe*`) | Done | Rich inventory |
| Shallow doctor | Done | Done | GPUI shallow only |
| Deep doctor | Done (`DoctorOptions.deep`) | **UI missing** | Residual `open:deep-doctor-ui` |
| Placement plan | Done | Done (summary text) | |
| Chat templates | Done (text multi-turn) | Done | No tools/audio |
| Engine spawn + generate stream | Done | Done | Process mux |
| Stop / cancel mid-generate | Done (`Stop`/`Cancel`, `stop_request`) | Done | Campaign closed |
| Live visual (EMAP/HITS/TIERS/PROF/HWINFO) | Done (`pump_visual`, frames) | Partial UI | Brain sample; HWINFO not dedicated strip; PROF text |
| Model registry scan | Done (`ModelRegistry`) | **UI missing** | Residual `open:model-registry-ui` |
| HF install | Done (feature `install`) | Done form | No cancel; residual `open:install-cancel` |
| Grammar on mux generate | Done on `GenerateRequest.grammar` | Missing end-to-end | Duplex hardcodes `grammar: None`; `ClientFrame::Submit` has **no grammar field** |
| Multi-slot sticky KV | `slot` on `ClientFrame::Submit` + `cache_slot` on request | Partial (always 0) | Residual `open:multi-slot` |
| `Hello.kv_slots` | On `ServerFrame::Hello` | Not surfaced for slot picker | |

### 3.2 Explicitly not product for native embed (by design)

| Item | Status |
|------|--------|
| OpenAI REST `/v1/chat/completions` | Intentionally absent from colibri-sys |
| Anthropic Messages rewrite | Python only |
| True `libcolibri` in-process engine | Phase D design; stub `ffi_available() == false` |
| Full Tauri SPA pixel parity in GPUI | Residual `open:tauri-parity` deferred |

### 3.3 Real sys gaps (not just UI)

| Gap | Detail | Effort class if fixed in sys |
|-----|--------|------------------------------|
| **Grammar on duplex path** | `GenerateRequest` supports GBNF; `EngineDuplex` maps Submit with `grammar: None`; frame enum lacks field | **S** (add optional field + map + tests) |
| **Install cancel** | `install_model` runs CLI/hf-hub to completion; no first-class cancel handle | **M** (cancel token / kill child + progress terminal state) |
| **Tools / Inkling audio chat** | Not in `render_chat` port | **L** (product scope expansion) |
| **Phase D FFI** | No no-`main` libcolibri | **L** strategic |

### 3.4 Residual map (living authority)

From `.agents/RESIDUAL.md` (2026-08-10), open product-high items:

- `open:brain-full-atlas` — full-res Brain + hover atlas
- `open:install-cancel`
- `open:model-registry-ui`
- `open:live-hwinfo-strip`
- `open:deep-doctor-ui`
- `open:multi-slot`
- `open:grammar-submit`

Strategic: `open:ffi-phase-d`, `open:tauri-parity`, `open:openai-rest` (intentional miss).

Polish: brain pulse decay animation, install min_free gate, visual pump join on drop.

---

## 4. Gap matrix

**Effort:** **S** = days / one implement slice · **M** = multi-day / multi-module · **L** = campaign-sized
**Status keys:** done · partial · missing · n/a · intentional miss

| Product capability | Web / Tauri | colibri-native | colibri-sys | Effort class (to product-MVP native) |
|--------------------|-------------|----------------|-------------|--------------------------------------|
| Connect to running HTTP gateway | done | n/a (embed path) | intentional miss REST | — |
| Spawn / own engine process | missing | done | done | — |
| Machine inventory (pre-start) | partial (post-connect HW from health) | done | done | — |
| Doctor shallow | missing in UI (CLI/server) | done | done | — |
| Doctor deep | missing | missing UI | done | **S** UI |
| Placement plan before load | missing | done (summary) | done | **S** polish only |
| Model path / store | n/a (server model) | done | done | — |
| Model registry picker | via HTTP `/models` only | missing | done | **S–M** UI |
| HF install into store | missing | done form | done | **M** cancel |
| Chat multi-turn stream | done | done | done | — |
| Stop mid-generate | done (HTTP abort) | done (mux STOP) | done | — |
| Temperature / max tokens UI | done | partial (host defaults) | done on frames | **S** |
| Reasoning / thinking toggle | done | missing | depends on template/engine | **S–M** |
| Multi-slot KV conversations | done | partial (slot 0 only) | wire ready | **M** UI + session map |
| Scheduler / queue dashboard | done | missing | via health/mux partially | **M** if needed |
| Live tiers bar | done (sidebar) | done (text strip) | done | **S** polish to bar |
| Live HWINFO strip | done (health poll) | partial (static probe) | absorbs HWINFO | **S** |
| Brain full canvas | done | partial (≤2048 sample) | done maps | **M** full-res + scroll |
| Brain atlas / tooltips | done | missing | data not required in sys | **M** (port atlas asset + hover) |
| Brain pulse decay animation | done (RAF) | one-shot pulse | n/a UI | **S** |
| Profiling charts | done | text strip | profile window API | **M** charts |
| GBNF grammar on submit | partial/server | missing | Generate yes / duplex no | **S** sys + **S** UI |
| i18n (en/zh/it) | done | missing | n/a | **M** if product requires |
| Brand / hero / empty state | done | lab chat only | n/a | **S–M** polish |
| API key / auth | done (memory) | n/a | n/a | — |
| Persist endpoint/model prefs | done (localStorage) | env + path field | config/env | **S** |
| Tools / function calling | via HTTP if server supports | missing | missing templates | **L** |
| OpenAI REST from host | gateway | intentional miss | intentional miss | — |
| True in-process engine FFI | n/a | missing | stub | **L** strategic |

---

## 5. Recommended production MVP vs full SPA parity

### 5.1 What “production” should mean for native

Recommend **two scopes**, not one vague “parity.”

#### A. Production MVP: **embed product** (recommended first ship)

Goal: a user can install a model, plan on their machine, start the engine, chat with stop, and see enough observability to trust the stack — **without Python and without a browser**.

**Include**

1. Keep engine lifecycle: probe → plan → start → chat → stop.
2. Model registry **picker** (scan store roots) + path override.
3. HF install with **cancel** and clear free-space gate.
4. Deep doctor optional control (button or advanced toggle).
5. Chat controls: temperature, max tokens; clear conversation.
6. Multi-slot if engine Hello advertises `kv_slots > 1` (sticky conversations).
7. Observability MVP: live tiers (keep or promote to bar), PROF text or simple table, Brain at sampled grid **or** progressive full grid without atlas.
8. Live HWINFO from engine when running (not only static probe).
9. Remove or demote **lab chrome** for release builds: honesty strip → About/dev menu; system dump messages → quiet status line.
10. Grammar only if a concrete product flow needs constrained decode; else defer (sys S-fix when needed).

**Exclude from MVP**

- Pixel clone of React layout / CSS.
- Full Brain atlas + educational tooltips (web leads).
- Profiling SPA charts (text/table enough for operators).
- Full i18n (ship English; add locales as follow-up if markets need).
- Local OpenAI REST from native host.
- Phase D FFI.
- Tools / audio templates.

**Why this MVP:** native already **wins** lifecycle and install vs Tauri; closing registry, install cancel, multi-slot, and deep doctor is mostly **UI on existing sys**. Full SPA parity is a second product.

#### B. Full SPA parity (later / optional)

Goal: GPUI matches web capability density: atlas Brain, chart Profiling, scheduler dashboard, i18n, hero UX, reasoning toggle fidelity, polished settings.

| Slice | Effort | Dependency |
|-------|--------|------------|
| Full Brain + atlas + decay | **M–L** | UI heavy; optional static atlas asset |
| Profiling charts | **M** | GPUI plotting or canvas |
| Scheduler dashboard | **M** | May need richer health frames if not only from mux |
| i18n matrix | **M** | String tables + locale switch |
| Reasoning + full inference chrome | **S–M** | Template / engine flags |
| Pixel/visual brand parity | **L** | Design system in GPUI |

Residual already labels this `open:tauri-parity` and out of scope for the fidelity demo. Keep Tauri+web as the **rich remote/dashboard** face if HTTP gateway remains a product path; native as the **local embed** face.

### 5.2 Dual-product decision (planning input)

| If operator wants… | Prefer |
|--------------------|--------|
| One-click local app, no Python, no separate serve | **colibri-native + colibri-sys** (MVP A) |
| Fancy dashboard against a long-running remote/local server | **web + Tauri** as today |
| Both | Keep both; share **colibri-sys** for host logic; do not force GPUI to reimplement HTTP SPA |

Do not treat “production parity” as “rewrite App.tsx in GPUI.” Treat it as **capability parity for the local-embed journey**, with SPA polish as a separate budget.

### 5.3 Suggested delivery order (MVP A)

| Order | Item | Residual id | Size |
|-------|------|-------------|------|
| 1 | Model registry picker UI | `open:model-registry-ui` | S–M |
| 2 | Install cancel + min_free UX | `open:install-cancel`, polish min_free | M |
| 3 | Multi-slot UI when kv_slots > 1 | `open:multi-slot` | M |
| 4 | Deep doctor toggle | `open:deep-doctor-ui` | S |
| 5 | Live HWINFO strip | `open:live-hwinfo-strip` | S |
| 6 | Chat controls + quiet production chrome | (new polish) | S |
| 7 | Grammar duplex field (only if product needs it) | `open:grammar-submit` | S |
| 8 | Brain full atlas / pulse decay | `open:brain-full-atlas`, pulse polish | M |
| — | SPA charts / i18n / full chrome | `open:tauri-parity` | L later |
| — | Phase D FFI | `open:ffi-phase-d` | L strategic |

### 5.4 Risks / honesty for planners

1. **Two UIs will drift** unless shared contracts live in colibri-sys (visual types, plan, doctor). Prefer sys as SoT; web can stay HTTP-facing.
2. **Web stop ≠ mux STOP.** Aborting fetch is different from cooperative engine stop; native is the better local cancel story.
3. **Brain scale:** full MoE maps (10k+ cells) need sampling or virtualization; web full canvas can struggle too on huge maps.
4. **Install cancel** is the biggest **sys** incomplete for “product install.” UI alone cannot finish it.
5. **Grammar** is a small wire hole (duplex always `None`); easy to over-scope into full GBNF authoring UI.

---

## 6. File index (quick cite)

| Area | Paths |
|------|--------|
| Web shell | `web/src/App.tsx`, `Brain.tsx`, `Profiling.tsx`, `i18n/*`, `lib/api.ts` |
| Tauri | `desktop/src-tauri/src/lib.rs`, `tauri.conf.json`, `desktop/README.md` |
| Native UI | `crates/colibri-native/src/main.rs`, `host.rs`, `text_input.rs` |
| Fidelity / residual | `crates/colibri-native/docs/fidelity.md`, `.agents/RESIDUAL.md` |
| Sys public surface | `crates/colibri-sys/src/lib.rs`, `stream/frame.rs`, `engine/serve.rs`, `engine/duplex.rs`, `model/registry.rs`, `model/install.rs`, `doctor.rs` |
| Sys human docs | `crates/colibri-sys/docs/user-guide.md`, `ffi-phase-d.md` |

---

## 7. One-line summary

**Web/Tauri** is a polished HTTP SPA for a separately run engine. **colibri-native** is a working embed proof: sys in-process, real plan/doctor/start/chat/stop/install/visual MVP, still lab-shaped. **Production MVP** should finish host UX (registry, install cancel, multi-slot, deep doctor, live HW, quiet chrome) on colibri-sys, not re-clone the full SPA. **Full SPA parity** is a later, larger campaign.
