# Plan alignment review r2: stop/cancel sys (step 1 only)

**Date:** 2026-08-10
**Role:** plan-alignment specialist (L2), read-only
**Plan step under review:** Native desktop residuals step 1 (colibri-sys only)
**Prior plan review:** [`.agents/reports/review-stop-sys-plan.md`](review-stop-sys-plan.md) (verdict: aligned)
**Impl (step 1):** [`.agents/reports/impl-stop-cancel-sys.md`](impl-stop-cancel-sys.md)
**Impl (review fix follow-up):** [`.agents/reports/impl-stop-sys-fix.md`](impl-stop-sys-fix.md)
**Process mop (post-fix):** [`.agents/reports/process-mop-stop-sys-fix.md`](process-mop-stop-sys-fix.md) (72 lib tests green)
**Recon SoT:** [`.agents/reports/recon-plan-four-gaps.md`](recon-plan-four-gaps.md) §3 + recommended order item 1 (sys half)
**Code scope checked:** `crates/colibri-sys/src/engine/{mod,serve,duplex}.rs`; GPUI only for out-of-scope leak check

## What step 1 requires (sys only)

From recon §3 “Sys fixes before or with UI” and recommended order **1** (sys slice, not the GPUI button):

| Requirement | Plan meaning |
|-------------|----------------|
| Unlock stop during generate | Do not hold `EngineHandle` mutex across `recv` / `rx.recv()` so STOP/CANCEL can run mid-stream |
| Explicit UI ↔ mux id | `ClientFrame` / duplex `req_id` is the mux `SUBMIT` id (not accidental lockstep with auto `next_id`) |
| Distinct STOP vs CANCEL wire | `stop_request` → `STOP`; `cancel_request` → `CANCEL`; duplex Stop/Cancel split |
| Red/green mock-pipe tests | Concurrency + wire + id mapping contracts guarded in-tree |

**Not** step 1 (sys): GPUI Stop button, keep session live during generate, visual pump product path, Phase D FFI.

## Checklist after stop-sys fix

| Plan item | Evidence in tree (post-fix) | Status |
|-----------|------------------------------|--------|
| Split locking mid `generate_stream` | `EngineHandle::generate_stream` (`mod.rs` ~140–160): lock → `begin_generate` → unlock → `recv_loop` → lock absorb. Doc states concurrent `with_client` STOP/CANCEL. Stdin STOP/CANCEL use `ServeClient`’s own `stdin: Mutex`. | **Met** |
| Map UI `req_id` to mux SUBMIT | `GenerateRequest::request_id: Option<u64>` (`serve.rs` ~39); `begin_generate` writes that id on `SUBMIT` (~281–365); duplex `handle_submit` sets `request_id: Some(req_id)` (`duplex.rs` ~167–176). Same id on Stop/Cancel frames. | **Met** |
| `cancel_request` → `CANCEL`; Stop → `STOP` | `cancel_request` / `stop_request` (`serve.rs` ~385–397). Duplex: `ClientFrame::Stop` → `stop_request`, `Cancel` → `cancel_request` (`duplex.rs` ~122–129). | **Met** |
| Red/green mock pipes | Original step 1 suite plus fix-pass: mid-stream stop **and** cancel, shutdown-wake, duplicate id, write-fail pending cleanup, `next_id` bump. Mop: 72 lib tests green. | **Met** |

## Fix-pass vs plan (hardening, not scope change)

`impl-stop-sys-fix` closed review bugs opened by the unlock design. All stay inside colibri-sys engine behavior required for a safe step 1 ship:

| Fix | Why it is still step 1 | Plan gap if missing? |
|-----|------------------------|----------------------|
| `shutdown` drains `pending` with “shutting down” | Unlock mid-recv newly allows process `EngineHandle::stop` / `shutdown` during generate; without wake, hang forever | Correctness of concurrent handle use after lock split (review bug, not a new plan bullet) |
| `pending.remove` on SUBMIT write/flush fail | Touched path in `begin_generate` (new API for unlock) | Pending leak under error (review bug) |
| Reject duplicate in-flight explicit ids | Protocol uniqueness; `Clone` + concurrent hosts | Silent overwrite of first flight (review suggestion, fixed) |
| `mid_stream_cancel_no_deadlock`, `explicit_request_id_bumps_next_auto_id`, shutdown-wake tests | Symmetric red proof for cancel + allocator + process stop | Coverage gaps only; primary STOP/id/wire already met in r1 |

None of these pull GPUI, install, Brain, or FFI into the slice.

## Scope check (step 1 vs later residual)

| Later residual | In this slice after fix? |
|----------------|---------------------------|
| GPUI Stop button / keep session live during generate | **No.** Desktop still has `generating` gate and `generate_async` that `take()`s the session (`host.rs` ~255–274); no Stop/Cancel control path under `colibri-desktop-gpui`. |
| Visual pump concurrency product work | **No.** Unlock enables it; no new pump-during-generate product path. |
| True FFI Phase D | **No.** |

Supporting sys-only pieces (still in-scope for step 1):

- `EngineHandle: Clone` + last-clone `Drop` shutdown
- Process shutdown wake of pending waiters (fix pass)
- Re-export / compile field `request_id: None` on examples/tests (from original impl)

## Open residual (not plan misses for step 1)

| Item | Severity | Notes |
|------|----------|-------|
| tests Issue 5: fixed `sleep(100ms)` on zero-id mock READY | nit / deferred | Still open in tests review; not a named step 1 contract |
| `with_client` holds handle mutex for whole callback | nit | Plan only required unlock across generate recv; left as-is (plan r1 Issue 2) |
| GPUI Stop + live session during generate | next residual | Correctly out of step 1 (sys only) |
| Optional `recv_loop_error_cancelled_after_cancel_request` serve-only test | optional | Engine mid-stream cancel already covers ERROR CANCELLED e2e |

## Issues

No plan misses for step 1. No out-of-scope desktop work in the fix pass.

### Issue 1 -- Severity: none (tracking only)
- Description: Step 1 **sys** is complete; the recon order item 1 still has a **UI half** (Stop button, session not taken for the whole generate) that is intentionally not done.
- Status: open residual, not a step 1 gap
- Response: Do not re-open step 1 for UI; next implement slice owns GPUI control path on top of this sys surface.

## Verdict

**aligned**

Step 1 requirements (lock split, explicit UI↔mux id, STOP vs CANCEL wire, mock-pipe red/green tests) remain present and are **strengthened** by the stop-sys fix pass (shutdown wake, pending cleanup, duplicate-id reject, mid-stream cancel + related tests). No missing plan items for the sys-only slice. No GPUI/desktop control-path leakage. Open items are deferred nits or the next residual (GPUI Stop), not plan gaps for step 1.
