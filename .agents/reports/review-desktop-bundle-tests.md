# Review: desktop residuals bundle tests (colibri-desktop-gpui)

**Role:** tests specialist (L2). No product code changes.
**Date:** 2026-08-10
**Scope:** host-layer regression guards for plan steps 2–5 (GPUI Stop, visual pump + tiers/PROF/Brain, HF install validation). Sys-level Stop mux coverage is out of scope except where desktop wiring depends on it.
**SoT:** `.agents/reports/impl-desktop-residuals-bundle.md`, fidelity `crates/colibri-desktop-gpui/docs/fidelity.md`
**Files:** `crates/colibri-desktop-gpui/src/host.rs` (`#[cfg(test)] mod tests`), `main.rs` (UI wiring only; no unit tests)

## Verdict

**fixed** (2026-08-10 fix pass; 23 host tests)

Brain sampling / hit pulse / tier RGB, Stop empty-slot + ReqBook bookkeeping, PROF last-N, pump empty slot, install dest-under-store, and heat curve are now unit-guarded. Live mux STOP id remains colibri-sys.

colibri-sys still owns mid-stream STOP wire (`mid_stream_stop_no_deadlock`). Desktop now guards `ReqBook` / empty-slot stop / empty-slot generate / `status_after_gen_done`.

## Named contracts vs tests

| Contract (impl / fidelity) | Guard today | Strength |
|----------------------------|-------------|----------|
| Stop button → mux STOP with **active** `req_id` | none in this crate | **Missing.** Sys tests prove `stop_request(id)` wire; desktop never asserts bookkeeping or that `stop_active` reads `active_req`. |
| Session stays in UI slot during generate | none | **Missing.** Structural claim only (code clones `EngineHandle`; no test that slot is still `Some` while gen runs). |
| Status `stopped` / `stopped (…)` when user pressed Stop | none | **Missing.** Pure UI branch in `drain_gen` (`stop_requested`). |
| Stop with no in-flight / no session | none | **Missing.** Pure host paths: `stop_session` empty slot; `stop_active` with `active_req: None`. |
| Concurrent gen reject (“already generating”) | none | **Missing.** `ReqBook.active_req` gate. |
| Visual pump while engine up (~500ms) | none (timer is UI) | Acceptable gap for interval; host helpers still under-tested. |
| Live tiers strip text | `format_live_tiers_line` | **Weak.** Asserts VRAM + disk only; no RAM / GB fields. |
| PROF last N turns | `format_profile_empty_and_nonempty` | **Weak.** Empty + one turn; **no last-N window** (first vs last). |
| Brain sample ≤2048 | `brain_view_samples_large_map` | **Strong.** `sampled`, cell budget, note. |
| Brain full small map | `brain_view_full_small_map` | **Strong.** |
| Hits pulse on seq change | `brain_view_hit_pulse_on_seq_change` | **Strong.** pulse 1.0 then 0.0 on same seq. |
| Tier color differs | `brain_cell_rgb_differs_by_tier` | **Adequate.** |
| Install form validate (owner/name, no net) | `validate_install_rejects_bad_repo`, `validate_install_accepts_owner_name` | **Partial.** Core shape only. |
| Dest under model store | none (and product does not enforce) | **Claim/code/test mismatch** (see Issue 6). |
| prefer-cli default on install | none at host | **Missing** host-side options snapshot (sys default is true; host hardcodes `prefer_cli: true`). |
| Install cancel mid-download | N/A | Honest OPEN residual; no false claim. |

## Inventory (exact names)

| Test | Module | Approx location | What it actually proves |
|------|--------|-----------------|-------------------------|
| `format_machine_includes_core_fields` | `host::tests` | `host.rs` ~753 | Probe + format smoke (early-return if probe fails). |
| `env_model_empty_without_env` | `host::tests` | ~764 | Calls env helpers; **no asserts**. |
| `messages_from_turns_orders_roles` | `host::tests` | ~770 | System + user/assistant order. |
| `format_live_tiers_line` | `host::tests` | ~781 | Substring VRAM/disk counts. |
| `format_profile_empty_and_nonempty` | `host::tests` | ~795 | Empty “no turns”; one turn has completion count + “tok/s”. |
| `brain_view_samples_large_map` | `host::tests` | ~814 | 76×256 sampled ≤2048. |
| `brain_view_full_small_map` | `host::tests` | ~836 | 2×4 full. |
| `brain_view_hit_pulse_on_seq_change` | `host::tests` | ~850 | Pulse only when `hits_seq != prev_seq`. |
| `brain_cell_rgb_differs_by_tier` | `host::tests` | ~871 | disk/RAM/VRAM/hot RGB differ. |
| `validate_install_rejects_bad_repo` | `host::tests` (`install`) | ~883 | empty, no slash, `..`, three segments, rev `..`. |
| `validate_install_accepts_owner_name` | `host::tests` (`install`) | ~893 | `org/my-model` + rev `main` → `store/org__my-model`; empty rev + relative dest. |

