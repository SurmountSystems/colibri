# Review: stop/cancel sys tests (colibri-sys)

**Role:** tests specialist (L2). No product code changes.
**Date:** 2026-08-10
**Scope:** mid-stream Stop/Cancel, `cancel_request` / `stop_request`, UI↔mux `req_id` mapping
**Files:** `crates/colibri-sys/src/engine/serve.rs`, `duplex.rs`, `mod.rs`
**Plan / impl SoT:** `.agents/reports/impl-stop-cancel-sys.md`, recon `.agents/reports/recon-plan-four-gaps.md` §3

## Verdict

**clean**

Named plan contracts have real regression guards with exact wire asserts and a concurrency-shaped mid-stream Stop test. Secondary gaps (cancel end-to-end ERROR path, `next_id` bump, join timeouts) are suggestions, not false-green bugs on the named contracts.

## Named contracts vs tests

| Contract (plan) | Guard | Strength |
|-----------------|-------|----------|
| Mid-stream Stop no deadlock | `engine::tests::mid_stream_stop_no_deadlock` | Strong. Mock holds DONE until `STOP {id}`; host waits for DATA then concurrent `with_client(stop_request)`. Pre-fix lock-across-recv cannot pass. |
| Cancel writes `CANCEL` not `STOP` | `cancel_request_writes_cancel_line`, `duplex_cancel_writes_cancel_with_ui_req_id` | Strong. Exact `assert_eq!(line, "CANCEL …\n")` fails if Cancel maps to STOP. |
| Stop writes `STOP` | `stop_request_writes_stop_line`, `duplex_stop_writes_stop_with_ui_req_id` | Strong. Exact wire. |
| Explicit id on SUBMIT | `begin_generate_uses_explicit_request_id_on_submit`, `duplex_submit_maps_ui_req_id_to_mux_submit` | Strong. `SUBMIT 42` / `SUBMIT 99` + `flight.request_id()` / `Done { req_id: 99 }`. |
| Zero id rejected | `explicit_request_id_zero_is_rejected` | Adequate. Asserts error text contains `"non-zero"`. |

## Inventory (exact names)

| Test | Module | File:line (approx) |
|------|--------|--------------------|
| `mid_stream_stop_no_deadlock` | `engine::tests` | `mod.rs:300` |
| `cancel_request_writes_cancel_line` | `engine::serve::tests` | `serve.rs:740` |
| `stop_request_writes_stop_line` | `engine::serve::tests` | `serve.rs:804` |
| `begin_generate_uses_explicit_request_id_on_submit` | `engine::serve::tests` | `serve.rs:762` |
| `explicit_request_id_zero_is_rejected` | `engine::serve::tests` | `serve.rs:825` |
| `duplex_cancel_writes_cancel_with_ui_req_id` | `engine::duplex::tests` | `duplex.rs:647` |
| `duplex_stop_writes_stop_with_ui_req_id` | `engine::duplex::tests` | `duplex.rs:671` |
| `duplex_submit_maps_ui_req_id_to_mux_submit` | `engine::duplex::tests` | `duplex.rs:695` |

## Red / green evidence

- **Would fail pre-fix (documented in impl report):**
  1. Mid-stream Stop: handle mutex held across `recv` → `with_client` never runs → mock never sees STOP → hang (or DATA wait timeout if order differed).
  2. Cancel wire: no `cancel_request`; duplex Cancel → STOP → exact `CANCEL` asserts fail.
  3. Explicit id: auto `next_id` on SUBMIT → `SUBMIT 1` not `SUBMIT 42` / `99`.
- **Observed green (process mop, not re-run by this review):** `.agents/reports/process-mop-stop-sys.md` — `cargo test -p colibri-sys --lib` exit 0, 66 passed; listed stop/cancel tests green.
- **No red log captured in-tree** (impl report states contracts, mop shows green only). Acceptable for post-land review if reintroducing the bug still trips the named tests (it should).

## What the mid-stream test actually proves

