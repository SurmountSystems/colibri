# Report: D0 docs honesty + D1 Inkling CPU FFI

**Date:** 2026-08-10
**Scope:** colibri D0 fidelity/docs honesty; D1 Inkling multi-family CPU static embed (`libinkling.a` + Rust `FfiFamily::Inkling`)

## Outcome

Both verticals complete. Process serve remains the product default. Multi-family CPU FFI is opt-in (`feature = "ffi"`) and now includes Inkling alongside GLM, Kimi, and DeepSeek V4.

## D0 — docs honesty

| File | Change |
|------|--------|
| `crates/colibri-native/docs/fidelity.md` | Replaced “stub / design only / Not true FFI” with **opt-in multi-family CPU static FFI (partial)**; architecture diagram shows process default + optional `ffi` |
| `crates/colibri-native/README.md` | Architecture note: subprocess engine is product default; `ffi` is not default |
| `crates/colibri-sys/docs/user-guide.md` | Lists `libinkling.a`; Inkling in opt-in family set; removed “Inkling out” residual |
| `crates/colibri-sys/docs/ffi-phase-d.md` | Inkling static lib, `COLIBRI_INKLING_STATIC_LIB`, linked families, residual scrub |
| `crates/colibri-sys/docs/README.md`, crate README, `lib.rs` feature table | Multi-family wording includes Inkling |

**Not claimed:** product-default in-process engine. Kill-switch and `prefer_process = true` unchanged.

## D1 — Inkling CPU FFI

### C

- `c/inkling.c`: `#ifndef INKLING_NO_MAIN` around CLI `main`; under `INKLING_NO_MAIN`, `coli_ink_engine_open` / `destroy` / `size` / `generate` (text-only greedy decode + token callback; disk walk requires `.safetensors`).
- `c/colibri_api.h`: `ColiInkEngine`, open/generate options, prototypes; family/engine id strings document `inkling`.
- `c/Makefile`: `libinkling` → `libinkling.a` via `inkling.lib.o` with `NOCUDA_CFLAGS` + `-DINKLING_NO_MAIN` (CPU only).

### Rust (`feature = "ffi"`)

- `build.rs`: builds/links `libinkling.a`; `COLIBRI_INKLING_STATIC_LIB` override; rerun-if-changed on `inkling.c`.
- `ffi/bindings.rs`: `ColiInk*` types + externs.
- `ffi/multi.rs`: `FfiFamily::Inkling`, `FfiEngine::Inkling(InkEngine)`, `from_model_family(Inkling)`, open/generate/size path.
- `ffi/mod.rs`: `linked_families()` includes Inkling.

### Tests

- `linked_families_include_product_engines` asserts Inkling
- `from_model_family_maps_inkling`
- `open_inkling_missing_errors` (missing dir / no weights)
- `family_available_tracks_env` includes Inkling

Size path: C `coli_ink_engine_size` + Rust inspect overlay on open (same pattern as Kimi). No in-tree tiny Inkling weight fixture for full open+size; missing-dir and linked-family coverage is what shipped.

## Verify (ran)

```text
make -C c libinkling LTO=0          # ok → c/libinkling.a (~210KB); coli_ink_* exported; no main
cargo fmt -p colibri-sys
cargo clippy -p colibri-sys --all-targets --features ffi -- -D warnings   # ok
cargo test -p colibri-sys --lib                                         # 91 passed
cargo test -p colibri-sys --lib --features ffi                          # 102 passed
```

Clippy mop: derived `Default` for pre-existing `GlmOpenOptions` (`derivable_impls`).

## Residual (out of this slice)

- GPU/Metal/CUDA objects in static archives
- NPU
- Inkling **audio** embed generate
- Product/desktop defaulting to FFI
- Full-weight golden token parity for Inkling (no tiny fixture wired here)

## Key paths

- `/home/hunter/Projects/surmount/colibri/c/inkling.c`
- `/home/hunter/Projects/surmount/colibri/c/colibri_api.h`
- `/home/hunter/Projects/surmount/colibri/c/Makefile`
- `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/build.rs`
- `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/ffi/{mod,multi,bindings}.rs`
- `/home/hunter/Projects/surmount/colibri/crates/colibri-native/docs/fidelity.md`
