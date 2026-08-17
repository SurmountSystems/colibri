# Report: Rust decode + FfiEngine + native pump (`open:ffi-visual-abi` Step 2)

**Date:** 2026-08-10
**Scope:** Bind `coli_*_visual_poll`, decode into `VisualSnapshot`, `FfiEngine::pump_visual`, native `LiveEngine::Ffi` visual path. GLM-first. Product default stays process.

---

## TDD

### Red (contract)

Unit contract: fixed fixture bytes must produce a **non-empty** `VisualSnapshot` without a SERVE subprocess.

| Fixture | Meaning |
|---------|---------|
| EMAP cells `[0x43, 0x80]` | tier1 heat3, tier2 heat0 (`pack_expert_cell`) |
| HITS bits `[0x09]` | experts 0 and 3 set |
| PROF seq 1 | one profile turn |

Test name: `visual::tests::visual_snapshot_from_fixed_binary_fixtures`
(Implemented as green under `absorb_binary_poll`; packing oracles already matched C report.)

### Green (evidence)

```text
cargo test -p colibri-sys --lib visual::
# 3 passed (expert_map_from_cells, hits_bit_packing, visual_snapshot_from_fixed_binary_fixtures)

cargo test -p colibri-sys --features ffi --lib glm_tiny_ffi_
# glm_tiny_ffi_pump_visual_nonempty ... ok
# glm_tiny_ffi_mid_generate_cooperative_cancel ... ok

cargo test -p colibri-sys --features ffi --lib
# 105 passed

cargo test -p colibri-native --features ffi
# 78 passed

cargo clippy -p colibri-sys --features ffi --all-targets -- -D warnings  # exit 0
cargo clippy -p colibri-native --features ffi --all-targets -- -D warnings  # exit 0
cargo fmt -p colibri-sys -p colibri-native
```

`glm_tiny` open smoke fills HWINFO (cores>0), TIERS, EMAP dims/cells after open. PROF remains empty until a completed generate (as designed).

---

## What landed

### 1. Shared binary decode (`visual.rs`)

- `pack_expert_cell` / `unpack_expert_cell` moved to always-available visual module (process mux re-exports).
- `ExpertMap::from_cells`, `ExpertHits::from_bits` (no hex required).
- `BinaryPollParts` + `VisualSnapshot::absorb_binary_poll` (profile append only when `seq` advances; cap 120 turns).
- Fixture unit test (no subprocess).

### 2. FFI bindings (`ffi/bindings.rs`)

- `COLI_VISUAL_*` want flags
- `ColiHwinfoSnap`, `ColiTiersSnap`, `ColiExpertGridDims`, `ColiProfSnap`
- `coli_glm_visual_poll`, `coli_kimi_visual_poll`, `coli_ink_visual_poll`

### 3. FfiEngine pump (`ffi/multi.rs`)

- Cached `VisualSnapshot` on `GlmEngine` / `KimiEngine` / `InkEngine`
- `poll_visual_into`: size probe (null buffers) then fill; map C → `BinaryPollParts` → absorb
- `FfiEngine::pump_visual` / `visual_snapshot` (DeepSeek V4 → empty until symbols exist)
- GLM fill is real; Kimi/Inkling call stubs (empty success)

### 4. Native (`host.rs` / `main.rs`)

- `LiveEngine::Ffi` `pump_visual` locks engine and calls `FfiEngine::pump_visual`
- Status copy no longer claims “Brain needs engine process”; says live visual poll (GLM)
- Cooperative cancel unchanged: `cancel` AtomicBool in token callback

### 5. STOP / cancel

- Process mid-stream cancel tests still present (`engine::tests::mid_stream_cancel_no_deadlock`)
- New FFI regression: `glm_tiny_ffi_mid_generate_cooperative_cancel` (`generate_ids` + token-cb `Err` after first token)

No mux multi-slot STOP invented for FFI.

---

## Files changed

| Path | Change |
|------|--------|
| `crates/colibri-sys/src/visual.rs` | Binary absorb API + fixture tests; pack/unpack |
| `crates/colibri-sys/src/lib.rs` | Re-export new visual helpers |
| `crates/colibri-sys/src/engine/mod.rs` | Re-export pack/unpack from visual |
| `crates/colibri-sys/src/engine/serve.rs` | Re-export pack/unpack (no local dup) |
| `crates/colibri-sys/src/ffi/bindings.rs` | Visual C types + poll decls |
| `crates/colibri-sys/src/ffi/multi.rs` | `pump_visual`, poll helper, soft glm_tiny tests |
| `crates/colibri-native/src/host.rs` | FFI pump wire + status strings |
| `crates/colibri-native/src/main.rs` | Ready status for in-process |
| `crates/colibri-sys/docs/ffi-phase-d.md` | Light update: visual poll GLM-wired (residual close separate) |

---

## Residual note (do **not** close here)

**`open:ffi-visual-abi`** product residual is **ready for Step docs** (next agent):

- C ABI (Step 1) + Rust/native wire (this step) are in tree and green under verification commands above.
- Docs residual should update residual trackers / user-facing notes and close the open item if product acceptance matches.
- Still out of scope for this residual as designed: product default flip, GPU embed, DeepSeek V4 visual symbols, full Kimi/Inkling fill (stubs OK), mux multi-slot STOP on FFI.

---

## Constraints respected

- Product default still process (`prefer_process` / no default flip)
- No GPU work
- No git commit / git add
- Cooperative cancel only on FFI
- fmt + clippy (`-D warnings`) + tests on touched packages
