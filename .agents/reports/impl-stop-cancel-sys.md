# Implement: Stop / cancel host concurrency + correct wire (colibri-sys)

**Date:** 2026-08-10
**Repo:** `/home/hunter/Projects/surmount/colibri`
**Plan step:** Native desktop residuals step 1 (sys only)
**Recon:** `.agents/reports/recon-plan-four-gaps.md` §3

## Outcomes

| Requirement | Status |
|-------------|--------|
| Do not hold `EngineHandle` mutex across whole `generate_stream` recv loop | Done |
| Map duplex/UI `req_id` to mux SUBMIT id explicitly | Done (same value on wire) |
| `ServeClient::cancel_request` writes `CANCEL <id>` | Done |
| `EngineDuplex`: Stop → STOP; Cancel → CANCEL | Done |
| Red/green tests with mock pipes | Done |

## Design: locks

**Before:** `EngineHandle::generate_stream` locked `inner` for the entire SUBMIT + `rx.recv()` loop. `with_client` (used for Stop) needed the same lock, so a second thread could never send STOP mid-stream (deadlock / hang until DONE).

**After:**

1. `ServeClient::begin_generate` writes SUBMIT, registers the pending channel, returns `InFlightGenerate { request_id, rx }`.
2. `InFlightGenerate::recv_loop` blocks on the channel only (no `ServeClient` / handle lock).
3. `EngineHandle::generate_stream`:
   - lock → `begin_generate` → unlock
   - `recv_loop` (unlocked; concurrent STOP/CANCEL OK)
   - lock → absorb visual telemetry → unlock
4. stdin writes for STOP/CANCEL use `ServeClient`'s own `stdin: Mutex` (independent of the handle mutex).
5. `EngineHandle` is `Clone` (shared `Arc`). Shutdown runs only when the last clone drops (`Drop` on `EngineInner`).

## Design: ids

**Before:** Mux id came from `ServeClient::next_id` (starts at 1). Duplex stamped UI `req_id` on `ServerFrame`s but STOP used that same number without writing it on SUBMIT. Worked only if both counters stayed in lockstep by accident.

**After:**

- `GenerateRequest::request_id: Option<u64>`
  - `None`: allocate from `next_id` (unchanged for direct `ServeClient` / `EngineHandle` callers)
  - `Some(n)` with `n != 0`: that exact id is written on `SUBMIT n …`
  - `Some(0)`: rejected as invalid
- When an explicit id is used, `next_id` is bumped past it so later auto ids do not collide.
- `EngineDuplex::handle_submit` sets `request_id: Some(ui_req_id)` so UI id **is** the mux id.
- Stop/Cancel use the same UI/mux id on the wire.

No separate HashMap is required while duplex owns the id.

## Wire commands

| API | Line |
|-----|------|
| `ServeClient::stop_request(id)` | `STOP {id}\n` |
| `ServeClient::cancel_request(id)` | `CANCEL {id}\n` |
| `ClientFrame::Stop { req_id }` | → `stop_request` |
| `ClientFrame::Cancel { req_id }` | → `cancel_request` |

## Files touched

- `crates/colibri-sys/src/engine/serve.rs` — `request_id`, `InFlightGenerate`, `begin_generate`, `cancel_request`, tests
- `crates/colibri-sys/src/engine/mod.rs` — unlock during stream, `Clone`, last-clone drop, mid-stream stop test
- `crates/colibri-sys/src/engine/duplex.rs` — Stop/Cancel split, explicit SUBMIT id, duplex tests
- `crates/colibri-sys/src/lib.rs` — re-export `InFlightGenerate`
- `crates/colibri-sys/examples/embed_chat.rs` — `request_id: None` field
- `crates/colibri-sys/tests/engine_real.rs` — `request_id: None` field

## Red / green evidence

**Contract (would fail on pre-fix code):**

1. **Mid-stream stop no deadlock** — concurrent `with_client(stop_request)` while `generate_stream` is in recv. Pre-fix held the handle mutex for the whole stream → stop could never acquire the lock → hang. New test `engine::tests::mid_stream_stop_no_deadlock` completes with STOP on the mock stdin and DONE.
2. **Cancel wire** — pre-fix had no `cancel_request`; duplex mapped Cancel to STOP. New tests assert `CANCEL 99\n` / duplex Cancel → `CANCEL 77\n`.
3. **Id mapping** — `SUBMIT 42` / duplex `SUBMIT 99` with UI `req_id` 42/99, not auto-1.

**Green tests added:**

| Test | Module |
|------|--------|
| `mid_stream_stop_no_deadlock` | `engine::tests` |
| `cancel_request_writes_cancel_line` | `engine::serve::tests` |
| `stop_request_writes_stop_line` | `engine::serve::tests` |
| `begin_generate_uses_explicit_request_id_on_submit` | `engine::serve::tests` |
| `explicit_request_id_zero_is_rejected` | `engine::serve::tests` |
| `duplex_cancel_writes_cancel_with_ui_req_id` | `engine::duplex::tests` |
| `duplex_stop_writes_stop_with_ui_req_id` | `engine::duplex::tests` |
| `duplex_submit_maps_ui_req_id_to_mux_submit` | `engine::duplex::tests` |

## Verify commands

```text
cargo fmt -p colibri-sys
# exit 0

cargo clippy -p colibri-sys --all-targets -- -D warnings
# exit 0

cargo test -p colibri-sys --lib
# exit 0
# 66 passed; 0 failed
```

## Out of scope (next residual)

- GPUI Stop button / keep session live during generate (host UI path)
- Visual pump concurrency (same lock design now enables it)
