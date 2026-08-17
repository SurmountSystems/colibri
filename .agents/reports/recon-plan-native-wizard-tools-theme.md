# Recon: native wizard, tools panel, dual themes

**Date:** 2026-08-11 · read-only · crate `colibri-native` (GPUI)

## Current shell layout (bullet)

- Single window `DesktopApp` (`main.rs`): root `bg(BG)` · optional About strip · **left rail (~292px) + main column**.
- **Left rail sections**
  - Brand (mark + `brand.name` / tagline)
  - **Lifecycle:** Machine (summary/Details + Refresh) · Doctor (Run checks / Deep check) · Plan/model (path input, Plan, Start engine, Scan models, registry rows)
  - **Runtime:** proportional tier bar · live HWINFO strip · **Inference** (temp, max tokens, reasoning, KV slot, GBNF)
  - **HF install** (`feature = "install"`, default on): repo/revision/dest/min-free + Install/Cancel
  - Footer: Language toggle (en↔it) · About · status line
- **Main:** top tabs **Chat | Brain | Profiling** + badges (tokens, tok/s, TTFT, slot) + Clear
- **Empty chat:** `chat_hero` (title, subtitle, description, 3 suggested prompts) when `chat_log` empty; composer always present
- **Bootstrap:** `probe_machine` + idle/real doctor + plan hint; model default only from `COLIBRI_MODEL` / `COLI_MODEL` (no disk prefs)
- **No** first-run wizard, **no** separate Tools tab, **no** light theme (dark mint only; web SPA also `color-scheme: dark`)

## Critical files

| Path | Role |
|------|------|
| `crates/colibri-native/src/main.rs` | Window, `DesktopApp` state, rail/main render, hero, bootstrap |
| `crates/colibri-native/src/theme.rs` | Mint dark tokens + `RAIL_WIDTH` / `HERO_MAX_W` |
| `crates/colibri-native/src/host.rs` | Probe, doctor, plan, registry, install, engine lifecycle |
| `crates/colibri-native/src/i18n.rs` | `Locale`, `t` / `t_fmt`, EN+IT tables |
| `crates/colibri-native/src/text_input.rs` | Inputs (placeholder when empty) |
| `crates/colibri-native/src/profiling_view.rs` / `atlas.rs` | Profiling + Brain (not wizard-critical) |
| `crates/colibri-sys` `probe` / `doctor` / `model/registry` / `paths` / `install` | Backing APIs |
| `web/src/index.css` | Token source of truth for dark family |

## What to reuse for wizard steps

| Wizard step | Existing surface |
|-------------|------------------|
| Welcome / empty | `chat_hero` + `hero.*` i18n (copy already cold-start aware) |
| Machine check | `probe_machine`, `format_machine` / Details, `refresh_probe` |
| Doctor readiness | `run_shallow_doctor` / deep; empty path → `format_idle_doctor_checklist` (Idle, not Fail) |
| Model path | `model_input`, `env_model_path`, Plan empty copy |
| Scan / pick | `registry_scan_roots`, `scan_model_registry`, `format_*_registry_*`, select row → set path |
| Install (optional) | HF form + `install_async` / cancel / free-space gate |
| Plan + start | `run_plan`, `start_engine` |
| Locale | `cycle_locale` / `Locale` (session-only today) |

Host formatters + unit tests already cover idle doctor and empty scan recovery; wizard should call those, not reimplement sys doctor.

## Theme hook points

- Tokens are **`pub const u32`** in `theme.rs`; UI uses `rgb(BG)`, `rgb(PANEL)`, … scattered across `main.rs` (and phase colors in profiling).
- Root window: `.bg(rgb(BG)).text_color(rgb(TEXT))`; rail hardcodes `0x080b0d` in one place (duplicate of `BG`).
- **No** `Theme` struct, runtime switch, or light palette. Dual theme needs a small palette type + pass-through (or module-level switch) before paint; profiling phase hues can stay fixed.
- Web has no light scheme either; inventing light is product design, not parity.

## Persistence options for first-run + theme choice

**Today:** no UI prefs file. Model path = env only. Locale defaults `En`. Engine/placement still env + model dir (`colibri-sys` config is env-driven, not TOML app config). Store path: XDG `~/.local/share/colibri/models` (+ env). Tuning JSON exists under `~/.config/colibri/tuning/` for autotune, not shell chrome.

**Pragmatic options for wizard/theme:**

1. **New small JSON** e.g. `~/.config/colibri/native-ui.json` (`first_run_done`, `theme`, `locale`, optional `last_model_path`) via existing `serde`/`serde_json` deps
2. **Reuse config dir** next to tuning under `~/.config/colibri/`
3. **Env overrides** for CI/headless (`COLIBRI_THEME`, skip wizard) without replacing disk prefs
4. **Do not** overload model `config.json` or process-wide `ColibriConfig` for UI chrome

## Risks / non-goals

- **Risks:** theme requires many call-site updates unless tokens are centralized; wizard must not block Machine/Doctor for operators who already set `COLIBRI_MODEL`; doctor checklist strings still EN-only; install gated by feature
- **Non-goals:** Tauri/web light mode; whole-disk model search; rewriting sys doctor CLI; tools panel as full settings rewrite of rail (prefer regrouping rail + optional modal/tab); pixel-perfect CSS

## i18n pattern (for new strings)

- Add keys to both `EN` and `IT` in `i18n.rs`; use `self.tr("key")` / `tr_fmt` with `{{name}}`
- Tests: completeness loop over keys in `i18n.rs`
- Prefer product copy keys (`wizard.*`, `tools.*`, `theme.*`); leave sys doctor body English unless product asks

## Suggested product shape (planning only)

- **First-run wizard:** modal or full-main overlay until prefs say done; steps = probe → path/scan/install → plan/doctor → start optional
- **Tools panel:** new main tab or rail section for locale, theme, About, env hints; keep lifecycle actions on rail for daily use
- **Dual themes:** light palette beside dark in `theme.rs`; load/save with first-run prefs
