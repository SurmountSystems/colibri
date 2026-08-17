# Impl report: desktop residual bundle review fixes

**Date:** 2026-08-10
**Package:** `colibri-desktop-gpui`
**Reviews:** `review-desktop-bundle-general.md`, `review-desktop-bundle-tests.md`

## What changed

### Bugs
1. **Brain heat curve** (`brain_cell_rgb`): aligned with web `Brain.tsx`
   `lum = 0.35 + 0.65 * min(heat / 24, 1)` (was heat/63 with different base).
   Test: `brain_cell_rgb_heat_saturates_at_24`.

### Dest under store
2. `resolve_install_dest` + `path_is_under_store` (lexical component prefix).
   - Empty → `store/owner__name`
   - Relative → `store/<path>` (no `..`)
   - Absolute → only if already under store
   Fidelity + impl-desktop-residuals-bundle wording updated.

### Host tests / bookkeeping
3. `ReqBook::begin` / `clear_matching` (used by `generate_async`).
4. Pure helpers: `status_after_gen_done`, `install_options_for_ui`.
5. Tests added or strengthened (23 total, was 11):
   - Stop: `stop_session_empty_slot_errors`, `generate_async_errors_when_no_session`
   - ReqBook: `req_book_allocates_and_blocks_overlapping`, `req_book_clear_only_matching_id`
   - Status: `status_after_gen_done_respects_stop_requested`
   - Pump: `pump_session_visual_none_when_slot_empty`
   - PROF: `format_profile_keeps_last_n_only`
   - Brain packed: decode asserts on small + large maps
   - Heat: `brain_cell_rgb_heat_saturates_at_24`
   - Install: dest escape reject, absolute under store, expanded repo rejects, prefer_cli, format_install_space
   - Tiers: RAM + GB fields

### Nits
6. `saw_terminal` avoids double `GenEvent::Error` after stream terminal.
7. Stop race: `stop_generate` sets `stop_requested` even when `active_req` missing yet.
8. `main` uses `status_after_gen_done` for Done status text.

## Verify

```text
cargo fmt -p colibri-desktop-gpui
cargo clippy -p colibri-desktop-gpui --all-targets -- -D warnings   # exit 0
cargo test -p colibri-desktop-gpui --features install               # 23 passed
```

## Residual

No new OPEN rows. Existing OPEN (install cancel, min-free gate, full atlas, pulse decay, Phase D) unchanged. Heat curve / dest honesty are closed in code + docs.

## Review status

All issues in both review files: **Status: fixed** with Response notes.
