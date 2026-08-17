# Recon: AMD GPU / ROCm detection and acceleration paths

**Repo:** `/home/hunter/Projects/surmount/colibri`
**Date:** 2026-08-10
**Scope:** read-only. No product edits.
**Host class sample (this machine):** AMD Ryzen AI 7 PRO 350 + Radeon 860M iGPU (`gfx1152`), ROCm under `/opt/rocm`, XDNA NPU via `amdxdna` / `xrt-smi` (`RyzenAI-npu6`). No `nvidia-smi`.

---

## 1. How probe / doctor detect GPUs today

### 1.1 Shared discovery model

GPU inventory is **not** from `/sys`, DRM, or HIP runtime APIs. It is:

1. **`nvidia-smi`** (if it returns devices), else
2. **`rocm-smi`** CSV.

Same policy in Python and Rust:

| Layer | Function | Behavior |
|-------|----------|----------|
| Python plan | `c/resource_plan.py` `discover_gpus` | NVIDIA first; else AMD |
| Python doctor | `c/doctor.py` `run_doctor` | calls `discover_gpus()` |
| Python launcher | `c/coli` `env_for` / Windows auto-GPU | `discover_gpus()` for sizing |
| Rust inventory | `crates/colibri-sys/src/probe.rs` `discover_gpus` | port of the same |
| Rust plan | `crates/colibri-sys/src/plan.rs` | `discover_gpus` or inject `PlanOptions.gpus` |
| Rust doctor | `crates/colibri-sys/src/doctor.rs` | `discover_gpus` + `cuda_linkage` |

```230:237:c/resource_plan.py
def discover_gpus():
    # NVIDIA first; if there are none (or no nvidia-smi), fall back to ROCm/HIP so
    # a working AMD engine isn't planned CPU-only and --gpu N stops failing (#662).
    devices = _discover_nvidia_gpus()
    if devices:
        return devices
    return _discover_amd_gpus()
```

```631:640:crates/colibri-sys/src/probe.rs
/// Discover GPUs (NVIDIA first, else ROCm).
///
/// Port of `resource_plan.discover_gpus`.
pub fn discover_gpus() -> Vec<GpuDevice> {
    let nvidia = discover_nvidia_gpus();
    if !nvidia.is_empty() {
        return nvidia;
    }
    discover_amd_gpus()
}
```

There is **no merge** of NVIDIA + AMD. A host with both only reports NVIDIA. There is **no Vulkan ICD inventory** in doctor/probe for GPU selection (Vulkan is a separate engine build/env path).

### 1.2 NVIDIA path (`nvidia-smi`)

```239:271:c/resource_plan.py
def _discover_nvidia_gpus():
    command = ["nvidia-smi", "--query-gpu=index,name,memory.total,memory.free",
               "--format=csv,noheader,nounits"]
    ...
        # Unified-memory chips ... memory.total/memory.free as "[N/A]"
        # Fall back to system RAM figures ...
```

Rust mirror: `probe.rs` `discover_nvidia_gpus` (~642–681). MiB → bytes (`* 1024 * 1024`). Missing binary or non-zero exit → empty list (soft fail).

### 1.3 AMD path (`rocm-smi` only)

```275:319:c/resource_plan.py
def _discover_amd_gpus():
    """ROCm/HIP discovery via rocm-smi (#662). ...
    reports BYTES (unlike nvidia-smi's MiB), so no unit scaling. Column names
    drift across ROCm versions, so match them by substring ...
    VERIFY on AMD hardware (labelled hardware-owner-needed) -- authored without
    a ROCm host to test against."""
    command = ["rocm-smi", "--showmeminfo", "vram", "--showproductname", "--csv"]
```

Rust: `probe.rs` `discover_amd_gpus` (~694–761): same flags, header substring match for total/used/name/device, free = total − used, **no unit scale**.

**Not used for GPU list:** `/sys/class/drm`, `/sys/bus/pci`, `amdgpu` sysfs VRAM files, `rocminfo`, HIP `hipGetDeviceCount`, `vulkaninfo`. Sysfs **is** used for NPU (`/sys/class/accel`) and CPU topology, not for GPU VRAM inventory.

### 1.4 Doctor “accelerator” check (wording still NVIDIA-centric)

