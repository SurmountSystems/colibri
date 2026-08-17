# Implement: native UI config TOML primary + JSON load compatibility

Date: 2026-08-11
Workspace: `/home/hunter/Projects/surmount/colibri`
Scope: `crates/colibri-native` prefs (+ short README / fidelity note)

## Goal

Native UI prefs: **TOML primary write**, **JSON load compatibility** for a sibling
`native-ui.json` (existing users / hand-written files). No engine config file
invented in colibri-sys. HF model `config.json` unchanged.

## What landed

### `crates/colibri-native/src/prefs.rs`

| Behavior | Detail |
|----------|--------|
| Primary path | `native-ui.toml` (`PREFS_FILE_NAME`) |
| Legacy sibling | `native-ui.json` (`PREFS_JSON_FILE_NAME`) |
| Load order | Valid TOML at path → else valid sibling JSON → else defaults |
| Corrupt TOML | Falls through to JSON, then defaults |
| Save | Always TOML via `toml::to_string_pretty`; does **not** delete JSON |
| Env | Unchanged: `COLIBRI_THEME` after load; `COLIBRI_SKIP_WIZARD` gate |
| Shared schema | `RawNativePrefs` deserializes from both formats (same field names) |

Helpers: `json_prefs_path_beside`, `parse_toml_text` / `parse_json_text` (private).

`load()` / `default_prefs_path()` still point at the TOML path; `load_from_path`
implements the dual-format order relative to that path’s directory.

### Docs

- Module docs: load order + save-always-TOML + leave JSON in place.
- `crates/colibri-native/README.md`: short **UI preferences** section.
- `crates/colibri-native/docs/fidelity.md`: prefs row notes JSON fallback.

### colibri-sys app config file

**Confirmed none.** `ColibriConfig` is in-memory + env only. JSON under sys is
HF model layout (`config.json`, tokenizer, indexes), doctor/plan stdout shapes,
and fixtures. No operator-facing app config file to dual-format. Skipped inventing
`engine.toml`.

## TDD (temp dir)

| Test | Result |
|------|--------|
| `load_json_prefs_when_no_toml` | green |
| `load_toml_prefs_succeeds` | green |
| `both_present_toml_wins_over_json` | green |
| `corrupt_toml_falls_back_to_json` | green |
| `save_always_writes_toml_and_leaves_json` | green |
| Prior prefs tests (defaults, corrupt-only, round-trip, partial, …) | green |

## Verify

```text
cargo test -p colibri-native prefs
→ 17 passed (prefs + related wizard prefs filters)

cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets -- -D warnings
→ clean
```

## Files touched

- `crates/colibri-native/src/prefs.rs` (load/save + tests)
- `crates/colibri-native/README.md` (UI preferences blurb)
- `crates/colibri-native/docs/fidelity.md` (one row)

## Out of scope (per task)

- doctor / wizard `main.rs` rewrites (other agent)
- HF `config.json` semantics
- tune profiles under `tuning/*.json`
- inventing a colibri-sys engine config file

## Residual

None for this slice. Optional later: explicit migrate flag to remove JSON after
successful TOML save (default remains leave-in-place).
