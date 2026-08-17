# Plan: Finish FFI after Phase D (spun residuals)

## Context

### What the residual table row meant

The row you quoted (`open:ffi-phase-d` = Partial: multi-family CPU + sizes + kill-switch; process still default) described **mid-campaign** state. On disk as of 2026-08-10 that residual is **CLOSED** at the **opt-in bar**:

| Closed (do not re-implement) | Evidence |
|------------------------------|----------|
| Multi-family CPU static libs | GLM, Kimi, Inkling, DeepSeek V4 `.a` + `FfiFamily` / `open_engine` |
| Size metadata on public types | `ModelInfo.disk_bytes`, C size summary |
| Kill-switch | `COLIBRI_FORCE_PROCESS`, `prefer_process` |
| Tiny golden | `glm_tiny_process_ffi_token_parity` |
| Desktop opt-in | `colibri-native` `feature=ffi` + `COLIBRI_PREFER_FFI` + process fallback |
| Docs honesty | `ffi-phase-d.md`, `fidelity.md`, residual CLOSED |

**Process remains the product default** on purpose. That is not unfinished Phase D; it is a separate residual.

### Goal of this plan

Complete what still makes in-process FFI feel “partial” for production use: the **three spun residuals** (plus optional golden polish). Do **not** reopen `open:ffi-phase-d` as a re-build of D0–D3.

| Residual | What “done” means |
|----------|-------------------|
| `open:ffi-visual-abi` | Brain / live PROF / HWINFO / cooperative stop work on pure FFI without a SERVE child |
| `open:ffi-product-default` | Product can default to FFI with process fallback + force-process kill-switch (only after visual + isolation story) |
| `open:ffi-gpu` | One platform can link GPU into the embed path (not full multi-backend day one) |

### Non-goals

- Re-ship Inkling / multi-family open / desktop opt-in (already green).
- Drop process SERVE forever.
- Full multi-family full-weight golden CI.
- NPU inference (`open:npu-inference` stays deferred).
- Claim Brain/PROF parity on FFI before visual ABI lands.

### Assumptions

1. Visual ABI is the **first** product-facing gap (native FFI path returns empty `VisualSnapshot` today).
2. Product-default FFI is **gated** on visual + written isolation policy (in-process crash can kill the host).
3. GPU is **one platform first** (recommend Linux CUDA or Metal depending on operator hardware; default plan picks **Linux CUDA** as the documented process path, with AMD/ROCm as explicit alternate if you prefer AMD host first).
4. Mux multi-slot / grammar on pure FFI is **out of scope** unless visual work uncovers a free win; process keeps full mux.

### Recon

- `.agents/reports/recon-plan-ffi-phase-d-complete.md`
- `.agents/reports/recon-plan-ffi-spun-residuals.md`
- Closeouts: `impl-ffi-d0-d1-inkling.md`, `impl-ffi-d2-golden.md`, `impl-ffi-d3-desktop.md`, `impl-ffi-d6-closeout.md`

---

## Approach

**Recommended path: visual ABI → (optional golden expand) → product-default flip → one-platform GPU.**

1. **`open:ffi-visual-abi` first**
   Extend C embed ABI beyond open/size/generate/destroy so the host can poll the same telemetry the process path prints (`HWINFO` / `EMAP` / `HITS` / `PROF` / `TIERS` from `c/telemetry.h`). Decode into existing `VisualSnapshot` types. Wire `FfiEngine` + native `LiveEngine::Ffi` `pump_visual`. Keep STOP as cooperative token-callback cancel (already on FFI); document that mux `STOP` multi-slot is process-only until a later design.

2. **Optional golden expand**
   If tiny fixtures exist for Kimi/V4/Inkling, add process↔FFI token parity tests the same way as GLM tiny. Not a gate for visual or default flip.

3. **`open:ffi-product-default` second**
   After Brain/PROF work on FFI and isolation docs are honest: flip `prefer_process` default to false only where product policy says so (crate default and/or native host default). Keep process fallback on FFI open failure and `COLIBRI_FORCE_PROCESS`. Update fidelity + residual.

4. **`open:ffi-gpu` third**
   One platform: link GPU backend objects into embed build (prefer **dynamic** load like Windows DLL path over bloating every static archive). CPU static libs stay the default `feature=ffi` matrix; GPU is an extra feature or Makefile flag.

