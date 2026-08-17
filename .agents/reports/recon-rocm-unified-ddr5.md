# Recon: ROCm/HIP build + unified DDR5 APU memory planning

**Repo:** `/home/hunter/Projects/surmount/colibri`
**Date:** 2026-08-11
**Scope:** read-only recon (no product edits).
**Operator symptom set:** doctor warns `AMD GPU detected but the engine is CPU-only (build with HIP=1)`; UI “Memory on GPU: 0.0 GB”; VRAM nearly full (~4.1 / 4.3 GB); plan warns `RAM budget cannot hold one expert slot per sparse layer`.

Related prior work (still accurate; this report adds **build path + unified-memory budgeting**):

- `.agents/reports/recon-amd-rocm-detection.md` (detection inventory)
- `.agents/reports/impl-phase-b-amd-rocm.md` (PATH-resilient rocm-smi, sysfs, vendor-aware doctor)
- `.agents/reports/impl-ffi-gpu-one-platform.md` (Linux CUDA FFI only; HIP not in FFI matrix)

---

## 1. Current truth (what works / what lies)

### 1.1 HIP/ROCm **engine** build (works, opt-in, process path)

| Path | How | Product today |
|------|-----|----------------|
| Linux process engine | `make -C c glm HIP=1` (or `colibri` / `libcolibri` with `HIP=1`) | Single source `c/backend_cuda.cu` + `c/backend_gpu_compat.h`; hipcc; links `-lamdhip64` with rpath `ROCM_HOME` (default `/opt/rocm`) |
| Windows HIP | `HIP_DLL=1` → host + `coli_hip.dll` + `COLI_HIP_RUNTIME_DIR` | Validated hybrid on gfx1151; experts often CPU unless expert tier configured |
| Default / CI | `CUDA=0 HIP=0` | **CPU-only** pure C binary |
| **Native FFI static embed** | Cargo `feature=ffi` | **CPU-only** always |
| **FFI GPU** | Cargo `ffi-cuda` / `COLIBRI_FFI_CUDA=1` | **Linux CUDA + GLM only** (`make libcolibri CUDA=1` + cudart). Residual `open:ffi-gpu` **closed for that bar only**. **No `ffi-hip`**, no `HIP=1` in `build.rs` |

**Lie vs operator expectation:** “ROCm is installed on the host” does **not** make the product GPU-capable. Doctor is correct: the **linked engine** must be a `HIP=1` (or HIP DLL) binary so `ldd` sees `libamdhip64`. A default `cargo build -p colibri-native --features ffi` embed and any process engine built without `HIP=1` stay CPU-only even with a full `/opt/rocm` tree.

Wire/env names stay CUDA-shaped on both vendors: `COLI_CUDA=1`, `COLI_GPU`, `CUDA_EXPERT_GB`, log tags `[CUDA]`. Documented in `GPU_BACKENDS.md` / `docs/ENVIRONMENT.md`.

### 1.2 Doctor / probe AMD path (mostly honest after Phase B)

**What works:**

- GPU discovery: `nvidia-smi` first, else `rocm-smi` (PATH + `ROCM_*` / `/opt/rocm/bin`), else amdgpu **sysfs** (`mem_info_vram_*`).
- Doctor check id stays `accelerator.cuda` (schema stability) but copy is vendor-aware.
- Linkage: Linux `ldd` for `libcudart` / `libamdhip64`; Windows `coli_hip.dll` / `coli_cuda.dll`.
- Low free VRAM: warn that display may own VRAM.

**What still misleads on APUs:**

| Symptom | Why it is not a full picture |
|---------|------------------------------|
| “Memory on GPU: 0.0 GB” | Live TIERS / resident expert VRAM is 0 when engine is CPU-only **or** hot tier budget is empty / no experts uploaded. Detection of a 4 GB carve-out is separate from residency. |
| “VRAM almost full (4.1 of 4.3 GB)” | True for the **carved device VRAM pool** rocm-smi/sysfs report. On UMA APUs that is often a small fixed window into system DDR5, not “the whole machine’s GPU-usable memory.” |
| “RAM budget cannot hold one expert slot…” | Planner math on **host** RAM after dense + runtime reserve; independent of GPU. Real on small free-RAM hosts or large dense footprints; not fixed by ROCm install alone. |
| No doctor “ROCm packages present” gate | Host library inventory can list `rocm`/`hip*` under `/opt/rocm`; **accelerator pass** is engine linkage + devices, not package presence. That split is intentional and correct. |

