# Recon: native / sys / C logging (and GNOME "Unknown")

Read-only inventory. No product edits. Operator: wizard Step 6 Ready, then GNOME `"Unknown" Is Not Responding`.

## 1. Does colibri-native or colibri-sys already log?

**No real logging stack. No log file. Default is off except a few stderr lines.**

| Mechanism | Present? | Evidence |
|-----------|----------|----------|
| `tracing` crate | Yes, **sys only** | `crates/colibri-sys/Cargo.toml:48` (`tracing = "0.1"`) |
| `tracing-subscriber` / `env_logger` / `log` crate | **No** (no init anywhere in workspace) | `rg tracing_subscriber\|env_logger` empty in `*.rs` / `*.toml` |
| `RUST_LOG` / `COLIBRI_LOG` | **Not read** | No matches in crates |
| File logs (XDG / journal) | **None in product** | Paths crate is model store only (`crates/colibri-sys/src/paths.rs:13-14`, `142-165`) |
| `eprintln!` (native) | 3 sites, always-on stderr | below |

**colibri-native** (`crates/colibri-native/Cargo.toml`) has no `tracing` / `log` / subscriber. `main` (`crates/colibri-native/src/main.rs:4987-5013`) starts GPUI with no logger.

Native stderr only:

- `crates/colibri-native/src/host.rs:1867-1870` FFI open failed, falling back to process
- `crates/colibri-native/src/host.rs:2182-2184` in-process generate failed, process fallback
- `crates/colibri-native/src/host.rs:2409-2410` plan warning (`tracing_warn_plan` is a name only; it prints, it does not call `tracing`)
- `crates/colibri-native/src/notify_os.rs:150` OS notification failed

If the app is not launched from a terminal, those lines go nowhere.

**colibri-sys** emits three `tracing` events and never installs a subscriber, so they are discarded:

- `crates/colibri-sys/src/plan.rs:573` `tracing::warn!` core count 0
- `crates/colibri-sys/src/probe.rs:766-769` `tracing::warn!` physical-core probe fallback
- `crates/colibri-sys/src/engine/serve.rs:608` `tracing::debug!` unknown mux line

Engine child **stderr is inherited**, not captured (`crates/colibri-sys/src/engine/serve.rs:184-186`). Same terminal-or-nothing problem. Stdout is the serve mux (not a log).

Tauri desktop (`desktop/src-tauri/src/lib.rs`) also has no logger.

## 2. Does the C engine write logs? Where?

**Yes, but they are stderr chatter and opt-in dumps, not a last-run file.**

- Operational messages: `fprintf(stderr, ...)` throughout `c/colibri.c` (CUDA, NUMA, OOM, shard errors, etc.). Always printed when that path runs. No level knob.
- `ROUTE_TRACE=<path>`: file of routing lines, **off unless set** (`c/route_trace.h:90-98`; `docs/ENVIRONMENT.md:104`).
- `COLI_LOGIT_DUMP=1`: top-5 logits to **stderr**, off unless set (`c/sample.h:100-115`; `docs/ENVIRONMENT.md:330`).
- `<model>/.coli_usage` and `<model>/.coli_kv`: expert-pin history and KV cache, **not diagnostic logs** (`c/route_trace.h:19-20`; `docs/ENVIRONMENT.md:49`).
- Convert supervisor (unrelated to native): `/tmp/convert_supervised.log` (`c/scripts/supervisor.sh:13`).

FFI-in-process uses the same C prints on the host process stderr.

## 3. Why GNOME says the app is named "Unknown"

Window title is set. **Wayland / X11 app id is not.**

Native open (`crates/colibri-native/src/main.rs:4997-5008`):

- `titlebar.title = Some("colibrì")`
- `WindowOptions` otherwise `Default` (includes `app_id: None`)

GPUI 0.2.2:

- `WindowOptions.app_id: Option<String>` documented as the desktop-environment identifier (`gpui-0.2.2/src/platform.rs:1123-1124`). Default is `None` (`:1238`).
- App id is applied **only if Some** (`gpui-0.2.2/src/window.rs:1201-1202`).
- Wayland: `xdg_toplevel.set_app_id` (`gpui-0.2.2/src/platform/linux/wayland/window.rs:943-946`).
- X11: same string becomes `WM_CLASS` instance and class (`gpui-0.2.2/src/platform/linux/x11/window.rs:1353-1364`).

Mutter's "Is Not Responding" dialog uses **app_id / WM_CLASS**, not the window title. Empty id → **"Unknown"**.

Tauri *does* set `identifier: "org.colibri.desktop"` (`desktop/src-tauri/tauri.conf.json:5`). Native GPUI has no `.desktop` file and no `app_id`.

Related hang (why the dialog appears): wizard Ready **Start** calls `start_engine` on the UI thread (`main.rs:4088-4091`, `982-996`). `EngineSession::start` is documented **Blocking** (`host.rs:1808-1814`). FFI open of a large leaf model (or a long process READY wait) freezes the GPUI loop. GNOME then marks the window unresponsive. Prefs on this host have `last_model_path = "~/.models"` (`~/.config/colibri/native-ui.toml`), which is **not** a model leaf (`host.rs:393-396` fail-fast). A hang on Ready means a **leaf** model was selected (or another long UI-thread call), not that missing-folder preflight.

## 4. Last-run / crash dump on this host

Looked, no secrets printed.

| Path | What is there |
|------|----------------|
| `~/.local/share/colibri/` | `models/` + `models/colibri.toml` only. **No `logs/`** |
| `~/.config/colibri/` | `native-ui.toml` only. No `install-checkpoint.toml` |
| `~/.cache/colibri` | **does not exist** |
| `~/.local/state/colibri` | **does not exist** |
| coredump / journal product file | **none written by this app** |

There is **no last-run log to open** after Force Quit.

## 5. Smallest default-on logging (recommend, do not implement)

Match existing dirs: config `~/.config/colibri`, data `~/.local/share/colibri`.

1. **Write** `~/.local/share/colibri/logs/native.log` (`$XDG_DATA_HOME/colibri/logs/native.log`). Create the directory on first line. One file, append, rotate at a few MB (keep 2-3 files). Not journal-only: Force Quit must leave a file.
2. **Init in `colibri-native` `main` before `Application::new`**: `tracing-subscriber` + file + stderr. Default filter `info` for `colibri_native` and `colibri_sys`.
3. **Reuse sys `tracing`**. Replace native `eprintln!` with `tracing::info!` / `warn!`. Add a few more: process start, `EngineSession::start` begin/end (ffi vs process, model path, elapsed), wizard Start click, plan/doctor start/end, generate start/stop/error, panic hook.
4. **Disable later**: `COLIBRI_LOG=off` (or `0`) skips the file. `RUST_LOG=...` overrides the filter. No `COLIBRI_LOG` today, so this is a new knob, not a conflict.
5. **C stderr**: do not invent a C log file in the first slice. Tee inherited engine stderr into the same native log when on the process path (today `Stdio::inherit()`).
6. **Same slice, one-liner**: set `WindowOptions.app_id` to `colibri` (or `org.colibri.native`) so GNOME does not say Unknown. Title stays `colibrì`.

Do **not** log tokens, prompts, API keys, or HF tokens.

Hang follow-up (separate from logging): move `EngineSession::start` off the UI thread so Ready cannot trip "Not Responding" while the log is being written.