Linkage **does** accept HIP on Linux:

```349:366:c/doctor.py
def cuda_linkage(engine_path):
    ...
        # A HIP/ROCm build links libamdhip64 (never libcudart), so match both
        # vendors here or a working AMD engine is reported CPU-only (#663).
        lines = [line for line in result.stdout.splitlines()
                 if "libcudart" in line or "libamdhip64" in line]
```

Rust port: `doctor.rs` `cuda_linkage` (~105–130): same `libcudart` / `libamdhip64` scan. **Windows branch in Rust returns `(false, false)` always** (no `coli_hip.dll` / marker string).

Doctor **check id and messages** still say CUDA/NVIDIA even when the device came from `rocm-smi`:

```470:484:c/doctor.py
    if gpu_indices == []:
        checks.append(_check("accelerator.cuda", "skip", "GPU use was explicitly disabled"))
    ...
    elif selected_gpus and linkage.get("missing"):
        checks.append(_check("accelerator.cuda", "fail", "CUDA runtime library is missing"))
    elif selected_gpus and linkage.get("linked"):
        checks.append(_check("accelerator.cuda", "pass", "CUDA engine and devices are available",
                             devices=[gpu["index"] for gpu in selected_gpus]))
    elif selected_gpus:
        checks.append(_check("accelerator.cuda", "warn", "NVIDIA GPU detected but the engine is CPU-only",
                             devices=[gpu["index"] for gpu in selected_gpus]))
    else:
        checks.append(_check("accelerator.cuda", "skip", "no NVIDIA GPU detected; CPU path is available"))
```

Same strings in `crates/colibri-sys/src/doctor.rs` ~920–982.

Plan pretty-print is also NVIDIA-hardcoded when empty:

```724:729:c/resource_plan.py
    if vram["devices"]:
        names = ", ".join(f"{gpu['index']}:{gpu['name']}" for gpu in vram["devices"])
        ...
    else:
        lines.append("VRAM   no NVIDIA device detected · CPU path")
```

### 1.5 What works / fails on Ryzen AI + Radeon iGPU (this host class)

**Observed on this host (2026-08-10):**

| Surface | Result |
|---------|--------|
| `nvidia-smi` | Absent → NVIDIA branch empty (correct) |
| `rocm-smi ... --csv` with PATH including `/opt/rocm/bin` | **Works.** Example: `AMD Radeon 860M Graphics`, `VRAM Total Memory (B)=4294967296` (4 GiB), `GFX Version=gfx1152`, device `card0` |
| Column heuristics | Match live CSV (`VRAM Total Memory (B)`, `VRAM Total Used Memory (B)`, `Card Series`) |
| `PATH` without `/opt/rocm` | `rocm-smi` **not found** → `discover_gpus()` = `[]` even though GPU + ROCm exist |
| Free VRAM at sample time | ~197 MB free of 4 GiB (display compositor often holds most of iGPU VRAM) → plan sizes a **tiny** hot tier and emits the “VRAM already in use” warning when free &lt; 75% of total (`resource_plan.py` ~589–597) |
| Sysfs DRM | `amdgpu` present (`/sys/class/drm/card1`, PCI `1002:1114`) but **ignored by discover** |
| DRM vs rocm-smi card id | sysfs `card1` vs rocm-smi `card0` (ordinal for `COLI_GPU` is the **smi** index, not DRM card number) |
| HIP libs for engine | `libamdhip64.so` at `/opt/rocm/lib/...`; often **rpath**-linked in `HIP=1` builds (`Makefile` `-L$(ROCM_HOME)/lib -Wl,-rpath,...`) |
| Host library inventory | probe falls back to `/opt/rocm/lib/libamdhip64.so` if ldconfig is thin (`probe.rs` ~1332–1343); ldconfig on this host already sees many hip*/roc* libs under `/opt/rocm` |

**Failure modes common on this host class:**

