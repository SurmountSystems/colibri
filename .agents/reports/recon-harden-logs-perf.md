# Recon: harden logs, reliability, and real next perf (after 2026-08-13 oomd)

Read-only inventory. No product edits.

Last logout (from prior recon, not re-diagnosed here): `native.log` went silent after `engine start end kind=ffi elapsed_ms=4805`. About 5 min 36 s later `systemd-oomd` SIGKILL’d GNOME Shell. The journal never named `colibri-native`. Correlation was clock-only. That run was CPU in-process FFI (`just install features=install,ffi`), not HIP.

Prior reports (paths only):  
`.agents/reports/recon-journal-session-crash.md`  
`.agents/reports/recon-cpu-vs-rocm-run.md`  
`.agents/reports/impl-compute-low-nice-ffi.md`  
`.agents/reports/pin-session-journal.md`  
`.agents/reports/pin-test-run-logging.md`  
Also used: `impl-native-logs-and-start-off-ui.md`, `impl-generate-pump-try-lock-setup.md`, `recon-test-run-logging.md`, `recon-native-crash-log.md`.

Residual OPEN left as listed. Do not treat `open:npu-inference`, `open:openai-rest`, `open:visual-pump-idle-stop`, `open:generate-progress-redesign`, or `open:hub-mid-file-byte-progress` as this harden work.

---

## Logging gaps that would have made the last logout about 10 minutes faster

### What exists

| Piece | Contract |
|-------|----------|
| Path | `$XDG_DATA_HOME/colibri/logs/native.log` (`/home/hunter/.local/share/colibri/logs/native.log` when `XDG_DATA_HOME` is unset) |
| Default | On. `COLIBRI_LOG=off` / `0` / `false` / `no` skips. Filter `colibri_native=info,colibri_sys=info` unless `RUST_LOG` is set |
| Dual write | tracing file + stderr. C banners (`[` …) via fd-2 tee after `install_host_stderr_tee` (before tracing init) |
| Rotate | 4 MiB, keep `native.log.1` and `.2` (`NATIVE_LOG_ROTATE_BYTES` / `NATIVE_LOG_BACKUP_COUNT`) |
| Secrets | `sanitize_log_text` redacts `hf_…`, `sk-…`, `HF_TOKEN=` / `API_KEY=` assignments. No prompts or generate tokens |
| Panic | Rust hook writes `panic:` via tracing and a flushed append. C `SIGSEGV` / `SIGKILL` / oomd write nothing |
| Start / generate | `format_engine_start_log` / `format_generate_log`: model path, `kind` (`ffi` or `process`), `elapsed_ms`, `req_id`, sanitized error |

`kind=ffi` is in-process vs process only. It is not CPU vs HIP vs CUDA. `archive_gpu_flavor` already classifies `cpu` / `HIP` / `CUDA` at **build** time and is not written to `native.log`.

### What the last file did not have

The second process was eight tracing lines and about 1 KiB. No C banner, no `generate begin`, no `panic:`, no rotate. The useful journal facts (pid, cgroup, 74.4G peak, oomd vs kernel OOM) were not in the product file.

Missing fields that would have matched `journalctl` in one pass:

1. **pid** and **comm** / argv0 on log open. Journal never printed `colibri-native`. The huge unit was `app-gnome-Alacritty-14969.scope`.
2. **cgroup** (`/proc/self/cgroup` leaf, e.g. that Alacritty scope). oomd names cgroups, not Rust targets.
3. **GPU / link flavor** on start: `cpu` vs `HIP` vs `CUDA`, plus `kind=ffi|process`. Last run needed a `readelf` / `nm` safari because the log said only `kind=ffi`.
4. **RSS / swap samples** after start and on a timer. C `emit_stream` already prints `[t=… RSS … GB]` every 16 tokens for CLI chat. FFI `coli_glm_emit_cb` does not. Rust never samples `/proc/self/status` (`VmRSS`, `VmSwap`). The 5 min 36 s gap after start is exactly this hole. SIGKILL cannot write a last line; a 5–10 s heartbeat would have shown RSS climbing before the compositor died.
5. **Structured fields.** `fmt::layer()` is human text with target. No `with_thread_ids`, no pid, no JSON, no `SYSLOG_IDENTIFIER`. Journal correlation is substring + UTC vs MDT arithmetic.
6. **sd_journal / syslog identity.** Product does not send to the journal. `native.log` is the only named artifact. After compositor death it is not a full session story (already pinned).

Rotation is fine for this incident (file never approached 4 MiB). The gap is identity + memory + flavor, not more backups.

Honest limit: oomd SIGKILL still will not flush a final line. Periodic RSS + pid/cgroup on the first line is what shrinks diagnosis. Do not add Sentry (AGENTS pin).

---

## Reliability gaps that can still kill the desktop

Landed and still true:

