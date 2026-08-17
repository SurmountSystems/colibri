# Recon: complete residual `open:ffi-phase-d`

**Date:** 2026-08-10
**Scope:** read-only inventory for closing (or honestly redefining) `open:ffi-phase-d`
**Sources:** `.agents/RESIDUAL.md`, `crates/colibri-sys/docs/ffi-phase-d.md`, `.agents/reports/impl-track-ffi-libcolibri.md`, tree evidence below
**No product edits** in this recon.

---

## Executive summary

**Multi-family CPU static FFI already shipped** as opt-in (`feature = "ffi"`). Process serve mux remains the **product default**. Residual `open:ffi-phase-d` is correctly **partial**, not fully closable without either:

1. finishing the listed gaps (Inkling, GPU link, golden parity, desktop default / auto-fallback), **or**
2. **redefining close criteria** to “multi-family CPU static opt-in + size + kill-switch + process default” and spinning remaining items into new residual ids.

Recommend option 2 for residual hygiene, plus a phased plan for any work that remains product-valuable.

---

## 1. What already shipped (evidence)

### C static libraries (no CLI `main`)

| Family | Make | Archive | Guard | Public C API |
|--------|------|---------|-------|--------------|
| GLM | `make -C c libcolibri` | `/home/hunter/Projects/surmount/colibri/c/libcolibri.a` | `-DCOLIBRI_NO_MAIN` | `coli_glm_*` in `c/colibri_api.h`; impl in `c/colibri.c` ~9518–9672 |
| Kimi K3 | `make -C c libkimi_k3` | `c/libkimi_k3.a` | `-DKIMI_NO_MAIN` | `coli_kimi_*` in `colibri_api.h`; `c/kimi_k3.c` `#ifndef KIMI_NO_MAIN` ~1749 |
| DeepSeek V4 | `make -f Makefile.deepseek-v4 libdeepseek-v4` | `c/libdeepseek_v4.a` | `-DCOLI_V4_SKIP_GENERATE_MAIN` | `coli_v4_*` in `c/deepseek_v4.h`; units in `c/Makefile.deepseek-v4` |

Makefile evidence:

- `c/Makefile` ~552–564: `libcolibri` → `colibri.lib.o` only (no `CUDA_OBJ` / `METAL_OBJ` / `VK_OBJ`)
- `c/Makefile` ~717–724: `libkimi_k3` → `kimi_k3.lib.o` only (no Vulkan object in archive)
- `c/Makefile.deepseek-v4` ~62–95: separate `*.lib.o` with `LIB_CFLAGS` + `libdeepseek_v4`

Shared size API: `c/colibri_api.h` (`ColiModelSizeSummary`, `coli_model_size_probe`), `c/coli_model_size.c`. Note: archives are “self-contained”; Makefile comments that `coli_model_size.o` is optional for probe-only hosts.

### Rust `colibri-sys` feature `ffi`

| Piece | Path / symbol |
|-------|----------------|
| Cargo feature | `crates/colibri-sys/Cargo.toml` `ffi = []` (empty deps; build.rs does the work) |
| Build / link | `crates/colibri-sys/build.rs`: make all three libs (or env prebuilts `COLIBRI_*_STATIC_LIB`), link `static=deepseek_v4`, `static=colibri`, `static=kimi_k3` + `m`/`gomp`/`pthread` |
| Module | `crates/colibri-sys/src/ffi/{mod,bindings,multi,v4}.rs` |
| Families | `FfiFamily::{Glm, Kimi, DeepseekV4}`; `linked_families()` returns all three |
| Open | `open_engine(family, model_dir) -> FfiEngine` |
| Wrappers | `GlmEngine`, `KimiEngine`, `V4Engine` / `V4Session` |
| Generate | `FfiEngine::generate` / family `generate` with token CB; kill-switch refuse on open/generate |
| Availability | `ffi_link_available()`, `ffi_available()`, `ffi_family_available(family)` |

`EngineHandle::start_blocking` / `start_with_plan` **always** spawn a subprocess (`engine/serve.rs`). Hosts must choose FFI **before** start; there is no dual-path inside `EngineHandle` (documented in `ffi-phase-d.md`).

### Size metadata (always-on, not only under `ffi`)

| Type | Fields | Where |
|------|--------|-------|
| `ModelInfo` | `disk_bytes`, `model_bytes`, `engine_id`, `family`, optional `param_count` | `src/model/mod.rs` |
| `ModelSizeInfo` | disk + optional tier bytes | `ModelInfo::size_info()`, plan overlay, install/registry summaries |
| FFI open | `FfiEngine::size_info()` prefers Rust inspect + C overlay | `ffi/multi.rs`, `ffi/v4.rs` |

