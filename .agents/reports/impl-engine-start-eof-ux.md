# Report: Engine start EOF UX (preflight + plain errors)

## Symptom
Wizard Ready **Start engine** with model path `~/.models` (empty store, not an HF model leaf) failed with:

```
Could not start engine: engine start failed: serve protocol error: EOF before READY
```

Operator saw lab handshake jargon instead of a clear "not a model yet" message.

## Root cause (product)
1. Host `EngineSession::start` only required `path.is_dir()`, then tried FFI open and/or process serve.
2. Process path spawns the engine binary and waits for the READY sentinel on stdout.
3. With a store root (no `config.json` / weights), the child exits or closes the pipe before READY.
4. `colibri-sys` maps that to `Error::Protocol("EOF before READY")`, then host wrapped it as `engine start failed: serve protocol error: EOF before READY`.
5. UI prefixed with `Could not start engine: ` only. No preflight, no plain map.

Almost always: path is not a loadable model leaf (or engine binary missing). Operator should never need to decode READY/EOF.

## Changes
### Host helpers (`crates/colibri-native/src/host.rs`)
- `is_model_leaf(path)` — dir + `config.json` file
- `ENGINE_START_NOT_A_MODEL` — plain preflight body (UI still prefixes `Could not start engine: `)
- `preflight_model_for_engine_start(path)` — expand `~`; reject empty / missing / non-dir / non-leaf **without** open or serve spawn
- `map_engine_start_error(err, model)` — pure map:
  - `EOF before READY` / serve protocol / waiting for READY →
    `engine quit before it was ready (often bad model path or missing engine). Model: {path}`
  - missing binary / COLI_ENGINE override →
    `engine binary not found. Build the engine or set COLI_ENGINE. Model: {path}`
  - preflight strings pass through

### Start path
- `EngineSession::start` calls preflight first (fail-fast).
- Process start errors (and FFI→process generate fallback errors) run through `map_engine_start_error`.

### Wizard Ready (`main.rs` + `i18n.rs`)
- Start engine button stays enabled.
- If path is not a model leaf, summary card shows warning (`wizard.ready.modelNotReady`, en + it) in warn color.
- Click still hits preflight:
  `Could not start engine: this folder is not a model yet. Use Install a model or choose a folder with config.json and weights.`

## Tests (TDD contracts)
| Test | Contract |
|------|----------|
| `is_model_leaf_requires_config_json` | leaf = dir + config.json |
| `preflight_rejects_empty_and_non_model_without_open` | empty / empty dir / missing → fixed plain string; leaf Ok |
| `map_engine_start_error_protocol_eof_is_plain` | lab EOF/protocol → plain + model path; no EOF/READY in output |
| `map_engine_start_error_missing_binary_is_plain` | locate-style missing binary → plain |
| `map_engine_start_error_passes_through_preflight` | preflight text unchanged |
| `engine_session_start_preflight_rejects_empty_store` | `EngineSession::start` on empty temp dir fails preflight (no serve) |

## Verify
```text
cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets -- -D warnings   # ok
cargo test -p colibri-native                                   # 217 passed
```

## Files
- `crates/colibri-native/src/host.rs` — preflight, map, start wire, tests
- `crates/colibri-native/src/main.rs` — Ready warning import + UI
- `crates/colibri-native/src/i18n.rs` — `wizard.ready.modelNotReady` en/it

## Out of scope
- Git commit
- Changing serve protocol / READY sentinel itself
- Auto-installing a model on Ready