1. **ROCm user tools not on PATH** → silent empty GPU list → doctor skip “no NVIDIA…”, plan “no NVIDIA device…”, CPU-only plan even with working `HIP=1` binary.
2. **No sysfs fallback** when `rocm-smi` is missing/broken.
3. **iGPU VRAM mostly used** → free-based hot tier near zero; operators need clear doctor copy (“display owns VRAM”, not “no GPU”).
4. **iGPU + dGPU / multi-agent** docs recommend `HIP_VISIBLE_DEVICES` (`GPU_BACKENDS.md` ~26–29); probe does **not** report visibility env or hidden agents.
5. **Windows HIP host**: `coli.cuda_binary()` only accepts `coli_cuda.dll` + `[CUDA] mode: routed experts` marker (`c/coli` ~325–334), **not** `coli_hip.dll`. Windows doctor Python path for CUDA DLL does the same (`doctor.py` ~368–380). HIP_DLL runtime needs `COLI_HIP_RUNTIME_DIR` (`backend_loader.c`, `docs/windows.md`).
6. **NVIDIA preferred exclusive** if a discrete NVIDIA is present; Radeon never appears in the same inventory.
7. Doc comment still says AMD discover was “without a ROCm host to test against” (`resource_plan.py` ~279–280); live CSV on this machine matches the parser.

---

## 2. C engine / coli / Python resource_plan accelerator selection

### 2.1 Product model (one ABI name for two vendors)

- Source: single `c/backend_cuda.cu` + `c/backend_gpu_compat.h` (CUDA pass-through or HIP remaps).
- Compile flags: Linux `CUDA=1` **or** `HIP=1` (mutually exclusive); Windows `CUDA_DLL=1` **or** `HIP_DLL=1`.
- Runtime **env names stay CUDA-shaped** for both vendors: `COLI_CUDA`, `COLI_GPU` / `COLI_GPUS`, `CUDA_EXPERT_GB`, `CUDA_DENSE`, etc. (`GPU_BACKENDS.md` ~129–145; `docs/ENVIRONMENT.md` CUDA section).
- HIP build still defines **`-DCOLI_CUDA`** so host paths in `colibri.c` stay one codepath (`c/Makefile` ~401–422).

```19:20:GPU_BACKENDS.md
| HIP (`HIP=1`) | Linux x86-64 | ROCm (hipcc), `ROCM_HOME=/opt/rocm` default; tested on ROCm 7.2 | `make -C c glm HIP=1 [HIP_ARCH=native\|gfxXXXX]` |
| HIP DLL (`HIP_DLL=1`) ... | Windows ... | ... → `c/coli_hip.dll` |
```

### 2.2 Selection chain

```
  host tools (nvidia-smi | rocm-smi)
           │
           ▼
  discover_gpus()  →  GpuDevice{index,name,total,free}
           │
           ▼
  build_plan / PlacementPlan  →  VRAM hot tier budget (free − 2 GiB reserve per GPU)
           │
           ▼
  environment_for_plan(..., cuda_enabled)
           │  sets COLI_CUDA=1, COLI_GPU(S), CUDA_EXPERT_GB  when enabled + devices + budget
           ▼
  coli env_for / engine main
           │  COLI_CUDA=1 → coli_cuda_init(devices)
           ▼
  HIP or CUDA runtime (linked or coli_hip.dll / coli_cuda.dll)
```

Python plan env export:

```663:704:c/resource_plan.py
def environment_for_plan(plan, env=None, cuda_enabled=True):
    ...
    if not cuda_enabled or not devices or vram["budget_bytes"] <= 0:
        return result
    if result.get("COLI_CUDA", "1") == "0":
        return result
    result.setdefault("COLI_CUDA", "1")
    if "COLI_GPU" not in result and "COLI_GPUS" not in result:
        key = "COLI_GPU" if len(devices) == 1 else "COLI_GPUS"
        result[key] = ",".join(map(str, devices))
    result.setdefault("CUDA_EXPERT_GB", f"{vram['budget_bytes'] / GB:.3f}")
```

`cuda_enabled` comes from **`cuda_binary()`** on the engine path (Linux: `ldd` sees `libcudart` **or** `libamdhip64`; Windows CUDA DLL only today).

CLI:

- `--gpu auto|none|0,1` → `COLI_GPU(S)` / `COLI_CUDA=0` (`c/coli` ~414–508).
- `--auto-tier` → full plan + env.
- `--gpu` / `--vram` without CUDA/HIP build → hard exit “needs the CUDA build” (message says `CUDA=1` only; no `HIP=1` hint).
- Linux bare chat does **not** auto-enable GPU; Windows bare chat does when CUDA DLL + discoverable GPU (`coli` ~444–487; `ENVIRONMENT.md` ~240–248). Windows message on failed discover blames **`nvidia-smi` only**, not `rocm-smi`.

