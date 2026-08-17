# L3 implementer: FFI RAM clamp + Start refuse + cancel that C obeys

You are an L3 general-purpose implementer. **No L4.** Do not spawn further agents.

Repo: `/home/hunter/Projects/surmount/colibri`

## File ownership

You may edit:

- `crates/colibri-native/src/host.rs`
- `crates/colibri-sys/src/plan.rs`
- `crates/colibri-sys/src/ffi/multi.rs` (and related ffi if needed: `mod.rs`, bindings)
- `c/colibri.c`
- `c/kimi_k3.c`
- `c/colibri_api.h` if needed
- `crates/colibri-sys/docs/ffi-phase-d.md`

Do **not** edit `text_input.rs`, `native_log.rs`, `log_init.rs`, `main.rs`, or `archive_gpu_flavor.rs`.
Do **not** enable `COLI_MMAP`. Do **not** flip `just install` to `ffi-hip`.
Keep doctor test `memory_ram_capacity_tight_is_warn_not_fail` **unchanged in meaning** (warn, not fail).

## Locked product decisions

- **Start engine refuses** when even a one-slot working set exceeds available RAM (same idea as CLI `cap_for_ram` `exit(2)`).
- If the floor fits, **clamp** expert cache to ~88% of available RAM (reuse CLI `cap_for_ram` math).
- `COLI_RAM_OVERCOMMIT=1` remains the override (do not invent a second flag).
- Doctor stays warn. Start is the gate.
- New refuse/status strings: **plain operational English**. Not brand theater. No em dashes.

## Goal A — FFI RAM clamp + Start refuse

1. **C embed open (GLM)** after `model_init` in `coli_glm_engine_open` (~9631):
   - Snapshot `MemAvailable` into `g_mem_avail_boot` (CLI already uses `mem_available_gb()` around 9288).
   - Call `cap_for_ram` with `RAM_GB` env if set, else auto 88%.
   - If even cap=1 projected peak exceeds boot MemAvailable and `COLI_RAM_OVERCOMMIT` is not 1: **return an error** (fill `error` buffer, free engine, return -1). **Do not `exit(2)` from embed.**
   - CLI `main()` may still `exit(2)`. Split with a flag/return code so embed is non-abort.
   - Default `cap=64` must not win over the plan/clamp.

2. **Kimi embed** (`coli_kimi_engine_open`): after `model_init`, same honesty if Kimi has an expert cache you can clamp. If there is no LRU cap, at least do not `exit(2)`. Host preflight still covers Kimi.

3. **Host preflight** in `EngineSession::start` / `start_blocking_inner`:
   - After leaf preflight (`preflight_model_for_engine_start`), **before** `coli_glm_engine_open` / `open_engine`.
   - Project floor working set vs probe RAM (reuse placement-plan / 88% / one-slot idea).
   - If floor cannot fit and overcommit is off: `Err` with short status. **Must not call `open_engine`.**
   - Injected-available-RAM seam required so tests do not need a 400G model.

4. **Apply `environment_for_plan`** (`RAM_GB`, physical-core `OMP_NUM_THREADS`) **before** FFI open, not only on the process child. `set_var` is OK if you save/restore in tests. Isolate env override variables (past fail: leaked classification env).

5. Document `COLI_RAM_OVERCOMMIT=1` in the FFI user-guide paragraph of `ffi-phase-d.md`. Isolation honesty: crash isolation (process) is not oomd isolation. FFI now clamps and Start can refuse.

## Goal B — Stop that C obeys

- `spec_decode` while loop (~6125) currently checks `g_intr`, `g_mux_stop`, `g_mux_cancel` only. Mux flags are serve-only.
- Honor embed stop (`e->stop` or one dedicated `g_embed_stop` set from `coli_glm_emit_cb` / existing cancel).
- Prefill `step` (~5749): check the same flag on a layer or chunk checkpoint (the existing `COLI_PREFILL_CHUNK` loop is a natural yield).
- Keep cooperative UI cancel (no engine mutex). Honest: Stop becomes "soon," not cycle-accurate inside one matmul.

## TDD (required)

Write tests first. Observe red. Then product. Do not rewrite expectations to finish green.

**RAM (names should match these filters):**

- Injected small `MemAvailable` / available bytes → clamp leaves cap at budget, **not 64**
- Floor peak above RAM → open/preflight returns error (not abort)
- Host preflight refuses **without** calling `coli_glm_engine_open` / `open_engine` (inject a call counter or `cfg(test)` hook)
- Keep `memory_ram_capacity_tight_is_warn_not_fail` green
- Default path (overcommit **unset**) is the refuse/clamp path; add an explicit overcommit-on test that does **not** refuse
- Isolate `COLI_RAM_OVERCOMMIT` in tests (save/restore)

**Cancel:**

- Helper or C/Rust test: stop flag set → decode/prefill loop exits before `n_new`
- You may extract `embed_decode_should_stop()` for a unit test if a full `spec_decode` toy is too heavy; still wire the real loop to that helper.

Commands (shape):

```
cargo test -p colibri-sys --lib --features ffi cap_for_ram
cargo test -p colibri-native --lib preflight
cargo test -p colibri-sys --lib memory_ram_capacity_tight_is_warn_not_fail
```

Also `cargo fmt` / `clippy -D warnings` on **touched packages** (`colibri-sys`, `colibri-native`).

C file changes: do not invent a new Python script. Prefer existing C tests under `c/tests/` if you add a stop-flag case; still have a cargo-visible contract for the Rust preflight.

## Report

Write `/home/hunter/Projects/surmount/colibri/.agents/reports/l3-ffi-ram-cancel.md` with:

- files changed
- RED then GREEN for each named contract (command, test name, fail reason before product)
- fmt/clippy/test + exit codes
- how embed avoids `exit(2)`
- how preflight avoids calling open
- leftover (Kimi clamp depth, mmap-without-touch not done)

Never git add / commit / push. No implement-run hex in product source or `ffi-phase-d.md`.
