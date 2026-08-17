# Report: open:ffi-product-default (plan Step 4)

**Date:** 2026-08-10
**Scope:** Native-host default flip only when built with `feature=ffi`.
**Residual:** `open:ffi-product-default` **closed**.

## Decision (approved plan defaults)

| Choice | Outcome |
|--------|---------|
| Scope | **Native host only** (`colibri-native` `resolve_prefer_process`) |
| Crate `ColibriConfig.prefer_process` | Stays **true** (process-prefer) for library embeds |
| Process fallback on FFI open failure | **Kept** (`EngineSession::start`) |
| `COLIBRI_FORCE_PROCESS` | Always wins (process) |
| Isolation | Documented accept in `ffi-phase-d.md` (host-kill risk; kill-switch + no-feature build) |
| GPU | Not in this slice |

## Product behavior

| Build / env | Start path |
|-------------|------------|
| No `feature=ffi` | Process only (`prefer_process` true unless `COLIBRI_PREFER_FFI`) |
| `feature=ffi`, no force env | **Try FFI first** (`prefer_process` false); fall back to process on open failure |
| `COLIBRI_FORCE_PROCESS=1` | Process always |
| `COLIBRI_PREFER_FFI=1` | Under `feature=ffi`: **redundant** (default already FFI-first). Without feature: still sets `prefer_process=false` (no static link → process via `must_use_process`) |

## Code

- `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/host.rs`
  - `resolve_prefer_process_from_flags(force, prefer_ffi)` pure helper
  - `resolve_prefer_process()` reads env → pure helper
  - Under `cfg(feature = "ffi")`: force → process; else → **FFI-first** (`false`)
  - Without feature: force → process; prefer_ffi → false; else process
- Crate config unchanged (`crates/colibri-sys/src/config.rs` still `prefer_process: true`)

## TDD

**Red contract (tests):**

| Test | Contract |
|------|----------|
| `resolve_prefer_process_from_flags_force_always_process` | Force always process |
| `resolve_prefer_process_from_flags_default_by_feature` | feature=ffi → prefer_process false; without → true |
| `resolve_prefer_process_env_matches_flags` | Live env agrees with pure helper |
| `host_start_config_matches_resolve_prefer_process_from_flags` | Start config + `should_try_ffi_open` match resolve |
| Existing force/prefer composition | Still green |

**Commands (green):**

```bash
cargo test -p colibri-native resolve_prefer_process
# 5 passed (no ffi): default process

cargo test -p colibri-native resolve_prefer_process --features ffi
# 5 passed: default prefer_process false

cargo test -p colibri-native --features ffi
# 81 passed

cargo test -p colibri-sys --lib prefer_process --features ffi
# 2 passed (crate still process-prefer default)

cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets --features install -- -D warnings
cargo clippy -p colibri-native --all-targets --features "install,ffi" -- -D warnings
# clean
```

## Docs

| File | Update |
|------|--------|
| `crates/colibri-native/docs/fidelity.md` | FFI row **done** as native default under feature; architecture diagram; residual closed |
| `crates/colibri-native/README.md` | Env table + feature table + architecture note |
| `crates/colibri-native/Cargo.toml` | Feature comment |
| `crates/colibri-sys/docs/ffi-phase-d.md` | Status, kill-switch, isolation accept, residual closed |
| `crates/colibri-sys/docs/user-guide.md` | Library vs native default wording |
| `.agents/RESIDUAL.md` | Move product-default to CLOSED; architecture reminder |

## Not done / out of scope

- GPU link (`open:ffi-gpu`)
- NPU (`open:npu-inference`)
- Flipping crate-wide `prefer_process` default
- Full Kimi/Inkling visual fill / V4 poll symbols
- Git commit (operator-owned)
