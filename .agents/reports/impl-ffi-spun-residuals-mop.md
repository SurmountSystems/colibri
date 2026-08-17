# Process mop: FFI spun residuals (product-default + GPU)

**Date:** 2026-08-10
**Repo:** `/home/hunter/Projects/surmount/colibri`
**Role:** process mop after parallel implementers (product-default + GPU)

## Residual honesty (`.agents/RESIDUAL.md`)

| Residual | Expected | On disk | Verdict |
|----------|----------|---------|---------|
| `open:ffi-visual-abi` | CLOSED | CLOSED (table + MVP status) | Honest |
| `open:ffi-product-default` | CLOSED | CLOSED (native-host only; crate `prefer_process` stays true) | Honest |
| `open:ffi-gpu` | CLOSED | CLOSED (one platform: Linux CUDA + GLM; default `ffi` CPU-only) | Honest |
| `open:npu-inference` | OPEN (deferred) | OPEN under High value / product | Honest |

No contradictory open/closed race found between parallel writers. Header and
MVP status already say native FFI-first; product-default + one-platform GPU
closed; NPU still deferred. Architecture reminder matches.

## Docs skim (`crates/colibri-sys/docs/ffi-phase-d.md`)

| Area | Status |
|------|--------|
| Status blurb (top) | Consistent: visual / product-default / GPU closed; NPU deferred |
| Cargo features / CUDA gates | Consistent with residual |
| Isolation policy | Consistent (native FFI-first accept; library process-prefer) |
| Still out of scope table | Only `open:npu-inference` open; three closed ids listed |
| **Visual section (stale)** | Had “Product default stays **process**” and “does **not** mean product-default FFI” after product-default closed |

**Fix applied (mop only):** rewrote the Visual / stop section so it no longer
claims product default is process-only, and retitled the capability table to
“Process path (library default / fallback)” vs “CPU FFI (native default under
`feature=ffi`)”. Product-default and GPU remain documented as separate closed
residuals below that section.

No other residual/docs edits required.

## Commands and exit codes

| Step | Command | Exit |
|------|---------|------|
| fmt | `cargo fmt -p colibri-sys -p colibri-native` | **0** |
| test sys | `cargo test -p colibri-sys --features ffi --lib` | **0** (107 passed, 0 failed, 1 ignored) |
| test native | `cargo test -p colibri-native --features ffi` | **0** (81 passed) |
| clippy sys | `cargo clippy -p colibri-sys --features ffi --all-targets -- -D warnings` | **0** |
| clippy native | `cargo clippy -p colibri-native --features ffi --all-targets -- -D warnings` | **0** |

Ignored: `ffi::cuda_gate_tests::ffi_cuda_linked_when_toolkit_present` (host-gated;
needs `ffi-cuda` + toolkit).

## Fixes from the wave

- **Code / tests / clippy:** none required (all green).
- **Docs:** one stale contradiction in `ffi-phase-d.md` visual section (above).

## Residual snapshot (post-mop)

**CLOSED (FFI-related):** `open:ffi-phase-d`, `open:ffi-visual-abi`,
`open:ffi-product-default`, `open:ffi-gpu`.

**OPEN (still):** `open:npu-inference` (deferred); also non-FFI opens
`open:openai-rest`, `open:visual-pump-idle-stop`.

No git commit (operator owns VCS).
