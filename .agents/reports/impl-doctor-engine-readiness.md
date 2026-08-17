# Implement report: doctor engine-built + tilde + wizard Thorough check

**Workspace:** `/home/hunter/Projects/surmount/colibri`
**Date:** 2026-08-11

## Summary

Three product fixes:

1. **Doctor `engine.binary`** no longer reports **"engine is not built"** when the process binary is missing. It reports **in-process pass** when FFI is available, or **"external engine program not found"** (with path + `COLIBRI_ENGINE` / `COLI_ENGINE` hint) when it is not.
2. **`~` / `~/` expansion** on model (and engine) paths for doctor, plan, and open.
3. **Wizard step 4 (Check readiness)** gains a **Thorough check** button next to **Run checks again**, wired to deep doctor (same as Tools).

## Fix A — engine readiness

### Product behavior

| Situation | `engine.binary` |
|-----------|-----------------|
| Process executable found and loadable | **pass** — "engine executable is ready" |
| Process file exists but not executable | **fail** — "engine exists but is not executable" |
| Process missing + in-process available | **pass** — "in-process engine is available" |
| Process missing + no in-process | **fail** — "external engine program not found at …; set COLIBRI_ENGINE or COLI_ENGINE, or build with `make -C c <engine>`" |

### Implementation

- `DoctorOptions.in_process_engine: Option<bool>` — inject for tests; `None` detects from Cargo `ffi` + `COLIBRI_FORCE_PROCESS` via `ffi_available()`.
- `resolve_doctor_engine_path` — explicit `engine_path`, else family-aware `locate_engine`, else family basename for messaging.
- `engine_binary_check` — pure decision helper used by `run_doctor` (unit-tested both branches).
- Native host `run_doctor_checks`:
  - expands model path
  - env engine override or `locate_engine` by `model_arch`
  - with `feature = "ffi"` and not process-prefer, sets `in_process_engine = Some(true)`
  - passes machine GPUs into doctor when available

### Tests (red→green contracts)

- `engine_missing_without_ffi_says_external_not_not_built`
- `engine_missing_with_ffi_passes_in_process`
- `doctor_engine_missing_no_ffi_message_contract`
- `doctor_engine_missing_with_in_process_passes`
- `doctor_ffi_feature_defaults_to_in_process_when_binary_missing` (cfg `ffi`)
- host: `doctor_missing_process_never_says_not_built`

## Fix B — tilde expansion

### API

- `colibri_sys::expand_user_path` in `paths.rs`
  - `~` → home
  - `~/…` → home joined with rest
  - `~other…` and absolute/relative paths unchanged

### Wired

- host: `env_model_path`, `env_engine_path`, `run_doctor_checks`, `run_plan`, `EngineSession::start`
- main: `DesktopApp::model_path`, bootstrap panels

### Tests

- `paths::tests::expand_user_path_tilde_home`
- `paths::tests::expand_user_path_leaves_other_paths`
- host: `expand_user_path_tilde_for_doctor_and_plan`

## Fix C — wizard Thorough check

Readiness actions row (mirror Tools):

- `wizard-btn-readiness-refresh` — **Run checks again** (shallow doctor + plan) — unchanged behavior
- `wizard-btn-readiness-deep` — **Thorough check** (`rail.deepCheck` i18n) → `run_deep_doctor` only

No new i18n keys; reuses EN/IT `rail.deepCheck`.

## Files touched

| Path | Change |
|------|--------|
| `crates/colibri-sys/src/paths.rs` | `expand_user_path` + tests |
| `crates/colibri-sys/src/lib.rs` | re-export `expand_user_path` |
| `crates/colibri-sys/src/doctor.rs` | engine resolve + FFI-aware check + tests |
| `crates/colibri-native/src/host.rs` | doctor/plan/open tilde + locate_engine + tests |
| `crates/colibri-native/src/main.rs` | model_path expand; readiness Thorough button |

## Verify

```text
cargo fmt -p colibri-sys -p colibri-native
cargo clippy -p colibri-sys -p colibri-native --all-targets -- -D warnings   # clean
cargo test -p colibri-sys --lib   # 99 passed
cargo test -p colibri-native      # 180 passed
```

## Not in scope

- prefs JSON/TOML work (other agent)
- Python `c/doctor.py` string parity (Rust host path only)
- Making `ffi` a default feature of `colibri-native`
- Auto-running deep doctor on enter Readiness
