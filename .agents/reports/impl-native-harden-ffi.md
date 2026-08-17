# Report: Native harden (composer keys, logs, FFI RAM, cancel)

**Date:** 2026-08-13
**Plan:** session `plan.md` (harden native)
**Residual:** composer keys, session heartbeat, FFI RAM clamp + Start refuse, embed cancel **closed**. Leftover later: mmap-without-touch, HIP explicit rebuild (operator-gated), cgroup `MemoryMax`. NPU / REST / visual-pump Join / generate % / hub mid-file bytes stay deferred.

## Named contract

1. Composer: word move/select/delete, Shift-select, Home/End = buffer start/end, Ctrl+Home/End alias the same. No Enter-to-send.
2. `native.log` start/generate lines include pid, comm, cgroup leaf, `kind`, flavor `cpu`/`HIP`/`CUDA`. Heartbeat every 8s while the engine is up. One short line. No prompts.
3. Start **refuses** when even a one-slot working set exceeds available RAM. Doctor stays warn. Floor fits → clamp expert cache (~88% RAM, CLI `cap_for_ram`). Embed returns error, not `exit(2)`. `COLI_RAM_OVERCOMMIT=1` override.
4. `spec_decode` and prefill `step` honor embed stop (`g_embed_stop`). Stop does not take the engine mutex.
5. `just install` stays CPU `ffi`. Do not enable `COLI_MMAP`. No Sentry. No `MemoryMax`.

## Hierarchy

Host L2 cannot launch workflows from this session (`Workflows can only be launched from a top-level session`). No `spawn_subagent` tool. All four slices were implemented in this L2 thread with disjoint file ownership respected in order: composer and log formatters first, then FFI RAM + cancel, then heartbeat wiring. Slice reports:

- `.agents/reports/l3-composer-keys.md`
- `.agents/reports/l3-logs-heartbeat.md`
- `.agents/reports/l3-ffi-ram-cancel.md`

## Red then green

### Composer (`text_input.rs`)

```text
cargo test -p colibri-native --bin colibri-native text_input
```

**RED (before product helpers):** five fails. Stubs returned the input offset / flatten identity. Failed: `word_left_from_after_space_lands_on_word_start`, `word_left_from_mid_word_lands_on_that_word_start`, `ctrl_backspace_deletes_previous_word_and_leading_spaces`, `ctrl_delete_deletes_next_word`, `paste_flattens_newlines`.

**GREEN:** same command, 14 passed (12 composer-key tests + 2 selection tests). Exit 0.

### Logs (`native_log.rs`)

```text
cargo test -p colibri-sys --lib native_log
```

**RED:** compile fail (`E0425`) missing `SessionIdentity`, `parse_proc_status_vm`, `cgroup_leaf`, `format_session_heartbeat_line`.

**GREEN:** same command, 15 passed. Exit 0.

### Heartbeat wiring (`log_init.rs` / `main.rs`)

```text
cargo test -p colibri-native --bin colibri-native session_heartbeat
```

**RED:** compile fail (`E0425`) missing `SESSION_HEARTBEAT_MS`, `session_heartbeat_line`, `write_session_heartbeat_to`.

**GREEN:** same command, 4 passed. Exit 0.

### RAM clamp + Start refuse

```text
cargo test -p colibri-sys --lib --features ffi cap_for_ram
cargo test -p colibri-native --bin colibri-native preflight
cargo test -p colibri-sys --lib memory_ram_capacity_tight
```

**RED:** compile fail missing `ClampExpertCapInput` / `clamp_expert_cap_for_ram` / `preflight_ram_for_engine_start`. After the product seam landed, `preflight_ram_refuses_without_calling_open` first returned `Ok` because a config-only leaf failed inspect and skipped RAM. Fixture `write_tiny_glm_leaf` (valid tiny safetensors) made the refuse path run. Contract unchanged.

**GREEN:** cap_for_ram 3 passed; preflight 6 passed; doctor `memory_ram_capacity_tight_is_warn_not_fail` 1 passed. Exit 0.

### Cancel

```text
cargo test -p colibri-sys --lib embed_decode_should_stop
```

**RED:** compile fail missing `embed_decode_should_stop`.

**GREEN:** `embed_decode_should_stop_when_flag_set` passed. Exit 0. C `spec_decode` / prefill `step` call `coli_decode_should_stop()` which includes `g_embed_stop`.

## fmt / clippy

| Command | Exit |
|---------|------|
| `cargo fmt -p colibri-native -p colibri-sys` | 0 |
| `cargo clippy -p colibri-native --all-targets -- -D warnings` | 0 |
| `cargo clippy -p colibri-sys --all-targets -- -D warnings` | 0 |
| `cargo clippy -p colibri-sys --all-targets --features ffi -- -D warnings` | 0 |

Clippy mop: `avail.checked_div(slot)` in `clamp_expert_cap_for_ram`; `#[cfg(test)]` on FFI open-attempt getters; Start uses `preflight_then_maybe_open` so the helper is on the default path.

## What landed

- Word bounds via `unicode-segmentation`; flatten paste newlines to spaces.
- Identity formatters + `/proc` fixture parsers; flavor from compile features (`cpu` / `HIP` / `CUDA`).
- Heartbeat on log init and every 8s while `EngineSession` is in the slot.
- Rust clamp seam + C embed `cap_for_ram(..., 1)` after sampling MemAvailable (before `model_init`); refuse tears down the Model; host preflight refuse; `RAM_GB` / `OMP_NUM_THREADS` refresh unless operator-set.
- `coli_embed_request_stop` / `clear` / `should_stop`; Stop sets the flag without the engine mutex. Default-path prefill checks stop between layers; leftover `layers_forward` is skipped.

## Review-fix (round 1)

All 18 open review issues addressed. Reports: `l3-fix-composer.md`, `l3-fix-logs.md`, `l3-fix-ffi.md`, `l3-fix-host.md`. Review file: `/tmp/grok-1000/grok-review-d2649b7b.md`.

## Honesty

- Heartbeat I/O is `/proc` plus one line every 8s. Not a last-line crash dump. SIGKILL still writes nothing.
- Stop is cooperative: between tokens / layers, not cycle-accurate inside one matmul. The optional `COLI_PREFILL_CHUNK` path also skips leftover `layers_forward` when stop is set.
- Start refuse and C clamp both use widest-slot math. Pessimistic vs a skinny expert. Override is `COLI_RAM_OVERCOMMIT=1`.
- `just install` remains CPU `ffi`. HIP rebuild is operator-gated.
- `COLI_MMAP` is not enabled.

## Non-goals (untouched)

NPU, OpenAI REST, generate % redesign, hub mid-file bytes, Sentry, hugepages, Enter-to-send, silent ffi-hip, systemd `MemoryMax`.
