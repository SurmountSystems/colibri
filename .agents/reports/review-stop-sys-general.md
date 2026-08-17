# Review: stop/cancel concurrency + wire (colibri-sys)

**Date:** 2026-08-10
**Scope:** Plan step 1 (sys only). Mutex unlock during generate, UI↔mux id mapping, STOP vs CANCEL wire, EngineHandle clone/drop, races, red/green coverage.
**Sources:** `.agents/reports/impl-stop-cancel-sys.md`, `.agents/reports/process-mop-stop-sys.md`, `crates/colibri-sys/src/engine/{mod,serve,duplex}.rs`
**Role:** L2 general review. No code changes in this pass.

## Summary

The core fix is correct and well aimed:

1. **Mutex scope:** `EngineHandle::generate_stream` locks only for `begin_generate` and post-DONE visual absorb; `InFlightGenerate::recv_loop` runs unlocked so concurrent `with_client(stop_request|cancel_request)` can take the handle mutex and write stdin.
2. **Id mapping:** Duplex sets `GenerateRequest::request_id: Some(ui_req_id)` so SUBMIT / STOP / CANCEL share one id; zero rejected; auto `next_id` bumped past explicit ids.
3. **Wire:** `stop_request` → `STOP {id}\n`, `cancel_request` → `CANCEL {id}\n`; duplex Stop/Cancel split matches protocol.
4. **Clone/drop:** `EngineHandle` is `Clone` over `Arc<Mutex<EngineInner>>`; process shutdown runs on last Arc drop via `Drop for EngineInner` (plus `ServeClient::Drop`). Intermediate clones do not kill the engine.
5. **Tests + mop:** Claimed contracts have dedicated tests; mop reports fmt/clippy/lib green (66 / 73 with install).

Unlocking the handle mutex during recv also **newly allows** `EngineHandle::stop` / `ServeClient::shutdown` to run while a generate is waiting on the event channel. That path can hang (see Issue 1). That is the main needs_fix item.

## What looks solid

| Area | Evidence |
|------|----------|
| Unlock during stream | `mod.rs` `generate_stream`: lock → `begin_generate` → unlock → `recv_loop` → lock absorb |
| Concurrent STOP | `mid_stream_stop_no_deadlock`: DATA then concurrent `stop_request(5)`, mock expects `STOP 5` |
| STOP vs CANCEL wire | `stop_request_writes_stop_line`, `cancel_request_writes_cancel_line`, duplex Stop/Cancel tests |
| UI id = mux id | `begin_generate_uses_explicit_request_id_on_submit` (`SUBMIT 42`), `duplex_submit_maps_ui_req_id_to_mux_submit` (`SUBMIT 99`) |
| stdin independent of handle lock | `ServeClient` uses own `stdin: Mutex`; stop/cancel do not need long-lived handle lock for I/O |
| Lock order (handle then stdin) | `with_client` holds handle mutex while `stop_request` takes stdin; `begin_generate` same order; no ABBA with current code |
| Feature gates | runtime/stream match product surface; re-export `InFlightGenerate` in `lib.rs` |

## Issues

### Issue 1 -- Severity: bug
- File: crates/colibri-sys/src/engine/serve.rs:393
- Description: `ServeClient::shutdown` sets `closed = true` and kills the child, but does **not** fail or drain `shared.pending`. The dispatcher on EOF only calls `fail(...)` when `!*closed`; if already closed it `break`s without waking waiters. After this change, `generate_stream` no longer holds the handle mutex during `recv_loop`, so another clone can call `EngineHandle::stop()` → `shutdown()` mid-generate. The generate thread then blocks forever in `rx.recv()` ("engine channel closed" never arrives). Pre-fix, the same handle mutex serialized generate and `stop`, so this hang was unreachable in practice. Python `Engine.close` always `_fail_pending` before teardown (`c/openai_server.py` ~1690–1695).
- Suggestion: On `shutdown`, after setting `closed`, drain `pending` and send `ServeEvent::Error("… shutting down")` (mirror dispatcher `fail`). Optionally still join/kill child. Add a red test: start mock generate mid-stream, call `EngineHandle::stop()` / `shutdown` from another thread, assert `generate_stream` returns error promptly (no hang).
- Status: fixed
- Response: `shutdown` now drains `pending` and sends `ServeEvent::Error("colibri engine is shutting down")` before kill. Tests: `serve::shutdown_wakes_pending_recv`, `engine::shutdown_during_generate_wakes_recv` (2s timeout, asserts "shutting down").

