# Report: D4 spin + D5 spin + D6 closeout (`open:ffi-phase-d`)

**Date:** 2026-08-10
**Scope:** Residual closeout only (docs + residual ids). No full visual/stop C ABI; no GPU link.

## Outcome

**Complete.** Phase D multi-family CPU static FFI is closed at the **opt-in complete**
bar. Process serve remains the product **default**. New residual ids spin out
visual ABI, optional product-default flip, and GPU link. NPU stays deferred.

## Close bar (approved plan)

| Slice | Action | Result |
|-------|--------|--------|
| D0–D3 | Already shipped (Inkling, golden, desktop opt-in) | Unchanged product code |
| D4 | **Spin only** — do not build full visual/stop C ABI | Documented; residual `open:ffi-visual-abi` |
| D5 | **Spin only** — do not build GPU link | Documented; residual `open:ffi-gpu` |
| D6 | Close `open:ffi-phase-d` + residual/docs/report | Done |

## What `open:ffi-phase-d` closed for

- Multi-family **CPU** static: GLM (`libcolibri.a`), Kimi (`libkimi_k3.a`), V4 (`libdeepseek_v4.a`), **Inkling** (`libinkling.a`)
- Size metadata on public types + C size probe
- Kill-switch: `prefer_process` default true, `COLIBRI_FORCE_PROCESS`
- Tiny golden process↔FFI token parity (GLM tiny)
- Desktop opt-in: `colibri-native` feature `ffi` + `COLIBRI_PREFER_FFI` / process fallback

**Process remains product default.** Not claimed: product-default in-process engine.

## New open residual ids

| Id | Gap | Notes |
|----|-----|--------|
| `open:ffi-visual-abi` | Brain / live PROF / HWINFO / mux STOP without engine process | D4 spin. Live visual and STOP still need serve mux + C engine process. |
| `open:ffi-product-default` | Flip product default to FFI | Optional spin. Needs visual ABI parity + isolation story + product decision. |
| `open:ffi-gpu` | One-platform GPU static/dynamic link | D5 spin. Archives are CPU-only today. |
| `open:npu-inference` | NPU decode | Still **deferred** (unchanged). |

## Files touched

| Path | Change |
|------|--------|
| `.agents/RESIDUAL.md` | Close `open:ffi-phase-d`; add visual / product-default / GPU opens; MVP status rewrite |
| `crates/colibri-sys/docs/ffi-phase-d.md` | Status **opt-in complete; process default**; D4/D5 sections; residual table |
| `crates/colibri-native/docs/fidelity.md` | Multi-family FFI row **done (opt-in)**; visual/STOP + CPU-only notes |
| `.agents/reports/impl-ffi-d6-closeout.md` | This report |

## Honesty checks

- Do **not** claim product-default in-process engine.
- Brain / live PROF / mux STOP → engine process until `open:ffi-visual-abi`.
- FFI archives → CPU-only until `open:ffi-gpu`.
- NPU inference still deferred (`open:npu-inference`).

## Verify

```bash
cargo fmt -p colibri-sys -p colibri-native
cargo clippy -p colibri-sys --all-targets -- -D warnings
cargo clippy -p colibri-sys --all-targets --features ffi -- -D warnings
cargo clippy -p colibri-native --all-targets --features install -- -D warnings
cargo clippy -p colibri-native --all-targets --features install,ffi -- -D warnings
cargo test -p colibri-sys --lib
cargo test -p colibri-sys --lib --features ffi
cargo test -p colibri-native
cargo test -p colibri-native --features ffi
```

| Step | Result |
|------|--------|
| `cargo fmt -p colibri-sys -p colibri-native` | ok |
| `cargo clippy -p colibri-sys --all-targets -- -D warnings` | ok |
| `cargo clippy -p colibri-sys --all-targets --features ffi -- -D warnings` | ok |
| `cargo clippy -p colibri-native --all-targets --features install -- -D warnings` | ok |
| `cargo clippy -p colibri-native --all-targets --features install,ffi -- -D warnings` | ok |
| `cargo test -p colibri-sys --lib` | **91 passed** |
| `cargo test -p colibri-sys --lib --features ffi` | **102 passed** |
| `cargo test -p colibri-native` | **78 passed** |
| `cargo test -p colibri-native --features ffi` | **78 passed** |

Docs-only closeout; no product code fallout.

## Prior wave reports

- `.agents/reports/impl-ffi-d0-d1-inkling.md`
- `.agents/reports/impl-ffi-d2-golden.md`
- `.agents/reports/impl-ffi-d3-desktop.md`
- `.agents/reports/impl-track-ffi-libcolibri.md`
