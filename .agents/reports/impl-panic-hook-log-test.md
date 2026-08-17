# Implement report: panic hook writes `panic:` to the log file

**Date:** 2026-08-13

Named contract: after a Rust panic, the log file contains a `panic:` line.

## What landed

One test: `log_init::tests::panic_hook_writes_panic_line_to_log_file` in
`crates/colibri-native/src/log_init.rs`.

It installs `install_panic_hook` against a tempfile, `catch_unwind`s a panic,
asserts the file contains `panic:`, then restores the previous hook so the cargo
test harness stays intact. A mutex serializes `take_hook` / `set_hook` because
those are process-global.

No production behavior change. `install_panic_hook` already took
`Option<PathBuf>` and already appended `panic: {info}` through
`append_native_log_line`. That is already a tempfile-capable sink. Production
still writes the default-on `native.log`. The C banner tee is unchanged. No
Sentry, minidump, `sigaction`, crash reporter, or fd 2 remap in tests. The
429 GB GLM model was not opened.

## TDD

The test was written first. The first run was **green**, not red.

Command:

```
cargo test -p colibri-native --bin colibri-native -- panic_hook_writes_panic_line_to_log_file
```

Exit **0**. `log_init::tests::panic_hook_writes_panic_line_to_log_file` passed.
The harness printed the expected `catch_unwind` panic text
(`colibri-native panic-hook contract`) because the production hook still calls
the previous hook.

**Fail reason:** none. The product already wrote `panic:` to the given path.
I did not break the hook to invent a red. That would have been a fake red.

## Green (same filter)

```
cargo test -p colibri-native --bin colibri-native log_init::tests
```

Exit **0**. 5 passed, including the new test and the existing tempfile
`log_init` tests (`rotating_file_*`, `append_native_log_line_*`,
`native_app_id_*`).

```
cargo test -p colibri-native --bin colibri-native handle_stderr_line_appends_c_banners_only
```

Exit **0**. 1 passed.

## Post-impl verify

| Step | Command | Exit |
|------|---------|------|
| fmt | `cargo fmt -p colibri-native` then `cargo fmt -p colibri-native -- --check` | 0 |
| clippy | `cargo clippy -p colibri-native --all-targets -- -D warnings` | 0 |
| tests | filters above | 0 |

`colibri-sys` was not touched. No fmt/clippy on that package.

## Honesty about the operator crash

This test does **not** diagnose the operator crash. The last
`~/.local/share/colibri/logs/native.log` has no `panic:` line. It stops after a
successful FFI engine start. A Rust panic after log init would have written
that line. `SIGSEGV`, C `abort()`, and `SIGKILL` still will not write a panic
line. This test makes the panic-to-file contract fail in CI if someone breaks
the hook. It is not a last-run dump and it is not a hard-fault reporter.
