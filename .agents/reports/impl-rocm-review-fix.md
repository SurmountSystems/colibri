# Implement: ROCm / UMA review-fix (post effort-3 reviews)

**Date:** 2026-08-11
**Tree:** `/home/hunter/Projects/surmount/colibri`
**Role:** L2 implementer (review-fix)
**Sources:** `review-rocm-uma-general.md`, `review-rocm-uma-plan.md`, `review-rocm-uma-tests.md`

## Summary

Residual honesty for `open:ffi-gpu` / HIP, P0 plan + Makefile goldens, and cheap P1 plan override / warm contracts landed. No live ROCm smoke. No UMA hot redesign; optional dense/runtime clamp left open in residual.

## Findings fixed

### 1. Residual honesty (plan F / plan review P0)

**File:** `.agents/RESIDUAL.md`

| Before | After |
|--------|--------|
| CLOSED table `open:ffi-gpu`: “Metal/Vulkan/**HIP**/NPU not in this slice” | Notes CUDA-first close, then **HIP later** under `plan-rocm-unified-ddr5`; points at ROCm closed section |
| MVP body: GPU embed closed **CUDA-only** wording | GPU embed closed for Linux GLM **CUDA and HIP** (one vendor per build); still not multi-family / Metal / Vulkan / NPU |
| ROCm closed section | Kept closed bar; added **open follow-ups** from general review (hot vs dense envelope clamp optional, hybrid warm, soft UMA names) |

No remaining residual claim that HIP FFI is out of product scope for this campaign.

### 2. P0 — Plan UMA exact budget formula

**File:** `crates/colibri-sys/src/plan.rs`
**Test:** `uma_apu_starved_carveout_nonzero_hot_from_system_ram`

- Asserts exact usable: `0.5 * (48 GiB − 4 GiB) = 22 GiB` (`UMA_HOT_FRACTION` × free after `UMA_OS_HEADROOM_BYTES`).
- Asserts `budget_bytes == min(22 GiB, expert_bytes)` (stub expert = 6 GiB → 6 GiB).
- Asserts `expert_cache + hot == pre-hot cache` (warm_cap = cache − hot).
- Asserts `CUDA_EXPERT_GB` tracks budget (within 0.1 GiB).

### 3. P0 — Makefile HIP dry-run non-vacuous

**File:** `c/tests/test_makefile_cuda_scope.py`
**Test:** `test_libcolibri_hip_packs_backend_object`

- Removed OR with already-required `backend_cuda`.
- Now requires `hipcc` or `HIPCC` in dry-run output.

### 4. P1 — Plan override goldens + stronger warm

**File:** `crates/colibri-sys/src/plan.rs`

| Test | Contract |
|------|----------|
| `coli_gpu_memory_discrete_override_forces_vram_minus_two_on_apu_name` | APU-shaped free 0.2 GiB + `COLI_GPU_MEMORY=discrete` → usable 0, hot 0, no UMA busy budget string |
| `coli_gpu_memory_unified_override_forces_shared_pool_on_rx_name` | RX name free 6 GiB + `unified` → usable 22 GiB (UMA share), **greater than** discrete free−2 (4 GiB) |
| `uma_warm_reduced_by_hot` | Equality: `uma.cache + uma.hot == disc.cache` when disc hot is 0; disc hot == 0; uma cache **strictly** smaller |

Env override tests share `with_coli_gpu_memory` mutex; all GPU plan tests hold it (pass `None` to clear) so parallel lib tests cannot race classification.

## Optional product not done

- **UMA hot clamp vs dense/runtime** (general review Medium): not almost there as a one-liner without behavior change. Left as residual open follow-up under ROCm section. No redesign of 50% fraction.

## Residual still open (unchanged product scope)

- Live ROCm generate / TIERS smoke: **operator-gated**
- Optional: clamp UMA hot so dense + runtime + hot ≤ free RAM (or headroom)
- Soft UMA mislabel on bare discrete names; hybrid iGPU+dGPU warm subtracts all hot
- Doctor family-scoped HIP honesty (non-GLM / force-process) — not in this fix pass
- NPU, Metal/Vulkan primary, multi-family GPU static

## Verify (green)

```text
cargo fmt -p colibri-sys
cargo clippy -p colibri-sys --all-targets -- -D warnings   # exit 0
cargo test -p colibri-sys --lib plan::tests                  # 10 passed
python3 c/tests/test_makefile_cuda_scope.py -v               # 8 passed (HIP included)
```

### Test names green

- `plan::tests::uma_apu_starved_carveout_nonzero_hot_from_system_ram`
- `plan::tests::discrete_free_vram_minus_two_gib_preserved`
- `plan::tests::uma_warm_reduced_by_hot`
- `plan::tests::coli_gpu_memory_discrete_override_forces_vram_minus_two_on_apu_name`
- `plan::tests::coli_gpu_memory_unified_override_forces_shared_pool_on_rx_name`
- `plan::tests::build_plan_with_gpu` (and other plan tests)
- `MakefileHipFfiScopeTest.test_libcolibri_hip_packs_backend_object`
- `MakefileHipFfiScopeTest.test_libcolibri_cuda_and_hip_mutually_exclusive`

## Done criteria

| Criterion | Status |
|-----------|--------|
| Residual honesty clean (no “HIP not in this slice” vs closed ROCm) | **Met** |
| P0 plan exact formula | **Met** + green |
| P0 Makefile HIP non-vacuous | **Met** + green |
| P1 plan goldens / warm | **Met** + green |