### Kill-switch + prefer_process default

| Control | Default | Symbol / env |
|---------|---------|--------------|
| Prefer process | **true** | `ColibriConfig::prefer_process` (`src/config.rs` ~179, Default ~229) |
| Force process | unset | `COLIBRI_FORCE_PROCESS` / `FORCE_PROCESS_ENV` / `force_process_from_env()` |
| Host helpers | | `must_use_process()`, `prefer_ffi_path()` |

Order: env force → `prefer_process` → lack of linked FFI (`must_use_process` ~372–388).

### Tests already green (from impl report; names verified in tree)

**Kill-switch / config (always):**

- `force_process_env_truthy_matrix`
- `prefer_process_default_forces_process_path`
- `prefer_process_false_allows_ffi_only_when_linked`

**FFI feature:**

- `link_available_when_feature_on`
- `ffi_available_respects_link`
- `open_missing_model_errors` (V4)
- `linked_families_include_product_engines`
- `open_glm_missing_errors` / `open_kimi_missing_errors`
- `family_available_tracks_env`
- `glm_tiny_open_has_disk_bytes` (optional skip if no weights / open fails after inspect)

Impl report claim: `cargo test -p colibri-sys --lib` 91 pass; `--features ffi` 99 pass (2026-08-10).

**Not covered by automated FFI tests:** token-level generate parity vs process, Kimi/V4 open on real tiny weights, visual/STOP/CANCEL ABI, multi-slot mux.

### Docs honesty (mostly current)

- Living SoT: `crates/colibri-sys/docs/ffi-phase-d.md` (status multi-family CPU static implemented; gaps listed)
- Residual: `.agents/RESIDUAL.md` `open:ffi-phase-d` **partial**
- Impl: `.agents/reports/impl-track-ffi-libcolibri.md`
- **Stale:** `crates/colibri-native/docs/fidelity.md` still says `ffi_available()` stub / “Phase D design-only” (rows ~39, ~57). Product path text (process engine) is still true for desktop; the “stub only” claim is wrong after multi-family wave.

---

## 2. Exact remaining gaps (triple source)

Aligned across `RESIDUAL.md`, `ffi-phase-d.md` § Still out of scope, `impl-track-ffi-libcolibri.md` § Residual still open:

| # | Gap | Residual wording | Status in tree |
|---|-----|------------------|----------------|
| 1 | **Inkling** no-main extract + FFI family | Explicit open | `c/inkling.c` still has bare `int main` (~2027); no `INKLING_NO_MAIN` / `libinkling`; `FfiFamily::from_model_family(Inkling) -> None` (`multi.rs` ~49) |
| 2 | **GPU** in static (or dynamic) link matrix | Explicit open | `libcolibri` / `libkimi` archives are CPU-only TUs; product bins link `CUDA_OBJ`/`METAL_OBJ`/`VK_OBJ` |
| 3 | **Golden** token/logit parity vs process on production (or even tiny) weights | Explicit open | Fixtures exist; no FFI vs process parity tests |
| 4 | **Desktop / product default** FFI | Explicit open | `colibri-native` does not enable `ffi`; always `EngineHandle` spawn |
| 5 | Host **auto-fallback** open-fail → process | Impl track #4 | Documented policy only; not wired in native host |
| 6 | **Visual poll** / multi-slot / concurrent cancel in-process ABI | Doc residual | `colibri_api.h` has open/size/generate only; no STOP/CANCEL/EMAP/HITS |
| 7 | **NPU inference** | Separate residual `open:npu-inference` | Deferred; not part of full FFI close unless redefined |

RESIDUAL one-liner (2026-08-10): multi-family CPU + `ffi` + kill-switch + size landed; still open Inkling, GPU, golden full weights, desktop default.

---

## 3. Engine families status

