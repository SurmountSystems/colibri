# Recon + plan: full Brain atlas (hover / experts.json)

**Scope:** map web Brain full canvas + atlas hover vs native GPUI Brain; what sys already exposes; concrete plan for `open:brain-full-atlas`.
**Date:** 2026-08-10.
**Mode:** read-only recon (no product edits).
**Residual id:** `open:brain-full-atlas` (still open in `.agents/RESIDUAL.md`).

---

## Executive summary

| Surface | Live EMAP/HITS grid | Heat /24 | Hits pulse decay | Full resolution | Hover tooltips | `experts.json` atlas |
|---------|---------------------|----------|------------------|-----------------|----------------|----------------------|
| **Web** `web/src/Brain.tsx` | yes (poll `/experts` 1.5s) | yes | yes (RAF `*=0.94`) | yes (all cells) | yes | yes (static `/experts.json`) |
| **Native** `colibri-native` | yes (`pump_visual` ~500ms) | yes | yes (batched RAF-equivalent) | **no** (stride sample ≤2048) | **no** | **no** |
| **sys** `colibri-sys` | typed map/hits on handle + duplex frames | N/A (raw cells) | N/A | full binary map | N/A | **not loaded** |

Native already matches web on **color/heat and multi-frame hit pulse**. The residual is **full-res grid (optional)** plus **atlas-backed hover** (and depth-role fallback when atlas missing). Web SPA still leads.

---

## 1. Web Brain.tsx (reference implementation)

**File:** `/home/hunter/Projects/surmount/colibri/web/src/Brain.tsx`

### Canvas / layout

- Full-page “Expert Cortex” with `HTMLCanvasElement` in a `ResizeObserver` wrapper.
- Cell size: `max(2, floor(min(wrapW/cols, wrapH/rows)))`; 1px gap when `cell >= 4`.
- Canvas pixel size = `cols × (cell+gap)` by `rows × (cell+gap)` (full map, no sampling).
- Header: rows×cols, tier totals (VRAM / RAM / disk counts from `map`), brightness + flash legend.
- i18n keys under `brain.*` in `web/src/i18n/en.ts` (and zh-CN / zh-TW).

### Live data: `GET /experts`

Poll every **1500 ms** while `connected`:

```ts
{ rows, cols, map: hex, hits: hex, seq: number }
```

- `map`: 2 hex chars per expert, row-major, `byte = (tier<<6) | heat` (`heat` 0..63).
- `hits`: 2 hex chars per packed byte; expert `i` → byte `i>>3`, bit `i&7`.
- On `seq` change with non-empty `hits`, set `pulseRef[i] = 1` for hit bits.

### Heat and tier color

```ts
const TIER_RGB = [[58,71,80], [90,155,216], [78,214,165]] // disk, RAM, VRAM
const lum = 0.35 + 0.65 * Math.min(heat / 24, 1)
// then pulse blends toward white: rgb += (255-rgb)*pulse
```

Heat saturates visually at **24**, not the full 0..63 pack range (log2-style heat).

### RAF pulse decay

- After each full grid paint: for each pulse `p[i]`, if `> 0.01` then `p[i] *= 0.94`, else 0.
- `requestAnimationFrame(draw)` only while any pulse is alive; keepalive interval 400 ms restarts draw when RAF id is cleared.
- Nominal ~60 Hz → each step multiplies by 0.94.

### Hover tooltips

- `onMouseMove` → cell from pointer + canvas scale + `(cell+gap)`.
- Tooltip state: `{ x, y, row, col, tier, heat }`.
- **Layer id mapping (GLM-shaped):**
  - Last grid row = MTP: `realLayer = 78`, label “(MTP)”.
  - Else: `realLayer = tip.row + 3` (sparse MoE layers start after dense 0..2).
- Atlas key: `` `${realLayer}:${tip.col}` ``.
- If atlas entry present:
  - Specialist if `label.startsWith("specialist")` → `brain.specialist` + `top` + entropy.
  - Else generalist + entropy.
  - Top-3 affinities by value: `"code 40% · poetry 12% · …"`.
