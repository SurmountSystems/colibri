# Review: ROCm HIP (process + ffi-hip) + UMA DDR5 + install-pause honesty

**Role:** L2 general product reviewer (effort-3 wave)
**Date:** 2026-08-11
**Tree:** `/home/hunter/Projects/surmount/colibri`
**Scope:** Plan `plan-rocm-unified-ddr5` product landings + install pause exclusivity/checkpoint.
**Mode:** Read-only. No product edits.

## Sources

| Kind | Path |
|------|------|
| Plan | `.agents/plans/plan-rocm-unified-ddr5.md` |
| Impl reports | `impl-rocm-uma-inventory-plan`, `impl-rocm-hip-process-path`, `impl-rocm-ffi-hip`, `impl-rocm-uma-runtime-smoke`, `impl-rocm-uma-docs-residual`, `impl-install-pause-ux-persist`, `process-mop-rocm-uma` |
| Code | `probe.rs`, `plan.rs`, `doctor.rs`, `linkage.rs`, `build.rs`, `ffi/mod.rs`, `engine/locate.rs`, `c/Makefile`, `c/resource_plan.py`, `c/colibri.c` (#653), `install_ui.rs`, `main.rs` (pause wire) |
| Residual | `.agents/RESIDUAL.md` (ROCm/UMA closed for product bar; smoke operator-gated) |

## Verdict

**Ship-worthy for the stated product bar** (inventory, planner UMA path, process HIP locate/doctor next-step, `ffi-hip` link + mutual exclusion, doctor not-CPU-only when HIP linked, install pause exclusive copy + checkpoint). Process mop reported green (fmt/clippy/tests). Live generate on ROCm APU remains correctly operator-gated.

No **critical** correctness holes found that reverse the campaign claims. Several **medium** honesty / OOM-budget gaps should get failing tests or residual pins before claiming “UMA plan cannot over-commit.”

---

## Findings (severity-ranked)

### Medium — UMA hot budget not clamped by remaining free RAM after dense/runtime

**Where:** `crates/colibri-sys/src/plan.rs` ~255–301; Python parity `c/resource_plan.py` ~824–853.

**What:** On integrated, hot is `0.5 * (available_memory − 4 GiB)`, then warm = `cache_bytes − hot`. Dense + runtime already reduce `cache_bytes`, but **hot itself is never clamped** to what free system RAM can still hold after dense/runtime.

**Why it matters:** For large MoE dense footprints on a UMA APU, the planner can emit a large `CUDA_EXPERT_GB` that, together with dense/runtime, exceeds free DDR. Warm subtraction only prevents double-counting the *warm* tier; it does not cap *hot*. Engine `#653` (`c/colibri.c` ~8234–8250) shrinks the RAM snapshot **after** GPU expert placement, so it does not fully protect placement-time OOM.

**Honesty vs plan:** Plan goals 4–5 (honest UMA budget, no double-count) are partly met (warm path). Risk table “Over-aggressive UMA hot tier OOMs” is only half-mitigated by the 50% fraction.

**Suggested failing test (do not implement here):**

```text
// plan::tests::uma_hot_clamped_when_dense_and_runtime_consume_most_free_ram
// Fixture: integrated APU, available_memory = 48 GiB, dense_bytes ≈ 30 GiB,
// expert_bytes large, free VRAM carve-out near empty.
// Expect: hot_expert_bytes + dense_bytes + runtime_bytes ≤ available_memory
//         (or ≤ available − OS headroom), not merely warm_cap = cache − hot.
```

Also assert env `CUDA_EXPERT_GB` respects the same envelope.

---

### Medium — Doctor “HIP available” is host-wide link, not family / runner path

**Where:** `doctor.rs` `merge_in_process_gpu_linkage` ~448–470, `resolve_doctor_linkage` ~430–440; `ffi/mod.rs` docs (HIP is **GLM** only); `GPU_BACKENDS.md` (deepseek_v4 process CPU for GPU experts).

**What:** If `ffi_hip_linked` is true, doctor marks accelerator pass even when:

1. The process engine for the **current family** is still CPU-only (e.g. DeepSeek V4 process has no HIP expert backend), or
2. The operator forced process with `COLIBRI_FORCE_PROCESS=1` / process-prefer library embeds while only in-process GLM is HIP-linked.

Native with `feature=ffi` defaults FFI-first (`resolve_prefer_process` → false), so the common desktop path matches the merge. The honesty gap is for **family mismatch** and **force-process** users.

**Suggested failing tests:**

```text
// doctor: accelerator for deepseek_v4 model path should not claim full HIP expert
// readiness solely because ffi_hip_linked (GLM) is true — either warn family-scoped
// or details must name “GLM embed HIP; this family may still be CPU for experts”.

// doctor: COLIBRI_FORCE_PROCESS + CPU process binary + ffi_hip_linked → details
// should not imply the *process* runner is HIP-linked (summary may still note
// in-process HIP exists as alternate).
```

---

### Medium — Soft UMA heuristic can misclassify small-VRAM discrete AMD without RX/Instinct in the name

**Where:** `probe.rs` `infer_gpu_integrated` ~156–178; `name_looks_like_integrated_gpu` ~94–135.

**What:** After name checks fail, any AMD device with `total_bytes ≤ 8 GiB` and system RAM ≥ 16 GiB is treated as integrated. Discrete product lines with clear “RX” / “Instinct” are excluded, but bare names (e.g. some “AMD Radeon Graphics” / Pro / workstation labels with ≤8 GiB VRAM) can flip to UMA and take the **system-RAM hot budget** instead of free−2 GiB.

**Mitigation already present:** `COLI_GPU_MEMORY=discrete` override always wins (`apply_gpu_memory_classification_with`). GTT support path is additive, not a hard gate.

**Suggested failing test:**

```text
// probe: device name "AMD Radeon Graphics", total=8 GiB, free=6 GiB, vendor=amd,
// gtt_total=None, system_ram=64 GiB — document intended contract:
// either remain discrete unless GTT/name patterns say UMA, or require stronger
// signals (GTT ≥ half VRAM or explicit 860M-style name). Current code returns true
// via soft path; if product intent is “soft UMA only when GTT supports”, this
// fixture should fail until product is fixed.
```

Operator residual: document that ambiguous names should set `COLI_GPU_MEMORY`.

---

### Low — Hybrid iGPU + dGPU: `any_uma` subtracts *all* hot (including discrete) from warm RAM

**Where:** `plan.rs` ~255–301.

**What:** `any_uma = gpus.iter().any(|g| g.integrated)`. Warm cache subtracts full `hot_bytes` (sum of UMA shares + discrete usable). Discrete hot lives on separate VRAM and should not reduce system-RAM warm.

**Impact:** Over-conservative warm on multi-GPU hybrid laptops; unlikely on pure APU hosts (this campaign’s primary machine).

**Suggested test:** two-device fixture (integrated free-starved + discrete 24 GiB free); expect warm reduction ≈ UMA hot only, not UMA+discrete.

---

### Low — Plan vs runtime `integrated` can disagree without a single detail note at plan time

**Where:** Plan Step E / `impl-rocm-uma-runtime-smoke.md`; runtime `#653` uses `coli_cuda_device_integrated`; host uses heuristics.

**What:** Intentional non-hard-fail is fine. Runtime only applies #653 when device props say integrated; host may still emit large `CUDA_EXPERT_GB` from UMA plan. Smoke report documents this; no host API cross-checks HIP props after start.

**Residual risk:** Host plan UMA + HIP prop `integrated=0` → large expert GB on carve-out path without #653 correction (true discrete mislabel or driver prop quirks). Override remains the operator escape hatch.

**Suggested test (unit):** pure documentation contract already covered; optional later integration note when engine props are queryable from Rust.

---

### Low — Install pause: cooperative mid-file wait is honest; no abandon control while Idle

**Where:** `install_ui.rs` exclusive status; `impl-install-pause-ux-persist.md` residual.

**What:** Pausing copy (“Waiting for current file to finish”) matches cooperative pause. Checkpoint only writes on `Paused`. Crash during Pausing loses checkpoint (acceptable). No dedicated “Abandon paused install” while Paused without cancel/resume path — residual already noted.

Not a dual-message bug regression: `show_active_progress_line` is false for Pausing/Paused/Cancelling; exclusive status owns prose.

---

### Informational / non-bugs (called out for honesty matrix)

| Topic | Assessment |
|-------|------------|
| Default `ffi` without `ffi-hip` is CPU-only | **Solid.** `ffi/mod.rs` + `cuda_gate_tests`; docs matrix. |
| Mutual exclusion `ffi-cuda` / `ffi-hip` | **Solid.** `build.rs` panic ~106–111; Makefile dry-run tests. |
| Process HIP next-step before ffi-hip alternate | **Solid.** `hip_process_rebuild_next_step`; doctor tests order HIP=1 before ffi-hip. |
| Discrete free−2 GiB golden | **Solid.** `discrete_free_vram_minus_two_gib_preserved`. |
| APU starved carve-out → non-zero hot + unified warning + env | **Solid** for the stub model size; see Medium on large dense. |
| Warm reduced by hot on UMA | **Solid** for expert-tier double-count (`uma_warm_reduced_by_hot`). |
| `requires_host_backing` always false | Pre-existing; UMA still relies on warm subtract + #653 rather than this flag. |
| deepseek_v4 process HIP | Documented CPU for GPU experts; not a silent claim of HIP. |
| Live ROCm generate / TIERS | Correctly operator-gated; residual honest. |
| Install exclusive pause copy + checkpoint | **Solid.** SM + exclusive + checkpoint tests; mop 285 native install tests green. |

---

## What looks solid

### Inventory (Step C)

- `GpuDevice.integrated`, GTT optional fields, carve-out remains `total_bytes`/`free_bytes`.
- Override `COLI_GPU_MEMORY` wins; name patterns for 860M-class; soft AMD path; sysfs GTT enrich.
- Doctor details: `shared_system_memory`, carve-out used/total, system free/total, UMA wording on low free.
- Python `resource_plan.py` / `doctor.py` parity for classification fields.

### Planner (Step D)

- Discrete path preserved (free − 2 GiB).
- UMA path uses shared free RAM fraction; busy carve-out warning is plain operational English.
- Warm/hot double-count for expert tiers is implemented and unit-tested on the stub fixture.
- `environment_for_plan` emits `COLI_CUDA=1` + non-zero `CUDA_EXPERT_GB` when UMA budget > 0.

### Process HIP (Step A)

- Linkage module pure-parseable (`ldd` + bytes markers); doctor rebuild hint with `make … HIP=1`, `ROCM_HOME`/`HIP_ARCH`, dual `COLI_ENGINE`/`COLIBRI_ENGINE`.
- Locate miss messages mention HIP=1.
- Docs: `GPU_BACKENDS.md` process section + family notes.

### ffi-hip (Step B)

- Feature implies `ffi`; build.rs HIP make + rpath + `amdhip64`; cfg `ffi_hip_linked`.
- CI-safe CPU fallback unless `COLIBRI_REQUIRE_FFI_HIP=1`.
- rocWMMA missing → portable `COLI_HIP_NO_WMMA` path (host truth; not claimed as full tensor cores).
- Doctor merge so AMD hosts with in-process HIP are not CPU-only solely for missing/CPU process binary.
- Native feature wiring + README matrix; not default on non-ROCm CI.

### Install pause honesty

- Root cause (dual ProgressView line + form status) fixed via phase gate `show_active_progress_line`.
- Exclusive Pausing/Paused/Cancelling copy; bar may freeze at last %.
- Checkpoint TOML next to prefs; clear on Done/Cancel/fresh Start; restore → Paused + Resume; non-cancel error keeps resume path when checkpoint exists.

### Docs / residual

- Feature matrix (ffi / ffi-hip / process HIP / ffi-cuda) consistent across GPU_BACKENDS, ENVIRONMENT, native/sys READMEs, residual MVP body.
- No invented brand theater in doctor/install strings reviewed.

---

## Residual risks (campaign-level)

1. **Operator host smoke still open** for real generate + TIERS non-zero Memory on GPU (process HIP and ffi-hip). Unit contracts do not replace that.
2. **gfx arch mismatch** (1102 vs 1152) still operator `HIP_ARCH`; fail-loud on hipcc is the mitigation.
3. **GLM-only HIP embed** while multi-family CPU FFI is product default: DeepSeek/Kimi/Inkling GPU on AMD still process/Vulkan story.
4. **UMA OOM envelope** (Medium finding) if large models land on ~89 Gi APU with aggressive expert GB.
5. **Heuristic UMA mislabel** on odd discrete names; override is documented but not UI-exposed.
6. **Plan + #653 interaction:** planner reduces warm; runtime reduces mem snapshot after place. Not proven free of double-subtract for warm caps under all env (`RAM_GB` still full budget). Step E notes “do not both over-subtract” but there is no unit test that simulates #653 + plan env together.

---

## Suggested failing tests (summary list)

| Priority | Test idea | Guards |
|----------|-----------|--------|
| P0 | `uma_hot_clamped_when_dense_and_runtime_consume_most_free_ram` | Medium OOM budget |
| P1 | Doctor family-scoped HIP honesty for non-GLM model | Medium doctor honesty |
| P1 | Doctor force-process + CPU binary + ffi_hip_linked details | Medium runner honesty |
| P2 | Soft UMA / bare “Radeon Graphics” 8 GiB contract | Medium heuristic |
| P2 | Hybrid iGPU+dGPU warm subtracts only UMA hot | Low hybrid |
| — | Install exclusive + checkpoint | Already green; keep |

---

## Alignment with plan acceptance

| Plan bar | Status in tree |
|----------|----------------|
| Process HIP locate + doctor not CPU-only when HIP-linked | Met (tests + linkage) |
| ffi-hip feature + link + mutual exclusion | Met |
| Default ffi CPU-only | Met |
| UMA inventory + doctor details | Met |
| UMA non-zero hot on starved carve-out | Met for stub sizes |
| No double-count warm/hot experts | Met for expert tiers; **hot vs dense envelope incomplete** |
| Discrete goldens | Met |
| Docs + residual honesty | Met (Step F report) |
| Live smoke | Operator-gated (explicit) |

---

## Bottom line

Landed work is **coherent, tested, and mostly honest** relative to the plan. The strongest follow-up is **clamp UMA hot to remaining free system RAM after dense/runtime** (or prove runtime placement cannot OOM and encode that envelope in tests). Second is **doctor wording that does not over-claim HIP for non-GLM families or force-process runs**. Install-pause exclusivity and restart-safe Resume look complete for the named contracts.

**No product edits made.** Review only.
