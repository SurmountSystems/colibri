# Recon: thorough doctor fails on I64 safetensors (tid2eid)

**Date:** 2026-08-11
**Symptom:** Deep/thorough doctor on DeepSeek-V4-Flash-0731 fails with:

```text
model-00002-of-00048.safetensors: tensor 'layers.0.ffn.gate.tid2eid': unsupported dtype: "I64"
```

(Operator may also see `model.layers.…` naming depending on checkpoint layout; same dtype.)

**Scope:** read-only recon. Product-correct fix recommendation + test contract.

---

## 1. Where unsupported dtype is enforced

### Primary (native doctor path)

| Layer | Path | Role |
|-------|------|------|
| Allowed table | [`crates/colibri-sys/src/doctor.rs`](../../crates/colibri-sys/src/doctor.rs) `SAFETENSORS_DTYPES` (≈L35–37) | dtype → element size |
| Lookup | same file, `dtype_size` (≈L474–479) | `None` → reject |
| Layout check | same file, `tensor_layout` (≈L515–566) | `unsupported dtype: {dtype:?}` |
| Deep scan | same file, `deep_container_report` loop (≈L727–750) | every non-`__metadata__` tensor → `tensor_layout`; error wrapped as `{shard}: tensor '{name}': {err}` |
| Check id | `model.container` fail (≈L974 / L1030) | thorough doctor surfaces this |

Fail path (simplified):

1. `run_doctor(..., deep: true)` → `deep_container_report`
2. For each `*.safetensors` shard: parse header only (no payload hash)
3. For each tensor: `tensor_layout` → `dtype_size` → **hard Err** if dtype missing from table
4. That Err becomes `model.container` **fail** (not warn)

### Python parity (same contract)

| Path | Role |
|------|------|
| [`c/doctor.py`](../../c/doctor.py) `SAFETENSORS_DTYPES` (L17–23) | same five dtypes |
| `_tensor_layout` (L71–96) | `raise ValueError(f"unsupported dtype: {dtype!r}")` |
| `deep_container_report` (≈L177+) | same per-tensor layout walk |

Rust port comment explicitly says it mirrors `doctor.SAFETENSORS_DTYPES` / `_tensor_layout`.

### Not the runtime load path

The **engine** safetensors reader already accepts I64. That is separate from doctor:

| Path | Behavior |
|------|----------|
| [`c/st.h`](../../c/st.h) `st_dtype_code` (≈L82–99) | accepts `I64`/`U64` → code 6; also F8 variants |
| `st_dtype_esz` (≈L104–110) | I64/U64 → **8** bytes |
| Comment in `st.h` | Explicit: before this, `st_init` **exit(1)** on first I64 on DeepSeek-V4 checkpoints; index tensors are raw-byte path, float readers reject by name |

So: **load path was fixed for DeepSeek-V4; doctor dtype allowlist was left narrow.**

---

## 2. Allowed dtypes list (doctor today)

**Doctor (`SAFETENSORS_DTYPES`) — both Rust and Python:**

| dtype | element size (bytes) |
|-------|----------------------|
| `BF16` | 2 |
| `F16` | 2 |
| `F32` | 4 |
| `U8` | 1 |
| `I8` | 1 |

**Special case in layout:** for `U8`/`I8` only, shape×elem_size vs `data_offsets` span is **not** strictly checked (packed quant / opaque blobs). All other listed dtypes require exact span match.

**Runtime (`st.h`) — already loadable:**

| dtype spellings | code | esz |
|-----------------|------|-----|
| BF16, F16, F32, U8, I8 | 0–3 | 2/2/4/1 |
| `F8_E4M3`, `F8_E4M3FN`, `float8_e4m3fn` | 4 | 1 |
| `F8_E8M0`, `F8_E8M0FNU` | 5 | 1 |
| `I64`, `U64` | 6 | 8 |

Pinned by [`c/tests/test_ue8m0.c`](../../c/tests/test_ue8m0.c) (dtype codes + sizes).

---

## 3. Is I64 (and I32/…) valid for integer index tensors?

**Yes for I64 on DeepSeek-V4.** This is not a corrupt weight; it is the product plan.

Evidence:

- [`c/deepseek_v4.c`](../../c/deepseek_v4.c) (≈L422–424): when `uses_hash_router`, layer plan adds
  `ffn.gate.tid2eid` as **`COLI_ST_I64`**, shape `[vocab_size, num_experts_per_tok]`
  (else F32 gate bias).
- [`c/deepseek_v4_internal.h`](../../c/deepseek_v4_internal.h): `#define COLI_ST_I64 6`
- Runtime uses the table as `const int64_t *` (`value(..., "ffn.gate.tid2eid", ...)` ≈L3114 / L3602).
- Tiny model tooling writes torch int64 as safetensors `"I64"` ([`c/tools/make_deepseek_v4_tiny.py`](../../c/tools/make_deepseek_v4_tiny.py)).
- Unit plan test: [`c/tests/test_deepseek_v4.c`](../../c/tests/test_deepseek_v4.c) expects tid2eid dtype `COLI_ST_I64`.

**Doctor’s job here** is header/layout integrity (offsets, non-overlap, shape×esz), **not** “every tensor must be a float weight.” Rejecting I64 is a **false fail** for a shippable checkpoint the runtime already loads.

