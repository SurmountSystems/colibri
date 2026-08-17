# Review r2: desktop residuals bundle tests (colibri-desktop-gpui)

**Role:** tests specialist re-review (L2). No product code changes.
**Date:** 2026-08-10
**Prior:** `.agents/reports/review-desktop-bundle-tests.md` (r1; issues marked fixed)
**Impl fix SoT:** `.agents/reports/impl-desktop-bundle-fix.md`
**Scope:** host-layer regression guards for plan steps 2–5 (Stop, visual pump + tiers/PROF/Brain, HF install). Sys mid-stream STOP wire remains colibri-sys.
**Files read:** `crates/colibri-desktop-gpui/src/host.rs` (`#[cfg(test)] mod tests` + helpers), `main.rs` drain/stop wiring for status contract only.

## Verdict

**clean**

Stop empty-slot + ReqBook + Done status, pump empty-slot, install dest-under-store + prefer_cli + form rejects, and Brain sample/pulse/heat-24 contracts are unit-guarded. All r1 Issues 1–10 are fixed in code and tests. Observed green: **23 passed**.

Live mux STOP with active `req_id` and full engine pump/snapshot remain intentionally colibri-sys. No blocking host-test gaps for shipped desktop contracts in this crate.

## Observed green (this re-review)

```text
cargo test -p colibri-desktop-gpui --features install
# test result: ok. 23 passed; 0 failed; 0 ignored
# exit 0
```

`--list` confirms 23 unit tests under `host::tests` (feature `install` on).

## Named contracts vs tests (post-fix)

