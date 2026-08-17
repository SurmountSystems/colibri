# Review: native wizard / theme campaign (reviewer 1 of 3)

**Scope:** `theme.rs` DOGE purity, prefs TOML path, `brain_cell_rgb` DOGE path, themed `text_input`, reports `impl-theme-palettes.md` / `impl-brain-prof-theme.md` / `impl-native-prefs-toml.md`
**Repo:** `/home/hunter/Projects/surmount/colibri`
**Date:** 2026-08-11
**Mode:** read-only product code (no edits)

## Verdict

Core DOGE palette and `brain_cell_rgb` DOGE path are pure and covered by unit tests. Prefs path is correct (`native-ui.toml`, not a wrong `prefs.toml`). Wizard first-run / skip flags are largely sound.

Real product bugs remain around **mint inactive-tab hardcode**, **selection soft-blend under DOGE**, **brain legend copy under DOGE**, and **wizard skip/finish swallowing prefs save errors**. Several high-value contracts have **no tests**.

---

## What looks correct

| Area | Evidence |
|------|----------|
| DOGE role palette purity | `doge_palette()` uses only `DOGE_*` consts; `doge_palette_every_color_is_one_of_eight` walks all 30 roles |
| DOGE brain cells | `brain_cell_rgb_doge` returns only eight hexes; `doge_brain_cell_colors_are_pure_eight` exhausts tier×heat×pulse |
| Profiling phases under DOGE | `ProfilePhase::color_in` + `phase_colors_doge_are_pure_eight` |
| Prefs file path | `~/.config/colibri/native-ui.toml` (XDG) / `%LOCALAPPDATA%\colibri\native-ui.toml`; name `native-ui.toml` matches plan |
| Defaults | Missing/corrupt → doge, `first_run_done=false`; unknown theme → doge |
| First-run gate | `should_show_wizard_with_skip(!first_run_done && !skip)`; `DesktopApp::new` opens wizard from that |
| Skip/Finish flag | `complete_wizard` sets `first_run_done`; shell sets `self.first_run_done` and persists |
| Theme wire | Startup `ThemeId::from_pref(prefs.theme)`; inputs get `palette(theme_id)`; `set_palette` on live switch |
| text_input chrome | Uses `p.primary` / `p.muted` / `p.text` / `p.secondary` / `p.border` (no mint hex left in file) |

---

## Bugs (severity + location)

### M1 — Medium: inactive tab fill hardcodes pure black

**File:** `crates/colibri-native/src/main.rs:1490`

```rust
.bg(rgb(if active { p.primary } else { 0x0000_0000 }))
```

`gpui::rgb` is opaque RGB. Inactive tabs paint **solid black**, not transparent. Under **mint** (`p.bg = #080b0d`), inactive tabs become pure-black chips on a near-black teal surface (visible blot). Under DOGE it is invisible (bg is black). Breaks mint polish and is a leftover non-palette paint path.

**Fix:** inactive → no fill, or `p.bg` / `p.panel`, never a bare literal.

**Suggested failing test** (pure helper or extract fill token):

```rust
#[test]
fn inactive_tab_fill_is_palette_not_literal_black() {
    // Contract: inactive tab chrome must come from ThemePalette, not 0x000000.
    // Prefer extracting:
    //   fn tab_bg(p: &ThemePalette, active: bool) -> u32
    let mint = mint_palette();
    assert_ne!(tab_bg(&mint, false), 0x0000_0000);
    assert_eq!(tab_bg(&mint, false), mint.bg); // or panel / transparent policy
    assert_eq!(tab_bg(&doge_palette(), true), doge_palette().primary);
}
```

---

### M2 — Medium: text selection is a soft midtone under DOGE

**File:** `crates/colibri-native/src/text_input.rs:25-28`, used at `:530`

```rust
fn selection_rgba(primary: u32) -> u32 {
    ((primary & 0x00ff_ffff) << 8) | 0x40
}
```

Alpha `0x40` over black composites to a **dim green wash** for DOGE primary (`#00FF00`). Plan non-goal: soft midtones under DOGE. Palette purity tests never see this because selection is not a `ThemePalette` role.

**Fix options:** DOGE selection = solid primary (or white/black invert) without alpha; mint may keep soft selection.

**Suggested failing test:**

```rust
#[test]
fn doge_selection_fill_is_pure_eight_or_opaque_primary() {
    let primary = doge_palette().primary; // DOGE_GREEN
    let packed = selection_rgba(primary); // if made pub(crate) for tests
    // Either assert opaque primary in DOGE path, or assert no alpha wash:
    // e.g. selection_for_theme(ThemeId::Doge, primary) ∈ DOGE_EIGHT
    let c = selection_rgb_for_theme(ThemeId::Doge, primary);
    assert!(DOGE_EIGHT.contains(&c), "0x{c:06X}");
}
```

---

### M3 — Medium: brain legend copy is mint-shaped under DOGE

