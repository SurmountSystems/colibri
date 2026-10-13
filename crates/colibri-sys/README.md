# colibri-sys

Embeddable **Rust host crate** for Colibrì.

| | |
|--|--|
| Edition / MSRV | **2024** / **1.85** |
| Version | `0.1.0` (see `Cargo.toml`) |
| Publish status | **Not on crates.io yet** (path or git dependency only) |
| Repo root (this machine) | `/home/hunter/Projects/surmount/colibri` |

## What it is / is not

**Is:** a process-first host that configures, probes, plans, checks, downloads
(optional), spawns, and streams around Colibrì **C engine subprocesses**
(`colibri`, `inkling`, `kimi_k3`, `deepseek_v4`). Your app links this crate;
the engine stays a separate binary on `PATH` or under `c/`.

**Is not (by default):**

- In-process engines as the library default (crate `prefer_process = true`).
  Multi-family **CPU-only** static (GLM / Kimi / Inkling / V4) is feature `ffi`
  ([docs/ffi-phase-d.md](docs/ffi-phase-d.md)); opt-in Linux CUDA for GLM is
  feature `ffi-cuda`; opt-in Linux HIP/ROCm for GLM is feature `ffi-hip`
  (one GPU vendor per binary). APU/UMA placement and process HIP: see
  [GPU_BACKENDS.md](../../GPU_BACKENDS.md). Size metadata is always on
  `ModelInfo` / `ModelSizeInfo`
- A crates.io package you can write as `colibri-sys = "0.1"` today
- A replacement for the product C engines or the Python OpenAI gateway

## Documentation

| Doc | Path |
|-----|------|
| **[User guide](docs/user-guide.md)** (Grok Build local path + HTTP harness §15) | `crates/colibri-sys/docs/user-guide.md` |
| **[Doc index](docs/README.md)** | `crates/colibri-sys/docs/README.md` |
| **[Phase D FFI](docs/ffi-phase-d.md)** | Multi-family CPU static opt-in; process still default |
| rustdoc | `cargo doc -p colibri-sys --no-deps --features install --open` |

```bash
cd /home/hunter/Projects/surmount/colibri
less crates/colibri-sys/docs/user-guide.md
cargo doc -p colibri-sys --no-deps --features install --open
```

## Not on crates.io yet — use a path dependency

Do **not** write `colibri-sys = "0.1"` expecting the registry. Point Cargo at
this tree until a publish exists.

### Absolute path (this machine / Grok Build host app)

```toml
# In the consumer Cargo.toml (e.g. Grok Build or another host app)
colibri-sys = { path = "/home/hunter/Projects/surmount/colibri/crates/colibri-sys", features = ["runtime", "stream"] }
```

### Portable forms

```toml
# Relative from your consumer crate to this repo
# colibri-sys = { path = "../colibri/crates/colibri-sys", default-features = true }

# Or set COLIBRI_ROOT and expand in docs / scripts; Cargo itself needs a real path:
# colibri-sys = { path = "/path/to/colibri/crates/colibri-sys" }
```

### Workspace member (same monorepo)

```toml
# colibri/Cargo.toml
[workspace]
members = ["crates/colibri-sys", "your-app"]

# your-app/Cargo.toml
[dependencies]
colibri-sys = { path = "../colibri-sys" }
```

### Optional git form (out-of-tree, when you pin a rev)

```toml
colibri-sys = { git = "https://github.com/SurmountSystems/colibri", rev = "<commit>", package = "colibri-sys" }
```

Path layout inside the git tree is still `crates/colibri-sys`. Adjust the
`package` / path if your fork layout differs.

**Rebuild note:** after you change this crate, rebuild the consumer
(`cargo build` / your usual Grok Build rebuild). Path deps recompile when
sources under the path change.

**Engine still required:** linking `colibri-sys` does not embed the C kernels.
Build or install `c/colibri` (or the sister engines), set `COLI_ENGINE` /
`ColibriConfig::engine`, or rely on `locate_engine` after an in-tree build.

## Features

| Feature | Default | Purpose |
|---------|---------|---------|
| `runtime` | on | process spawn + serve mux (`EngineHandle`) |
| `stream` | on | rkyv duplex frames + codec |
| `tokio` | on | async duplex helpers (requires `runtime`) |
| `install` | off | HF multi-shard snapshot (`hf` CLI or `hf-hub`) |
| `ffi` | off | Multi-family CPU static (GLM/Kimi/Inkling/V4; [Phase D](docs/ffi-phase-d.md)); process still default |

Minimal probe/plan/doctor (no engine process):

```toml
colibri-sys = { path = "...", default-features = false }
```

Full host + downloads:

