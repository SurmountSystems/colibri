# Report: `impl:progress-widget` pure math

Date: 2026-08-11
Scope: `crates/colibri-native` progress helpers only (no install/generate GPUI wire-up)

## Done

### New module
`/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/progress.rs`

Pure functions (no GPUI):

| API | Behavior |
|-----|----------|
| `percent_done(done, total) -> u8` | 0..=100; `total == 0` → 0; `done > total` → 100; wide math avoids overflow |
| `eta_secs(remaining, rate_per_sec) -> Option<u64>` | `None` if rate ≤ 0 or non-finite; ceil division; remaining 0 → `Some(0)` |
| `format_eta(secs) -> String` | `None` → `"Calculating..."`; &lt;60s → `"about Ns left"`; &lt;1h → `"about N min left"`; else hours |
| `format_progress_line(status, percent, eta)` | `"{status} {percent}% · {eta}"` |
| `generate_progress(generated, max_output, tok_per_sec)` | percent + ETA from remaining tokens / tok/s |
| `install_progress(bytes_*, files_*, rate)` | Prefer **bytes** when total &gt; 0 known; else **files**; else `(0, None)` |
| `install_progress_view` / `generate_progress_view` | Build `ProgressView` |
| `ProgressView { percent, eta_secs, label }` | `.line()` for UI later |

ASCII ellipsis (`...`) in copy (not unicode `…`).

### Wire-up
- `mod progress;` in `main.rs` (already present alongside `prefs` / theme work).
- `#![allow(dead_code)]` on the module until install/generate UI calls these symbols (this slice deliberately does not paint the bar).

### TDD
**34 unit tests**, edge cases covered:

- zero total, done &gt; total, large u64 percent
- zero / negative / NaN / Inf rate → no ETA
- generate midway / full / over max / zero rate
- install prefers bytes over files; fallbacks; missing totals

**Command (green while tree compiled):**
```text
cargo test -p colibri-native --bin colibri-native progress
# 34 passed; 0 failed
```

## Not in this slice

- GPUI progress strip / bar in main
- Host install byte rate plumbing
- Live generate bar under composer

Next related board item: `impl:install-generate-progress`.

## Notes / tree state

- Package-level compile at handoff may be red from **parallel** theme wiring in `main.rs` / `theme.rs` (palette-first call sites mid-flight). Progress tests were green before that wave broke the bin; progress itself has no theme dependency.
- Trivial fix applied during verify: `ThemePalette::all_role_colors` return size `31` → `30` to match the 30 role fields (compile error unrelated to progress math). Theme implementer may still be editing; re-check if they add a role.
- Package `clippy -D warnings` was also red on unwired `prefs.rs` dead_code (parallel prefs slice). Progress has no clippy hits once allowed for not-yet-wired API.

## Files touched

| Path | Change |
|------|--------|
| `crates/colibri-native/src/progress.rs` | **New** pure math + tests |
| `crates/colibri-native/src/main.rs` | `mod progress` (if not already) |
| `crates/colibri-native/src/theme.rs` | array length 30 (verify unblock only) |

No git commit (operator-owned).
