# Slice report: session identity formatters

**Scope:** `crates/colibri-sys/src/native_log.rs` and `lib.rs` re-exports.
**Note:** Implemented in the L2 thread (workflow spawn blocked from this session). Did not edit `archive_gpu_flavor.rs` (Display names reused). Did not edit `host.rs` or `colibri.c`.

## Contract

Start/generate lines include pid, comm, cgroup, flavor. Heartbeat formatter for pid/comm/cgroup/flavor/RSS/VmSwap. Fixture `/proc` parsers. No prompts.

## Red

```text
cargo test -p colibri-sys --lib native_log
```

Fail: compile (`E0425`) missing `SessionIdentity`, `parse_proc_status_vm`, `cgroup_leaf`, `format_session_heartbeat_line`.

## Green

Same command, 15 passed, exit 0.

## Landed

- `linked_engine_flavor()`: `ffi-hip` → `HIP`, `ffi-cuda` → `CUDA`, else `cpu`.
- `parse_proc_status_vm`, `cgroup_leaf`, `session_identity_now`, `format_session_heartbeat_line`.
- `format_engine_start_log` / `format_generate_log` append identity fields.

Timer wiring is L2: `log_init.rs` + `main.rs` (8s while engine up).
