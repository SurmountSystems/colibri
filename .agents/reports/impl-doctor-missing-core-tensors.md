# impl: doctor missing core tensors (DeepSeek-V4)

## Problem

Thorough doctor on a complete DeepSeek-V4-Flash-0731 tree reported:

`Overall: Fail · 3 required core tensor(s) are missing`

while model scan, path, config, and tokenizer all passed.

## Root cause

`model.required` in deep doctor used a single GLM/HF core list:

| Required (old) | Present in V4 Flash? |
|----------------|----------------------|
| `model.embed_tokens.weight` | no |
| `model.norm.weight` | no |
| `lm_head.weight` | no |

Those three are the exact fail set. DeepSeek-V4 checkpoints (and `c/deepseek_v4.c`) use short names:

| V4 engine / checkpoint | Loaded in C as |
|------------------------|----------------|
| `embed.weight` | `coli_st_find(..., "embed.weight")` |
| `norm.weight` | `coli_tensor_load_f32(..., "norm.weight", ...)` |
| `head.weight` | `coli_st_find(..., "head.weight")` |

Confirmed on live store
`/home/hunter/.local/share/colibri/models/DeepSeek-V4-Flash-0731`
(`model_type: deepseek_v4`, all three V4 names present, GLM names absent).
Tiny fixture writer `c/tools/make_deepseek_v4_tiny.py` renames HF → V4 the same way.

## Fix

Family-aware required core tensors (config `model_type` via `model_arch`):

| Family | Required names |
|--------|----------------|
| DeepseekV4 | `embed.weight`, `norm.weight`, `head.weight` |
| Glm, Inkling, Kimi, Olmoe | `model.embed_tokens.weight`, `model.norm.weight`, `lm_head.weight` |

Incomplete installs still fail (every name in the family list must be present).
GLM names on a DeepSeek-V4 tree do **not** count as substitutes.

Fail summary now names the missing tensors, e.g.:

`2 required core tensor(s) are missing: norm.weight, head.weight`

Details include `family`, `required_names`, and `missing_tensors`.

### Files

- `crates/colibri-sys/src/doctor.rs` — family lists, `required_core_tensors`, deep check, summary
- `c/doctor.py` — same family rule + summary (parity with the Rust port)

## TDD

Red intent (observed before green on the old constant):

- Synthetic DeepSeek-shaped tensor set + `model_type: deepseek_v4` would fail `model.required` under the GLM list.

Green evidence:

| Test | Contract |
|------|----------|
| `required_core_tensors_deepseek_v4_uses_engine_names` | V4 list is engine names; other families keep GLM |
| `deep_accepts_deepseek_v4_core_tensor_names` | complete V4 synthetic shard → `model.required` pass |
| `deep_rejects_incomplete_deepseek_v4_core_tensors` | only `embed.weight` → fail naming `norm`/`head` |
| `deep_rejects_glm_names_when_family_is_deepseek_v4` | GLM-only cores on V4 family still fail |
| `deep_rejects_missing_core_tensor` | GLM incomplete still fails; summary names missing |

Live smoke (Python deep path on operator model):

```
required ('embed.weight', 'norm.weight', 'head.weight')
required status pass
missing_tensors []
```

## Verify commands

```bash
cargo fmt -p colibri-sys
cargo clippy -p colibri-sys --all-targets -- -D warnings
cargo test -p colibri-sys --lib doctor
```

All 28 doctor lib tests passed. Clippy clean. fmt applied.

## Not changed

- Native host display: only formats doctor check summaries; no native change needed.
- No weakening of incomplete-install detection.
