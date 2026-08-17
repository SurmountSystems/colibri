# Report: CPU FFI in-process niceness (thread + OpenMP team)

**Date:** 2026-08-13
**Residual:** `open:ffi-inprocess-priority` **closed**

## Named contract

Nice only in-process compute, not GPUI.

1. Linux nice is per-thread. `setpriority(PRIO_PROCESS, 0, 10)` from a worker nices that thread only.
2. Reuse `ENGINE_CHILD_NICE = 10`.
3. The generate worker and the engine-start worker call `set_current_thread_nice`.
4. After the start worker is niced, one OpenMP parallel region nices each libgomp team member (`coli_nice_compute_threads`).
5. Do not `setpriority` the whole pid. Do not walk `/proc/self/task`. Do not nice stderr tee or the visual pump.
6. Niceness does not stop systemd-oomd and does not schedule HIP kernels.

## Red (before product edit)

Test: `process_priority::tests::set_current_thread_nice_does_not_change_other_thread`

```text
cargo test -p colibri-sys --lib process_priority -- --exact set_current_thread_nice_does_not_change_other_thread
```

Fail: compile error `E0425` cannot find `set_current_thread_nice` (only private `set_current_process_nice` existed).

OpenMP / FFI-path tests (added next, still before the C hook):

```text
cargo test -p colibri-sys --lib --features ffi coli_nice_compute_threads_nices_openmp_team_not_caller
```

Fail: compile errors `E0425` / `E0603`: `coli_nice_compute_threads` and `coli_openmp_team_all_at_nice` missing; `apply_ffi_compute_niceness_call_count` missing.

## Green (same filters)

```text
cargo test -p colibri-sys --lib process_priority::tests::set_current_thread_nice_does_not_change_other_thread
# exit 0 — 1 passed

cargo test -p colibri-sys --lib --features ffi process_priority
# exit 0 — 6 passed (includes OpenMP team hook + apply_does_not_demote_parent_process)

cargo test -p colibri-sys --lib --features ffi glm_open_invokes_compute_thread_niceness
# exit 0 — 1 passed

cargo test -p colibri-sys --lib process_priority
# exit 0 — 5 passed (no ffi feature)

cargo test -p colibri-native dispatch_blocking_start
# exit 0 — 2 passed

cargo test -p colibri-native generate_async_errors_when_no_session
# exit 0 — 1 passed
```

## What landed

- Promoted private `set_current_process_nice` to public `set_current_thread_nice`. Child `pre_exec` still uses it.
- Native `generate_async` worker and `dispatch_blocking_start` (engine start / mmap / drop previous) call it at the top of the closure.
- C `coli_nice_compute_threads(nice)` runs `#pragma omp parallel { setpriority(PRIO_PROCESS, 0, nice) }`. Called after GLM `model_init` and at GLM generate / generate_ids. Rust `apply_ffi_compute_niceness` calls the same hook on every family open and generate (GLM, Kimi, Inkling, V4).
- `COLI_COMPUTE_NICE` is 10 and matches `ENGINE_CHILD_NICE`.

## What is niced vs still default

| Thread / work | Nice now |
|---------------|----------|
| Process-mode engine child | 10 (already; unchanged) |
| FFI start worker (`dispatch_blocking_start`) | 10 |
| FFI / process generate worker (`generate_async`) | 10 |
| OpenMP / libgomp team (FFI start + generate) | 10 |
| GPUI UI thread | default (0) |
| Visual pump | default |
| `colibri-stderr-tee` | default |
| HIP / ROCm kernels | not scheduled by nice |

## Residual

Closed `open:ffi-inprocess-priority` in `.agents/RESIDUAL.md`. Process-mode child was already niced. FFI now nices the current thread plus the OpenMP team at `ENGINE_CHILD_NICE`. GPUI stays default. HIP kernel scheduling is still not nice.

## Honest: oomd / memory logout

Niceness does not reduce RSS or swap. It will not stop systemd-oomd. The earlier session logout was memory pressure (journal: oomd killed GNOME). This slice only cuts CPU contention between compute threads and the desktop.

## Files changed

- `crates/colibri-sys/src/process_priority.rs`
- `crates/colibri-sys/src/lib.rs`
- `crates/colibri-sys/src/engine/mod.rs`
- `crates/colibri-sys/src/ffi/mod.rs`
- `crates/colibri-sys/src/ffi/bindings.rs`
- `crates/colibri-sys/src/ffi/multi.rs`
- `crates/colibri-sys/src/ffi/v4.rs`
- `crates/colibri-native/src/host.rs`
- `c/colibri_api.h`
- `c/colibri.c`
- `.agents/RESIDUAL.md`

## Post-impl verify

```text
cargo fmt -p colibri-sys -p colibri-native
# exit 0

cargo clippy -p colibri-sys --all-targets --features ffi -- -D warnings
# exit 0

cargo clippy -p colibri-native --all-targets -- -D warnings
# exit 0
```