- If no entry: **depth role** from `depthRoleKey(row, rows, isMtp)`:
  - MTP → `brain.mtp`
  - row fraction: early / lowerMiddle / upperMiddle / late / final (`brain.early` … `brain.final`).

### Static atlas load

```ts
fetch("/experts.json").then(r => r.ok ? r.json() : null).then(d => {
  if (d?.experts) setAtlas(d.experts)
})
```

Once on mount; silent catch if missing. Vite serves `web/public/experts.json` as static root (also publishable into `web/dist` via atlas tool `--web`).

### Not in Brain.tsx but related

- Docs mention a separate 3-D “Atlas galaxy” view (`docs/api.md`); Brain hover is the primary in-product consumer of the same JSON shape.
- Poll is HTTP; no WebSocket push for EMAP/HITS mid-turn (HITS on gateway path is largely end-of-turn; see explore-visual-telemetry).

---

## 2. Native brain panel (already landed)

**Files:**

- `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/host.rs` — `BrainView`, sampling, RGB, pulse decay.
- `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/main.rs` — `brain_panel`, `apply_visual_snapshot`, `VISUAL_PUMP_MS = 500`.
- `/home/hunter/Projects/surmount/colibri/crates/colibri-native/docs/fidelity.md` — status **partial**.

### Data path

```
Engine process mux (EMAP/HITS lines)
  → colibri-sys ServeClient telemetry
  → VisualSnapshot (ExpertMap + ExpertHits)
  → EngineHandle::visual_snapshot / expert_map / expert_hits
  → host::pump_session_visual
  → App::apply_visual_snapshot
  → brain_view_from_map + apply_brain_pulse_decay
  → GPUI brain_panel (colored div grid)
```

### Sampling

- `BRAIN_MAX_CELLS = 2048`.
- If `rows*cols <= 2048`: full grid.
- Else: aspect-preserving stride sample; `sampled = true`; status note `"sampled Dr×Dc of Sr×Sc (max 2048 cells)"`.
- Full GLM map ~**76×256 = 19 456** experts → always sampled in native today.

### Heat + pulse (web parity, with one aesthetic delta)

- `brain_cell_rgb`: same tier RGB and `lum = 0.35 + 0.65 * min(heat/24, 1)`.
- Hit pulse: native blends toward **warm white** (`255, 240, 180` with coefficients); web toward **pure white** `(255-rgb)*pulse`. Close enough for MVP; full atlas work can optionally match pure white.
- On `hits_seq` change: pulse 1.0 on hit experts; `apply_brain_pulse_decay` carries prior pulse with `steps = brain_pulse_decay_steps_for_ms(500)` ≈ 31 × `*=0.94`.
- Fresh hits (`pulse >= 0.99`) not decayed that tick; dimension mismatch drops prior pulses.

### UI

- Title “Brain”, status line, scrollable max-height 180px grid of tiny divs (3–6 px by col count).
- Legend text only: “Gray = disk · Blue = system RAM · Green = GPU · Bright = hot · Flash = hit”.
- **No** mouse handlers, **no** tooltips, **no** atlas load, **no** depth-role copy.

### Tests already present (`host.rs`)

- `brain_view_samples_large_map` (76×256)
- `brain_view_full_small_map`
- `brain_view_hit_pulse_on_seq_change`
- `brain_pulse_decay_math_matches_web_raf`
- `brain_pulse_decay_steps_for_ms_maps_pump_cadence`
- `apply_brain_pulse_decay_carries_and_preserves_fresh_hits`

---

## 3. `experts.json` location and format

### Locations

| Path | Role |
|------|------|
| `web/public/experts.json` | Dev SPA static asset (Vite public root) |
| `web/dist/experts.json` | Published dashboard copy (`analyze.py --web`) |
| `c/tools/expert_atlas/` | Offline sweep + analyze pipeline (`README.md`, `probes.json`, `analyze.py`, `validate.py`) |
| Generated `atlas_out/experts.json` | Full analysis dump (array-style experts; not the web key shape) |

Repo ships a **large** published atlas under `web/public/experts.json` (multi-megabyte JSON; do not re-read whole file in agents).

### How it is produced

