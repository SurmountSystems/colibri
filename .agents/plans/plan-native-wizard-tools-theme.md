# Plan: Native setup wizard, Tools panel, DOGE + mint themes (revised)

## Context

Operator feedback: **colibri-native is hard to use** at cold start. The left rail is dense; empty Chat does not teach a path. Native should lead over Tauri/web for first-run and daily setup.

**Revise notes:**

| Ask | Plan response |
|-----|----------------|
| Brain + Profiling faithful under theming rules (Rust/GPUI) | Explicit workstream: re-theme both tabs with palette tokens; keep SPA-shaped Brain grid + Profiling charts; DOGE maps phase colors to pure octal only |
| Prefer **TOML** over JSON for config | `native-ui.toml` via `toml` + serde (add dep); no JSON prefs |
| User-friendly, no jargon | Product copy pass: plain American English in wizard/tools/rail; no lab/decoder-ring labels in UI |
| Long work needs **determinate** bar, **%**, and **time left** | Shared progress widget for HF install + generate; improve install byte/file reporting where missing; generate ETA from max tokens + live tok/s |
| Step 4 Check readiness is right | Keep as first-class wizard step: Doctor + placement summary before theme/finish |
| Implement with hierarchical subagents, effort 3 | See **Implementation orchestration** below |

Goals:

