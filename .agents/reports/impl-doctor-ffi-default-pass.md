# Implement report: Doctor FFI default + clearer fail copy

**Status:** done
**Date:** 2026-08-11
**Scope:** default-enable `ffi` on `colibri-native`; clearer process-only doctor fail copy; TDD.

## What landed

### A. Default-enable FFI on colibri-native

- `crates/colibri-native/Cargo.toml`: `default = ["install", "ffi"]` (was `["install"]`).
- Feature map already correct: `ffi = ["colibri-sys/ffi"]`.
- Cargo comment: process-only via `--no-default-features --features install` or runtime `COLIBRI_FORCE_PROCESS=1`.
- `crates/colibri-native/README.md`:
  - Default features note: install + ffi; process-only and C static libs pointers.
  - Features table: `ffi` Default = **yes**.
  - Build section: default build needs C static engines; process-only flag shown; clippy without forcing install-only.

### B. Doctor fail copy when process-only

In `crates/colibri-sys/src/doctor.rs`:

- **Summary** (fail, process missing + no in-process):
  `Model files look ready; process engine binary not found ({path}).`
- **Details**: JSON `{ "path": "...", "hint": "..." }` where hint is plain operational English:
  set `COLIBRI_ENGINE` / `COLI_ENGINE`, build with `make -C c <engine>`, or rebuild native with the `ffi` feature (default: install + ffi).
- Pass path when `in_process_available` unchanged: `"in-process engine is available"`.
- Never says "not built" / "engine is not built".

#### All supported model families (not DeepSeek-only)

Copy and path handling are **family-agnostic by construction**:

- `engine_binary_check` only formats the resolved `Path` into summary/details.
- `resolve_doctor_engine_path` uses override → `locate_engine` → `ModelFamily::engine_basename()` for any family.
- Summary/hint never name Glm, Inkling, Kimi, DeepseekV4, or Olmoe.

| ModelFamily | `engine_basename()` |
|-------------|---------------------|
| Glm | `colibri` |
| Olmoe | `colibri` (same binary as Glm) |
| Inkling | `inkling` |
| Kimi | `kimi_k3` |
| DeepseekV4 | `deepseek_v4` |

Operator follow-up (2026-08-11): do **not** scope fail copy or tests to DeepSeek V4 only. Table-driven tests cover **every** family above.

### C. Tests

| Test | Contract |
|------|----------|
| `engine_missing_without_ffi_says_model_ready_binary_not_found` | fail; summary has model-ready + binary not found + path; hint has env + make/ffi |
| `engine_missing_process_only_fail_summary_all_family_basenames` | **all 5 families**: fail shape + basename in path + family-neutral hint |
| `engine_missing_with_ffi_passes_in_process` | pass; in-process wording (kept) |
| `engine_missing_with_ffi_passes_all_family_basenames` | pass for each **distinct** basename (4: colibri, inkling, kimi_k3, deepseek_v4) |
| `doctor_engine_missing_no_ffi_message_contract` | end-to-end run_doctor with `in_process_engine: Some(false)` → new summary + hint |
| `doctor_engine_missing_with_in_process_passes` | pass when injected true (kept) |
| `doctor_ffi_feature_defaults_to_in_process_when_binary_missing` | cfg(feature=ffi) (kept; not in default colibri-sys filter without ffi) |

## Verify

```
cargo fmt -p colibri-sys -p colibri-native
cargo clippy -p colibri-sys -p colibri-native --all-targets -- -D warnings   # exit 0
cargo test -p colibri-sys --lib doctor   # 19 passed (incl. all-family tables)
cargo test -p colibri-native             # 248 passed (bin tests; no --lib target)
```

Default `ffi` on native **linked and tested successfully** in this environment (colibri-sys build.rs / existing C static artifacts). No process-only fallback needed for green.

### Follow-up verify (all families)

```
cargo fmt -p colibri-sys -p colibri-native
cargo test -p colibri-sys --lib doctor   # 19 passed
cargo clippy -p colibri-sys -p colibri-native --all-targets -- -D warnings   # exit 0
```

## Files touched

| Path | Change |
|------|--------|
| `crates/colibri-native/Cargo.toml` | default features include `ffi` |
| `crates/colibri-native/README.md` | default build / features table / build notes |
| `crates/colibri-sys/src/doctor.rs` | fail summary + details.hint; tests |

## Product copy note

Native-only / sys operational English (not SPA i18n). Source: plan + AGENTS product copy fidelity (plain operational, no marketing slogans).

## Residual

None for this plan. Optional later: surface `details.hint` more prominently in native checklist formatting if UI only shows summary today.
