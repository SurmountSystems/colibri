# Plan alignment review: stop/cancel sys (step 1 only)

**Date:** 2026-08-10
**Role:** plan-alignment specialist (L2), read-only
**Plan step under review:** Native desktop residuals step 1 (colibri-sys only)
**Impl summary:** [`.agents/reports/impl-stop-cancel-sys.md`](impl-stop-cancel-sys.md)
**Code scope:** `crates/colibri-sys/src/engine/` (+ small compile follow-ons)
**Recon backdrop:** [`.agents/reports/recon-plan-four-gaps.md`](recon-plan-four-gaps.md) §3

## Plan step 1 checklist (required)

| Plan item | Evidence in tree | Status |
|-----------|------------------|--------|
| Split locking so STOP/CANCEL work mid `generate_stream` | `EngineHandle::generate_stream` locks only for `begin_generate` and post-stream visual absorb; `recv_loop` runs unlocked (`mod.rs` ~140–160). `InFlightGenerate` documents concurrent stop/cancel (`serve.rs` ~56–60). Stdin STOP/CANCEL use `ServeClient`’s own `stdin: Mutex`. | **Met** |
| Map UI `req_id` to mux SUBMIT id explicitly | `GenerateRequest::request_id: Option<u64>` (`serve.rs` ~36–39); `begin_generate` writes that id on `SUBMIT` (`serve.rs` ~281–330); duplex `handle_submit` sets `request_id: Some(req_id)` (`duplex.rs` ~167–176). Same value on frames and STOP/CANCEL. | **Met** |
| `cancel_request` → `CANCEL`; Stop → `STOP` | `ServeClient::cancel_request` → `CANCEL {id}` (`serve.rs` ~385–390); `stop_request` → `STOP {id}` (~377–382). Duplex: `ClientFrame::Stop` → `stop_request`, `Cancel` → `cancel_request` (`duplex.rs` ~122–129). | **Met** |
| Red/green mock pipe tests | Mid-stream stop concurrency: `engine::tests::mid_stream_stop_no_deadlock`. Wire + id: serve cancel/stop lines, explicit SUBMIT id, zero reject; duplex cancel/stop/submit mapping tests. Process mop: 66 lib tests green. | **Met** |

## Scope check (step 1 vs later residual)

| Later residual (recon / impl “out of scope”) | In this slice? |
|----------------------------------------------|----------------|
| GPUI Stop button / keep session live during generate | **No.** No Stop/Cancel symbols under `colibri-desktop-gpui`. `generate_async` still exists; unchanged by this slice. |
| Visual pump concurrency product work | **No.** Unlock design enables it; no new pump-during-generate product path. |
| True FFI Phase D | **No.** |

Supporting non-plan noise that stays in-scope for step 1:

- `EngineHandle: Clone` + `Drop` on `EngineInner` (last clone shuts down) so a second thread can hold a handle for STOP mid-stream.
- `request_id: None` on `examples/embed_chat.rs` and `tests/engine_real.rs` (struct field compile fix).
- Re-export `InFlightGenerate` from `lib.rs`.

None of that is desktop UI work.

## Issues

No plan misses and no out-of-scope desktop work found for step 1.

### Issue 1 -- Severity: nit
- File: `crates/colibri-sys/src/engine/mod.rs:300`
- Description: Mid-stream concurrency is covered for **STOP** (`mid_stream_stop_no_deadlock`) but not a parallel **CANCEL** mid-`recv` test. Wire CANCEL is covered separately; lock split is the same path for both.
- Suggestion: Optional follow-up test mirroring the stop case with `cancel_request` + mock `ERROR id CANCELLED` if you want symmetric red proof. Not required to call step 1 done.
- Status: fixed
- Response: Added `mid_stream_cancel_no_deadlock` (symmetric to stop: DATA barrier, CANCEL wire, ERROR CANCELLED, generate Err).

### Issue 2 -- Severity: nit
- File: `crates/colibri-sys/src/engine/mod.rs:168`
- Description: `with_client` still holds the **handle** mutex for the whole callback (including the STOP/CANCEL stdin write). That is fine for step 1: generate no longer holds the handle mutex across recv, so stop can acquire. A future tighter design could release the handle lock after cloning/`&ServeClient` access, but the plan only required unlock across the generate recv loop.
- Suggestion: Leave as-is for step 1; only revisit if stop and visual absorb contend in production.
- Status: fixed
- Response: Left as-is per suggestion; step 1 only required unlock across generate recv. No product change.

## Verdict

**aligned**

Step 1 requirements (lock split, explicit UI↔mux id, STOP vs CANCEL wire, mock-pipe red/green tests) are present in `colibri-sys` engine code and tests. No missing plan items for this slice. No GPUI/desktop control-path work leaked into the slice (correctly deferred).
