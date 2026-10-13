# colibri-sys user guide

**colibri-sys** is the Rust host crate for embedding [Colibrì](https://github.com/SurmountSystems/colibri): machine probe, placement plan, model registry and install, doctor checks, supervised C engine process, visual telemetry, and optional rkyv duplex frames.

Inference kernels stay in **C binaries** by default for library embeds (`colibri`, `inkling`, `kimi_k3`, `deepseek_v4` on the serve mux; `ColibriConfig.prefer_process = true`). Optional Cargo feature `ffi` **links multi-family CPU static libs** (`libcolibri.a`, `libkimi_k3.a`, `libinkling.a`, `libdeepseek_v4.a`) and open by family (`ffi::open_engine`). The **colibri-native** desktop host, when built with `feature = "ffi"`, defaults to try FFI first (process fallback on open failure; kill-switch `COLIBRI_FORCE_PROCESS`). Embed open runs the same RAM clamp as the CLI (`cap_for_ram`, about 88% of available RAM) and returns an error instead of `exit(2)` when even one expert slot cannot fit. Native Start refuses before open in that case. `COLI_RAM_OVERCOMMIT=1` overrides the refuse (same as CLI). Model size is on public types (`ModelInfo::disk_bytes`, `ModelSizeInfo`). See [ffi-phase-d.md](ffi-phase-d.md).

## Paths on this machine

Repository **working tree** (run all `cargo` / relative paths from here unless noted). On other machines, replace the absolute prefix with your clone root (or treat `$COLIBRI_ROOT` as that root in scripts; Cargo still needs a real path string).

| Role | Absolute path |
|------|----------------|
| **PWD / repo root** | `/home/hunter/Projects/surmount/colibri` |
| Crate root | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys` |
| This user guide | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/user-guide.md` |
| Doc index | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/README.md` |
| Crate README | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/README.md` |
| Phase D FFI | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/ffi-phase-d.md` |
| Generated rustdoc (after `cargo doc`) | `/home/hunter/Projects/surmount/colibri/target/doc/colibri_sys/index.html` |
| C engine tree | `/home/hunter/Projects/surmount/colibri/c` |
| Product OpenAI gateway (Python) | `/home/hunter/Projects/surmount/colibri/c/openai_server.py` |
| Serve protocol | `/home/hunter/Projects/surmount/colibri/docs/serve_protocol.md` |
| OpenAI API notes | `/home/hunter/Projects/surmount/colibri/docs/api.md` |
| Grok Build custom models (host) | `/home/hunter/.grok/docs/user-guide/11-custom-models.md` |
| Grok config | `/home/hunter/.grok/config.toml` |

`file://` URLs for the browser (this machine):

- User guide: `file:///home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/user-guide.md`
- rustdoc: `file:///home/hunter/Projects/surmount/colibri/target/doc/colibri_sys/index.html`

| | |
|--|--|
| Crate path (repo-relative) | `crates/colibri-sys/` |
| Edition / MSRV | 2024 / 1.85 |
| Version | see `Cargo.toml` (`0.1.0` as of this guide) |
| Publish status | **Not on crates.io yet** (path or git only) |
| Doc index | [docs/README.md](README.md) |
| **Grok Build harness** | [§15](#15-grok-build-style-completion-harness) |
| **Local path for Grok Build / other Cargo apps** | [§1](#1-add-the-dependency) and [§1.1](#11-grok-build-local-integration-path-dependency) |

---

## 1. Add the dependency

### Not on crates.io yet — use a path dependency

There is **no** crates.io release of `colibri-sys` to pull with
`colibri-sys = "0.1"`. Depend on the clone with a **path** dependency (or git
with a pinned rev). You need this repository on disk next to (or absolute from)
your consumer project.

### Absolute path (this machine)

```toml
# In the consumer Cargo.toml
colibri-sys = { path = "/home/hunter/Projects/surmount/colibri/crates/colibri-sys", features = ["runtime", "stream"] }
# or relative from consumer:
# colibri-sys = { path = "../colibri/crates/colibri-sys", default-features = true }
```

Repo root on this machine: `/home/hunter/Projects/surmount/colibri`.

### Portable relative / `$COLIBRI_ROOT`

```toml
# Relative from your app crate to the colibri clone (adjust depth as needed)
colibri-sys = { path = "../colibri/crates/colibri-sys", default-features = true }

# Cargo does not expand env vars in path =. In docs and shell scripts you can
# write: $COLIBRI_ROOT/crates/colibri-sys  (export COLIBRI_ROOT=/path/to/colibri)
# then put the expanded absolute path into your consumer Cargo.toml.
```

### Workspace member (same monorepo)

```toml
# /home/hunter/Projects/surmount/colibri/Cargo.toml  (or your monorepo root)
[workspace]
members = ["crates/colibri-sys", "your-app"]

# your-app/Cargo.toml
[dependencies]
colibri-sys = { path = "../colibri-sys" }
```

### Optional git dependency

```toml
colibri-sys = {
  git = "https://github.com/SurmountSystems/colibri",
  rev = "<commit>",
  package = "colibri-sys",
}
```

Use a real commit hash. Layout inside the tree remains `crates/colibri-sys`.

### Features

| Feature | Default | What you get |
|---------|---------|----------------|
| `runtime` | on | Locate/spawn C engine, serve mux, `EngineHandle` |
| `stream` | on | rkyv `ClientFrame` / `ServerFrame`, encode/decode |
| `tokio` | on | Async duplex session helpers (requires `runtime`) |
| `install` | **off** | Hugging Face multi-shard download orchestration |
| `ffi` | **off** | Opt-in multi-family **CPU-only** static link (GLM / Kimi / Inkling / V4); process still default ([ffi-phase-d.md](ffi-phase-d.md)) |
| `ffi-cuda` | **off** | Implies `ffi`; Linux CUDA GLM embed when toolkit present |
| `ffi-hip` | **off** | Implies `ffi`; Linux HIP/ROCm GLM embed when ROCm present (exclusive vs `ffi-cuda`) |

Minimal probe/plan/doctor (no engine process):

```toml
colibri-sys = { path = "...", default-features = false }
```

Full host + downloads:

```toml
colibri-sys = { path = "...", features = ["install"] }  # keeps default features
```

### Engine binaries still required

A path dependency only gives you the **Rust host**. Inference needs built Colibrì
artifacts (for example `c/colibri` after `make -C c colibri` or product install)
on `PATH`, via `COLI_ENGINE`, or via `ColibriConfig::engine` /
`locate_engine`. Without an engine binary, probe/plan/doctor still work;
`EngineHandle` does not.

Example that only probes and plans:

```bash
cd /home/hunter/Projects/surmount/colibri
cargo run -p colibri-sys --example plan_probe -- /path/to/model
```

### 1.1 Grok Build local integration (path dependency)

Use this when another **Grok Build** (or host app) Cargo project should call
`colibri-sys` APIs (probe, plan, model store, doctor, or a harness under §15).

1. Clone or keep this repo at a known path. Operator machine default:

   `/home/hunter/Projects/surmount/colibri`

2. In the **consumer** `Cargo.toml` (not this crate’s):

```toml
[dependencies]
# In the consumer Cargo.toml
colibri-sys = { path = "/home/hunter/Projects/surmount/colibri/crates/colibri-sys", features = ["runtime", "stream"] }
# or relative from consumer:
# colibri-sys = { path = "../colibri/crates/colibri-sys", default-features = true }
```

3. Features to enable:

   | Goal | Features |
   |------|----------|
   | Default process host + stream | `runtime`, `stream`, `tokio` (all default on) |
   | Explicit minimal engine embed | `features = ["runtime", "stream"]` (or default-features true) |
   | HF multi-shard install | add `"install"` |
   | In-process FFI | optional `"ffi"` + `prefer_process = false` for library embeds; colibri-native with `feature = "ffi"` defaults FFI-first |

4. Rebuild the consumer after changes under `crates/colibri-sys` (path deps
   recompile when those sources change).

5. Probe / plan / model store in code:

```rust
use colibri_sys::{ColibriConfig, MachineInfo, PlacementPlan, PlanOptions, ProbeOptions};

// Discoverable inventory (RAM, GPUs, model_store volume, …)
let machine = MachineInfo::probe()?;

// Force model-store volume (free-space + install default)
let cfg = ColibriConfig::default().model_store("/data/colibri-models");
let machine = MachineInfo::probe_for_config(&cfg)?;
// or: MachineInfo::probe_with(&ProbeOptions { model_store: Some(path), .. })

let plan = PlacementPlan::build("/path/to/model", &PlanOptions {
    available_memory: Some(machine.available_memory),
    gpus: Some(machine.gpus.clone()),
    physical_cpus: Some(machine.physical_cores),
    ..Default::default()
})?;
```

6. For **chat completions into Grok Build**, prefer HTTP custom models (§15 Path
   A/B), not linking Grok’s agent loop into this crate. Path dep above is for a
   harness binary or in-tree tool that owns `EngineHandle`.

7. Full completions-via-gateway copy-paste: [§15](#15-grok-build-style-completion-harness).

---

## 2. Probe the machine (full inventory)

The public inventory lives on **`MachineInfo`** and nested public types, all
re-exported from the crate root (`lib.rs`). Fields are public; dependents read
them directly.

### Public API field list (`MachineInfo`)

| Field | Type | Meaning |
|-------|------|---------|
| `total_memory` | `u64` | Total physical RAM (bytes) |
| `available_memory` | `u64` | Reclaimable RAM without swapping (bytes) |
| `swap_total` | `u64` | Total swap / pagefile (bytes; 0 if none/unknown) |
| `swap_free` | `u64` | Free swap / pagefile (bytes) |
| `physical_cores` | `u32` | Physical CPU cores (not SMT siblings) |
| `logical_cores` | `u32` | Logical CPUs (hardware threads / SMT) |
| `sockets` | `u32` | CPU sockets |
| `cpu` | `CpuInfo` | Architecture, vendor, model, generation, hybrid, SIMD |
| `gpus` | `Vec<GpuDevice>` | Discrete GPUs (NVIDIA and/or AMD/ROCm; multi-vendor inventory) |
| `npus` | `Vec<NpuDevice>` | NPUs / AI accelerators (e.g. AMD XDNA / Ryzen AI). **Inventory only** — NPU inference is not driven by this crate yet |
| `model_store` | `ModelStoreVolume` | Default/override store path + free/total bytes |
| `host_libraries` | `Vec<HostLibrary>` | Relevant shared libs (CUDA, ROCm/HIP, XRT, Vulkan, …) |
| `disk_free` | `Option<u64>` | Legacy extra free-space probe (prefer `model_store`) |
| `disk_path` | `Option<PathBuf>` | Path for legacy free-space probe |

#### Nested: `CpuInfo`

| Field | Type | Meaning |
|-------|------|---------|
| `architecture` | `String` | Kernel arch (`x86_64`, `aarch64`, …) |
| `vendor` | `Option<String>` | CPU vendor string |
| `model_name` | `Option<String>` | Marketing / brand string |
| `family` / `model` / `stepping` | `Option<u32>` | CPUID identity when known |
| `generation_hint` | `Option<String>` | Microarchitecture hint (e.g. Zen 5 Strix Point) |
| `threads_per_core` | `Option<u32>` | SMT ratio when known |
| `big_little` | `Option<BigLittleInfo>` | Hybrid topology when detectable |
| `simd` | `Vec<SimdFeature>` | Curated ISA features with `present` (AVX512*, NEON, …) |
| `isa_flags` | `Vec<String>` | Extra interesting raw flags |

#### Nested: `BigLittleInfo` / `SimdFeature` / `NpuDevice` / `HostLibrary` / `ModelStoreVolume`

| Type | Public fields |
|------|----------------|
| `BigLittleInfo` | `hybrid`, `capacity_classes`, `note` |
| `SimdFeature` | `name`, `family`, `present`, `detail` |
| `NpuDevice` | `kind`, `name`, `device_path`, `details` |
| `HostLibrary` | `name`, `path`, `category` |
| `ModelStoreVolume` | `path`, `source` (`ModelStoreSource`), `free_bytes`, `total_bytes` |
| `GpuDevice` | `index`, `name`, `total_bytes`, `free_bytes`, `vendor` (`nvidia` / `amd` / empty), `source` (`nvidia-smi` / `rocm-smi` / `sysfs`), optional `arch` |

### Entry points

```rust
use colibri_sys::{MachineInfo, ProbeOptions, ColibriConfig};

// Discoverable default model store + full inventory
let machine = MachineInfo::probe()?;

// Override model-store volume for free-space probe
let machine = MachineInfo::probe_with(&ProbeOptions {
    model_store: Some("/data/colibri-models".into()), // Some(path)
    disk_path: None,
})?;

// Config override: one-liner so hosts cannot miss model_store
let cfg = ColibriConfig::default().model_store("/data/models");
let machine = MachineInfo::probe_for_config(&cfg)?;
// same as: MachineInfo::probe_with(&ProbeOptions::from_config(&cfg))?
assert_eq!(cfg.resolved_model_store(), std::path::PathBuf::from("/data/models"));
assert_eq!(machine.model_store.path, cfg.resolved_model_store());
```

Example that prints **every** public inventory field:

```bash
cargo run -p colibri-sys --example plan_probe
```

### Model store path (discoverable + override)

| Priority | Source |
|----------|--------|
| 1 | Explicit: `ProbeOptions::model_store = Some(path)`, or `ColibriConfig::model_store` via `probe_for_config` / `ProbeOptions::from_config` |
| 2 | Env `COLIBRI_MODEL_STORE` or `COLI_MODEL_STORE` |
| 3 | `$XDG_DATA_HOME/colibri/models` or `~/.local/share/colibri/models` (Windows: `%LOCALAPPDATA%\colibri\models`) |

`ColibriConfig::model_store = None` means “use discoverable default” (env then platform). Set `Some(path)` to force the volume used for free-space checks and installs.

The product CLI still requires an explicit model **directory** (`COLI_MODEL`); the store root is where **colibri-sys** defaults to installing / sizing free space.

### How resources are detected

| Field | Source |
|-------|--------|
| Total / available RAM, swap | Linux `/proc/meminfo`; Windows `GlobalMemoryStatusEx`; macOS `vm_stat` + `sysctl` |
| Physical cores | `lscpu -p=core,socket` (dedupe); mac `hw.physicalcpu` |
| Logical cores | `std::thread::available_parallelism` |
| big.LITTLE / hybrid | Linux `/sys/devices/system/cpu/cpu*/cpu_capacity` |
| SIMD / ISA | `lscpu` Flags / `/proc/cpuinfo` flags (AVX-512*, NEON/ASIMD, SVE, AMX, …) |
| Generation hint | Vendor + family/model + model name (e.g. Zen 5 Strix Point) |
| GPUs | `nvidia-smi` **and** `rocm-smi` / sysfs (AMD devices get `vendor=amd`, `source=rocm-smi` or `sysfs`) |
| NPUs | `/sys/class/accel`, `xrt-smi examine` (XDNA / Ryzen AI), soft OpenVINO marker — probe inventory only |
| Host libraries | `ldconfig -p` + common path fallbacks (CUDA, ROCm/HIP `libamdhip64`, Vulkan, OpenCL, OMP, XRT, ONNX, …) |
| Doctor GPU runtime | Engine binary linkage via `ldd` (Linux): CUDA `libcudart` or HIP `libamdhip64`; check id stays `accelerator.cuda` with vendor-aware summaries |
| Model volume free | Unix `df` on store path (or nearest existing ancestor). **Windows disk free is currently a fixed placeholder** (`500 GB` free / `1000 GB` total), not real volume accounting. |

SSD cache file `.coli_ssd` (written by the engine) is still parsed by `parse_ssd_cache` / `ssd_probe_state`.

---

## 3. Inspect models and registry

A **model** is a directory: `config.json`, tokenizer assets, `*.safetensors` shards, optional side files (`.coli_usage`, `.coli_kv`, `.coli_ssd`).

```rust
use colibri_sys::{ModelInfo, ModelRegistry, model_arch};

let info = ModelInfo::inspect("/path/to/model")?;
// info.family, info.shards, info.model_bytes, info.has_config, info.has_tokenizer
let family = model_arch(info.path.as_path()); // Glm | Inkling | Kimi | DeepseekV4 | Olmoe

let mut reg = ModelRegistry::open(["/models", "/data/colibri-models"])?;
reg.refresh()?;
for entry in reg.entries() {
    println!("{:?} {:?}", entry.path, entry.status);
}
```

Family routing matches `c/coli` (`model_arch` / `engine_for`).

---

## 4. Placement plan

Builds plan **v2** (same ideas as `coli plan --json`):

```rust
use colibri_sys::{ColibriConfig, MachineInfo, PlacementPlan, PlanOptions};

let machine = MachineInfo::probe()?;
let model = "/path/to/model";

let opts = PlanOptions {
    available_memory: Some(machine.available_memory),
    gpus: Some(machine.gpus.clone()),
    physical_cpus: Some(machine.physical_cores),
    cpu_sockets: Some(machine.sockets),
    context: 4096,
    ..Default::default()
};

let plan = PlacementPlan::build(model, &opts)?;
// plan.tiers.ram / .vram / .disk
// plan.projected_hit_rate, plan.bottleneck_class, plan.warnings

let cfg = ColibriConfig::default().model(model);
let env = cfg.apply_plan(&plan); // setdefault-style EnvMap for the child process
```

**Quality doctrine:** placement changes **speed**, not answers (except `experimental-fast`). Plan math is GLM-shaped, same as Python; other families get inspect + env passthrough.

---

## 5. Doctor (standard and deep)

```rust
use colibri_sys::{DoctorOptions, run_doctor, exit_code};

let report = run_doctor(
    "/path/to/model",
    &DoctorOptions {
        deep: true, // header/layout/shard/index/mirror; no payload hash
        available_memory: Some(64 * colibri_sys::GB),
        engine_path: None, // or path to c/colibri for linkage checks
        ..Default::default()
    },
)?;

println!("status={} mode={}", report.status, report.mode);
for c in &report.checks {
    println!("  [{}] {} — {}", c.status, c.id, c.summary);
}
std::process::exit(exit_code(&report));
```

Port of `c/doctor.py`. Deep mode sets `report.mode == "deep"` and appends
container / shard sequence / required-tensor / index / mirror checks (headers
only; no payload hashing).

AMD/ROCm hosts: doctor uses the same GPU inventory as probe and reports HIP
linkage when the engine is linked against `libamdhip64` (or Windows `coli_hip.dll`).
In-process HIP (`feature=ffi-hip`) also counts as linked so doctor is not stuck
on CPU-only solely because a process binary is missing. APU / UMA devices get
shared-system-memory details; placement uses a unified RAM budget (override:
`COLI_GPU_MEMORY`). Default `feature=ffi` without `ffi-hip` / `ffi-cuda` stays
CPU-only for GPU kernels. See [GPU_BACKENDS.md](../../../GPU_BACKENDS.md).

---

## 6. Embed the engine (process + serve mux)

Requires a **built C engine** and a **model directory**. Default features include `runtime` + `tokio`.

```rust
use colibri_sys::{
    ColibriConfig, EngineHandle, GenerateRequest, PlacementPlan, PlanOptions,
};

let model = "/path/to/model";
let engine = "/path/to/colibri"; // or rely on COLI_ENGINE / locate

let cfg = ColibriConfig::default()
    .model(model)
    .engine(engine)
    .max_tokens(256)
    .kv_slots(1);

let plan = PlacementPlan::build(model, &PlanOptions {
    context: cfg.ctx,
    ..Default::default()
})?;

let handle = EngineHandle::start_with_plan(cfg, &plan)?;

// cache_slot: sticky multi-turn KV slot when kv_slots > 1 (0..kv_slots-1).
// grammar: optional GBNF string for constrained decode (None = free form).
let result = handle.generate(GenerateRequest {
    prompt: "Hello".into(),
    max_tokens: 64,
    temperature: 0.8,
    top_p: 0.95,
    cache_slot: 0,
    grammar: None, // or Some("root ::= \"yes\" | \"no\"".into())
})?;

println!("{}", result.text);
if let Some(tiers) = handle.tiers() {
    println!("vram={} ram={} disk={}", tiers.vram, tiers.ram, tiers.disk);
}
if let Some(map) = handle.expert_map() {
    println!("experts {}x{}", map.rows, map.cols);
}

handle.stop()?;
```

### Engine discovery

`locate_engine` searches, in order:

1. Explicit `ColibriConfig::engine` / `COLI_ENGINE`
2. Common install / in-tree candidates under `c/` for the model family

Embed sets `COLI_NO_OMP_TUNE` by default so the child does not re-exec for OpenMP tuning.

### Protocol

Wire format is the product **serve mux** (`SERVE=1`, `SERVE_BATCH=1`). Spec: repo `docs/serve_protocol.md`. Visual lines (HWINFO, TIERS, EMAP, HITS, PROF) feed `VisualSnapshot` on the handle.

### Example binary

```bash
# From repository root, after building c/colibri
export COLIBRI_TEST_ENGINE=./c/colibri
export COLIBRI_TEST_MODEL=./c/glm_tiny   # or any model dir
cargo run -p colibri-sys --example embed_chat
```

Without those env vars, `embed_chat` prints usage and exits 0 (CI-safe).

---

## 7. Visual APIs

After a turn (or when the engine has emitted telemetry), snapshot helpers:

| Method (on `EngineHandle`) | Data |
|----------------------------|------|
| `tiers()` | Expert counts VRAM/RAM/disk + GB |
| `hwinfo()` | Cores, RAM, GPU summary |
| `expert_map()` | R×C cells: `(tier << 6) \| heat` per expert |
| `expert_hits()` | Bitmap + sequence (Brain flash) |
| `profile_window()` / last PROF | Phase timings |

Packing matches `c/telemetry.h` / the web Brain UI (`web/src/Brain.tsx`).

Subscribe bitset for streams: `visual::Subscribe` (map, hits, prof, hw, tokens, …).

---

## 8. rkyv duplex stream

Feature `stream` (default). Frames are length-prefixed rkyv archives.

| Decode API | Use when |
|------------|----------|
| `decode_frame` | Trusted local peer (fast path) |
| `decode_frame_checked` | Untrusted network/file (bytecheck; fails closed) |

```rust
use colibri_sys::{
    ClientFrame, ServerFrame, PROTOCOL_VERSION,
    encode_frame, decode_frame_checked,
};

let hello = ServerFrame::Hello {
    protocol_version: PROTOCOL_VERSION,
    model_id: "demo".into(),
    engine_name: "colibri".into(),
    kv_slots: 1,
};
let bytes = encode_frame(&hello)?;
let back: ServerFrame = decode_frame_checked(&bytes)?;

// Host → engine control (slot = sticky KV session; grammar = optional GBNF)
let _submit = ClientFrame::Submit {
    req_id: 1,
    slot: 0,
    max_tokens: 32,
    temperature: 0.8,
    top_p: 0.95,
    prompt: "Hi".into(),
    grammar: None,
};
```

With `tokio`, `DuplexSession` / `duplex_pair` wrap async halves for tests and custom transports.

### 8.1 EngineDuplex: rkyv over the serve mux (not REST)

Native hosts (GPUI, embed binaries) should talk **in-process** colibri-sys APIs only. `EngineDuplex` is the typed **app ↔ host** control plane:

| Layer | Protocol |
|-------|----------|
| App / UI actor | `ClientFrame` / `ServerFrame` (rkyv) via `EngineDuplex` |
| Bridge | Translates frames to mux calls |
| C engine process | stdin/stdout **serve mux** (`SUBMIT` / `DATA` / `DONE` / visual lines) |

This is **not** REST, **not** OpenAI HTTP, **not** gRPC, and **not** in-process FFI. The Python `c/openai_server.py` gateway is an optional HTTP face of the **same** mux; desktop embeds do not need it.

```rust
use colibri_sys::{
    ClientFrame, EngineDuplex, EngineHandle, /* … */
};

// After EngineHandle::start_with_plan / start_blocking (or from_client in tests):
let mut duplex = EngineDuplex::new(handle, "glm-local");
let _hello = duplex.hello(); // ServerFrame::Hello

// Progressive tokens as ServerFrame::Token while the mux streams DATA:
// slot: sticky multi-turn KV session when Hello.kv_slots > 1
// grammar: Some(gbnf) for constrained decode; None for free form
duplex.handle_with(&ClientFrame::Submit {
    req_id: 1,
    slot: 0,
    max_tokens: 64,
    temperature: 0.8,
    top_p: 0.95,
    prompt: rendered_prompt, // see §8.2
    grammar: None,
}, |frame| {
    // Token / Accept / Done / Hwinfo / Tiers / …
    Ok(())
})?;
```

`ServeClient::generate_stream` / `EngineHandle::generate_stream` also expose progressive `ServeEvent::Data` without rkyv if the host prefers the mux API directly.

### 8.2 Chat templates (no Python)

`render_chat` / `render_chat_simple` port the text multi-turn templates from `c/openai_server.py` (GLM, Kimi K3, DeepSeek V4, Inkling). Use the model family from `config.json` (`model_arch` / `ModelFamily`) so SUBMIT gets a **rendered prompt**, not raw OpenAI messages.

```rust
use colibri_sys::{
    ChatMessage, ChatRenderOptions, ModelFamily, render_chat, render_chat_simple,
};

let messages = vec![
    ChatMessage::system("You are helpful."),
    ChatMessage::user("Hi"),
    ChatMessage::assistant("Hello!"),
    ChatMessage::user("How are you?"),
];
let prompt = render_chat_simple(&messages, ModelFamily::Glm)?;
// or: render_chat(&messages, family, &ChatRenderOptions { enable_thinking: true, .. })?
```

Tool calling and Inkling audio remain host/Python gaps; text chat is enough for a native multi-turn UI.

---

## 9. Install models (feature `install`)

```toml
colibri-sys = { path = "...", features = ["install"] }
```

```rust
use colibri_sys::model::install::{
    install_model, install_model_cancellable, InstallCancel, InstallOptions, InstallSource,
};

let result = install_model(
    InstallSource::HuggingFace {
        repo_id: "org/model-int4".into(),
        revision: None,
        allow_patterns: Some(vec![
            "config.json".into(),
            "tokenizer*".into(),
            "*.safetensors".into(),
        ]),
    },
    InstallOptions {
        dest: "/models/my-model".into(),
        prefer_cli: true,       // use `hf` on PATH when present
        min_free_bytes: 100 * colibri_sys::GB, // hard-gates before download; 0 = off
        inspect_after: true,
        register: false,
        ..Default::default()
    },
    None, // optional progress callback
)?;

// Cooperative cancel: kills the `hf` CLI child or stops the hub loop between files.
let cancel = InstallCancel::new();
// From another thread or UI button: cancel.request();
// Graceful pause (same between-file stop; message INSTALL_PAUSED_MSG): cancel.request_pause();
let result = install_model_cancellable(
    InstallSource::HuggingFace {
        repo_id: "org/model-int4".into(),
        revision: None,
        allow_patterns: None,
    },
    InstallOptions {
        dest: "/models/my-model".into(),
        prefer_cli: true,
        min_free_bytes: colibri_sys::GB, // e.g. 1 GiB free required
        ..Default::default()
    },
    &cancel,
    None,
);
// On cancel: Err with message INSTALL_CANCELLED_MSG ("install cancelled")
// On pause: Err with message INSTALL_PAUSED_MSG ("install paused") — not a hard failure
```

- Prefers the **`hf` CLI** when available; otherwise **hf-hub** 1.x (`list_tree` + selective `download_file`).
- **`min_free_bytes`**: if the destination volume has less free space than this, install fails before download (hard gate). Set `0` to disable.
- **`InstallCancel` / `install_model_cancellable`**: cooperative cancel or pause; CLI path kills the child process; hub path checks between files. Pause vs cancel differs only in the error message and UI handling.
- **Hub resume**: files already present under `dest` with size matching HF metadata are **skipped** (`local_file_is_complete`; size heuristic only, not content hash).
- Detects incomplete downloads; optional post-install `ModelInfo::inspect`.
- Quant **convert** that needs torch still shells out to `c/tools/*.py` via `convert_subprocess` (documented last resort).

Live network test (ignored by default):

```bash
cargo test -p colibri-sys --features install -- --ignored
```

---

## 10. Config and environment

`ColibriConfig` is the typed host subset used for spawn:

| Field | Product knob |
|-------|----------------|
| `model` | `SNAP` / `COLI_MODEL` |
| `engine` | `COLI_ENGINE` |
| `policy` | `COLI_POLICY` |
| `ram_gb` | `RAM_GB` |
| `ctx` | `CTX` |
| `cap` | expert cache slots |
| `max_tokens` | `NGEN` |
| `temperature` / `top_p` | sampling |
| `gpu_indices` / `cuda_enabled` / `vram_gb` | GPU path |
| `kv_slots` | mux slots |
| `mirror` | `COLI_MODEL_MIRROR` |
| `no_omp_tune` | `COLI_NO_OMP_TUNE` (default true for embed) |

Precedence when applying a plan: **explicit host fields and extra_env** override; plan uses setdefault for the rest (same spirit as Python `environment_for_plan`).

Full product surfaces: repo `docs/SETTINGS.md`, `docs/ENVIRONMENT.md`.

---

## 11. Examples and tests

Always start from the repo root:

```bash
cd /home/hunter/Projects/surmount/colibri
```

```bash
# Machine probe + optional plan
cargo run -p colibri-sys --example plan_probe
cargo run -p colibri-sys --example plan_probe -- /path/to/model

# Real engine chat (needs built binary + model)
export COLIBRI_TEST_ENGINE=/home/hunter/Projects/surmount/colibri/c/colibri
export COLIBRI_TEST_MODEL=/home/hunter/Projects/surmount/colibri/c/glm_tiny
cargo run -p colibri-sys --example embed_chat

# Unit + integration
cargo test -p colibri-sys
cargo test -p colibri-sys --features install

# API docs in browser → target/doc/colibri_sys/index.html
cargo doc -p colibri-sys --no-deps --features install --open
```

---

## 12. Architecture (host vs engine)

```
Your app
  └── colibri-sys
        ├── probe / plan / doctor / registry / install   (Rust)
        ├── EngineHandle ──spawn──► C engine binary
        │                    stdin/stdout serve mux
        ├── VisualSnapshot (tiers, EMAP, HITS, PROF)
        └── stream frames (optional rkyv duplex)
```

| Layer | Owned by |
|-------|----------|
| Weights, decode, GPU kernels | C engines |
| Serve line protocol | C + this crate’s mux client |
| HTTP OpenAI server | Still product Python (`c/openai_server.py`) unless you replace it |
| Placement plan / doctor / HF install | This crate (Rust ports of former Python host) |

---

## 13. Residual (not finished product)

| Item | Notes |
|------|--------|
| In-process engine | Multi-family CPU static (`feature = "ffi"`); opt-in Linux CUDA/HIP for GLM (`ffi-cuda` / `ffi-hip`); library default process; native host FFI-first under `feature=ffi`. Metal/Vulkan FFI static and multi-family GPU not claimed: [ffi-phase-d.md](ffi-phase-d.md) |
| NPU inference | Inventory only (`open:npu-inference` deferred) |
| Desktop Tauri dep | Deferred; not wired |
| Sister-engine plan math | GLM-shaped; same as Python |
| Windows GPU probe | RAM path real; GPU discovery less tested |
| Host ROCm live generate | Operator-gated smoke (model + HIP link); unit plan/doctor cover UMA without live GPU |

---

## 14. Quality and contracts

- Placement must not silently change model precision or router semantics.
- Plan JSON should stay comparable to `coli plan --json` / `c/tests/test_resource_plan.py`.
- Telemetry packing must match `c/telemetry.h` for Brain-compatible UIs.

For implementer history: `/home/hunter/Projects/surmount/colibri/.agents/reports/impl-colibri-sys.md` and `impl-colibri-sys-followups.md` (agent reports, not user API docs).

---

## 15. Grok Build–style completion harness

Goal: expose Colibrì as an **OpenAI Chat Completions** endpoint so **Grok Build** (and any OpenAI-compatible client) can use it as a custom model with `api_backend = "chat_completions"`.

Host docs for Grok’s side:

- `/home/hunter/.grok/docs/user-guide/11-custom-models.md`
- Config file: `/home/hunter/.grok/config.toml`

Product HTTP surface today (Python, already OpenAI-shaped):

- `/home/hunter/Projects/surmount/colibri/c/openai_server.py`
- Spec: `/home/hunter/Projects/surmount/colibri/docs/api.md`

`colibri-sys` is the **Rust host library** under that HTTP face: probe → plan → doctor → `EngineHandle` / serve mux → tokens. It does **not** yet ship a full HTTP server; you either (A) point Grok at the existing Python gateway, or (B) implement a thin Grok Build–style harness that uses `colibri-sys` for completions.

### 15.1 Path A — Use the product OpenAI gateway (fastest)

From repo root:

```bash
cd /home/hunter/Projects/surmount/colibri

# Build engine if needed
make -C c colibri   # or ./c/setup.sh

# Serve (typical local port 8000; see docs/api.md / coli serve)
export COLI_MODEL=/absolute/path/to/your/model
export COLI_ENGINE=/home/hunter/Projects/surmount/colibri/c/colibri
# Prefer: ./c/coli serve  …  or python3 c/openai_server.py per product docs
```

Smoke:

```bash
curl -s http://127.0.0.1:8000/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -H 'Authorization: Bearer local' \
  -d '{"model":"glm-5.2-colibri","messages":[{"role":"user","content":"hi"}],"stream":false}'
```

Point **Grok Build** at that base URL in `/home/hunter/.grok/config.toml`:

```toml
[model.colibri-local]
model = "glm-5.2-colibri"                 # must match server --model-id / /v1/models
name = "Colibri local"
description = "Local Colibrì via OpenAI chat completions"
base_url = "http://127.0.0.1:8000/v1"
api_backend = "chat_completions"          # Grok Build default; set explicitly
api_key = "local"                         # dummy unless COLI_API_KEY is set on the server
temperature = 0.7
top_p = 0.95
max_completion_tokens = 2048
context_window = 131072                   # set to real KV/ctx budget you run with
```

Then:

```bash
grok -m colibri-local -p "Say hello from local Colibrì"
# TUI: /model colibri-local
```

That is the same pattern as other custom models in Grok Build: **HTTP Chat Completions**, not a special binary protocol.

### 15.2 Path B — Implement a Grok Build–style harness on colibri-sys

Use this when you want a **Rust** completion service (or embed completions in another binary) without depending on Python for the hot path.

#### Recommended layering

```
Grok Build (or other client)
  └── HTTP  POST /v1/chat/completions  (+ optional SSE stream)
        └── your harness binary  (axum / hyper / warp / …)
              └── colibri-sys
                    ├── MachineInfo / PlacementPlan / ColibriConfig / doctor
                    ├── EngineHandle  (one long-lived process, multi-turn)
                    └── GenerateRequest → text / token stream
```

Do **not** make Grok Build link `colibri-sys` directly unless you are shipping an in-tree Grok feature. Prefer **HTTP** so Grok stays a thin `chat_completions` client (see custom models guide).

#### Completion contract Grok Build expects

Match OpenAI Chat Completions enough for Grok’s `api_backend = "chat_completions"`:

| Piece | Requirement |
|-------|-------------|
| `POST /v1/chat/completions` | JSON body: `model`, `messages[]`, `stream`, `temperature`, `top_p`, `max_tokens` / `max_completion_tokens` |
| `GET /v1/models` | List at least one id Grok will send as `model` |
| Non-stream response | `choices[0].message.role/content`, `finish_reason` |
| Stream response | SSE `data: {choices:[{delta:{content}}]}` chunks, then `data: [DONE]` |
| Auth | Optional `Authorization: Bearer …`; accept dummy key if clients require one |
| Errors | HTTP 4xx/5xx with OpenAI-shaped `{"error":{"message","type",…}}` when practical |

Reference implementation to mirror (behavior, not language):
`/home/hunter/Projects/surmount/colibri/c/openai_server.py`
and `/home/hunter/Projects/surmount/colibri/docs/api.md`.

#### Map chat messages → engine prompt

1. Concatenate `messages` into a single prompt string the model family understands (chat template / role tags). GLM and sisters differ; start with a simple `system` / `user` / `assistant` join if you lack a template, then harden.
2. Cap with harness `max_tokens` from the request (and `ColibriConfig::max_tokens` / plan `CTX`).
3. Call:

```rust
// Pseudocode inside the harness request handler
let handle: &EngineHandle = /* long-lived, Mutex or actor */;
let out = handle.generate(GenerateRequest {
    prompt,
    max_tokens: req_max_tokens,
    temperature: req_temp,
    top_p: req_top_p,
    cache_slot: 0, // or sticky per-conversation slot when KV slots > 1
    grammar: None,
})?;
// non-stream: wrap out.text in OpenAI JSON
// stream: if ServeClient/token callbacks exist, emit SSE deltas; else chunk out.text
```

4. Prefer **one** `EngineHandle` per process (cold start is expensive). Serialize concurrent generates if `kv_slots == 1`; raise `kv_slots` only when the product path supports multi-slot mux (see serve protocol).

#### Lifecycle the harness must own

| Phase | colibri-sys API |
|-------|-----------------|
| Boot | `MachineInfo::probe`, `ModelInfo::inspect`, `PlacementPlan::build`, `run_doctor` (fail closed if doctor `error`) |
| Start | `ColibriConfig::default().model(...).engine(...).apply_plan` / `EngineHandle::start_with_plan` |
| Ready | Wait for serve `READY` (handled inside mux client) |
| Request | `generate` / mux SUBMIT → DATA → DONE |
| Visual (optional) | `tiers`, `expert_map`, `expert_hits` for a local dashboard, not required by Grok |
| Shutdown | `handle.stop()` on SIGTERM |

Absolute engine default for this tree:

```text
/home/hunter/Projects/surmount/colibri/c/colibri
```

Set `COLI_ENGINE` / `ColibriConfig::engine` to that path (or `locate_engine` after build).

#### Suggested harness binary layout (implementation checklist)

1. New binary crate (e.g. `crates/colibri-openai` or external app). Path dep
   (not crates.io):

```toml
# In the consumer Cargo.toml
colibri-sys = { path = "/home/hunter/Projects/surmount/colibri/crates/colibri-sys", features = ["runtime", "stream"] }
# or relative from consumer:
# colibri-sys = { path = "../colibri/crates/colibri-sys", default-features = true }
```

2. Features: default `runtime` + `tokio` (+ `stream` if you need duplex frames);
   optional `install` for first-run model pull. Library embeds prefer process
   `EngineHandle` (`prefer_process = true`) unless they set `prefer_process =
   false` with `feature = "ffi"`. Native desktop with `feature = "ffi"` tries
   FFI first (see Phase D).
3. CLI flags: `--model-dir`, `--engine`, `--host 127.0.0.1`, `--port 8000`, `--model-id`, `--api-key`.
4. On start: probe → plan → doctor → spawn `EngineHandle` once. Engine binary still
   required for the default path (e.g. `/home/hunter/Projects/surmount/colibri/c/colibri`).
5. Routes: `/v1/models`, `/v1/chat/completions` (stream + non-stream). Use
   `ModelInfo::disk_bytes` / `size_info()` when advertising model size.
6. Integration test: mock mux peer (colibri-sys already has mock-style unit tests) **or** ignored real-engine test with `COLIBRI_TEST_*`.
7. Document Grok `config.toml` block (same as Path A) pointing at this harness’s `base_url`.
8. Smoke probe without HTTP: `cargo run -p colibri-sys --example plan_probe -- "$MODEL_DIR"`.

#### Streaming notes for Grok Build

- Grok custom models use streaming when the UI streams; implement SSE early.
- If you only have full-string `GenerateResult` today, still support `stream: true` by chunking or by driving `ServeClient` events (`ServeEvent` token data) if exposed from your handle path.
- Set `max_completion_tokens` and `context_window` in Grok config to match harness limits so compaction behaves sanely.

#### What not to do

- Do not claim crate-wide `prefer_process = false` by default; library embeds stay process-prefer. Native `colibri-native` with `feature = "ffi"` is FFI-first (see Phase D).
- Do not put multi-hundred-GB models inside the Grok or harness package.
- Do not invent a second placement formula; always plan via `colibri-sys`.
- Do not wire Grok’s internal agent tool loop into the engine: Grok remains the agent; Colibrì is the **completion backend**.

### 15.3 Minimal end-to-end checklist (this machine)

```bash
cd /home/hunter/Projects/surmount/colibri

# 1) Docs
less /home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/user-guide.md
cargo doc -p colibri-sys --no-deps --features install --open

# 2) Library sanity
cargo test -p colibri-sys
cargo run -p colibri-sys --example plan_probe -- "$MODEL_DIR"

# 3a) Path A: product server + Grok
#     start coli/openai_server on :8000 with MODEL_DIR
#     add [model.colibri-local] to /home/hunter/.grok/config.toml (above)
grok -m colibri-local -p "ping"

# 3b) Path B: implement harness with EngineHandle, then same config.toml base_url
```

### 15.4 Related absolute paths (quick)

| Need | Path |
|------|------|
| User guide | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/user-guide.md` |
| Embed example | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/examples/embed_chat.rs` |
| Mux client | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/engine/serve.rs` |
| Python OpenAI SoT | `/home/hunter/Projects/surmount/colibri/c/openai_server.py` |
| Grok custom models | `/home/hunter/.grok/docs/user-guide/11-custom-models.md` |
| Grok config | `/home/hunter/.grok/config.toml` |
