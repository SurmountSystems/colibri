# Tests review: ROCm / UMA / HIP / install-pause

**Role:** L2 tests reviewer (effort-3, read-only)
**Tree:** `/home/hunter/Projects/surmount/colibri`
**Date:** 2026-08-11
**Plan:** `.agents/plans/plan-rocm-unified-ddr5.md`
**Green evidence (process mop):** `.agents/reports/process-mop-rocm-uma.md` — fmt/clippy/tests all exit 0 (plan 9, doctor 35, probe 20, linkage 9, locate 5, ffi 166+2 ignored, native install 285, makefile py 8).

No product edits. Suggestions only.

---

## Verdict

Core contracts from the plan and impl reports are **present and mostly honest**. Several high-value asserts are **weak** (`> 0`, loose inequalities, OR-ed makefile HIP shape). A few plan table rows and Python parity paths have **no red contract**. Install pause pure helpers are strong; **host checkpoint lifecycle** is untested at the policy layer.

Overall: **good floor, not false-green-proof.** Prefer tightening goldens before claiming UMA budget math or HIP dry-run recipe shape are locked.

---

## Coverage map (what exists)

| Area | Module / file | Named tests (spot-check) | Strength |
|------|---------------|--------------------------|----------|
| UMA plan goldens | `crates/colibri-sys/src/plan.rs` | `uma_apu_starved_carveout_nonzero_hot_from_system_ram`, `discrete_free_vram_minus_two_gib_preserved`, `uma_warm_reduced_by_hot` | Mixed: discrete exact; UMA mostly `> 0` |
| Probe integrated / GTT | `probe.rs` | `parse_rocm_smi_csv_gfx115x_igpu_fixture`, `apu_fixture_integrated_discrete_unchanged`, `coli_gpu_memory_override_wins`, `sysfs_amd_fallback_from_fixture_tree` | Strong on name/soft/override/sysfs GTT bytes |
| Doctor merge HIP | `doctor.rs` | `merge_in_process_*` (4), `accelerator_amd_*`, `accelerator_uma_details_note_shared_memory` | Strong on merge pure + AMD CPU/HIP/missing; UMA details only low-free path |
| Linkage parsers | `linkage.rs` | `parse_ldd_hip_*`, `parse_ldd_cuda_*`, `bytes_marker_*`, `probe_fixture_with_hip_marker`, `next_step_names_make_hip_and_env` | Strong pure parsers; few edge combos |
| ffi-hip gates | `ffi/mod.rs` `cuda_gate_tests` | `default_ffi_without_gpu_features_is_cpu_only`, `ffi_hip_feature_reports_request_not_necessarily_link`, ignored `ffi_hip_linked_when_toolkit_present` | Correct CI design; no mutual-exclusion unit |
| Makefile HIP dry-run | `c/tests/test_makefile_cuda_scope.py` | `test_libcolibri_hip_packs_backend_object`, `test_libcolibri_cuda_and_hip_mutually_exclusive` | Exclusion OK; HIP shape assert **weak** |
| Install pause + exclusive + checkpoint | `install.rs`, `install_ui.rs` | pause msg/mid-download; SM; exclusive strip; checkpoint round-trip | Pure layer strong; host save/clear policy not unit-tested |
| Locate / process env | `engine/locate.rs` | override + COLI/COLIBRI; soft HIP miss wording | Soft false-green on miss wording |

Impl reports claimed: plan/probe/doctor UMA, linkage, ffi merge, makefile HIP, install pause UX. Process mop re-ran those filters green.

---

## Findings by area

### 1. UMA plan goldens (`plan.rs`)

**Present (good)**

- APU starved carve-out (~0.2 GiB free, 48 GiB RAM, `integrated: true`) → `vram.budget_bytes > 0`, `hot_expert_bytes > 0`, warning contains `"unified system memory"`, `COLI_CUDA=1`, `CUDA_EXPERT_GB > 0`.
- Discrete free 24 GiB → `usable_bytes == free − 2 GiB`, budget `min(usable, expert_bytes)`.
- UMA vs discrete starved comparison: UMA hot strictly greater.

**Weak / false-green risk**