```bash
cd c
./tools/expert_atlas/sweep.sh
python3 tools/expert_atlas/analyze.py --stats atlas_out/stats --out atlas_out/experts.json \
  --web web/dist/experts.json   # or web/public/experts.json for SPA
```

`--web` writer (`analyze.py` ~lines 125–135):

```json
{
  "categories": ["code_python", "code_sql", "math_proof", "chinese", "german", "poetry", "law", "medicine", ...],
  "experts": {
    "3:42": {
      "affinity": { "code_python": 0.4, "poetry": 0.1 },
      "entropy": 1.23,
      "top": "code_python",
      "label": "specialist: code_python"
    },
    "10:7": {
      "affinity": { "...": 0.12 },
      "entropy": 2.8,
      "top": "...",
      "label": "generalist"
    }
  }
}
```

Rules:

- Key = `"layer:expert"` (absolute layer id, not EMAP row index).
- `label` = `"specialist: {top}"` if `spec >= 0.5` else `"generalist"`.
- `affinity` omits zero topics; entropy is bits of affinity distribution.
- Measurement traps (top-p, MTP drafts, `.coli_usage` accumulation, autocorrelation) are documented in `c/tools/expert_atlas/README.md`; sweep forces controls.

### How web loads it

1. Static file next to SPA (`/experts.json`), **not** the live engine.
2. Separate from `GET /experts` (live EMAP/HITS).
3. Optional: gateway may also serve `experts.json` from static root if published (`docs/serve_protocol.md`).

Native has **no** equivalent path today (no HTTP SPA; no file load; no embed of atlas).

---

## 4. What colibri-sys already exposes (EMAP / HITS / visual)

**Module:** `crates/colibri-sys/src/visual.rs` (+ absorb from `engine/serve.rs`, duplex frames).

| Type / API | Content |
|------------|---------|
| `ExpertMap { rows, cols, cells: Vec<u8> }` | Full grid; `tier_at` / `heat_at`; `from_hex` |
| `ExpertHits { rows, cols, bits, seq }` | LE bit packing; `hit(index)`; `from_hex` |
| `VisualSnapshot` | tiers, hwinfo, expert_map, expert_hits, profile, seqs |
| `EngineHandle::expert_map()` / `expert_hits()` / `visual_snapshot()` | poll after generate / pump |
| `ServerFrame::ExpertMap { rows, cols, cells }` | rkyv duplex push under `Subscribe::VISUAL` |
| `ServerFrame::ExpertHits { rows, cols, bits, seq }` | only when `seq` changes |
| ServeClient | `emap_hex()`, `hits_hex()`, `hits_seq()`, `tiers()`, `hwinfo()` |

Packing matches `c/telemetry.h` / serve protocol:

- EMAP: `byte = (tier << 6) | heat`
- HITS: 1 bit per expert, cleared on emit at engine; host increments `hits_seq` on each HITS line

**Not in sys (and not needed for residual unless we want shared crate types):**

- Loading / parsing `experts.json`
- Layer-index ↔ EMAP-row mapping (`row+3`, MTP last row → 78)
- UI hover geometry

Optional design note: FFI doc mentions `coli_visual_poll` for EMAP/HITS without SERVE text (`ffi-phase-d.md`); product path remains process mux + `VisualSnapshot`. True FFI is deferred (`open:ffi-phase-d`).

---

## 5. Concrete implementation plan (native full atlas + hover)

Residual: `open:brain-full-atlas` — full-resolution Brain (optional no sample) + hover atlas tooltips. **Not** a full SPA Brain rewrite / 3-D galaxy (`open:tauri-parity`).

### 5a. Atlas types + load (host, pure Rust)

1. Add `ExpertAtlas` in `colibri-native` `host.rs` (or small `atlas.rs` module):

   ```rust
   struct AtlasEntry {
     affinity: HashMap<String, f32>, // or Vec<(String, f32)> sorted later
     entropy: f32,
     top: String,
     label: String,
   }
   struct ExpertAtlas {
     categories: Vec<String>,
     experts: HashMap<(u32, u32), AtlasEntry>, // (layer, expert)
   }
   ```

2. Parse with `serde_json` from file bytes. Accept web shape (`"layer:expert"` string keys).

