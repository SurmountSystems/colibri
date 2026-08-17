# Recon: test-run logging vs a real native crash

Read-only inventory. No product edits. Operator crashed `colibri-native` while using it, and said test runs (product tests and implementer verify of a real crash class) must leave enough logs to diagnose.

## Honest finding

The GUI already writes a last-run file. Tests and implementer verify do not use it.

A `cargo test` / nextest pass on parse helpers, banner filters, and `try_lock` cannot explain a crash that happened while chatting against the 429 GB GLM model. Those tests never start `main`, never open that model, never install the stderr tee, and never write the operator's `native.log`. If the crash was a C abort or `SIGSEGV` in-process, the Rust panic hook would not run either. We do not have logs for that class unless C already flushed a `[` banner into the tee before the process died.

That is the gap. Not "no logging at all." Logging exists for a live GUI. Test runs and the last verify reports did not leave a dump of this crash.

## 1. Product runtime (`colibri-native`)

Default-on file: `$XDG_DATA_HOME/colibri/logs/native.log` (this host: `/home/hunter/.local/share/colibri/logs/native.log`). Off only if `COLIBRI_LOG` is `off` / `0` / `false` / `no`. Default filter is `colibri_native=info,colibri_sys=info`. `RUST_LOG` replaces that filter when set.

`main` order (`crates/colibri-native/src/main.rs`):

1. `install_host_stderr_tee()`
2. `init_native_logging()`
3. then GPUI `Application::new()`

| Piece | Present? | What it actually does |
|-------|----------|------------------------|
| Default-on `native.log` | Yes | `log_init.rs` + `colibri-sys` `native_log.rs` / `paths.rs`. Dual-write file + stderr. Rotate at 4 MiB, keep `native.log.1` and `.2`. Redacts `hf_…` / `sk-…` and common `HF_TOKEN=` / `API_KEY=` assignments. |
| Panic hook | Yes | `install_panic_hook` in `log_init.rs`. Writes `tracing::error!` and `append_native_log_line` with `panic: …`. Installed whenever file init is attempted (including file-open failure, still Enabled + hook). |
| C banner tee | Yes, GUI only | `stderr_tee.rs` remaps fd 2 on Unix. Drain thread echoes every line to the saved TTY. File-appends only lines that start with `[`. Also parses `[prefill]` into the chip snapshot. |
| Rust operational lines | Yes | Engine start begin/end, start clicked, start failed, FFI open fallback, generate begin/end (`GenerateTrace` Drop). No prompts, no generate tokens. |
| C abort / `SIGSEGV` | No | No `sigaction`, no abort handler. A hard C fault kills the process. The panic hook does not run. Last flushed tracing lines and `[` banners may remain. `SIGKILL` (Force Quit) is the same: no hook. |
| Crash reporter | No | By design. No Sentry, no minidump product. |

`colibri-sys` has the policy helpers and three `tracing` events (`plan`, `probe`, `serve`). It does not install a subscriber. The native host does.

## 2. `cargo test` / nextest

`colibri-native` is a bin crate. Unit tests compile that bin as a test harness. They do **not** call `main`, `install_host_stderr_tee`, or `init_native_logging`.

What tests cover today:

- Policy: `native_log_enabled_*`, filter, sanitize, path, `ensure_log_directory` (`colibri-sys`).
- File helper: rotate, redact, `append_native_log_line` to a **tempfile** (`log_init::tests`).
- Tee helper: `handle_stderr_line_appends_c_banners_only` with fake strings to a tempfile. Does not `dup2` fd 2.
- No test that the panic hook writes a line.
- No test that `init_native_logging_with` installs a subscriber and emits a tracing line (once-per-process `try_init` would be awkward in the same test binary).

Consequences:

- Failing tests print the usual Rust assert / panic text on the harness stdout/stderr. Cargo and nextest show that on failure. There is no workspace nextest config and no `RUST_LOG` in `just rust-nextest`.
- `tracing::info!` / `warn!` in product code is discarded during tests (no subscriber).
- The operator `native.log` is **not** written by these tests.
- Real C `fprintf` banners are **not** captured. The tee is never installed in tests (and must not be: remapping fd 2 would scramble the harness).
- Tests do not stub logging off. They simply never turn it on. Isolated file tests use tempdirs.

`colibri-sys` tests are the same shape: policy and formatters, no live `native.log`, no GUI, no 429 GB model.

A green `cargo test -p colibri-native` filter on parse / tee / `try_lock` means those helpers match their strings. It is not a last-run dump.

## 3. `just check` / `just install`

`just check` is fmt, clippy, `cargo nextest run --workspace --all-targets`, C `make check`, Python, web. No log directory, no `COLIBRI_LOG`, no tail, no artifact upload of `native.log`.

`just install` is `cargo install --path crates/colibri-native …`. It does not run the app and does not capture logs.

## 4. Last verify reports (prefill)

`.agents/reports/impl-prefill-progress.md` and `.agents/reports/process-mop-prefill.md`:

- Verify was `cargo fmt`, `clippy`, and seven unit tests (parse prefill, format, apply without the engine mutex, `try_lock` snapshot, 0% generate honesty, banner filter).
- No real app launch.
- No GLM open.
- No tail of `native.log`.
- The implement report told the **operator** to rebuild and run the new binary. That is not implementer verify of the crash class.

Expected, and confirmed.

## Smallest fix (do not implement here)

Stay inside the existing file. Do not add a crash reporter, Sentry, or a generate-% redesign.

**Product (small):**

1. Keep default-on `native.log`, the panic hook, and the C banner tee as they are.
2. Add one `#[test]` that the panic path writes a line: install the hook against a tempfile, `catch_unwind` a panic, assert the file contains `panic:`. Restore the previous hook so the harness stays intact.
3. Keep the existing tempfile test that `handle_stderr_line` appends `[` banners only. That already covers the tee's file contract without `dup2` in tests.

**Process (already pinned in project `AGENTS.md` § Native crash logs; do not invent a second rule):**

1. After an operator crash or hang, the first artifact is the tail of `native.log` and `native.log.1` / `.2`. Unit tests are not that dump.
2. When the operator just crashed the real app, implementer verify is not done until that tail is read (and, if the tree was just rebuilt, the new binary was actually run). Targeted cargo tests stay required for TDD. They do not replace the file.
3. If the crash class cannot appear in the file (`SIGKILL`, or C `SIGSEGV` / abort with no hook), say that in the implement report. Do not claim we logged a signal that never writes.

What this still will not catch: launching the 429 GB GLM model inside CI, or capturing a raw C segfault. Those stay out of scope on purpose.
