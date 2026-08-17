# Report: native UI sharp corners + stop button + path overflow

**Date:** 2026-08-11
**Crate:** `colibri-native`
**Scope:** Operator screenshot feedback (wizard over DOGE shell): hard edges, stop states, model path overflow.

## What changed

### 1. Sharp corners (border radius 0)

- Added `theme::CORNER_RADIUS = 0.0` with unit test `corner_radius_is_sharp`.
- Removed every product `.rounded_md|sm|lg|xl|full()` call from:
  - `crates/colibri-native/src/main.rs` (panels, buttons, tabs, wizard card, hero mark, legend dots, etc.)
  - `crates/colibri-native/src/text_input.rs` (field chrome)
- Default GPUI radius is already 0; chrome now relies on that (1px solid borders only).
- Grep audit: no remaining intentional `.rounded_*` in product code (only docs on `CORNER_RADIUS`).

**Platform note:** Window chrome / OS decorations are outside GPUI style chains; product elements are sharp rectangles.

### 2. Stop engine / chat Stop button

Pure paint helpers in `main.rs`:

| Helper | Usable | Idle |
|--------|--------|------|
| `stop_button_paint(p, can_stop)` | solid `danger` fill + border, `primary_fg` text | panel fill, 1px `danger` border, `danger` text (hollow) |
| `start_button_paint(p, engine_live)` | solid `ok` when engine down | hollow `ok` outline when live (parallel polish) |

Wire-up:

- Rail **Stop engine**: `can_stop = engine_is_live()`
- Chat **Stop**: `can_stop = generating`
- DOGE danger is pure `#FF0000` (`DOGE_RED`) via palette.

### 3. Model path / text overflow

- Placeholder shortened: `"Model path (COLIBRI_MODEL / COLI_MODEL)"` → `MODEL_PATH_PLACEHOLDER = "Model folder path"` (fits slim rail).
- `TextInput`: `min_w_0` + `overflow_hidden` on field box; paint uses `with_content_mask` so glyphs never spill past bounds.
- Rail/Tools path display and model summary: `overflow_hidden` + `whitespace_nowrap` + `text_ellipsis` + `min_w_0`.

## Tests (red/green contracts)

| Test | Contract |
|------|----------|
| `theme::tests::corner_radius_is_sharp` | `CORNER_RADIUS == 0.0` |
| `chrome_tests::stop_button_usable_is_solid_danger` | solid danger fill/border; DOGE pure red |
| `chrome_tests::stop_button_idle_is_hollow_danger_outline` | panel fill, danger border/text, not solid |
| `chrome_tests::start_button_solid_when_engine_down_hollow_when_live` | start parallel |
| `chrome_tests::model_path_placeholder_is_short_and_clear` | ≤24 chars, no env-var overflow |

## Verify

```text
cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets -- -D warnings   # ok
cargo test -p colibri-native                                   # 169 passed
```

## Files touched

- `crates/colibri-native/src/theme.rs` — `CORNER_RADIUS` + test
- `crates/colibri-native/src/main.rs` — paint helpers, stop/start wire, path truncate, drop rounded
- `crates/colibri-native/src/text_input.rs` — sharp field, clip paint

## Non-goals (not done)

- Wizard content redesign, i18n sweep of remaining EN-only placeholders
- Mint soft-radius reintroduction (mint also uses hard edges for chrome consistency)
- OS window corner policy