**Not detected for planning:** GTT / host-visible heap / “shared DDR5” as an expandable GPU budget. Sysfs has `mem_info_gtt_*` on many amdgpu nodes; product never reads them. `rocm-smi --showmeminfo vram` only.

### 1.3 Placement plan vs runtime unified memory

**Planner (Python + Rust, same policy):**

```
usable_per_gpu = free_bytes − 2 GiB reserve
vram_budget    = min(sum usable, expert_bytes, optional --vram)
```

- AMD `total_bytes` / `free_bytes` come **only** from discrete-style **VRAM** figures.
- No `integrated` flag on `GpuDevice`.
- No NVIDIA-style “memory [N/A] → fall back to system RAM” for AMD (NVIDIA path does that for GB10-class unified chips).
- Result on a 4 GiB UMA iGPU with display load: free often ≪ 2 GiB → **usable saturates to 0** → no hot tier env (`CUDA_EXPERT_GB` omitted / zero) even if system has 32–128 GiB DDR5.

**Runtime engine (HIP/CUDA same path, after `HIP=1` build):**

- Device line uses `prop.totalGlobalMem` (HIP maps `cudaGetDeviceProperties`). On some APUs (docs: Windows 8060S) this can report **tens of GB** (full-ish unified), while plan still used **4 GB VRAM** from smi. Detection and runtime can disagree.
- Expert tier fill uses free mem via `coli_cuda_mem_info` → `hipMemGetInfo` / `cudaMemGetInfo`.
- `#653` / `coli_cuda_device_integrated`: if `prop.integrated`, after placing the GPU expert tier the boot `MemAvailable` snapshot is reduced so RAM cache does not double-count the same pool. **This only runs after a HIP/CUDA engine is actually placing experts.** It does **not** teach the **planner/doctor** to size APU budgets from DDR5.
- Metal (Apple) is the product’s mature “unified hierarchy” story; AMD UMA is not mirrored in plan.

**Bottom line lies:**

1. Doctor “CPU-only (HIP=1)” is **true** for a non-HIP binary; not a false positive of detection failure after Phase B.
2. “4 GB VRAM almost full” is **true for carved VRAM** and **incomplete for UMA capacity**.
3. Plan treating APU VRAM like discrete HBM/GDDR is the main **planning lie** for unified DDR5 hosts.
4. “ROCm installed” ≠ “product can run HIP” until the engine is built/linked with HIP and pointed at by native/process locate.

---

## 2. Recommended product behavior (unified DDR5 APUs)

Goals: use host ROCm when present; size tiers so a 4 GiB carve-out does not pretend the machine has only 4 GiB of GPU-usable memory, without double-counting RAM + “VRAM” on the same physical pool.

### 2.1 Capability model (three layers)

| Layer | Meaning | Pass condition |
|-------|---------|----------------|
| **A. Host ROCm** | Tools + runtime libs present | `rocm-smi` or `/opt/rocm`, `libamdhip64`, optional gfx from enumerator |
| **B. Engine HIP** | This binary can call HIP | `ldd` → `libamdhip64` (Linux) or `coli_hip.dll` bind (Windows) |
| **C. Placement** | Budget and env for this run | Devices + memory model (discrete vs unified) → `COLI_CUDA` / `CUDA_EXPERT_GB` / RAM |

Doctor already nearly does A∩B for accelerator. Plan needs **C** for UMA.

### 2.2 Memory model for AMD APUs (recommended)

When a device is classified **integrated / UMA** (see detection heuristics below):

1. **Do not** treat carved `mem_info_vram_total` (~4 GB) as the only GPU expert budget.
2. **Do** treat weights as living on **one physical DDR5 pool**, with:
   - **Compute on GPU** when HIP (or Vulkan) can run kernels.
   - **Working-set budget** derived primarily from **system free RAM** (same spirit as Metal / NVIDIA unified fallback), minus dense + runtime + OS headroom.
   - **Optional carve-out awareness:** display may pin most of the small VRAM window; that should warn “display-owned carve-out,” not “GPU has no memory.”
