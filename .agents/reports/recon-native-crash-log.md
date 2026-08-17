# Recon: last colibri-native run log

We do not automatically receive crash dumps in chat. This is what is on disk.

## Last-run log

**Yes. There is a last-run log.**

- Path: `/home/hunter/.local/share/colibri/logs/native.log`
- Rotated siblings: none (`native.log.1` / `native.log.2` are not present)
- Other files under `~/.local/share/colibri/`: only `logs/` and `models/`. No second crash file.
- Size: 8 lines, about 1 KiB. The file ends on a complete line, not mid-write.
- Last content timestamp: `2026-08-13T20:44:41.715971Z`. This agent could not `stat` mtime (no shell).
- `coredumpctl` was not run (no shell on this explore pass).

The log is from **today** and includes a **second process start** at `20:42:58Z`, which is consistent with a rebuild-and-rerun after the first session. It is **not** a crash transcript. It stops after a successful engine start. There is no `panic:`, no `[prefill]`, no `[stop]`, and no generate begin/end.

## What the log shows (sanitized)

Two sessions, same GLM path (`~/.local/share/colibri/models/GLM-5.2-colibri-int4-g64-with-int8-mtp`):

1. `17:10:09Z` native log opened. `17:10:44Z` Start engine (rail). Engine start begin, then **engine start end** `kind=ffi` `elapsed_ms=7400`.
2. `20:42:58Z` native log opened again (new process). `20:44:36Z` Start engine (rail). Engine start begin, then **engine start end** `kind=ffi` `elapsed_ms=4805`.

**Last thing recorded:** FFI engine start finished successfully. Nothing after that.

If the crash was “trying to use it” (send a prompt), this file never reached generate. `GenerateTrace::begin` logs `generate begin` as soon as the generate worker has a route, before FFI/process generate. That line is missing, so either generate never entered that worker, or the process died before that tracing line was flushed.

## Would a panic have landed in `native.log`?

**Yes, if it was a Rust panic after log init.**

`main` installs the stderr tee first, then `init_native_logging`, which installs a panic hook. The hook writes `panic: {info}` through tracing **and** `append_native_log_line` (direct append + flush). There is no `panic:` line in this file.

## Would SIGSEGV / SIGKILL / OOM have landed in `native.log`?

**No, not as a labeled crash.**

Those paths do not run the Rust panic hook. The process is gone. The tee drain thread dies with it. An unfinished stderr line (no newline) is never appended. OOM killer and SIGKILL write nothing to this file.

## Gaps (tee vs tracing)

- Tracing file layer records Rust `info`/`error` (engine start, generate begin/end, panic hook).
- The stderr tee **only appends lines that start with `[`** (`[prefill]`, `[stop]`, and similar C banners). It does **not** copy ordinary tracing lines into the file (that would double-write).
- C `abort()`, SIGSEGV, and other hard exits can skip both the panic hook and a final tee flush.
- No `[prefill]` in the file means either prefill never printed a banner, or generate never started.

## Bottom line for the operator

We have the last native log. It shows a fresh process at `20:42:58Z` and a successful FFI engine start at `20:44:41Z`. It does **not** show a panic or a generate. Chat does not have a separate dump. If the window vanished after Start engine, or on the first send, the likely missing evidence is a hard process death (signal / abort / OOM), not a Rust panic written to this file.
