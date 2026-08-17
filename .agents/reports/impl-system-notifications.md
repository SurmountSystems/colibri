# Implement report: OS system notifications

**Repo:** `/home/hunter/Projects/surmount/colibri`
**Slice:** Desktop/OS notifications (not in-app toast) for install complete + inference stop
**Date:** 2026-08-11

## Outcome

`colibri-native` raises real OS notifications via **`notify-rust` 4** (FreeDesktop / macOS / Windows toast) on:

1. **Successful model install** (`InstallEvent::Done`)
2. **Inference end** (`GenEvent::Done` finished or user-stopped; `GenEvent::Error` once)

Fail soft: daemon/bus errors only `eprintln!`; install/generate never crash.

## Files

| Path | Change |
|------|--------|
| `crates/colibri-native/src/notify_os.rs` | **New** module: gating, title/body builders, thin send wrapper, unit tests |
| `crates/colibri-native/src/main.rs` | `mod notify_os`; wire install Done + gen Done/Error |
| `crates/colibri-native/Cargo.toml` | `notify-rust = "4"` |

## Wire points

### Install success

`ColibriApp::drain_install` on `InstallEvent::Done(r)`:

- After UI status / model path set
- `notify_install_complete(&r.dest)` once per Done
- No notify on Progress / Paused / Error / cancel

### Inference stop

`ColibriApp::drain_gen`:

| Frame | Kind | Title (example) |
|-------|------|-----------------|
| `GenEvent::Done`, `!stop_requested` | `Finished` | Reply finished · N tok · R tok/s |
| `GenEvent::Done`, `stop_requested` | `StoppedByUser` | Generation stopped |
| `GenEvent::Error`, `stop_requested` | `StoppedByUser` | Generation stopped |
| `GenEvent::Error`, `!stop_requested` | `Error` | Generation failed + truncated summary |

Tokens never notify. One notify per terminal event.

## Copy (product fidelity)

Plain operational English; no marketing slogans. Brand appname `colibrì` only for the daemon app field.

- Install: title `Model install complete`, body `Download finished: {last path segment}`
- Finished: `Reply finished` (+ optional tok stats)
- User stop: `Generation stopped` / `Stopped by user`
- Error: `Generation failed` + body truncated to 160 chars

## API surface (`notify_os`)

- `should_notify_install_complete`, `should_notify_inference_end`
- `inference_end_kind(stop_requested, is_error)`
- `install_complete_copy`, `inference_end_copy`, `model_label_from_dest`, `truncate_plain`
- `send_os_notification` (fail soft)
- `notify_install_complete`, `notify_inference_end`

CI: pure helpers + gating tested; `show()` not asserted (no desktop bus required).

## Verify

```text
cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets -- -D warnings   # exit 0
cargo test -p colibri-native -- notify_os                     # 11 passed
```

Also green: `host::tests::status_after_gen_done_respects_stop_requested`.

## Out of scope / notes

- No in-app toast changes.
- Install pause/cancel/error: no OS notify (success-only install).
- Live manual check needs a notification daemon (e.g. `mako`/`dunst`/`org.freedesktop.Notifications`); tests do not require it.
- No git commit (operator-owned).
