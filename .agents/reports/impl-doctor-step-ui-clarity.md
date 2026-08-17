# Implement report: Doctor step UI clarity

**Repo:** `/home/hunter/Projects/surmount/colibri`
**Date:** 2026-08-11
**Ask:** Doctor step looked broken (truncated recovery wall, duplicate essays) and buttons were unclear (random green/blue chips, no labels that say what they do).

## Summary

Compact missing-path Health check (4 short lines). Memory plan and registry status no longer repeat the same recovery essay. Wizard Doctor buttons use verb labels, a helper line, consistent min width, and hierarchy that flips when the path needs recovery (Scan / Install solid green; Run doctor hollow).

## Layout / copy

### Health check (`format_missing_model_directory`)

```
Overall: Needs model
Path: …
This folder is missing.
Default store: …
```

No env-var soup, no multi-paragraph Scan/Install essay, no sys doctor fail dump. Same compact pattern for not-a-model-folder (`format_not_a_model_folder`).

### Idle / empty path

Short three-line idle checklist (`Overall: Idle` + path none + Scan/Install hint).

### Memory plan (missing / unset path)

One line only: `No memory plan yet. …`
Health check owns recovery detail. Plan does not re-list store + Scan/Install.

### Registry status (empty store)

One line: `No models under {store} (folders with config.json, depth ≤2).`
No `COLIBRI_MODEL` / `COLIBRI_MODEL_STORE` paste in the status strip.

### Panel boxes

Doctor and plan bodies: `w_full` + `min_w_0`, `min_h` so short copy is not crushed, `max_h(200)` + `overflow_scroll` so longer checklists still scroll instead of clipping mid-word.

## Button clarity (wizard Readiness)

| Control | EN label | Style when path OK | Style when path missing / not a leaf | Action |
|---------|----------|--------------------|--------------------------------------|--------|
| Primary | **Run doctor** | solid green | hollow (panel + border) | deep doctor + recovery |
| Secondary | **Quick check** | hollow | hollow | shallow doctor + plan |
| Recovery | **Scan for models** | hollow | solid green | scan store + recovery doctor + plan |
| Recovery | **Install a model** | hollow | solid green | jump to Model install form |

Helper line under the row (muted cyan / label):

> Run doctor checks this path. Scan looks under the default store. Install downloads a model.

Buttons share `min_w(112)`, `BTN_PAD_*`, wrap row. i18n keys: `wizard.readiness.runDoctor`, `.refresh`, `.scan`, `.install`, `.actionsHint` (EN + IT).

Recovery behavior unchanged: deep/shallow still recover missing path, auto-pick one store model, list many, or show compact StillMissing.

## Files

| Path | Change |
|------|--------|
| `crates/colibri-native/src/host.rs` | Compact missing/idle/empty-scan/plan copy; `format_not_a_model_folder`; tests |
| `crates/colibri-native/src/main.rs` | Readiness button hierarchy, helper line, panel sizing |
| `crates/colibri-native/src/i18n.rs` | Labels + helper + IT + contract tests |
| `crates/colibri-native/docs/fidelity.md` | Doctor step + empty scan honesty |

## TDD

| Contract | Test |
|----------|------|
| Missing-path copy ≤5 non-empty lines, no env soup | `format_missing_model_directory_is_compact` |
| Leads with Needs model + missing line | `format_missing_model_directory_leads_with_recovery` |
| Shallow/deep missing path is recovery not dump | `run_shallow_doctor_missing_path_is_recovery_not_check_dump` |
| Plan missing path defers (short, no store essay) | `run_plan_missing_path_defers_to_health_check` |
| Empty registry status short, no COLIBRI_ | `format_empty_registry_scan_is_short` |
| Idle compact | `format_idle_doctor_checklist_is_idle_not_fail` |
| Run doctor / Scan for models / helper | `i18n::tests::wizard_and_tools_keys_en_it` |

## Verify

```text
cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets -- -D warnings   # ok
cargo test -p colibri-native                                    # 190 passed
```

## Operator check

1. Open wizard Doctor with a missing path (e.g. typed `~/.models`): Health check is four short lines; plan is one deferral line; no env soup under the box.
2. Buttons read **Run doctor**, **Quick check**, **Scan for models**, **Install a model**; helper line under the row.
3. Missing path: Scan + Install are solid green; Run doctor is outline. Path ok: Run doctor solid green; Scan/Install outline.
4. Scan still auto-sets path when the store has one model; Install still jumps to the download form.
