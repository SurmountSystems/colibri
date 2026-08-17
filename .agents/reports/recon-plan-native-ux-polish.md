# Recon plan: native UX polish (doctor / scan / HF labels)

**Date:** 2026-08-10 · read-only. Priors: `recon-native-ui-doctor-scan.md`, `recon-native-hf-install-parity.md`.

## Critical files table

| Concern | File | Symbols / notes |
|---------|------|-----------------|
| Empty path → cwd | `crates/colibri-native/src/main.rs` | `run_doctor` / `run_deep_doctor` (~376–399); `bootstrap_panels` (~2644–2649): empty → `PathBuf::from(".")` |
| Doctor API + overall | `crates/colibri-sys/src/doctor.rs` | `run_doctor`; `model.config` fail if no `config.json` (~978–990); any `"fail"` → overall `"error"` (~1221–1227) |
| Doctor UI text | `crates/colibri-native/src/host.rs` | `run_shallow_doctor` / `run_deep_doctor` / `run_doctor_checks`; `format_doctor_checklist`; `doctor_overall_label` (`error`→**Fail**); empty/`.` → `Model: (none selected)` |
| Scan UI | `main.rs` | `scan_registry` (~797–824); empty msg hardcodes store + `dirs with config.json` |
| Scan host bridge | `host.rs` | `registry_scan_roots` (store only + optional extra); `scan_model_registry`; `format_registry_entry` |
| Depth-1 inventory | `crates/colibri-sys/src/model/registry.rs` | `ModelRegistry::refresh`: root if `config.json`, else **immediate children only** |
| Model store path | `colibri-sys` paths (`default_model_store_path`) | `~/.local/share/colibri/models` (XDG); env `COLIBRI_MODEL_STORE` / `COLI_MODEL_STORE` |
| HF form | `main.rs` | `repo_input` / `revision_input` / `dest_input` / `min_free_input` placeholders EN only; form panel ~2041–2114 (no field labels above inputs) |
| Min free | `host.rs` | `DEFAULT_INSTALL_MIN_FREE_BYTES` (=1 GiB); `parse_min_free_gb`; `format_install_space_with_min`; install API `model/install.rs` |
| Placeholder hide | `text_input.rs` | Placeholder only when **content empty** (~438–441) → default `"1"` hides `min free GB (0 = off)` |
| i18n | `crates/colibri-native/src/i18n.rs` | `EN`/`IT` tables: rail buttons, `hero.description` (“Doctor … without a model”). **No** install field keys; doctor checklist strings are **not** i18n’d |

## Recommended approach (minimal high impact)

1. **Doctor empty-model UX (native host first):** If model path empty (or only `.`), **do not** run sys doctor against cwd. Emit a friendly checklist: Overall **Idle/Skip** (or Warning), `Model: (none selected)`, one skip/info line “set a model path or Scan models”. Keep real `run_doctor` for non-empty paths. Optional: soften `hero.description` so it does not imply overall Pass without a model. Prefer host-only unless product wants sys `DoctorOptions` “no model mode.”
2. **Scan depth / discovery:** Minimal: document in empty status that only **one-level** dirs with `config.json` under the store count; point operators at paste path / `COLIBRI_MODEL`. Medium (if old GLM is under store nested): optional depth-2 or bounded walk in `ModelRegistry::refresh` with dedupe + tests. Do **not** auto-scan arbitrary HF hub cache unless explicitly scoped later.
3. **HF min-free label:** Add a visible label (or always-visible caption) **Min free disk (GB)** above/beside `min_free_input`; default value can stay `1`. Put EN+IT keys in `i18n.rs` (`install.minFree`, repo/revision/dest labels too for consistency). Placeholders alone are insufficient when text is non-empty.
4. **Copy consistency pass:** Move remaining hard-coded rail/status install strings toward i18n where user-facing; align Plan empty copy with Doctor empty (“set path first”).

## Red tests ideas

- `format_doctor_checklist` / host wrapper: empty path → no `Overall: Fail` from cwd `config.json`; shows none-selected + skip/idle.
- `run_shallow_doctor`/`bootstrap` path: empty model field never calls doctor with `"."` as a real container (or asserts formatter branch).
- `ModelRegistry::refresh`: nested `store/a/b/config.json` (if depth change accepted) found; depth-1 still finds `store/m/config.json`.
- `parse_min_free_gb` + UI contract already green; add assert that install form label key exists EN/IT for min free (once i18n’d).

## Risks / non-goals

- **Risks:** Overall status string change may break existing host tests that expect `Overall: Fail` on synthetic reports; recursion in scan can pick junk dirs; i18n only half-applied leaves IT mixed.
- **Non-goals:** Tauri/React install parity (desktop has no install UI); rewriting `coli doctor` CLI semantics; scanning `~/.cache/huggingface` by default; full doctor string localization in sys; changing default min-free from 1 GiB.