**Not recommended:** flip product default before visual ABI (Brain/PROF die on “default”). Re-open Phase D for more family open paths. Full GPU matrix in one PR.

---

## Critical files

| Path | Why |
|------|-----|
| `c/colibri_api.h` | Embed ABI today is open/size/generate/destroy only |
| `c/telemetry.h` | Process telemetry layouts to mirror |
| `c/colibri.c`, `c/kimi_k3.c`, `c/inkling.c` | Family engines; `*_NO_MAIN` libs |
| `c/Makefile`, `c/Makefile.deepseek-v4` | Static lib + GPU process targets |
| `c/backend_cuda.cu`, `backend_gpu_compat.h`, `backend_metal.mm`, `backend_vulkan.c`, `backend_loader.c` | GPU backends (process/DLL today) |
| `crates/colibri-sys/src/ffi/{mod,multi,v4,bindings}.rs` | `open_engine`, generate, cancel |
| `crates/colibri-sys/build.rs` | Links four CPU `.a` |
| `crates/colibri-sys/src/engine/{mod,serve,duplex}.rs` | Process visual parse + `stop_request` + `pump_visual` |
| `crates/colibri-sys/src/visual.rs` | `VisualSnapshot`, expert maps |
| `crates/colibri-sys/src/config.rs` | `prefer_process`, kill-switch helpers |
| `crates/colibri-sys/docs/ffi-phase-d.md` | Phase D SoT + spun residual names |
| `crates/colibri-native/src/host.rs` | Dual path, empty FFI visual, env resolve |
| `crates/colibri-native/src/main.rs` | Visual pump timer, Brain/PROF UI |
| `crates/colibri-native/docs/fidelity.md` | Honesty matrix |
| `.agents/RESIDUAL.md` | Open/closed residual board |

---

## Reuse

| Symbol / module | Path | How |
|-----------------|------|-----|
| `ServeClient` line parsers | `engine/serve.rs` | Mirror decode into shared helpers for C buffer → snapshot |
| `pack_expert_cell` / `ExpertMap` / `ExpertHits` | `visual.rs` | Same layout as process hex |
| `EngineHandle::pump_visual` | `engine/mod.rs` | Pattern for FFI poll |
| `stop_request` / mux STOP | `engine/serve.rs` | Process-only reference; FFI keeps token-fn cancel |
| `FfiEngine` / `open_engine` | `ffi/multi.rs` | Add poll/snapshot methods |
| `prefer_process` / `must_use_process` | `config.rs` | Flip + kill-switch for product-default |
| `resolve_prefer_process` / `LiveEngine` | `native/host.rs` | Default flip + non-empty FFI visual |
| Process GPU Makefile flags | `c/Makefile` | One-platform embed link matrix |

---

## Steps

### Step 1 — Visual ABI design + C surface (`open:ffi-visual-abi`)

1. Define C poll API (or snapshot struct) that exposes the same fields process prints (`telemetry.h` contract). Prefer **poll** over inventing a second stdout mux inside the process.
2. Implement for at least **one** family (GLM) in the static lib; stub or no-op cleanly for others until extended.
3. **Red:** unit test that fixed fixture bytes decode to a non-empty `VisualSnapshot` without a subprocess.
4. **Green:** C + Rust decode path.

### Step 2 — Rust + native wire-up

1. `FfiEngine`: poll/snapshot API; `pump_visual` equivalent.
2. Native `LiveEngine::Ffi`: fill `VisualSnapshot` so Brain/PROF pump is not always empty.
3. STOP: keep cooperative cancel; add regression test that mid-generate cancel returns early on FFI path.
4. Docs: `ffi-phase-d.md` + `fidelity.md` visual row; residual update when Brain/PROF work on opt-in FFI without process.

### Step 3 — Optional golden expand

1. If fixtures exist, add family token-parity tests (same pattern as GLM tiny).
2. Skip if no tiny model dirs; do not invent large CI downloads.

### Step 4 — Product default (`open:ffi-product-default`)

**Depends on Steps 1–2 + isolation policy in docs.**

