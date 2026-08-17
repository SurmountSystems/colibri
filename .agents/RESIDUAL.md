# Residual: native desktop / colibri-sys host campaign

**Updated:** 2026-08-13 (harden review closed at 0 product issues; last pass was test quality only; highest-value-next note)
**Scope:** colibri-sys + colibri-native embed path (not Tauri SPA).

Living list of what is closed vs still open after Phases A–F (chrome, AMD/ROCm,
registry/install cancel, inference controls, observability polish, deep doctor
UI, docs), Brain atlas, SPA visual parity, Phase D multi-family CPU static FFI
(D0–D3), GLM embed visual poll (`open:ffi-visual-abi`), native product default
flip (`open:ffi-product-default`), one-platform Linux CUDA embed
(`open:ffi-gpu`), and native setup wizard + Tools tab + DOGE/mint themes +
determinate progress. Chat is not authority; this file is **open product residual
only**. Standing process rules live in project root **`AGENTS.md`** (including
product copy fidelity).

---

## CLOSED (this campaign)

| Item | Where |
|------|--------|
| Stop/cancel sys: unlock generate, explicit req_id on SUBMIT, STOP vs CANCEL wire | `colibri-sys` engine/serve/duplex; 72 lib tests |
| `EngineHandle` Clone; mid-stream stop via `with_client` | `engine/mod.rs` |
| GPUI Stop button → mux STOP with active req_id; status `stopped` on Done | `colibri-native` host + main |
| Session stays in slot during generate (no take-out orphan) | `host::EngineSession::generate_async` |
| Visual pump timer while engine up (`pump_visual` ~500ms) | main.rs |
| Live tiers strip (VRAM/RAM/disk expert counts) | main + `format_live_tiers` |
| PROF strip (last N turns, tok/s, phase times) | main + `format_profile_turns` (column labels polished Phase E) |
| Brain panel: tier/heat cells + hits pulse; stride sample ≤2048 cells | main + `brain_view_from_map` |
| Heat lum matches web `min(heat/24, 1)` | `brain_cell_rgb` |
| Hits pulse multi-frame decay (web RAF `*= 0.94`, scaled to pump ms) | `apply_brain_pulse_decay` + pump |
| Brain full atlas: web-shaped `experts.json`, hover tips, depth roles, full-grid toggle | `atlas.rs` + main `brain_panel`; residual `open:brain-full-atlas` closed |
| Live engine HWINFO strip (plain labels; updates with visual pump) | `format_live_hwinfo` + main strip |
| HF install feature on desktop (`install` default); form + progress channel; UI hub-prefer for counters | Cargo.toml, host, main |
| Phase D multi-family CPU static FFI **complete** (`open:ffi-phase-d`) | GLM / Kimi / V4 / **Inkling** static libs; size metadata; kill-switch; tiny golden process↔FFI; desktop feature + process fallback. See `crates/colibri-sys/docs/ffi-phase-d.md`; report `.agents/reports/impl-ffi-d6-closeout.md` |
| Embed visual poll ABI (`open:ffi-visual-abi`) | C `coli_*_visual_poll` + Rust `FfiEngine::pump_visual` + native FFI pump. **GLM full fill** (EMAP/HITS/PROF/HWINFO/TIERS without SERVE child). Kimi/Inkling stub empty success; V4 empty. Cooperative cancel via token callback only; mux multi-slot STOP remains process-only. Reports: `.agents/reports/impl-ffi-visual-c-api.md`, `impl-ffi-visual-rust-native.md`, `impl-ffi-visual-docs-residual.md` |
| Native product-default FFI (`open:ffi-product-default`) | **Native-host only:** `colibri-native` with `feature=ffi` defaults try FFI first (`resolve_prefer_process` → `prefer_process=false`); process fallback on open failure; `COLIBRI_FORCE_PROCESS` always wins. Crate `ColibriConfig.prefer_process` stays **true** for library embeds. Isolation story documented in `ffi-phase-d.md`. Report: `.agents/reports/impl-ffi-product-default.md` |
| One-platform GPU embed (`open:ffi-gpu`) | **Linux CUDA + GLM** closed first (report `impl-ffi-gpu-one-platform.md`). **HIP later:** Cargo `ffi-hip` + process HIP + UMA planner closed under plan `plan-rocm-unified-ddr5` (see ROCm closed section below). Default `feature=ffi` stays **CPU-only** until `ffi-cuda` or `ffi-hip`. Mutual exclusion: one vendor per build. Still not claimed in that original CUDA slice alone: Metal/Vulkan/NPU; multi-family GPU static. |
| Fidelity matrix rows updated | `crates/colibri-native/docs/fidelity.md` |
| Phase C lifecycle: registry picker UI, install cancel, min free gate | colibri-sys install/registry; colibri-native host + main |
| Phase D inference UX: temperature (0–2), max tokens (1–32768), reasoning toggle | `GenerateControls` + Inference panel; chat template `enable_thinking` |
| Sticky multi-slot / `cache_slot` UI | `switch_cache_slot_transcript`; Submit uses selected slot; `COLIBRI_KV_SLOTS` / `KV_SLOTS` |
| GBNF grammar on ClientFrame Submit + duplex → mux | `ClientFrame::Submit.grammar`; PROTOCOL_VERSION 2; duplex tests |
| Deep doctor UI (`open:deep-doctor-ui`) | GPUI **Deep check** button + `run_deep_doctor` / `run_doctor_checks`; checklist shows Depth: quick / thorough |
| Production-facing docs for local embed MVP | native README, fidelity A–F, sys user-guide AMD/ROCm + install cancel + grammar/slot |
| SPA visual parity (`open:tauri-parity`) | Layout-density parity for native vs `web/`: left rail + Chat/Brain/Profiling tabs, chat hero + prompts, topbar badges, tier bar, Profiling charts; i18n en+it. Theme default is now **DOGE** (mint still available) |
| Native UI prefs TOML (`feat:native-prefs-toml`) | `prefs.rs`: primary `native-ui.toml` write; load TOML then sibling `native-ui.json` then defaults; save always TOML, leave JSON in place; first_run, theme, locale, last_model_path; `COLIBRI_THEME` / `COLIBRI_SKIP_WIZARD`. No colibri-sys engine config file. Report: `impl-native-config-toml.md` |
| Doctor engine readiness wording (`feat:doctor-engine-readiness`) | `engine.binary`: process ready / not executable / **in-process available** / **external engine program not found** (never bare "not built"); tilde expand on model/engine paths; wizard step 4 **Thorough check** → deep doctor. Report: `impl-doctor-engine-readiness.md` |
| DOGE default + mint palettes (`feat:native-doge-theme`) | `theme.rs` ThemeId/ThemePalette; Brain + Profiling palette-aware; DOGE eight pure colors; mint = prior SPA tokens |
| Tools panel + slim rail (`feat:native-tools-panel`) | Main tab **Tools**; rail keeps lifecycle + live strips; clutter (doctor, install, theme, locale, About, advanced controls) on Tools |
| Setup wizard first-run (`feat:native-setup-wizard`) | `wizard.rs` six steps; first-run gate; Skip/Finish persist `first_run_done`; re-open Setup; review fixes (save-error status, plain readiness copy, single model-input parent) |
| Determinate progress (`feat:native-determinate-progress`) | `progress.rs` + install/generate strips (bar / % / ETA); hub prefer for counters; phase floors; Done keeps 100% until next run; sys hub byte/file fill |
| In-process FFI compute niceness (`open:ffi-inprocess-priority`) | Process-mode child already niced (`ENGINE_CHILD_NICE=10`). FFI now nices the current **thread** plus the OpenMP team at the same nice (start worker + generate worker + `coli_nice_compute_threads`). GPUI, stderr tee, and visual pump stay default. HIP/ROCm kernels are not scheduled by nice. Niceness does not stop systemd-oomd. Report: `.agents/reports/impl-compute-low-nice-ffi.md` |
| Composer word/select chords (`open:native-composer-keys`) | `text_input.rs`: word move/select/delete, Shift-select, Home/End buffer start/end, Ctrl+Home/End aliases. Report: `.agents/reports/impl-native-harden-ffi.md` |
| Session identity + RSS heartbeat (`open:native-session-heartbeat`) | Start/generate lines include pid, comm, cgroup leaf, `kind`, flavor `cpu`/`HIP`/`CUDA`. Heartbeat every 8s while the engine is up; one short line on log init. No prompts. Report: `.agents/reports/impl-native-harden-ffi.md` |
| FFI RAM clamp + Start refuse (`open:ffi-ram-clamp`) | Embed samples MemAvailable before `model_init`, then `cap_for_ram` (error, not `exit(2)`); refuse tears down the Model. Host preflight refuses Start when one expert slot cannot fit; inspect fail is fail-closed. Floor fits → clamp cache to ~88% RAM. `COLI_RAM_OVERCOMMIT=1` override. Doctor `memory_ram_capacity_tight_is_warn_not_fail` stays warn. `just install` stays CPU `ffi`. Report: `.agents/reports/impl-native-harden-ffi.md` |
| FFI embed Stop in C (`open:ffi-embed-cancel`) | `spec_decode` honors `g_embed_stop`. Prefill honors it on the default path: `layers_forward` checks between layers, and after a `COLI_PREFILL_CHUNK` break leftover `layers_forward` is skipped (`coli_prefill_should_run_leftover`). Cooperative: soon, not inside one matmul. |

