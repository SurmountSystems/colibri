# Fidelity matrix: Python / Tauri desktop → colibri-sys → GPUI

Maps the flows a desktop user needed via HTTP/Python to the colibri-sys API this
native host uses, and the status of the GPUI surface.

Status legend:

- **done**: implemented and wired in this crate
- **partial**: API exists; UI is summary-only or incomplete
- **missing**: not in this MVP (still Python/Tauri/HTTP or not ported)

| Python / desktop feature | colibri-sys API | GPUI status | Notes |
|--------------------------|-----------------|-------------|-------|
| Machine inventory (`coli info` / probe) | `MachineInfo::probe` / `probe_with` / `probe_for_config` | **done** | RAM, swap, cores, generation_hint, SIMD present, model_store free_bytes, GPU/NPU counts |
| Doctor (`coli doctor`) | `run_doctor`, `DoctorOptions`, `DoctorReport` | **done** | Quick **Run checks** on start + button; **Deep check** / wizard **Doctor** runs thorough validation (`DoctorOptions.deep`). Empty path → **Overall: Idle** (host; no cwd probe). Missing path → short recovery (default store + Scan / Install), not a long fail dump; Doctor/Quick check scan the store and auto-set when exactly one model is found |
| Placement plan (`coli plan`) | `PlacementPlan::build`, `PlanOptions` | **done** | Summary text (policy, hit%, bottleneck, RAM budget, warnings) |
| Model path / store | `ColibriConfig::model`, `model_store`, env `COLIBRI_MODEL` / `COLI_MODEL` | **done** | Cold start: env → existing prefs path → single store model → first of many → **default store** (`~/.local/share/colibri/models` / platform); not a random `~/.models`. Install sets path on success |
| Chat templates | `render_chat` / `render_chat_simple`, `ChatMessage`, `ModelFamily` | **done** | Text multi-turn; tools / Inkling audio not ported |
| Generate (non-HTTP) | `EngineHandle` + `EngineDuplex` + `ClientFrame` / `ServerFrame` | **done** | Token stream via background thread + channel; session stays in slot |
| Engine spawn (serve mux) | `EngineHandle::start_blocking` / `start_with_plan` | **done** | C subprocess; used without `feature=ffi`, under `COLIBRI_FORCE_PROCESS`, or after FFI open failure |
| OpenAI REST `/v1/chat/completions` | *(not in colibri-sys by design)* | **missing** | Intentionally absent; native path replaces HTTP |
| SSE streaming from gateway | mux `DATA` → `ServerFrame::Token` | **done** | App-side rkyv frames, not SSE |
| Brain / EMAP / HITS poll | `EngineHandle::expert_map` / visual frames, `pump_visual`; opt-in FFI: `FfiEngine::pump_visual` | **done** | Live grid + hits pulse (web RAF `*=0.94`); heat `min(heat/24,1)`; hover tips (atlas affinities or depth-role fallback); default sample ≤2048 with **Full grid** toggle / `COLIBRI_BRAIN_FULL=1`; atlas via `COLIBRI_EXPERTS_JSON` / cwd `experts.json`. **Process path:** serve mux frames. **Opt-in FFI (GLM):** `coli_glm_visual_poll` without SERVE child; Kimi/Inkling stub empty; V4 empty |
| Profile / PROF turns | `pump_visual` / profile window; opt-in FFI: last embed PROF via poll | **done** | Profiling **page** (tab): phase share bars, tok/s + stacked phase columns, reverse turn table (web phase model); text strip helper retained for tests. GLM FFI fills after generate; stubs/V4 empty until family fill |
| HWINFO live strip | `ServerFrame::Hwinfo` / `hwinfo()`; opt-in FFI: poll HWINFO | **done** | Live strip in rail Runtime section; plain labels (RAM free/total, cores, CPU/GPU names, VRAM); updates with visual pump (process mux or GLM FFI poll) |
| Tiers bar | `tiers()` / live `TiersSnap`; opt-in FFI: poll TIERS | **done** | Proportional VRAM/RAM/disk bar + legend + counts text (mint / blue / slate); GLM FFI via poll |
| SPA shell chrome | *(layout only)* | **done** | Slim left rail (~292) + top tabs **Chat \| Brain \| Profiling \| Tools**; chat hero + suggested prompts; topbar live badges (tokens, tok/s, TTFT); i18n en + it |
| Setup wizard (first-run) | `prefs::NativePrefs` + `wizard` module | **done** | First launch when `!first_run_done` and not `COLIBRI_SKIP_WIZARD`; steps Welcome → machine → model (scan/optional download) → **Doctor** (**Run doctor** / **Quick check** / **Scan for models** / **Install a model**, helper line, compact missing-path Health check) → look (theme) → Ready; Skip/Finish set `first_run_done`; first-run rail Setup slab then hides; re-open via **Setup** on Tools |
| UI prefs (TOML) | `prefs` module | **done** | `~/.config/colibri/native-ui.toml` (`first_run_done`, `theme`, `locale`, `last_model_path`); load falls back to sibling `native-ui.json` if TOML missing/invalid; saves always TOML; `COLIBRI_THEME` / `COLIBRI_SKIP_WIZARD` env overrides |
| Theme (DOGE default + mint) | `theme::ThemeId` / `ThemePalette` | **done** | Default **DOGE** (eight pure colors; see [0001_DOGE.md](https://github.com/SurmountSystems/specs/blob/main/0001_DOGE.md)); **mint** keeps prior SPA tokens; Tools + wizard switch live and save prefs; Brain + Profiling use palette (DOGE phase map is pure eight) |
| Tools tab | *(layout only)* | **done** | Machine details, doctor (quick + deep), plan/scan/registry, HF install, theme, language, About, muted Setup reopen, advanced inference (reasoning, slots, grammar); rail keeps model path, Start/Stop, live tiers/HWINFO, temp/max tokens; giant green Setup slab only before first-run is done |
| Determinate progress | `progress` helpers + host install/generate | **done** | Shared strip: thick bar + **%** + ETA for **generate** (max tokens + tok/s) and **HF install** (bytes preferred, else files; hub path for counters; phase floors so CLI/no-counter and inspect/register stay non-zero). Done keeps 100% until next run or error clear |
| Tauri webview SPA chrome | *(web/)* | **partial** | Different product path (`desktop/`); native matches design system / layout density (“same product family”), not pixel-perfect CSS |
| Model install (HF) | `model::install` (feature `install`) | **done** | Form on Tools (and wizard expand): repo, revision, dest under model store (absolute only if under store; `..` rejected); progress channel; UI defaults hub path for determinate counters (`prefer_cli: false`); **Pause** (cooperative after current file + indeterminate wait) / **Resume** (skip complete shards by size) / Cancel; always-visible **Min free disk (GB)** label (default 1; `0` = gate off) |
| Model registry scan | `ModelRegistry` | **done** | Scan models under store roots to depth ≤2 for dirs with `config.json` (cap 64); empty scan status is one short line (store + depth); recovery CTAs are the Doctor step buttons |
| Multi-slot / `cache_slot` sticky | `ClientFrame::Submit { slot }` + sticky transcripts | **done** | Session slot Prev/Next when `kv_slots > 1`; conversation follows slot; env `COLIBRI_KV_SLOTS` / `KV_SLOTS` |
| Temperature (0–2) | `GenerateControls` / Submit `temperature` | **done** | Inference panel field; clamped |
| Max tokens (1–32768) | `GenerateControls` / Submit `max_tokens` | **done** | Inference panel field; clamped |
| Reasoning toggle | `ChatRenderOptions.enable_thinking` | **done** | Host applies chat template before Submit |
| Anthropic Messages rewrite | *(Python only)* | **missing** | Out of scope for native embed |
| Grammar / GBNF on Submit | `ClientFrame::Submit.grammar` → `GenerateRequest.grammar` | **done** | PROTOCOL_VERSION 2; optional GBNF field on Inference panel |
| Stop / cancel mid-generate | `ClientFrame::Stop` / `stop_request`; FFI: cooperative token-cb cancel | **done** | Process: Stop button → mux `STOP` with active req_id; status `stopped` on Done. **Opt-in FFI:** cooperative cancel only (token callback `Err` / non-zero); no mux multi-slot STOP on pure FFI |
| Auth / API key | N/A (no HTTP) | **done** (n/a) | No local gateway to protect |
| Multi-family CPU static FFI | `feature = "ffi"`: `open_engine`, `FfiFamily` (GLM / Kimi / Inkling / V4), `FfiEngine::pump_visual` | **done** (native default under `feature=ffi`) | Cargo `ffi` links multi-family **CPU-only** static engines by default. **Native host** (`colibri-native`) defaults to try FFI first when built with `feature=ffi` (no `COLIBRI_PREFER_FFI` required); open/generate fall back to process on failure; `COLIBRI_FORCE_PROCESS=1` always forces process. Crate `ColibriConfig.prefer_process` remains **true** for library embeds. **Brain / live PROF / HWINFO / TIERS on pure FFI (GLM):** visual poll ABI shipped (`open:ffi-visual-abi` closed); Kimi/Inkling stub; V4 empty. Mid-generate cancel is cooperative (token cb), not mux multi-slot STOP. **GPU:** opt-in Linux CUDA (`ffi-cuda`) or HIP (`ffi-hip` / native feature `ffi-hip`) for GLM; default `ffi` CPU-only; vendors mutually exclusive. Isolation: in-process fault can kill host (documented; kill-switch + process fallback remain). Residual `open:ffi-product-default` **closed**. |

## Architecture (honest)

```
GPUI window (this crate)
    └── colibri-sys (in-process host library)
            ├── probe / plan / doctor / chat templates / optional HF install
            ├── feature = "ffi" CPU static open_engine   [native default when linked]
            │       └── pump_visual → coli_*_visual_poll (GLM full; Kimi/Inkling stub; V4 empty)
            │       └── on open failure → process path below
            └── EngineDuplex (rkyv ClientFrame ↔ ServerFrame)
                    └── ServeClient line protocol
                            └── C engine process (stdin/stdout, SERVE=1)
                                [no feature=ffi, FORCE_PROCESS, or FFI open failure]
```

| Phrase | Meaning |
|--------|---------|
| **Host in-process** | colibri-sys is linked into the GPUI binary |
| **Engine process** | Decode in a separate C binary (serve mux); used without `feature=ffi`, under `COLIBRI_FORCE_PROCESS`, or after FFI open failure |
| **Not REST** | No OpenAI gateway; no SSE |
| **Native FFI-first** | Cargo `ffi` links multi-family **CPU-only** static libs by default; opt-in `ffi-cuda` (NVIDIA) or `ffi-hip` (AMD/ROCm) for Linux GLM embed (one vendor); **colibri-native** defaults to try FFI first; kill-switch `COLIBRI_FORCE_PROCESS`; process fallback on open failure |
| **Library embeds** | `ColibriConfig.prefer_process` still defaults **true** (process-prefer) until the embed opts out |
| **Visual (GLM FFI)** | Live Brain, PROF, HWINFO, TIERS via embed poll on pure FFI (no SERVE child). Kimi/Inkling stub; V4 empty |
| **STOP** | Process: mux `STOP` with `req_id`. FFI: cooperative token-callback cancel only (no multi-slot mux STOP) |
| **Isolation** | In-process fault can kill the host; documented accept for native `feature=ffi` default; `COLIBRI_FORCE_PROCESS` + process fallback remain |

Python `c/openai_server.py` is the HTTP face of the **same** mux. This host does
not start that server. Phase D: multi-family CPU FFI complete; **native host**
defaults to FFI when built with `feature=ffi` (`open:ffi-product-default`
closed). See colibri-sys [ffi-phase-d.md](../../colibri-sys/docs/ffi-phase-d.md).

## Brain panel

Full MoE maps can exceed 10k experts (GLM-style ~76×256 ≈ 19k).

| Mode | Behavior |
|------|----------|
| **Default sample** | Stride-sample to ≤ **2048** cells (`BRAIN_MAX_CELLS`) so the div grid stays light |
| **Full grid** | UI **Full grid** toggle or env `COLIBRI_BRAIN_FULL` / `COLI_BRAIN_FULL` = `1`/`true`/`yes` paints every expert (one GPUI div per cell; large maps may feel heavy) |

Color is tier (disk/RAM/VRAM) with heat brightness (`heat/24` saturation, matching
web). Hits flash when `hits_seq` changes and then **decay** across visual pump
ticks using the web RAF curve (`*= 0.94` per ~16 ms step, batched for the ~500 ms
pump).

### Hover atlas

- Hover a cell: tip shows layer/expert (GLM map: last row → MTP layer 78, else
  `row+3`), tier, heat, then either measured affinity from `experts.json` or a
  depth-role fallback (early / lower-middle / upper-middle / late / final / MTP).
- Load order (first hit wins): env `COLIBRI_EXPERTS_JSON` / `COLI_EXPERTS_JSON`,
  then cwd `experts.json` (same web shape as `web/public/experts.json`).
- Missing atlas is fine: tips still show depth roles (web parity).
- Display→source mapping uses the same strides as the sampler so tips name the
  expert that was actually sampled under stride mode.

Pure helpers: `atlas.rs` (parse, layer map, tip text) + `host::display_to_source`
/ `brain_view_from_map_with_max`. Lives under the SPA **Brain** tab (full-page).
Pixel / layout-density SPA shell parity is closed (`open:tauri-parity`); this is
not a 3-D galaxy or webview clone.
