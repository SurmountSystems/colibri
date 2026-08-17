# Review r2: desktop residuals bundle (post-fix)

**Date:** 2026-08-10
**Role:** L2 primary general re-review. **No code changes.**
**Fix summary:** `.agents/reports/impl-desktop-bundle-fix.md`
**Prior general review:** `.agents/reports/review-desktop-bundle-general.md`
**Also cross-checked:** `review-desktop-bundle-tests.md` (fix responses), `host.rs` / `main.rs` / `docs/fidelity.md` / web `Brain.tsx`, residual OPEN list.

## Summary

Every issue from the r1 general review (Issues 1–7) is **closed in code and covered by host unit tests** (or an intentional pure-helper seam). Dest-under-store is enforced, heat curve matches web, Stop/ReqBook bookkeeping has pure guards, packed Brain decode is asserted, double-terminal Error is gated, stop race labels Done as `stopped`, and install options / space format are pinned.

**Observed green (this pass):**

```text
cargo test -p colibri-desktop-gpui --features install
# 23 passed; 0 failed
```

Product OPEN residual (install cancel, min-free gate, full atlas, pulse decay, Phase D, etc.) is unchanged and still honest. None of that is a regression or an incomplete fix from this pass.

## Prior issues (r1) — verification

| # | Severity | Claimed fix | Code evidence | Test evidence | Status |
|---|----------|-------------|---------------|---------------|--------|
| 1 | bug | heat/24 lum | `brain_cell_rgb`: `heat_f = heat/24`, `lum = 0.35 + 0.65 * heat_f` matches `web/src/Brain.tsx` | `brain_cell_rgb_heat_saturates_at_24` (full VRAM RGB at 24=63; mid r≥50; cold 0.35) | **closed** |
| 2 | suggestion | dest under store | `resolve_install_dest` + `path_is_under_store` (lexical; reject `..`; absolute only if under store); fidelity row updated | `validate_install_rejects_dest_escape`, `validate_install_accepts_absolute_under_store` | **closed** |
| 3 | suggestion | Stop/session host tests | `ReqBook::begin` / `clear_matching`; empty-slot stop/generate; status helper | `stop_session_empty_slot_errors`, `generate_async_errors_when_no_session`, `req_book_*`, `status_after_gen_done_respects_stop_requested` | **closed** |
| 4 | suggestion | packed decode asserts | small map indices 0/3/4/7; large map cells[0] tier 2 heat 40 | `brain_view_full_small_map`, `brain_view_samples_large_map` | **closed** |
| 5 | nit | no double Error | `saw_terminal` in generate worker | structural (stream terminal path) | **closed** |
| 6 | nit | stop race status | `stop_generate` sets `stop_requested` on stop fail while generating; comment documents race | uses `status_after_gen_done` in `drain_gen` | **closed** |
| 7 | nit | prefer_cli + space | `install_options_for_ui`; `format_install_space` | `install_options_prefer_cli_true`, `format_install_space_includes_dest_and_gb` | **closed** |

Tests-review items (PROF last-N, empty pump, expanded install rejects, tiers RAM/GB) also present and green. Live mux STOP id remains colibri-sys (intentional split; not a desktop host gap for this re-review).

## Issues

_None open from the fix pass or r1 findings._

## Non-issues / correctly deferred (unchanged residual)

| Item | Why not a fix-pass issue |
|------|---------------------------|
| `open:install-cancel` | No first-class cancel in install path |
| `open:install-min-free-gate` | UI still `min_free = 0`; free space informational |
| `open:brain-full-atlas` | Sampled grid; fidelity **partial** |
| `open:brain-pulse-decay` | One-shot pulse; web RAF decay not ported (heat curve fixed separately) |
| `open:ffi-phase-d` | Docs honest; process mux is product path |
| Brief Stop race: STOP not retried if `active_req` not set yet | r1 Issue 6 accepted status-only fix; generate may complete after a failed early Stop. Window is short (`begin` runs early in the worker). Not re-opened. |
| No desktop mock that `stop_request(id)` is called with `active_req` | Sys mid-stream tests + host ReqBook pure tests are the intended split |

## Verdict

**clean**

| Severity | Open count |
|----------|------------|
| bug | 0 |
| suggestion | 0 |
| nit | 0 |
| **total open** | **0** |

No further fix round required for the desktop residual bundle review scope (plan steps 2–6 host surface). Campaign OPEN rows above remain product residual, not review debt from this pass.
