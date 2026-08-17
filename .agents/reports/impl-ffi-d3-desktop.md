# Report: D3 desktop opt-in + auto-fallback (colibri-native)

**Date:** 2026-08-10
**Scope:** `colibri-native` optional in-process FFI with process default and auto-fallback

## Outcome

Complete. Process serve remains the product default. Desktop can opt into multi-family CPU FFI when built with Cargo feature `ffi` and runtime prefer (`COLIBRI_PREFER_FFI=1` or `prefer_process=false`). Open and generate failures fall back to the engine process with plain-English status. `COLIBRI_FORCE_PROCESS=1` always forces process.

## Deliverables

| Item | Status |
|------|--------|
| Cargo feature `ffi = ["colibri-sys/ffi"]` (not default) | done |
| Host start prefers FFI open when opted in + family maps | done |
| Generate via FFI when session is FFI-backed | done |
| Fallback to process on open/generate failure + status | done |
| `COLIBRI_FORCE_PROCESS=1` forces process | done (via `resolve_prefer_process` + colibri-sys kill-switch) |
| Brain / PROF need process (documented in status/UI) | done |
| Pure routing helper tests | done |

## Design

### Feature / config / env

- **Cargo:** `colibri-native` feature `ffi` → `colibri-sys/ffi` (off by default; `install` stays default).
- **Env:**
  - `COLIBRI_PREFER_FFI` truthy → `prefer_process = false` at start
  - `COLIBRI_FORCE_PROCESS` truthy → always process (wins over prefer-FFI)
- **Helpers** (`host.rs`): `env_flag_truthy`, `env_prefer_ffi`, `env_force_process_path`, `resolve_prefer_process`, `should_try_ffi_open`, `EnginePathKind`, `engine_path_status_line`

### Session backends

`EngineSession` holds `LiveEngine`:

- `Process(EngineHandle)` — product default; duplex generate; live Brain/PROF/HWINFO
- `Ffi { engine: Arc<Mutex<…>>, model_path }` (feature `ffi`) — in-process open/generate; empty visual snapshot; cooperative cancel via `AtomicBool` (no mux STOP)

Start:

1. Build `ColibriConfig` with `prefer_process(resolve_prefer_process())`
2. If `should_try_ffi_open` and feature on and `FfiFamily::from_model_family` maps → `open_engine`
3. On open success → FFI session; status includes size + “Brain needs engine process”
4. On open failure → process start + status “In-process open failed (…); using engine process”

Generate:

- Process: existing `EngineDuplex` path (unchanged)
- FFI: `generate_ffi` under engine mutex only (session lock not held; Stop can set cancel)
  - V4: session generate + `generated_text` when available
  - GLM / Kimi / Inkling: token-id API only → progress dots + summary line noting full detokenize stream is process-path
- On FFI generate failure → start process for same model, replace session, re-run process generate once

UI (`main.rs`): engine chip includes `path_status()`; FFI ready status mentions Brain/profiling need the engine process.

## Docs

- `crates/colibri-native/Cargo.toml` — `ffi` feature
- `crates/colibri-native/README.md` — feature table, env keys, architecture
- `crates/colibri-native/docs/fidelity.md` — multi-family FFI row updated (desktop opt-in + fallback)

## Tests

- `env_flag_truthy_matrix`
- `resolve_prefer_process_pure_defaults` (respects live env if set)
- `resolve_prefer_process_respects_force_over_prefer_ffi` / `should_try_ffi_open` with/without `ffi`
- `engine_path_kind_labels_are_plain` (Brain note on FFI status line)

## Verify (ran)

```text
cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets --features install -- -D warnings          # ok
cargo clippy -p colibri-native --all-targets --features install,ffi -- -D warnings     # ok
cargo test -p colibri-native                                                           # 78 passed
cargo test -p colibri-native --features ffi                                            # 78 passed
```

## Residual (out of this slice)

- Full UTF-8 detokenize stream on multi-family CPU FFI (C API is token-id only today)
- Live Brain / EMAP / HITS / PROF / HWINFO without process mux
- Multi-slot KV / grammar on pure FFI
- Product defaulting to FFI (`prefer_process` remains true)

## Key paths

- `/home/hunter/Projects/surmount/colibri/crates/colibri-native/Cargo.toml`
- `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/host.rs`
- `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/main.rs`
- `/home/hunter/Projects/surmount/colibri/crates/colibri-native/README.md`
- `/home/hunter/Projects/surmount/colibri/crates/colibri-native/docs/fidelity.md`