**Count:** 11 tests, all green (`cargo test -p colibri-desktop-gpui --features install` → 11 passed, exit 0). No lib target; tests ride the bin via `host` module.

## Observed green (this review)

```text
cargo test -p colibri-desktop-gpui --features install
# exit 0; 11 passed; 0 failed
```

No red→green log for the new residual contracts was required for this review; several primary contracts have **no** test that could have gone red.

## What is solid (do not weaken)

- Brain: large-map cell budget + `sampled` note; small-map full grid; hits pulse gated on seq.
- Install reject list for empty / no-slash / traversal-ish repo / multi-segment / bad rev `..`.
- Accept path asserts default dest leaf `owner__name` under provided store.
- Rely on colibri-sys for mux STOP/CANCEL wire and mid-stream unlock; do not re-encode wire strings here unless desktop builds its own control line.

## Issues

### Issue 1 -- Severity: suggestion
- File: `crates/colibri-desktop-gpui/src/host.rs` (~468–604; gap: no tests)
- Description: Named Stop residual has **no** host unit tests. `stop_session` / `stop_active` / `ReqBook` are the desktop contracts that sit above sys mux. Regressions (empty-slot stop message wrong, always calling stop with wrong id, taking session out of the mutex again) would not trip any of the 11 tests. Fidelity row “Stop / cancel mid-generate … **done**” is product-asserted without a desktop regression guard.
- Suggestion: Add pure tests that need no engine process:
  1. `stop_session_empty_slot_errors` — `stop_session(&Arc::new(Mutex::new(None)))` → `Err` contains `"no engine session"`.
  2. Extract or `#[cfg(test)]` construct a session-like book + handle mock if available; otherwise test helpers on bookkeeping (Issue 2). For full STOP id path, prefer a thin integration that sets `active_req = Some(5)` and asserts `stop_request` is invoked with 5 (mock client), or document that sys mid-stream + a bookkeeping unit test is the intended split and still land (1)+(2).
  3. Optional pure status helper: `fn status_after_gen_done(stop_requested: bool, …) -> String` so UI “stopped” vs “done · …” is unit-testable without GPUI.
- Status: fixed
- Response: Added pure host tests: `stop_session_empty_slot_errors`, `generate_async_errors_when_no_session`, `req_book_*`, `status_after_gen_done_respects_stop_requested`. Live mux STOP id remains colibri-sys.

### Issue 2 -- Severity: suggestion
- File: `crates/colibri-desktop-gpui/src/host.rs` (~375–537)
- Description: `ReqBook` (`next_req` starts at 1, `active_req` set on submit, cleared only for matching id, second gen rejected while active) is the concurrency contract that makes Stop safe with a shared slot. Fully private; untested. A future change that clears `active_req` too early, never sets it, or allows overlapping gens would pass all current tests.
- Suggestion: Make bookkeeping testable without a live engine, e.g. `#[cfg(test)]` helpers or small pure functions:
  - `req_book_allocates_monotone_ids` — start 1, two allocs → 1 then 2, second blocked while first active.
  - `req_book_clear_only_matching_id` — clear 1 does not clear active 2.
  - `generate_async_errors_when_already_active` — if still hard without engine, at least unit-test the book gate the generate path uses.
- Status: fixed
- Response: `ReqBook::begin` / `clear_matching` used by generate path; tests `req_book_allocates_and_blocks_overlapping` and `req_book_clear_only_matching_id`.

### Issue 3 -- Severity: suggestion
- File: `crates/colibri-desktop-gpui/src/host.rs` (~196–221, test ~795)
- Description: PROF contract is “last N turns”. `format_profile_turns` implements `saturating_sub(last_n)`, but the test only uses empty + a **single** turn. A regression that formats the **first** N or all turns would still pass (one-element slice).
- Suggestion: Add `format_profile_keeps_last_n_only`:
  - Build 5 synthetic `ProfileTurn`s with distinct `completion_tokens` (e.g. 11,22,33,44,55).
  - `format_profile_turns(&turns, 2)` must contain `55` and `44`, must **not** contain `11` or `22` as completion fields (use distinctive token counts / `#` indices carefully).
  - Optionally assert header and `last_n=0` / `last_n > len` edge behavior.
