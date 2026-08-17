# D2: tiny GLM process↔FFI token parity

**Date:** 2026-08-10
**Crate:** `colibri-sys` (`feature = "ffi"`)
**Status:** Green — process free-generate and FFI `generate_ids` match on `glm_tiny`

---

## Goal

At least one automated test that proves **process generate** and **FFI generate** produce the **same token sequence** on tiny GLM (greedy / temperature 0).

## Fixtures

| Path | Role |
|------|------|
| `c/glm_tiny/` | ~2.4 MB random-weight GLM-5.2 tiny (`model.safetensors` + config) |
| `c/ref_glm.json` | Oracle: `prompt_ids`, `full_ids`, `tf_pred` |
| `c/colibri` | Process engine binary (CLI free-generate) |

No multi-GB weights. Text generate is not used (tiny has no `tokenizer.json`); both sides use the same **prompt token ids** from the oracle.

## What landed

### C (`COLIBRI_NO_MAIN`)

- `coli_glm_generate_ids` in `c/colibri_api.h` + `c/colibri.c`
- Greedy free-generate from raw prompt ids (mirrors CLI `generate()`, no tokenizer, eos = -1)
- Forces `g_temp = 0` for the call duration

### Rust (`feature = "ffi"`)

| API | Role |
|-----|------|
| `GlmOpenOptions` / `oracle_parity()` | Open with `cap=64`, `expert_bits=16`, `dense_bits=16` (CLI `./colibri 64 16 16`) |
| `open_glm(path, options)` | GLM open with explicit bits |
| `GlmEngine::generate_ids` / `FfiEngine::generate_ids` | Collect greedy continuation tokens |

Default `open_engine(Glm, …)` still uses C defaults (4/8 bits). **Parity requires matching load bits** — default 4/8 diverges from the oracle after ~9 tokens.

### Test

**Name:** `ffi::multi::tests::glm_tiny_process_ffi_token_parity`

1. Load `ref_glm.json` `prompt_ids` / expected `full_ids[np..]`
2. FFI: `open_glm(…, oracle_parity())` → `generate_ids` → token vec
3. Assert FFI == oracle continuation
4. Process: spawn `c/colibri 64 16 16` with `SNAP=glm_tiny`, `REF=ref_glm.json`, `COLI_TEMP=0`, `COLI_NO_OMP_TUNE=1` (no `PROMPT`)
5. Parse `GLM C engine : …` token line
6. Assert process == FFI == oracle

Skip policy (not `#[ignore]`): missing weights/ref → early return. Missing process binary → skip process half unless `COLIBRI_REQUIRE_PROCESS_PARITY=1`.

---

## Red → green evidence

### Red (before matching open bits)

FFI opened with default `expert_bits=4`, `dense_bits=8` while process/oracle use 16/16:

```
assertion failed: FFI generate_ids must match ref_glm.json full_ids continuation (greedy)
  left:  [207, 187, 119, 103, 103, 103, 103, 103, 119, 249, …]
  right: [207, 187, 119, 103, 103, 103, 103, 103, 119, 34, …]
```

### Green

```bash
cargo test -p colibri-sys --lib --features ffi glm_tiny_process_ffi_token_parity -- --nocapture
# ok (process + FFI both match 20-token continuation)

cargo test -p colibri-sys --lib --features ffi
# 102 passed; 0 failed

cargo clippy -p colibri-sys --all-targets --features ffi -- -D warnings
# clean
```

Oracle continuation (20 tokens after 12 prompt ids):

```
207 187 119 103 103 103 103 103 119 34 103 103 103 103 103 136 112 7 119 34
```

Manual process check (pre-test): `SNAP=./glm_tiny COLI_NO_OMP_TUNE=1 COLI_TEMP=0 ./colibri 64 16 16` → **Matching tokens: 20/20**.

---

## Files touched

| File | Change |
|------|--------|
| `c/colibri_api.h` | Declare `coli_glm_generate_ids` |
| `c/colibri.c` | Implement greedy generate-from-ids |
| `crates/colibri-sys/src/ffi/bindings.rs` | Bind `coli_glm_generate_ids` |
| `crates/colibri-sys/src/ffi/multi.rs` | `GlmOpenOptions`, `open_glm`, `generate_ids`, parity test |
| `crates/colibri-sys/src/ffi/mod.rs` | Re-export `GlmOpenOptions`, `open_glm` |

---

## Notes / residual

- Process half uses the **CLI free-generate** path (token id printout), not serve-mux `EngineHandle` (mux emits UTF-8 text, not raw ids). Same C engine binary and same greedy decode as serve would use under temp 0.
- Full-weight golden parity remains out of scope (`open:ffi-phase-d` residual).
- Inkling FFI open path may be landing in parallel; this slice does not own it.
- Rebuild static lib after C API change: `make -C c libcolibri LTO=0` (also done by `build.rs` under `feature = "ffi"`).