1. **No exact usable/budget golden for UMA formula.**
   Product math: `UMA_OS_HEADROOM=4 GiB`, `UMA_HOT_FRACTION=0.5` → for 48 GiB free:
   `(48 − 4) × 0.5 = 22 GiB` shared hot; discrete usable from 0.2 GiB free is 0; usable = max(share, discrete).
   With stub `expert_bytes = 6 GiB`, budget clamps to 6 GiB. Tests never assert `usable_bytes` or that share equals `22 * GB` (or budget == `min(22*GB, expert_bytes)`). A regression that sets hot to `1` still passes `> 0`.

2. **`uma_warm_reduced_by_hot` inequality is nearly tautological.**
   ```text
   uma.expert_cache_bytes < disc.expert_cache_bytes + uma.hot
   ```
   When `uma.hot > 0` and caches are similar, this almost always holds without proving `warm_cap = cache_bytes − hot`. Better: equality of UMA `expert_cache_bytes + hot_expert_bytes` to the discrete (starved-hot) `expert_cache_bytes` under same RAM budget.

3. **Plan table rows missing as goldens** (plan Step D / Verification):
   - Override **discrete** on APU-shaped inventory → classic free−2 path (probe has override; **plan does not** pin budget after reclassification).
   - Explicit **unified** on discrete-named GPU → shared-pool path.
   Note: `build_from_info` always re-runs `apply_gpu_memory_classification` (env override + name heuristics). Injecting `integrated: false` on `"AMD Radeon 860M Graphics"` alone will **not** force discrete plan; only `COLI_GPU_MEMORY=discrete` (or non-APU name) will. That contract is untested at plan layer.

4. **Healthy carve-out UMA** (free VRAM not near zero, integrated) still non-zero hot + no “carve-out busy” warning — not covered (only busy path asserts the busy string).

5. **Python `c/resource_plan.py` UMA parity has no unit tests.** Impl claimed Python parity; `c/tests/test_resource_plan.py` has no `integrated` / UMA cases. Drift between Rust and Python is unguarded.

### 2. Probe integrated heuristics (`probe.rs`)

**Present (good)**

- 860M rocm-smi CSV: free near zero, arch gfx1152, classify integrated under 64 GiB system RAM.
- Name patterns + soft AMD small-VRAM + large RAM; discrete RX not integrated; soft fails when system RAM 8 GiB.
- Override `unified`/`uma`/`discrete`/`dgpu` parse + `apply_gpu_memory_classification_with`.
- Sysfs fixture: GTT total/free filled; non-AMD card skipped; HIP ordinal index.

**Gaps**

1. **GTT-supporting path alone** (`infer_gpu_integrated`: AMD + small VRAM + GTT ≥ half VRAM, **without** name match and with system RAM **below** soft threshold) has no dedicated test. Soft path and name path can mask GTT-only regressions.

2. Digit+`S` APU names (e.g. `8060S`) and bare `"AMD Radeon Graphics"` are covered only indirectly if at all; worth one name table test.

3. No test that `gtt_*` lands on devices after **rocm-smi parse + sysfs enrich** (only pure sysfs discover). Enrichment helper untested if separate.

### 3. Doctor merge HIP + UMA (`doctor.rs`)

**Present (good)**

- Pure merge: process CPU + ffi_hip → hip linked; ffi_cuda; process hip preserved over ffi_cuda; neither leaves CPU.
- AMD CPU-only: warn, no NVIDIA, hint `HIP=1` / make before `ffi-hip`, COLI/COLIBRI env.
- HIP linked: pass, no rebuild hint; low free → warn; missing libamdhip64 → fail HIP wording.
- Simulated post-merge HIP → not CPU-only warn.
- UMA low-free + HIP: status warn, summary UMA/shared, details `integrated`, `shared_system_memory`, carve-out bytes, system memory available > 0.

**Gaps / weak**

1. **Healthy UMA pass path** (`linked` + integrated + free not near zero → pass + `"; shared system memory (UMA), not discrete VRAM only"`) is product code but **not** asserted. Only the low-free UMA branch is covered.

2. **CPU-only + integrated** summary append `"; GPU shares system memory (UMA)"` not asserted (fixture is already integrated via `amd_gpu_fixture`).

3. **`merge_in_process` when process `missing: true`** (linked false, kind hip): merge currently overwrites to `linked: true, missing: false` if `ffi_hip`. Intended or not, behavior is unspecified by tests. Recommend an explicit contract test either way.

4. **Python doctor** (`c/tests/test_doctor.py`) has AMD HIP ready / CPU engine, but **no UMA details** (`integrated` / `shared_system_memory` / carve-out). Parity hole vs Rust.

