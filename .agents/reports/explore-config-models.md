# Explore: config, hardware probe, model management (colibri-sys planning)

Read-only map of `/home/hunter/Projects/surmount/colibri` for what a Rust `colibri-sys` layer should own vs leave to engines / offline tools.

---

## 1. Configuration model (no product config file)

Colibri is **env-var + CLI driven**, not TOML/YAML app config.

| Layer | What it is | SoT |
|-------|------------|-----|
| **CLI** (`c/coli`) | User surface: `--model`, `--ram`, `--ctx`, `--cap`, `--gpu`, `--vram`, `--auto-tier`, `--policy`, serve flags | `docs/SETTINGS.md`, argparse in `c/coli` |
| **Engine env** | Hundreds of knobs (`RAM_GB`, `CTX`, `COLI_CUDA`, `PIPE`, …); four engines, mostly disjoint knob sets | `docs/ENVIRONMENT.md` (scanned from `getenv` sites) |
| **Server env** | `COLI_MODEL`, `COLI_API_KEY`, queue, CORS, tool salvage | same doc § Server/CLI |
| **Model dir state** | Side files next to weights | see below |
| **Tune profiles** | Measured scheduling JSON under XDG / LocalAppData | `c/autotune.py` |

**Model-dir artifacts (not “config files” but durable machine state):**

| Path | Role |
|------|------|
| `config.json` | HF arch geometry (`model_type`, layers, experts, MLA dims) |
| `tokenizer.json` | Required by `coli` before launch |
| `*.safetensors` | Shards (HF `model-N-of-M` or converter `out-N`) |
| `model.safetensors.index.json` | Optional weight map (doctor deep checks) |
| `.coli_usage` | Expert heat history (pin / ranking / mirror plan) |
| `.coli_kv` | Persisted KV (`KVSAVE`) |
| `.coli_ssd` | Cached F_NOCACHE SSD probe (`v2 <gbs> <st_dev>`); Metal+macOS defaults |
| `dense-int4g64/` | Optional Inkling dense sidecar |

**Tune profile store:**
`~/.config/colibri/tuning/<fingerprint>.json` (Linux/mac) or `%LOCALAPPDATA%/colibri/tuning/` (Windows). Schema v1; fingerprint = SHA256 of CPU model, plan CPU/GPU, model path + file sizes/mtimes, engine mtime. Only quality-preserving knobs: `OMP_NUM_THREADS`, `COLI_NUMA`, `PIPE`, `DIRECT`, `COLI_CUDA_PIPE`, `COLI_CUDA_ASYNC`.

**Precedence pattern:** flags → env → planner `setdefault` → engine defaults. Explicit user env always wins over plan/tune (`environment_for_plan` / `apply_profile` use setdefault semantics).

There is **no** central registry file of installed models; location is always `COLI_MODEL` / `--model`.

---

## 2. Hardware / health probes

### `c/resource_plan.py` (placement planner)

Detects and plans **without loading the engine or CUDA context**:

| Probe | How |
|-------|-----|
| **RAM available** | Linux `/proc/meminfo` MemAvailable; Windows `GlobalMemoryStatusEx`; macOS `vm_stat` reclaimable pages (+ `sysctl hw.memsize` fallback) |
| **Physical cores** | Win `GetLogicalProcessorInformationEx`; mac `hw.physicalcpu`; Linux `lscpu -p=core,socket` dedupe; else logical + warning |
| **CPU sockets** | Linux `lscpu -p=socket` only (else 1) |
| **GPUs** | `nvidia-smi` CSV (index, name, total/free MiB); fallback `rocm-smi` VRAM CSV; unified-memory N/A → system RAM figures |
| **Disk free** | `shutil.disk_usage(model_path)` |
| **Model geometry** | Scan safetensors headers: dense vs expert bytes, median expert size, `per_cap_bytes` (sum of median expert per layer) |
| **SSD probe** | **Read-only** parse of `<model>/.coli_ssd` (strict grammar shared with C); never re-measures |

