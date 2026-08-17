# Recon: `[prefill]` progress without the engine mutex

Read-only. No product edits. Operator paste is GLM FFI: engine start ~7400 ms, then `[stop]` and `[prefill]` on stderr, then GNOME Force Quit (`SIGKILL`). No `generate begin`/`generate end` in that paste (older binary or generate-log slice not in that tree).

Goal: append those C lines to `~/.local/share/colibri/logs/native.log` and show live status such as `Prefill layer 13/78 · 47 tokens` while generate holds the FFI mutex for the whole `coli_glm_generate`.

---

## 1. Exact C print sites

### `[prefill] layer N/M`

| | |
|--|--|
| File | [`c/colibri.c:5628-5632`](../../c/colibri.c) |
| Function | `layers_forward_rows` (called by `layers_forward` at [`c/colibri.c:5686-5687`](../../c/colibri.c)) |
| Call chain (FFI GLM) | `coli_glm_generate` → `step` ([`c/colibri.c:9720`](../../c/colibri.c)) → `layers_forward` → `layers_forward_rows` |

Print:

```c
if(S>=8 && (i%4==0 || i==c->n_layers-1))
    fprintf(stderr,"[prefill] layer %d/%d · %d token · +%.2fs\n",
            i+1, c->n_layers, S, now_s()-tl0);
```

**Frequency:** yes, every 4 layers, plus the last layer. Loop index `i` is 0-based; the printed layer is `i+1`. With 78 layers that is layers **1, 5, 9, 13, …, 77, 78**. Matches the paste (`1/78`, `5/78`, `9/78`, `13/78`).

**Gates:**

- `S >= 8`. `S` is the current forward batch (prompt length, or a `COLI_PREFILL_CHUNK` slice). A prompt shorter than 8 tokens prints **nothing**. The paste `47 token` is `S`, not a layer count.
- `S` is **this** forward, not a running total. Default `COLI_PREFILL_CHUNK` is 0 ([`c/colibri.c:5761-5762`](../../c/colibri.c)), so one shot prints the same `S` on every line. If chunking is on and the chunk is `>= 8`, each chunk reprints with `S == chunk`.
- Same `if` also fires for any other `layers_forward` with `S>=8` (MTP / batched verify). Decode `S=1` does not print.

**Clock:** `tl0` is set once before the layer loop ([`c/colibri.c:5627`](../../c/colibri.c)). `+%.2fs` is seconds since the start of this forward, not since Send.

**Family:** GLM only (`colibri.c`). Kimi uses a different banner, `\r[K3] prefill %d/%d` ([`c/kimi_k3.c:1881`](../../c/kimi_k3.c)). Inkling prints a summary after the fact ([`c/inkling.c:1695`](../../c/inkling.c)). This paste is GLM (78 layers).

### `[stop] N stop tokens:`

| | |
|--|--|
| File | [`c/sample.h:155-158`](../../c/sample.h) |
| Function | `stops_arm_tok` |
| FFI GLM | `coli_glm_generate` calls it once after tokenize setup ([`c/colibri.c:9684`](../../c/colibri.c)), **before** `step` / prefill |

Print (once per arm, not periodic):

```c
fprintf(stderr, "[stop] %d stop tokens:", g_nstop);
for (int i = 0; i < g_nstop; i++) fprintf(stderr, " %d", g_stop[i]);
if (nsp) fprintf(stderr, " (%d from the tokenizer's special set)", nsp);
fprintf(stderr, "\n");
```

That is why the paste is `[stop] 18 stop tokens: …` then `[prefill]`. The 18 numbers are tokenizer IDs, not user text.

A second `[stop]` line exists only in batched serve mode ([`c/sample.h:148-152`](../../c/sample.h)): `[stop] batched serve mode: filtered N …`. Native FFI does not set `SERVE`/`SERVE_BATCH`, so that line is off on the host path that printed this paste.

