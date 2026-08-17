# Recon: generate still freezes the GPUI event loop

**Date:** 2026-08-13

Engine start is off the UI thread. Generate still makes GNOME say `"org.colibri.native" Is Not Responding`. Cause is not `coli_glm_generate` on the UI thread. Cause is the **visual pump on the UI thread blocking on the same `Mutex` the generate worker holds** for the whole FFI call (tokenize + full prefill + decode).

Start-off-UI worked: log shows `engine start begin` then `engine start end … kind=ffi elapsed_ms=7400`. No generate log lines after that (generate does not log).

---

## 1. Chat send / generate path

FFI generate is **not** on the UI thread.

| Step | Thread | File:line |
|------|--------|-----------|
| `send_chat` sets `generating`, empty assistant bubble, `Generating... 0%` | UI | `crates/colibri-native/src/main.rs:1184-1271` |
| `EngineSession::generate_async` → `thread::spawn` | UI returns immediately | `host.rs:2063-2069`, call at `main.rs:1268` |
| `generate_ffi` → `FfiEngine::generate` → `GlmEngine::generate` → `coli_glm_generate` | **worker** | `host.rs:2203-2213`, `2344-2418`; `ffi/multi.rs:126-135`, `314-375` |
| First token only after C **prefill** `step(...)` | **worker**, still under mutex | `c/colibri.c:9720-9722` |
| Visual poll every 500ms via `this.update` → `apply_visual_snapshot` | **UI** | `main.rs:1293-1339` |

`send_chat` then starts `ensure_visual_pump` + `schedule_gen_poll` (`main.rs:1269-1270`). Pump is already running after Ready (`apply_started_session` at `main.rs:1126-1127`).

Prefill is **not** on the UI thread. The UI freezes because the next pump **waits** for that worker.

---

## 2. Lock ordering (this is the hang)

Documented in product comments:

- `host.rs:1788-1792` — FFI generate holds the engine mutex for the call; visual pump also takes that mutex.
- `host.rs:2014-2016` — `pump_visual` **blocks if a generate currently holds the engine mutex**.

Sequence:

1. Worker: `ffi_engine.lock()` then `generate_ffi` for the **entire** `coli_glm_generate` (`host.rs:2203-2213`). Not released between prefill and tokens.
2. UI (already looping): `apply_visual_snapshot` → `pump_session_visual` (`main.rs:1337-1339`, `host.rs:2435-2438`).
3. `pump_session_visual` holds the **session** mutex, then `EngineSession::pump_visual` does `engine.lock()` (`host.rs:2023-2028`). **`std::sync::Mutex::lock`, not `try_lock`.**

Not a classic ABBA deadlock (worker does not take the session lock during generate). It is a **UI-thread stall**: the GPUI event loop sits in `Mutex::lock` until generate finishes. GNOME "Not Responding" is that stall. Same look as the old start-on-UI freeze.

`pump_session_visual` also holds the session mutex for the whole wait (`host.rs:2436-2438`). `stop_session` needs that same session lock (`host.rs:2442-2447`). While the pump is stuck, nothing else on the UI thread can run.

C `coli_glm_visual_poll` (`c/colibri.c:9767`) is not safe to call concurrently with generate (same `Model *`). The Rust mutex is the only serializer. Poll must use **`try_lock` + last cached snapshot**, and must not run on the UI thread if it ever waits.

---

## 3. Why 0% sticks

Send paints 0% immediately:

- `main.rs:1239` — `progress_view_for_generate(0, controls.max_tokens, 0.0)`
- `host.rs:3030-3036` → `progress.rs:195-208` — `generated == 0` and `max_output > 0` is **honest 0%**. Denominator is max **output** tokens, not prefill.

First progress bump is `drain_gen` on a Token (`main.rs:1371-1421`). GLM FFI only emits after prefill:

```c
float *logit = step(m, pids, np, 0);           /* full prompt, no on_token */
spec_decode(..., coli_glm_emit_cb, ...);       /* first new token here */
```

`c/colibri.c:9720-9722`. Screenshot Disk 19456 / GPU 0 / RAM 0: every expert is on disk, so `step` can run a long time. No Token → 0% is expected until first decode token.

It **stays** 0% even after tokens exist because `schedule_gen_poll` also uses `this.update` (`main.rs:1371-1378`). That update is queued behind the stuck visual pump, so the channel is never drained and the window never paints.

`generate_ffi` only sends `"·"` on token 1 and every 8th (`host.rs:2412-2415`). Empty assistant bubble matches: no drain, no paint.

---

## 4. Stop during the hang

**No. Stop does not work while the window is Not Responding.**

- Paint is solid red because `generating == true` (`main.rs:1236`, `stop_button_usable_is_solid_danger` at `main.rs:5245`). That is color only.
- Click is `stop_generate` on the UI thread (`main.rs:1274-1290`). Event loop is blocked in visual `engine.lock()`. The click is not handled.
- If Stop could run, FFI only does `cancel.store(true)` and does **not** take the engine mutex (`host.rs:2051-2053`). Good. Useless during prefill: Rust checks cancel only in the token callback (`host.rs:2406-2408`), which runs only after `step`.
- C `coli_glm_emit_cb` sets `e->stop` (`c/colibri.c:9635-9641`) but `spec_decode` loops on `g_intr` / `g_mux_*` only (`c/colibri.c:6125`). It never reads `e->stop`. Cooperative cancel cannot abort in-process GLM generate.

---

## 5. native.log (product events only)

Path: `/home/hunter/.local/share/colibri/logs/native.log` (exists). Entire last-run body:

```
native log file path=.../colibri/logs/native.log
start engine clicked source="rail" model=.../GLM-5.2-colibri-int4-g64-with-int8-mtp
engine start begin model=.../GLM-5.2-colibri-int4-g64-with-int8-mtp
engine start end model=.../GLM-5.2-colibri-int4-g64-with-int8-mtp kind=ffi elapsed_ms=7400
```

No generate begin/end, no token, no stop. Matches a silent worker still inside `coli_glm_generate` while the UI is wedged. Prompt text is not in the log.

---

## 6. Named TDD contract

**Behavior:** After Send, the window must keep pumping. 0% until first token is allowed. GNOME must not say Not Responding. Visual poll must not take the generate lock on the UI thread.

Suggested tests (native + host, no 429 GB model):

1. `ffi_visual_pump_must_not_block_while_generate_holds_engine_mutex` — with a dummy `Mutex` held, `EngineSession::pump_visual` / `pump_session_visual` must `try_lock` and return the last snapshot (or default), never `lock()`.
2. `apply_visual_snapshot_path_does_not_block_on_engine_mutex` — UI helper used by the 500ms pump never calls blocking `engine.lock()`.
3. `generate_progress_zero_tokens_is_zero_percent` — already implied by `generate_progress` (`progress.rs:195-208`); keep: 0 generated / N max → `Some(0)`, not a fake floor.
4. `stop_active_ffi_does_not_take_engine_mutex` — `stop_active` sets cancel while another thread holds the engine mutex (timeout if it waits).
5. Optional: `glm_ffi_prefill_does_not_invoke_token_callback` — documents that cancel cannot fire until after `step`.

Implementer fix (not this recon): pump visual on a **background** thread with `try_lock`; apply a cloned snapshot on the UI thread; never `lock()` the FFI engine from `this.update`.
