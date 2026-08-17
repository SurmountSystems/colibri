# Implement: thorough doctor accepts I64 (and st.h dtypes)

**Date:** 2026-08-11
**Bug:** thorough doctor fails on DeepSeek `tid2eid` with `unsupported dtype: "I64"`.
**Recon:** `.agents/reports/recon-doctor-i64-safetensors.md`

## Root cause

Deep layout (`tensor_layout` / `SAFETENSORS_DTYPES`) only allowed BF16, F16, F32, U8, I8. Runtime `c/st.h` already accepts I64/U64 (routing maps) and F8_* (native FP8 weights). Doctor was never updated after the engine load path.

## Fix

Expanded doctor dtype allowlist to match `st_dtype_code` / `st_dtype_esz` in `c/st.h`:

| dtype spellings | element size |
|-----------------|--------------|
| BF16, F16 | 2 |
| F32 | 4 |
| U8, I8 | 1 (span exemption unchanged) |
| F8_E4M3, F8_E4M3FN, float8_e4m3fn | 1 |
| F8_E8M0, F8_E8M0FNU | 1 |
| I64, U64 | 8 |

Unknown dtypes (e.g. F64) still hard-fail with `unsupported dtype`. Wrong byte spans still hard-fail. No demotion to warn.

## Files

| File | Change |
|------|--------|
| `crates/colibri-sys/src/doctor.rs` | Expanded `SAFETENSORS_DTYPES`; alignment comment; unit tests |
| `c/doctor.py` | Same table + comment (CLI parity) |
| `c/tests/test_doctor.py` | Parity unit test for `_tensor_layout` |

## TDD

### Red (before allowlist expand)

```text
cargo test -p colibri-sys --lib tensor_layout_
```

- `tensor_layout_accepts_i64_routing_table` → `unsupported dtype: "I64"`
- `tensor_layout_accepts_u64_and_f8_runtime_dtypes` → `unsupported dtype: "U64"`
- `tensor_layout_rejects_i64_span_mismatch` → failed only because I64 not allowed yet
- `tensor_layout_rejects_unknown_dtype` → already green (F64)

### Green (after fix)

```text
cargo fmt -p colibri-sys
cargo clippy -p colibri-sys --all-targets -- -D warnings
cargo test -p colibri-sys --lib doctor
```

- 24 doctor tests passed (including 4 new layout dtype tests)
- Python: `test_tensor_layout_accepts_i64_and_f8_runtime_dtypes` OK

## Contract covered

1. I64 shape×8 span OK → accept (routing table / tid2eid)
2. U64 + all st.h F8 spellings with 1-byte esz → accept
3. F64 / unknown → still `unsupported dtype` fail
4. I64 wrong span → still span-disagree fail

## Out of scope

No changes to `c/st.h` or DeepSeek load path (already correct). No I32/BOOL/F64 engine support.