Other `stops_arm_tok` call sites (CLI generate / mux): [`c/colibri.c:6656`](../../c/colibri.c), [`7296`](../../c/colibri.c), [`7475`](../../c/colibri.c).

---

## 2. Is there already a C callback, telemetry field, visual poll, or atomic?

**No host-readable prefill progress API.** Nothing the UI can poll mid-`step` without either parsing stderr or adding new C.

| Candidate | Why it does not help mid-prefill |
|-----------|----------------------------------|
| `ColiTokenFn` ([`c/colibri_api.h:24-26`](../../c/colibri_api.h)) | First call is after `step` returns, inside `spec_decode` ([`c/colibri.c:9720-9722`](../../c/colibri.c)). Prefill does not invoke it. |
| `coli_glm_visual_poll` ([`c/colibri.c:9767`](../../c/colibri.c), flags in [`c/colibri_api.h:102-107`](../../c/colibri_api.h)) | HWINFO / TIERS / EMAP / HITS / PROF only. PROF is written **after** generate (`coli_glm_record_prof`). No layer/token prefill fields. Documented **not safe** on the same `Model*` as generate ([`crates/colibri-native/src/host.rs:2473-2475`](../../crates/colibri-native/src/host.rs)). |
| `VisualSnapshot` ([`crates/colibri-sys/src/visual.rs`](../../crates/colibri-sys/src/visual.rs)) | Same five kinds. No prefill. |
| Mux `ACCEPT` ([`c/colibri.c:7205`](../../c/colibri.c)) | Process stdout, pre-prefill commit (`id`, prompt token count). Not a layer ticker. FFI has no mux. |
| `_Atomic int g_cur_moe_layer` ([`c/colibri.c:1154`](../../c/colibri.c), store at [`3812`](../../c/colibri.c)) | **file-static**, not exported. Only updated when `g_pilot_real` is on (default **off**, `PILOT_REAL=1`). Reset to `-1` at the start of each `layers_forward_rows` ([`5605`](../../c/colibri.c)). Pilot handshake, not a progress ABI. |
| HITS / EMAP during generate | Would need `coli_glm_visual_poll` on the same `Model*` the worker is mutating. Rust `try_lock` correctly refuses that. |

Prior art for **parsing the banner** (not a C ABI): the Python `coli` chat spinner tails an errlog and takes the last `[prefill]` line ([`c/coli:1099-1105`](../../c/coli)). Format rewrite: `"[prefill] "` → `"prefill "`. That is the existing product copy pattern, not a native wire.

---

## 3. FFI vs process: stderr inherit and native.log

### FFI (this paste)

`coli_glm_generate` runs **in the host process**. `fprintf(stderr, …)` goes to the host's stderr fd.

`colibri-native` logging ([`crates/colibri-native/src/log_init.rs:157-208`](../../crates/colibri-native/src/log_init.rs)):

- `tracing-subscriber` writes **its own** events to stderr **and** `$XDG_DATA_HOME/colibri/logs/native.log`.
- It does **not** intercept libc `fprintf`.
- C `[prefill]` / `[stop]` therefore appear on a terminal if one is attached, and **never** enter `native.log` today.

That matches a last-run file that only has `engine start begin/end` (and, after the try_lock slice, `generate begin`/`end` from Rust). The C banners are missing from the file even when they printed on the TTY.

GNOME `.desktop` launch with no TTY: those `fprintf` lines go nowhere unless something tees the fd.

### Process

[`crates/colibri-sys/src/engine/serve.rs:184-186`](../../crates/colibri-sys/src/engine/serve.rs):

```rust
.stdin(Stdio::piped())
.stdout(Stdio::piped())
.stderr(Stdio::inherit());
```

Child stderr inherits the **host** stderr. Same TTY-or-nothing problem. Native does not capture it into `native.log`.

If the host first installs a stderr tee (section 4), inherit automatically follows the tee. No `serve.rs` change required for the process path.

### Python CLI (contrast)

