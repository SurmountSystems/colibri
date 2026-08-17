# Review r2: stop/cancel review fixes (colibri-sys)

**Date:** 2026-08-10
**Scope:** Re-verify general Issues 1–3 product fixes after `impl-stop-sys-fix`.
**Sources:** `.agents/reports/review-stop-sys-general.md`, `.agents/reports/impl-stop-sys-fix.md`, `crates/colibri-sys/src/engine/{serve,mod,duplex}.rs`
**Role:** L2 general re-review. No code changes in this pass.

## Summary

All three claimed product fixes are present in code and covered by dedicated tests. No new open issues from this re-check.

| Claimed fix | Code | Test | Verdict |
|-------------|------|------|---------|
| Shutdown drains pending | `ServeClient::shutdown` drains + `ServeEvent::Error("… shutting down")` before kill | `serve::shutdown_wakes_pending_recv`, `engine::shutdown_during_generate_wakes_recv` | **fixed** |
| Write failure cleans pending | `write_ok` Err path `pending.remove` before return | `serve::begin_generate_write_failure_cleans_pending` | **fixed** |
| Duplicate in-flight ids rejected | `pending.contains_key` → `Error::invalid("… already in flight")` before insert | `serve::duplicate_in_flight_request_id_is_rejected` | **fixed** |

Also confirmed still in tree (prior Issues 4–6 / related): mid-stream cancel test, explicit-id next_id bump test, shutdown doc as process tear-down (not mux STOP). Duplex Stop/Cancel still route to the correct wire helpers.

## Verification detail

### 1. Shutdown drains pending

```405:428:crates/colibri-sys/src/engine/serve.rs
pub fn shutdown(&mut self) -> Result<()> {
    *self.shared.closed.lock() = true;
    // Drain pending before killing so mid-stream generate_stream wakes.
    {
        let pending: Vec<_> = self
            .shared
            .pending
            .lock()
            .drain()
            .map(|(_, tx)| tx)
            .collect();
        for tx in pending {
            let _ = tx.send(ServeEvent::Error("colibri engine is shutting down".into()));
        }
    }
    // … flush stdin, kill/wait child
}
```

- Order matches the hang root cause: set `closed` first (so dispatcher EOF does not double-`fail`), then drain waiters, then kill.
- `recv_loop` maps `ServeEvent::Error` to `Error::engine(msg)` (not only channel-closed).
- `begin_generate` rejects new work when already closed (`"colibri engine is shutting down"`).
- Engine-level path: `EngineHandle::stop` → `client.shutdown()` while generate is unlocked in `recv_loop`; test asserts no hang within 2s and error contains `"shutting down"`.

### 2. Write failure cleans pending

```345:360:crates/colibri-sys/src/engine/serve.rs
let write_ok = (|| -> std::io::Result<()> {
    let mut stdin = self.stdin.lock();
    stdin.write_all(header.as_bytes())?;
    // … payload / grammar / trailing newline / flush
    Ok(())
})();
if let Err(e) = write_ok {
    // Mirror NUL paths: never leave a dead Sender in pending after insert.
    self.shared.pending.lock().remove(&request_id);
    return Err(Error::engine(e.to_string()));
}
```

- Insert still happens before write (needed so dispatcher can deliver); any write/flush failure removes the id, same as NUL prompt/grammar paths.
- Test forces `FailWriter` after insert; second `begin_generate(Some(11))` fails on I/O again, **not** `"already in flight"`.

### 3. Duplicate ids rejected

```306:315:crates/colibri-sys/src/engine/serve.rs
let mut pending = self.shared.pending.lock();
// Protocol: ids must be unique among in-flight requests.
if pending.contains_key(&request_id) {
    return Err(Error::invalid(format!(
        "request id {request_id} is already in flight"
    )));
}
pending.insert(request_id.clone(), tx);
```

- Check and insert share one lock hold (no check-then-insert race between two hosts).
- Test: first `Some(7)` stays registered and completes DONE; second `Some(7)` errors with `"already in flight"`.

## What looks solid (unchanged)

| Area | Evidence |
|------|----------|
| Unlock during stream | `generate_stream`: lock → `begin_generate` → unlock → `recv_loop` → lock absorb |
| Concurrent STOP / CANCEL | `mid_stream_stop_no_deadlock`, `mid_stream_cancel_no_deadlock` |
| STOP vs CANCEL wire | serve + duplex exact line tests |
| UI id = mux id | explicit SUBMIT + duplex Submit mapping |
| Process stop vs mux STOP | `shutdown` doc; drain + kill behavior |

## Issues

_(none)_

## Focus checklist

| Focus | Result |
|-------|--------|
| Shutdown wakes mid-stream generate | **Pass** (drain + Error; serve + EngineHandle tests) |
| Pending not leaked on SUBMIT write fail | **Pass** |
| Duplicate explicit id rejected | **Pass** (first flight intact) |
| Closed after stop rejects new generate | **Pass** (`begin_generate` closed check) |
| Prior hang regression (unlock + process stop) | **Pass** |

## Verdict

**clean**

| Severity | Open count |
|----------|------------|
| bug | 0 |
| suggestion | 0 |
| nit | 0 |
| **total open** | **0** |

Claimed fixes for general Issues 1–3 are real in product code and regression-tested. No new remaining issues opened in this re-review.