Engine gate (`colibri.c` ~9116–9209): `COLI_GPU(S)` requires `COLI_CUDA=1`; `coli_cuda_init` binds selected ordinals. Logging often still tagged `[CUDA]` even on AMD/HIP (e.g. Windows HIP docs show `[CUDA] device 0: AMD Radeon...`).

### 2.3 CPU vs HIP/ROCm vs CUDA vs Vulkan

| Path | How selected | AMD relevance |
|------|----------------|---------------|
| **CPU** | Default binary; or `COLI_CUDA=0` / `--gpu none` | Always works |
| **CUDA backend** | `CUDA=1` or `CUDA_DLL=1` + `COLI_CUDA=1` | N/A (NVIDIA) |
| **HIP/ROCm backend** | `HIP=1` or `HIP_DLL=1` + same `COLI_CUDA=1` env | Discrete RDNA + some APUs (validated RX 9070 XT; Windows gfx1151 hybrid) |
| **Vulkan** | Separate `VK=1` build + `COLI_VULKAN=1` (+ expert/dense/attn knobs) | **Primary “any AMD GPU including ROCm-dropped” path** via RADV (`docs/vulkan.md`); doctor/probe do **not** surface it as `accelerator.*` |
| **Metal** | Darwin only | N/A |

Vulkan is explicitly documented as often **faster than HIP** on RX 9070 and as the path when ROCm drops older GPUs. It is **orthogonal** to `discover_gpus` / `COLI_CUDA` planning.

### 2.4 WMMA / tensor cores under HIP

`backend_gpu_compat.h`: under HIP, portable kernels always; WMMA only if rocWMMA present and arch not in no-WMMA set; `COLI_CUDA_TC_W4A16` documented NVIDIA-only at dispatch (`GPU_BACKENDS.md` ~141–145).

---

## 3. What “ROCm acceleration available” means in product terms

Not a single boolean in doctor. In practice it means **all** of the following for the HIP path:

1. **Engine build**
   - Linux: linked against `libamdhip64` (`HIP=1`, rpath to `ROCM_HOME`).
   - Windows: host built with `HIP_DLL=1` + **`coli_hip.dll` beside the exe** + **`COLI_HIP_RUNTIME_DIR`** absolute path to dir containing **`amdhip64_7.dll`** (fail-closed identity check in `backend_loader.c`).

2. **Device visible to the HIP runtime**
   - Ordinals after `HIP_VISIBLE_DEVICES` / ROCm visibility (documented mask for iGPU).
   - Kernel `/dev/kfd`, `amdgpu` driver (host sample has both).

3. **Plan / launcher enablement**
   - `discover_gpus()` non-empty **via `rocm-smi` on PATH** (or injected fixture).
   - `cuda_binary()` true (HIP linkage / DLL).
   - `COLI_CUDA=1` + `COLI_GPU`/`COLI_GPUS` + optional `CUDA_EXPERT_GB` / `CUDA_DENSE`.

4. **Actual work on GPU ≠ “device line printed”**
   Product docs: residency is proven by **`[CUDA] resident set: N tensors` with N &gt; 0**, not by device discovery (`GPU_BACKENDS.md` ~111–114; `docs/windows.md` ~347–364).
   On validated Windows HIP hybrid, **dense may be on GPU while routed experts stay on CPU** unless expert placement is configured and validated.

5. **Env naming**
   - Enable flag is still **`COLI_CUDA`**, not `COLI_HIP`.
   - Prefer **`COLI_TEMP`** over `TEMP` because ROCm/Windows treat `$TEMP` as a directory (`ENVIRONMENT.md` ~44; `coli` ~411).
   - `HIP_VISIBLE_DEVICES` is ROCm’s mask; Colibri does not re-export a separate product flag for it.

6. **Host library inventory (probe only)**
   Categories `rocm` / `xrt` / `vulkan` on `HostLibrary` (`probe.rs` ~145–154, ~1283–1305). Useful for UI/info; **not** the doctor accelerator pass/fail gate.

