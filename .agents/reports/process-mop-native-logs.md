# Process mop: native logs and off-UI engine start

**Date:** 2026-08-13
**After:** implementer `019ffbee-95d6-7a00-aa81-2e9c7b430bc5`
**Implementer report:** `.agents/reports/impl-native-logs-and-start-off-ui.md`
**Scope:** fmt, clippy, named tests on `colibri-sys` and `colibri-native`. Fix compile/lint/test fallout only.

## Result

Clean mop. No product edits. No compile, lint, or test fallout.

## Commands and exit codes

| Step | Command | Exit |
|------|---------|------|
| 1. fmt | `cargo fmt -p colibri-sys -p colibri-native` | **0** |
| 1b. fmt check | `cargo fmt -p colibri-sys -p colibri-native -- --check` | **0** |
| 2. clippy sys | `cargo clippy -p colibri-sys --all-targets -- -D warnings` | **0** |
| 3. clippy native | `cargo clippy -p colibri-native --all-targets -- -D warnings` | **0** |
| 4. tests sys | `cargo test -p colibri-sys --lib -- native_log_path native_log_enabled native_log_filter sanitize_log engine_start_log ensure_log_directory native_log_disabled` | **0** |
| 5. tests native | `cargo test -p colibri-native --bin colibri-native -- rotating_file native_app_id should_dispatch engine_starting dispatch_blocking` | **0** |

## Clippy notes

- `colibri-sys`: finished in 0.15s, no warnings.
- `colibri-native`: finished in 0.28s. Build-script note that `c/libcolibri.a` is a HIP archive and is rebuilt CPU-only for `feature=ffi`. Future-incompat warning on `proc-macro-error2 v2.0.1` (dependency, not product). Neither is fallout from this slice.

## Test counts

**colibri-sys** (`--lib` filter above): **10 passed**, 0 failed, 166 filtered out.

- `native_log::tests::native_log_disabled_for_off_and_zero`
- `native_log::tests::engine_start_log_line_has_path_not_secrets`
- `native_log::tests::native_log_filter_default_is_native_and_sys_info`
- `paths::tests::native_log_path_suffix_is_colibri_logs_native`
- `native_log::tests::native_log_enabled_default_on`
- `paths::tests::native_log_path_uses_home_local_share`
- `paths::tests::native_log_path_uses_xdg_data_home`
- `native_log::tests::native_log_filter_honors_rust_log`
- `paths::tests::ensure_log_directory_creates_missing_path`
- `native_log::tests::sanitize_log_text_redacts_hf_token_and_api_key`

**colibri-native** (`--bin colibri-native` filter above): **6 passed**, 0 failed, 292 filtered out.

- `host::tests::should_dispatch_engine_start_blocks_generating_and_already_starting`
- `host::tests::engine_starting_status_includes_elapsed_seconds`
- `log_init::tests::native_app_id_is_org_colibri_native`
- `log_init::tests::rotating_file_rotates_when_over_max`
- `log_init::tests::rotating_file_writes_and_redacts_secrets`
- `host::tests::dispatch_blocking_start_does_not_run_on_caller_thread`

## Edits

None. Tree left as the implementer left it. No `git add` / `git commit`.