- Engine start is off the UI thread (`dispatch_blocking_start` / `spawn_engine_start`). Worker is niced.
- Visual pump uses `try_lock` (engine + session). GPUI no longer sits in `Mutex::lock` during generate.
- Stop sets a cooperative FFI flag without taking the engine mutex.
- Process fallback exists on FFI **open** failure and on FFI **generate** error (starts a serve child and retries once).
- Rust panic hook writes `panic:`.
- CPU FFI start/generate thread + OpenMP team are nice 10. GPUI, stderr tee, visual pump stay 0. HIP kernels are not scheduled by nice. Niceness does not reduce RSS and does not stop oomd.

What can still freeze or kill the session:

1. **In-process RSS is the host RSS.** Default native is FFI-first. A 400G GLM tree in the same pid as GPUI. Isolation docs already say an in-process fault kills the host. The 2026-08-13 death was memory pressure, then oomd killing **GNOME Shell** (`session.slice`), not the app cgroup. Process fallback does not set `MemoryMax` and does not move the child out of the terminal scope. A niced serve child can still fill RAM and trip the same oomd policy.
2. **FFI start does not apply `PlacementPlan`.** Process `start_process` builds a plan and `EngineHandle::start_with_plan` → `serve_env_with_plan` (`RAM_GB`, `OMP_NUM_THREADS` = physical cores, policy). FFI `EngineSession::start` calls `coli_ffi::open_engine` with `GlmOpenOptions::default()` (`cap=0` → C `cap=64`) and never sets plan env. `cap_for_ram` / `CAP_RAISE` live in C **CLI `main`**, not in `coli_glm_engine_open`. So the UMA planner the doctor shows is **not** the budget the default embed runs with.
3. **FFI cancel does not stop C decode.** Rust `on_token` can return `Err` and `coli_glm_emit_cb` sets `e->stop`. `spec_decode`’s loop only checks `g_intr`, `g_mux_stop`, `g_mux_cancel`. Those mux flags are serve-only. FFI `step()` prefill has no cancel at all. Stop can unstick the UI (try_lock) while OpenMP keeps burning until `n_new` or the machine swaps.
4. **C abort / SIGSEGV / SIGKILL.** No `sigaction`. Panic hook does not run. Last flushed tracing / `[` banner may remain. `model_init` can still `abort` on fatal load (comment on `coli_glm_engine_open`).
5. **Generate holds the FFI mutex for tokenize + full prefill + `spec_decode`.** UI stays live (try_lock). Brain/PROF freeze on the last snapshot. Not a compositor freeze anymore. Still a long uninterruptible compute burst in the GUI pid.
6. **`COLIBRI_FORCE_PROCESS` is crash isolation, not an oomd fence.** Child is niced. Same machine RAM. No product cgroup memory cap.

`open:visual-pump-idle-stop` is a Join-on-drop polish. Pump already exits when the session slot is cleared and uses try_lock. It did not cause this logout. Do not rank it here.

---

## Performance work that is real (code-backed) vs theater

### Already landed (do not re-sell)

- Thread + OpenMP niceness 10 on FFI start/generate (`open:ffi-inprocess-priority` closed).
- UMA inventory + planner (doctor / Memory plan / process env). Conservative unified RAM share.
- Default Cargo `ffi` is **CPU-only**. `ffi-hip` / `ffi-cuda` opt-in, one vendor. `just install` is `install,ffi`.
- Cold overflow copy: intended disk experts are a **note**, not a scare warning (`impl-cold-expert-miss.md`).
- Visual pump try_lock; start off UI.

### Real next work (in the tree)

| Item | Evidence | Why it is not a slogan |
|------|----------|------------------------|
| Apply the placement plan to FFI open | Plan env only on process `start_with_plan`. FFI open ignores `RAM_GB`, physical-core `OMP_NUM_THREADS`, policy | Last run’s doctor/UMA numbers did not bind the embed. Process path would have set a RAM budget. |
| Fail-loud when CPU FFI + huge leaf | `kind` does not record flavor. Host has ROCm; binary did not link it | Loading ~400G through CPU FFI into ~90 GiB RAM is the measured memory picture. HIP as **silent new default** is a product-policy flip (`just install` stays CPU). Honest start: refuse, warn, or require `ffi-hip` / `FORCE_PROCESS` + plan. |
| HIP as an **opt-in product install**, not a comment | `just install features=install,ffi-hip`; residual already says live HIP generate is operator-gated | Real for this APU if the operator wants GPU/UMA kernels. Not a one-line default flip. HIP still shares system RAM on UMA; planner + `RAM_GB` still required. Nice does not schedule HIP. |
| FFI OpenMP team size | FFI `setenv("COLI_NO_OMP_TUNE","1")` in `coli_glm_engine_open`. Plan’s `OMP_NUM_THREADS` never applied | Default libgomp can be SMT-wide. Plan already wants physical cores for memory-bound i4. |
| Expert cache vs 429G | FFI `cap=64` and no `cap_for_ram`. CLI always clamps (“senza clamp la LRU cresce… OOM-kill”) | First generate/prefill can fill 64 slots × MoE layers in the GUI pid. That is the desktop-killer path if they send a prompt. |
| `COLI_MMAP` + populate | Default `COLI_MMAP=0`. Weight mmap is `MAP_SHARED` **without** `MAP_POPULATE`. `MAP_POPULATE` is only on io_uring rings | Prefault would make start elapsed_ms and RSS honest instead of silent later faults. Only real if mmap is actually turned on. |
| Wire cancel into `spec_decode` + prefill | `e->stop` unused by the while loop; `g_mux_*` is serve-only | Stop that does not stop compute still burns RAM/CPU until max tokens. |
| KV slots on FFI | FFI session hard-codes `kv_slots: 1`. Env `COLIBRI_KV_SLOTS` is process/mux | Real ABI gap. Not what killed the session. Do not lead with it. |
| Speculative decode as a **speed** project | MTP/`spec_decode` already in C; FFI already calls it | Cancel wiring is the harden slice. Draft-depth tuning is not the oomd lesson. |

