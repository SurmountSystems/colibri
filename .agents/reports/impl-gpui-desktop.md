# Implement: colibri-desktop-gpui (native GPUI + colibri-sys)

**Repo:** `/home/hunter/Projects/surmount/colibri`
**Date:** 2026-08-10
**Status:** done (build + unit tests + clippy `-D warnings` green)

---

## What shipped

New workspace crate: `crates/colibri-desktop-gpui`

| Path | Role |
|------|------|
| `Cargo.toml` | package `colibri-desktop-gpui`, edition 2024, rust-version 1.85 |
| `src/main.rs` | GPUI window: Machine / Doctor / Plan / Chat + honesty strip |
| `src/host.rs` | colibri-sys glue (probe, doctor, plan, EngineSession + duplex generate) |
| `src/text_input.rs` | single-line field adapted from gpui 0.2.2 `examples/input.rs` |
| `README.md` | run / env / honesty / relation to Tauri |
| `docs/fidelity.md` | Python/desktop → sys API → GPUI status table |

Root workspace `Cargo.toml` members: `colibri-sys`, `colibri-desktop-gpui`.
Root `README.md`: tree entry + bullet under Rust embed host.

---

## Layout

```
Window title: colibrì (native)
┌─ honesty strip ─────────────────────────────────────────────────┐
│ Host: colibri-sys in-process · Engine: serve mux · Frames: rkyv │
├─ left (~420px) ──────────────────┬─ right (flex) ───────────────┤
│ Machine (re-probe)               │ Chat log                     │
│ Doctor (run)                     │                              │
│ Plan: path field · Plan · Start  │ input + Send                 │
│   engine                         │                              │
└──────────────────────────────────┴──────────────────────────────┘
```

**Background work:** generate runs on a `std::thread` with `mpsc` token events;
UI polls every ~40 ms via `cx.spawn` so the window stays responsive.

**Without a model:** probe + shallow doctor still run; chat explains that a model
path and engine start are required.

---

## How to run

```bash
cargo run -p colibri-desktop-gpui

# with chat:
export COLIBRI_MODEL=/path/to/model   # or COLI_MODEL
export COLI_ENGINE=/path/to/colibri   # optional
cargo run -p colibri-desktop-gpui
```

Env also: `COLIBRI_MODEL_STORE` / `COLI_MODEL_STORE`.

---

## Fidelity summary

| Area | Status |
|------|--------|
| Machine probe | done |
| Doctor (shallow) | done |
| Placement plan | done |
| Chat templates + EngineDuplex stream | done |
| Brain / profile / live tiers | missing (sys APIs exist) |
| HF install / registry picker | missing |
| REST / Tauri SPA | intentionally out of path |

Full table: `crates/colibri-desktop-gpui/docs/fidelity.md`.

---

## Verify (exit codes)

```bash
cargo build -p colibri-desktop-gpui          # exit 0
cargo test -p colibri-desktop-gpui           # exit 0 (3 tests)
cargo test -p colibri-sys --lib              # exit 0 (58 tests)
cargo clippy -p colibri-desktop-gpui --all-targets -- -D warnings  # exit 0
cargo fmt -p colibri-desktop-gpui
```

**Runtime display:** needs X11 or Wayland. Compile succeeds without opening a
window; `cargo run` will fail on a headless host without a display.

---

## Known gaps

1. No Brain / Atlas / PROF panels (visual APIs not bound in UI).
2. Always KV slot 0; no Stop/Cancel button mid-generate.
3. Engine start is blocking on the UI thread (can stall briefly while the C
   process handshakes); token streaming after start is off-thread.
4. Deep doctor not exposed.
5. True in-process `libcolibri` FFI still not available (serve mux process only).
6. Text input is single-line MVP (IME path from gpui example; no multi-line).

---

## Dependencies

- `colibri-sys` path, features `runtime`, `stream`, `tokio` (defaults)
- `gpui = "0.2"` (resolved 0.2.2 on menhera)
- `unicode-segmentation` for the text field
