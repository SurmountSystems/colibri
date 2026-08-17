# Review: desktop residuals bundle (plan steps 2–6)

**Date:** 2026-08-10
**Scope:** GPUI Stop, visual pump + tiers/PROF, Brain panel, HF install, Phase D honesty.
**Sources:** `.agents/reports/impl-desktop-residuals-bundle.md`, `.agents/reports/process-mop-desktop-bundle.md`, `.agents/RESIDUAL.md`, `crates/colibri-desktop-gpui/{src/host.rs,src/main.rs,docs/fidelity.md,README.md}`, `crates/colibri-sys/docs/ffi-phase-d.md`, web reference `web/src/Brain.tsx`, serve protocol STOP/DONE.
**Role:** L2 primary general review (role-swap). **No code changes** in this pass.

## Summary

The **session-not-orphaned Stop design is correct** and is the strongest part of this bundle: `generate_async` keeps `EngineSession` in the UI slot, clones `EngineHandle`, allocates an explicit `req_id` into a shared `ReqBook`, and Stop uses `with_client(stop_request)` against the same id. That matches plan step 1 sys unlock work and protocol `STOP → normal DONE`.

Visual pump, live tiers, PROF strip, install form wiring, and Phase D honesty docs are largely in good shape. Residual OPEN rows match real gaps (install cancel, min-free gate, full atlas, pulse decay).

**Not clean:** Brain heat brightness does **not** match the web reference curve (wrong heat mapping for typical log2 heat values). Critical host paths for Stop/session bookkeeping lack unit tests. Install form does **not** enforce “dest under store” despite the impl report claim; absolute dest escapes the store.

## What looks solid

| Area | Evidence |
|------|----------|
| Session stays in slot during generate | `host.rs` `generate_async`: lock → clone handle + book + id → unlock; never `take()`s the `Option<EngineSession>` |
| Stop concurrent with generate | `stop_session` → `stop_active` → `engine.with_client(|c| c.stop_request(id))`; sys unlock during `recv_loop` (prior stop-sys) |
| STOP vs CANCEL product choice | UI Stop uses mux **STOP** (graceful → DONE). Status `"stopped"` on `GenEvent::Done` when `stop_requested` |
| Req id bookkeeping | `ReqBook { next_req, active_req }`; set before duplex submit; `clear_active` on template fail and after `handle_with` |
| No second generate while active | Worker rejects if `active_req.is_some()`; UI also gates on `generating` |
| Visual pump while engine up | `ensure_visual_pump` ~500ms; `pump_session_visual` → `pump_visual` + snapshot; exits when slot empty |
| Pump vs generate concurrency | Pump locks session briefly then handle mutex; generate holds handle only for begin/absorb; no HTTP |
| Tiers / PROF formatting | Pure helpers + unit tests; last N = 8 in UI |
| Brain sampling ≤2048 | `BRAIN_MAX_CELLS`; stride sample; src vs disp + “sampled” note; tests for large/small map |
| Hits pulse on seq change | `pulse_on` when `hits_seq != prev_seq && hits_seq > 0`; hit bit at **source** index; unit test |
| Tier base colors | Match web `TIER_RGB` spirit: disk `(58,71,80)`, RAM `(90,155,216)`, VRAM `(78,214,165)` |
| Install feature gate | `default = ["install"]` → `colibri-sys/install`; form + progress channel + error path |
| prefer_cli | `InstallOptions { prefer_cli: true, … }` hard-coded in `install_async` |
| Free space display | `install_free_bytes` / `format_install_space`; `disk_free_bytes` walks nearest existing ancestor |
| min_free honesty | UI sets `min_free = 0`; residual `open:install-min-free-gate` is accurate |
| Phase D honesty | UI strip, README table, fidelity matrix, `ffi-phase-d.md` host-in-process ≠ engine-in-process; `ffi_available() == false` |
| Residual honesty | CLOSED vs OPEN list matches code (no mid-download cancel, no full atlas, no install cancel) |
| Mop green | 11 desktop tests, 72 sys lib tests, clippy `-D warnings` on both packages (mop report) |

## Focus checklist

| Focus | Result |
|-------|--------|
| Stop mid-generate; session not orphaned | **Pass** (architecture + code path). No host unit test for the contract. |
| Visual pump concurrency with generate | **Pass** (shared `EngineHandle`, unlocked stream recv at sys layer). |
| Brain sampling / heat mapping | **Partial.** Sampling OK. **Heat brightness wrong vs web** (Issue 1). Packed tier/heat decode not asserted (Issue 4). |
| Install progress/error; prefer_cli; free space | **Pass** for progress/error channel + prefer_cli + free line. Soft min_free=0 documented. Dest “under store” **overclaimed** (Issue 2). |
| Honesty FFI vs process | **Pass** |
| Missing tests for critical paths | **Fail** for Stop/session and packed Brain decode (Issues 3–4). Format helpers covered. |

