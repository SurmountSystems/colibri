# Recon: plan inputs for FFI Phase D (post-closeout)

**Date:** 2026-08-10
**Sources:** `.agents/RESIDUAL.md`, `crates/colibri-sys/docs/ffi-phase-d.md`, fidelity, closeout reports, Cargo + `ffi/` code.

## Status of open:ffi-phase-d

**Closed** (opt-in bar only). Residual CLOSED table: “Phase D multi-family CPU static FFI **opt-in complete** (`open:ffi-phase-d`)”. Updated 2026-08-10. Process remains product **default**. D6 closeout: `.agents/reports/impl-ffi-d6-closeout.md`.

## Already shipped (opt-in bar)

| Slice | Outcome |
|-------|---------|
| D0 docs honesty | fidelity / README / user-guide / ffi-phase-d: opt-in CPU multi-family, process default |
| D1 Inkling | `libinkling.a` + `FfiFamily::Inkling` with GLM / Kimi / V4 |
| D2 golden | `glm_tiny_process_ffi_token_parity` (greedy token ids on `c/glm_tiny`) |
| D3 desktop | `colibri-native` feature `ffi` + `COLIBRI_PREFER_FFI` + process fallback |
| D4/D5 | **Spin only** (no code) → new residual ids |
| D6 | Residual + docs closeout; tests green (sys 91 / ffi 102; native 78 ×2) |

**Code facts**

- `FfiFamily { Glm, Kimi, Inkling, DeepseekV4 }`; `open_engine(family, model_dir)` in `crates/colibri-sys/src/ffi/multi.rs`
- `prefer_process: true` default; `COLIBRI_FORCE_PROCESS` kill-switch; desktop `COLIBRI_PREFER_FFI=1` → prefer_process false
- Features: both crates `ffi` **off** by default (`colibri-sys` `ffi = []`; native `ffi = ["colibri-sys/ffi"]`)
- **Process is still product default** (`must_use_process` / host `resolve_prefer_process`)

## Still open (spun residuals)

| Id | Gap |
|----|-----|
| `open:ffi-visual-abi` | Brain / live PROF / HWINFO / mux STOP without engine process (D4 spin) |
| `open:ffi-product-default` | Flip product default to FFI (needs visual + isolation + product OK) |
| `open:ffi-gpu` | One-platform GPU static/dynamic link; archives CPU-only today (D5 spin) |
| `open:npu-inference` | Deferred (unchanged; not Phase D) |

## Gaps vs a full production-default close

- Visual ABI: opt-in FFI open/generate only; empty visual; cooperative cancel, not mux STOP
- Product default: still `prefer_process = true`; host dual-path is opt-in only
- GPU: `NOCUDA` / no Metal/Vulkan in static matrix
- Golden: GLM tiny only; not full-weight all-family parity
- Also not claimed: full UTF-8 detokenize on multi-family FFI; Inkling audio in embed generate
- Crash isolation: in-process fault can kill host (docs § Thread and device)

## Suggested plan slices (D-order, no implementation)

1. **Visual ABI (`open:ffi-visual-abi`)** — C poll + Rust surface for expert map / hits / PROF / HWINFO; mux-equivalent STOP with req_id on pure FFI path
2. **Golden expand (optional polish)** — more families/tiny fixtures if fixtures exist; keep process↔FFI token contract
3. **Product default (`open:ffi-product-default`)** — only after (1) + isolation policy; flip `prefer_process` / desktop default; keep kill-switch
4. **GPU (`open:ffi-gpu`)** — one platform at a time (CUDA or Metal/Vulkan); not NPU
5. **NPU** — leave deferred unless product re-ranks `open:npu-inference`

## Related reports (1-line each)

| File | Outcome |
|------|---------|
| `impl-ffi-d0-d1-inkling.md` | Docs honesty + Inkling static + Rust family |
| `impl-ffi-d2-golden.md` | GLM tiny process↔FFI token parity green |
| `impl-ffi-d3-desktop.md` | Native opt-in FFI + fallback + force-process |
| `impl-ffi-d6-closeout.md` | Close open:ffi-phase-d; spin visual/default/GPU |
| `recon-plan-ffi-complete.md` | Pre-impl plan recon (superseded by shipped D0–D6) |
| `impl-track-ffi-libcolibri.md` | Earlier multi-family track impl |
| `recon-plan-ffi-libcolibri.md` | Early true-libcolibri recon |

## Canonical docs

- `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/ffi-phase-d.md` — “Opt-in complete; process default”
- `/home/hunter/Projects/surmount/colibri/crates/colibri-native/docs/fidelity.md` — FFI row **done (opt-in)**
- `/home/hunter/Projects/surmount/colibri/.agents/RESIDUAL.md` — CLOSED + OPEN spun ids