**Inventory-only “ROCm present”** (libs under `/opt/rocm`, `hipcc` installed) is **weaker** than “ROCm acceleration available for Colibri,” which requires HIP-built engine + runtime bind + enabled env + non-empty residency.

---

## 4. Concrete improvements (detection + reporting + doctor) — not implementing

### Detection

1. **PATH-resilient `rocm-smi`**: probe common locations (`/opt/rocm/bin/rocm-smi`, `ROCM_HOME`/`ROCM_PATH`/`HIP_PATH` derived) before giving up.
2. **Sysfs DRM fallback** when smi is missing: `amdgpu` `mem_info_vram_total` / `mem_info_vram_used`, product name, PCI id; mark `source=sysfs` and confidence lower (free may be approximate).
3. **Optional HIP runtime probe** (`hipGetDeviceCount` via small linked helper or `rocminfo`) as tertiary path; report gfx arch (`gfx1152`) next to name.
4. **Multi-vendor inventory**: list NVIDIA **and** AMD (and integrated flag) instead of exclusive NVIDIA-first; let plan select by index + vendor.
5. **`GpuDevice` fields**: `vendor` (`nvidia`|`amd`), `source` (`nvidia-smi`|`rocm-smi`|`sysfs`), `arch` (`gfx1152` / sm_), `integrated` (from HIP `integrated` or PCI class).
6. **Windows discover**: use `rocm-smi` / HIP tools when present; fix **`cuda_binary` / doctor linkage** for `coli_hip.dll` + marker/`[HIP]` host (today Windows HIP host is invisible to launcher auto-enable).
7. **Vulkan presence** (ICD + optional `VK=1` engine): separate doctor check `accelerator.vulkan`, not folded into CUDA messaging.

### Reporting / plan

8. Rewrite empty-device line: “no GPU discovered” + **why** (no smi, empty CSV, free VRAM 0).
9. When free ≪ total on iGPU: explicit warning “most VRAM in use (display?); hot tier may be empty; try headless / free VRAM / force budget.”
10. Plan output names **AMD/HIP** when devices came from rocm-smi; keep `COLI_CUDA` as wire name with a one-line gloss.
11. CLI errors: mention `make … HIP=1` / `HIP_DLL=1`, not only `CUDA=1`.
12. Surface `HIP_VISIBLE_DEVICES` / device ordinal mapping in doctor details when set.

### Doctor

13. Rename check id to **`accelerator.gpu`** (keep alias) with vendor-aware summaries:
    - pass: “HIP engine and AMD device(s) available” / “CUDA engine and NVIDIA device(s)…”
    - warn: “GPU detected but engine is CPU-only” (drop “NVIDIA” when device is AMD)
    - fail missing runtime: “HIP runtime (libamdhip64) missing” vs CUDA
14. Split **device discover** vs **engine linkage** vs **runtime DLL bind** (Windows `COLI_HIP_RUNTIME_DIR`).
15. Report NPU inventory under `accelerator.npu` as **info/skip**, never as inference readiness (see §5).
16. Optional deep check: spawn engine with `COLI_CUDA=1` and parse `resident set` (expensive; gate behind `--deep`).
17. Align Rust Windows `cuda_linkage` with Python (and HIP DLL).
18. Tests: fixture with live-shaped `rocm-smi` CSV from gfx115x iGPU; PATH-without-rocm case; free-VRAM-starved iGPU plan warning copy.

### Host-class notes (Ryzen AI + 860M)

19. Document that **4 GiB UMA iGPU** is a small hot tier even when “detected”; Vulkan may be the better default story for APUs without full ROCm support for that gfx.
20. Masking / arch: `HIP_ARCH=gfx1152` (or whatever `rocm_agent_enumerator` emits); `native` can pull in unsupported agents if multiple GPUs appear.

---

## 5. NPU / XDNA — DEFERRED for inference; inventory only

**Operator pin:** NPU/XDNA **inference is deferred**. Do not plan product inference on XDNA in this pass.

### What exists (inventory / host tools only)

