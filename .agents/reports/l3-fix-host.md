# L3 report: host review-fix (Issues 2, 4, 7, 9, 11, 15)

Workflow spawn from L2 was blocked. Slice done in the L2 thread. Files: `crates/colibri-native/src/host.rs` only.

## Product

- `PLAN_ENV_WRITTEN`: operator-set env sticky; keys this function wrote refresh each Start.
- Inspect fail → `ENGINE_START_RAM_UNMEASURABLE` unless overcommit.
- `ffi_generate_error_should_fallback`: no process start on `stopped` / cancel; send `Done`.
- `ffi_open_error_should_fallback`: no process start on RAM / measure refuse.
- Refuse copy tests assert `ENGINE_START_RAM_TOO_SMALL`.

## RED

Compile-fail missing `clear_plan_env_written_for_tests`, `ffi_generate_error_should_fallback`, `ffi_open_error_should_fallback`. After product: refresh test `first == second` failed because live MemAvailable moved (`35.467` vs `35.475`); stale `999` was already overwritten.

## GREEN

Same test now asserts `999` does not stick and the new value is a positive plan number.

```
cargo test -p colibri-native --bin colibri-native -- apply_plan_env preflight_ram ffi_generate_stopped ffi_open_ram
```

Included in the 28-pass native targeted run. Exit 0. Env isolated with `PLAN_ENV_TEST` + `RestoreEnv`.
