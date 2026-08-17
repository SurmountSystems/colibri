# Doctor mkdir missing model path

## Goal

When **Run doctor** / **Quick check** sees a model path that does not exist, create the folder (`create_dir_all`) instead of only showing "This folder is missing."

## Behavior

1. Expand path (`expand_user_path`).
2. If missing → `ensure_model_directory` (`create_dir_all` + parents).
3. **Created** → short doctor copy: `Created this folder. Install a model or scan.`
4. **Create failed** → short copy: `Could not create this folder: {error}` (permissions, parent is a file, …).
5. Path exists, no `config.json` → `This folder has no config.json.` (not a full check dump).
6. Path is a model leaf → normal sys doctor checklist.
7. Recovery path (`run_doctor_with_recovery`) creates before scan so empty typed paths are real directories.
8. Cold start: once, `ensure_model_directory` on the default store root (not on every keystroke).

Security: only the user/app model path and default store are created, never arbitrary system paths from untrusted input.

## Files

| Area | Path |
|------|------|
| Helper | `crates/colibri-sys/src/paths.rs` — `EnsureModelDir`, `ensure_model_directory`, `ensure_default_model_store` |
| Export | `crates/colibri-sys/src/lib.rs` |
| Formatters + doctor | `crates/colibri-native/src/host.rs` — `format_created_model_directory`, `format_could_not_create_model_directory`, `ensure_model_path_for_doctor`, `run_doctor_checks` |
| Wizard / Tools recovery | `crates/colibri-native/src/main.rs` — `run_doctor_with_recovery` mkdir first; cold-start store ensure |

## Tests (red → green)

- `paths::ensure_model_directory_creates_missing_path`
- `paths::ensure_model_directory_fails_when_parent_is_file`
- `paths::ensure_model_directory_rejects_empty_path`
- `host::run_shallow_doctor_creates_missing_path` (temp dir → exists + "Created this folder.")
- `host::run_shallow_doctor_uncreatable_path_reports_error` (parent is file)
- `host::run_shallow_doctor_unwritable_root_is_recovery_not_check_dump` (`/no/such/...`)
- Formatter compact/created/could-not-create checks

## Verify

```text
cargo fmt -p colibri-sys -p colibri-native
cargo clippy -p colibri-sys -p colibri-native --all-targets -- -D warnings
cargo test -p colibri-sys --lib
cargo test -p colibri-native --bin colibri-native
```

All green (102 sys lib, 194 native bin).

## Operator UX

With path `/home/hunter/.models` missing, **Run doctor** now creates `~/.models` (or that absolute path) and shows created copy, not "This folder is missing." Install / Scan still needed for model files.
