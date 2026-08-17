# Install progress bar fill matches percent label

**Date:** 2026-08-11
**Status:** done
**Board:** `bug:install-progress-bar-pct`

## Problem

Operator screenshot: label said `Downloading... 4% · about 1 hour left` but the bar paint did not read as ~4% (fat green slab + blue remainder).

## Root cause (verified in code)

1. **GPUI flex layout bug** in `progress_strip_el` (`main.rs`):
   - Fill and rest children both used `.flex_grow()` (grow factor 1.0) with
     `flex_basis(px(pct))` / `flex_basis(px(rest))`.
   - Free space after bases is split **evenly**, so at 4% on a ~400px track
     fill was ~basis 4 + half of remaining ≈ tens of percent of the track,
     not 4%.

2. **Track color (DOGE):** track used `p.chip` which is **DOGE blue**, with
   fill `p.primary` (green). Remainder painted as a second bright
   progress-like color → dual-fill look.

Phase floor math in `host.rs` was **not** the bug here: when counters exist,
label and `ProgressView.percent` already match real download percent (4%).
Floor only applies when there are no counters (CLI path).

## Fix

### Pure helper (`progress.rs`)

- `fill_fraction(percent: u8) -> f32` → `percent.min(100) / 100.0`
- `ProgressView::fill_fraction()` delegates to the same helper
- Label percent and fill fraction are one number

### Paint (`main.rs` `progress_strip_el`)

- Track: single dark unfilled color `p.panel` + border
- Fill child only: `.w(relative(frac))` + `p.primary` (DOGE green)
- No second “rest” flex child, no `flex_grow` for progress width

## Tests (TDD)

| Test | Asserts |
|------|---------|
| `fill_fraction_zero_four_full` | 0 → 0.0, 4 → 0.04, 100 → 1.0 |
| `fill_fraction_caps_above_100` | 200 / u8::MAX → 1.0 |
| `label_percent_and_fill_fraction_are_identical` | line shows same % as `fill_fraction * 100` for many values |
| `install_view_four_percent_fill_matches_line` | install 4/100 → 4% line + 0.04 fill; 400px track → 16px fill |

## Verify

```text
cargo fmt -p colibri-native
cargo test -p colibri-native progress   # 46 passed
cargo clippy -p colibri-native --all-targets -- -D warnings  # clean
```

## Files touched

- `crates/colibri-native/src/progress.rs` — `fill_fraction` + tests
- `crates/colibri-native/src/main.rs` — `progress_strip_el` paint only (surgical; no wizard layout rewrite)

## Not changed

- Install phase floors / `progress_view_for_install` percent math (already aligned with label when counters present)
- Tier share bar flex heuristic (separate UI; not this bug)
- Marketing / i18n copy
