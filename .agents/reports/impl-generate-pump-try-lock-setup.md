# impl: generate visual pump try_lock + hide rail Setup after Finish

**Date:** 2026-08-13

Generate stayed off the UI thread. The window still froze because the 500ms visual pump called `engine.lock()` while `coli_glm_generate` held that mutex for tokenize + prefill + decode. GPUI sat in `Mutex::lock`. GNOME said `"org.colibri.native" Is Not Responding`. Tokens never painted because `drain_gen` was queued behind that stuck update.

After Finish, `first_run_done` was already true. The giant green rail `btn-setup` (`p.primary`) still always painted.

No 429 GB model was loaded. Proof is unit tests plus compile.

## Contract A — window keeps pumping during generate

| Piece | Behavior |
|-------|----------|
| `pump_visual_try_lock` | `try_lock` on the engine mutex. On miss, return the last snapshot immediately. Never `lock()`. |
| `EngineSession::pump_visual` | FFI path uses that helper. Caches `last_visual`. Process path unchanged (mux, no FFI mutex). |
| `pump_session_visual` | `try_lock` on the session slot. On miss, `None` so the UI keeps the last painted snapshot. |
| `apply_visual_snapshot` | Still UI-thread apply only. Calls `pump_session_visual`. Does not wait on generate. |
| Generate | Stays on the worker. Still holds the engine mutex for the whole FFI call. `coli_glm_visual_poll` is not called concurrently (try_lock is the serializer). |
| Stop | `request_ffi_generate_cancel` sets the existing cancel flag. Does not take the engine mutex. C `spec_decode` cancel was not rewritten. Stop can run because the event loop is no longer stuck. |
| 0% | Unchanged honesty: 0 generated / N max is 0%. First bump is still the first decode token. |
| Logs | `generate begin` / `generate end` via `format_generate_log`. Kind, req_id, elapsed_ms, sanitized error. No prompt text, no tokens. |

## Contract B — rail Setup after Finish

| Piece | Behavior |
|-------|----------|
| `show_rail_setup_primary_cta(first_run_done)` | `false` when first-run is done |
| `rail_setup_primary_fill` | `Some(p.primary)` only when that helper is true; else `None` (do not paint the slab) |
| Left rail `btn-setup` | Painted only when fill is `Some` |
| Tools `tools-btn-setup` | Muted text control (`setup.open`), not a primary full-width rail slab |
| `setup.reopen` | Native-only operational: "Open setup again anytime from Tools." / Italian Strumenti |
| Hero first-run CTA | Still gated on `!first_run_done` |

## TDD

Observed RED (exit 101), then GREEN (exit 0). Same test names.

RED command (blocking helper + always-show rail stub + old reopen copy):

```text
cargo test -p colibri-native --bin colibri-native -- \
  pump_visual_try_lock_returns_last_snapshot_when_mutex_held \
  pump_session_visual_does_not_block_when_session_mutex_held \
  show_rail_setup_primary_cta_false_when_first_run_done \
  rail_setup_primary_fill_absent_after_first_run \
  setup_reopen_hint_points_at_tools_not_only_rail \
  generate_progress_zero_tokens_is_zero_percent
```

RED result: **exit 101**. Failures:

- `pump_visual_try_lock_returns_last_snapshot_when_mutex_held` — poll ran after waiting (helper used `lock()`)
- `pump_session_visual_does_not_block_when_session_mutex_held` — waited 400.096967ms
- `show_rail_setup_primary_cta_false_when_first_run_done` — helper still true after Finish
- `rail_setup_primary_fill_absent_after_first_run` — left `Some(65280)`, right `None`
- `setup_reopen_hint_points_at_tools_not_only_rail` — copy still said "from the left rail"
- `generate_progress_zero_tokens_is_zero_percent` — already ok (honest 0%)

GREEN (after try_lock + rail gate + Tools path + generate log lines):

```text
cargo test -p colibri-native --bin colibri-native -- \
  pump_visual_try_lock_returns_last_snapshot_when_mutex_held \
  pump_visual_try_lock_polls_when_mutex_free \
  pump_session_visual_does_not_block_when_session_mutex_held \
  request_ffi_generate_cancel_does_not_wait_on_engine_mutex \
  show_rail_setup_primary_cta_false_when_first_run_done \
  rail_setup_primary_fill_absent_after_first_run \
  setup_reopen_hint_points_at_tools_not_only_rail \
  generate_progress_zero_tokens_is_zero_percent \
  show_first_run_setup_cta_false_when_first_run_done
```

GREEN result: **9 passed, exit 0**.

```text
cargo test -p colibri-sys --lib generate_log_line_has_kind_not_prompt_or_tokens
```

GREEN result: **1 passed, exit 0**.

Related existing tests also ok: `generate_async_errors_when_no_session`, `pump_session_visual_none_when_slot_empty`, `stop_session_empty_slot_errors`, `wizard_and_tools_keys_en_it`.

## Verify

```text
cargo fmt -p colibri-native -p colibri-sys                          # exit 0
cargo clippy -p colibri-native --all-targets -- -D warnings         # exit 0
cargo clippy -p colibri-sys --all-targets -- -D warnings            # exit 0
```

## Files

- `crates/colibri-native/src/host.rs` — try_lock pump, last snapshot cache, generate begin/end, FFI cancel helper
- `crates/colibri-native/src/main.rs` — apply_visual_snapshot note, rail slab gate, Tools reopen control
- `crates/colibri-native/src/i18n.rs` — `setup.reopen` en/it
- `crates/colibri-native/src/progress.rs` — keep-guard for 0 generated → 0%
- `crates/colibri-native/docs/fidelity.md` — reopen path is Tools
- `crates/colibri-sys/src/native_log.rs` — `format_generate_log`
- `crates/colibri-sys/src/lib.rs` — export

## Not in this slice

- C `spec_decode` still does not read `e->stop`. Stop is cooperative after prefill's first token callback.
- Generate % denominator is still max output tokens (`open:generate-progress-redesign` stays deferred).
- No real GLM weights were opened.
