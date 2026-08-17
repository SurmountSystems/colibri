# Slice report: FFI RAM clamp + embed cancel

**Scope:** `host.rs`, `plan.rs`, FFI bindings, `c/colibri.c`, `c/kimi_k3.c`, `c/colibri_api.h`, `ffi-phase-d.md`, user-guide.
**Note:** Implemented in the L2 thread (workflow spawn blocked from this session).

## Contract

Start refuses when one-slot working set exceeds RAM. Doctor stays warn. Floor fits → clamp ~88% RAM. Embed `cap_for_ram` returns error, not `exit(2)`. `COLI_RAM_OVERCOMMIT=1` override. Apply `RAM_GB` / `OMP_NUM_THREADS` before FFI open. `spec_decode` and prefill honor embed stop. No `COLI_MMAP`. No silent ffi-hip.

## Red

```text
cargo test -p colibri-sys --lib --features ffi cap_for_ram
cargo test -p colibri-native --bin colibri-native preflight
```

Fail: compile missing clamp types / host preflight helpers. After the seam landed, config-only fixture skipped RAM (inspect fail). `write_tiny_glm_leaf` made refuse run. Contract unchanged.

## Green

- `cap_for_ram` 3 passed (clamp below 64, refuse without overcommit, overcommit allows floor).
- `preflight` 6 passed (`preflight_ram_refuses_without_calling_open`, overcommit skip).
- `memory_ram_capacity_tight_is_warn_not_fail` passed.
- `embed_decode_should_stop_when_flag_set` passed.

## Landed

- `clamp_expert_cap_for_ram` + `ram_overcommit_from` (C `atoi` non-zero).
- Host `preflight_then_maybe_open` on the default Start path.
- C `cap_for_ram(..., embed)` after GLM `model_init`; `embed=1` returns `-1`.
- `g_embed_stop` + `coli_embed_*`; `coli_decode_should_stop` in `spec_decode` and prefill `step`.
- Isolation paragraph in `ffi-phase-d.md`; user-guide documents clamp + overcommit.