### 4. Linkage parsers (`linkage.rs`)

**Present (good)**

- ldd: hip linked, hip not found, cuda linked, cpu-only.
- Bytes markers hip/cpu; probe shell script fixture with `libamdhip64` → hip via bytes fallback.
- Next-step string: HIP=1, make, ROCM, env, ffi-hip.

**Gaps**

1. **Both CUDA and HIP lines in one ldd blob** → kind must be `"hip"` (code prefers hip). Untested.
2. **One runtime linked, one not found** (linked + missing flags together) untested.
3. Empty basename → default `"colibri"` in `hip_process_rebuild_next_step` untested.
4. Windows DLL sibling path: no `cfg(windows)` tests (acceptable if no Windows CI; note residual).

### 5. ffi-hip gates (`ffi/mod.rs`)

**Present (good)**

- Default `ffi` alone: no cuda/hip feature or linked cfg; `ffi_gpu_linked` false.
- Feature-on tests assert **request** flags only; link not required without toolkit (CI-safe).
- Ignored toolkit smokes for cuda and hip with clear env names (`COLIBRI_REQUIRE_FFI_*`).

**Gaps**

1. Mutual exclusion `ffi-cuda` + `ffi-hip` is proven by operator `cargo build` panic in impl report, **not** a stable unit/integration test in-tree (build.rs panic is fine; a tiny test that documents the panic string is optional).
2. No pure test of “feature on + linked false ⇒ doctor still CPU-only without injected linkage” beyond compile-gated merge injection (already covered with injection).

### 6. Makefile HIP dry-run (`c/tests/test_makefile_cuda_scope.py`)

**Present (good)**

- `libcolibri HIP=1 HIP_ARCH=gfx1100`: `-DCOLI_CUDA`, `backend_cuda`, `libcolibri.a`.
- CUDA+HIP together: nonzero exit + exclusion message.
- Skipped cleanly when toolchain rejects HIP.

**False-green risk (high)**

```python
self.assertTrue(
    "hipcc" in out or "HIP" in out or "backend_cuda" in out,
    ...
)
```

`backend_cuda` was **already** required on the previous line. The third OR makes the hipcc/HIP check vacuous. A recipe that packs `backend_cuda` via **CUDA** path under a mis-set `HIP=1` could still pass if markers match.

**Gaps**

- No dry-run for **process** `colibri HIP=1` (only FFI `libcolibri`).
- No assert that link line mentions `amdhip64` / `lamdhip64` when present in dry-run text.

### 7. Install pause: exclusive copy + checkpoint

**Present (good) — pure `install_ui`**

- Full SM: pause/resume, cancel, pause-then-cancel, JobPaused while Installing.
- Exclusive: `show_active_progress_line` only Installing/Idle; paused never active `"Downloading..."`; exclusive lines for Paused/Pausing.
- Checkpoint: round-trip, missing, corrupt, empty repo unusable, clear idempotent, path next to prefs.

**Present (good) — `colibri-sys` install**

- `local_file_is_complete` size / nested / zero-size heuristic.
- `request_pause` → `INSTALL_PAUSED_MSG` not cancel; mid-download pause via mock CLI.

**Gaps**

1. **Host lifecycle policy** (main.rs): save on Paused, clear on Done / Cancel / fresh Start, keep on error for Resume — **no unit test**. Pure helpers cannot catch a wiring regression that stops calling `persist_install_checkpoint` / `clear_checkpoint_default`. Highest install residual for tests: extract a small pure policy function (event → save|clear|keep) and table-test it, or thin host-facing helper.

2. **`exclusive_status_for_phase(Cancelling, …)`** not asserted (only Paused/Pausing/Installing).

3. **Checkpoint empty dest** unusable (only empty repo).

4. **Percent clamp** (`Some(150)` → 100) untested.

5. **Cancel wins over pause** after both requested (`request_pause` then `request` or reverse): product uses atomic kind; last store wins. No test documents intended last-writer semantics.

6. Pause tests use **CLI mock path** (`prefer_cli: true`). Hub multi-file: pause after file1 + skip-complete on resume is the product resume story; only `local_file_is_complete` unit covers skip, not multi-file install loop with pause.

### 8. Locate (`engine/locate.rs`)

- Override path works.
- `locate_missing_message_mentions_hip_option` can **no-op green** when repo `c/colibri` exists (`if let Err`). Soft contract. Prefer forced miss via bad override only for HIP wording, or assert on a dedicated error builder.

