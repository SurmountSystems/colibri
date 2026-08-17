# impl:brain-prof-theme — report

**Slice:** Theme-aware text inputs + Brain cell colors + Profiling verify
**Repo:** `/home/hunter/Projects/surmount/colibri`
**Plan:** `.agents/plans/plan-native-wizard-tools-theme.md`
**Prior:** `.agents/reports/impl-theme-palettes.md`
**DOGE:** [0001_DOGE.md](https://github.com/SurmountSystems/specs/blob/main/0001_DOGE.md) (accessed: 2026-08-11)

## Delivered

### 1. `text_input.rs` — palette-driven chrome

| Item | Status |
|------|--------|
| `TextInput` stores `ThemePalette` | done |
| `TextInput::new(..., palette)` takes themed args from parent | done |
| Cursor = `p.primary` | done |
| Selection = primary with alpha `0x40` | done |
| Text = `p.text`; placeholder = `p.muted` | done |
| Field bg = `p.secondary`; border = `p.border` | done |
| `set_palette` for live theme switch (Tools/wizard later) | done |
| No mint hex hardcodes left in this file | done |

Parent (`DesktopApp::new`) resolves `palette(theme_id)` once and passes it into every `TextInput::new`.

### 2. `host.rs` `brain_cell_rgb` — theme-aware

Signature:

```rust
pub fn brain_cell_rgb(theme: ThemeId, tier: u8, heat: u8, pulse: f32) -> u32
```

| Theme | Behavior |
|-------|----------|
| **Mint** | Soft SPA curve (web `TIER_RGB` + `lum = 0.35 + 0.65 * min(heat/24, 1)` + warm pulse). Prior unit pins preserved. |
| **DOGE** | Pure discrete map only. Every return value ∈ `DOGE_EIGHT`. |

**DOGE discrete map:**

| State | Color |
|-------|-------|
| hit pulse (`> 0.05`) | White |
| heat 0 | Black |
| disk warm / hot | Blue / Magenta |
| RAM warm / hot | Cyan / Magenta |
| VRAM warm / hot | Green / Yellow |

Heat hot threshold: `heat >= 12` (mid of the /24 curve).

### 3. Brain grid wire (`main.rs`)

```rust
brain_cell_rgb(self.theme_id, *tier, *heat, *pulse)
```

### 4. Profiling — already themed (verified)

- All phase paints use `phase.color_in(&p)` / `s.phase.color_in(p)`.
- No mint phase hardcodes on the paint path.
- Tests: `phase_colors_match_web` (mint), `phase_colors_doge_are_pure_eight`.

### 5. Empty states — plain English

| Key | Change |
|-----|--------|
| `profile.title` (en/it) | Em dash → colon |
| `profile.empty` (en/it) | Em dash → period; full sentence |
| `brain.waiting` | Already plain English (unchanged) |
| `profile.connectHint` | Already plain English (unchanged) |

### 6. Tests

| Test | Contract |
|------|----------|
| `doge_brain_cell_colors_are_pure_eight` | All tier×heat×pulse combos under DOGE ∈ eight hexes |
| `brain_cell_rgb_differs_by_tier` | Mint soft bases differ; DOGE warm tiers differ; cold≠hot VRAM |
| `brain_cell_rgb_heat_saturates_at_24` | Mint web heat/24 pins (unchanged values) |

### 7. Verify

```
cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets -- -D warnings   # clean
cargo test -p colibri-native --bin colibri-native             # 144 passed
```

## Files touched

| Path | Change |
|------|--------|
| `crates/colibri-native/src/text_input.rs` | ThemePalette chrome |
| `crates/colibri-native/src/host.rs` | Theme-aware `brain_cell_rgb` + doge purity test |
| `crates/colibri-native/src/main.rs` | Pass palette into inputs; theme_id into brain cells |
| `crates/colibri-native/src/i18n.rs` | Plain empty/title copy (en + it) |

## Out of scope / leftover

- Live theme switch UI (Tools/wizard) still future; `TextInput::set_palette` is ready.
- Brain legend copy still says "Mint = GPU" under all themes (i18n only; not part of this paint pass).
- Mint module consts in `theme.rs` remain documented aliases (not product paint).

## Residual closeout note

This closes the leftover sites listed in `impl-theme-palettes.md` for Brain grid cells, `brain_cell_rgb`, and `text_input` mint hardcodes under DOGE.