**`build_plan()` output (version 2 JSON):** policy, model byte stats, `cpu.{physical_cores,sockets}`, tiers disk/ram/vram, projected hit rate, bottleneck class (`disk` / `mixed` / `compute` / `memory`), `tune` map with reasons, warnings, `ssd_probe_*`.

**Budget math (GLM-shaped MoE):**

- RAM budget = `--ram` or **88% of available** (floor raised to 8 GB if computed &lt; 4 GB).
- Subtract dense + runtime estimate (KV from config dims × ctx + fixed overhead) → expert cache bytes → **cap** = slots/layer.
- VRAM: free − **2 GB reserve** per GPU; hot experts = min(requested, free, expert bank); warm = remaining in RAM; cold = rest on disk.
- VRAM and RAM tiers are **independent** (GPU-resident experts do not need full host copies).

**`environment_for_plan`:** sets `COLI_POLICY`, `OMP_NUM_THREADS` (physical cores only; does **not** set `OMP_PROC_BIND`), auto-tune keys, `RAM_GB`, and if CUDA enabled `COLI_CUDA`, `COLI_GPU(S)`, `CUDA_EXPERT_GB`. Balanced policy default `REPIN=64`.

**CLI wiring:** `coli plan` prints plan; `--auto-tier` applies it before launch; Windows bare `chat`/`run`/`serve` auto-enables CUDA + plan sizing when `coli_cuda.dll` + `nvidia-smi` exist.

### `c/doctor.py` (read-only health)

`run_doctor()` checks (JSON schema_version 1): model path/config/tokenizer, writability for persistence, engine binary + unresolved `ldd` libs, CUDA/HIP linkage (posix `ldd`; Windows string marker + `coli_cuda.dll`), GPU selection, plan-based RAM/disk/shards, SSD probe display, optional **`--deep`**: full safetensors header/layout, shard sequence, core tensors, index agreement, mirror header/size admission (`COLI_MODEL_MIRROR`). Never hashes payloads or starts the engine.

### Engine-side probes (C, not Python)

SSD F_NOCACHE measure + write `.coli_ssd`; runtime VRAM/Metal/Vulkan; `compat_meminfo` (same reclaimable definition as Python). Python only **surfaces** cached SSD results.

---

## 3. Model families, engines, paths

`c/coli` picks binary from `config.json` `model_type` (`model_arch` / `engine_for`):

| Family | Typical size | Engine binary | Routed by coli? | Get weights |
|--------|--------------|---------------|-----------------|-------------|
| **GLM-5.2** | ~744B MoE, ~372 GB int4 | `colibri` (ex-`glm`) | yes (default) | HF preconverted int4 **or** `coli convert` from FP8 |
| **Inkling** | 975B / 41B act, ~469 GiB int4 | `inkling` | yes (`inkling` in type) | HF `nbeerbower/Inkling-colibri-int4` or `convert_inkling_int4.py` |
| **Kimi K3** | 2.8T / 104B act, ~1.45 TB experts MXFP4 | `kimi_k3` | yes (`kimi`) | HF snapshot or `k3_repack.py` |
| **DeepSeek V4 Flash** | ~284B | `deepseek_v4` | yes | `hf download deepseek-ai/DeepSeek-V4-Flash-0731` |
| **OLMoE** | ~7B research | `olmoe` | **banner only**; not auto-routed (falls through to GLM path) | `convert_olmoe*.py` |

Shared: `st.h`, `quant.h`, tokenizer helpers. Sister engines use **separate env families** (`K3_*`, `INK_*`, OLMoE `HOT`/`WIDE`/…); GLM-centric `resource_plan` math assumes GLM tensor name patterns (`model.layers.L.mlp.experts.E.`).

**Recommended GLM int4 (quickstart):**
`mastouri/GLM-5.2-colibri-int4-g64-with-int8-mtp` (gs64 + int8 MTP). Older non-gs64 containers are known quality footguns.

---

## 4. Download / convert / mirror / setup