### Theater (do not queue as this harden)

- NPU inference, local OpenAI REST, generate % redesign, hub mid-file bytes.
- “HIP by default” without plan apply, flavor log, and an install that actually links `libamdhip64`.
- More log rotation, Sentry, or a second crash reporter.
- Niceness 11 vs 10. Nice did not stop oomd.

---

## Recommended ordered product slices

1. **Session identity + memory heartbeat in `native.log`.** On init and every few seconds while an engine is up: pid, comm, cgroup leaf, RSS, swap, `kind`, archive/link flavor (`cpu`/`HIP`/`CUDA`). Keep redact + no prompts. This is the 10-minute diagnosis slice. Contract tests: line contains pid and flavor; heartbeat test can fake `/proc` or inject bytes.

2. **Bind the UMA / RAM plan to FFI open (or refuse).** Same `environment_for_plan` keys the process child already gets: `RAM_GB`, `OMP_NUM_THREADS`, policy, and a real cap / `CAP_RAISE=0` when the model is larger than the unified budget. If FFI cannot honor the plan, do not start CPU FFI on a 400G leaf. Show the existing operational copy. Doctor notes are not enough.

3. **Hard memory fence for the desktop.** Either default large-leaf to **process** with a documented cgroup/`MemoryMax` (or a serve-child limit), or keep FFI but fail before RSS can starve `session.slice`. Process-without-a-cap is not a fence. Isolation docs should say crash isolation ≠ oomd isolation.

4. **FFI cancel that C obeys.** `spec_decode` must honor embed stop (`e->stop` or a dedicated flag). Prefill `step` needs checkpoints. Token-callback-only cancel is already documented as incomplete.

5. **HIP install as an explicit operator path**, after 2. Rebuild with `ffi-hip` on this ROCm host, log flavor=HIP, still apply the UMA `RAM_GB` clamp. Do not change default `just install` in the same slice unless the operator asks.

Skip for this harden: KV multi-slot on FFI, mmap populate unless `COLI_MMAP` is actually enabled, residual OPEN rows above, niceness retune.

---

## Critical files

| Path | Why |
|------|-----|
| `crates/colibri-native/src/log_init.rs` | Rotate, panic hook, tracing layers (no pid / JSON) |
| `crates/colibri-native/src/stderr_tee.rs` | C `[` banners + prefill parse only |
| `crates/colibri-native/src/main.rs` | Tee then log init; `app_id`; visual pump |
| `crates/colibri-sys/src/native_log.rs` | Enable/filter/sanitize; start/generate formatters |
| `crates/colibri-sys/src/paths.rs` | `default_native_log_path` |
| `crates/colibri-sys/src/archive_gpu_flavor.rs` | `cpu` / HIP / CUDA (build, not logged) |
| `crates/colibri-native/src/host.rs` | Start off-UI, FFI-first, no plan on FFI, try_lock pump, generate fallback, cancel flag, FFI `kv_slots: 1` |
| `crates/colibri-sys/src/ffi/multi.rs` | `open_engine` default cap; generate trampoline |
| `crates/colibri-sys/src/plan.rs` | `environment_for_plan` (process only today) |
| `crates/colibri-sys/src/process_priority.rs` | Nice 10 thread + child |
| `c/colibri.c` | `coli_glm_engine_open` (no `cap_for_ram`); `coli_glm_generate` / `step` / `spec_decode`; `coli_glm_emit_cb`; CLI `cap_for_ram`; mmap without populate |
| `c/omp_tune.h` | Physical-core OpenMP (skipped when `COLI_NO_OMP_TUNE=1`) |
| `.agents/RESIDUAL.md` | Isolation + CPU-only default + closed nice; OPEN list unchanged |

Do not implement from this report.
