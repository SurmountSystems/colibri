# Plan acceptance: ROCm HIP + unified DDR5 (APU)

**Role:** L2 plan-acceptance reviewer (effort-3, read-only)
**Date:** 2026-08-11
**Plan:** `.agents/plans/plan-rocm-unified-ddr5.md` Steps A–F
**Tree:** `/home/hunter/Projects/surmount/colibri`

## Verdict

| Overall | Notes |
|---------|--------|
| **Partial accept** | Product bar for **UMA inventory + planner**, **process HIP path (docs/locate/doctor)**, and **`ffi-hip` link/doctor/feature matrix** is **landed and process-mop green**. **Step E host smoke** and **Step F residual honesty** are **not complete**. Residual still claims HIP FFI is not in scope for `open:ffi-gpu`. |

**`ffi-hip` is product-complete for the required link/build/doctor bar**, not docs-only. Live generate residency is operator-gated and still open (Step E).

---

## Report inventory

| Report | Present | Plan steps |
|--------|---------|------------|
| `impl-rocm-uma-inventory-plan.md` | **Yes** | C + D |
| `impl-rocm-hip-process-path.md` | **Yes** | A |
| `impl-rocm-ffi-hip.md` | **Yes** | B (+ docs matrix side) |
| `process-mop-rocm-uma.md` | **Yes** | fmt/clippy/tests mop; all exit 0 |
| `impl-rocm-uma-runtime-smoke.md` | **Missing** | E (also referenced from `GPU_BACKENDS.md` / `ffi-phase-d.md` as if present) |
| `impl-rocm-uma-docs-residual.md` | **Missing** | F (docs partially landed inside A/B/C reports) |

---

## Matrix: plan step → status → evidence

