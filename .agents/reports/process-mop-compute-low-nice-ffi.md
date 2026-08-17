# Process mop: compute low-nice FFI

**Date:** 2026-08-13  
**Slice:** CPU FFI in-process niceness (thread + OpenMP team)  
**Implement report:** `.agents/reports/impl-compute-low-nice-ffi.md`

Floor-sweep only. No product edits. No residual invented. Index left alone.

## Commands and exit codes

| Step | Command | Exit |
|------|---------|------|
| 1 fmt | `cargo fmt -p colibri-sys -p colibri-native` | 0 |
| 1b check | `cargo fmt --check -p colibri-sys -p colibri-native` | 0 |
| 2 clippy sys | `cargo clippy -p colibri-sys --all-targets --features ffi -- -D warnings` | 0 |
| 3 clippy native | `cargo clippy -p colibri-native --all-targets -- -D warnings` | 0 |
| 4a tests | `cargo test -p colibri-sys --lib process_priority` | 0 (5 passed) |
| 4b tests | `cargo test -p colibri-sys --lib --features ffi process_priority` | 0 (6 passed) |
| 4c tests | `cargo test -p colibri-sys --lib --features ffi glm_open_invokes_compute_thread_niceness` | 0 (1 passed) |
| 4d tests | `cargo test -p colibri-native dispatch_blocking_start` | 0 (2 passed) |
| 4e tests | `cargo test -p colibri-native generate_async_errors_when_no_session` | 0 (1 passed) |

Clippy for both packages finished from incremental `dev` artifacts (sys ~0.19s after a package-cache wait, native ~0.32s). That is a real `-D warnings` pass, not a skip. Native clippy printed the existing `proc-macro-error2` future-incompat note; it is not a deny-warnings failure and is not in the niceness files.

## Fmt dirtied the tree?

No. `cargo fmt` wrote nothing. `cargo fmt --check` on the same packages also exited 0. Working-tree dirt on the niceness files is the implementer's already-landed edits, not rustfmt.

## Mop fixes

None. Nothing failed. No compile, clippy, or test hygiene edits.

## C hook

Did not run a full `make -C c` marathon. The existing targeted contract is `process_priority::tests::coli_nice_compute_threads_nices_openmp_team_not_caller`. It ran under command 4b (ffi `process_priority`) and passed. That is the in-tree check for `coli_nice_compute_threads`.

## GPUI still default

No regression noticed.

- `set_current_thread_nice` in native is only at the top of the `thread::spawn` closures in `generate_async` and `dispatch_blocking_start`. Those comments say the worker is not GPUI.
- FFI `apply_ffi_compute_niceness` nices the calling thread plus the OpenMP team. Comments say call only from FFI start / generate workers, not the UI thread.
- Tests that encode "not the caller / not the parent process" stayed green:
  - `set_current_thread_nice_does_not_change_other_thread`
  - `apply_does_not_demote_parent_process`
  - `coli_nice_compute_threads_nices_openmp_team_not_caller`
  - `dispatch_blocking_start_nices_worker_not_caller`
  - `dispatch_blocking_start_does_not_run_on_caller_thread`

No `setpriority` of the whole pid and no `/proc/self/task` walk in the niceness path.

## Still failing

Nothing in this mop. All listed commands exited 0.
