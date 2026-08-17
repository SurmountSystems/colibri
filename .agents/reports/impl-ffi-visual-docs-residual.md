# Report: docs + residual closeout (`open:ffi-visual-abi` Step docs)

**Date:** 2026-08-10
**Scope:** Docs and residual only. No product code changes. No git commit.

---

## Outcome

**`open:ffi-visual-abi` is CLOSED.**

Honest close meaning:

- Brain / live PROF / HWINFO / TIERS work on **pure FFI for the GLM opt-in path**
  without a SERVE child (`coli_glm_visual_poll` → `FfiEngine::pump_visual` →
  native `LiveEngine::Ffi` pump).
- Cooperative cancel on FFI via token callback only.
- Process serve mux remains the **product default**.
- Still **not** claimed by this residual:
  - Kimi / Inkling visual **fill** (stubs return empty success)
  - DeepSeek V4 visual poll symbols (**empty** snapshot)
  - Mux multi-slot STOP on pure FFI (process-only)
  - Product-default FFI (`open:ffi-product-default` stays open)
  - GPU link / NPU inference

Prior implement reports (evidence, not re-run here):

| Step | Report |
|------|--------|
| C ABI | `.agents/reports/impl-ffi-visual-c-api.md` |
| Rust + native | `.agents/reports/impl-ffi-visual-rust-native.md` (105 ffi tests, 78 native) |

---

## Files updated

| Path | Change |
|------|--------|
| `crates/colibri-sys/docs/ffi-phase-d.md` | Status table: visual poll shipped (GLM full; Kimi/Inkling stub; V4 empty); visual section marked closed; residual table drops `open:ffi-visual-abi`; isolation policy subsection (host-kill risk, `COLIBRI_FORCE_PROCESS`, product-default gated); acceptance + references |
| `crates/colibri-native/docs/fidelity.md` | Brain / PROF / HWINFO / TIERS / Stop rows: process vs opt-in GLM poll honesty; FFI row no longer claims visual needs process; architecture diagram + phrase table updated |
| `.agents/RESIDUAL.md` | `open:ffi-visual-abi` moved to CLOSED with one-line outcome; open list keeps product-default, GPU, NPU, openai-rest, visual-pump-idle-stop; architecture reminder and MVP status rewritten for honesty |

---

## Isolation policy note (also in ffi-phase-d)

- In-process FFI shares the host address space; a fault can **kill the host**.
- Kill-switch: `COLIBRI_FORCE_PROCESS=1`.
- Product default stays `prefer_process = true`.
- **`open:ffi-product-default`** remains open until isolation story is accepted
  (plan Step 4 / product decision). Visual poll alone does not flip the default.

---

## Open residuals (unchanged intent)

| Id | Status |
|----|--------|
| `open:ffi-product-default` | Open (isolation + product decision) |
| `open:ffi-gpu` | Open (CPU-only archives) |
| `open:npu-inference` | Deferred |
| `open:openai-rest` | Intentionally absent |
| `open:visual-pump-idle-stop` | Polish |

---

## Constraints respected

- Docs / residual only; no product code edits
- No git commit / stage
- Honest about stubs, V4 empty, cooperative cancel, process default
