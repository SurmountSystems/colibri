# Recon: SPA / Tauri pixel parity vs colibri-native

**Date:** 2026-08-10
**Repo:** `/home/hunter/Projects/surmount/colibri`
**Mode:** read-only recon. No product edits.
**Write path:** `.agents/reports/recon-plan-spa-tauri-parity.md`

**Sources (verified this pass):** `web/src/*`, `desktop/*`, `crates/colibri-native/*`, `crates/colibri-native/docs/fidelity.md`, `.agents/RESIDUAL.md`, prior `recon-prod-parity-native-web.md` (capability-focused; **stale on residual open list** after Phases A–F), marketing stills under `docs/media/`.

---

## Executive answer

| Question | Honest answer |
|----------|----------------|
| Is **capability** parity for the local-embed journey done? | **Mostly yes.** Residual marks Phases A–F closed. Native has probe, doctor (quick + deep), plan, start engine, chat + stop, install + cancel, registry, multi-slot, inference controls, live tiers/HWINFO/PROF text, Brain grid with heat + pulse decay. |
| Is **pixel** parity done? | **No.** Different layout, palette, typography, navigation model, charts, i18n, brand chrome, and density. Residual correctly parks this as `open:tauri-parity`. |
| Should GPUI become a full SPA clone? | **Not as the default next campaign.** Treat **dual product** as the rational plan unless the operator forces a single shell. |
| What “pixel parity” means here | Same **visual design system and layout density** as `web/` (sidebar + tabs + charts + hero + locale), not “has a chat box and a brain grid.” |

---

## 1. Web surface (`web/`)

### 1.1 Role

React/Vite SPA for an **OpenAI-compatible** colibrì server. Default endpoint `http://127.0.0.1:8000/v1`. Does **not** spawn the C engine, install models, run doctor/plan, or own process lifecycle. Operator runs serve/gateway separately.

### 1.2 File map and sizes (source lines, approx.)

| Path | Lines | Role |
|------|------:|------|
| `web/src/App.tsx` | ~379 | Shell: sidebar + topbar tabs + chat composer |
| `web/src/Brain.tsx` | ~183 | Full-res canvas MoE map, tooltips, atlas |
| `web/src/Profiling.tsx` | ~160 | Share bars, SVG charts, turn table |
| `web/src/index.css` | ~193 | Full design system (not just Tailwind tokens) |
| `web/src/i18n/en.ts` | ~128 | English string table |
| `web/src/i18n/zh-CN.ts` | ~114 | Simplified Chinese |
| `web/src/i18n/zh-TW.ts` | ~same | Traditional Chinese |
| `web/src/i18n/it.ts` | ~same | Italian |
| `web/src/i18n/index.ts` | small | Locale provider |
| `web/src/lib/api.ts` | ~207 | `/health`, `/models`, SSE chat, `/profile`, helpers |
| `web/src/lib/runtime.ts` | small | Scheduler/KV capability helpers |
| `web/src/lib/storage.ts` | small | Persist baseUrl/model; purge legacy API key |
| `web/src/components/ui/*` | 4 files | badge, button, input, textarea |
| `web/src/ErrorBoundary.tsx` | small | Recoverable UI crash chrome |
| `web/public/experts.json` | asset | Static atlas affinities for Brain tooltips |
| `web/package.json` | — | React 18, Vite 8, Tailwind 4, lucide; no chart lib (hand SVG) |

**Rough product UI surface:** ~1.5–2k lines of TSX/CSS/i18n (excluding tests and `node_modules`).

### 1.3 Layout (what you actually see)

```
┌────────── 292px sticky sidebar ──────────┬──────────── main ────────────┐
│ Brand (Georgia italic "colibrì")         │ Topbar: model · tabs · badges │
│ CONNECTION: endpoint, API key, Probe     │ Chat | Brain | Profiling      │
│ RUNTIME: HW strip, scheduler grid,       │                              │
│          tier bar (VRAM/RAM/disk),       │ Chat: hero empty-state OR    │
│          session token totals            │       message list + composer│
│ INFERENCE: model, KV session, temp       │ Brain: full canvas + tip     │
│            range, max tokens, reasoning  │ Profiling: tiles + charts    │
│ Foot: transport note + locale switcher   │                              │
└──────────────────────────────────────────┴──────────────────────────────┘
```