---

## OPEN

### High value / product

| Id | Gap | Notes |
|----|-----|--------|
| `open:npu-inference` | Drive inference on detected NPUs (XDNA / Ryzen AI / …) | **Deferred.** Probe/doctor may list NPU inventory; no NPU decode path in this campaign. |

### Strategic / deferred

| Id | Gap | Notes |
|----|-----|--------|
| `open:openai-rest` | Local OpenAI REST from native host | Intentionally absent |

### Process / polish

| Id | Gap | Notes |
|----|-----|--------|
| `open:visual-pump-idle-stop` | Pump loop already exits when the engine session is cleared; **no** explicit Join/cancel handle on app drop | Left open: clean Join needs storing a GPUI task handle and cancel-on-drop; not a one-line fix without redesign |
| `open:generate-progress-redesign` | Generate % still uses max-tokens as denominator (not measured completion tokens alone) | **Deferred** after wizard review; Done strip paints 100%; full redesign out of scope |
| `open:hub-mid-file-byte-progress` | Hub install progress updates per file before that shard finishes downloading | Sys path; finer mid-file bytes not claimed |

### Later / operator-gated (not this harden plan)

The approved harden plan landed. These were **looked at, later** on purpose. They are not high-value residual unless you ask.

| Id | Gap | Notes |
|----|-----|--------|
| `later:mmap-without-touch` | File-backed experts via `COLI_MMAP` **without** `madvise(WILLNEED)` + touch every 4K page | Default load stays pread + malloc. Opt-in mmap today faults every expert in. Do not treat mmap flags as an oomd fence. |
| `later:hip-explicit-rebuild` | Explicit `just install features=install,ffi-hip` on this ROCm host | Operator-gated. Default `just install` stays CPU `ffi`. Clamp + flavor log must stay. |
| `later:cgroup-memory-max` | systemd `MemoryMax` on the app scope so a runaway cannot take `session.slice` | Real fence later. Process mode still does not isolate oomd without a memory cap. |

