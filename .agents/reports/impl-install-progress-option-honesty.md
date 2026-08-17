# Install progress Option honesty (no fake 0% / ETA)

**Date:** 2026-08-11
**Status:** done
**Board:** `bug:install-progress-option-honesty`
**Prior:** `.agents/reports/impl-install-progress-stuck-zero.md` (mid-file + ETA cap)

## Operator ask

> Just make sure the nonsense numbers are addressed at first. Option is better than showing inaccurate anything.

## Contract

| Situation | UI |
|-----------|-----|
| Percent unknown / untrustworthy (no total, `done == 0`, no counters) | **Omit** `%` (not `0%`) |
| ETA unknown / untrustworthy (rate bad, `done == 0`, absurd estimate) | **Omit** entirely (not "Calculating...", not "about N hours left") |
| Progress bar | Empty track when fraction is `None`; fill only when `Some` and trustworthy |
| Footer / chrome | Same: no `N%` when percent is `None` |
| Mid-way with known total + rate | Show `%` and may show ETA |

## API shape (`progress.rs`)

- `ProgressView.percent: Option<u8>` (was bare `u8`)
- `install_progress(...) -> (Option<u8>, Option<u64>)`
- `generate_progress(...) -> (Option<u8>, Option<u64>)`
- `install_percent(done, total) -> Option<u8>` — `None` when total 0 or done 0; floor **1%** once any work landed so integer math never prints mid-transfer `0%`
- `format_eta(secs) -> Option<String>` — `None` input → `None` (no filler string)
- `format_progress_line(status, Option<u8>, Option<u64>)` — concatenates only known pieces
- `format_install_chrome_status(file, percent)` — footer helper without fake `%`
- `fill_fraction(Option<u8>)` — `None` → `0.0` empty track

## Host / shell

- `progress_view_for_install`: download uses pure install math; **removed fake 5% CLI floor**
- Phase floors only for `inspect` / `register` / `done` (`install_phase_percent_floor` → `Option`)
- Start install view: `ProgressView::new(None, None, "Downloading...")`
- Drain path footer uses `format_install_chrome_status`

## TDD (red contracts → green)

Named contracts (written first as expectations; green after Option APIs):

| Test | Asserts |
|------|---------|
| `install_zero_done_no_percent_no_eta` | done 0 → `None` %, no `%` / `about` / hours in line |
| `install_neither_totals_is_none_no_eta` | no totals → label only |
| `format_progress_line_unknown_omits_percent_and_eta` | `"Downloading..."` only |
| `format_progress_line_percent_only_no_eta_filler` | `"… 12%"` with no Calculating |
| `format_eta_unknown_is_none` | no Calculating filler |
| `format_install_chrome_status_omits_unknown_percent` | footer without `%` |
| `install_partial_file_advances_percent` | mid-way shows `%` and ETA |
| `progress_view_zero_done_no_absurd_eta` (host) | view + empty fill |
| `progress_view_for_install_cli_no_counters_omits_percent` | no invented floor |
| `progress_view_mid_file_partial_advances` | 50/500 MiB → 10% + ETA |

## Verify

```text
cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets -- -D warnings   # clean
cargo test -p colibri-native                                  # 275 passed
```

Did not touch `colibri-sys` this slice (native formatters only).

## Files

- `crates/colibri-native/src/progress.rs`
- `crates/colibri-native/src/host.rs`
- `crates/colibri-native/src/main.rs`

## Expected UX

- Start / zero bytes: `Downloading... · out-00000.safetensors · 12.5/372.0 GiB` (no `0%`, no ETA)
- Mid transfer: `Downloading... 10% · about 1 min left · file · GiB pair`
- CLI without counters: `Downloading...` (empty bar), not fake 5%
- Footer: `Installing · file` until percent is real, then `Installing · file · N%`

## Not in scope

- Prefer-cli byte stream (still coarse)
- Product marketing copy
- Git commit (operator-owned)
