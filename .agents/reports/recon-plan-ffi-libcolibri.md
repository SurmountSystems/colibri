# Recon: true in-process `libcolibri` FFI

**Date:** 2026-08-10
**Scope:** read-only inventory for planning true link-time embed of Colibrì engines into `colibri-sys`.
**Related:** `crates/colibri-sys/docs/ffi-phase-d.md`, `.agents/reports/explore-c-engine-ffi.md`, process path under `crates/colibri-sys/src/engine/`.

---

## 1. Current Phase D design and stubs

### Design doc (proposed only, not accepted architecture)

| Path | Role |
|------|------|
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/ffi-phase-d.md` | Phase D spike: required exports, kill-switch, acceptance criteria |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/user-guide.md` | States `ffi` is stub; process embed is product path |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/README.md` | Same honesty table |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-native/docs/fidelity.md` | Native GPUI: host in-process, **engine still C subprocess** |

Key Phase D claims (verified against tree):

- Host embeds **colibri-sys** in-process (probe, plan, doctor, duplex, install). Engine remains a **separate C process** on the serve mux.
- `ffi_available()` must stay `false` until a real library link exists.
- Kill-switch sketch: `COLIBRI_FORCE_PROCESS=1`, `ColibriConfig::prefer_process` (name TBD, **not implemented**), fallback on init failure.
- Acceptance: CI builds no-`main` lib (CPU Linux), golden generate on `c/glm_tiny`, kill-switch tests, thread/device ownership docs, operator OK on export set.

### Cargo feature and Rust stub

**`/home/hunter/Projects/surmount/colibri/crates/colibri-sys/Cargo.toml`**

```toml
# Reserved: bindgen / libcolibri when a no-main C library exists. Stub only.
ffi = []
```

No `build.rs`. No `links = "colibri"`. No bindgen / `cc` / link search.

**`/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/lib.rs`** (feature-gated module only):

```rust
#[cfg(feature = "ffi")]
pub mod ffi {
    pub fn ffi_available() -> bool { false }
}
```

Enabling `features = ["ffi"]` only compiles that stub. No `LibEngine`, no dlopen, no symbol table.

### Product process path (what ships today)

| Piece | Path | Behavior |
|-------|------|----------|
| Spawn + SERVE mux | `crates/colibri-sys/src/engine/serve.rs` | `Command::new(engine)`; forces `SERVE=1`, `SERVE_BATCH=1`, default `COLI_NO_OMP_TUNE=1` |
| High-level handle | `crates/colibri-sys/src/engine/mod.rs` | `EngineHandle::start_blocking` / `start_with_plan` → locate binary → `ServeClient::spawn` |
| rkyv bridge | `crates/colibri-sys/src/engine/duplex.rs` | `ClientFrame` ↔ mux lines (not REST, not FFI) |
| Locate | `crates/colibri-sys/src/engine/locate.rs` | `COLI_ENGINE`, libexec, in-tree `c/` |
| Visual | `crates/colibri-sys/src/visual.rs` + mux parsers | Parse `EMAP`/`HITS`/`TIERS`/`HWINFO`/`PROF` hex/text from stdout |
| Stop / cancel | `serve.rs` `stop_request` / `cancel_request` | Writes `STOP <id>` / `CANCEL <id>` on stdin while generate recv does not hold the handle mutex |

Wire contract: `/home/hunter/Projects/surmount/colibri/docs/serve_protocol.md` (mux = `run_serve_mux` when `SERVE_BATCH=1`).

---

## 2. C engine structure and shared-library feasibility

### Binaries, not libraries

**`/home/hunter/Projects/surmount/colibri/c/Makefile`**

