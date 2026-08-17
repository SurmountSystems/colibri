# Implement report: Doctor cold-start recovery (make it work)

**Repo:** `/home/hunter/Projects/surmount/colibri`
**Date:** 2026-08-11
**Ask:** Doctor step still fails on `~/.models` (directory does not exist). Make product recovery work.

## Summary

Host-first recovery for missing / non-model paths. Cold start fills the rail with the **product default store** (or a scanned model under it), not a random empty `~/.models`. Doctor leads with plain recovery copy and CTAs (**Scan models**, **Install model**) instead of a long fail dump. Doctor / Quick check scan the default store and auto-set the path when exactly one usable model is found.

## Behavior

### 1. Default model path on cold start

`resolve_startup_model_path` order:

1. `COLIBRI_MODEL` / `COLI_MODEL` (env)
2. Prefs `last_model_path` when that path **exists**
3. If prefs path is missing and store has **one** usable model → auto-pick
4. Empty prefs + one store model → that model
5. Empty prefs + many → first usable model (status notes count)
6. Empty prefs + no models → **default store path** (`default_model_store_path`, e.g. `~/.local/share/colibri/models`)

Startup also scans the store once and seeds the registry picker when non-empty.

### 2. Missing path → recovery checklist (not 5 fails)

When the expanded path does not exist, `run_doctor_checks` returns `format_missing_model_directory` **without** calling sys doctor:

- Overall: Fail
- One plain line: **This folder does not exist.**
- Default model store path
- Scan models / Install model recovery
- Note about custom paths like `~/.models`

Plan text for a missing path also names the default store and Scan / Install.

### 3. Doctor / Quick check scan-when-missing

`run_doctor_with_recovery` (used by shallow, deep, wizard leave-Model, readiness Scan):

- Empty path → idle checklist
- Path is a model leaf (`config.json` present) → normal doctor
- Otherwise scan default store depth ≤2:
  - **One** usable model → set path, re-run doctor, status note
  - **Many** → list in doctor text + registry rows; pick a row to set path and re-doctor
  - **None** → recovery checklist

`scan_registry` also auto-sets the path when the current field is empty/missing/not a leaf and exactly one model is found.

### 4. Wizard Doctor step buttons

| Control | Action |
|---------|--------|
| **Doctor** (primary) | Deep doctor + recovery |
| **Quick check** | Shallow doctor + plan (+ recovery) |
| **Scan models** | Registry scan + recovery doctor + plan |
| **Install model** (feature `install`) | Jump to Model step with download form open |

Registry status + clickable model rows also show on the Doctor step when a scan has filled them.

### 5. Intentionally not done

- Silent `mkdir` of a wrong path (e.g. `~/.models`)
- Renaming Doctor
- Changing sys `coli doctor` CLI empty-path semantics beyond host policy
- Creating the store directory automatically

## Files

| Path | Change |
|------|--------|
| `crates/colibri-native/src/host.rs` | Missing-path formatters, startup resolve, scan outcome helpers, tests |
| `crates/colibri-native/src/main.rs` | Startup scan + store default; `run_doctor_with_recovery`; readiness Scan/Install + registry rows |
| `crates/colibri-native/docs/fidelity.md` | Doctor / path / wizard honesty |

## TDD (red → green contracts)

| Contract | Test |
|----------|------|
| Missing path leads with recovery, not check dump | `format_missing_model_directory_leads_with_recovery`, `run_shallow_doctor_missing_path_is_recovery_not_check_dump` |
| No "not built" on missing path | `doctor_missing_process_never_says_not_built` (updated) |
| Empty prefs → default store display | `resolve_startup_model_path_empty_prefs_uses_store` |
| Env wins; missing prefs auto-picks one | `resolve_startup_model_path_env_wins`, `…_missing_prefs_auto_picks_single` |
| Scan pick one / many / plan store hint | `pick_single_usable_model_none_when_many`, `missing_path_scan_*`, `run_plan_missing_path_suggests_store` |

## Verify

```text
cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets -- -D warnings   # ok
cargo test -p colibri-native                                    # 189 passed
```

## Operator check

1. Cold start with empty prefs: rail shows `…/colibri/models` (or one scanned model), not `~/.models` unless you typed it.
2. Type `~/.models` → Doctor / Quick check: short recovery + default store; Scan may auto-set if one model exists under the store.
3. Doctor step has **Doctor**, **Quick check**, **Scan models**, **Install model**.