1. **Stateful setup wizard** on first start (machine + models); re-open from dashboard.
2. **Tools panel** for clutter (Doctor, install, theme, language, About).
3. **DOGE default theme**, mint kept. Spec: [0001_DOGE.md](https://github.com/SurmountSystems/specs/blob/main/0001_DOGE.md) (accessed: 2026-08-11).
4. **Brain + Profiling** stay first-class and theme-correct (not orphaned mint hardcodes).
5. **Determinate progress** for download and inference (bar + percent + estimated time left).

### Non-goals

- Tauri parity.
- Whole-disk model search / HF hub cache crawl.
- Soft gray midtones in DOGE (spec forbids).
- Perfect CSS pixel match to web.
- 3-D Atlas (native stays 2-D grid unless separately ranked).
- Guaranteed exact ETAs (estimates only; show “about N min left” and degrade gracefully when rate unknown).

### Assumptions

1. Default theme **DOGE**. Mint fully maintained.
2. Skip sets `first_run_done` so users are not trapped; Setup remains available.
3. Completing wizard does not force Start engine.
4. Config is **TOML** at `~/.config/colibri/native-ui.toml`.
5. Progress ETAs are best-effort; never block completion on ETA math.

---

## Approach

### A. User preferences (TOML)

**Path:** `~/.config/colibri/native-ui.toml` (XDG; Windows via same family as model store).

Example:

```toml
version = 1
first_run_done = false
theme = "doge"          # "doge" | "mint"
locale = "en"           # "en" | "it"
last_model_path = ""    # optional absolute path
```

Module: `crates/colibri-native/src/prefs.rs` with `toml` + `serde`.

Env: `COLIBRI_THEME`, `COLIBRI_SKIP_WIZARD=1`.

Tests: missing file → defaults; round-trip; reject unknown theme to doge.

**Not** JSON. Add direct workspace dep on `toml` if not already direct.

### B. Theme system (DOGE default + mint)

`ThemeId` + `ThemePalette`; mint = current SPA-family tokens; DOGE = only eight pure colors ([spec Clause 4](https://github.com/SurmountSystems/specs/blob/main/0001_DOGE.md)).

| Role | DOGE (default mapping) |
|------|------------------------|
| Background / panels | Black `#000000` |
| Body text | White `#FFFFFF` |
| Secondary / muted | Cyan `#00FFFF` |
| Active / OK / primary | Green `#00FF00` |
| Danger / stop | Red `#FF0000` |
| Warn | Yellow `#FFFF00` |
| Info / speed accent | Magenta or Cyan (fixed in code) |
| Borders | White or Blue |

Wire **all** paint paths including **Brain** and **Profiling** (no leftover `0x4ed6a5` hardcodes when theme is DOGE). Profiling phase colors: fixed map to DOGE roles under DOGE; mint keeps soft phase hues.

### C. Shell IA

**Main tabs:** Chat | Brain | Profiling | **Tools**

**Slim rail:** brand · model summary · Start/Stop · live tiers/HWINFO when running · temp + max tokens · status · **Setup** button

**Tools panel:** full machine details, Doctor, Plan/Scan, HF install, theme, language, About

**Setup wizard:** full-main (or overlay) steps:

| Step | Plain-language title | Content |
|------|----------------------|---------|
| 1 | Welcome | What this app does; Start setup |
| 2 | Your machine | CPU, memory, graphics (probe + refresh) |
| 3 | Choose a model | Scan store, pick list, paste folder, or download |
| 4 | Check readiness | **Doctor** (quick + optional deep) and **placement plan** for the chosen model path: memory on GPU vs system RAM vs disk, plain-language pass/fail, what to fix before start |
| 5 | Look and feel | DOGE (default) vs mint; live preview |
| 6 | Ready | Summary; Finish; optional Start engine |

Step 4 is intentional product friction for clarity: users see whether the machine can host the model before they leave setup. Back still works if they change the model on step 3.

Back / Next / Skip / Finish. Re-open via **Setup**.

First-run:

```
launch → load native-ui.toml
  if !first_run_done && !skip env → wizard
  else → dashboard (Chat), apply theme + last model path
```

### D. Brain + Profiling fidelity (in scope)

Not a drive-by. Treat as first-class with theme work:

| Surface | Keep / improve |
|---------|----------------|
| **Brain** | Expert map grid, hover tips, full-grid toggle; colors from palette (tier/heat via DOGE or mint roles); empty state plain English when engine off |
| **Profiling** | Phase share bars, tok/s, stacked phases, turn table; phase colors from palette map; empty state when no live stats |

Regression: existing Brain/Profiling unit tests stay green; add palette-driven smoke if useful. No new 3-D Atlas.

### E. Determinate progress (install + inference)

Shared **progress strip** component (GPUI): filled bar + **percent** + **time left** text + short status.

#### Download / install

Today: status line only; `InstallProgress` has optional bytes/files; bytes often empty.

Ship:

1. Prefer **bytes transferred / total** when known → percent + ETA from rate.
2. Else **files done / total** → percent + ETA.
3. Else phase-based soft estimate only if we cannot get totals (document as degraded; still show spinner phase name in plain English: “Downloading…”, “Checking files…”).
4. Improve host/sys progress reporting so hub/CLI paths fill bytes or files when available (smallest change that enables determinate bar).

Cancel remains.

#### Inference / generate

Today: live tokens, tok/s, TTFT; **no** remaining/ETA.

Ship:

1. Denominator = configured **max output tokens** (and/or stop when done).
2. Percent ≈ `generated / max_output` (cap 100%; on finish show 100%).
3. Time left ≈ `(max - generated) / tok_s` when tok/s > 0; else “Calculating…” until first tokens.
4. Show bar under composer or in status during generate; hide when idle.

Plain copy: “Generating… 42% · about 1 min left”, not internal phase codes.

### F. Copy rules (user-friendly)

- Wizard and Tools: complete American English; no PROF/EMAP/HWINFO jargon in primary labels (optional detail under Advanced or muted secondary if needed for power users).
- Prefer: “Memory on GPU”, “Check model”, “Download model”, “Looks (theme)”.
- Hero points to **Setup** when no model.
- EN + IT keys for all new strings.

---

## Critical files

| Path | Why |
|------|-----|
| `crates/colibri-native/src/prefs.rs` | **New** TOML prefs |
| `crates/colibri-native/src/theme.rs` | DOGE + mint palettes |
| `crates/colibri-native/src/progress.rs` | **New** bar + % + ETA helpers |
| `crates/colibri-native/src/main.rs` | Wizard, Tools, slim rail, progress UI |
| `crates/colibri-native/src/host.rs` | Install progress plumbing; generate metrics |
| `crates/colibri-native/src/profiling_view.rs` | Theme-aware Profiling |
| `crates/colibri-native/src/atlas.rs` + Brain UI in main | Theme-aware Brain |
| `crates/colibri-native/src/i18n.rs` | Plain-language strings EN/IT |
| `crates/colibri-sys/src/model/install.rs` | Richer progress (bytes/files) when cheap |
| `Cargo.toml` (native) | `toml` dependency |
| fidelity + residual | Honesty |

---

## Reuse

| Existing | How |
|----------|-----|
| probe / doctor / plan / scan / install / start | Wizard + Tools |
| Brain grid + atlas tips | Re-theme, keep behavior |
| Profiling charts | Re-theme phase map |
| InstallProgress | Extend + determinate UI |
| Live tok/s / token counts | Generate progress math |
| i18n tables | New keys only |

---

## Implementation orchestration (effort 3)

Parent coordinates only. Work runs as **hierarchical subagents**:

| Role | Depth | Job |
|------|-------|-----|
| Parent (L1) | Chat | Goals, board, spawn/wait, read reports, status |
| Implementers (L2) | general-purpose | Disjoint slices below; red→green TDD; own report under `.agents/reports/` |
| Reviewers (L2) | general-purpose | After each major feature slice: **3 reviewers** (effort 3); findings → fix implementer |
| Process mop (L2) | general-purpose | After product waves: fmt + clippy + tests on touched packages |

**Role swap:** alternate primary implementer vs primary general reviewer across major features (prefs/theme → tools/wizard → progress) so the same persona is not always the writer.

**Parallel when safe:** prefs+theme palettes can start together if files disjoint; Tools rail and wizard step shell after theme tokens exist; install progress sys can parallel native progress widget.

## Steps

1. **Prefs (TOML)** — load/save/tests; default doge; first_run_done false.
2. **Theme palettes** — ThemeId, DOGE eight-color test, mint; wire root + shared tokens.
3. **Brain + Profiling theme pass** — all colors from palette; empty states plain English.
4. **Progress helpers** — pure functions: percent, ETA from (done, total, rate); unit tests.
5. **Install determinate UI** — bar/%/ETA; improve progress fields from sys/host as needed.
6. **Generate determinate UI** — bar/%/ETA from max tokens + tok/s.
7. **Tools panel + slim rail** — move clutter; keep chat-critical controls.
8. **Setup wizard** — steps 1–6 including **Check readiness** (doctor + plan); first-run gate; re-open Setup; Skip finishes first-run.
9. **Copy scrub** — jargon out of user-facing wizard/tools strings.
10. **Effort-3 review waves** — 3 reviewers on theme+Brain/PROF; tools+wizard; progress; then fix rounds.
11. **Docs + process mop** — fidelity, residual, fmt, clippy, tests.

---

## Risks

| Risk | Mitigation |
|------|------------|
| DOGE harsh for charts | Mint option; DOGE role map documented |
| ETA wrong early | “About…” wording; hide until rate known |
| Install without totals | File count fallback; plain phase text |
| Large main.rs | Prefer `wizard.rs` / `progress.rs` modules if size hurts |
| TOML parse errors | Fail soft to defaults + status line |

---

## Verification

| Slice | Proof |
|-------|--------|
| Prefs TOML | Round-trip; defaults; doge default theme |
| DOGE palette | All colors ∈ eight hexes |
| Brain/PROF theme | No mint-only hardcodes when doge selected (grep + visual) |
| Progress math | Unit tests for % and ETA edge cases (zero rate, done>total) |
| Install UI | Progress shows % when files or bytes present |
| Generate UI | During generate shows % vs max tokens when max known |
| Wizard | first_run false → wizard; Finish/Skip → dashboard |
| Regression | `cargo test -p colibri-native`; clippy; sys tests if install progress touched |

Manual: first launch wizard → machine → model → theme DOGE → Finish; Tools theme mint; download shows bar; chat generate shows bar; Brain/Profiling readable in both themes.

---

## Open questions

- **Q1 — Skip sets first_run_done?** Default **yes**.
- **Q2 — GBNF / session slot on rail or Tools only?** Default: **Tools** (keep temp + max tokens on rail).
- **Q3 — Generate percent against max tokens always, even if model stops early?** Default: **yes**; jump to 100% on Done.

---

## Board after approval (seed)

| Id | Work |
|----|------|
| `feat:native-setup-wizard` | First-run + re-open setup |
| `feat:native-tools-panel` | Tools tab + slim rail |
| `feat:native-doge-theme` | DOGE default + mint + Brain/PROF themed |
| `feat:native-determinate-progress` | Install + generate bar/%/ETA |
| `impl:native-prefs-toml` | native-ui.toml |
| `impl:theme-palettes` | DOGE/mint + wire |
| `impl:brain-prof-theme` | Brain + Profiling palette pass |
| `impl:progress-widget` | Shared bar + math tests |
| `impl:install-generate-progress` | Wire install + generate |
| `impl:tools-panel` | Move clutter |
| `impl:setup-wizard` | Step machine + steps |
| `impl:plain-copy-i18n` | EN/IT plain English |
| `impl:native-wizard-mop` | fmt/clippy/tests |

---

### Critical Files for Implementation

- `crates/colibri-native/src/prefs.rs` — TOML first-run + theme + last model
- `crates/colibri-native/src/theme.rs` — DOGE + mint
- `crates/colibri-native/src/progress.rs` — bar, percent, ETA
- `crates/colibri-native/src/main.rs` — wizard, Tools, shell
- `crates/colibri-native/src/profiling_view.rs` + Brain UI — theme fidelity
- `crates/colibri-sys/src/model/install.rs` — richer progress when needed
- Spec: https://github.com/SurmountSystems/specs/blob/main/0001_DOGE.md (DOGE v1.0.0, accessed 2026-08-11)