1. Decide default flip scope: crate `prefer_process` default, and/or native host only.
2. **Red:** after flip, default start path tries FFI when `feature=ffi` is built; without feature still process; `COLIBRI_FORCE_PROCESS` always process.
3. Process fallback on open failure stays.
4. Close residual; update fidelity.

### Step 5 — One-platform GPU (`open:ffi-gpu`)

1. Pick platform (plan default: **Linux CUDA** static/dynamic link for one family; alternate AMD HIP if operator prioritizes ROCm host).
2. Extend Makefile + `build.rs` (or dynamic loader) without making GPU required for default `feature=ffi`.
3. **Red:** build with GPU feature fails to link or fails a smoke open when GPU flag is on but backend missing; green when toolchain present (may be host-gated / `#[ignore]` without CUDA).
4. Residual + docs: CPU default, GPU opt-in, NPU still deferred.

### Step 6 — Closeout mop

1. fmt / clippy / targeted tests on touched crates.
2. Residual honesty: only close each residual when its done bar is true; leave NPU open.

---

## Risks

| Risk | Mitigation |
|------|------------|
| In-process crash kills desktop | Docs + keep process kill-switch; do not flip default until isolation story is written |
| Visual layout drift vs process | Single decode path / shared fixtures; process golden lines as oracle |
| OpenMP / re-entrancy | One generate per handle; document thread rules (already in ffi-phase-d) |
| GPU rpath / cudart on host | Prefer dynamic load; keep default `feature=ffi` CPU-only |
| Product-default before visual | Hard gate in this plan: Step 4 after Steps 1–2 |

---

## Verification

| Slice | Red → green |
|-------|-------------|
| Visual decode | New test: fixture → non-empty `VisualSnapshot` without subprocess; then same test green after C/Rust wire |
| FFI cancel | Existing or new mid-generate cancel test on `feature=ffi` |
| Product default | Config/host tests: default path, force-process override, process fallback |
| GPU | Feature-gated link/smoke; ignore without device if needed |
| Regression | `cargo test -p colibri-sys --features ffi`; `cargo test -p colibri-native --features ffi`; clippy on touched packages |

Manual: native with `COLIBRI_PREFER_FFI=1` after Step 2 shows live Brain/PROF during generate without SERVE child (or documents remaining gaps honestly).

---

## Open questions

- **Q1 — GPU platform first:** Linux CUDA (plan default) vs AMD HIP vs Apple Metal for the first embed GPU slice?
- **Q2 — Product-default scope:** flip crate-wide `prefer_process` default, or only `colibri-native` host resolution when built with `feature=ffi`?
- **Q3 — Visual families:** ship GLM visual poll first then expand, or require all four families before closing `open:ffi-visual-abi`?

If Q1–Q3 are unanswered at approve time, implement with defaults: **CUDA first**, **native-host default flip only (crate stays process-prefer until native proves stable)**, **GLM-first visual then expand**.

---

## Board after approval (seed)

| Id | Work |
|----|------|
| `feat:ffi-visual-abi` | Close `open:ffi-visual-abi` |
| `impl:ffi-visual-c-api` | C poll/snapshot ABI + GLM |
| `impl:ffi-visual-rust-native` | Rust decode + native pump |
| `impl:ffi-visual-docs-residual` | Docs + residual close |
| `feat:ffi-product-default` | Close `open:ffi-product-default` (gated) |
| `feat:ffi-gpu` | Close `open:ffi-gpu` one platform |
| `impl:ffi-gpu-one-platform` | Makefile/build.rs + smoke |
| optional `impl:ffi-golden-expand` | More family tinies if fixtures exist |

---

### Critical Files for Implementation

- `c/colibri_api.h` — extend embed ABI for visual/stop telemetry
- `c/telemetry.h` — process layout contract to mirror
- `crates/colibri-sys/src/ffi/` — poll + generate cancel
- `crates/colibri-sys/src/visual.rs` + `engine/serve.rs` — decode reuse
- `crates/colibri-native/src/host.rs` — non-empty FFI visual + default path
- `crates/colibri-sys/build.rs` + `c/Makefile` — GPU link matrix
- `crates/colibri-sys/docs/ffi-phase-d.md` + `.agents/RESIDUAL.md` — honesty