| Contract | Guard | Strength |
|----------|-------|----------|
| Stop empty slot | `stop_session_empty_slot_errors` | **Strong.** `Err` contains `"no engine session"`. |
| Generate empty slot | `generate_async_errors_when_no_session` | **Strong.** Channel `GenEvent::Error` with `"no engine"` (2s timeout). |
| ReqBook single-flight + ids | `req_book_allocates_and_blocks_overlapping` | **Strong.** id 1, second begin `"already generating"`, clear → id 2. |
| ReqBook clear matching only | `req_book_clear_only_matching_id` | **Strong.** clear 99 leaves active 1. |
| Status `"stopped"` vs `"done · …"` | `status_after_gen_done_respects_stop_requested` | **Strong** for Done path. UI uses same helper on Done. |
| Stop → mux STOP with active `req_id` | none in this crate | **Acceptable split.** `stop_active` reads `active_req` then `with_client(stop_request)`; wire proven in colibri-sys (`mid_stream_stop_no_deadlock`). |
| Session stays in slot during gen | none | **Structural.** Code clones handle and keeps session in mutex; no host regression test. Not a false fidelity claim. |
| UI Error status `stopped ({e})` | none pure | **Nit residual.** Inline in `main.rs` drain; Done path is extracted and tested. |
| Pump empty slot | `pump_session_visual_none_when_slot_empty` | **Strong.** |
| Live pump + snapshot | sys / live engine | **Acceptable gap** for this crate (no mock handle). |
| Live tiers (VRAM/RAM/disk + GB) | `format_live_tiers_line` | **Strong.** VRAM 10, RAM 20, disk 100, 4.0, 16.0, GB. |
| PROF last N only | `format_profile_keeps_last_n_only` (+ empty/nonempty) | **Strong.** 5 turns, last_n=2 keeps c=55/44, drops 11/22/33; last_n=0/99 edges. |
| Brain sample ≤2048 | `brain_view_samples_large_map` | **Strong.** budget + sampled note + **packed tier/heat decode** at cell 0. |
| Brain full small map | `brain_view_full_small_map` | **Strong.** full grid + decode asserts on corners. |
| Hits pulse on seq change | `brain_view_hit_pulse_on_seq_change` | **Strong.** |
| Tier RGB differs | `brain_cell_rgb_differs_by_tier` | **Adequate.** |
| Heat curve heat/24 (web align) | `brain_cell_rgb_heat_saturates_at_24` | **Strong.** 24==63 full VRAM base RGB; mid heat /24 brightness pin; cold lum 0.35. |
| Install repo/rev rejects | `validate_install_rejects_bad_repo` | **Strong.** empty, no slash, `..`, 3 segments, space, leading/trailing `/`, rev slash, `\`; uppercase accept pinned. |
| Install accept owner/name + relative dest | `validate_install_accepts_owner_name` | **Strong.** default `store/owner__name`; relative join. |
| Dest under store (no escape) | `validate_install_rejects_dest_escape` | **Strong.** relative `..`, absolute outside, absolute with `..`. |
| Absolute under store allowed | `validate_install_accepts_absolute_under_store` | **Strong.** |
| prefer_cli (and panel options) | `install_options_prefer_cli_true` | **Strong.** prefer_cli, inspect_after, register, min_free, dest. |
| Free-space line | `format_install_space_includes_dest_and_gb` | **Strong.** path + `8.00` + GB. |
| Install cancel mid-download | N/A | Honest OPEN residual. |

## Inventory (exact names, 23)

| Test | Proves |
|------|--------|
| `format_machine_includes_core_fields` | Probe smoke (early return if probe fails). |
| `env_model_empty_without_env` | Smoke only (env not hermetic). |
| `messages_from_turns_orders_roles` | System + user/assistant order. |
| `format_live_tiers_line` | VRAM/RAM/disk counts + GB fields. |
| `format_profile_empty_and_nonempty` | Empty + one turn. |
| `format_profile_keeps_last_n_only` | Last-N window + edges. |
| `brain_view_samples_large_map` | Sample budget + tier/heat at (0,0). |
| `brain_view_full_small_map` | Full 2×4 + packed decode. |
| `brain_view_hit_pulse_on_seq_change` | Pulse only on seq change. |
| `brain_cell_rgb_differs_by_tier` | disk/RAM/VRAM/hot differ. |
| `brain_cell_rgb_heat_saturates_at_24` | Web heat/24 curve. |
| `status_after_gen_done_respects_stop_requested` | stopped vs done string. |
| `stop_session_empty_slot_errors` | Empty-slot stop message. |
| `pump_session_visual_none_when_slot_empty` | Empty-slot pump → None. |
| `generate_async_errors_when_no_session` | Empty-slot generate error event. |
| `req_book_allocates_and_blocks_overlapping` | Begin / overlap / clear / next id. |
| `req_book_clear_only_matching_id` | Clear only matching id. |
| `validate_install_rejects_bad_repo` | Expanded reject + uppercase pin (`install`). |
| `validate_install_accepts_owner_name` | Happy path dest under store (`install`). |
| `validate_install_rejects_dest_escape` | Escape reject (`install`). |
| `validate_install_accepts_absolute_under_store` | Absolute under store (`install`). |
| `format_install_space_includes_dest_and_gb` | Space line (`install`). |
| `install_options_prefer_cli_true` | Panel InstallOptions (`install`). |

## r1 issue closeout

| r1 Issue | Status | Evidence |
|----------|--------|----------|
| 1 Stop host tests | **fixed** | empty-slot stop/generate, ReqBook, `status_after_gen_done` |
| 2 ReqBook bookkeeping | **fixed** | `begin` / `clear_matching` + two unit tests |
| 3 PROF last-N | **fixed** | `format_profile_keeps_last_n_only` |
| 4 Pump empty slot | **fixed** | `pump_session_visual_none_when_slot_empty` |
| 5 Install reject edges | **fixed** | space, `/`, rev slash, `\`, uppercase pin |
| 6 Dest under store | **fixed** | product enforce + escape/accept tests |
| 7 prefer_cli pin | **fixed** | `install_options_for_ui` + test |
| 8 format_install_space | **fixed** | dest + 8.00 GB |
| 9 tiers RAM/GB | **fixed** | strengthened asserts |
| 10 env smoke / stop status | **fixed** | env left smoke; status helper tested |

## False-green analysis (r2)

| Risk | Assessment |
|------|------------|
| Desktop Stop broken while sys STOP still green | **Mitigated for host bookkeeping.** Empty slot + ReqBook + status tested. Wire id still sys-only (documented split). |
| Session taken out of slot during gen | **Still structural only.** Acceptable without mock engine. |
| PROF shows first N not last N | **Blocked** by last-N test. |
| Install path escape | **Blocked** by dest escape + under-store absolute tests. |
| Brain over-draws huge maps | **Blocked** by cell budget. |
| Hits never pulse | **Blocked** by seq-change test. |
| Heat curve regresses to /63 | **Blocked** by saturate-at-24 + mid-heat r≥50 pin. |
| Packed tier/heat decode wrong | **Blocked** by small + large map decode asserts. |
| prefer_cli silently false | **Blocked** at host options helper. |

## What is solid (do not weaken)

- Brain: cell budget ≤2048, sampled note, packed decode on small and large maps, pulse on seq change only, heat/24 full VRAM RGB equality at 24 vs 63.
- Stop host surface: empty-slot messages, ReqBook single-flight and clear-matching, Done status via pure helper used by UI.
- Install: offline form validation, lexical under-store dest, prefer_cli snapshot, free-space line.
- PROF last-N window with distinctive completion counts.
- Rely on colibri-sys for mux STOP/CANCEL wire and mid-stream unlock; do not re-encode wire strings here.

## Remaining non-blocking gaps (not issues)

1. **No host test that `stop_active` invokes `stop_request(active_req)`** without a mock `EngineHandle`. Intended split with colibri-sys.
2. **No pure test for Error-path `stopped ({e})`** in `main.rs` (Done path only).
3. **`env_model_empty_without_env` still assertion-free** (honest smoke; CI env not hermetic).
4. **No GPUI widget / 500ms timer tests** (bin crate, no harness). Interval remains UI-only.

None of these reopen r1 blocking concerns or false-green the named stop/pump/install/brain contracts.

## Scope notes

- Full GPUI click/timer tests not expected.
- Install cancel, min-free hard gate, full Brain atlas, pulse multi-frame decay remain product OPEN residual, not missing tests of shipped behavior.
- Phase D honesty is docs-only.

## Summary

Fix pass closed every r1 host-test gap that mattered for desktop Stop, pump empty-slot, install dest/prefer_cli, Brain sample/pulse/heat, PROF last-N, and tiers fields. **23** host tests green. Live STOP wire and live pump stay at colibri-sys by design.

**Verdict: clean**