---

## High-value suggested contracts (do not implement here)

### Plan

```rust
#[test]
fn uma_usable_bytes_is_half_free_ram_after_os_headroom() {
    // available_memory = 48 * GB, integrated APU, free VRAM ~0.2 GiB
    // assert_eq!(plan.tiers.vram.devices[0].usable_bytes, 22 * GB);
    // assert_eq!(plan.tiers.vram.budget_bytes, (22 * GB).min(info.expert_bytes));
}

#[test]
fn uma_warm_cache_equals_pre_hot_cache_minus_hot() {
    // Same fixtures as uma_warm_reduced_by_hot
    // assert_eq!(
    //   plan_uma.tiers.ram.expert_cache_bytes + plan_uma.tiers.vram.hot_expert_bytes,
    //   plan_disc.tiers.ram.expert_cache_bytes  // disc hot ~0 on starved carve-out
    // );
}

#[test]
fn coli_gpu_memory_discrete_override_forces_vram_minus_two_on_apu_name() {
    // Set COLI_GPU_MEMORY=discrete (or inject via test-only PlanOptions if added)
    // APU name + free 0.2 GiB → usable_bytes == 0 (not shared-pool 22 GiB)
    // hot_expert_bytes == 0; no "unified system memory budget" busy string required
}

#[test]
fn coli_gpu_memory_unified_override_forces_shared_pool_on_rx_name() {
    // RX 7900-shaped free 24 GiB but COLI_GPU_MEMORY=unified
    // usable from shared pool (not free−2 only); document which wins when discrete carve-out larger
}

#[test]
fn uma_healthy_carveout_still_emits_coli_cuda_and_expert_gb() {
    // integrated, free VRAM 3 of 4 GiB, free RAM large
    // hot > 0; warnings must NOT contain "carve-out is busy"
}
```

### Probe

```rust
#[test]
fn gtt_support_classifies_integrated_without_name_or_soft_ram() {
    // name: opaque PCI, vendor amd, total 4 GiB, gtt_total >= 2 GiB,
    // system_ram = 8 GiB (soft path false) → infer_gpu_integrated true via GTT
}

#[test]
fn name_looks_like_integrated_digit_s_and_graphics() {
    // assert!(name_looks_like_integrated_gpu("AMD Radeon 8060S"));
    // assert!(name_looks_like_integrated_gpu("AMD Radeon Graphics"));
    // assert!(!name_looks_like_integrated_gpu("Radeon RX 7600M XT")); // if product says discrete
}
```

### Doctor

```rust
#[test]
fn accelerator_uma_healthy_free_pass_notes_shared_memory() {
    // HIP linked, integrated, free 3/4 GiB
    // status pass; summary contains "shared system memory (UMA)"
    // details integrated + shared_system_memory true
}

#[test]
fn accelerator_amd_cpu_only_integrated_mentions_uma_in_summary() {
    // CPU linkage + integrated fixture
    // summary contains "CPU-only" and "UMA" / "shares system memory"
}

#[test]
fn merge_in_process_hip_when_process_reports_missing_runtime() {
    // process: linked false, missing true, kind hip + ffi_hip true
    // Document expected: either preserve missing or ffi wins — assert one intentional outcome
}
```

### Linkage

```rust
#[test]
fn parse_ldd_prefers_hip_kind_when_both_cuda_and_hip_present() { ... }

#[test]
fn parse_ldd_linked_and_missing_when_one_runtime_not_found() { ... }

#[test]
fn next_step_empty_basename_defaults_to_colibri() { ... }
```

### Makefile HIP

```python
def test_libcolibri_hip_uses_hipcc_not_only_backend_name(self):
    out = recipe("libcolibri", "HIP=1", "HIP_ARCH=gfx1100")
    self.assertIn("hipcc", out)  # or stronger: hipcc on backend_cuda compile line
    # optional: self.assertRegex(out, r"-l\s*amdhip64|lamdhip64")

def test_process_colibri_hip_packs_backend(self):
    out = recipe("colibri", "HIP=1", "HIP_ARCH=gfx1100")
    self.assertIn("-DCOLI_CUDA", out)
    self.assertIn("backend_cuda", out)
```

### Install pause / checkpoint