## Issues

### Issue 1 -- Severity: bug
- File: crates/colibri-desktop-gpui/src/host.rs:327-349 (`brain_cell_rgb`)
- Description: Web Brain (`web/src/Brain.tsx`) scales heat as `lum = 0.35 + 0.65 * min(heat / 24, 1)` so heat saturates at **24** (log2-style heat buckets rarely need the full 0..63 range). Native uses `heat_f = heat / 63.0` and `0.45 + 0.55 * heat_f`. At heat=12, web ~0.68 vs native ~0.56; at heat=24, web is **full bright** while native is still ~0.66. Operators reading the native Brain will systematically under-read routing heat relative to the SPA and the product pitch (“brightness = routing heat”). Tier RGB bases match; the **heat curve does not**. Comment claims “matches web TIER_RGB spirit” only for base colors.
- Suggestion: Align `brain_cell_rgb` with web: `heat_f = (heat as f32 / 24.0).clamp(0.0, 1.0)` and `lum = 0.35 + 0.65 * heat_f`, then `channel * lum` (plus existing pulse). Add a unit test that heat=24 yields near-max channel for a tier (and heat=12 is materially brighter under /24 than under /63). Pulse decay multi-frame remains OPEN residual; one-shot pulse is fine.
- Status: fixed
- Response: Aligned `brain_cell_rgb` to web curve (`heat/24`, lum `0.35+0.65*heat_f`). Unit test `brain_cell_rgb_heat_saturates_at_24` pins full VRAM RGB at heat=24/63 and mid-heat /24 brightness. Pulse decay remains OPEN residual.

### Issue 2 -- Severity: suggestion
- File: crates/colibri-desktop-gpui/src/host.rs:677-684 (`validate_install_form`); impl report line on “dest under store”
- Description: Impl report claims form validation enforces “dest under store.” Code allows **absolute** `dest_override` without checking it lives under the model store (`if d.is_absolute() { d } else { store.join(d) }`). Relative dests are under store; absolute paths can install anywhere the process can write. Residual does not list a containment rule. Either enforce containment (canonicalize + `starts_with(store)`) or drop the “under store” claim and document absolute dest as intentional operator choice.
- Suggestion: Prefer relative-only + store join for MVP, or reject absolute paths that escape store with a clear error. Test both. Update impl/fidelity wording to match.
- Status: fixed
- Response: Enforced dest under store via `resolve_install_dest` / `path_is_under_store` (lexical component prefix). Rejects `..` and absolute paths outside store; absolute under store still allowed. Tests: `validate_install_rejects_dest_escape`, `validate_install_accepts_absolute_under_store`. Fidelity + impl report wording updated.

### Issue 3 -- Severity: suggestion
- File: crates/colibri-desktop-gpui/src/host.rs:486-587 (generate/stop); tests module
- Description: Plan step 2’s critical contract is “Stop mid-generate; session not orphaned; STOP with active req_id.” Sys has strong tests (`mid_stream_stop_no_deadlock`, duplex STOP id). **Desktop host has zero tests** for: empty-slot generate error, `active_req` set/clear semantics (would need a small pure/test seam or mock handle), or “Stop while `generating` with no `active_req` yet” (UI race between `generating = true` and worker setting `active_req` → `stop_session` returns “nothing generating” while Send is still running). Structural review says the happy path is right; regression risk on future refactors is high without a host test.
- Suggestion: (1) Unit test `generate_async` with `Arc<Mutex<None>>` → channel receives `GenEvent::Error` containing “no engine”. (2) Extract or `#[cfg(test)]` helpers for req-id allocate/clear and assert single-flight. (3) Optional: document the short `generating`/`active_req` race; consider setting `active_req` only after SUBMIT success, or have Stop retry/no-op until id appears without flipping `stop_requested` on hard fail.
- Status: fixed
- Response: Added `ReqBook::begin` / `clear_matching`; tests `stop_session_empty_slot_errors`, `generate_async_errors_when_no_session`, `req_book_allocates_and_blocks_overlapping`, `req_book_clear_only_matching_id`, `status_after_gen_done_respects_stop_requested`. Stop race: UI still sets `stop_requested` on stop fail while generating (Issue 6).

