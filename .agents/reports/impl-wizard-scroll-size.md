# Implement report: wizard UI sizing + scroll

**Date:** 2026-08-11
**Slice:** Wizard body scrolls; catalog / registry lists cap height; footer (Back / Skip / Next) stays reachable on tall steps (especially Model: catalog + download form + progress).

## What scrolled

| Surface | Behavior |
|---------|----------|
| **Wizard body** (`#wizard-body`) | `flex_1` + `min_h_0` + `overflow_scroll` inside the card. All steps share this (Welcome, Machine, Model, Doctor, Look and feel, Ready). |
| **Wizard footer** (`#wizard-nav`) | Outside the scroll body, `flex_shrink_0`, top pad only. Always visible when the card is height-capped. |
| **Wizard card** (`#wizard-card`) | `max_h_full` + `min_h_0` + `overflow_hidden` so the stage height bounds the card; stage is `overflow_hidden`. |
| **Supported models list** (`{wizard\|tools}-catalog-list`) | `max_h(CATALOG_LIST_MAX_H)` = **168px** + `overflow_scroll` + tight `gap_1` rows. Used by wizard Model and Tools. |
| **Registry lists** (`#wizard-reg-list`, `#wizard-readiness-reg-list`) | `max_h(REGISTRY_LIST_MAX_H)` = **112px** + `overflow_scroll`. |
| **Existing nested panels** | Machine body, doctor body, plan body still have their own max-height + scroll (unchanged numbers). |

## Size tweaks

| Change | Detail |
|--------|--------|
| Layout tokens | `theme.rs`: `WIZARD_CATALOG_LIST_MAX_H` (168), `WIZARD_REGISTRY_LIST_MAX_H` (112), `WIZARD_LIST_ROW_H` (28). |
| Pure helpers | `wizard.rs`: `CATALOG_LIST_MAX_H`, `REGISTRY_LIST_MAX_H`, `LIST_ROW_H`, `list_exceeds_max_height(row_count, row_h, max_h)`. Locked to theme via unit test. |
| Catalog rows | Still compact `px_2` / `py_1` / `text_xs` (rail density); list viewport gap `gap_1`. |
| Install form | `w_full` / `min_w_0`; pad uses `RAIL_CARD_PAD`; progress + status full width of panel. |
| Copy | Unchanged (product fidelity). Sharp corners still radius 0. |

## Structure (wizard paint)

```
#wizard-view  (flex_1, min_h_0, items_center, justify_center, overflow_hidden)
  #wizard-card  (max_w, max_h_full, flex_col, min_h_0, overflow_hidden)
    #wizard-body  (flex_1, min_h_0, overflow_scroll)  ← step content
    #wizard-nav   (flex_shrink_0)                     ← Back / Skip / Next
```

## Tests (red contract → green)

- `wizard::tests::catalog_list_max_h_matches_theme_token`
- `wizard::tests::catalog_row_count_triggers_scroll_path` (exact fit does not exceed; one more row does; 20 rows does)
- `wizard::tests::registry_list_scroll_threshold`
- Theme `spacing_tokens_are_positive_and_ordered` extended for the new list tokens

Layout itself is GPUI paint (hard to unit-test); pure height/threshold helpers cover the scroll-path contract.

## Verify commands

```bash
cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets -- -D warnings
cargo test -p colibri-native --bin colibri-native
```

**Results (this run):** fmt clean; clippy `-D warnings` clean; **229** tests passed.

## Files touched

- `crates/colibri-native/src/theme.rs` — list max-height / row tokens + spacing test pins
- `crates/colibri-native/src/wizard.rs` — pure list height helpers + tests
- `crates/colibri-native/src/main.rs` — wizard card/body/nav scroll structure; catalog list viewport; registry list viewports; install form width/pad

## Residual / not claimed

- Live pixel proof on a short window still needs operator eyeball (Model step with download open + install progress).
- Nested scroll (body + catalog + doctor panels) can feel busy if all three overflow at once; acceptable for this slice.
- Tools view already had full-page `overflow_scroll`; catalog list cap still applies there for density.