**Design tokens (web):** near-black teal-tinted background `#080b0d`, primary mint `#4ed6a5`, glass sidebar blur, serif brand mark, uppercase section labels, dense operator dashboard.

**Marketing still** (`docs/media/colibri-dashboard.png`): older or aspirational chrome (tabs labeled Chat / Brain / **Atlas**, sidebar mini charts for last-turn phases and per-GPU tok/s). **Current code** uses Chat / Brain / **Profiling** and does **not** ship those sidebar sparklines. Pixel parity planning should target **current `web/src`**, not the still alone.

### 1.4 App / Chat

- Multi-turn stream via SSE `streamChat`; AbortController stop (HTTP abort, not mux STOP).
- Sticky conversations per `cache_slot` when `kv_slots > 1`.
- Empty-state hero: orb, “Ask the giant. Keep the machine yours.”, three suggested prompts.
- Live badges: tokens, tok/s, TTFT, usage, truncated, queue wait, slot, Clear.
- Persist `colibri.baseUrl` + `colibri.model`; API key memory-only.

### 1.5 Brain

- Polls `GET /experts` ~1.5s when connected.
- Full-resolution canvas; cell size from ResizeObserver.
- Tier RGB + heat `min(heat/24, 1)`; hits pulse with RAF decay `*= 0.94`.
- Hover tooltips; optional atlas from `/experts.json` (affinity / specialist labels).
- Layer depth copy (early / middle / late / MTP) via i18n.
- **Leads native** on resolution, hover, atlas education, full-page layout.

### 1.6 Profiling

- Polls `GET /profile` ~2s; rolling window last ~40 turns.
- Phase colors: I/O wait, expert matmul, attention, LM head, other.
- UI: metric tiles, horizontal share bars, dual SVG column charts (tok/s + stacked phases), reverse table + disk-service note.
- **Leads native** completely (native is labeled text columns only).

### 1.7 i18n and sidebar controls

| Area | Detail |
|------|--------|
| Locales | en, zh-CN, zh-TW, it |
| Connection | endpoint, optional key, Probe server, connected state |
| Runtime | HW from `/health`, scheduler active/queued/completed/failures, tier bar, session stats |
| Inference | model select, KV session, temperature **range** slider 0–2, max tokens, reasoning toggle |
| Locale | Globe select in sidebar foot |
| Missing vs native | No doctor, plan, HF install, registry scan, start engine, GBNF field |

---

## 2. Desktop Tauri shell (`desktop/`)

### 2.1 Role (by design)

**Thin window only.** Packages the shared React app. No second frontend. Dev: Vite `web/` on `:5173`. Release: `web/dist`.

| Path | Size | Role |
|------|-----:|------|
| `desktop/README.md` | short | Explicit: no engine bundle, no FS/process perms in first increment |
| `desktop/src-tauri/src/lib.rs` | **6 lines** | `tauri::Builder::default().run(...)` only |
| `desktop/src-tauri/src/main.rs` | tiny | Entry |
| `desktop/src-tauri/tauri.conf.json` | ~42 | Window 1280×820 (min 860×600), CSP allows localhost HTTP to engine |
| `desktop/src-tauri/Cargo.toml` | — | `tauri` 2.x only; **no colibri-sys** |

### 2.2 What Tauri does **not** do

- Does not start the C engine or Python gateway.
- Does not download models.
- Does not link colibri-sys.
- Does not add native menus, system tray, or deep OS integration beyond a window + CSP.

**Implication for “SPA/Tauri pixel parity”:** Tauri **already is** pixel-identical to `web/` because it **is** `web/`. The parity problem is **web design system vs colibri-native GPUI**, not Tauri vs React.

---