| Flow | Entry | Notes |
|------|-------|--------|
| **Prebuilt engine** | GitHub Releases tarball/zip | Python still needed for `coli` + server |
| **Build** | `c/setup.sh` → `make colibri`; optional CUDA/Metal/VK flags | Self-test on `glm_tiny` |
| **GLM FP8 download** | `c/tools/download_glm52.py` (`GLM_DIR=…`); `c/download_fp8.py` (ModelScope-first, hardcoded DEST sample) | Hundreds of GB; pin revision env |
| **GLM convert** | `coli convert` → `tools/convert_fp8_to_int4.py` (shard-resumable) + int8 MTP pass | Offline torch/safetensors |
| **Inkling convert** | `convert_inkling_int4.py`, `convert_inkling_dense_int4.py` | Dense int4 sidecar for low-RAM |
| **K3 / OLMoE / V4** | family tools + docs | Not one unified download CLI |
| **Partial mirror** | `coli mirror` → `tools/mirror_plan.py` | Schema `colibri.partial-mirror.v1`; usage-ranked shards; receipt `.colibri-mirror.json` |
| **Dual-SSD** | env `COLI_MODEL_DIRS` (split), `COLI_MODEL_MIRROR` (replicas), `COLI_DISK_WEIGHTS` | Engine runtime, not download |

Formats registry: `docs/FORMATS.md` (qt fmt 0–8, stamps, rANS offline-only).

---

## 5. Placement / tiering vs machine resources

**Python planner (pre-launch):** sizes RAM cap, VRAM expert GB, OMP threads, PIPE/DRAFT/NUMA/CUDA_PIPE hints from bottleneck class.

**C engine (runtime):** expert LRU/LFRU (`tier.h`), pin hot store from `.coli_usage`, VRAM/Vulkan expert tiers, CAP/CAP_RAISE, RSS_GUARD, Metal SSD-aware defaults, multi-mirror striping.

**Policies:** `quality` | `balanced` | `experimental-fast` (last may drop quant/router preservation flags in plan metadata; tune still quality-preserving).

**Quality rule (product doctrine):** placement changes **speed**, not answers (except experimental-fast / CACHE_ROUTE-class opts).

---

## 6. What colibri-sys should own vs reuse

### Own in Rust (first-class APIs)

1. **Config surface**
   - Typed view of CLI-equivalent + env knobs with same precedence.
   - Persist optional user settings if product needs them (upstream has none; invent carefully, keep env override).
   - Paths: model dir, mirror dirs, API key, host/port, policy, ram/ctx/cap/gpu.

2. **Hardware probe**
   - Port of `memory_available`, `physical_cpu_count`, `cpu_socket_count`, `discover_gpus` (nvidia-smi / rocm-smi / optional Metal presence).
   - Disk free on model volume.
   - Read/validate `.coli_ssd` with **byte-identical** grammar to C/Python (tests already share vectors).
   - **Do not** reimplement C F_NOCACHE writer unless you own Metal startup; call engine or leave measure-on-first-run.

3. **Model registry / inventory**
   - Scan known roots or operator-configured paths; parse `config.json` + shard inventory + size; classify family (`model_type` rules from `coli`).
   - Status: present / incomplete / missing tokenizer / deep-health summary.
   - No upstream multi-model DB; this is **new product** on top of “directory is the unit of install.”

4. **Resource plan apply**
   - Port `analyze_model` + `build_plan` + `environment_for_plan` (contract = `coli plan --json` + tests in `c/tests/test_resource_plan.py`).
   - Feed env map into engine spawn / serve.

5. **Doctor-class health**
   - Standard checks without deep payload hash; optional deep header scan.
   - Engine binary presence + shared-lib readiness (platform-specific).

6. **Update / install orchestration (orchestration only)**
   - Kick HF/`hf` downloads, track progress, resume, space checks.
   - Register destination path; refresh inventory.
   - Invoke converters as **external tools** first (Python) or later native ports.

### Reimplement in Rust (logic is pure + well-tested)

| Component | Why Rust | Keep lockstep with |
|-----------|----------|---------------------|
| `resource_plan` core | Pure math + OS probes; high value for desktop/sys | `test_resource_plan.py`, plan JSON v2 |
| `doctor` standard path | No torch; filesystem + headers | `test_doctor.py` |
| SSD cache parse | Tiny, security-sensitive grammar | `ssd_cache_vectors.txt` / C |
| Model arch routing | Small | `coli.model_arch` |
| Tune profile load/apply | JSON schema v1 | fingerprint algorithm |