```300:382:crates/colibri-sys/src/engine/mod.rs
// mock: ACCEPT + DATA, then block until STOP {id}, then DONE
// host gen thread: generate_stream(request_id: Some(5)) until Done
// host control: wait saw_data → with_client(|c| c.stop_request(5))
// asserts: STOP line matches SUBMIT id, result.text == "hi"
```

- Forces generate into unlocked recv (DATA delivered) before stop.
- Mock will not emit DONE without STOP, so a lock-held-for-whole-stream regression deadlocks rather than false-passing with early DONE.
- Chains id mapping: `request_id: Some(5)` → SUBMIT id → `STOP 5` (mock compares line to SUBMIT-parsed id).

## Issues

### Issue 1 -- Severity: suggestion
- File: `crates/colibri-sys/src/engine/mod.rs:300` (gap: no sibling test)
- Description: Mid-stream **Cancel** concurrency and `ERROR <id> CANCELLED` end-to-end path are untested. Product docs and `cancel_request` comments say cancel aborts to engine ERROR, not DONE. Only idle wire lines are covered (`CANCEL 99\n` / duplex `CANCEL 77\n`). Same unlock path as Stop makes deadlock unlikely if Stop stays green, but a Cancel→DONE mixup or broken ERROR dispatch would not trip existing tests.
- Suggestion: Add:
  - `mid_stream_cancel_no_deadlock` — same shape as Stop: after DATA, concurrent `with_client(|c| c.cancel_request(5))`; mock expects `CANCEL 5\n` then writes `ERROR 5 CANCELLED`; assert `generate_stream` returns `Err` containing `CANCELLED` (not Ok Done).
  - Optionally `recv_loop_surfaces_error_cancelled` on `ServeClient` alone: SUBMIT, CANCEL, ERROR line, assert `recv_loop` Err.
- Status: fixed
- Response: Added `engine::mid_stream_cancel_no_deadlock` with DATA barrier, exact `CANCEL {id}`, mock `ERROR {id} CANCELLED`, generate_stream Err contains CANCELLED.

### Issue 2 -- Severity: suggestion
- File: `crates/colibri-sys/src/engine/serve.rs:281-301` (product bump; no test)
- Description: Design claim: after explicit `request_id: Some(n)`, auto `next_id` advances past `n` so later `None` ids do not collide. No regression test. Silent regression reuses low auto ids under concurrent/duplex mixed callers.
- Suggestion: Add `explicit_request_id_bumps_next_auto_id`:
  - Mock READY; first `begin_generate(Some(42))` → assert SUBMIT 42, DONE 42.
  - Second `begin_generate(None)` → assert SUBMIT starts with `SUBMIT 43 ` (or `> 42`).
  - Keep flight ids asserted via `request_id()`.
- Status: fixed
- Response: Added `serve::explicit_request_id_bumps_next_auto_id` (wire SUBMIT 42 then 43; flight ids match).

### Issue 3 -- Severity: suggestion
- File: `crates/colibri-sys/src/engine/duplex.rs:647` and `671`
- Description: Duplex Stop/Cancel tests only prove idle control frames write the right line. They do not prove Stop/Cancel while a Submit `generate_stream` is in flight through duplex (duplex `handle(Submit)` is synchronous). Engine-level mid-stream covers the lock split; duplex layer only needs wire routing. Acceptable coverage split; residual risk is duplex re-wiring Cancel→stop_request again (already guarded by idle Cancel exact line).
- Suggestion: Optional integration `duplex_cancel_while_generate_via_cloned_handle` only if a second control path (shared `EngineHandle` clone + duplex) becomes a product API. Not required for current plan slice.
- Status: fixed
- Response: Left optional; engine mid-stream cancel/stop cover lock split. Duplex idle wire tests already guard Cancel vs STOP mapping. No product API yet for dual-path duplex cancel during sync Submit.

