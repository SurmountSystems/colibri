# Report: live `[prefill]` status and C banners in native.log

Prefill progress is now a status phrase plus a stderr tee. Generate stays 0% until the first decode token. The GNOME SIGKILL on the pasted run was Force Quit, not a C crash. Mid-prefill cancel was not added.

## What landed

- `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/prefill.rs`
  Parse the C `[prefill]` banner. Format `Prefill layer 13/78 · 47 tokens` (native-only operational English; C prints singular `token`; not an i18n key). Snapshot lives on a mutex that is not the FFI engine mutex. `apply_prefill_status` never locks the engine mutex.
- `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/stderr_tee.rs`
  Host stderr tee **before** tracing init: `pipe` + `dup2` onto fd 2. Drain thread reads lines, echoes each line to the saved TTY fd, appends only `[` C banners to `native.log` via `append_native_log_line` / `sanitize_log_text`. Tracing lines stay one copy in the file.
- `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/main.rs`
  Install the tee in `main` before `init_native_logging`. 40 ms generate poll and 500 ms visual pump set the chip from the snapshot while `generating && live_token_count == 0`. New generate clears a stale tick. Generate bar is unchanged.
- `serve.rs` was not changed. Process child `Stdio::inherit()` already hits host stderr after the tee.

## TDD

Red (stubs: parse `None`, format `""`, apply `None`), before product logic:

```
cargo test -p colibri-native --bin colibri-native -- \
  parse_prefill_line format_prefill_status apply_prefill_status_does_not_take_engine_mutex
```

Fail reasons:

- `parse_prefill_line_extracts_layer_total_tokens`: `singular token banner` (`None`)
- `format_prefill_status_is_plain_operational_english`: left `""`, right `"Prefill layer 13/78 · 47 tokens"`
- `apply_prefill_status_does_not_take_engine_mutex`: left `None`, right `Some("Prefill layer 13/78 · 47 tokens")`
- `parse_prefill_line_rejects_stop_banner` already passed (stub was `None`)

Green after product edit (same filters plus existing honesty tests):

```
cargo test -p colibri-native --bin colibri-native -- \
  parse_prefill_line format_prefill_status apply_prefill_status_does_not_take_engine_mutex \
  pump_visual_try_lock_returns_last_snapshot_when_mutex_held \
  generate_progress_zero_tokens_is_zero_percent \
  handle_stderr_line_appends_c_banners_only
```

Result: 7 passed, 0 failed.

## Post-impl verify

| Step | Command | Outcome |
|------|---------|---------|
| fmt | `cargo fmt -p colibri-native` | clean (sys not touched) |
| clippy | `cargo clippy -p colibri-native --all-targets -- -D warnings` | exit 0 |
| tests | filter above | 7 passed |

## What you should run

Rebuild and install the native host (`just install`, or the same `cargo install --path crates/colibri-native ...` you already use). Then run **that new binary**. The old process still has no tee and no chip.

After rebuild, a GLM FFI chat should:

1. Keep the generate bar at **0%** until the first decode token (still honest).
2. Replace the stuck `Generating... 0%` chip with `Prefill layer 13/78 · 47 tokens` while prefill prints.
3. Append `[stop]` and `[prefill]` lines to `~/.local/share/colibri/logs/native.log` (and still paste them on a terminal).
4. Stay responsive enough that Force Quit is optional. Prefill still cannot be cancelled mid-`step`.