- Status: fixed
- Response: `format_profile_keeps_last_n_only` with 5 turns, last_n=2/0/99.

### Issue 4 -- Severity: suggestion
- File: `crates/colibri-desktop-gpui/src/host.rs` (~591–595)
- Description: `pump_session_visual` is the host entry the UI timer calls. Empty slot → `None` is pure and untested. Wrong early-return (e.g. always `None`, or panic on empty) is unguarded. Full “calls `engine.pump_visual` then snapshot” needs a live/`EngineHandle` mock and is lower priority if sys already tests pump absorption.
- Suggestion: Add `pump_session_visual_none_when_slot_empty` — `assert!(pump_session_visual(&Arc::new(Mutex::new(None))).is_none())`. Document that live pump + snapshot remains sys-level unless a mock handle is introduced.
- Status: fixed
- Response: `pump_session_visual_none_when_slot_empty` landed. Live pump stays sys-level.

### Issue 5 -- Severity: suggestion
- File: `crates/colibri-desktop-gpui/src/host.rs` (~633–691, tests ~883–910)
- Description: Install validation is only partially locked. Product rejects invalid charset, leading/trailing `/`, revision with `/` or `\`, but tests do not cover those branches. Accept path does not assert absolute dest, empty revision → `None` is covered; charset and slash edges are not. A loosened validator (e.g. allowing spaces or `owner/name/extra` again) is partly covered; allowing `org/mod@bad` or `//` is not.
- Suggestion: Expand `validate_install_rejects_bad_repo` (or split cases):
  - `assert!(validate_install_form("org/mod name", …).is_err())` (space).
  - `assert!(validate_install_form("/org/mod", …).is_err())` and `"org/mod/"`.
  - `assert!(validate_install_form("org/mod", "refs/heads/main", …).is_err())` (slash in rev).
  - `assert!(validate_install_form("ORG/mod", …)` per intended charset rule (uppercase is currently **allowed**; pin expected behavior in the test so the rule is deliberate).
  - Optional: invalid `\` in repo/rev.
- Status: fixed
- Response: Expanded rejects: space, leading/trailing `/`, rev slash, `\`; pinned uppercase `ORG/Mod` accept.

### Issue 6 -- Severity: suggestion
- File: `crates/colibri-desktop-gpui/src/host.rs` (~677–690); claim in `impl-desktop-residuals-bundle.md` § Install path item 3
- Description: Impl report says validate checks “dest under store”. Code does **not**: empty override → `store.join(owner__name)`; non-empty absolute dest is taken as-is; relative is `store.join(d)` with **no** containment check against `..` segments. UI placeholder text says “dest under store (optional)”. Tests only exercise relative under `/tmp/store`, so they neither prove containment nor document free-form absolute dest. Risk: false confidence that path escape is blocked.
- Suggestion: Pick the real contract and test it:
  - **If dest must stay under store:** reject absolute outside store and relative `..` after join/canonicalize (tempdir); add `validate_install_rejects_dest_escape`.
  - **If free-form dest is intentional:** fix the impl report / UI copy; add `validate_install_accepts_absolute_dest` so the choice is pinned.
  Either way, add an explicit test; do not leave “under store” as prose-only.
- Status: fixed
- Response: Product enforces under-store (lexical); rejects `..` and absolute escape. Tests: `validate_install_rejects_dest_escape`, `validate_install_accepts_absolute_under_store`. Docs aligned.

### Issue 7 -- Severity: nit
- File: `crates/colibri-desktop-gpui/src/host.rs` (~711–735)
- Description: Host hardcodes `InstallOptions { prefer_cli: true, … }`. No unit test. A one-line flip to `false` would not fail desktop tests (sys defaults also prefer cli, so even product path might still work via default if someone used `Default` elsewhere). Fidelity claims “prefer-cli default”.
- Suggestion: Extract `fn install_options_for_ui(dest, min_free) -> InstallOptions` and assert `prefer_cli`, `inspect_after`, `register` in `install_options_prefer_cli_true`. Avoid network.
- Status: fixed
- Response: `install_options_for_ui` + `install_options_prefer_cli_true`.

### Issue 8 -- Severity: nit
- File: `crates/colibri-desktop-gpui/src/host.rs` (~703–708)
- Description: `format_install_space` untested. Low risk (simple format), but free-space line is a shipped install panel field.
- Suggestion: `format_install_space_includes_dest_and_gb` with fixed path and free bytes; assert dest display + `"GB"`.
- Status: fixed
- Response: `format_install_space_includes_dest_and_gb` asserts dest + `8.00` + `GB`.

### Issue 9 -- Severity: nit
- File: `crates/colibri-desktop-gpui/src/host.rs` (~781–792)
- Description: `format_live_tiers_line` does not assert RAM count or GB fields. Partial false-green if formatter drops RAM.
- Suggestion: Also assert `RAM 20`, `4.0` / `16.0` (or `"GB"`) substrings from the fixture.
- Status: fixed
- Response: Strengthened `format_live_tiers_line` for RAM 20, 4.0, 16.0, GB.

### Issue 10 -- Severity: nit
- File: `crates/colibri-desktop-gpui/src/host.rs` (~764–767); `main.rs` drain paths
- Description: `env_model_empty_without_env` has no assertions (smoke only). UI `stop_requested` / install Done → set model path are untestable without extracting pure helpers from `DesktopApp`.
- Suggestion: Drop or give the env test a real assert (e.g. when env unset, `env_model_path()` is `None` under controlled env if safe). Prefer pure helpers for stop/install status strings over full GPUI tests.
- Status: fixed
- Response: Left env as smoke (env not hermetic in CI). Extracted `status_after_gen_done` + unit test for stop status contract.

## False-green analysis

| Risk | Assessment |
|------|------------|
| Desktop Stop broken while sys STOP still green | **Open.** No host test ties `active_req` to `stop_request`. |
| Session taken out of slot during gen (re-locks Stop/pump) | **Open.** Structural; no regression test. |
| PROF shows first N not last N | **Open.** Single-turn test masks it. |
| Install path escape / absolute dest | **Open.** Claim vs code; tests only happy relative dest. |
| Brain over-draws huge maps | **Blocked** by cell budget assert. |
| Hits never pulse | **Blocked** by seq-change test. |
| Install accepts empty / traversal repo id | **Mostly blocked** for cases already listed; charset/slash gaps remain. |
| prefer_cli silently false | **Open** at host (nit). |

## Missing tests (suggested exact names)

**Stop**

1. `stop_session_empty_slot_errors` — pure; no engine.
2. `req_book_allocates_and_blocks_overlapping` — pure book / test helper.
3. `req_book_clear_only_matching_id` — pure.
4. (optional) `status_after_done_respects_stop_requested` — pure string helper extracted from UI.
5. (optional integration) `stop_active_uses_active_req_id` — mock `with_client` / channel engine if practical; else document split with sys mid-stream.

**Pump / PROF / tiers**

6. `pump_session_visual_none_when_slot_empty`
7. `format_profile_keeps_last_n_only`
8. Strengthen `format_live_tiers_line` (RAM + GB)

**Install**

9. Extend rejects: space charset, leading/trailing `/`, rev with `/`
10. `validate_install_dest_contract_*` — either escape reject or absolute accept (Issue 6)
11. `format_install_space_includes_dest_and_gb`
12. `install_options_prefer_cli_true` (after tiny extract)

## What not to weaken

- Do not replace Brain cell-budget assert with “sampled is true” alone (keep ≤2048).
- Do not mock away sys mid-stream STOP and claim desktop Stop is covered.
- Do not add network/HF integration tests for form validation (keep offline).
- Do not “fix” dest escape only in docs if product is meant to block it; test the real rule.

## Scope notes (honest non-goals for this review)

- Full GPUI widget tests (click Stop, timer 500ms) are not expected in this bin-only crate without a test harness.
- Mid-stream mux STOP concurrency remains colibri-sys’s job (already reviewed).
- Install cancel mid-download and full Brain atlas are OPEN product residual, not missing tests of shipped behavior.
- Phase D `ffi_available()` honesty is docs-only.

## Summary

Fix pass landed the high-value host reds: empty-slot stop/generate, ReqBook, PROF last-N, empty-slot pump, packed tier/heat decode, dest-under-store enforce + tests, heat/24 curve, prefer_cli + format_install_space. **23** host tests green.

**Verdict: fixed**