## 3. colibri-native surface vs web gaps

### 3.1 Role

GPUI binary: **local embed product path**. Links colibri-sys in-process; spawns C engine over serve mux; rkyv `ClientFrame` / `ServerFrame`; **no REST**. Documented as production MVP for embed, **not** a full SPA clone (`README.md`, `fidelity.md`, `RESIDUAL.md` `open:tauri-parity`).

### 3.2 File map and sizes

| Path | Lines | Role |
|------|------:|------|
| `crates/colibri-native/src/main.rs` | ~1691 | Entire window chrome + panels + chat render |
| `crates/colibri-native/src/host.rs` | ~2156 | Sys glue, formatters, Brain sample, tests |
| `crates/colibri-native/src/text_input.rs` | ~few hundred | Lean single-line GPUI input |
| `crates/colibri-native/docs/fidelity.md` | ~70 | Capability matrix (authoritative for **done/partial/missing**) |
| `crates/colibri-native/Cargo.toml` | — | gpui 0.2, colibri-sys; feature `install` default |

### 3.3 Layout (current product chrome)

```
Title: colibrì (native) · engine label · status · About (toggle)
Live memory-placement strip (tiers text)
Live HWINFO strip
┌──── left ~440px scroll ─────┬────────── right chat ──────────┐
│ Machine (summary/details)   │ Chat log (you / colibrì / note)│
│ Doctor (Run checks / Deep)  │ Composer + Send / Stop         │
│ Plan / model + Start engine │                                │
│ Registry scan list          │                                │
│ Inference controls          │                                │
│ Profiling (text, last 8)    │                                │
│ Brain (grid ≤2048 sample)   │                                │
│ HF install (+ Cancel)       │                                │
└─────────────────────────────┴────────────────────────────────┘
```

**Design tokens (native):** slate/blue lab palette (`BG 0x121218`, accent **blue** `0x3b82f6`, green OK), not web mint/teal. No Georgia brand, no glass blur, no tabbed main views, no hero empty state.

### 3.4 Capability vs chrome (post Phase F)

**Aligned enough for embed MVP (capability):**

| Capability | Web/Tauri | Native |
|------------|-----------|--------|
| Chat stream + stop | HTTP abort | mux STOP |
| Temp / max tokens / reasoning | yes | yes (panel fields) |
| Multi-slot sticky | yes | yes when `kv_slots > 1` |
| Live tiers | sidebar bar | text strip (content yes, bar chrome no) |
| Live HW | health poll | HWINFO strip |
| Brain heat + pulse decay | RAF full canvas | pump-scaled decay; sampled grid |
| PROF data | charts | text columns (`PROF_LAST_N = 8`) |
| Doctor / plan / start / install | **missing** | **done** (native wins lifecycle) |
| i18n | 4 locales | English hard-coded |
| Scheduler dashboard | yes | missing |
| GBNF grammar field | not in SPA UI | present on Inference panel |

**Still open on residual (not “pixel”):**

| Id | Gap |
|----|-----|
| `open:brain-full-atlas` | Full-res grid + hover atlas / `experts.json` |
| `open:tauri-parity` | Pixel / full SPA chrome (charts, brand layout, i18n density, …) |
| `open:npu-inference` | Inventory only |
| `open:ffi-phase-d` | True in-process libcolibri (strategic) |
| `open:openai-rest` | Intentional miss on native path |
| `open:visual-pump-idle-stop` | Polish: join handle on drop |

**Stale prior recon note:** `recon-prod-parity-native-web.md` still lists registry UI, install cancel, multi-slot, deep doctor, live HWINFO, grammar as open. **Those are closed** per current `RESIDUAL.md` and fidelity matrix. Use residual + fidelity as SoT for capability; this report for pixel/strategy.

### 3.5 Visual gap inventory (what “not pixel parity” means)