### Issue 4 -- Severity: suggestion
- File: crates/colibri-desktop-gpui/src/host.rs:835-848, 813-833 (brain tests)
- Description: `brain_view_full_small_map` packs bytes `64..67` (tier 1) but never asserts `cells[i].0` / `.1` decode via `tier_at`/`heat_at`. `brain_view_samples_large_map` packs `(2<<6)|40` at index 0 but does not assert the sampled cell 0 is VRAM tier 2 heat 40. Sampling size tests pass while a broken tier/heat extract would still green.
- Suggestion: Assert packed decode on the full small map (e.g. index 0 → tier 0 heat 0; index 4 → tier 1 heat 0; index 7 → tier 1 heat 3). On the large map, assert `view.cells[0] == (2, 40, 0.0)` when (0,0) is always the first sample.
- Status: fixed
- Response: Full small map asserts indices 0/3/4/7 tier+heat; large map asserts `cells[0]` is tier 2 heat 40.

### Issue 5 -- Severity: nit
- File: crates/colibri-desktop-gpui/src/host.rs:582-584 (`generate_async` after `handle_with`)
- Description: On `handle_with` `Err`, worker always sends `GenEvent::Error(format!("generate: {e}"))` even when the stream callback already emitted `ServerFrame::Error` (duplex sets `saw_engine_error` and still returns `Err`). For **Stop** this is fine: protocol is DONE, not ERROR. For engine ERROR / future Cancel path, UI can process two terminal errors in one `drain_gen` and overwrite status / append two error lines. Low urgency for current Stop-only UI.
- Suggestion: Only send the trailing Error when no terminal frame was already emitted (mirror duplex `saw_engine_error`), or treat first terminal event as final in `drain_gen`.
- Status: fixed
- Response: `saw_terminal` flag in generate worker; trailing `GenEvent::Error` only when no Done/Error frame was already sent.

### Issue 6 -- Severity: nit
- File: crates/colibri-desktop-gpui/src/main.rs:315-330, 487-516
- Description: (a) Brief race: UI sets `generating` before worker sets `active_req`; immediate Stop fails with “nothing generating” / “stop failed” while generation continues; `stop_requested` stays false so Done will show normal “done · …” not “stopped”. (b) Install poll is independent of visual pump (good), but `ensure_visual_pump` from `start_install` is a no-op without an engine session (also fine; `schedule_install_poll` covers install). Worth a one-line comment near Stop so a future “fix” does not invent a second pump.
- Suggestion: On stop fail due to missing `active_req` while `generating`, still set `stop_requested` and/or retry once after a short poll; or set `active_req` earlier under the same lock that the UI uses for generating. Document install poll vs visual pump.
- Status: fixed
- Response: `stop_generate` sets `stop_requested = true` on any stop fail while generating so Done still shows “stopped”. Comment documents the race.

### Issue 7 -- Severity: nit
- File: crates/colibri-desktop-gpui/src/host.rs:711-744 (`install_async`); tests
- Description: `prefer_cli: true` is correct and matches product default, but there is no test that pins the option (easy to regress to `Default` if someone rewrites the call). `format_install_space` has no unit test (trivial string contract).
- Suggestion: Tiny test that builds the same `InstallOptions` shape (or assert via a thin pure `default_install_options(dest)` helper) for `prefer_cli` and `min_free_bytes`. One assert on `format_install_space` text.
- Status: fixed
- Response: Extracted `install_options_for_ui`; tests `install_options_prefer_cli_true` and `format_install_space_includes_dest_and_gb`.

## Non-issues / correctly deferred

| Item | Why OK |
|------|--------|
| No mid-download cancel | Residual `open:install-cancel`; sys has no first-class cancel |
| min_free_bytes = 0 | Residual `open:install-min-free-gate`; free space is informational |
| Hits one-shot pulse (no RAF decay) | Residual `open:brain-pulse-decay` |
| Sampled Brain, no atlas hover | Residual `open:brain-full-atlas`; fidelity **partial** |
| No live HWINFO strip UI | Residual `open:live-hwinfo-strip` |
| True libcolibri FFI | Residual `open:ffi-phase-d`; docs honest |
| `start_engine` blocked while generating | Prevents killing the process mid-stream from UI |

## Verdict

**fixed** (2026-08-10 fix pass)

| Severity | Open count | Fixed |
|----------|------------|-------|
| bug | 0 | 1 heat mapping |
| suggestion | 0 | 3 dest + Stop tests + packed decode |
| nit | 0 | 3 double Error, Stop race, prefer_cli pin |
| **total open** | **0** | **7** |

## Suggested fix order

1. `brain_cell_rgb` align to web heat/24 + unit test (Issue 1).
2. Assert packed decode in existing Brain tests (Issue 4).
3. Host test: `generate_async` with no session (Issue 3).
4. Resolve dest-under-store claim vs absolute path (Issue 2).
5. Optional nits 5–7 if still in the same pass.
