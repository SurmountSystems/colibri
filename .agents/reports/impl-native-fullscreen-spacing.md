# Report: colibri-native fullscreen launch + UI spacing

## Goal

Default launch fullscreen; rail and wizard less cramped; keep hard edges and Stop hollow/solid red contract.

## What shipped

### 1. Fullscreen on start

- **API:** GPUI `WindowBounds::Fullscreen(restore_bounds)` via `WindowOptions.window_bounds`.
- **Restore size:** centered 1280×820 (same as the old windowed default). Leaving fullscreen restores that size.
- **Not maximize:** GPUI has `WindowBounds::Maximized` too; product request was full screen, and Linux Wayland/X11 support fullscreen cleanly in gpui 0.2.2 (`toggle_fullscreen` on open).
- **Dev override:** `COLIBRI_WINDOWED=1` (also `true` / `yes` / `windowed`, case-insensitive) opens centered windowed instead.
- **Helpers (tested):**
  - `launch_window_mode_from_env(Option<&str>) -> LaunchWindowMode`
  - `initial_window_bounds(mode, restore) -> WindowBounds`

Documented in `crates/colibri-native/README.md` under How to run.

### 2. Spacing tokens (`theme.rs`)

| Token | Value (px) | Use |
|-------|------------|-----|
| `RAIL_PAD` | 20 | Left rail outer padding (was ~16 / `p_4`) |
| `RAIL_SECTION_GAP` | 16 | Between brand / Engine / Chat settings / footer (was ~12) |
| `RAIL_CARD_PAD` | 16 | Engine + inference card inner pad (was ~12) |
| `RAIL_CARD_GAP` | 12 | Inside cards / button rows (was ~8) |
| `BTN_PAD_X` / `BTN_PAD_Y` | 12 / 8 | Start/Stop and primary chrome buttons (was ~8 / 4) |
| `WIZARD_MAX_W` | 720 | Card max width (was 640) |
| `WIZARD_STAGE_PAD` | 32 | Empty margin around card (was ~24) |
| `WIZARD_CARD_PAD` | 32 | Card inner padding (was ~24) |
| `WIZARD_CONTENT_GAP` | 16 | Step label / title / body rhythm (was ~12) |
| `FIELD_PAD_X` / `FIELD_PAD_Y` | 12 / 8 | Text field chrome (was ~8 / 4) |
| `FIELD_MIN_H` | 36 | Field height (was 30) |

Hard edges unchanged: `CORNER_RADIUS = 0`. Stop paint helpers untouched (hollow danger when idle, solid when live).

### 3. Call sites

- `left_rail`: tokens for pad, section gap, engine card, Start/Stop, Setup footer.
- `rail_inference_panel`: card pad/gap tokens.
- `wizard_view`: stage pad, card pad/max width, content gap, nav button pad.
- `text_input::Render`: field pad and min height tokens.

## Verify

```text
cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets -- -D warnings   # exit 0
cargo test -p colibri-native                                  # 173 passed
```

New tests:

- `theme::tests::spacing_tokens_are_positive_and_ordered` (const asserts)
- `chrome_tests::launch_window_mode_defaults_to_fullscreen`
- `chrome_tests::launch_window_mode_windowed_override`
- `chrome_tests::initial_window_bounds_matches_mode`

## Files

- `crates/colibri-native/src/theme.rs` — spacing tokens + tests
- `crates/colibri-native/src/main.rs` — launch mode, rail/wizard spacing
- `crates/colibri-native/src/text_input.rs` — field padding
- `crates/colibri-native/README.md` — fullscreen + `COLIBRI_WINDOWED`

## Notes

- No redesign of wizard copy or step machine.
- No rounded corners reintroduced.
- Optional maximize path not used; if a compositor mishandles exclusive fullscreen, `COLIBRI_WINDOWED=1` is the escape hatch for dev; product can later prefer Maximized only if field reports demand it.