### Keep Python / C (do not port first)

| Component | Reason |
|-----------|--------|
| **C engines** (`colibri`, `inkling`, `kimi_k3`, `deepseek_v4`, `olmoe`) | Inference SoT; sys spawns them |
| **Quant convert** (`convert_fp8_to_int4`, inkling/K3 repack) | Torch + hours of offline work; call as subprocess |
| **Download scripts** | HF hub / ModelScope; thin wrappers OK in Rust (`hf` CLI or `hf-hub` crate) |
| **openai_server.py** | Until HTTP gateway is rewritten; sys can reverse-proxy or reimplement later |
| **Runtime tiering inside decode** | Stays in C (`tier.h`, expert_store) |
| **autotune measure loop** | Needs engine REPLAY harness; can schedule later |

### Reuse strategy

- **Spawn** existing `coli` / engines for chat/serve until native gateway exists.
- **Mirror plan JSON contracts** (`plan --json`, doctor report, partial-mirror schema) as the Rust ↔ CLI boundary so Python can remain fallback.
- **Do not** invent a second placement formula; port tests first (red/green against golden plan JSON).
- Sister engines: plan is GLM-centric; for K3/Inkling/V4 expose **env passthrough** + family-specific defaults docs, or gate full planner to GLM until formulas exist.

---

## 7. Suggested colibri-sys API sketch

```
probe_machine() -> MachineInfo          # RAM, cores, sockets, GPUs, OS
inspect_model(path) -> ModelInfo        # family, size, shards, completeness
plan_placement(model, MachineInfo, opts) -> PlanV2
apply_plan(PlanV2, env) -> EnvMap       # setdefault semantics
doctor(model, opts) -> DoctorReport
list_models(roots) / register_model(path)
install_model(source, dest) -> Job      # download + optional convert subprocess
load_tune_profile(plan, model, engine) / run_tune(...)  # phase 2
spawn_engine(engine, EnvMap) / serve(...)
```

---

## 8. Key absolute paths

| Path | Role |
|------|------|
| `/home/hunter/Projects/surmount/colibri/docs/SETTINGS.md` | CLI flags |
| `/home/hunter/Projects/surmount/colibri/docs/ENVIRONMENT.md` | All env knobs |
| `/home/hunter/Projects/surmount/colibri/docs/quickstart.md` | Install path + model download |
| `/home/hunter/Projects/surmount/colibri/docs/api.md` | Serve / HTTP |
| `/home/hunter/Projects/surmount/colibri/c/resource_plan.py` | Plan + HW probe |
| `/home/hunter/Projects/surmount/colibri/c/doctor.py` | Health |
| `/home/hunter/Projects/surmount/colibri/c/autotune.py` | Measured profiles |
| `/home/hunter/Projects/surmount/colibri/c/coli` | Launcher, routing, auto-tier |
| `/home/hunter/Projects/surmount/colibri/c/tools/mirror_plan.py` | Partial mirror |
| `/home/hunter/Projects/surmount/colibri/c/tools/download_glm52.py` | FP8 fetch |
| `/home/hunter/Projects/surmount/colibri/c/tools/convert_fp8_to_int4.py` | GLM convert |
| `/home/hunter/Projects/surmount/colibri/docs/{inkling,kimi_k3,deepseek-v4,FORMATS}.md` | Family + formats |

---

## 9. Bottom line for colibri-sys

Upstream **configuration is the process environment plus a model directory**, not a config tree. **Hardware probe + placement plan** are already a clean Python library with JSON I/O; that is the highest-leverage **reimplement-in-Rust** slice. **Model management** is directory-centric: inventory + download/convert orchestration + doctor. Leave **decode, quant formats, and sister-engine kernels** in C; leave **heavy conversion** as tools. Align contracts with `coli plan --json` / doctor reports so behavior stays comparable to stock colibri.