3. Load order (first hit wins):
   - Explicit UI/path or env e.g. `COLIBRI_EXPERTS_JSON` / `COLI_EXPERTS_JSON`
   - Path next to model store or cwd `experts.json`
   - Optional compile-time / install path under repo `web/public/experts.json` only in dev docs (do **not** force multi-MB default into binary)

4. Missing file → empty atlas; UI still shows depth-role fallback (web parity).

5. Keep atlas **out of** `colibri-sys` unless a second consumer appears; residual is native UI.

### 5b. EMAP row → absolute layer

Port web mapping into a pure function (unit-tested):

```text
fn emap_row_to_layer(row: u32, rows: u32) -> (layer: u32, is_mtp: bool)
  if row + 1 == rows → (78, true)   // MTP head (GLM-5.2 convention in Brain.tsx)
  else → (row + 3, false)
```

Document that **78** and **+3** are GLM-5.2 Brain.tsx conventions (75 MoE × 256 + MTP ≈ 19 456). If other families use different dense prefixes, park a residual or key off model family later; do not invent multi-model atlas without evidence.

### 5c. Hover tooltip (GPUI)

1. Track pointer over the brain grid: either
   - **Preferred for full-res later:** single canvas-like element + hit-test by cell size (closer to web), or
   - **MVP with current div grid:** `on_mouse_move` on grid / per-cell with `(disp_r, disp_c)`.

2. Map **display** cell → **source** `(src_r, src_c)` using the same strides as `brain_view_from_map` (must store strides or recompute from `src_*` / `disp_*`).

3. Build tip lines:
   - `Layer {L}{MTP?} · Expert {c}`
   - Tier name + heat (`never routed` vs `~2^{heat}` style, plain English)
   - If atlas hit: specialist/generalist + entropy + top-3 affinity percents
   - Else: depth role strings (copy from web `brain.*` English; no i18n system required for native MVP)

4. Position: fixed panel under grid or near cursor; clamp to window.

5. State on `App`: `brain_tip: Option<BrainTip>`, clear on leave.

### 5d. Optional full resolution (no sample)

Two-phase so default stays light:

| Phase | Behavior |
|-------|----------|
| **Default** | Keep `BRAIN_MAX_CELLS = 2048` sampling; status still shows “showing Dr×Dc of Sr×Sc”. |
| **Full-res mode** | UI toggle or env `COLIBRI_BRAIN_FULL=1`: set effective max to `rows*cols` (or `usize::MAX`). |

Performance notes for full 19k cells:

- Current **one GPUI div per cell** will struggle at full res; prefer:
  - render into a **single image / canvas element** (if GPUI path allows), or
  - retain sampling for paint but use **full map only for hit-test + tooltip** (partial win), or
  - raise limit modestly (e.g. 4096–8192) before true full-res paint.

Plan recommendation:

1. **Hover works on sampled grid first** (tooltip for sampled experts; note when sampled).
2. **Store full `ExpertMap` reference** in app state (already via pump) so tooltip can use source indices even if display is sampled.
3. **Full-res paint** as second step with canvas/image path + tests for “no sample when mode on and cells ≤ threshold”.

### 5e. Wire-up checklist

1. On engine start / first visual with non-empty map: ensure atlas loaded once.
2. `apply_visual_snapshot` unchanged for heat/pulse; only extend view with stride metadata for reverse mapping if not already recoverable.
3. `brain_panel`: hover handlers + tip child; optional “Full grid” checkbox.
4. Fidelity.md: move Brain row toward **done** for hover + optional full-res; keep honest if paint still sampled by default.
5. RESIDUAL: complete `open:brain-full-atlas` only when hover + atlas + documented full-res path land; not when only raising max cells.

### 5f. Out of scope (do not pull in)

- 3-D Atlas galaxy (`open:tauri-parity`)
- Re-running expert_atlas sweep from desktop
- Changing engine EMAP packing or serve protocol
- Python gateway `/experts` HTTP in native path
- Pure white vs warm pulse micro-diff unless free while touching `brain_cell_rgb`

---

## 6. Tests needed

Red/green TDD on pure host logic first; GPUI interaction only if the crate already has UI test harness (today: unit tests in `host.rs`).