| Mechanism | Location | Role |
|-----------|----------|------|
| `NpuDevice` + `discover_npus()` | `crates/colibri-sys/src/probe.rs` ~1166–1275 | Scan `/sys/class/accel` (modalias / `amdxdna` driver → kind `xdna`), optional `xrt-smi examine` (RyzenAI / firmware), soft OpenVINO tool marker |
| `MachineInfo.npus` | same | Serialized with full probe; **not** used by `PlacementPlan` VRAM tier |
| Host libs `xrt` | `discover_host_libraries` patterns `libxrt_*` | Presence only |
| CPU generation hint | `generation_hint` ~985–989 | Labels “Strix Point / Ryzen AI 300 series” from CPU name — **CPU**, not NPU offload |
| Native UI summary | `crates/colibri-native/src/host.rs` ~87–98 | Prints NPU name/kind next to GPUs |
| Tree artifact | `c/tools/npu_xrt/` | **Only `__pycache__` left** (`.py` sources absent); not a live inference integration in-tree |

### Observed on this host

- `/sys/class/accel/accel0` → driver `amdxdna`, PCI vendor `1022`
- `xrt-smi examine`: `RyzenAI-npu6`, NPU firmware `1.1.2.64`
- Probe would emit kind `xdna` / “AMD XDNA NPU” (or xrt-enriched name)

### What does **not** exist

- No `COLI_NPU` / XRT expert tier in `resource_plan` / `environment_for_plan`
- No doctor check that means “NPU acceleration available for decode”
- No C engine path uploading MoE experts to XDNA
- Doctor `accelerator.cuda` never consults `npus`

**Reporting recommendation while deferred:** keep NPU in machine inventory and doctor as **informational** (“XDNA present; inference not used by Colibri”). Do not set status pass/fail on NPU for serving readiness.

---

## 6. Key file index (absolute paths)

| Path | Role |
|------|------|
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/probe.rs` | GPU/NPU/lib inventory |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/doctor.rs` | Rust doctor + linkage + accelerator.cuda copy |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/plan.rs` | Placement plan / GPU filter |
| `/home/hunter/Projects/surmount/colibri/c/resource_plan.py` | discover_gpus, build_plan, environment_for_plan |
| `/home/hunter/Projects/surmount/colibri/c/doctor.py` | Python doctor |
| `/home/hunter/Projects/surmount/colibri/c/coli` | Launcher, cuda_binary, --gpu / auto-tier |
| `/home/hunter/Projects/surmount/colibri/c/colibri.c` | COLI_CUDA / COLI_GPU init |
| `/home/hunter/Projects/surmount/colibri/c/backend_gpu_compat.h` | CUDA↔HIP shim |
| `/home/hunter/Projects/surmount/colibri/c/backend_loader.c` | Windows coli_hip.dll + COLI_HIP_RUNTIME_DIR |
| `/home/hunter/Projects/surmount/colibri/c/Makefile` | HIP=1 / HIP_DLL=1 |
| `/home/hunter/Projects/surmount/colibri/GPU_BACKENDS.md` | Product backend contract |
| `/home/hunter/Projects/surmount/colibri/docs/vulkan.md` | AMD without ROCm / often faster on RDNA4 |
| `/home/hunter/Projects/surmount/colibri/docs/windows.md` | Windows HIP runtime bind |
| `/home/hunter/Projects/surmount/colibri/docs/ENVIRONMENT.md` | COLI_CUDA, COLI_VULKAN, TEMP/ROCm note |

---

## 7. Bottom line

- **Detection** for planning/doctor is almost entirely **`nvidia-smi` then `rocm-smi`**. No DRM/HIP API fallback.
- On **Ryzen AI + Radeon iGPU**, discovery **works when `rocm-smi` is on PATH** (verified here: 860M, 4 GiB, gfx1152) and **fails closed to “no GPU”** if PATH omits `/opt/rocm/bin`.
- **Acceleration** for AMD is either **HIP** (same `COLI_CUDA` env ABI, `libamdhip64` / `coli_hip.dll`) or **Vulkan** (`COLI_VULKAN`, separate build); product copy still often says “NVIDIA/CUDA.”
- **“ROCm acceleration available”** should mean: HIP-capable engine + runtime bind + visible device + enabled env + non-zero **resident** tensors — not merely ROCm packages installed or NPU present.
- **XDNA/NPU**: inventory only; **inference deferred** by operator.
