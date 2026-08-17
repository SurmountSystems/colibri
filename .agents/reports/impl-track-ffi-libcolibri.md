# Track FFI: multi-family in-process static engines + size API

**Date:** 2026-08-10
**Scope:** residual `open:ffi-phase-d` (V4 first wave + multi-family + size metadata)
**Status:** Multi-family CPU static + Rust `ffi` + kill-switch + size metadata
**landed**. Process serve mux remains **product default**. Residual stays
**partial** (Inkling, GPU, golden parity, product-default FFI).

---

## Honesty

| Claim | Truth |
|-------|--------|
| Product default inference path | **Process** + serve mux (`EngineHandle` / `ServeClient`) |
| `feature = "ffi"` | Opt-in link of `libcolibri.a` + `libkimi_k3.a` + `libdeepseek_v4.a` |
| Family select on Rust API | `FfiFamily` + `open_engine` / `FfiEngine` |
| Model size on public types | `ModelInfo.disk_bytes`, `ModelSizeInfo`, plan overlay, `FfiEngine::size_info` |
| Inkling in-process | **Not** extracted this wave |
| GPU / NPU in static lib | **Not** done |
| Golden generate vs process on full weights | **Not** run (tiny fixtures + open/kill-switch only) |
| Desktop auto-uses FFI | **No**; hosts must opt in with `prefer_process = false` and feature |

---

## Wave 1 (prior) — DeepSeek V4 CPU

### A1 — Static lib without CLI main

- `c/Makefile.deepseek-v4`: target `libdeepseek-v4` → `libdeepseek_v4.a`
- Separate `*.lib.o` objects with `-fPIC -DCOLI_V4_SKIP_GENERATE_MAIN`
- Default `LIB_LTO=0` (easier for rustc consume)
- CLI `deepseek_v4` binary path unchanged

### A2–A4 — Rust link, availability, kill-switch

| File | Role |
|------|------|
| `crates/colibri-sys/build.rs` | make/link static libs under `feature = "ffi"` |
| `crates/colibri-sys/src/ffi/{mod,bindings,v4}.rs` | V4 wrappers + availability |
| `src/config.rs` | `prefer_process` default true, `COLIBRI_FORCE_PROCESS` |

### A5 — Tests + docs (V4)

V4 open/kill-switch/link tests green; process default documented.

---

## Wave 2 (this revision) — Multi-family + size metadata

### Multi-family C static libs

| Family | Flag | Make target | Archive | Public C API |
|--------|------|-------------|---------|--------------|
| GLM | `COLIBRI_NO_MAIN` | `make -C c libcolibri` | `libcolibri.a` | `coli_glm_*` in `colibri_api.h` |
| Kimi K3 | `KIMI_NO_MAIN` | `make -C c libkimi_k3` | `libkimi_k3.a` | `coli_kimi_*` |
| DeepSeek V4 | `COLI_V4_SKIP_GENERATE_MAIN` | `make -f Makefile.deepseek-v4 libdeepseek-v4` | `libdeepseek_v4.a` | `coli_v4_*` in `deepseek_v4.h` |
| Shared size probe | — | linked into libs | — | `ColiModelSizeSummary`, `coli_model_size_probe` |

Symbols checked: no `main` in archives; open/generate/destroy/size present for GLM/Kimi; V4 engine/session symbols present.

### Rust multi-family surface

| API | Role |
|-----|------|
| `FfiFamily::{Glm,Kimi,DeepseekV4}` | Family enum for open |
| `FfiEngine` | Opened engine enum |
| `open_engine(family, model_dir)` | Unified open |
| `ffi_family_available` / `linked_families` | Per-family availability |
| `GlmEngine` / `KimiEngine` / `V4Engine` | Family wrappers + `size_info` / generate |

`build.rs` builds or links all three archives (env overrides:
`COLIBRI_V4_STATIC_LIB`, `COLIBRI_GLM_STATIC_LIB`, `COLIBRI_KIMI_STATIC_LIB`).

### Model size metadata (mandatory)