### Issue 2 -- Severity: bug
- File: crates/colibri-sys/src/engine/serve.rs:337
- Description: `begin_generate` inserts into `pending` before writing SUBMIT. NUL prompt/grammar paths remove the entry; stdin write/flush errors after insert do **not**. The local `rx` is dropped on `Err`, leaving a dead `Sender` in the map until a later DONE/ERROR for that id (which may never come if the write was incomplete). Python cleans with `pending.pop` in the write `except` path. Touched code path for the new unlock/id design.
- Suggestion: On any error after `insert`, `pending.remove(&request_id)` before returning (defer pattern, or scope guard). Add a test that forces a broken stdin write after insert and asserts the id is not left in pending (or a second generate with the same explicit id still works).
- Status: fixed
- Response: Write/flush of SUBMIT is a single `write_ok` block; on `Err`, `pending.remove` then return engine error (mirrors NUL cleanup). Test: `begin_generate_write_failure_cleans_pending` (FailWriter + second same id is not "already in flight").

### Issue 3 -- Severity: suggestion
- File: crates/colibri-sys/src/engine/serve.rs:305
- Description: Explicit `request_id: Some(id)` always `insert`s into `pending` with no check that `id` is already in-flight. Protocol requires ids unique among in-flight (`docs/serve_protocol.md`). A second concurrent `begin_generate` with the same id overwrites the HashMap entry, drops the first `Sender`, and the first `recv_loop` fails with "engine channel closed" while the second flight may see mixed or engine `DUPLICATE_ID` traffic. Duplex is sequential per call, but `EngineHandle` is intentionally `Clone` for concurrent stop and can host concurrent generates.
- Suggestion: If `pending` already contains the string id, return `Error::invalid` / engine-style duplicate before insert (or reject with a clear message). Optional test: two `begin_generate` with `Some(7)` without completing the first → second errors, first still receives.
- Status: fixed
- Response: Before insert, reject if `pending.contains_key` with `Error::invalid("request id {id} is already in flight")`. Test: `duplicate_in_flight_request_id_is_rejected` (second Some(7) errors; first still completes DONE).

### Issue 4 -- Severity: suggestion
- File: crates/colibri-sys/src/engine/mod.rs:300
- Description: Red/green covers mid-stream **STOP** no-deadlock and cancel **wire** bytes, but not mid-stream **CANCEL** end-to-end (concurrent `cancel_request` during unlocked recv, mock replies `ERROR <id> CANCELLED`, `recv_loop` / duplex emit Error). Lock path matches STOP, so risk is lower than Issue 1, but the cancel completion contract is only half-tested.
- Suggestion: Add `mid_stream_cancel_no_deadlock` (mirror stop test) and optionally a duplex test that cancel during generate surfaces `ServerFrame::Error` with the UI `req_id`.
- Status: fixed
- Response: Added `engine::mid_stream_cancel_no_deadlock`: DATA barrier, concurrent `cancel_request(5)`, mock expects `CANCEL 5` then `ERROR 5 CANCELLED`, generate_stream Err contains CANCELLED.

### Issue 5 -- Severity: nit
- File: crates/colibri-sys/src/engine/serve.rs:287
- Description: Explicit id bumps `next_id` when `id >= *next` so later `None` allocations avoid collision. No unit test asserts that after `Some(42)`, a following `None` allocates `43` (or that `Some(5)` when next is already `10` leaves next unchanged).
- Suggestion: Small pure allocation test with mock pipes: explicit 42, then None → SUBMIT 43.
- Status: fixed
- Response: Added `explicit_request_id_bumps_next_auto_id`: Some(42) then None → flight id 43 and wire SUBMIT 43.

### Issue 6 -- Severity: nit
- File: crates/colibri-sys/src/engine/serve.rs:393
- Description: Doc comment says "Graceful shutdown: close stdin, wait for child" but the body flushes, sets `closed`, and `kill`s the child; it does not close stdin for protocol EOF ("in-flight finish first"). Naming/docs drift relative to mux `stop_request` vs process `EngineHandle::stop`.
- Suggestion: Align the comment with kill/teardown behavior, or implement stdin close + wait if true graceful exit is desired. Distinct names in docs (process stop vs mux STOP) help hosts.
- Status: fixed
- Response: Doc now describes tear-down (mark closed, wake waiters, kill child) and notes this is process stop, not mux STOP.

## Focus checklist

| Focus | Result |
|-------|--------|
| Mutex scope: stop during generate without deadlock | **Pass** for mux STOP/CANCEL via `with_client`; regression hang for process `shutdown` (Issue 1) |
| request_id mapping UI ↔ mux SUBMIT | **Pass** (explicit Some, duplex Submit) |
| STOP vs CANCEL wire | **Pass** |
| Clone/drop EngineHandle | **Pass** (last Arc drops EngineInner → shutdown; clones share process) |
| Race / wrong-id silent no-ops | Partial: wrong STOP id is fire-and-forget (OK); host duplicate explicit id is silent overwrite (Issue 3); pending leak on write fail (Issue 2) |
| Red/green for claimed contracts | **Pass** for listed impl tests; gaps on shutdown-wake and cancel mid-stream (Issues 1, 4) |

## Verdict

**fixed** (post impl-stop-sys-fix)

| Severity | Open count |
|----------|------------|
| bug | 0 |
| suggestion | 0 |
| nit | 0 |
| **total open** | **0** |

All six issues addressed in `impl-stop-sys-fix` pass.
