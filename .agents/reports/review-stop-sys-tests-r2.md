# Review r2: stop/cancel sys tests (after stop-sys fix)

**Role:** tests specialist (L2). No product code changes.
**Date:** 2026-08-10
**Scope:** Re-review after `/home/hunter/Projects/surmount/colibri/.agents/reports/impl-stop-sys-fix.md`
**Prior:** `/home/hunter/Projects/surmount/colibri/.agents/reports/review-stop-sys-tests.md`
**Files:** `crates/colibri-sys/src/engine/serve.rs`, `duplex.rs`, `mod.rs`

## Verdict

**clean**

Primary plan contracts remain strong. Prior test Issues 1–4 and 6 are fixed with real regression guards. New shutdown / write-cleanup / duplicate-id / next_id / mid-stream cancel tests match the product contracts and would fail if those bugs returned. Only residual is prior Issue 5 (sleep on zero-id mock), still deferred low priority and outside stop/cancel contracts.

## Observed green (this review)

```text
cargo test -p colibri-sys --lib -- mid_stream_ cancel_request stop_request \
  explicit_request_id duplicate_in_flight begin_generate_write_failure \
  shutdown_wakes shutdown_during duplex_cancel duplex_stop duplex_submit
# 15 passed, 0 failed (filtered)
```

Covered filters include all primary stop/cancel tests plus the six new fix-wave tests.

## Prior issues disposition

| Prior # | Severity | Status after fix | Notes |
|---------|----------|------------------|-------|
| 1 | suggestion | **fixed** | `engine::tests::mid_stream_cancel_no_deadlock`: DATA barrier, exact `CANCEL {id}`, mock `ERROR {id} CANCELLED`, `generate_stream` `Err` contains `CANCELLED` (not Ok Done). |
| 2 | suggestion | **fixed** | `serve::tests::explicit_request_id_bumps_next_auto_id`: wire `SUBMIT 42` then `SUBMIT 43`; flight ids 42 then 43. |
| 3 | suggestion | **fixed** (accepted residual) | Duplex mid-flight cancel optional; idle duplex CANCEL/STOP exact lines + engine mid-stream cover the real risk. No new dual-path product API. |
| 4 | nit | **fixed** (for hang-sensitive paths) | `shutdown_wakes_pending_recv` and `shutdown_during_generate_wakes_recv` use `recv_timeout(2s)` before join. Mid-stream stop/cancel keep join-as-hang-signal (mock still requires STOP/CANCEL before terminal). |
| 5 | nit | **still open** (deferred) | `explicit_request_id_zero_is_rejected` still sleeps 100ms on mock READY. Not stop/cancel; low flake risk on channel pipes. |
| 6 | nit | **fixed** (no change) | SUBMIT `starts_with` remains intentional for id mapping only. |

## New regression tests (fix wave)

| Test | Module | Contract strength |
|------|--------|-------------------|
| `mid_stream_cancel_no_deadlock` | `engine::tests` | **Strong.** Same unlock + DATA barrier as Stop; exact CANCEL; ERROR path forces Err not Done. |
| `shutdown_during_generate_wakes_recv` | `engine::tests` | **Strong.** `EngineHandle::stop` mid-stream; mock never DONE; 2s timeout fails on hang; asserts "shutting down". |
| `shutdown_wakes_pending_recv` | `serve::tests` | **Strong.** ServeClient-level drain; 2s hang bound; "shutting down". Slightly weaker barrier (50ms sleep vs AtomicBool DATA); still fails closed if pending not drained. |
| `begin_generate_write_failure_cleans_pending` | `serve::tests` | **Strong.** FailWriter after insert; second same id is write error, not "already in flight". |
| `duplicate_in_flight_request_id_is_rejected` | `serve::tests` | **Strong.** Second `Some(7)` errors; first still completes DONE. (80ms mock sleep only paces DONE after host check; sequential host path does not need it for correctness.) |
| `explicit_request_id_bumps_next_auto_id` | `serve::tests` | **Strong.** Exact next auto id after explicit. |

## Primary plan contracts (re-check)

| Contract | Guard | Still solid? |
|----------|-------|--------------|
| Mid-stream Stop no deadlock | `mid_stream_stop_no_deadlock` | Yes. Mock waits for STOP before DONE; DATA then `with_client(stop_request)`. |
| Cancel writes CANCEL not STOP | `cancel_request_writes_cancel_line`, duplex Cancel exact | Yes. |
| Stop writes STOP | `stop_request_writes_stop_line`, duplex Stop exact | Yes. |
| Explicit id on SUBMIT | `begin_generate_uses_explicit_request_id_on_submit`, duplex Submit 99 | Yes. |
| Zero id rejected | `explicit_request_id_zero_is_rejected` | Yes (adequacy unchanged). |
| Cancel ERROR e2e | `mid_stream_cancel_no_deadlock` (new) | **Now covered** (was open gap in r1). |
| Process stop wakes recv | `shutdown_*` pair (new) | **Now covered** (product Issue 1 from general review). |

Product paths verified by reading code (not only test names):

- `ServeClient::shutdown` drains `pending` with `ServeEvent::Error("…shutting down")` then kill (`serve.rs` ~405–428).
- Write failure after insert removes pending (`serve.rs` ~356–359).
- Duplicate in-flight rejected before insert (`serve.rs` ~307–313).
- `EngineHandle::generate_stream` releases mutex before `recv_loop` (`mod.rs` ~148–153); `stop` → `client.shutdown()`.

## False-green analysis (updated)

| Risk | Assessment |
|------|------------|
| Cancel silently becomes STOP | **Blocked** — exact CANCEL at serve + duplex. |
| Mid-stream stop false-pass without concurrent stop | **Blocked** — mock waits for STOP. |
| Cancel ERROR never surfaces | **Blocked** — mid-stream cancel expects Err + CANCELLED. |
| Shutdown hang after unlock-during-recv | **Blocked** — serve + engine shutdown wake tests with 2s timeout. |
| Write fail leaves zombie pending id | **Blocked** — FailWriter cleanup test. |
| Duplicate id overwrites first waiter | **Blocked** — duplicate reject + first still DONE. |
| Auto id collides after explicit | **Blocked** — bump to 43. |
| Timing flake mid-stream | Low — DATA barrier + channel pipes; hang tests use explicit 2s. |

## Issues still open

### Issue 5 (carried) -- Severity: nit
- File: `crates/colibri-sys/src/engine/serve.rs` (`explicit_request_id_zero_is_rejected`)
- Description: Mock uses `thread::sleep(100ms)` after READY so handshake completes. Wire cancel/stop tests correctly block on `read_line` instead. Mild flake risk under extreme load only.
- Suggestion: Mock that keeps the pipe open without a fixed sleep (or block on a host signal). Not required for stop/cancel slice.
- Status: **open** (deferred, out of stop/cancel scope)

No new stop/cancel test holes found that would change the clean verdict.

## What not to weaken

- Keep exact `"CANCEL …\n"` / `"STOP …\n"` asserts (not prefix-only if STOP could match).
- Keep mid-stream mocks that wait for STOP/CANCEL before DONE/ERROR.
- Keep hang-sensitive shutdown tests with wall-clock timeout (do not remove and rely only on suite-wide hang).

## Summary

r1 residual (cancel ERROR e2e, next_id bump) is landed with the right failure modes. Fix-wave adds real guards for shutdown wake, write-fail pending leak, and duplicate ids. Fifteen stop/sys-related tests re-run green in this review.

**Verdict: clean**