**File:** `crates/colibri-native/src/i18n.rs:196-197` (IT: `:404-405`)

EN: `"Gray = disk · Blue = system RAM · Green = GPU · Bright = hot · Flash = hit"`

Actual **DOGE** map (`host.rs:677-710`):

| State | Color |
|-------|-------|
| cold (heat 0) | Black (not gray) |
| disk warm / hot | Blue / Magenta |
| RAM warm / hot | **Cyan** / Magenta (not blue) |
| VRAM warm / hot | Green / Yellow |
| pulse | White (not generic “Bright”) |

Legend mis-teaches the DOGE heat map. `impl-brain-prof-theme.md` residual is slightly wrong (says “Mint = GPU”; product text says “Green = GPU”) but correctly flags legend debt.

**Suggested failing test:**

```rust
#[test]
fn brain_legend_doge_matches_discrete_map_copy() {
    // When theme is DOGE, legend must not claim Gray disk / Blue RAM.
    let en = t(Locale::En, "brain.legend"); // or theme-aware key
    // Prefer split keys: brain.legend.doge vs brain.legend.mint
    assert!(!en.to_ascii_lowercase().contains("gray"), "{en}");
    assert!(en.to_ascii_lowercase().contains("black") || en.contains("Cyan"), "{en}");
}
```

(Or pin exact DOGE string once product copy is chosen.)

---

### M4 — Medium: wizard Skip/Finish hides prefs save failures

**File:** `crates/colibri-native/src/main.rs:499-526`

```rust
complete_wizard(...);
self.first_run_done = true;
self.persist_prefs_status(cx);  // may set "Could not save settings: …"
self.status = "Setup skipped · …"; // always overwrites
```

Same pattern for Finish (`"Setup complete"`). If `save()` fails, UI reports success while `first_run_done` is true only in memory; next cold start shows the wizard again with no error.

**Fix:** only set success status when `save` Ok; keep error status from `persist_prefs_status`.

**Suggested failing test** (extract pure status helper):

```rust
#[test]
fn wizard_complete_status_preserves_save_error() {
    // fn status_after_wizard_complete(save_ok: bool, finish: bool) -> &'static str
    assert!(status_after_wizard_complete(false, false).contains("Could not save"));
    assert_eq!(status_after_wizard_complete(true, false), "Setup skipped · you can open Setup anytime");
    assert_eq!(status_after_wizard_complete(true, true), "Setup complete");
}
```

---

### L1 — Low: DOGE text field fill equals panel/bg (black on black)

**File:** `theme.rs:252` (`secondary: DOGE_BLACK`) + `text_input.rs:617` (field bg = `p.secondary`)

Fields rely only on white border for separation. Not a purity bug; weak affordance under DOGE. Chip blue for fill might read better.

---

### L2 — Low: `set_theme_id` seeds snapshot with Doge then `apply_theme`

**File:** `main.rs:443-449`

```rust
let mut snap = shell_prefs_snapshot(..., ThemeId::Doge.to_pref(), ...);
apply_theme(&mut snap, id.to_pref());
```

Final theme is correct (`apply_theme` overwrites). Misleading for readers; prefer `id.to_pref()` in the snapshot directly (as `persist_prefs` already does).

---

### L3 — Low: dead local prefs on skip/finish

**File:** `main.rs:499-507`, `:514-522`

Local `NativePrefs` is mutated by `complete_wizard` then discarded; persistence uses `self.*` + `persist_prefs_status`. Works only because `self.first_run_done = true` is duplicated. Easy to regress if one path is edited alone.

**Suggested failing test:**

```rust
#[test]
fn complete_wizard_then_save_round_trips_first_run() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("native-ui.toml");
    let mut prefs = NativePrefs::default();
    let mut w = WizardState::open_at_start();
    complete_wizard(&mut prefs, &mut w);
    prefs.save_to_path(&path).unwrap();
    let loaded = load_from_path(&path);
    assert!(loaded.first_run_done);
    assert!(!w.open);
}
```

(This is a missing integration-style unit test, not currently red against existing pure `complete_wizard` tests.)

---

### L4 — Low / design: `COLIBRI_SKIP_WIZARD` does not set `first_run_done`

Documented in `prefs.rs` and plan. Not a bug unless product intent is permanent skip. No disk write → wizard returns next launch without env. Tests cover the gate with `should_show_wizard_with_skip`.

---

### Info: prefs path is not wrong

Reviewer prompt mentioned “prefs.toml”. Product + plan use **`native-ui.toml`** under `…/colibri/`. Path logic in `platform_default_prefs_path` matches plan section A. No path bug found.

---

## Missing tests (high value)