- `all` → `colibri$(EXE)` only (phony `glm` same).
- Link: single TU `colibri.c` + optional GPU objects → **executable** (`colibri$(EXE): colibri.c ...` ~line 549).
- Sibling engines: `kimi_k3`, `inkling`, `olmoe`; V4 via `Makefile.deepseek-v4` → `deepseek_v4` binary.
- Install: bins under `libexec/colibri`, not `.a`/`.so` for inference.
- Only shared objects today: tool codecs `tools/libiq3.so`, `tools/librans_c.so`; Windows GPU `coli_cuda.dll` / `coli_hip.dll` (backend ABI, not app API).

**No** `libcolibri.a` / `libcolibri.so` / `COLIBRI_NO_MAIN` Make target exists. Phase D’s `COLIBRI_NO_MAIN=1` is **design prose only**.

### Where `main` and serve live

| Engine | Main | Serve entry | Notes |
|--------|------|-------------|--------|
| GLM / default | `c/colibri.c` `int main` ~8864 | After load, `SERVE` → `run_serve_mux` (`SERVE_BATCH`) or `run_serve` ~9418–9422 | OMP re-exec first thing in `main` (`execv` self unless `COLI_NO_OMP_TUNE` / `COLI_OMP_TUNED`) |
| Inkling | `c/inkling.c` ~2027 | `SERVE=1` branch ~2134 | Separate amalgamation |
| Kimi K3 | `c/kimi_k3.c` ~1749 | `serve_loop` ~1731 | Vulkan GPU path |
| DeepSeek V4 | `c/deepseek_v4.c` `main` under `#ifndef COLI_V4_SKIP_GENERATE_MAIN` ~8511 | `v4_serve_main` when `SERVE=1` | Serve calls `coli_v4_session_generate` internally |

