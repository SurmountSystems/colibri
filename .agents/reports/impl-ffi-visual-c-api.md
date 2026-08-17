# Report: C visual poll ABI (`open:ffi-visual-abi` Step 1)

**Date:** 2026-08-10
**Scope:** C embed poll/snapshot ABI for HWINFO / EMAP / HITS / PROF / TIERS (no SERVE subprocess). GLM implemented; Kimi/Inkling stub empty success.

---

## ABI design summary

Prefer **poll** over an in-process stdout mux. Hosts call `coli_*_visual_poll` on a timer or after generate; STOP stays cooperative via `ColiTokenFn` non-zero return (no mux multi-slot STOP).

### Header (`c/colibri_api.h`)

| Piece | Role |
|-------|------|
| `COLI_VISUAL_HWINFO/TIERS/EMAP/HITS/PROF/ALL` | `want` bitset |
| `ColiHwinfoSnap` | cores, RAM GB, gpus, VRAM GB, cpu/gpu names (128-byte strings) |
| `ColiTiersSnap` | vram/ram/disk expert counts + GB |
| `ColiExpertGridDims` | EMAP/HITS rows × cols |
| `ColiProfSnap` | same fields as mux `PROF` line + `seq` + `valid` |
| `coli_glm_visual_poll(...)` | full fill for GLM |
| `coli_kimi_visual_poll` / `coli_ink_visual_poll` | stub: return 0, zeros |

### Return codes

| Code | Meaning |
|------|---------|
| `0` | success (including empty data before first generate) |
| `-1` | bad args / engine not open |
| `-2` | caller buffer too small (`*emap_cells_len` / `*hits_bits_len` set to needed) |

### Binary layouts (match `c/telemetry.h` + process lines)

| Kind | Layout |
|------|--------|
| **EMAP** | `rows * cols` bytes, row-major. Cell: `(tier << 6) \| heat`. tier: 0 disk, 1 RAM, 2 VRAM; heat: log2-bucket usage 0..63 |
| **HITS** | `ceil(rows*cols/8)` bytes. Expert index `i` → bit `(i & 7)` of byte `(i >> 3)` (little-endian bits). **Destructive:** successful HITS fill clears hit marks (same as `hits_emit`) |
| **PROF** | last completed embed generate: wall_s, prompt_tokens, completion_tokens, expert_disk_s, expert_wait_s, expert_matmul_s, attention_s, lm_head_s, forwards; `valid=1` after first generate; `seq` increments per generate |
| **HWINFO / TIERS** | same fields as process `HWINFO` / `TIERS` lines (no hex) |

Shared fill helpers live in `c/telemetry.h` (`hwinfo_fill`, `tiers_fill`, `emap_fill`, `hits_fill`); process `*_emit` wrappers call them so SERVE and FFI stay one layout.

### PROF on embed generate

`coli_glm_generate` / `coli_glm_generate_ids` snapshot `ProfBase` at start and write `ColiGlmEngine.last_prof` at end (same deltas as `mux_done` PROF).

---

## Files changed

| Path | Change |
|------|--------|
| `c/colibri_api.h` | Visual types, want flags, `coli_*_visual_poll` decls |
| `c/telemetry.h` | Fill helpers; emit wrappers reuse them |
| `c/colibri.c` | GLM poll + PROF record on generate |
| `c/kimi_k3.c` | Kimi poll stub |
| `c/inkling.c` | Inkling poll stub |
| `c/tests/test_visual_poll_api.c` | Null-engine + packing oracle + optional glm_tiny smoke |

---

## How to build

```bash
make -C c libcolibri      # GLM: coli_glm_visual_poll
make -C c libkimi_k3     # stub coli_kimi_visual_poll
make -C c libinkling     # stub coli_ink_visual_poll

# unit smoke
make -C c libcolibri
cd c && gcc -O2 -fopenmp -pthread -I. tests/test_visual_poll_api.c \
  -o tests/test_visual_poll_api libcolibri.a -lm -fopenmp -pthread
./tests/test_visual_poll_api
COLIBRI_VISUAL_SMOKE_DIR=./glm_tiny ./tests/test_visual_poll_api
```

Symbols: `nm -g c/libcolibri.a | grep coli_glm_visual_poll` → `T coli_glm_visual_poll`.

---

## What Rust must wire next (Step 2)

1. **bindgen / manual FFI** for the new structs and `coli_glm_visual_poll` (and stub symbols so link of multi-family matrix stays clean).
2. **`FfiEngine` / `GlmEngine`:** `poll_visual()` → fill existing `VisualSnapshot` (`crates/colibri-sys/src/visual.rs`):
   - `HwinfoSnap` ← `ColiHwinfoSnap`
   - `TiersSnap` ← `ColiTiersSnap` (`vram` ← `vram_experts`, etc.)
   - `ExpertMap { rows, cols, cells }` ← raw EMAP bytes (no hex)
   - `ExpertHits { rows, cols, bits, seq }` ← HITS bytes + `hits_seq`
   - `ProfileTurn` push when `prof.valid` and `prof.seq` advances (`profile_seq`)
3. **`pump_visual` on FFI path** (native `LiveEngine::Ffi`): call poll instead of empty snapshot.
4. **Reuse** `pack_expert_cell` / unit tests in `visual.rs` / `serve.rs` for byte contracts; prefer binary buffer over re-hexing.
5. **Do not** invent mux STOP for FFI; keep token-callback cancel.
6. Kimi/Inkling: treat empty poll as “no visual yet” until fill is implemented.

### Fixed fixtures for Rust unit tests (no subprocess)

```text
# EMAP 1x2 hex oracle (process line form) == raw cells
cells: [0x43, 0x80]   # tier1 heat3, tier2 heat0
hex:   "4380"
# HITS experts 0 and 3 set
bits:  [0x09]
hex:   "09"
```

Rust red test (Step 2): decode these into non-empty `VisualSnapshot` without spawning SERVE; green when FFI poll maps the same bytes into those types.

---

## Test evidence

| Command | Result |
|---------|--------|
| `make -C c libcolibri` | exit 0 (pre-existing unused-var warnings only) |
| `make -C c libkimi_k3 libinkling` | exit 0; stubs exported |
| `make -C c colibri` | exit 0 (CLI still builds with fill helpers) |
| `./tests/test_visual_poll_api` | `test_visual_poll_api: ok` (packing + null engine) |
| `COLIBRI_VISUAL_SMOKE_DIR=./glm_tiny ./tests/test_visual_poll_api` | `smoke ok: cores=16 emap 2x8 (16 bytes) hits_len=2 prof_valid=0` |
| generate_ids + poll (glm_tiny, 2 tokens) | `rc=0 prof_valid=1 seq=1 wall≈0.5 prompt=4 completion=2 emap=2x8 hits_seq=1` |

---

## Out of scope (this step)

- Rust bindgen / native `pump_visual` wire-up (Step 2)
- Product default flip / GPU embed
- DeepSeek V4 visual symbols (separate header; not required for GLM-first)
- Mux multi-slot STOP on FFI