3. **Avoid double count:** if a non-zero “hot” GPU expert budget is assigned on UMA, subtract it from the RAM warm budget (mirror `#653` **in the planner**, not only after pin).
4. **Discrete AMD dGPU** (RX 9070 XT class): keep today’s free-VRAM − 2 GB reserve model.

### 2.3 Detection heuristics for `integrated` / UMA (no operator guess if possible)

Prefer any of (first match wins for planning flag):

1. Runtime/HIP probe later: `hipDeviceProp_t.integrated` (same as `#653`) when a HIP engine can answer; optional doctor deep check.
2. Inventory signals without loading HIP:
   - PCI device class / name patterns (Radeon 8xxM, 8060S, “Graphics” iGPU vs discrete RX/Instinct).
   - `total_vram` small **and** large system RAM (e.g. VRAM ≤ 8 GiB and RAM ≥ 16 GiB) as **soft** UMA candidate with low confidence.
   - Sysfs: presence of substantial `mem_info_gtt_total` relative to VRAM (document units; treat as supporting signal, not sole budget).
3. Explicit operator override: env/config `COLI_GPU_MEMORY=unified|discrete` (or plan flag) when heuristics are wrong.

### 2.4 Build/product default for this host class

**Shortest path to “use ROCm on host” for operator:**

1. Build process engine: `make -C c colibri HIP=1 HIP_ARCH=native` (or explicit `gfx1152` / host arch).
2. Point native at it (`COLIBRI_ENGINE` / doctor engine path / locate).
3. Enable GPU env (`COLI_CUDA=1`, device index) via plan once (1)+(2) pass doctor.

**FFI HIP embed** is a larger, optional later slice (mirror `ffi-cuda`); **not** required for process-first product. Residual already says Metal/Vulkan/HIP not in closed `open:ffi-gpu` bar.

**Vulkan** remains a valid alternate AMD path (`docs/vulkan.md`), orthogonal to HIP discovery; do not conflate with `accelerator.cuda`.

### 2.5 UI / doctor copy (operational English, faithful where SPA keys exist)

- Keep warning **“AMD GPU detected but the engine is CPU-only (build with HIP=1)”** when linkage is CPU.
- Add readiness next step when ROCm present + CPU engine: plain “rebuild the engine with HIP=1 against this ROCm” (native-only operational; no brand theater).
- On UMA + low free carve-out: warn about display/carve-out, and state that **plan will use system memory for residency** when unified mode is on.
- “Memory on GPU: 0.0 GB” while CPU-only is expected; after HIP + placement, show real resident expert GB from TIERS/HWINFO.

---

## 3. Critical files (absolute paths)

| Path | Role |
|------|------|
| `/home/hunter/Projects/surmount/colibri/c/Makefile` | `HIP=1` / `HIP_DLL=1`, `ROCM_HOME`, hipcc, rpath |
| `/home/hunter/Projects/surmount/colibri/c/backend_gpu_compat.h` | CUDA↔HIP shim (`hipMemGetInfo`, `hipDeviceProp_t`, …) |
| `/home/hunter/Projects/surmount/colibri/c/backend_cuda.cu` | Init, `totalGlobalMem` log, `coli_cuda_device_integrated`, mem_info |
| `/home/hunter/Projects/surmount/colibri/c/colibri.c` | Expert tier placement; `#653` unified RAM correction |
| `/home/hunter/Projects/surmount/colibri/c/backend_loader.c` | Windows HIP DLL + optional `device_integrated` resolve |
| `/home/hunter/Projects/surmount/colibri/c/resource_plan.py` | discover + plan + `environment_for_plan` |
| `/home/hunter/Projects/surmount/colibri/c/doctor.py` | Python doctor accelerator (parity) |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/build.rs` | FFI: CUDA only, no HIP |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/Cargo.toml` | `ffi` / `ffi-cuda` features |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/probe.rs` | `GpuDevice`, AMD discover, no GTT/unified flag |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/plan.rs` | free−2GB VRAM budget; RAM slot warning |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/doctor.rs` | `accelerator.cuda`, HIP=1 message |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/host.rs` | Doctor display, “Memory on GPU”, engine locate |
| `/home/hunter/Projects/surmount/colibri/GPU_BACKENDS.md` | HIP build contract |
| `/home/hunter/Projects/surmount/colibri/docs/windows.md` | APU HIP hybrid notes (84 GB `totalGlobalMem` example) |
| `/home/hunter/Projects/surmount/colibri/.agents/RESIDUAL.md` | `open:ffi-gpu` closed CUDA-only; HIP FFI not claimed |