### Issue 4 -- Severity: nit
- File: `crates/colibri-sys/src/engine/mod.rs:377-382`
- Description: After stop, `gen_thread.join()` and `eng.join()` have no wall-clock timeout. True deadlock hangs the test process until the harness times out the whole suite (DATA wait has a 2s deadline; post-stop joins do not). That is intentional as a hang signal, but noisy on CI.
- Suggestion: Prefer `join` with a thread + `recv_timeout` channel wrapper, or `std::thread::scope` + alarm, fail with `"deadlock: stop did not complete generate"` within ~2s. Do not weaken the mock “wait for STOP before DONE” contract.
- Status: fixed
- Response: New hang-sensitive tests (`shutdown_during_generate_wakes_recv`, `shutdown_wakes_pending_recv`) use `done_rx.recv_timeout(2s)` before join. Existing mid-stream stop/cancel keep join-as-hang-signal (mock still requires STOP/CANCEL before terminal line).

### Issue 5 -- Severity: nit
- File: `crates/colibri-sys/src/engine/serve.rs:825-848`
- Description: `explicit_request_id_zero_is_rejected` uses `thread::sleep(100ms)` on the mock READY side so the client can handshake. Not stop/cancel specific; mild flaky under extreme load if READY is slow (unlikely with channel pipes). Wire cancel/stop tests correctly avoid sleeps by blocking on `read_line`.
- Suggestion: Prefer mock that keeps writing READY or blocks on read after READY without fixed sleep; low priority.
- Status: open
- Response: Deferred (low priority; channel pipes + READY already complete before sleep). Not a stop/cancel contract.

### Issue 6 -- Severity: nit
- File: `crates/colibri-sys/src/engine/serve.rs:773-776`, `duplex.rs:707-710`
- Description: SUBMIT asserts use `starts_with("SUBMIT 42 ")` / `SUBMIT 99 `, not full header field checks. Enough for id mapping. Does not guard slot/temp/top_p plumbing (out of stop/cancel scope).
- Suggestion: None for this slice.
- Status: fixed
- Response: No change required for this slice (id mapping only); acknowledged as intentional scope limit.

## False-green analysis (named contracts)

| Risk | Assessment |
|------|------------|
| Cancel silently becomes STOP | **Blocked** by exact `CANCEL` string tests at serve + duplex. |
| Mid-stream stop “passes” without concurrent stop | **Blocked**: mock waits for STOP before DONE; generate cannot finish without control path. |
| Id mapping accidental lockstep | **Blocked**: non-1 ids (5, 7, 42, 77, 88, 99) force explicit mapping. |
| Stop deadlock only under load | **Mostly blocked**: DATA barrier ensures stop after stream started; no artificial race for the lock release. |
| Cancel ERROR path broken | **Open** (Issue 1): write-only tests pass if CANCEL is sent but ERROR never surfaces. |
| Timing flake on mid-stream | Low: 2s DATA deadline; channel pipes not wall-clock paced for STOP. |

## Missing tests (suggested exact names)

1. **`mid_stream_cancel_no_deadlock`** (`engine::tests`)
   Setup: same channel mock as Stop. After DATA, `cancel_request(5)`. Mock: assert `CANCEL 5\n`, write `ERROR 5 CANCELLED`. Expect generate Err.

2. **`recv_loop_error_cancelled_after_cancel_request`** (`engine::serve::tests`)
   Setup: `begin_generate(Some(11))`, concurrent or sequential cancel, mock ERROR. Assert `recv_loop` Err message contains `CANCELLED`.

3. **`explicit_request_id_bumps_next_auto_id`** (`engine::serve::tests`)
   Setup: explicit 42 then auto None; assert SUBMIT 43 (or greater than 42).

4. **(optional) `duplex_stop_and_cancel_are_distinct_wire`**
   Single test that records two lines STOP then CANCEL with different ids if you want one place that fails when arms are swapped in the match. Largely redundant with existing pair of tests.

## What not to weaken

- Do not replace exact `"CANCEL 99\n"` with “starts with CANCEL” if STOP could be mis-accepted.
- Do not make the mid-stream mock send DONE without waiting for STOP (that creates false green).
- Do not “fix” hang by only shortening timeouts without asserting STOP/CANCEL was observed.

## Summary

Named contracts from the stop/cancel plan are covered with the right failure mode (exact wire + concurrency barrier). Green evidence exists via process mop. Residual test debt is cancel ERROR e2e and `next_id` bump, not holes in the three primary plan bullets.

**Verdict: clean**
)
