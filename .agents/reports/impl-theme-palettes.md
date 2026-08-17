# impl:theme-palettes — report

**Slice:** DOGE default + mint theme palettes for colibri-native
**Repo:** `/home/hunter/Projects/surmount/colibri`
**Plan:** `.agents/plans/plan-native-wizard-tools-theme.md`
**DOGE:** [0001_DOGE.md](https://github.com/SurmountSystems/specs/blob/main/0001_DOGE.md) (accessed: 2026-08-11)

## Delivered

### 1. `crates/colibri-native/src/theme.rs` (refactored)

| Item | Status |
|------|--------|
| `ThemeId { Doge, Mint }` default **Doge** | done |
| `ThemePalette` with all prior role consts + badge washes + phase hues | done |
| `mint_palette()` = prior SPA/mint token values | done |
| `doge_palette()` = only the eight pure hexes | done |
| `palette(id) -> ThemePalette` | done |
| Layout consts `RAIL_WIDTH`, `HERO_MAX_W` kept | done |
| Module docs cite DOGE link + accessed 2026-08-11 | done |
| Mint-only module consts kept as **compat aliases** (mint values) for gradual migration / tests | done |
| `ThemeId::from_pref(ThemePref)` for prefs wire-up | done |

**DOGE role map (fixed in code):**

| Role | Color |
|------|-------|
| bg / panel / secondary / primary_wash / badge fills | Black `#000000` |
| text / user_body / assist_body | White `#FFFFFF` |
| muted / label | Cyan `#00FFFF` |
| primary / ok / primary_border / badge live border | Green `#00FF00` |
| primary_fg (on green) | Black |
| chip / tier_disk | Blue `#0000FF` |
| danger | Red `#FF0000` |
| warn / badge warn border | Yellow `#FFFF00` |
| speed / badge speed border | Magenta `#FF00FF` |
| border | White |
| phase_io_wait | Blue |
| phase_matmul | Green |
| phase_attention | Yellow |
| phase_lm_head | Magenta |
| phase_other | Cyan |

### 2. Unit tests

- `doge_palette_every_color_is_one_of_eight` — every `ThemePalette` role ∈ the eight DOGE hexes
- `doge_role_map_basics`, `mint_palette_matches_legacy_consts`, `palette_dispatch`, `theme_id_default_is_doge`, `theme_id_parse`
- `profiling_view::phase_colors_match_web` (mint via `color_in`)
- `profiling_view::phase_colors_doge_are_pure_eight`

### 3. Paint path wire-up

- `DesktopApp.theme_id: ThemeId` (default Doge)
- `DesktopApp::palette() -> ThemePalette` helper (`p.bg`, `p.primary`, …)
- Startup loads `prefs::load()` and sets `theme_id = ThemeId::from_pref(prefs.theme)` (honors `native-ui.toml` + `COLIBRI_THEME`)
- Also applies prefs locale + last model path when present (same load; no second prefs pass)
- **All** `main.rs` shell paint paths converted from mint module consts to `p.*` (root window, rail, chat, brain chrome, profiling chrome, badges, tabs, free helpers `prof_tile` / `share_bar_el` / `profile_table_el`)
- Profiling phases use `ProfilePhase::color_in(&palette)` so DOGE uses pure phase colors

### 4. Verify

```
cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets -- -D warnings   # clean
cargo test -p colibri-native --bin colibri-native             # 137 passed
```

## Remaining sites (for `impl:brain-prof-theme` / follow-up)

These still use mint (or non-palette) hardcodes and will look mint even under DOGE until themed:

| Location | What |
|----------|------|
| `text_input.rs` (~493, 509, 587, 595, 597) | Cursor / selection / field bg / border / text mint hexes (`0x4ed6a5`, `0xe9eff0`, `0x10171a`, `0x202a2f`) |
| `host.rs` `brain_cell_rgb` (~631+) | Tier/heat/pulse cell colors (soft mint family; tests pin RGB behavior) |
| `main.rs` Brain grid cells | Still calls `brain_cell_rgb` (not palette-driven heat map) |

Mint module consts in `theme.rs` remain as documented aliases only; product paint in `main.rs` no longer imports them.

## Out of scope this slice

- Tools panel / wizard UI for switching theme live
- Saving theme back to prefs from UI
- Full Brain heat-map recolor under DOGE (needs palette-aware `brain_cell_rgb` or equivalent)

## How to use in later slices

```rust
let p = self.palette(); // or palette(self.theme_id)
div().bg(rgb(p.bg)).text_color(rgb(p.text))
// phases:
.bg(rgb(phase.color_in(&p)))
// switch:
self.theme_id = ThemeId::Mint; // then cx.notify()
```