| Dimension | Web | Native today | Effort class to match web |
|-----------|-----|--------------|---------------------------|
| Shell layout | 292px branded sidebar + tabbed main | 440px ops stack + chat | **L** (full re-layout) |
| Brand / hero | Serif mark, empty-state marketing | Functional empty chat | **M** |
| Palette / type | Teal mint, Inter, CSS variables | Blue accent, GPUI rgb constants | **M** |
| Tier **bar** (proportional) | Colored flex bar + legend | Text line | **S** |
| Brain page | Full viewport canvas + tooltips + atlas | 180px max-height sampled cells | **M–L** |
| Profiling page | Tiles + share + SVG + table | Multiline text strip | **M** |
| Topbar live badges | Tokens / tok/s / TTFT / queue | Status string only | **S–M** |
| Scheduler grid | 4-cell dashboard | absent | **M** (need data path) |
| Locale | 4 languages | none | **M** |
| Range slider UX | Temperature slider | Text fields | **S** |
| Error boundary polish | Recoverable React boundary | process-level failures | **S** if desired |

---

## 4. What “pixel parity” means vs capability already done

### Capability parity (mostly done for embed)

User can **install → plan → start → chat → stop → observe** without Python/browser. Fidelity rows for probe/doctor/plan/chat/stop/install/registry/slots/grammar/HWINFO/tiers/PROF/Brain-MVP are **done** or **partial** with intentional limits. That is **product capability parity for the local-embed journey**, not SPA clone.

### Pixel parity (not done; residual `open:tauri-parity`)

Means matching **look and interaction density** of `web/`:

1. Same **spatial language**: branded left rail, tabbed primary views (Chat / Brain / Profiling full-bleed).
2. Same **visual system**: colors, type hierarchy, badges, empty states, spacing rhythm.
3. Same **observability chrome**: proportional tier bar, SVG (or equivalent) PROF charts, full Brain page with hover/atlas.
4. Same **locale surface** (if product requires multi-language).
5. Side-by-side screenshot diff that a non-engineer would call “the same app.”

**Does not mean:**

- Re-implement HTTP/SSE inside GPUI.
- Drop native lifecycle (start/install/doctor) to match web’s thinner model.
- Force `ffi_available() == true`.
- Match marketing PNGs that diverge from current source.

### Capability ≠ chrome

| | Capability parity | Pixel parity |
|--|-------------------|--------------|
| Success test | Operator completes embed journey without Python | Screenshots / UI review match SPA |
| Current status | **MVP complete** (residual) | **Deferred** |
| Primary cost | Already spent (A–F) | New campaign: GPUI design system + charts + Brain page |
| Risk if forced | Low | High: dual maintenance or abandon Tauri polish |

---

## 5. Suggested parallel workstreams (honest options)

### Option A — Dual product (recommended default)

| Stream | Owner surface | Goal |
|--------|---------------|------|
| **A1. Tauri SPA as product chrome** | `web/` + thin `desktop/` | Rich dashboard against long-running / remote gateway; keep charts, i18n, Brain atlas |
| **A2. Improve native embed** | `colibri-native` + sys | Close remaining residual (full Brain atlas optional); quiet polish; not pixel-clone |
| **A3. Shared contracts only** | `colibri-sys` | Visual types, plan/doctor/install remain single SoT; do not share React |

**Pros:** Each path keeps its strength (SPA polish vs local lifecycle). Matches current README positioning.
**Cons:** Two UIs drift unless sys contracts stay shared.
**Force-merge cost avoided.**

### Option B — GPUI rewrite toward SPA pixel parity

| Stream | Goal |
|--------|------|
| **B1. Design system in GPUI** | Tokens, sidebar, top tabs, empty hero matching `index.css` |
| **B2. Brain full page** | Full-res or virtualized canvas + hover + atlas asset |
| **B3. Profiling charts** | GPUI canvas/paths or embedded plot; same phases as web |
| **B4. i18n** | Port string tables |
| **B5. Optional deprecate Tauri** | Only if operator wants one binary |

**Pros:** One local binary can look like marketing stills.
**Cons:** Large GPUI UI campaign; charts + canvas + i18n are **L**; web skill already exists; risk of half-finished clone.
**Do not start without explicit product OK.**

