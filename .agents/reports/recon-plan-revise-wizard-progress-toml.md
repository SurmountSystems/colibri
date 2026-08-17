# Recon: Brain/Profiling, install + gen progress, TOML (plan revise)

Date: 2026-08-11 · Scope: `colibri-native` + `colibri-sys` install · no product edits

## 1. Brain + Profiling (native)

**Shell:** left rail + main column; top tabs `Chat | Brain | Profiling` (`MainView` in `crates/colibri-native/src/main.rs`). Mint shell / design-family parity; not pixel-perfect CSS (`docs/fidelity.md`).

**Brain (tab page `brain_panel_full`):**
- Live expert grid from `pump_visual` / FFI visual poll (process mux or GLM embed).
- Tier color + heat (`heat/24`); hits pulse with web-style decay (`*=0.94` per RAF step, batched on ~500 ms pump).
- Default stride-sample ≤2048 cells; **Full grid** toggle / `COLIBRI_BRAIN_FULL`.
- Hover tips: `atlas.rs` + `experts.json` (`COLIBRI_EXPERTS_JSON` or cwd); depth-role fallback if missing.
- One GPUI `div` per cell (web SPA uses canvas + 1.5s `/experts` poll).

**Profiling (tab + `profiling_view.rs`):**
- Web phase model port: IoWait / ExpertMatmul / Attention / LmHead / Other.
- Metric tiles, phase share bars, tok/s columns, stacked wall-time columns, reverse turn table; last 40 turns (`PROF_CHART_N`).
- Data from profile window / PROF after generate (GLM FFI fills; Kimi/Inkling stub; V4 empty until family fill).

**Obvious gaps vs web SPA (fidelity + README):**
- Not the separate web **Atlas 3-D galaxy** page; native Brain is 2-D grid only.
- Layout density “same product family,” not CSS clone; no webview SPA path in native.
- Visual/PROF empty or stub on non-GLM FFI families; process path needs serve mux frames.
- Web: full canvas map + HTTP; native: sampled div grid + in-process visual pump.

## 2. Install progress API + UI

**API** (`colibri-sys` `InstallProgress` in `model/install.rs`):
```text
phase: String, message: String,
bytes_done/total: Option<u64>,
file: Option<String>,
files_done/total: Option<u32>
```
**Phases emitted:** `download` → `inspect` (opt) → `register` (opt) → `done` (local path starts at `register`).

**Fill reality:**
- Prefer-`hf` CLI: progress is coarse phase/message; usually **no** bytes/files.
- `hf-hub` fallback: per-file `files_done`/`files_total` + `file` name; **bytes_* still typically `None`** (struct fields exist, not wired to byte counters).

**UI** (`main.rs` `drain_install`): single muted **text** line
`[phase] message · d/t B · file · files fd/ft`
No bar, percent, or ETA. Cancel supported. Status area `max_h` ~80 scroll.

## 3. Generate / inference progress

**Live (chat topbar):** token count (event ticks), **tok/s** after first token (`live_token_count / elapsed` from `stream_start`), **TTFT** ms, session slot. Status on Done: `done · N tok · X.XX tok/s` (or `stopped`).

**No** remaining-token estimate, max_tokens progress fraction, or generate ETA found. `max_tokens` is a control only.

**Profiling page:** historical per-turn tok/s and phase splits, not mid-stream remaining.

FFI path often pulses `·` every 8 tokens (ids only) unless V4 detokenize buffer; process path streams real UTF-8 tokens + Done `tokens_per_second`.

## 4. TOML deps

- Workspace members (`colibri-sys`, `colibri-native`): **no direct** `toml` / `toml_edit` in any `Cargo.toml`.
- Both use **`serde_json`**.
- `Cargo.lock` has transitive `toml` 0.8.x (e.g. cbindgen) and `toml` 1.x / `toml_edit` via build tooling only. Adding first-party TOML means a new direct dep.