```toml
colibri-sys = { path = "...", features = ["install"] }  # keeps defaults
```

## Quick start

```rust
use colibri_sys::{ColibriConfig, MachineInfo, PlacementPlan, PlanOptions};

let machine = MachineInfo::probe()?;
let plan = PlacementPlan::build("/path/to/model", &PlanOptions {
    available_memory: Some(machine.available_memory),
    gpus: Some(machine.gpus),
    physical_cpus: Some(machine.physical_cores),
    ..Default::default()
})?;
let env = ColibriConfig::default()
    .model("/path/to/model")
    .apply_plan(&plan);
```

### Public machine inventory (`MachineInfo`)

| Area | Public fields / types |
|------|------------------------|
| RAM | `total_memory`, `available_memory` |
| Swap | `swap_total`, `swap_free` |
| Cores | `physical_cores`, `logical_cores`, `sockets`, `cpu.threads_per_core` |
| CPU identity | `cpu.architecture`, `vendor`, `model_name`, `family`, `model`, `stepping`, `generation_hint` |
| big.LITTLE | `cpu.big_little` (`BigLittleInfo`) |
| SIMD / ISA | `cpu.simd` (`SimdFeature`: AVX512*, NEON, …), `cpu.isa_flags` |
| GPUs / NPUs | `gpus`, `npus` (e.g. XDNA / Ryzen AI) |
| Model store | `model_store` (`path`, `source`, `free_bytes`, `total_bytes`) |
| Host libs | `host_libraries` |

Override store path with `ProbeOptions { model_store: Some(path), .. }`, or
`MachineInfo::probe_for_config(&cfg)` / `ProbeOptions::from_config(&cfg)` when
using `ColibriConfig.model_store`. Full field tables:
[user guide §2](docs/user-guide.md).

### Doctor

```rust
use colibri_sys::{DoctorOptions, exit_code, run_doctor};

let report = run_doctor("/path/to/model", &DoctorOptions {
    deep: true,
    available_memory: Some(64 * colibri_sys::GB),
    ..Default::default()
})?;
assert_eq!(report.mode, "deep");
std::process::exit(exit_code(&report));
```

### Examples

```bash
# Probe + plan only (no engine binary required)
cargo run -p colibri-sys --example plan_probe -- /path/to/model

# Embed a real engine (from repo root after building c/colibri)
export COLIBRI_TEST_ENGINE=./c/colibri
export COLIBRI_TEST_MODEL=./c/glm_tiny
cargo run -p colibri-sys --example embed_chat
```

### Grok Build as a chat_completions backend

Prefer **HTTP** (product Python gateway or a thin Rust harness) over linking
Grok Build into this crate. Copy-paste checklist: [user guide §15](docs/user-guide.md).
Local path wiring for any Cargo consumer is in
[user guide §1](docs/user-guide.md) and the path section above.

## Python → Rust ports

| Python / C host | Rust module |
|-----------------|-------------|
| `c/resource_plan.py` | `probe`, `plan` |
| `c/doctor.py` (standard + deep) | `doctor` |
| `c/coli` (`model_arch`, `engine_for`) | `model` |
| `c/openai_server.py` Engine mux | `engine::serve` |
| HF download scripts | `model::install` (feature `install`) |

C engines stay C. Torch quant convert may still shell out to `c/tools/*.py`
(`model::install::convert_subprocess`).

## Stream trust model

| API | Validation |
|-----|------------|
| `decode_frame` | Trusted local pipes |
| `decode_frame_checked` | Untrusted buffers (rkyv bytecheck; fails closed) |

## Tests

```bash
cargo test -p colibri-sys
cargo test -p colibri-sys --features install
COLIBRI_TEST_ENGINE=./c/colibri COLIBRI_TEST_MODEL=./c/glm_tiny \
  cargo test -p colibri-sys -- --ignored
```

## Product contracts (repo root)

| Topic | Path |
|-------|------|
| Serve mux | `docs/serve_protocol.md` |
| CLI / env | `docs/SETTINGS.md`, `docs/ENVIRONMENT.md` |
| Telemetry packing | `c/telemetry.h` |

## Quality doctrine

Placement changes **speed**, not answers (except `experimental-fast`).
Keep plan behavior lockstep with `coli plan --json` / resource plan tests.

## Residual

| Gap | Notes |
|-----|--------|
| Product-default in-process engine | Multi-family CPU FFI opt-in only; process default; see [Phase D](docs/ffi-phase-d.md) |
| Desktop dep | Deferred |
| Sister-engine plan math | GLM-shaped (same as Python) |
| SSD `st_dev` on non-unix | Foreign for v2 |
| Windows GPU discovery | RAM path real; GPU less tested |