[`c/coli:1010-1043`](../../c/coli) uses `stderr=PIPE` plus a drain into a tempfile, then `prefill_tick` reads that file. Native has no equivalent. The Python drain is `read()` until EOF (not line-at-a-time); treat it as format prior art, not a drain to copy.

### Stdio buffering

Each `[prefill]` line ends in `\n`. glibc `stderr` is unbuffered by default, so a TTY and a pipe both see lines as they are printed. No `setvbuf` at this site. A tee must drain the pipe or a later burst of banners can fill the OS pipe and stall C.

---

## 4. Smallest product path (do not implement here)

Keep generate on the worker. Keep `pump_visual_try_lock` (never `engine.lock()` on the UI thread). Do **not** add a mid-prefill `coli_glm_visual_poll` field this slice: that would require a new C ABI **and** still cannot run while generate holds the `Model*`.

### A. Capture C lines into `native.log`

1. Before `init_native_logging` / `Application::new`, install a **host stderr tee**:
   - `pipe` + save the real stderr fd + `dup2` the write end onto fd 2.
   - Background thread: read **lines**, write each line to the saved tty fd (keep terminal paste), and append sanitized text to `native.log` via existing `append_native_log_line` / `RotatingFile` ([`log_init.rs:110-118`](../../crates/colibri-native/src/log_init.rs)).
2. **File double-write pitfall:** tracing already has a file layer. If the tee also appends every byte on stderr, Rust `tracing` lines appear twice. Smallest: tee-to-file only lines that look like C banners (`[` …), **or** drop the tracing file layer and let the tee own the file. Prefer banner-only file append so today's `generate begin` format stays one copy.
3. Process child `Stdio::inherit()` then hits the same tee. FFI `fprintf` hits it with no C change.
4. Drain line-by-line (not `read()` until EOF). Do not take the engine mutex on this thread.
5. Reuse `sanitize_log_text`. Do not log prompt text. `[stop]` IDs are numeric; fine in the file, do not put them on the status chip.

### B. Live status without the engine mutex

Same tee thread, on each `[prefill]` line:

1. Parse into `{layer, total, tokens, elapsed_s}` (section 5).
2. Store in `AtomicU32` / `AtomicU64` (or a `Mutex` that is **not** the FFI engine mutex).
3. Existing UI timers already run without waiting on generate:
   - `schedule_gen_poll` 40 ms ([`main.rs:1372-1386`](../../crates/colibri-native/src/main.rs))
   - visual pump 500 ms + `try_lock` ([`main.rs:1293-1320`](../../crates/colibri-native/src/main.rs), [`host.rs:2476-2499`](../../crates/colibri-native/src/host.rs))
4. While `generating && live_token_count == 0`, set `self.status` from the snapshot. Rail already paints `brand.native · {status}` ([`main.rs:3188-3192`](../../crates/colibri-native/src/main.rs)).
5. **Copy (operational, not brand):** `Prefill layer 13/78 · 47 tokens`. C prints singular `token`; UI should say `tokens`. Optional elapsed is in the log line; not required on the chip.
6. Keep generate **0%** honesty: denominator is max **output** tokens ([`progress.rs:195-208`](../../crates/colibri-native/src/progress.rs)). First decode token still bumps the bar. Prefill is a status phrase, not a fake percent of 78 layers on that bar.

GNOME Wait becomes optional because the event loop already pumps (try_lock slice) **and** the chip is no longer stuck on `Generating... 0%` with no other signal.

### What not to do this slice

- Call `engine.lock()` or `coli_glm_visual_poll` from the UI or tee thread during generate.
- Export `g_cur_moe_layer` (pilot-only, wrong default).
- Change `ColiTokenFn` to fire per layer (ABI + would still be on the generate worker; UI would need a channel anyway).
- Invent mux STOP / cancel during `step` (separate residual; prefill still cannot abort today).

C atomics next to the `fprintf` would be a later hardening if stderr is lost. Not needed if the tee works.

---

