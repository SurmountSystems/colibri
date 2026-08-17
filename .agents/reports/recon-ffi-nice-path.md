# Recon: compute niceness for `colibri-native`

**Date:** 2026-08-13  
**Scope:** how priority works today. No product edits.

Residual `open:ffi-inprocess-priority` is still accurate: process-mode children can be niced; in-process FFI cannot take a process-wide `setpriority` without hurting GPUI. Thread-priority demotion is not implemented.

## What exists

All niceness lives in `crates/colibri-sys/src/process_priority.rs`.

| Symbol | Role |
|--------|------|
| `ENGINE_CHILD_NICE = 10` | Unix nice for heavy **child processes** (default is 0; 1..=19 is lower priority) |
| `apply_low_compute_priority(&mut Command)` | Child-only. Unix: `pre_exec` + `setpriority(PRIO_PROCESS, 0, 10)`. Windows: below-normal class. |
| `set_current_process_nice` | Private. Used only from that `pre_exec`. |

There is no `PRIO_PTHREAD`, no `libc::nice`, and no thread helper. Grep of `crates/` for `setpriority` / `nice` hits only this module.

**Call sites of `apply_low_compute_priority`:**

1. `ServeClient::spawn` (`engine/serve.rs`) — process-mode engine.
2. `hf download` child (`model/install.rs`).
3. Python convert last-resort child (`model/install.rs`).

Not applied: `colibri-native` itself, FFI generate, engine-start worker, visual pump, stderr tee.

Prior report: `.agents/reports/impl-compute-low-nice.md`.

---

## 1. Process-mode engine child: is it niced?

**Yes. Unix nice 10.**

`ServeClient::spawn` calls `apply_low_compute_priority` after wiring env. The child does `setpriority` after fork and before exec. `exec` keeps that nice. Later OpenMP / C threads in that child inherit it.

Integration test `spawned_child_has_elevated_nice` asserts `getpriority(child_pid) == 10`.

The host also forces `COLI_NO_OMP_TUNE=1` on that child so GLM does not re-exec for hot-thread spin. That is wait-policy / thread-count, not niceness.

Windows would be below-normal. This host is Linux.

---

## 2. FFI in-process: lower priority, or only off the UI thread?

**Only off the UI thread. Same nice as GPUI (usually 0).**

`EngineSession::generate_async` does `std::thread::spawn` with no name and no `setpriority`. That worker then:

- Process route: talks to the already-niced child over the mux.
- FFI route: takes the engine mutex and calls `generate_ffi` → `FfiEngine::generate` → `coli_glm_generate` (or Kimi / Inkling / V4) **on that same worker**. C `step` / `spec_decode` run there. Token callbacks run there.

`coli_glm_engine_open` does `setenv("COLI_NO_OMP_TUNE", "1", 0)` so FFI never hits the standalone `main()` re-exec. GLM still uses OpenMP in the matmuls (`#pragma omp` in `c/quant.h` and friends). Those workers are created by libgomp inside **this** process at default nice.

Other host threads, also default nice:

| Thread | Job | Nice? |
|--------|-----|-------|
| GPUI UI thread | Window, input, paint | default (must stay) |
| `generate_async` worker | Blocking FFI generate or mux client | default |
| `dispatch_blocking_start` / `spawn_engine_start` | mmap / FFI open / drop previous | default |
| Visual pump | GPUI `cx.spawn` ~500 ms timer; `apply_visual_snapshot` on the UI thread via `try_lock` | not a compute thread |
| `colibri-stderr-tee` | Drain fd 2 | IO only |
| Install worker | Hub download orchestration; **children** are niced | worker itself default |

So FFI compute is not on the GPUI thread, and it is not demoted.

---

## 3. Would nicing the whole `colibri-native` process also nice GPUI?

**Yes. Do not do that.**

`apply_low_compute_priority` is written to avoid `setpriority` on the caller for this reason. Test `apply_does_not_demote_parent_process` locks it.

On Linux, nice is per-thread, not POSIX process-wide. See [setpriority(2)](https://man7.org/linux/man-pages/man2/setpriority.2.html) (accessed: 2026-08-13). `setpriority(PRIO_PROCESS, pid, 10)` on the process id typically hits the **main thread** (GPUI), which is the worst single thread to demote. There is no `PRIO_PTHREAD` on Linux; `who` is a tid.

Nicing every thread in `/proc/self/task` would still demote GPUI. Same failure.

---

## 4. Smallest product path (Linux): nice only FFI compute

Reuse `ENGINE_CHILD_NICE = 10`. Do not change the UI process.

**Step A (necessary, small, TDD-able).** Promote the existing private `set_current_process_nice` to a current-**thread** helper, for example `set_current_thread_nice(nice: i32)`, still `setpriority(PRIO_PROCESS, 0, nice)` called **from the worker**. Call it at the top of the `generate_async` closure (and, if open/prefill is heavy, at the top of `dispatch_blocking_start`).

Named tests in `process_priority.rs` (same style as today):

- `set_current_thread_nice_does_not_change_other_thread` — spawn a thread, set nice 10 there, `getpriority(0)` on that thread is 10; caller `getpriority(0)` unchanged.
- Keep `apply_does_not_demote_parent_process`.

**Step B (required if the stall is CPU).** Step A alone is not enough. GLM work is mostly the OpenMP team, which libgomp created earlier (often during `model_init` on the start worker) at nice 0. Those tasks keep nice 0 even if the Rust worker is later niced.

Smallest complete CPU path: after Step A, run one OpenMP parallel region that calls the same helper on each team member (a tiny C hook next to `coli_glm_generate`, or a Rust `omp parallel` is the wrong crate). That nices the persistent pool without touching the GPUI thread.

Do **not** nice the stderr tee or the visual pump. Do not `renice` the whole pid.

HIP/ROCm kernels are not scheduled by nice. Host-side prefill / submit threads still are. GPU runtime threads created at open (nice 0) will not follow Step A unless they run that OpenMP body or you walk their tids on purpose. Do not invent a broader GPU scheduler story.

---

## 5. Memory vs CPU (this crash)

Journal for the 20:50:17Z logout: `systemd-oomd` SIGKILL of GNOME Shell after `session.slice` memory pressure. Alacritty scope (the terminal that held `colibri-native`) peaked about **74G RAM and 106G swap** on a host with about 90 GiB RAM.

**Niceness does not reduce RSS or swap. It will not stop oomd.** A niced process can still fill RAM and get the session killed.

Niceness can still cut **CPU** contention: if gnome-shell lagged because `colibri-native` plus OpenMP sat on every core at nice 0, demoting only those compute threads can give the compositor time. The journal also had `your system is too slow` next to reclaim. If the stall was swap/reclaim, the desktop stays laggy at any nice.

Honest split:

- Logged out: memory / oomd. Nice is irrelevant.
- Felt laggy / “not sufficiently nice”: possible CPU fight **and/or** reclaim. Nice only addresses the CPU fight. FFI today is default nice 0 in the UI process, so the CPU side is real product debt (`open:ffi-inprocess-priority`). It is not a substitute for staying inside RAM.