| Step | Plan goal | Status | Evidence |
|------|-----------|--------|----------|
| **A** | HIP process engine product path: build docs, locate/`COLI_ENGINE`/`COLIBRI_ENGINE`, doctor next-step, `ldd` acceptance contract | **Met** (code + unit tests); **host process build smoke open** | Report: `.agents/reports/impl-rocm-hip-process-path.md`. Code: `crates/colibri-sys/src/linkage.rs`, `engine/locate.rs`, doctor HIP rebuild hint, `GPU_BACKENDS.md` process section, `docs/ENVIRONMENT.md`, `c/Makefile` HIP process comments. Mop: linkage 9 + locate 5 + doctor 35 green. |
| **A gap** | Process HIP for DeepSeek `deepseek_v4` “at least” | **Partial / honest gap** | Plan Step A names GLM + `deepseek_v4`. Process report + `GPU_BACKENDS.md`: `deepseek_v4` process is **CPU for GPU experts today**; HIP process GPU object is `colibri`/`glm` + `inkling`. Documented, not silent lie. |
| **B** | `ffi-hip` in-process: Makefile static HIP, Cargo feature, `build.rs` make HIP=1 + `amdhip64` + `ffi_hip_linked`, mutual exclusion vs `ffi-cuda`, doctor merge, native feature | **Met (product-complete for required bar)** | Report: `.agents/reports/impl-rocm-ffi-hip.md`. Code: `c/Makefile` `libcolibri HIP=1`, `crates/colibri-sys/Cargo.toml` `ffi-hip`, `build.rs`, `ffi/mod.rs` helpers + gate tests, doctor `merge_in_process_gpu_linkage`, native `Cargo.toml` + README. Impl verified `ldd … \| grep amdhip64` on this host; mop clippy `ffi-hip` green. **Not** multi-family GPU: GLM-only (same honesty as `ffi-cuda`). |
| **C** | Inventory UMA: `integrated`, GTT, heuristics, override, doctor details | **Met** | Report: `impl-rocm-uma-inventory-plan.md`. Code: `probe.rs` fields + classification; doctor details `integrated` / `shared_system_memory` / carve-out; Python `resource_plan.py` / `doctor.py` parity. Tests: probe APU fixture, override wins, sysfs GTT, doctor UMA details. Mop probe 20 green. |
| **D** | Planner unified budget + no double-count + env emission; discrete goldens | **Met** (core contracts) | Report: same inventory-plan. Code: `plan.rs` UMA share of free RAM, warm − hot (#653 mirror), busy-carve-out warning, `environment_for_plan` → `COLI_CUDA=1` + `CUDA_EXPERT_GB`. Tests: `uma_apu_starved_carveout_nonzero_hot_from_system_ram`, `discrete_free_vram_minus_two_gib_preserved`, `uma_warm_reduced_by_hot`. Mop plan 9 green. |
| **D gap** | Plan table: “Override discrete on APU” and “Explicit unified + large free RAM” as named goldens | **Partial** | Override classification tested in **probe** (`coli_gpu_memory_override_wins`); plan re-applies `apply_gpu_memory_classification` from env. No dedicated **plan-level** golden that forces `COLI_GPU_MEMORY=discrete` on APU-shaped free VRAM and asserts classic free−2GiB. Explicit unified path covered by integrated fixtures, not a separate env-only plan test. Behavior implemented; named goldens incomplete. |
| **E** | Runtime integrated confirm; process + ffi-hip smoke; no double over-subtract with #653 | **Missing / incomplete** | No `impl-rocm-uma-runtime-smoke.md`. C runtime already has `coli_cuda_device_integrated` (`c/backend_cuda.cu`, used in `colibri.c` #653). No Rust doctor “plan vs runtime integrated mismatch → one detail” wiring found. Docs point at missing smoke report. Operator host process/`ffi-hip` generate smoke not recorded. |
| **F** | Docs matrix + residual honesty for HIP process and ffi-hip | **Partial** | Docs largely present: `GPU_BACKENDS.md` (process + FFI matrix + UMA), `docs/ENVIRONMENT.md` (`COLI_GPU_MEMORY`, `COLIBRI_FFI_*`), native README, `ffi-phase-d.md`, fidelity GPU row. **Residual not updated:** `.agents/RESIDUAL.md` still says `open:ffi-gpu` closed as **CUDA-only** and “Not claimed: … HIP FFI static”. No dedicated docs-residual report. |

---

## Plan goals (top-level) checklist

| # | Goal | Status |
|---|------|--------|
| 1 | Use installed ROCm for real inference on APU class | **Partial** — link + plan paths ready; **live inference smoke not accepted** |
| 2 | HIP process engines so doctor not stuck CPU-only | **Met** for doctor/locate/docs; operator must still build HIP binary on host |
| 3 | `ffi-hip` required product path | **Met** for feature/build/link/doctor (product code, not docs-only) |
| 4 | Plan/doctor treat UMA honestly | **Met** |
| 5 | No double-count VRAM hot + RAM warm | **Met** in planner tests |
| 6 | Discrete AMD path stays free VRAM − 2 GiB | **Met** (`discrete_free_vram_minus_two_gib_preserved`) |
| 7 | Plain operational copy | **Met** (no brand theater found in touched doctor/docs) |

---

## Non-goals (violations?)

| Non-goal | Violated? |
|----------|-----------|
| Keep CUDA-shaped wire names | **No** — still `COLI_CUDA` / `CUDA_EXPERT_GB` |
| NPU inference | **No** — residual still deferred `open:npu-inference` |
| Vulkan as primary accelerator | **No** |
| Guarantee DeepSeek-V4-Flash full hot on ~89 Gi | **No** — UMA budget is conservative; no false full-residency claim |
| Ship prebuilt HIP binaries in CI for every gfx | **No** — optional feature + CI CPU fallback |

---

## `ffi-hip` required goal: product-complete vs docs-only

| Bar (plan Step B) | Result | Evidence |
|-------------------|--------|----------|
| Makefile static `HIP=1` for `libcolibri` | **Yes** | `c/Makefile` libcolibri HIP docs + `CUDA_OBJ` archive |
| Cargo `ffi-hip` implies `ffi` | **Yes** | `colibri-sys` / `colibri-native` Cargo.toml |
| `build.rs` HIP make + link amdhip64 + cfg | **Yes** | `build.rs`; mop clippy under `ffi-hip` |
| Mutual exclusion vs `ffi-cuda` | **Yes** | build panic string; makefile dry-run tests |
| Default `ffi` CPU-only honesty | **Yes** | feature docs + gate tests |
| Doctor: not CPU-only solely for missing process binary when `ffi_hip_linked` | **Yes** | `merge_in_process_gpu_linkage` + doctor tests |
| Native optional feature + docs | **Yes** | not default; documented matrix |
| CI without ROCm | **Yes** | CPU fallback + warning; ignored link smoke |
| Multi-family static HIP (inkling/kimi/v4) | **Out of scope / honest GLM-only** | Matches closed `ffi-cuda` bar; Makefile says other families CPU in FFI static matrix |
| Live generate TIERS / Memory on GPU | **Not this step** | Deferred Step E; not claimed as green in impl |

**Conclusion:** required **`ffi-hip` product path is complete** for embed link + doctor honesty. Residual text still understates it (acceptance failure for Step F honesty, not for Step B code).

---

## Discrete goldens preserved?

| Contract | Preserved? | Evidence |
|----------|------------|----------|
| Discrete free 24 GiB → usable free − 2 GiB | **Yes** | `plan::tests::discrete_free_vram_minus_two_gib_preserved` |
| Discrete RX fixture not classified integrated | **Yes** | `probe` APU vs RX fixtures; soft heuristic rejects large discrete VRAM |
| Override discrete wins over APU name | **Yes** (classification) | `coli_gpu_memory_override_wins` |

---

## Process mop

`.agents/reports/process-mop-rocm-uma.md`: fmt, clippy (default / ffi / ffi-hip / native), plan/doctor/probe/linkage/locate, `ffi` lib tests, makefile dry-run, native install tests — **all exit 0**, no product fixes needed.

Note: mop did **not** re-run ignored `ffi_hip_linked_when_toolkit_present` under `COLIBRI_REQUIRE_FFI_HIP=1` (impl report did separately).

---

## Acceptance failures

1. **Step E missing:** no runtime-smoke report; no recorded operator process HIP or `ffi-hip` native generate smoke (TIERS / Memory on GPU non-zero or documented ROCm/arch failure). Dangling doc links to `.agents/reports/impl-rocm-uma-runtime-smoke.md`.
2. **Step F residual honesty failure:** `.agents/RESIDUAL.md` still states GPU embed closed as CUDA-only and “Not claimed: … HIP FFI static” after `ffi-hip` landed. Plan required residual to treat ffi-hip as in-scope and closed for the product bar when link + UMA plan + doctor pass.
3. **Step A family honesty (minor):** plan wording “at least … DeepSeek deepseek_v4” for process HIP not met; documented as engine limitation, not a silent regression. Accept as known gap unless operator wants DeepSeek process HIP as follow-on.
4. **Step D named goldens (minor):** env override discrete / explicit unified lack dedicated plan-level tests (classification + main UMA goldens exist).
5. **Step E product detail (minor):** plan vs runtime `coli_cuda_device_integrated` mismatch → “detail once, not hard fail” not found as doctor/UI behavior (runtime #653 exists in C).

---

## Gaps: fix agents vs operator host smoke only

### Fix agents (code/docs/residual)

| Priority | Work |
|----------|------|
| **P0** | Update `.agents/RESIDUAL.md`: close/reword GPU embed for **Linux GLM HIP via `ffi-hip`** (keep Metal/Vulkan/NPU/multi-family GPU out). Align MVP status paragraph with landed bar. |
| **P0** | Add or rewrite Step E artifact: `.agents/reports/impl-rocm-uma-runtime-smoke.md` (checklist + results), **or** remove dangling pointers from `GPU_BACKENDS.md` / `ffi-phase-d.md` until smoke is run. |
| **P1** | Optional plan goldens: APU-shaped + `COLI_GPU_MEMORY=discrete` → classic free−2GiB; force unified path via env if not already covered by classification re-apply tests. |
| **P1** | Optional: doctor/host one-line when plan `integrated` and runtime `coli_cuda_device_integrated` disagree (plan Step E). |
| **P2** | DeepSeek process HIP (only if product wants that family on AMD process path; currently honest CPU-for-GPU-experts). |

### Operator host smoke only (no fix agent required for product code)

| Check | How |
|-------|-----|
| Process HIP binary | `make -C c colibri HIP=1`; `ldd c/colibri \| grep libamdhip64` |
| Doctor not CPU-only | Doctor with that binary + AMD inventory; UMA notes when carve-out busy |
| ffi-hip native link | `cargo build -p colibri-native --features ffi-hip`; `ldd …/colibri-native \| grep amdhip64` |
| Generate smoke | Process HIP **and** ffi-hip native with plan env + model that fits → TIERS / Memory on GPU non-zero **or** document arch/ROCm failure (`HIP_ARCH`, rocWMMA optional) |
| gfx | Override `HIP_ARCH` if native enumerator wrong (860M: 1102 vs 1152 class) |

---

## Summary for parent

- **Accept with residual:** Steps **A (code)**, **B (product ffi-hip)**, **C**, **D**, mop green.
- **Do not full-close plan** until **E smoke record** and **F residual honesty** land.
- **Discrete goldens preserved.** Non-goals clean.
- **`ffi-hip` is not docs-only.** Residual currently **lies** relative to the tree.