| Family | Process product binary | CPU static FFI | Notes |
|--------|------------------------|----------------|-------|
| **GLM** | `c/colibri` | **Done** `libcolibri.a` / `FfiFamily::Glm` | `ModelFamily::Olmoe` maps to Glm for FFI (`from_model_family`) |
| **Kimi K3** | `c/kimi_k3` | **Done** `libkimi_k3.a` / `FfiFamily::Kimi` | GPU in process path is Vulkan (`VK=1`), not CUDA (#783 in Makefile) |
| **DeepSeek V4** | `c/deepseek_v4` | **Done** `libdeepseek_v4.a` / `FfiFamily::DeepseekV4` | Experimental C API predated this wave |
| **Inkling** | `c/inkling` | **Not started** | Separate amalgamation; audio/DMel; CUDA ink backend `backend_cuda_ink.*`; Metal MoE hooks in `inkling.c` |
| **Olmoe** | `c/olmoe` | No dedicated family | Treated as GLM-shaped for FFI mapping; process binary exists |
| **NPU** | none | none | `open:npu-inference` deferred |

Product locate table (process default): `colibri`, `inkling`, `kimi_k3`, `deepseek_v4` (`ffi-phase-d.md` § Current process embed).

---

## 4. GPU link path: what would be needed

Today’s **product** GPU story is build-time flags on **executables**, not on static FFI archives:

| Backend | Build flags (process) | Evidence |
|---------|----------------------|----------|
| CUDA | `CUDA=1` → `backend_cuda.o` + cudart; Windows `CUDA_DLL=1` → `coli_cuda.dll` + `backend_loader.c` | `c/Makefile` ~154–210, ~549, ~573–587 |
| HIP | `HIP=1` same source via `backend_gpu_compat.h`; Windows `HIP_DLL=1` | Makefile ~214–302 |
| Metal | `METAL=1` → `backend_metal.mm` | linked into `colibri` / `inkling` deps |
| Vulkan | `VK=1` / `VULKAN=1` → `backend_vulkan.o` + SPIR-V; primary GPU path for Kimi | Makefile ~567–571, `kimi_k3` target |

**Static FFI today:** `libcolibri.a` = single `colibri.lib.o` without GPU objects; `libkimi_k3.a` without `VK_OBJ`. Enabling GPU in-process means at least:

1. **Feature matrix on Make + build.rs**
   e.g. `COLIBRI_FFI_CUDA=1` / Cargo features `ffi-cuda`, `ffi-metal`, `ffi-vulkan`, `ffi-hip` (or one `ffi-gpu` with platform defaults).
   Build GPU objects with `-fPIC`, archive or link them into the host, add system libs (`cudart`, Metal frameworks, `vulkan`, HIP).

2. **Windows dynamic GPU DLLs already exist for CUDA/HIP**
   Prefer reusing `coli_cuda.dll` / HIP sibling + `backend_loader.c` for process-like isolation of the vendor runtime, rather than static-linking cudart into every desktop binary.

3. **Device ownership rules** (already warned in `ffi-phase-d.md`):
   in-process fault takes the host; OpenMP + CUDA contexts are process-global; host must not nest conflicting runtimes.

4. **Kimi Vulkan**
   Process path strips CUDA for Kimi; FFI GPU for Kimi is almost certainly **Vulkan objects + loader**, not CUDA.

5. **Inkling GPU**
   Separate `backend_cuda_ink.cu` / Metal paths; cannot assume GLM `backend_cuda.o` is enough.

6. **API surface**
   Current `coli_glm_generate` / `coli_kimi_generate` do not expose device selection beyond process env (`GPU_DEV`, `NOGPU`, `COLI_VULKAN`, …). GPU FFI either inherits env (brittle) or grows open options (device index, backend enum).

**Honest minimum for “GPU link residual done”:** document + implement one platform (e.g. Linux CUDA for GLM only) with a cargo feature, doctor/probe honesty, and a smoke open/generate; do **not** claim multi-backend GPU FFI without per-family evidence.

---

## 5. Golden parity: fixtures and process vs FFI

### Fixtures available

| Fixture | Path | Role |
|---------|------|------|
| GLM tiny weights | `c/glm_tiny/` (`config.json`, `model.safetensors`, …) | Real small model for open/generate |
| GLM token oracle | `c/ref_glm.json` (`prompt_ids`, `full_ids`, `tf_pred`) | Transformers reference; from `c/tools/make_glm_oracle.py` |
| V4 tiny + ref | `c/deepseek_v4_tiny/` (`config.json`, `ref.json`, `tokenizer.json`) | Schema’d transformers oracle (`prompt_ids_short`, …) |
| Olmoe ref | `c/ref_olmoe_real.json` | Process bootstrap style |
| C test fixtures | `c/tests/fixtures/` (`glm52_replay_*.json`, kimi wire, e8, ssd vectors) | Not FFI parity harnesses |
| Process integration | `crates/colibri-sys/tests/engine_real.rs` | `#[ignore]` smoke: `COLIBRI_TEST_ENGINE` + `COLIBRI_TEST_MODEL` → `EngineHandle::generate` |
| Placement “golden” | `tests/plan_golden.rs` | Plan geometry only, not tokens |
| Chat templates | `chat.rs` `*_multi_turn_golden` | Prompt formatting, not engine logits |

### How process vs FFI compare today

| Dimension | Process (product) | FFI (opt-in) |
|-----------|-------------------|--------------|
| Entry | SERVE mux SUBMIT / DATA / DONE | `coli_*_generate` token callback |
| Stop/cancel | `STOP` / `CANCEL` mid-turn | CB return non-zero only (partial); no req_id cancel |
| Visual | stdout EMAP/HITS/TIERS/HWINFO/PROF | **None** on C API |
| Multi-slot KV | mux slots / env | **None** |
| Golden test | C/Python oracles + ignored Rust smoke | **Only** open + `disk_bytes` on `glm_tiny` |
| Full production weights | operator machines | **Not run** (impl honesty) |

**Concrete parity recipe (not implemented):**

1. Build process binary and `feature = "ffi"` host on same machine/flags.
2. For `c/glm_tiny` + `ref_glm.json`: run greedy (`temperature=0`) process generate and FFI generate on same prompt ids; assert token sequence match (or match `tf_pred` / `full_ids` as C unit tests do).
3. Same for V4 tiny + `deepseek_v4_tiny/ref.json` via `coli_v4_session_generate`.
4. Optional: Kimi needs a tiny fixture (may not ship in-tree weights; skip if absent).
5. Full-weight parity remains optional/expensive; residual can close “tiny golden” without multi-hundred-GB runs.

---

## 6. Desktop product default (`colibri-native`)

| Fact | Evidence |
|------|----------|
| **Process-only today** | `colibri-native/Cargo.toml`: `colibri-sys` features `runtime`, `stream`, `tokio` only; **no** `ffi` |
| Engine start | `host.rs` ~986–989: `EngineHandle::start_with_plan` / `start_blocking` only |
| Config | No use of `prefer_process(false)` / `prefer_ffi_path` in native sources |
| Fidelity doc | Still “not true FFI”; architecture diagram is process-only (correct for shipped desktop) |

### What opt-in would look like

1. Cargo: `colibri-native` optional feature
   `ffi = ["colibri-sys/ffi"]` (not default).
2. Build: desktop builds with `ffi` must have a C toolchain + OpenMP; ship size jumps with three static archives.
3. Runtime: if `cfg.prefer_ffi_path()` and family maps via `FfiFamily::from_model_family`, call `open_engine`; on error fall back to process (policy already in docs).
4. Product UX gap: native chat/brain/stop are built around **mux** duplex. Pure FFI generate has no visual pump or mid-stream STOP unless you add a parallel path or re-home visual to C callbacks.
5. Kill-switch: `COLIBRI_FORCE_PROCESS=1` must keep process path for operator escape.

**Until (3)+(4) exist, “product default FFI” is not a one-line `prefer_process = false`.** Desktop can enable link + experimental settings, but SPA-class Brain/PROF/stop remain process-path features.

---

## 7. Concrete phased plan

### Option A — Fully close residual as originally scoped (large)

| Phase | Deliverable | Done when |
|-------|-------------|-----------|
| **D1 Inkling CPU** | `INKLING_NO_MAIN` / `libinkling.a`, `coli_ink_*` in API, `FfiFamily::Inkling`, build.rs link, open/size/generate tests | Inkling in `linked_families()`; open smoke on tiny if fixture exists |
| **D2 Tiny golden parity** | Test: process vs FFI token match on `glm_tiny` + optional V4 tiny | Named test green; documented command |
| **D3 Desktop opt-in (not default)** | `colibri-native` feature `ffi` + settings + open-fail→process fallback | Manual path works; default still process |
| **D4 Visual / stop ABI (optional but product-blocking for default)** | C poll or callbacks for EMAP/HITS; cancel; wire native pump | Brain/stop work without mux **or** document permanent process requirement for those panels |
| **D5 GPU (platform slice)** | e.g. Linux CUDA static/dynamic for GLM only | Feature flag; smoke generate; doctor honesty |
| **D6 Product default** | Flip `prefer_process` only after D3+D4 + crash-isolation policy | Residual fully closed; fidelity/docs updated |

This is multi-wave campaign work, not a mop.

### Option B — Honest partial close (recommended residual hygiene)

**Close or rename** `open:ffi-phase-d` with acceptance = current multi-family CPU static + size + kill-switch + process default (already met per `ffi-phase-d.md` acceptance §1–5).

**Spin new open ids:**

| New id | Scope |
|--------|--------|
| `open:ffi-inkling` | Inkling extract + family |
| `open:ffi-golden-tiny` | Process↔FFI token parity on tiny fixtures |
| `open:ffi-desktop-optin` | Native feature + fallback (still process default) |
| `open:ffi-visual-abi` | Visual/stop/multi-slot without mux (if product wants it) |
| `open:ffi-gpu` | Explicit platform matrix; start with one backend |
| keep | `open:npu-inference` separate |

**Partial-close criteria (copy into residual when operator accepts):**

1. Three CPU static libs build; no `main` in archives.
2. `feature = "ffi"` links all three; `FfiFamily` / `open_engine` / size_info.
3. `prefer_process` default true; `COLIBRI_FORCE_PROCESS` tested.
4. Size fields always on public types without `ffi`.
5. Docs state process is product default; multi-family FFI opt-in only.
6. Explicit non-goals listed as separate residuals (above).

Also fix **stale** `colibri-native/docs/fidelity.md` FFI rows as a docs mop when residual is redefined.

### Option C — Medium close (CPU families complete)

Treat residual closed when **D1 + D2** land, and keep desktop default / GPU as separate residuals. Matches “multi-family” wording better than Option B, still refuses product-default claim.

---

## 8. Risks

| Risk | Why it matters | Mitigation |
|------|----------------|------------|
| **Process isolation loss** | In-process OOM / SIGSEGV / bad weights kill the GPUI host | Keep process default; kill-switch; reserve FFI for trusted local models / tools |
| **Dual-path drift** | Mux protocol gains STOP/grammar/slots/visual; FFI API is open/generate only | Single SoT tests; either grow C API or never make FFI product default for desktop UX |
| **Build complexity** | Three archives + OpenMP + optional GPU + Windows DLL matrix; long `build.rs` makes | Prebuilt env overrides already exist; keep GPU out of default `ffi`; CI matrix per feature |
| **OpenMP re-exec / global state** | Process path uses `COLI_NO_OMP_TUNE`; static lib shares process OpenMP with host | Document one owner thread; avoid nested runtimes |
| **Stale docs** | fidelity.md still “stub” undermines honesty | Same-turn doc fix when residual reclassified |
| **Size / ship cost** | Linking V4+GLM+Kimi static into desktop bloats binary | Keep `ffi` off native default; optional feature or dynamic load later |
| **Incomplete generate parity** | Open smoke ≠ correct tokens | Tiny golden (D2) before trusting FFI for quality |
| **Inkling / Olmoe edge cases** | Audio, special formats; Olmoe→Glm mapping may be wrong for some dirs | Explicit family detection; refuse open with clear error |

---

## Architecture (current default)

```
GPUI / colibri-native
  └── colibri-sys (in-process host: probe, plan, doctor, duplex)
        ├── EngineHandle ──spawn──► C process (SERVE mux)   ← product default
        └── feature "ffi" ──link──► coli_glm_* / coli_kimi_* / coli_v4_*  ← opt-in CPU
```

Host in-process ≠ engine in-process. Residual close must not collapse that distinction without D4+D6.

---

## Recommended next step for the operator

1. **Accept Option B** (redefine partial close + split residuals) unless full product-default FFI is a hard goal.
2. If still chasing “complete multi-family,” schedule **Option C: D1 Inkling + D2 tiny golden** next.
3. Do **not** flip desktop default until visual/stop story is decided (process forever for Brain, or new ABI).

---

## File index (absolute)

| Role | Path |
|------|------|
| Residual | `/home/hunter/Projects/surmount/colibri/.agents/RESIDUAL.md` |
| Phase D SoT | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/ffi-phase-d.md` |
| Impl report | `/home/hunter/Projects/surmount/colibri/.agents/reports/impl-track-ffi-libcolibri.md` |
| Prior recon (pre-ship; historical) | `/home/hunter/Projects/surmount/colibri/.agents/reports/recon-plan-ffi-libcolibri.md` |
| C API | `/home/hunter/Projects/surmount/colibri/c/colibri_api.h` |
| Rust FFI | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/ffi/` |
| build.rs | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/build.rs` |
| Config kill-switch | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/config.rs` |
| Native host | `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/host.rs` |
| Native Cargo | `/home/hunter/Projects/surmount/colibri/crates/colibri-native/Cargo.toml` |
| Fidelity (stale FFI rows) | `/home/hunter/Projects/surmount/colibri/crates/colibri-native/docs/fidelity.md` |