| Gap | Why it matters | Suggested test name / location |
|-----|----------------|--------------------------------|
| Inactive tab fill token | M1 | `main` helper or small chrome unit: `inactive_tab_fill_is_palette_not_literal_black` |
| DOGE selection purity | M2 | `text_input` or theme: `doge_selection_fill_is_pure_eight` |
| DOGE brain **role map** pins | Purity only; wrong colors could still pass | `host`: `doge_brain_cell_role_map_pins` assert heat0 black, pulse white, disk warm blue, RAM warm cyan, VRAM hot yellow |
| `ThemeId::from_pref` / `to_pref` round-trip | Wire-up for prefs | `theme`: `theme_id_pref_round_trip` |
| complete + save first_run | L3 | `wizard`: `complete_wizard_then_save_round_trips_first_run` |
| Wizard complete status on save fail | M4 | pure status helper test |
| text_input palette roles under DOGE | Regression if someone reintroduces mint hexes | `doge_text_input_roles_are_pure_eight` over secondary/border/text/muted/primary |
| Brain legend DOGE | M3 | `i18n`: theme-aware legend string pins |

Existing good coverage: prefs load/save/skip parse, doge palette purity, doge brain purity, mint heat/24, wizard step machine, theme switch temp-dir save.

---

## Report honesty (impl reports)

| Report | Issue |
|--------|--------|
| `impl-theme-palettes.md` | “Remaining sites” still lists `text_input` / `brain_cell_rgb` as mint hardcodes. **Stale** after `impl-brain-prof-theme` closed those paint paths. Do not treat as open product residual without re-verify. |
| `impl-brain-prof-theme.md` | Paint claims match code. Legend residual phrasing (“Mint = GPU”) does not match live EN string; real issue is mint-shaped **Gray/Blue/Bright** legend under DOGE. |
| `impl-native-prefs-toml.md` | Accurate for path/API. Header “Wizard: not built” is **stale** (wizard + Tools shipped in `impl-tools-and-wizard.md`). |

---

## DOGE purity summary (theme.rs)

`doge_palette()` audit (all roles ∈ eight):

| Role group | Mapping |
|------------|---------|
| Surfaces | black |
| Text / bodies | white |
| Muted / label | cyan |
| Primary / ok / live border | green |
| primary_fg | black |
| chip / tier_disk | blue |
| danger | red |
| warn / warn border | yellow |
| speed / speed border | magenta |
| border | white |
| phases | blue, green, yellow, magenta, cyan |

No non-DOGE literals inside `doge_palette()`. Mint-only module consts remain aliases only and are not used on main paint paths grepped for hardcoded `0x..` outside `theme.rs`.

---

## Priority fix order

1. **M1** inactive tab fill (mint regression, one-liner)
2. **M4** wizard status after save fail (trust / first-run trap)
3. **M2** DOGE selection midtone (spec)
4. **M3** brain legend copy under DOGE
5. Missing tests for doge brain role map + complete+save round-trip
6. L1–L3 polish

---

## Suggested red tests (copy-paste set)

```rust
// host.rs — pins discrete DOGE map (beyond purity)
#[test]
fn doge_brain_cell_role_map_pins() {
    use crate::theme::{DOGE_BLACK, DOGE_BLUE, DOGE_CYAN, DOGE_GREEN, DOGE_MAGENTA, DOGE_WHITE, DOGE_YELLOW, ThemeId};
    assert_eq!(brain_cell_rgb(ThemeId::Doge, 0, 0, 0.0), DOGE_BLACK);
    assert_eq!(brain_cell_rgb(ThemeId::Doge, 2, 0, 0.0), DOGE_BLACK);
    assert_eq!(brain_cell_rgb(ThemeId::Doge, 1, 8, 0.0), DOGE_CYAN);
    assert_eq!(brain_cell_rgb(ThemeId::Doge, 0, 8, 0.0), DOGE_BLUE);
    assert_eq!(brain_cell_rgb(ThemeId::Doge, 2, 8, 0.0), DOGE_GREEN);
    assert_eq!(brain_cell_rgb(ThemeId::Doge, 2, 12, 0.0), DOGE_YELLOW);
    assert_eq!(brain_cell_rgb(ThemeId::Doge, 0, 12, 0.0), DOGE_MAGENTA);
    assert_eq!(brain_cell_rgb(ThemeId::Doge, 1, 1, 0.1), DOGE_WHITE);
}

// theme.rs
#[test]
fn theme_id_pref_round_trip() {
    for id in [ThemeId::Doge, ThemeId::Mint] {
        assert_eq!(ThemeId::from_pref(id.to_pref()), id);
    }
}

// wizard.rs
#[test]
fn complete_wizard_then_save_round_trips_first_run() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("native-ui.toml");
    let mut prefs = NativePrefs::default();
    let mut w = WizardState::open_at_start();
    complete_wizard(&mut prefs, &mut w);
    prefs.save_to_path(&path).unwrap();
    assert!(load_from_path(&path).first_run_done);
    assert!(!w.open);
}
```

---

## Out of scope notes

- Progress strip already uses palette (`progress_strip_el`).
- `COLIBRI_THEME` env override on every `load()` is intentional; can surprise users who saved mint while env forces doge (document, not fix unless product says so).
- No product code edited in this review.