### Must-have unit tests (`colibri-native` / `host`)

| Test intent | Assert |
|-------------|--------|
| Parse web-shaped experts.json fixture (tiny synthetic file in tests or `include_str!`) | categories + `"3:0"` specialist affinity/entropy/top/label |
| Bad / missing keys | empty or skip; no panic |
| `emap_row_to_layer` | row 0 → layer 3; last row of 76 → 78 MTP; mid row → row+3 |
| Display cell → source index under known stride | matches `brain_view_from_map` sampling for 76×256 |
| Tooltip text builder with atlas | specialist line + top-3 order by affinity |
| Tooltip text without atlas | depth-role string for early / late / MTP |
| Full-res mode | `brain_view_from_map` with max_cells = total → `sampled == false` for 76×256 (extract `BRAIN_MAX_CELLS` as param) |
| Heat/pulse regression | existing tests stay green |

### Sys (only if touch)

- No new sys tests unless atlas is wrongly pushed into sys; existing `expert_map_from_cells` / `hits_bit_packing` / duplex ExpertMap frames already cover wire.

### Manual / operator check

1. Start native with engine; generate a turn; Brain pulses and heat update.
2. Point `COLIBRI_EXPERTS_JSON` at `web/public/experts.json` (or a small fixture).
3. Hover a cell: layer/expert, tier, heat, affinity or depth role.
4. Toggle full-res (if shipped): status says full; machine remains usable.

### Fixture note

Do **not** check multi-MB production `experts.json` into unit tests. Ship a **10-entry** fixture under `crates/colibri-native/tests/fixtures/experts_web_shape.json` (or `include_str!` in the test module).

---

## 7. Residual tracking

| Id | Status | Definition of done |
|----|--------|--------------------|
| `open:brain-full-atlas` | **Open** | (1) Optional load of web-format `experts.json`; (2) hover tooltip with affinity/specialist/generalist + depth-role fallback; (3) optional full-res path documented and testable (default may still sample for paint); (4) fidelity + residual updated; (5) unit tests for parse, layer map, tooltip, full-res flag. |

Related closed items (do not re-open): heat/24, pulse decay, 2048 sample, visual pump.

Related still open / separate: `open:tauri-parity` (charts / full SPA), `open:ffi-phase-d`.

---

## 8. Key file index (absolute paths)

| Path | Why |
|------|-----|
| `/home/hunter/Projects/surmount/colibri/web/src/Brain.tsx` | Full web cortex + hover + atlas |
| `/home/hunter/Projects/surmount/colibri/web/public/experts.json` | Published static atlas |
| `/home/hunter/Projects/surmount/colibri/web/src/i18n/en.ts` | `brain.*` copy for depth roles |
| `/home/hunter/Projects/surmount/colibri/c/tools/expert_atlas/analyze.py` | Web JSON shape writer |
| `/home/hunter/Projects/surmount/colibri/c/tools/expert_atlas/README.md` | Measurement method + traps |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/host.rs` | BrainView, sample, RGB, decay, tests |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/main.rs` | brain_panel, pump 500ms |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-native/docs/fidelity.md` | Partial status + residual id |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/visual.rs` | ExpertMap / ExpertHits / VisualSnapshot |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/engine/mod.rs` | `expert_map` / `expert_hits` APIs |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/engine/serve.rs` | EMAP/HITS line parse |
| `/home/hunter/Projects/surmount/colibri/docs/serve_protocol.md` | Wire formats |
| `/home/hunter/Projects/surmount/colibri/.agents/RESIDUAL.md` | `open:brain-full-atlas` living status |
| `/home/hunter/Projects/surmount/colibri/.agents/reports/explore-visual-telemetry.md` | Broader visual inventory |

---

## 9. Suggested implement slice order

1. **Atlas parse + layer map + tooltip pure functions + red tests** (no GPUI).
2. **GPUI hover on current sampled grid** + load path env/file.
3. **Parametrize max cells / full-res toggle** + tests; only then consider canvas paint for 19k cells.
4. Fidelity + residual closeout when (1)+(2) land and (3) is either done or explicitly deferred with residual note.

---

*End recon. Product code untouched.*