**GLM mux** (`run_serve_mux` ~7265): READY sentinel, stdin poll (`select` / `PeekNamedPipe`), `SUBMIT`/`STOP`/`CANCEL`, continuous batch decode, telemetry via `hwinfo_emit` / `tiers_emit` / `emap_emit` (`c/telemetry.h`). Mid-turn stop uses process-global `g_mux_stop` / `g_mux_cancel` (~6073, polled in emit path #678).

### Shared library: feasible only after extract

| Blocker | Evidence |
|---------|----------|
| `main` is the product | Process lifecycle, signal handlers, mode dispatch in CLI mains |
| Process globals | Many `static` / `g_*` in `colibri.c` (CUDA/VK/Metal, PROF, mux stop, expert hit maps in `telemetry.h`) |
| OOM / config fail → `exit(1)` | Widespread in load/alloc paths (`colibri.c` falloc/OOM helpers) |
| OpenMP re-exec | `main` may `execv("/proc/self/exe", argv)` before any load |
| No host-facing GLM ABI | Control is env (`SNAP`, `SERVE`, …) + argv cap, not function exports |
| Headers are implementation | `st.h`, `tok.h`, `quant.h`, `telemetry.h` (header-static helpers), not versioned SDK |
| Multi-family | Four engines with different knobs (`docs/ENVIRONMENT.md`); one soname is non-trivial |

**GPU backends:** optional at build time (`CUDA=1`, `HIP=1`, `METAL=1`, `VK=1`). In-process link inherits full device/driver ownership and env matrix (`docs/cuda.md`, `metal.md`, `vulkan.md`, `GPU_BACKENDS.md`). Windows already uses a **GPU DLL** (`backend_loader.c` + `coli_cuda_*` exports); that is **not** a substitute for engine FFI.

### Existing experimental C API (DeepSeek V4 only)

**`/home/hunter/Projects/surmount/colibri/c/deepseek_v4.h`** (lines 72–156):

| Symbol | Role |
|--------|------|
| `coli_v4_config_parse` / `coli_v4_config_load` | Config |
| `coli_v4_prompt_build` | Prompt modes |
| `coli_v4_engine_open` / `coli_v4_engine_destroy` | Opaque engine; copies model dir; options struct |
| `coli_v4_engine_config` / `memory_summary` / `target_model_dir` | Accessors |
| `coli_v4_session_create` / `destroy` | Session borrows engine |
| `coli_v4_session_generate` | Prompt → token callback `ColiV4SessionTokenFn` (non-zero return = stop) |
| `coli_v4_session_generated_text` | Accumulated text buffer |

Documented as **experimental, may change**, V4-scoped. Implementations in `deepseek_v4.c` (`coli_v4_engine_open` ~6481, `coli_v4_session_generate` ~7705). Built only into the `deepseek_v4` binary via unit amalgamation (`Makefile.deepseek-v4` + `-DCOLI_V4_UNIT_*`).

`COLI_V4_SKIP_GENERATE_MAIN` can strip CLI `main` when compiling units, but **no Make target** builds a static/shared library today. `COLI_V4_SKIP_GENERATE_MAIN` is unused by production recipes.

**Shape reference for Phase D** (matches `ffi-phase-d.md` table: open / session / generate / destroy / token CB). Missing vs Phase D sketch: version symbol, cancel-by-id, visual poll, multi-family, multi-slot mux semantics.

### Key symbols for generate / stop / visual (today)

| Concern | Process (product) | In-process (desired / partial) |
|---------|-------------------|--------------------------------|
| Generate | Mux `SUBMIT` → `DATA` … `DONE` | V4: `coli_v4_session_generate`; GLM: **none** |
| Stop | `STOP <id>` → normal DONE + stats | V4: token CB return non-zero; GLM: `g_mux_stop` inside process only |
| Cancel | `CANCEL <id>` → `ERROR … CANCELLED` | No host C API |
| Visual | stdout `EMAP`/`HITS`/`TIERS`/`HWINFO`/`PROF` | Emit helpers in `telemetry.h` are **static inline in amalgamation**, not exported |
| Version | Python `c/version.py` | No C `coli_version` |

---

## 3. What true FFI would require

### Link and build

1. **C product target** (Make or equivalent):
   - Static archive first: e.g. `libcolibri.a` (CPU), later optional `.so` with explicit export map.
   - Compile without `main` (`#ifndef COLIBRI_NO_MAIN` around `colibri.c` main, or split CLI TU).
   - V4: build units with `-DCOLI_V4_SKIP_GENERATE_MAIN` into `libdeepseek_v4.a` (or unified multi-family lib).
   - Mirror existing flag matrix: OpenMP, arch, optional CUDA/Metal/Vulkan/HIP objects and link lines from `c/Makefile`.
2. **Public header** (new): e.g. `c/colibri_api.h` with stable names, error codes, opaque handles. Do **not** export whole internal headers.
3. **Rust `colibri-sys`**:
   - `build.rs` (or prebuilt artifact path) finds lib + include.
   - Feature `ffi` becomes real: `links`, `bindgen` or hand bindings, version check.
   - Keep process path default; `ffi_available()` true only when linked and init succeeds.

### ABI

- Prefer **error codes + caller-owned buffers** (V4 pattern: `char *error, size_t error_size`), not `longjmp` / process abort.
- Options structs (copy strings on open) so host env does not need hundreds of `getenv` sites for embed.
- Semver or git-describe via `coli_version()`.
- Document what is **not** ABI: GPU internal `coli_cuda_*`, quant internals, serve line protocol (process-only).

### Memory ownership

| Resource | Rule (proposed, matches V4 comments) |
|----------|--------------------------------------|
| Model / engine | `open` allocates; `destroy` only after all sessions free |
| Session | Borrows engine; host destroys sessions first |
| Prompt / options strings | Copied at open/generate or clearly “must live until return” |
| Token text | Callback delivers token id / optional UTF-8 slice owned by engine for the call only; host copies |
| Visual buffers | Host provides out-buffer + size, or allocates via documented free function |
| File maps / expert cache | Engine-owned until `model_close` |

Today GLM load paths call `exit(1)` on OOM and bad tensors. Embed requires **return error** paths for those sites on the public call tree (or a documented “process-kill is OK” non-goal, which is weak for desktop).

### Threading

| Today | Implication for FFI |
|-------|---------------------|
| Serve mux runs decode on engine main + OpenMP workers | Library needs explicit: “generate is not re-entrant per session”; multi-session may need internal mutex or single-owner thread |
| Rust host uses a stdout dispatcher thread + concurrent STOP writer | In-process stop must be **thread-safe cooperative flag** (like `g_mux_stop`) or dedicated cancel API, not stdin |
| OpenMP team process-global | Host must not nest conflicting OpenMP runtimes; document `omp_set_num_threads` ownership (`coli_omp_tune_threads` in `main` today) |
| GPU contexts | One process, one driver client; second engine instance may be illegal |

### Cancel

| Mode | Process | FFI target |
|------|---------|------------|
| Soft stop | `STOP` → DONE + keep KV/stats | `coli_cancel(session, req_id)` or token CB stop; same cleanup as natural end |
| Hard cancel | `CANCEL` → CANCELLED | Distinct API; drop in-flight without DONE |
| Process kill | `EngineHandle::stop` / drop child | Last resort; FFI cannot “kill” itself without aborting host |

V4 token callback already supports cooperative stop. Multi-request IDs (mux) need ID-scoped flags if multi-slot in-process is a goal.

### Re-exec and crash isolation

- Library builds must **never** `execv` self (set `COLI_NO_OMP_TUNE` equivalent in `coli_runtime_init`, or skip OMP re-exec entirely for embed).
- Subprocess death is recoverable; in-process SEGV / `exit` takes the host (desktop, GPUI). Phase D kill-switch **process fallback** remains mandatory for production hosts.

---

## 4. Minimal vs full FFI scope for production

### Minimal (first shippable “true FFI”)

Goal: prove in-process generate without rewriting all families.

| Include | Detail |
|---------|--------|
| One family | Prefer **V4** (API exists) **or** GLM-only after extracting open/generate from `colibri.c` for `c/glm_tiny` |
| Static link, CPU | Linux `lib*.a` + OpenMP; no CUDA/Metal day one |
| API surface | `version`, `engine_open/close`, `session_create/free`, `generate` + token CB, cooperative cancel via CB or `cancel` |
| Rust | `ffi` feature links static; `LibEngine` thin safe wrapper; golden test vs process path on tiny fixture |
| Kill-switch | `COLIBRI_FORCE_PROCESS=1` + init-fail → process embed |
| Non-goals of minimal | Multi-slot continuous batch, full visual poll parity, all families, GPU, grammar field, HF install changes |

Acceptance bar from Phase D still applies, scoped to that one family + CPU.

### Full (production-grade multi-family embed)

| Include | Detail |
|---------|--------|
| All product engines | `colibri`, `inkling`, `kimi_k3`, `deepseek_v4` under one or modular libs |
| Placement parity | Host plan env → session create options (tiers, pin, CTX, KV slots) |
| Generate parity | temp/top_p, max tokens, grammar/GBNF, multi-slot sticky KV |
| Stop / cancel | ID-scoped; concurrent with generate from another host thread |
| Visual | `coli_visual_poll(session, mask, out)` equivalent to EMAP/HITS/TIERS/HWINFO/PROF without text protocol |
| GPU feature matrix | Same as Make; careful driver lifecycle and re-init |
| Dynamic load optional | `.so` + soname for optional plugins; static still for hermetic desktop |
| Quality doctrine | Placement changes speed, not answers; golden logits/tokens match process |

Until full lands, **process + serve mux remains the production integration path** for GLM desktop and servers.

---

## 5. Risks, non-goals, suggested phases

### Risks

| Risk | Why it matters |
|------|----------------|
| Host crash on engine fault | No process isolation; desktop GPUI dies with C bug / OOM `exit` |
| Global state / re-entrancy | Single amalgamation not multi-instance safe |
| OpenMP + host runtime | Re-exec and wait-policy interactions already subtle in `main` |
| GPU driver ownership | Competing with other in-process ML libs |
| Binary size / link times | Full amalgamation + CUDA objects in Rust binary |
| ABI churn | V4 header already “may change”; premature bindgen freezes experimental surface |
| Dual paths drift | Process mux and FFI generate must stay golden-matched |
| Security | In-process maps attacker-influenced model files into host address space (same as process, but no OS process boundary) |

### Explicit non-goals (align with Phase D + fidelity doc)

- Rewrite decode kernels in Rust (`colibri-native` / sys host only).
- Bind every internal header.
- Claim “in-process engine” while `ffi_available()` is false.
- Desktop / Tauri / GPUI dependency on FFI in the same effort as host residual (fidelity: process embed is done path).
- Replace serve protocol for Python `openai_server.py` day one (process ABI stays).
- Architecture acceptance without operator OK (design spike status).

### Suggested phases (concrete paths)

| Phase | Work | Primary paths |
|-------|------|----------------|
| **P0 — honesty / no code claim** | Keep stub; docs already correct | `docs/ffi-phase-d.md`, `src/lib.rs` `ffi` stub, `fidelity.md` |
| **P1 — C extract spike (V4)** | Make target static lib without `main`; export list = `deepseek_v4.h` | `c/Makefile.deepseek-v4` (+ new `lib` rule), `c/deepseek_v4.c` (`COLI_V4_SKIP_GENERATE_MAIN`), `c/deepseek_v4.h` |
| **P1b — optional GLM extract** | Factor load + single-session generate out of `main`/`run_text`; `#ifndef COLIBRI_NO_MAIN`; replace critical `exit` with errors on public path | `c/colibri.c`, new `c/colibri_api.h`, `c/Makefile` |
| **P2 — Rust bind + golden** | `build.rs` + `src/ffi/` real module; `ffi_available()` reflects link+probe; test vs process on `c/glm_tiny` or `c/deepseek_v4_tiny` | `crates/colibri-sys/build.rs` (new), `src/lib.rs`, `tests/` |
| **P3 — kill-switch product** | `COLIBRI_FORCE_PROCESS`, config prefer_process, init fallback | `src/config.rs`, `src/engine/mod.rs` |
| **P4 — cancel + visual** | Cooperative cancel API; poll visual without stdout lines | C API + `src/visual.rs` dual path |
| **P5 — multi-family / GPU** | inkling/kimi/GLM unified or modular libs; optional CUDA/Metal/VK features | `c/Makefile*`, Cargo features mirror Make |
| **P6 — desktop opt-in** | `colibri-native` uses FFI only when available and not forced process | `crates/colibri-native/`, fidelity matrix update |

### Prior inventory

Deeper C build/link notes: `/home/hunter/Projects/surmount/colibri/.agents/reports/explore-c-engine-ffi.md` (still accurate on “executables only,” V4 API, serve protocol as de facto ABI).

---

## Bottom line

1. **Phase D is design-only.** Feature `ffi` is an empty Cargo flag + `ffi_available() → false`. No `build.rs`, no `libcolibri`.
2. **Product inference path is subprocess + serve mux** (`EngineHandle` / `ServeClient`), documented and implemented.
3. **True FFI is feasible but not free:** engines are CLI amalgamations with `main`, re-exec, globals, and `exit` on failure. Closest existing C embed shape is **experimental DeepSeek V4** (`coli_v4_*` in `deepseek_v4.h`); GLM has **no** host C API.
4. **Minimal production FFI:** one family, static CPU lib, open/session/generate/cancel, process kill-switch, golden parity. **Full:** multi-family, multi-slot, visual, GPU, concurrent cancel, no-exit error model.
5. **Until then:** do not document in-process decode; process embed is the correct product story for native desktop and servers.