## 5. Named TDD contract (no 429 GB model)

Pure string + status helpers. No `coli_glm_open`, no leaf model.

**Parser** (sys or native, no GPUI):

| Input | Result |
|-------|--------|
| `[prefill] layer 13/78 · 47 token · +21.80s` | `layer=13`, `total=78`, `tokens=47`, `elapsed_s=21.80` |
| same with `tokens` (plural) | same numbers |
| `[prefill] layer 1/78 · 47 token · +0.00s` | first tick |
| `[stop] 18 stop tokens: 1 2 …` | `None` (not a prefill tick) |
| `"Generating... 0%"` / empty / garbage | `None` |

**Formatter:**

```text
format_prefill_status(13, 78, 47) == "Prefill layer 13/78 · 47 tokens"
```

**UI / host (mutex):**

- Applying prefill status reads only the atomic / channel snapshot.
- Must **not** call `engine.lock()` or `coli_glm_visual_poll`.
- Existing `pump_visual_try_lock_returns_last_snapshot_when_mutex_held` stays green.
- `generate_progress_zero_tokens_is_zero_percent` stays: 0 generated / N max → `Some(0)`. Prefill copy does not rewrite that percent.

Suggested test names:

1. `parse_prefill_line_extracts_layer_total_tokens`
2. `parse_prefill_line_rejects_stop_banner`
3. `format_prefill_status_is_plain_operational_english`
4. `apply_prefill_status_does_not_take_engine_mutex` (same shape as the visual try_lock test: hold a dummy mutex, apply must return immediately)

Suggested filter (after implement):

```text
cargo test -p colibri-native --bin colibri-native -- \
  parse_prefill_line \
  format_prefill_status \
  apply_prefill_status_does_not_take_engine_mutex \
  pump_visual_try_lock_returns_last_snapshot_when_mutex_held \
  generate_progress_zero_tokens_is_zero_percent
```

---

## File:line index

| What | Where |
|------|--------|
| `[prefill]` fprintf, every 4 layers + last, `S>=8` | [`c/colibri.c:5628-5632`](../../c/colibri.c) |
| `layers_forward` wrapper | [`c/colibri.c:5686-5687`](../../c/colibri.c) |
| `step` → `layers_forward` (FFI prefill) | [`c/colibri.c:5749-5776`](../../c/colibri.c), generate at [`9720`](../../c/colibri.c) |
| `[stop] N stop tokens:` once per arm | [`c/sample.h:155-158`](../../c/sample.h) |
| FFI arms stops then prefills | [`c/colibri.c:9684`](../../c/colibri.c), [`9720`](../../c/colibri.c) |
| Token callback type (post-prefill only) | [`c/colibri_api.h:24-26`](../../c/colibri_api.h) |
| Visual poll flags (no prefill) | [`c/colibri_api.h:74-107`](../../c/colibri_api.h) |
| `g_cur_moe_layer` (not exported) | [`c/colibri.c:1154`](../../c/colibri.c) |
| Process stderr inherit | [`crates/colibri-sys/src/engine/serve.rs:186`](../../crates/colibri-sys/src/engine/serve.rs) |
| native.log = tracing only | [`crates/colibri-native/src/log_init.rs:157-208`](../../crates/colibri-native/src/log_init.rs) |
| FFI generate holds engine mutex | [`crates/colibri-native/src/host.rs:2235-2246`](../../crates/colibri-native/src/host.rs) |
| Visual pump `try_lock` | [`crates/colibri-native/src/host.rs:2470-2499`](../../crates/colibri-native/src/host.rs) |
| Send paints 0% + status | [`crates/colibri-native/src/main.rs:1236-1245`](../../crates/colibri-native/src/main.rs) |
| Status chip | [`crates/colibri-native/src/main.rs:3188-3192`](../../crates/colibri-native/src/main.rs) |
| Python `[prefill]` tail (prior art) | [`c/coli:1099-1105`](../../c/coli) |
