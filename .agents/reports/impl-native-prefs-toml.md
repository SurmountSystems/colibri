# Report: `impl:native-prefs-toml`

**Slice:** Native UI preferences as TOML (`native-ui.toml`)
**Package:** `colibri-native`
**Date:** 2026-08-11
**Wizard:** not built (API only)

## Delivered

### New module
[`crates/colibri-native/src/prefs.rs`](../../crates/colibri-native/src/prefs.rs)

| Piece | Behavior |
|-------|----------|
| Path | `~/.config/colibri/native-ui.toml` via `XDG_CONFIG_HOME` or `~/.config`; Windows `%LOCALAPPDATA%\colibri\native-ui.toml` (same colibri folder family as model store) |
| Fields | `version=1`, `first_run_done`, `theme` (`doge`\|`mint`), `locale` (`en`\|`it`), `last_model_path` |
| Defaults | Missing or corrupt file → defaults (`first_run_done=false`, theme **doge**, locale **en**, empty model path) |
| Theme parse | Unknown / empty → **doge** (whole file still loads other fields) |
| Env | `COLIBRI_THEME` applied in `load()` / `apply_env_overrides()`; `COLIBRI_SKIP_WIZARD=1` (`true`/`yes` also) gates `should_show_wizard()` without writing the file |
| API | `load` / `load_from_path`, `save` / `save_to_path`, `ThemePref`, `LocalePref`, `NativePrefs`, path helpers |

### Dependencies
- [`crates/colibri-native/Cargo.toml`](../../crates/colibri-native/Cargo.toml): direct `toml = "0.8"` (serde already present)

### Wiring
- `mod prefs;` in [`main.rs`](../../crates/colibri-native/src/main.rs)
- No first-run wizard UI; no Tools theme controls yet

## TDD / tests

Command:

```bash
cargo test -p colibri-native --bin colibri-native prefs
```

**Result:** 10 passed

| Test | Contract |
|------|----------|
| `defaults_when_file_missing` | load missing path → defaults |
| `defaults_when_file_corrupt` | bad TOML → defaults |
| `round_trip_temp_dir` | save + load under temp dir |
| `unknown_theme_becomes_doge` | parse + file `theme = "neon"` → doge; other fields kept |
| `skip_wizard_env_parse_and_gate` | truthy skip values; wizard gate with/without skip |
| `theme_env_override_via_parse` | mint / unknown theme strings |
| `locale_parse_and_unknown` | en / it / fallback |
| `platform_path_contains_colibri_and_filename` | path shape |
| `partial_toml_fills_defaults` | sparse file |
| `public_api_surface_smoke` | product entry points (`load`, env keys, `as_str`, …) |

## Verify

```bash
cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets -- -D warnings   # green
cargo test -p colibri-native --bin colibri-native prefs       # 10 ok
```

## Example file

```toml
version = 1
first_run_done = false
theme = "doge"
locale = "en"
last_model_path = ""
```

## Incidental (concurrent theme work)

Half-migrated theme palette work left `main.rs` call sites / imports broken and blocked the binary compile. Minimal unblocks so prefs tests could run:

- Restored mint const imports alongside `ThemeId` / `ThemePalette`
- Passed `&p` into `section_title` / `panel` / `badge_chip` where signatures already required palette
- `#![allow(dead_code)]` on `theme.rs` for symbols not yet fully wired (same pattern as `progress.rs` / `prefs.rs`)

Not a full Brain/PROF theme pass; theme implementer still owns that slice.

## Not in this slice

- Setup wizard UI / first-run navigation
- Tools panel theme/locale controls
- Applying `NativePrefs` theme/locale into live paint at startup (API ready for next slices)

## Next

- Wire `prefs::load()` at launch; apply theme + locale; gate wizard on `should_show_wizard()`
- Save on Finish / Skip / Tools theme or language change