| Type | Fields (raw where required) |
|------|------------------------------|
| `ModelInfo` | `disk_bytes`, `model_bytes` (same), `engine_id`, `family`, optional `param_count` |
| `ModelSizeInfo` | `disk_bytes`, `family`, `engine_id`, optional `param_count`, optional `tier_vram_bytes` / `tier_ram_bytes` / `tier_disk_bytes` when plan known |
| Plan / registry / install summaries | Size fields wired where model inspect applies |
| C `ColiModelSizeSummary` | `disk_bytes` + family/engine strings + optional param/dense/expert |
| FFI open | Prefer Rust `ModelInfo::inspect`; overlay C size / V4 memory summary |

Unit tests: tiny fixtures (e.g. `glm_tiny_open_has_disk_bytes`), plan size overlay, `param_count_from_config`.

### Kill-switch (unchanged contract)

- Env `COLIBRI_FORCE_PROCESS` → open/generate refuse; `ffi_available()` false
- `ColibriConfig::prefer_process` default **true**
- Process mux (`EngineHandle`) still always subprocess

---

## Verify (all green this wave)

```bash
cargo fmt -p colibri-sys
cargo clippy -p colibri-sys --all-targets -- -D warnings
cargo clippy -p colibri-sys --all-targets --features ffi -- -D warnings
cargo test -p colibri-sys --lib
# 91 passed
cargo test -p colibri-sys --lib --features ffi
# 99 passed
```

Make smoke (CPU Linux):

```bash
cd c
make libcolibri LTO=0
make libkimi_k3 LTO=0
make -f Makefile.deepseek-v4 libdeepseek-v4 LTO=0
```

---

## Docs + residual honesty

| Doc | Update |
|-----|--------|
| `crates/colibri-sys/docs/ffi-phase-d.md` | Multi-family + size + kill-switch; residual gaps listed |
| `crates/colibri-sys/docs/user-guide.md` | Feature table, residual, Grok harness notes |
| `crates/colibri-sys/README.md` | Is/not + residual row |
| `crates/colibri-sys/src/lib.rs` | Feature table comment |
| `.agents/RESIDUAL.md` | `open:ffi-phase-d` **partial** (multi-family + size landed; product-default / Inkling / GPU / golden still open) |

**Do not fully close `open:ffi-phase-d`:** product-default in-process is not true;
Inkling not extracted; GPU/golden/desktop default remain.

---

## Files touched (multi-family wave, non-exhaustive)

### C

- `c/colibri_api.h`, `c/coli_model_size.c`
- `c/colibri.c` (`COLIBRI_NO_MAIN`, `coli_glm_*`)
- `c/kimi_k3.c` (`KIMI_NO_MAIN`, `coli_kimi_*`)
- `c/Makefile` (`libcolibri`, `libkimi_k3`)
- `c/Makefile.deepseek-v4` (prior `libdeepseek-v4`)

### Rust (colibri-sys)

- `build.rs` (three libs)
- `src/ffi/{mod,bindings,v4,multi}.rs`
- `src/model/mod.rs` (+ registry/install/plan size fields)
- `src/config.rs`, `src/lib.rs`

### Docs / residual

- `docs/ffi-phase-d.md`, user-guide, crate README
- `.agents/RESIDUAL.md`
- This report

---

## Residual still open under `open:ffi-phase-d`

1. Inkling no-main extract and FFI family
2. GPU objects in static link matrix
3. Golden token/logit parity vs process on real production weights
4. Host auto-fallback wiring (open fail → process) in desktop
5. Visual poll / multi-slot / concurrent cancel ABI
6. Product defaulting to FFI (`prefer_process` remains true)
7. NPU (separate residual `open:npu-inference`)

---

## Architecture (unchanged default)

```
GPUI / host app
  └── colibri-sys (in-process host)
        ├── EngineHandle ──spawn──► C engine process (default)
        └── feature ffi ──link──► coli_glm_* / coli_kimi_* / coli_v4_* (CPU opt-in)
```
