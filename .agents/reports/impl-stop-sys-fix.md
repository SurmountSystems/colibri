# Impl: stop/sys review fixes

**Date:** 2026-08-10
**Scope:** Open review bugs/suggestions for colibri-sys stop/cancel (plan step 1 follow-up)
**Reviews:** `review-stop-sys-general.md`, `review-stop-sys-tests.md`, `review-stop-sys-plan.md`

## Product changes

### 1. Shutdown wakes pending receivers (general Issue 1 — bug)

`ServeClient::shutdown` now, after `closed = true`:

1. Drains `shared.pending` and sends `ServeEvent::Error("colibri engine is shutting down")` to every waiter.
2. Flushes stdin and kills/waits the child (unchanged).

This mirrors Python `Engine.close` → `_fail_pending`. Without it, unlock-during-`recv_loop` allowed `EngineHandle::stop` mid-generate to hang forever (dispatcher EOF with `closed` skips `fail`).

Doc comment updated: process tear-down (not mux STOP), kill + wake waiters.

### 2. Pending cleaned on stdin write failure (general Issue 2 — bug)

`begin_generate` SUBMIT write/flush is a single fallible block. On any I/O error after insert, `pending.remove(&request_id)` runs before returning (same as NUL prompt/grammar paths).

### 3. Reject duplicate in-flight ids (general Issue 3)

Before insert, if `pending` already has the string id, return `Error::invalid("request id {id} is already in flight")`.

## Tests added

| Test | Module | Contract |
|------|--------|----------|
| `shutdown_wakes_pending_recv` | `serve::tests` | Mid-stream shutdown → recv ends with "shutting down" within 2s |
| `shutdown_during_generate_wakes_recv` | `engine::tests` | `EngineHandle::stop` mid-generate wakes; no hang |
| `begin_generate_write_failure_cleans_pending` | `serve::tests` | FailWriter after insert; second same id not "already in flight" |
| `duplicate_in_flight_request_id_is_rejected` | `serve::tests` | Second Some(7) errors; first still gets DONE |
| `explicit_request_id_bumps_next_auto_id` | `serve::tests` | Some(42) then None → SUBMIT 43 / flight 43 |
| `mid_stream_cancel_no_deadlock` | `engine::tests` | Concurrent cancel after DATA → CANCEL wire + ERROR CANCELLED |

## Verify

```text
cargo fmt -p colibri-sys
cargo clippy -p colibri-sys --all-targets -- -D warnings   # exit 0
cargo test -p colibri-sys --lib                            # 72 passed
```

## Review file status

| Review | Open → fixed |
|--------|----------------|
| general | Issues 1–6 all fixed |
| tests | Issues 1–4, 6 fixed; Issue 5 (sleep on zero-id mock) left open / deferred low priority |
| plan | Issues 1–2 fixed (cancel test added; with_client lock left as-is) |

## Files touched

- `crates/colibri-sys/src/engine/serve.rs` — shutdown drain, write cleanup, duplicate id, tests
- `crates/colibri-sys/src/engine/mod.rs` — mid-stream cancel + process-stop wake tests
- `.agents/reports/review-stop-sys-*.md` — Status/Response updates
)
