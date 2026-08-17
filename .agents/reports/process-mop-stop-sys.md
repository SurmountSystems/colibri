# Process mop: colibri-sys (stop/cancel concurrency)

**Role:** process mop only (fmt → clippy → tests). No product feature work.
**Crate:** `colibri-sys`
**Context:** stop/cancel concurrency just landed (`impl-stop-cancel-sys`).
**Date:** 2026-08-10

## Commands

| Step | Command | Exit |
|------|---------|------|
| 1. fmt | `cargo fmt -p colibri-sys` | **0** |
| 2. clippy (default) | `cargo clippy -p colibri-sys --all-targets -- -D warnings` | **0** |
| 3. test (default lib) | `cargo test -p colibri-sys --lib` | **0** (66 passed) |
| 4. clippy (install) | `cargo clippy -p colibri-sys --all-targets --features install -- -D warnings` | **0** |
| 5. test (install lib) | `cargo test -p colibri-sys --features install --lib` | **0** (73 passed, 1 ignored) |

## Fallout fixes

None. No product or test edits required.

## Notes

- Install-feature suite includes one ignored live network test: `model::install::tests::live_hf_snapshot_tiny` (`live network: HF hub`).
- Stop/cancel related tests observed green under default lib run, including:
  - `engine::duplex::tests::duplex_cancel_writes_cancel_with_ui_req_id`
  - `engine::duplex::tests::duplex_stop_writes_stop_with_ui_req_id`
  - `engine::serve::tests::cancel_request_writes_cancel_line`
  - `engine::serve::tests::stop_request_writes_stop_line`
  - `engine::tests::mid_stream_stop_no_deadlock`

## Status

**Clean.** fmt, clippy (-D warnings), and lib tests pass with and without `--features install`.
