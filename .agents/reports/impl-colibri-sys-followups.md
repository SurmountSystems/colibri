# Implement report: colibri-sys residual follow-ups

**Date:** 2026-08-10
**Workspace:** `/home/hunter/Projects/surmount/colibri`
**Crate:** `crates/colibri-sys`
**Plan:** session plan *colibri-sys residual follow-ups* (revised: **no desktop**)

Desktop `src-tauri/**` was not touched.

## Map of changes

| Slice | Paths | What landed |
|-------|--------|-------------|
| 1 Deep doctor | `src/doctor.rs` | Port of `c/doctor.py` deep path: `safetensors_header`, `tensor_layout`, `shard_sequence_report`, `deep_container_report`, wired when `DoctorOptions.deep`. Mode `"deep"`. Checks: `model.container`, `model.shard_sequence`, `model.required`, `model.index`, `storage.mirror`. Synthetic U8 fixtures in unit tests. |
| 2 Windows RAM | `src/probe.rs` | Real `GlobalMemoryStatusEx` → `ullAvailPhys` + `GetPhysicallyInstalledSystemMemory` fallback via kernel32 FFI. Pure `windows_memory_available_with` unit-tested off Windows; `cfg(windows)` host test for non-zero. |
| 3 Multi-shard HF | `src/model/install.rs` | Full snapshot: mockable `HfCliRunner`, `hf download --include`, hf-hub `info().siblings` + `get` per allow_pattern, progress phases, incomplete detection, post `ModelInfo::inspect` + optional registry register. Live network test `#[ignore]`. |
| 4 rkyv bytecheck | `src/stream/codec.rs`, `stream/mod.rs`, `lib.rs`, `Cargo.toml` | `decode_frame_checked` / `decode_server_frame_checked` / `read_frame_with`; trust model documented. rkyv `bytecheck` feature explicit. |
| 5 Phase D FFI | `docs/ffi-phase-d.md`, `src/lib.rs` ffi stub, README | Design spike: required exports, V4 experimental API note, re-exec kill-switch, acceptance criteria. `ffi_available() == false`. |
| 6 Docs | `README.md` | Residual list updated; deep doctor, stream trust, install, FFI residual rows. |

### Python → Rust citations (this wave)

| Python | Rust |
|--------|------|
| `doctor.deep_container_report`, `_safetensors_header`, `_tensor_layout`, `_shard_sequence_report` | `doctor::{deep_container_report, safetensors_header, tensor_layout, shard_sequence_report}` |
| `doctor.run_doctor` deep branch | `run_doctor` when `opts.deep` |
| `resource_plan.memory_available` win32 | `probe::windows_memory_available` |

## Test commands + exit codes

```text
cargo fmt -p colibri-sys                                          # exit 0
cargo clippy -p colibri-sys --all-targets --features install -- -D warnings  # exit 0
cargo test -p colibri-sys                                         # exit 0
  lib: 41 passed
  plan_golden: 2 passed
  ssd_cache_vectors: 1 passed
  engine_real: 1 ignored
  doctests: 1 passed
cargo test -p colibri-sys --features install                      # exit 0
  lib: 48 passed, 1 ignored (live_hf_snapshot_tiny)
  (+ same integration/doctests as above)
```

## Remaining gaps

1. **SSD `st_dev` trust** on non-unix still always foreign for v2.
2. **Sister-engine plan math** still GLM-shaped (same as Python).
3. **Desktop** dependency on `colibri-sys` still deferred (plan revise).
4. **Real `libcolibri`**: design only; no C no-main extract in this wave.
5. **Windows GPU/ROCm** discovery untested on Windows hosts (RAM path is real).
6. **Deep doctor** does not reject duplicate JSON keys the way Python `object_pairs_hook` does (serde_json last-wins); not required for the plan contracts exercised here.
7. **hf-hub** path needs network for real multi-hundred-GB pulls; production large installs still prefer `hf` CLI when available.

## Operator notes

- No desktop edits.
- No git mutations.
- Red/green covered for deep shard index, Windows memory selection, mock multi-shard install, corrupt rkyv fail-closed.
