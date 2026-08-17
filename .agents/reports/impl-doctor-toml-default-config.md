# Doctor TOML default config (store + UI prefs)

## Goal

Doctor on an empty / non-model folder should scaffold **product-owned TOML**
(same spirit as mkdir), not leave the user staring at raw "This folder has no
config.json". Distinguish app/store scaffold from real Hugging Face model leaves.

## Done

### Store path: `colibri.toml`

- Constant `STORE_CONFIG_FILE_NAME` = `colibri.toml` in
  `crates/colibri-native/src/host.rs`.
- `ensure_store_colibri_toml(dir)` writes default body only when missing.
- Body is path notes only (`version = 1` + comments). **Never** invents HF
  `config.json` or pretends to be transformers config.
- `scaffold_doctor_defaults(model_dir)` = store TOML + UI prefs ensure.

### UI prefs: `native-ui.toml`

- `prefs::ensure_prefs_file_if_missing(path)` and
  `prefs::ensure_default_prefs_file()`.
- If TOML missing: load from sibling JSON or defaults, then **write TOML only**.
- Does not overwrite an existing TOML file.

### Doctor behavior (`run_doctor_checks` + wizard recovery)

| Situation | Behavior |
|-----------|----------|
| Path missing | `create_dir_all`, write `colibri.toml`, ensure `native-ui.toml`, short "created folder and default colibri.toml" copy |
| Dir exists, no `config.json` | Write `colibri.toml` if missing, ensure prefs; "Created default colibri.toml here" / "not a model yet" |
| Real model leaf (`config.json`) | Full sys doctor; **no** store `colibri.toml` scaffold on that leaf; still ensure UI prefs |

Copy no longer leads with "This folder has no config.json."

### Memory plan

- `run_plan` on non-model dir returns:
  `No memory plan yet. Not a model folder yet. Install a model or paste a model folder.`
- Does not surface raw `missing config.json` as the only UI string.

## Files

- `crates/colibri-native/src/host.rs` — scaffold helpers, doctor/plan copy, tests
- `crates/colibri-native/src/prefs.rs` — ensure prefs TOML
- `crates/colibri-native/src/main.rs` — wizard recovery also scaffolds

## Tests (observed)

```text
cargo test -p colibri-native --bin colibri-native
# 201 passed

Notable filters:
  ensure_store_colibri_toml_is_idempotent
  run_shallow_doctor_empty_dir_writes_colibri_toml
  run_shallow_doctor_creates_missing_path
  run_shallow_doctor_real_model_leaf_still_runs_checks
  run_plan_empty_dir_is_not_raw_missing_config
  format_not_a_model_folder_is_plain_english
  ensure_prefs_file_if_missing_writes_toml_once
  ensure_prefs_file_if_missing_promotes_json_values
```

Also: `cargo fmt -p colibri-native`, `cargo clippy -p colibri-native --all-targets -- -D warnings` clean.

## Not done / out of scope

- No fake HF architecture TOML that replaces `config.json` for load.
- Registry still discovers models by `config.json` only.
- sys `doctor.rs` / Python `doctor.py` unchanged (native host recovery only).
- Hardened one env-sensitive startup test (`~/.models` exists on this host).
