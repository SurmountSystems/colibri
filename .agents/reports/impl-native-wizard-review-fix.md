# Implement: native wizard / tools / DOGE / progress review fixes

**Date:** 2026-08-11
**Repo:** `/home/hunter/Projects/surmount/colibri`
**Scope:** effort-3 review findings from `review-native-wizard-theme.md`, `review-native-wizard-tools.md`, `review-native-wizard-progress.md`

## Summary

All **high** review items and the practical **medium** list were fixed with red/green unit tests where contracts are pure. Full generate % redesign stayed out of scope.

## Findings: fixed vs deferred

### High (must fix) — all fixed

| # | Finding | Fix |
|---|---------|-----|
| 1 | Skip/Finish save-error clobber | `persist_prefs_status` returns `bool`; success status applied only via `wizard_may_set_success_status` + `wizard_complete_success_status`. Save failure keeps `"Could not save settings: …"`. |
| 2 | `format_plan` lab jargon | New `format_plan_readiness` / plain bottleneck labels for wizard + Tools. Lab dump kept as unused `format_plan_lab`. Empty path copy no longer leads with env var soup. |
| 3 | Done 100% cleared same drain | Generate and install drains keep the 100% strip on Done (`keep_done_progress`); clear only on Error. Next generate/install replaces the strip. |
| 4 | CLI install stuck at 0% | UI `install_options_for_ui` now `prefer_cli: false` (hub path emits file/byte counters). No-counter / CLI path uses phase floor (`install_phase_percent_floor`, download ≥ 5%). Inspect/register use 95%/98% floors. |
| 5 | Engine status jargon | Status copy is plain: `"Engine ready (in-process). Expert map and live stats update while you chat."` |

### Medium — fixed

| # | Finding | Fix |
|---|---------|-----|
| 6 | Inactive tabs `0x000000` | `tab_bg_color` uses `p.panel` when inactive, `p.primary` when active. |
| 7 | text_input DOGE selection midtone | DOGE primary → solid `rgb(primary)`; mint keeps soft alpha wash. |
| 8 | Brain legend mint-shaped under DOGE | Theme keys `brain.legend.doge` / `brain.legend.mint` (EN+IT); brain panel uses `brain_legend_key(theme_id)`. |
| 9 | Post-hub 100% then 0% on inspect/register | Phase floors 95% / 98% / 100% in `progress_view_for_install`. |
| 10 | Shared `model_input` double-parent | `model_input_site` mounts the editor in exactly one of Rail / Tools / Wizard per frame; others show path text. |

### Deferred / out of scope (unchanged)

| Item | Notes |
|------|--------|
| Full generate % redesign | Still max-tokens denominator; optional note only. Done strip now paints 100%. |
| Hub mid-file byte granularity | Still updates per file before download of that shard (sys path). |
| Rail density / sticky Setup footer | Not expanded. |
| Deep doctor on wizard readiness | Still Tools-only deep. |
| Tok/s TTFT window for ETA | Not changed (dead elapsed arithmetic removed in gen drain). |

## Tests added

| Test | Module |
|------|--------|
| `wizard_complete_status_preserves_save_error` | `wizard` |
| `complete_wizard_then_save_round_trips_first_run` | `wizard` |
| `readiness_plan_copy_is_plain_english` | `host` |
| `install_options_prefer_hub_for_progress` | `host` (replaces prefer_cli_true) |
| `progress_view_for_install_cli_no_counters_not_stuck_at_zero` | `host` |
| `progress_view_inspect_register_stay_high` | `host` |
| `doge_selection_fill_is_pure_eight_or_opaque_primary` | `text_input` |
| `mint_selection_uses_soft_alpha_path` | `text_input` |
| `inactive_tab_fill_is_palette_not_literal_black` | `chrome_tests` (main) |
| `brain_legend_key_is_theme_aware` | `chrome_tests` |
| `model_input_single_parent_per_frame` | `chrome_tests` |
| `engine_ready_status_has_no_lab_jargon` | `chrome_tests` |

## Commands + exit codes

| Command | Exit |
|---------|------|
| `cargo fmt -p colibri-native` | 0 |
| `cargo clippy -p colibri-native --all-targets -- -D warnings` | 0 |
| `cargo test -p colibri-native` | 0 (**164** passed, 0 failed) |

## Key product paths touched

- `crates/colibri-native/src/wizard.rs` — success-status helpers + save round-trip test
- `crates/colibri-native/src/main.rs` — skip/finish, drains, tabs, legend, model input site, engine status
- `crates/colibri-native/src/host.rs` — plain plan, install progress floors, hub-prefer UI options
- `crates/colibri-native/src/text_input.rs` — DOGE solid selection
- `crates/colibri-native/src/i18n.rs` — theme-aware brain legends

No git commit or stage.
