# Implement report: default-on native logs and off-UI engine start

**Date:** 2026-08-13

Wizard Step 6 Ready (or rail Start) was freezing the GPUI thread inside `EngineSession::start`. GNOME then showed `"Unknown" Is Not Responding`. There was no last-run log file.

This slice does two things:

1. Default-on native file logging at `$XDG_DATA_HOME/colibri/logs/native.log`.
2. Dispatch engine start off the UI thread so the window keeps pumping while mmap / FFI open / process READY wait run.

Finish / Skip / Back still do not start the engine.

## RED (observed before product bodies)

Stubs compiled. Named contract tests failed.

```
cargo test -p colibri-sys --lib -- native_log_path native_log_enabled native_log_filter sanitize_log engine_start_log ensure_log_directory
```

Exit **101**. Failures:

- `paths::tests::native_log_path_uses_xdg_data_home` (stub `/stub-colibri-data/...`)
- `paths::tests::native_log_path_uses_home_local_share`
- `paths::tests::native_log_path_suffix_is_colibri_logs_native`
- `paths::tests::ensure_log_directory_creates_missing_path`
- `native_log::tests::native_log_filter_default_is_native_and_sys_info`
- `native_log::tests::native_log_filter_honors_rust_log`
- `native_log::tests::sanitize_log_text_redacts_hf_token_and_api_key`
- `native_log::tests::engine_start_log_line_has_path_not_secrets`

```
cargo test -p colibri-sys --lib -- native_log_disabled
```

Exit **101**. `native_log_disabled_for_off_and_zero` (stub always-on).

```
cargo test -p colibri-native --bin colibri-native -- rotating_file native_app_id should_dispatch engine_starting dispatch_blocking
```

Exit **101**. Failures:

- `should_dispatch_engine_start_blocks_generating_and_already_starting`
- `engine_starting_status_includes_elapsed_seconds`
- `dispatch_blocking_start_does_not_run_on_caller_thread`
- `native_app_id_is_org_colibri_native`
- `rotating_file_rotates_when_over_max`
- `rotating_file_writes_and_redacts_secrets`

No real 429 GB model was opened. Thread / preflight / path stubs only.

## GREEN (same tests after product)

```
cargo test -p colibri-sys --lib -- native_log_path native_log_enabled native_log_filter sanitize_log engine_start_log ensure_log_directory native_log_disabled platform_default resolve_override
```

Exit **0**. 11 passed.

```
cargo test -p colibri-native --bin colibri-native -- rotating_file native_app_id should_dispatch engine_starting dispatch_blocking start_async_preflight start_button append_native_log engine_session_start_preflight generate_async_errors
```

Exit **0**. 12 passed.

## Post-impl verify

| Step | Command | Exit |
|------|---------|------|
| fmt | `cargo fmt -p colibri-sys -p colibri-native` | 0 |
| fmt check | `cargo fmt -p colibri-sys -p colibri-native -- --check` | 0 |
| clippy sys | `cargo clippy -p colibri-sys --all-targets -- -D warnings` | 0 |
| clippy native | `cargo clippy -p colibri-native --all-targets -- -D warnings` | 0 |

## How to find the log on this host

`XDG_DATA_HOME` is unset. `HOME` is `/home/hunter`.

**File:** `/home/hunter/.local/share/colibri/logs/native.log`

Rotated backups (when the active file exceeds 4 MiB): `native.log.1`, `native.log.2`.

Disable later: `COLIBRI_LOG=off` or `COLIBRI_LOG=0`. `RUST_LOG=...` overrides the filter (default `colibri_native=info,colibri_sys=info`).

Lines also go to stderr when a terminal is attached (always dual-write when logging is on).

A panic hook appends `panic: ...` to the same file.

Do not expect prompts, generate tokens, HF tokens, or API keys in this file. The writer redacts `hf_...`, `sk-...`, and common `HF_TOKEN=` / `API_KEY=` assignments as a backstop.

## Behavior

### A. Default-on native log

- Init in `main` before `Application::new`.
- Directory created on first line.
- Native `eprintln!` sites replaced with `tracing`.
- `EngineSession::start` logs begin/end (ffi vs process, model path, elapsed ms, error).
- Wizard / rail / chat-send Start clicks log `source=` (`wizard_ready`, `rail`, `chat_send`).
- FFI open fallback and generate fallback are `warn`.
- Window `app_id` is `org.colibri.native`. Title stays `colibrì`.

### B. Start does not freeze the UI

- Start click sets `starting`, paints `"Starting engine…"`, returns.
- Worker thread drops any previous session, then runs `EngineSession::start`.
- UI poll (250 ms) updates `"Starting engine… still starting (Ns)"`. No fake percent.
- Second Start while `starting` or `generating` does not begin another open on the UI thread.
- Chat send while starting does not wait on the UI thread; it shows the living start line.
- Finish still only persists prefs and closes the wizard.
- When start finishes, existing ready / error copy. FFI then process fallback stays on the same "still starting" line.

## Files changed

- `crates/colibri-sys/src/paths.rs` (data dir + log path helpers)
- `crates/colibri-sys/src/native_log.rs` (new: enable / filter / sanitize / start line)
- `crates/colibri-sys/src/lib.rs` (exports)
- `crates/colibri-native/Cargo.toml` (`tracing`, `tracing-subscriber`)
- `crates/colibri-native/src/log_init.rs` (new: rotating file, subscriber, panic hook, app id)
- `crates/colibri-native/src/host.rs` (start wrap, off-thread dispatch, status helpers)
- `crates/colibri-native/src/main.rs` (init, `app_id`, async start, in-progress Start paint)
- `crates/colibri-native/src/notify_os.rs` (tracing instead of `eprintln!`)
