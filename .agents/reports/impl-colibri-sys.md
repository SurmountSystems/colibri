# Implement report: colibri-sys

**Date:** 2026-08-09
**Workspace:** `/home/hunter/Projects/surmount/colibri`
**Crate:** `crates/colibri-sys` (edition 2024, rust-version 1.85)

## What landed

Root workspace + embeddable host crate with plan phases A–C (process embed path). Desktop `src-tauri` untouched.

### Layout

```
Cargo.toml                          # workspace members = crates/colibri-sys
crates/colibri-sys/
  Cargo.toml
  README.md
  src/
    lib.rs
    error.rs
    config.rs                       # ColibriConfig, EnvMap, setdefault, apply_plan
    probe.rs                        # MachineInfo, GPUs, SSD grammar
    plan.rs                         # PlacementPlan v2 + environment_for_plan
    doctor.rs                       # standard path schema_version 1
    visual.rs                       # ExpertMap/Hits, tiers, hw, profile, Subscribe
    model/
      mod.rs                        # ModelInfo::inspect, family routing
      registry.rs                   # ModelRegistry scan/register
      install.rs                    # feature install: hf CLI / hf-hub
    engine/
      mod.rs                        # EngineHandle
      locate.rs                     # COLI_ENGINE / libexec / in-tree c/
      serve.rs                      # SERVE_BATCH mux client + mock test
    stream/
      mod.rs / frame.rs / codec.rs  # rkyv ClientFrame/ServerFrame + length-prefix
      session.rs                    # Tokio DuplexSession
  tests/
    ssd_cache_vectors.rs            # shared C/Python vectors
    plan_golden.rs                  # fixed geometry plan + env
    engine_real.rs                  # #[ignore] real engine
    fixtures/ssd_cache_vectors.txt
  examples/
    plan_probe.rs
    embed_chat.rs
```

### Features

| Feature | Default | Status |
|---------|---------|--------|
| `runtime` | on | spawn + serve mux |
| `stream` | on | rkyv frames + codec |
| `tokio` | on | duplex session |
| `install` | off | HF download orchestration |
| `ffi` | off | stub (`ffi_available() == false`) |

## Python → Rust map

| Python / source | Rust |
|-----------------|------|
| `c/resource_plan.py` (`memory_available`, cores, GPUs, `parse_ssd_cache`, `build_plan`, `environment_for_plan`, `_auto_tune`) | `probe`, `plan` |
| `c/doctor.py` (`run_doctor` standard) | `doctor` (deep = skip note) |
| `c/coli` (`model_arch`, `engine_for`) | `model` |
| `c/openai_server.py` (`Engine` mux) | `engine::serve` |
| `c/telemetry.h` packing | `visual` + `pack_expert_cell` |
| HF download scripts | `model::install` (feature) |

## C tools still invoked as subprocess

- Engine binaries: `colibri`, `inkling`, `kimi_k3`, `deepseek_v4` (locate + spawn with `SERVE`/`SERVE_BATCH`/`SNAP`/`COLI_NO_OMP_TUNE`)
- Discovery helpers: `nvidia-smi`, `rocm-smi`, `lscpu`, `df`
- Optional: `ldd` (doctor linkage), `hf` CLI (install feature)
- Quant convert: `convert_subprocess` → existing `c/tools/*.py` (documented last resort; not default)

## Test commands + results

```text
cargo fmt -p colibri-sys                          # exit 0
cargo clippy -p colibri-sys --all-targets -- -D warnings   # exit 0
cargo test -p colibri-sys                         # exit 0
  lib: 26 passed
  plan_golden: 2 passed
  ssd_cache_vectors: 1 passed
  engine_real: 1 ignored
  doctests: 1 passed
cargo test -p colibri-sys --features install      # exit 0 (+ install local unit test)
```

## How to try examples

```bash
# Probe + optional plan (no engine required)
cargo run -p colibri-sys --example plan_probe -- /path/to/model

# Real engine (build c/colibri first)
export COLIBRI_TEST_ENGINE=./c/colibri
export COLIBRI_TEST_MODEL=./c/glm_tiny
cargo run -p colibri-sys --example embed_chat
cargo test -p colibri-sys --test engine_real -- --ignored
```

## Known gaps / follow-ups

1. **Deep doctor** (`deep_container_report`) not ported; standard path + skip note for `--deep`.
2. **Windows** RAM probe is stubbed (returns 0; plan floors apply). GPU/ROCm untested on this host.
3. **SSD st_dev trust** on non-unix is always foreign for v2 (no volume dev).
4. **Model install** full multi-hundred-GB snapshot is CLI-first; `hf-hub` path only pulls metadata files as a minimal fallback.
5. **rkyv decode** uses trusted `access_unchecked` + aligned copy (length-prefix offset); add bytecheck for untrusted transports later.
6. **Sister-engine plan math** is GLM-shaped (same as Python); K3/Inkling/V4 get inspect + env passthrough.
7. **Desktop dep** not wired (optional per plan Q2).
8. **Phase D FFI** remains stub only.

## Operator notes honored

- Python host logic rewritten in Rust with port citation comments.
- C engines stay subprocesses.
- Unit + integration tests (mock mux peer; real engine behind `#[ignore]` + env).
- No git commit / no desktop identity change.