### Option C — Hybrid chrome (middle path)

Keep Tauri for “pretty remote dashboard.” On native, lift **only high-value visual chips** without full layout rewrite:

1. Proportional tier bar (not text-only).
2. Topbar tok/s / TTFT badges during generate.
3. Brain panel expand to larger height + optional atlas tooltips.
4. Simple PROF bar table (not full SVG page).
5. Teal brand accent alignment (cheap trust).

**Pros:** Improves native feel for **S–M** total without claiming pixel parity.
**Cons:** Still two products; residual `open:tauri-parity` stays open until B.

### Dual-product vs force-merge (decision table)

| If operator wants… | Choose |
|--------------------|--------|
| One-click local app, no Python, no separate serve | **Native embed** (A2); keep SPA as optional |
| Fancy multi-user / remote / long-lived server dashboard | **Web + Tauri** (A1) |
| Single binary that looks exactly like the SPA | **B** (expensive); plan multi-phase |
| “Just make native prettier” without killing Tauri | **C** |
| Kill one path | Explicit deprecation decision; do not soft-orphan either without writing residual |

**Recommendation:** **A + selective C.** Do not force-merge SPA into GPUI as the next “MVP.” Capability MVP is already the closed campaign story.

---

## 6. Concrete phased plan (with paths and sizes)

Sizes: **S** = days / one implement slice · **M** = multi-day multi-module · **L** = campaign.

### Phase 0 — Decide scope (operator)

| Deliverable | Notes |
|-------------|--------|
| Pick **A**, **B**, or **C** | Document in residual / AGENTS if product law |
| Define success screenshots | Current `web/` (not necessarily `docs/media` stills) |

**No code.** If dual product: Phase 1 = C polish only; Phase B never starts.

---

### Phase 1 — Native visual chips (Option C floor) — **S–M**

Target residual: partial credit toward polish; **does not close** `open:tauri-parity`.

| Step | Work | Paths | Size |
|------|------|-------|------|
| 1.1 | Align accent palette toward web mint (or document intentional lab blue) | `main.rs` color consts (`BG`, `ACCENT`, …) | **S** |
| 1.2 | Proportional tier bar from `TiersSnap` | `main.rs` strip; reuse `format_live_tiers` / host | **S** |
| 1.3 | Live generate badges (tok count / tok/s / TTFT) | `main.rs` title/status; gen poll path | **S** |
| 1.4 | Larger Brain viewport (e.g. 180 → 320+ px); keep sample | `brain_panel` in `main.rs` | **S** |
| 1.5 | PROF as compact labeled table (still text, clearer columns) | `format_profile_turns` in `host.rs`, panel in `main.rs` | **S** |

**Acceptance:** Operator preference, not screenshot-identity with SPA.

---

### Phase 2 — Brain full atlas (residual high value) — **M**

| Step | Work | Paths | Size |
|------|------|-------|------|
| 2.1 | Raise or remove `BRAIN_MAX_CELLS` 2048 with scroll/virtualize | `host.rs` `brain_view_from_map`, `main.rs` | **M** |
| 2.2 | Hover cell inspector (layer, expert, tier, heat) | GPUI hit-test on grid; no canvas API | **M** |
| 2.3 | Optional atlas: ship or load `experts.json`-shaped data | `web/public/experts.json` as reference; native asset path TBD | **M** |

Closes `open:brain-full-atlas` when full-res + tooltips land. Still **not** full SPA pixel parity.

---

### Phase 3 — Profiling charts (SPA-class) — **M**

| Step | Work | Paths | Size |
|------|------|-------|------|
| 3.1 | Reuse profile window data already in sys | `colibri-sys` visual / `pump_visual` | — |
| 3.2 | Share bars + stacked turn columns in GPUI | new module e.g. `crates/colibri-native/src/prof_charts.rs` + `main.rs` tab or panel | **M** |
| 3.3 | Match phase colors from `Profiling.tsx` PHASES | keep palette constants shared in comment or small rust const | **S** |