---

## 4. Implementation slices (ordered) + acceptance

### Slice 0 — Operator unblock (process HIP, no planner change)

**Work:** Documented/rebuild path only (or product “rebuild engine with HIP” if already in doctor UX).

- Build: `make -C c colibri HIP=1` with host ROCm; set `ROCM_HOME` if non-default.
- Point native/doctor `engine_path` at that binary.
- Confirm `ldd` shows `libamdhip64`; doctor accelerator → pass or low-VRAM warn (not CPU-only).

**Acceptance:**

- Doctor no longer says CPU-only for that engine path.
- Starting with `COLI_CUDA=1` prints a `[CUDA] device …` line for the AMD GPU (tag name is still CUDA-shaped).
- Residency still may be 0 until expert/dense placement env is set; that is Slice 2.

### Slice 1 — Inventory: mark unified / expose full picture (probe + doctor)

**Work (Rust + Python parity):**

- Add `GpuDevice` fields (serde-defaulted): e.g. `integrated: bool` (or `memory_kind: discrete|unified|unknown`), optional `gtt_total_bytes`, keep `total_bytes` as **device VRAM carve-out**.
- Optional: read sysfs `mem_info_gtt_total` / `mem_info_gtt_used` as supporting metadata.
- Doctor details: report carve-out used/total **and** system RAM free/total; if integrated heuristic, summary note “shared system memory (UMA), not discrete VRAM only.”
- Tests: fixtures for 860M-class 4 GiB VRAM CSV + large MemAvailable; discrete RX-class unchanged.

**Acceptance:**

- On APU fixture, doctor/plan inventory shows UMA/integrated flag without requiring HIP link.
- Discrete fixture unchanged messages for free-VRAM clamp.

### Slice 2 — Planner: unified budget (core product fix for DDR5)

**Work:**

- When `integrated` / unified mode:
  - Hot GPU expert budget from **shared pool** policy, e.g. fraction of free system RAM (tunable; start conservative: same 88% RAM envelope, split hot vs warm with no double count), **not** `min(free_vram − 2GB)`.
  - Still clamp by model expert size and optional explicit `--vram` / `CUDA_EXPERT_GB`.
  - Subtract planned hot bytes from warm RAM cache budget.
  - Warnings: replace “VRAM almost full so no GPU tier” with “device VRAM carve-out is busy; using unified system memory budget X GB for GPU-resident experts” when unified mode applies.
- When discrete: keep free−2GB policy.
- `environment_for_plan`: emit `COLI_CUDA=1` + non-zero `CUDA_EXPERT_GB` on UMA when shared budget allows and engine HIP-linked (caller still passes `cuda_enabled`).
- Golden tests: APU free VRAM ~0.2 GB, system free 48 GB → non-zero vram tier budget and reduced ram cache; discrete 24 GB free → classic path.

**Acceptance:**

- Operator host class: plan no longer forces 0 hot tier solely because carve-out free &lt; 2 GB.
- RAM + VRAM planned expert bytes ≤ free system RAM − safety headroom on UMA fixtures.
- Discrete golden plans bit-identical (or within documented epsilon).

### Slice 3 — Runtime alignment + optional HIP probe

**Work:**

- Prefer `coli_cuda_device_integrated` when engine is up (already exists) to confirm Slice 1 heuristics; log one line if plan said discrete but runtime says integrated (or reverse).
- Optional: doctor `--deep` or native readiness: spawn/link check that `hipMemGetInfo` / device props match inventory order of magnitude (catch smi 4 GB vs prop 80 GB mismatch in **details**, not as a hard fail).
- Ensure process path with HIP binary + plan env actually places experts (`resident set` / TIERS vram &gt; 0 on a small model smoke).

**Acceptance:**

- On validated APU + HIP engine + model that fits: TIERS / “Memory on GPU” non-zero after warm placement, or documented reason if ROCm cannot place (arch/WMMA/upload fail).
- `#653` and planner UMA do not both over-subtract (single source of truth for double-count).