```rust
// Prefer pure policy if extracted from main.rs:
#[test]
fn checkpoint_policy_paused_saves_done_and_cancel_clear_start_clears() {
    // event table: Paused→Save, Done→Clear, Cancel→Clear, FreshStart→Clear, Error+existing→Keep
}

#[test]
fn exclusive_status_cancelling_is_not_active_download() {
    let s = exclusive_status_for_phase(InstallUiPhase::Cancelling, Some(16), 0).unwrap();
    assert_eq!(s, "Cancelling...");
    assert!(!line_looks_like_active_download(&s));
}

#[test]
fn checkpoint_empty_dest_unusable() { ... }

#[test]
fn checkpoint_percent_clamped_to_100() {
    let cp = InstallCheckpoint::new("r/m", "main", "/d", "0", Some(150));
    assert_eq!(cp.percent, Some(100));
}

#[test]
fn cancel_after_pause_request_reports_cancel_message() {
    // cancel.request_pause(); cancel.request(); check_cancel → INSTALL_CANCELLED_MSG
}

#[test]
fn hub_skip_complete_after_partial_tree() {
    // multi-file mock hub runner: complete file1, pause before file2;
    // re-run skips file1 via local_file_is_complete; only file2 downloaded
}
```

### Python parity (if Python remains a product planner)

```python
def test_uma_apu_starved_carveout_nonzero_hot(self): ...
def test_discrete_free_minus_two_gib(self): ...
def test_infer_gpu_integrated_860m(self): ...
# doctor: test_amd_uma_details_shared_system_memory
```

---

## False-green / weak assert inventory (priority)

| Priority | Issue | Location |
|----------|--------|----------|
| P0 | UMA hot only `> 0`; no exact 50%×(free−4 GiB) usable | `plan::uma_apu_starved_*` |
| P0 | Makefile HIP hipcc check OR-ed with already-required `backend_cuda` | `test_libcolibri_hip_packs_backend_object` |
| P1 | Warm double-count assert almost tautological | `plan::uma_warm_reduced_by_hot` |
| P1 | Plan override discrete/unified goldens missing | plan Step D table |
| P1 | Host checkpoint save/clear/keep unwired by tests | `main.rs` vs pure `install_ui` |
| P2 | UMA doctor healthy-free pass path untested | `doctor.rs` |
| P2 | GTT-only integrated heuristic untested | `probe.rs` |
| P2 | Python UMA plan/doctor parity tests missing | `c/tests/` |
| P2 | Locate HIP miss wording can pass without asserting | `locate.rs` |
| P3 | ldd dual-runtime / missing+linked edges | `linkage.rs` |
| P3 | Cancelling exclusive status / dest unusable / percent clamp | `install_ui.rs` |

---

## What not to over-test

- Live ROCm inference / `ldd` on real HIP binary: operator Step E; keep `#[ignore]` toolkit smokes.
- `build.rs` mutual exclusion: cargo panic is enough if documented; optional CLI test is heavy.
- Full GPUI paint for exclusive strip: pure `show_active_progress_line` + exclusive helpers already pin the product rule.

---

## Suggested verify filters after tightening (implementer later)

```text
cargo test -p colibri-sys --lib plan
cargo test -p colibri-sys --lib probe
cargo test -p colibri-sys --lib doctor
cargo test -p colibri-sys --lib linkage
cargo test -p colibri-sys --lib --features ffi cuda_gate
cargo test -p colibri-native --features install install_ui
python3 c/tests/test_makefile_cuda_scope.py -v
# optional: python3 c/tests/test_resource_plan.py -v  # when UMA cases added
```

---

## Summary for parent / residual

| Slice | Test bar today | Highest next red |
|-------|----------------|------------------|
| UMA inventory | Solid | GTT-only classify |
| UMA plan | Present but soft | Exact usable/budget + override plan goldens |
| Doctor HIP merge | Solid pure merge | Healthy UMA pass copy; missing+ffi edge |
| Linkage / next-step | Solid | Dual-runtime ldd |
| ffi-hip gates | Correct CI shape | Optional exclusion doc test |
| Makefile HIP | Exclusion good | **Require hipcc (or link amdhip64) without vacuous OR** |
| Install exclusive + checkpoint pure | Solid | Host event→checkpoint policy; hub multi-file resume |
| Python parity | Behind Rust | UMA plan + doctor details |

**Process mop green is not the same as strong contracts.** No blocker claimed for shipping if operator accepts soft goldens; residual for test hardening should prioritize P0/P1 rows above.