### Highest value next

The native harden plan is finished (composer keys, session heartbeat, FFI RAM
clamp + Start refuse, embed cancel). A later review closed at zero open product
issues. The last pass only tightened tests: crate-level FFI test lock, fail-hard
`glm_tiny` preload inject, and the exact `ENGINE_START_RAM_UNMEASURABLE` Start
string. Doctor `memory.ram` stays warn.

Do not start an auto implement loop on operator-gated leftovers. An explicit HIP
rebuild (`just install features=install,ffi-hip`) is the operator's TTY action
if they want live HIP generate on this ROCm host. systemd `MemoryMax` and
mmap-without-touch stay later unless they ask.

The remaining OPEN items are deferred product (NPU inference, local OpenAI REST)
or polish (visual-pump join-on-drop, generate % redesign, hub mid-file byte
progress). None of those unblock the local-embed journey that already shipped.

---

## Production MVP status (local embed)

**Complete for the local-embed journey (Phases A–F):** product chrome, AMD/ROCm
probe + doctor awareness, registry + install cancel + min free, inference
controls (temperature / max tokens / reasoning / grammar / multi-slot), live
tiers + HWINFO + PROF + Brain atlas (hover + sample/full toggle), deep doctor UI,
production docs.

**Phase D multi-family CPU FFI:** **closed** for GLM, Kimi, V4, and Inkling
static libs, size metadata, kill-switch, tiny golden process↔FFI parity, and
desktop feature with process fallback.

**Visual poll (`open:ffi-visual-abi`):** **closed.** Brain / live PROF / HWINFO /
TIERS work on pure FFI for **GLM** without a SERVE child (cooperative cancel
only). Still not claimed: full Kimi/Inkling visual fill (stubs), V4 poll
symbols (empty), mux multi-slot STOP on pure FFI, NPU inference
(`open:npu-inference`, deferred).

**Native product default (`open:ffi-product-default`):** **closed.** When
`colibri-native` is built with `feature=ffi`, start tries FFI first (no
`COLIBRI_PREFER_FFI` required). Process fallback on open failure;
`COLIBRI_FORCE_PROCESS` always forces process. Crate
`ColibriConfig.prefer_process` remains **true** for library embeds.

**GPU embed (`open:ffi-gpu`):** **closed** for Linux GLM GPU embed on **CUDA**
and **HIP** (one vendor per build). Default `feature=ffi` archives are
**CPU-only**. Opt-in CUDA: `feature=ffi-cuda` / `COLIBRI_FFI_CUDA=1` and
`make libcolibri CUDA=1`. Opt-in HIP: `feature=ffi-hip` / process
`make … HIP=1` (see ROCm section). Without a matching toolkit the build falls
back to CPU (CI-safe). Still not claimed: multi-family GPU static, Metal/Vulkan
FFI static. NPU: inventory only (`open:npu-inference`).