### Slice 4 — Native packaging / rebuild UX (optional product)

**Work:** native doctor “make it work” or readiness CTA: if AMD + ROCm host libs + CPU engine → clear next step to build/select HIP engine (not invent slogans; operational English). Optional later: `ffi-hip` mirror of `ffi-cuda` (large; not required if process engine is product default for GPU).

**Acceptance:**

- Operator can go from red CPU-only accelerator to HIP-linked engine without reading Makefile archaeology.
- No claim that FFI embed is HIP-capable until Slice 4b exists.

### Slice 4b — `ffi-hip` (optional, separate residual)

Mirror `build.rs` `HIP=1` + link `amdhip64` when `feature=ffi-hip` / `COLIBRI_FFI_HIP=1`. Out of minimal vertical; residual honesty if deferred.

### Slice 5 — Vulkan honesty (optional parallel)

Separate `accelerator.vulkan` check; not required to fix ROCm/DDR5, but useful when HIP arch unsupported and Vulkan is the real accelerator.

---

## 5. Minimal vertical for the operator (what to land first)

| Priority | Slice | Why |
|----------|-------|-----|
| **P0** | **0** Build/select `HIP=1` process engine | Fixes true CPU-only doctor; without this, memory planning cannot place GPU experts |
| **P0** | **2** Unified plan budget | Fixes “4 GB almost full ⇒ no GPU tier” on large DDR5 APUs |
| **P1** | **1** Inventory flags + doctor detail | Makes warnings honest; supports Slice 2 without magic |
| **P1** | **3** Runtime confirm + smoke | Proves end-to-end residency, not only plan text |
| **P2** | **4 / 4b / 5** | UX polish, FFI HIP, Vulkan doctor |

**Not blocking:** renaming `COLI_CUDA` wire names; multi-vendor NVIDIA+AMD merge; NPU inference (still deferred).

---

## 6. Open operator questions (only if blocking)

None are strictly blocking for Slice 0–2 if heuristics + override exist. Optional prefs:

1. **Default on ambiguous AMD laptop:** prefer **HIP process** rebuild, or document **Vulkan** as primary when gfx is on ROCm’s edge? (Product can default HIP when `libamdhip64` + `HIP=1` engine exist, else surface Vulkan separately.)
2. **UMA hot-tier aggressiveness:** how much of free DDR5 may plan assign as “GPU-resident” vs warm RAM (e.g. 25% / 50% / Metal-like single pool with lower headroom)? Default proposal: conservative shared pool, explicit `--vram` / env override.
3. **Target form factor:** pure APU (860M / 8060S class) vs discrete ROCm dGPU + separate RAM (latter already works with current free-VRAM plan once HIP-built).

---

## 7. Mapping operator symptoms → root cause

| Observed | Root cause | Fix slice |
|----------|------------|-----------|
| `engine is CPU-only (build with HIP=1)` | Process/FFI binary not linked with HIP | 0 (and 4b if FFI required) |
| Memory on GPU 0.0 GB | No HIP path and/or zero hot budget / no resident experts | 0 then 2–3 |
| VRAM 4.1 / 4.3 GB used | Real carve-out under display; plan treats it as full discrete budget | 1–2 |
| RAM budget cannot hold one expert slot | Dense + runtime vs free RAM (and/or large model); may be real | Model/`--ram`/free memory; UMA rebalance in 2 may help only if GPU hot was wrongly competing or RAM was mis-sized |

---

## 8. Bottom line

- **HIP/ROCm compute path exists** in the C engine (`HIP=1`) and is production-documented; **native FFI GPU is CUDA-only**.
- **Doctor AMD detection is largely fixed** (Phase B); the CPU-only warning means **rebuild/select a HIP engine**, not “ROCm missing.”
- **Unified DDR5 is half-implemented:** runtime `#653` after placement; **planner still sizes AMD from carved VRAM free − 2 GB**, so APUs look like “4 GB cards almost full” and starve the hot tier despite large system memory.
- **Smallest product vertical:** (0) HIP process engine on host ROCm + (2) UMA-aware placement that budgets shared DDR5 without double-counting, with (1) honest inventory flags and (3) residency smoke.
