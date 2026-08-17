# L3 implementer: session identity + heartbeat formatters

You are an L3 general-purpose implementer. **No L4.** Do not spawn further agents.

Repo: `/home/hunter/Projects/surmount/colibri`

## File ownership (hard)

You may edit ONLY:

- `crates/colibri-sys/src/native_log.rs`
- `crates/colibri-sys/src/archive_gpu_flavor.rs`
- If needed: a small new helper module under `crates/colibri-sys/src/` such as `session_identity.rs` (or keep helpers in `native_log.rs`)
- `crates/colibri-sys/src/lib.rs` **only** to `mod` the helper and **re-export** the new public formatters so `colibri-native` can call them later

Do **not** edit `host.rs`, `colibri.c`, `log_init.rs`, or `main.rs`. L2 will wire the timer after you land the formatters.

## Goal

Make `native.log` lines diagnosable: pid, comm, cgroup leaf, RSS, VmSwap, `kind=ffi|process`, and link flavor `cpu` / `HIP` / `CUDA`.

Plan step 2. Recon: `.agents/reports/recon-harden-logs-perf.md`

Keep redact (`sanitize_log_text`). No prompts. No Sentry.

## What to implement

1. **`format_engine_start_log`**
   - Keep the existing signature so `host.rs` does not need a compile fix from you.
   - Append identity fields to the line: at least `pid=` and `flavor=`.
   - Flavor from existing `archive_gpu_flavor` / `GpuArchiveFlavor` Display (`cpu` / `HIP` / `CUDA`).
   - Prefer a `linked_engine_flavor() -> &'static str` that uses compile-time features:
     - `feature = "ffi-hip"` → `"HIP"`
     - `feature = "ffi-cuda"` → `"CUDA"`
     - else `"cpu"`
   - Reuse Display names; do not invent `rocm` / `gpu`.
   - Existing test `engine_start_log_line_has_path_not_secrets` must still pass.

2. **Heartbeat line formatter** (public, for L2 to call every 5–10s):
   - Something like `format_session_heartbeat_line(kind: Option<&str>) -> String`
   - One short line. No prompts.
   - Include: pid, comm, cgroup leaf, flavor, kind (if given), rss, vmswap.
   - Suggested keys: `heartbeat pid=… comm=… cgroup=… flavor=cpu kind=ffi rss_kb=… vmswap_kb=…`

3. **`/proc` parse helpers** (fixture-tested; do not require a live engine):
   - `parse_proc_status_vm(text) -> (rss_kb, vmswap_kb)` from a `VmRSS:` / `VmSwap:` blob
   - `cgroup_leaf(text) -> &str` last path component of `/proc/self/cgroup`
   - `comm` trim of `/proc/self/comm`
   - Live readers (`read_self_status`, `read_self_cgroup`, `read_self_comm`) may wrap `/proc/self/*` and fall back to empty/0 on non-Linux.

4. **`format_generate_log`**
   - Optional: also append pid + flavor. Do not drop existing fields. Keep no-prompt contract.

## TDD (required)

Write tests first. Observe red. Then product.

**Required:**

- A test that `format_engine_start_log` (or a sibling used by it) includes **pid**, **kind**, and **flavor**
- Heartbeat / parser test accepts a **fixture** `VmRSS` / `VmSwap` blob (not live `/proc` as the only assert)
- Parser edge: missing `VmSwap` → 0 (or documented default), do not panic

If you add env-based classification, isolate override variables in the test (save/restore or pass args; do not leak `COLI_*` / feature env into other tests).

## Verify

```
cargo test -p colibri-sys --lib native_log
cargo test -p colibri-sys --lib archive_gpu_flavor
cargo fmt -p colibri-sys
cargo clippy -p colibri-sys --all-targets -- -D warnings
```

If you add `session_identity.rs`, also run a filter for that module.

## Report

Write `/home/hunter/Projects/surmount/colibri/.agents/reports/l3-logs-heartbeat.md` with RED/GREEN (command, test name, fail reason before product edit), fmt/clippy/test + exit codes, public symbol names L2 should call, files changed.

Never git add / commit / push. No implement-run hex in product source.