**ROCm HIP + UMA (plan `plan-rocm-unified-ddr5`):** **closed for product code
bar** (2026-08-11). Process HIP path, Cargo `ffi-hip` in-process embed, UMA
inventory/planner, and doctor honesty landed. Reports:
`impl-rocm-hip-process-path.md`, `impl-rocm-ffi-hip.md`,
`impl-rocm-uma-inventory-plan.md`, `impl-rocm-uma-runtime-smoke.md`,
`impl-rocm-uma-docs-residual.md`. Default `ffi` without `ffi-hip` stays
CPU-only. Live host generate / full TIERS smoke on ROCm APU remains
**operator-gated** (model + local ROCm). Open follow-ups from review: stronger
UMA hot envelope vs dense/runtime (optional clamp), hybrid iGPU+dGPU warm
subtract, soft UMA name heuristics on ambiguous discrete labels. Vulkan as
primary accelerator: not in this campaign (optional later honesty only).

**Isolation (accepted for native FFI-first):** in-process engine fault can kill
the host. Crash isolation (process) is not oomd isolation. A serve child in the
same user slice can still fill RAM. Embed samples MemAvailable before
`model_init` (same as the CLI) and runs `cap_for_ram` after load; a refuse
returns an error (does not `exit(2)`) and tears down the Model. Native Start
preflight refuses before open when one expert slot cannot fit. Inspect failure
fails closed unless overcommit is on. A C RAM refuse or a cooperative
`stopped` generate does not start a process fallback. `COLI_RAM_OVERCOMMIT=1`
remains the override. Operators who need process isolation for SIGSEGV set
`COLIBRI_FORCE_PROCESS=1` or build without `feature=ffi`. Prefer process for
long-running / untrusted workloads. See
`crates/colibri-sys/docs/ffi-phase-d.md`. CPU FFI compute threads and the
OpenMP team are niced (`open:ffi-inprocess-priority` closed); HIP kernels are
not. Niceness does not reduce RSS and will not stop systemd-oomd. There is no
cgroup `MemoryMax` yet. Default load is still pread plus malloc (`COLI_MMAP`
is not enabled). Session heartbeat logs pid, flavor, RSS, and swap every 8s
while the engine is up.

**Closed earlier:** full Brain atlas (`open:brain-full-atlas`); SPA visual /
layout-density parity for native (`open:tauri-parity`).

**Native shell UX (wizard / Tools / theme / progress):** **closed** for the
approved plan. First-run setup wizard, Tools tab + slim rail, DOGE default +
mint, TOML prefs (JSON load compat), determinate install/generate progress, and
review fixes (save-error status, plain readiness copy, install progress floors,
Done 100% hold, DOGE tab/selection/legend). Doctor readiness: FFI-aware engine
wording, `~` path expand, wizard Thorough check. Honest deferred polish only:
generate % redesign, hub mid-file bytes. Reports:
`.agents/reports/impl-tools-and-wizard.md`, `impl-native-wizard-review-fix.md`,
`impl-theme-palettes.md`, `impl-install-generate-progress.md`,
`impl-doctor-engine-readiness.md`, `impl-native-config-toml.md`,
`process-mop-native-wizard.md`, `process-mop-readiness-config.md`.

---

## Architecture reminder (do not regress)

```
GPUI → colibri-sys (in-process host)
         ├── feature=ffi CPU static open_engine + pump_visual   [native default when linked]
         │     └── optional ffi-hip / ffi-cuda (Linux GLM; one vendor)
         │     └── GLM: coli_glm_visual_poll (Kimi/Inkling stub; V4 empty)
         │     └── on open failure → process path
         └── ServeClient mux → C engine process   [no feature=ffi, FORCE_PROCESS, or fallback]
              └── process HIP=1 / CUDA=1 when that binary is GPU-linked
```

- **Host in-process** is always true for colibri-sys; **engine in-process** only
  when native `feature=ffi` succeeds (else process).
- Default `ffi` is **CPU-only** for GPU kernels until `ffi-hip` or `ffi-cuda`.
- rkyv duplex is app↔host frames, not REST.
- GLM FFI: Brain / PROF / HWINFO / TIERS without SERVE child; cancel is
  cooperative (token callback), not mux multi-slot STOP.
- Library embeds stay process-prefer until they set `prefer_process = false`.
- UMA/APU: plan hot experts from shared system RAM; discrete: free VRAM − 2 GiB.
