# Report: demote inference / heavy work (low compute priority)

**Date:** 2026-08-11
**Repo:** colibri
**Crate:** `colibri-sys` (engine serve spawn + install children)

## Goal

Keep the GPUI / host UI process at **default** scheduling priority. Run inference
and other CPU-heavy **child** work at lower priority so the UI does not fight
the engine for CPU under load.

## What changed

### New module: `crates/colibri-sys/src/process_priority.rs`

| Symbol | Role |
|--------|------|
| `ENGINE_CHILD_NICE = 10` | Unix niceness for heavy children |
| `engine_child_nice()` | Pure helper (tests / docs) |
| `engine_child_nice_is_elevated(n)` | Contract: nice in `1..=19` |
| `apply_low_compute_priority(&mut Command)` | Configure spawn only |

**Why 10 (not 19):** clear step below default nice `0` so the UI is preferred
when both are runnable, without max demotion (`19`) that can make the engine
feel badly laggy under light desktop load. Raising niceness is always allowed
without privileges.

**Platforms:**

| OS | Mechanism |
|----|-----------|
| Unix (Linux, macOS, …) | `Command::pre_exec` + `setpriority(PRIO_PROCESS, 0, 10)` in the **child** after fork, before exec |
| Windows | `BELOW_NORMAL_PRIORITY_CLASS` (`0x4000`) via `creation_flags` |
| Other | no-op |

The helper **never** calls `setpriority` on the calling process, so wiring it at
spawn cannot permanently nice-down the UI.

Public re-exports from `colibri_sys::{ENGINE_CHILD_NICE, apply_low_compute_priority, engine_child_nice, engine_child_nice_is_elevated}`.

### Call sites

1. **`ServeClient::spawn`** (`engine/serve.rs`) — process-mode engine child
   (product inference path). Documented on `engine/mod.rs`.
2. **`SystemHfCli::download`** (`model/install.rs`) — long-running `hf download`
   child.
3. **`convert_subprocess`** (`model/install.rs`) — Python convert last-resort
   child.

### Not demoted (by design)

| Path | Why |
|------|-----|
| **UI / colibri-native host** | Must stay at default priority |
| **In-process FFI inference** | Shares the host process; process-wide `setpriority` would demote GPUI. Residual: `open:ffi-inprocess-priority` |
| **Probe / doctor one-shots** (`ldd`, `nvidia-smi`, …) | Short-lived diagnostics; not inference |
| **Golden multi.rs oracle spawn** | Test-only free-generate, not the serve product path |

## Tests (red/green)

Contracts:

1. `ENGINE_CHILD_NICE` is elevated (`1..=19`, fixed at `10`).
2. `apply_low_compute_priority` does **not** change parent process nice.
3. Spawned child (Unix) has nice `== ENGINE_CHILD_NICE` via `getpriority`.

```text
cargo test -p colibri-sys --lib process_priority
# exit 0 — 4 passed

cargo test -p colibri-sys --lib process_priority --features install
# exit 0 — 4 passed

cargo test -p colibri-sys --lib engine::
# exit 0 — 24 passed
```

## Post-impl checks

```text
cargo fmt -p colibri-sys
# exit 0

cargo clippy -p colibri-sys --all-targets --features install -- -D warnings
# exit 0
```

## Residual

Pinned in `.agents/RESIDUAL.md`:

- **`open:ffi-inprocess-priority`:** FFI-in-process cannot be process-niced
  without demoting the UI. Mitigations left open: keep work off the UI thread
  (already the intent of async host paths); optional future thread-priority
  demotion if measured need exists. Process serve path is the demoted path.

## Acceptance checklist

| # | Criterion | Status |
|---|-----------|--------|
| 1 | Process-mode engine child starts at elevated niceness | Yes (`ServeClient::spawn` + Unix integration test) |
| 2 | UI/host not permanently niced as side effect of starting inference | Yes (`apply` parent-nice test + pre_exec child-only) |
| 3 | FFI isolation honesty | Residual pin + report section |
| 4 | Tests green; commands + exit codes listed | Above |