**I32 / U32 / BOOL / F64:** not currently in `st_dtype_code`; engine would still `exit(1)` at load. Doctor does not need to accept every HF safetensors dtype unless product decides doctor is “generic layout only.” For this bug, **align doctor with `st.h`**, not invent I32 load support.

**FP8 dtypes:** DeepSeek-V4 native checkpoints also carry `F8_E4M3*` / `F8_E8M0*`. If doctor only adds I64, thorough scan will likely **fail next** on the first FP8 weight tensor. Fix should include the full runtime-supported set, not only I64.

---

## 4. Fix options

| Option | Pros | Cons | Verdict |
|--------|------|------|---------|
| **A. Expand doctor allowlist to match `st.h`** (I64/U64 + F8 spellings; esz correct) | Smallest true fix; keeps span checks; parity with engine; no skip holes | Must dual-update Python + Rust tables | **Recommended** |
| B. Skip non-weight / unknown dtypes | Unblocks scan | Loses overlap/span checks on real tensors; name heuristics fragile (`tid2eid` only is too narrow) | Reject as primary |
| C. Warn-not-fail on unknown dtype | Softens UX | Still mislabels valid V4 models as “dirty”; weak contract | Reject for known runtime dtypes |
| D. Only allow I64, ignore F8 | Fixes quoted error only | Immediate next fail on same model family | Incomplete |

**Do not** treat I64 as “unsupported weights” and fail. **Do not** skip only integer tensors without size knowledge (byte-span validation needs esz).

### Product-correct fix (A detail)

1. **Rust** `SAFETENSORS_DTYPES` in `crates/colibri-sys/src/doctor.rs` → add at least:

   ```text
   F8_E4M3, F8_E4M3FN, float8_e4m3fn  → 1
   F8_E8M0, F8_E8M0FNU              → 1
   I64, U64                         → 8
   ```

2. **Python** `c/doctor.py` `SAFETENSORS_DTYPES` → same keys/sizes (parity for CLI/`test_doctor.py`).

3. Keep strict span check for these dtypes (same as F32/BF16). Leave U8/I8 span exemption as today (packed quant).

4. Optional comment: “must stay aligned with `st_dtype_code` / `st_dtype_esz` in `c/st.h`” so the tables do not drift again.

5. **Out of scope for this bug:** teaching doctor I32 or full HF dtype enum; changing engine to load new integer types.

---

## 5. Test contract (red → green)

### Contract (plain language)

1. Thorough doctor **accepts** a shard tensor with dtype `I64` when shape × 8 equals `data_offsets` span (e.g. name `layers.0.ffn.gate.tid2eid` or `…ffn.gate.tid2eid`).
2. Thorough doctor **accepts** runtime FP8 dtypes used by V4 (at least one of `F8_E4M3` / `float8_e4m3fn` and `F8_E8M0`) with 1-byte esz span check.
3. Thorough doctor still **fails** on a truly unknown dtype (e.g. `"F64"` or `"BOGUS"`) with a message containing `unsupported dtype`.
4. I64 with **wrong** byte span still fails (`shape and dtype disagree with the tensor byte span`).
5. Existing deep tests (overlap, core tensors, shard sequence) remain green.

### Suggested Rust tests (`colibri-sys` `doctor.rs` tests)

- **`tensor_layout_accepts_i64_routing_table`**
  meta: `dtype: "I64"`, `shape: [4, 2]`, `data_offsets: [0, 64]` → `Ok((0, 64))`.

- **`tensor_layout_rejects_unknown_dtype`**
  `dtype: "F64"` (or `"BOGUS"`) → Err contains `unsupported dtype`.

- **`tensor_layout_rejects_i64_span_mismatch`**
  shape product × 8 ≠ end−start → span error.

- **`deep_accepts_i64_and_f8_in_container`** (optional but strong)
  write a mini shard with U8 core tensors + one I64 + one F8_E4M3 layout-valid entry; `model.container` **pass**.

### Suggested Python parity

- Extend [`c/tests/test_doctor.py`](../../c/tests/test_doctor.py) with one deep fixture that includes I64 (and optionally F8) so CLI doctor stays aligned.

### Observed red (expected before fix)

- `tensor_layout` / deep container on I64-only meta → `unsupported dtype: "I64"` (matches operator paste).

### Green

- Same tests pass after allowlist expansion; no change to runtime load code required for this doctor bug.

---

## 6. Files to touch (implementer)

| File | Change |
|------|--------|
| `crates/colibri-sys/src/doctor.rs` | Expand `SAFETENSORS_DTYPES`; unit tests |
| `c/doctor.py` | Expand `SAFETENSORS_DTYPES` (parity) |
| `c/tests/test_doctor.py` | Optional deep I64 acceptance test |

**Do not need for this bug:** `c/st.h`, `deepseek_v4.c` (already correct).

---

## 7. Root cause one-liner

Deep doctor validates **every** tensor against a **float/quant-only** dtype allowlist (BF16/F16/F32/U8/I8). DeepSeek-V4 stores expert routing maps as **I64** (`tid2eid`) and weights as **F8_***; the engine already accepts those dtypes in `st.h`, but doctor was never updated, so thorough scan false-fails on a valid checkpoint.