Closes a large slice of `open:tauri-parity` observability, not layout.

---

### Phase 4 — SPA shell layout in GPUI (Option B core) — **L**

| Step | Work | Paths | Size |
|------|------|-------|------|
| 4.1 | Sidebar + main column grid matching `index.css` proportions | rewrite `Render for DesktopApp` (~400–800 lines of layout churn in `main.rs`) | **L** |
| 4.2 | Tabbed Chat / Brain / Profiling full views | `main.rs`; Brain/Profiling leave left stack | **M–L** |
| 4.3 | Hero empty state + suggested prompts | `main.rs` | **S–M** |
| 4.4 | Port i18n tables | new `crates/colibri-native/src/i18n/` from `web/src/i18n/*.ts` (~400+ lines strings × 4) | **M** |
| 4.5 | Scheduler dashboard if mux/health exposes counts | may need sys/frame work | **M** |

**Acceptance for true pixel parity:** side-by-side with Tauri window on same engine session; design review sign-off.

**Estimated campaign size if B is chosen end-to-end:** on order of **rewriting most of `main.rs` (~1.7k)** plus chart/Brain modules, comparable to a **new frontend** (weeks, not a residual mop).

---

### Phase 5 — Web/Tauri stream (parallel, always valid under A)

| Step | Work | Paths | Size |
|------|------|-------|------|
| 5.1 | Keep SPA as rich remote face | `web/`, `desktop/` | ongoing |
| 5.2 | Optional: sidebar sparklines if product wants marketing still fidelity | `App.tsx` / CSS | **M** |
| 5.3 | Do **not** bolt colibri-sys into Tauri without a separate plan | would change `desktop/` role | **L** separate |

Tauri stays **~6 lines of Rust** + conf unless product expands native permissions.

---

### Phase 6 — Explicit non-goals (unless operator pivots)

| Item | Why |
|------|-----|
| OpenAI REST inside native | Intentional miss; web owns HTTP face |
| Phase D FFI for “pretty UI” | Orthogonal to pixels |
| Deleting Tauri “because native exists” | Loses free SPA polish and remote-dashboard path |
| Bulk port of React components to GPUI 1:1 | Wrong abstraction; port **design intent**, not DOM |

---

## 7. Architecture reminder (do not confuse shells)

```
web (React) ──HTTP/SSE──► Python openai_server ──mux──► C engine
desktop (Tauri) ──loads web/dist or Vite──► same HTTP path

colibri-native (GPUI)
  └── colibri-sys (in-process)
        └── EngineDuplex (rkyv) → ServeClient → C engine process
```

- **Host in-process** ≠ **engine in-process**.
- **Pixel parity** is a UI campaign on the GPUI side (or abandoning GPUI for Tauri).
- **Capability parity** for embed is largely **closed**.

---

## 8. One-line summary

**Web + Tauri already share one polished SPA;** colibri-native is a **different, capability-complete embed product** with lab-adjacent chrome. Pixel parity is a **large optional redesign** (`open:tauri-parity`), not the unfinished half of Phases A–F. Prefer **dual product + selective native visual chips**; only run a full GPUI SPA rewrite if the operator explicitly wants one shell that looks like `web/`.

---

## 9. Cite index

| Area | Paths |
|------|--------|
| Web shell | `/home/hunter/Projects/surmount/colibri/web/src/App.tsx`, `Brain.tsx`, `Profiling.tsx`, `index.css`, `i18n/*`, `lib/api.ts` |
| Tauri | `/home/hunter/Projects/surmount/colibri/desktop/README.md`, `desktop/src-tauri/src/lib.rs`, `tauri.conf.json` |
| Native | `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/main.rs`, `host.rs`, `docs/fidelity.md`, `README.md` |
| Residual | `/home/hunter/Projects/surmount/colibri/.agents/RESIDUAL.md` |
| Prior capability recon (stale opens) | `/home/hunter/Projects/surmount/colibri/.agents/reports/recon-prod-parity-native-web.md` |
