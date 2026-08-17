# Implement report: Native cold-start UX (doctor, scan, HF labels)

**Repo:** `/home/hunter/Projects/surmount/colibri`
**Plan:** `.agents/plans/plan-native-cold-start-ux.md`
**Date:** 2026-08-10

## Summary

Host-first cold-start polish for **colibri-native**: empty model doctor is **Idle** (no cwd / `"."` probe), HF install fields have always-visible EN+IT labels (min free no longer bare `1`), store scan is depth ≤2 with recovery copy, hero/plan/fidelity copy aligned.

## Red evidence (TDD)

| Contract | Test (fail shape before product edit) | Green |
|----------|--------------------------------------|-------|
| Empty doctor Idle, not Fail | `host::tests::format_idle_doctor_checklist_is_idle_not_fail`, `run_shallow_doctor_empty_path_is_idle`, `model_path_unset_for_doctor_empty_and_whitespace` | Pass |
| Empty scan message has store + depth + recovery | `host::tests::format_empty_registry_scan_has_store_rule_and_recovery` | Pass |
| Nested store models (depth 1 + 2) | `model::registry::tests::registry_scans_depth_one_and_two_under_store` | Pass |
| Install min-free label keys EN+IT | `i18n::tests::install_labels_en_and_it`, `en_table_has_core_surface` includes `install.minFree` | Pass |

Prior product behavior (documented in recon): empty path → `PathBuf::from(".")` → Overall Fail. That path is removed from `run_doctor` / `run_deep_doctor` / `bootstrap_panels`; idle branch lives in `run_doctor_checks`.

## Green verification

```
cargo fmt -p colibri-sys -p colibri-native
cargo test -p colibri-sys --lib          # 93 passed
cargo test -p colibri-native             # 86 passed
cargo clippy -p colibri-sys --all-targets -- -D warnings
cargo clippy -p colibri-native --all-targets -- -D warnings
```

All exit 0.

## Behavior delivered

### 1. Doctor empty path (host)

- `model_path_unset_for_doctor`: empty / whitespace only.
- Deliberate `.` still runs real sys doctor.
- `format_idle_doctor_checklist`: Overall **Idle**, Model (none selected), Depth (no model), `[info]` recovery line.
- `run_doctor_checks` returns idle without calling `run_doctor` when unset.
- UI `run_doctor` / `run_deep_doctor` / `bootstrap_panels` no longer rewrite empty → `"."`.

### 2. HF install labels

- Always-visible captions above repo / revision / dest / min free (same pattern as Temperature).
- i18n keys: `install.repo`, `install.revision`, `install.dest`, `install.minFree`, `install.minFreeHelp` (EN + IT).
- Default min free remains `1` (`DEFAULT_INSTALL_MIN_FREE_BYTES`); help notes `0` turns gate off.
- Label stays visible when field is `1` (placeholder alone no longer the only hint).

### 3–4. Scan Tier A + B

- `format_empty_registry_scan` / `format_registry_scan_status` for status text.
- `ModelRegistry::refresh`: depth ≤2 (`REGISTRY_SCAN_MAX_DEPTH = 2`), cap 64 entries (`REGISTRY_SCAN_MAX_ENTRIES`), require `config.json`, no recurse into model leaves, dedupe by path.
- Depth-3 layouts still ignored (test asserts).

### 5. Copy

- Hero: machine works without model; Doctor idle until path set (EN + IT).
- Plan empty: “Set a model path first, then run Plan.”
- `run_plan` empty string slightly aligned with set-path-first.
- `docs/fidelity.md` rows for doctor / install / registry scan updated.

## Files touched

| Path | Change |
|------|--------|
| `crates/colibri-native/src/host.rs` | Idle doctor helpers, empty-scan formatters, plan empty copy, tests |
| `crates/colibri-native/src/main.rs` | Drop `.` fallback; scan status; HF labeled form |
| `crates/colibri-native/src/i18n.rs` | install.* keys EN+IT; hero description; tests |
| `crates/colibri-sys/src/model/registry.rs` | Bounded depth-2 walk + tests |
| `crates/colibri-native/docs/fidelity.md` | Honesty for idle doctor / scan depth / min-free label |

## Non-goals (still out)

- Tauri/React HF install parity
- HF hub cache scan
- Extra scan roots list
- Changing CLI `coli doctor` empty-path semantics

## Residual

None for this plan slice. Operators with models **outside** the store still need path paste / env / move-or-symlink (Tier A copy). Depth 3+ under store still not listed (by design, cap depth 2).
