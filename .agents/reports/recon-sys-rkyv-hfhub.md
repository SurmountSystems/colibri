# Recon: colibri-sys public API, runtime shape, rkyv, hf-hub, GPUI notes

**Scope:** `/home/hunter/Projects/surmount/colibri/crates/colibri-sys` (read-only).
**Date:** 2026-08-10.
**No product edits.**

---

## 1. Public API map

### Features (`Cargo.toml`)

| Feature | Default | Role |
|---------|---------|------|
| `runtime` | on | Process spawn + serve mux (`EngineHandle`, `ServeClient`) |
| `stream` | on | rkyv frames + length-prefix codec |
| `tokio` | on (pulls `runtime`) | Async duplex session helpers |
| `install` | **off** | HF download (`hf` CLI or `hf-hub` 0.4) |
| `ffi` | **off** | Compile-only stub; no link |

### Always-on surface (`src/lib.rs` re-exports)

- **Config:** `ColibriConfig`, `EnvMap`, `Policy`
- **Doctor:** `DoctorCheck`, `DoctorOptions`, `DoctorReport`, `exit_code`, `run_doctor`
- **Error:** `Error`, `Result`
- **Model:** `ModelEntry`, `ModelFamily`, `ModelInfo`, `ModelRegistry`, `ModelStatus`, `model_arch`, `model_arch_from_type`
- **Paths / store:** model-store resolve helpers, `MODEL_STORE_ENV_KEYS`, …
- **Plan:** `PlacementPlan`, `PlanOptions`, `PlanPolicy`, `PlanTiers`, `environment_for_plan`
- **Probe:** `MachineInfo`, GPU/NPU/CPU types, disk helpers, SSD cache parse
- **Visual:** `VisualSnapshot`, `ExpertMap`, `ExpertHits`, `HwinfoSnap`, `TiersSnap`, `ProfileTurn`, `Subscribe`, `decode_hex_bytes`
- **Const:** `VERSION`

### `runtime` surface

| Type / fn | File | Role |
|-----------|------|------|
| `EngineHandle` | `src/engine/mod.rs` | High-level supervised process: `start_blocking`, `start_with_plan`, `generate`, visual getters, `stop` |
| `ServeClient` | `src/engine/serve.rs` | Line mux over child stdin/stdout; `spawn`, `from_pipes`, `generate`, telemetry |
| `GenerateRequest` / `GenerateResult` / `DoneStats` | `serve.rs` | SUBMIT payload and DONE STAT stats |
| `ServeEvent` | `serve.rs` | Internal channel events (Data/Accept/Done/Error) |
| `locate_engine` / `EngineLocate` | `src/engine/locate.rs` | Binary discovery (`COLI_ENGINE`, libexec, in-tree `c/`) |
| `READY_SENTINEL` | `serve.rs` | `\x01\x01READY\x01\x01` handshake bytes |
| `pack_expert_cell` / `unpack_expert_cell` | `serve.rs` | EMAP cell packing |

**`EngineHandle` lifecycle (honest):**

1. Resolve model family from `config.json`.
2. Locate C engine binary (`colibri` / `inkling` / `kimi_k3` / `deepseek_v4`).
3. Build env (`serve_env` or `serve_env_with_plan`).
4. `ServeClient::spawn`: `Command` with piped stdin/stdout, `SERVE=1`, `SERVE_BATCH=1`, optional `COLI_NO_OMP_TUNE=1`.
5. Wait for READY on stdout; background thread `colibri-serve-stdout` runs `dispatch_loop`.
6. `generate` writes `SUBMIT …\n` + prompt (+ grammar) to stdin; collects `DATA` until `DONE`.
7. Drop / `stop` → kill child.

Example: `examples/embed_chat.rs`. Tests: mock pipes in `serve.rs`; optional real engine in `tests/engine_real.rs`.

### `stream` surface (rkyv duplex)

| Item | File | Notes |
|------|------|--------|
| `ClientFrame` / `ServerFrame` | `src/stream/frame.rs` | rkyv 0.8 `Archive`/`Serialize`/`Deserialize` enums |
| `PROTOCOL_VERSION` | `frame.rs` | `u16 = 1` (Hello) |
| `encode_frame` / `decode_frame` / `decode_frame_checked` | `src/stream/codec.rs` | `u32le` length + rkyv body; trusted vs bytecheck |
| `write_frame` / `read_frame*` | `codec.rs` | Sync IO helpers |
| `DuplexSession` / `duplex_pair` | `src/stream/session.rs` | Tokio async over any `AsyncRead+AsyncWrite` (tests/custom transport) |

**Important:** rkyv frames are **not** the engine serve mux. Serve mux is text line protocol (`SUBMIT`/`DATA`/`DONE`/visual lines). User guide §8: mapping engine events into `ServerFrame` is **host glue** on top of `ServeClient` / `EngineHandle` snapshots. There is no code path where `EngineHandle::generate` encodes/decodes rkyv today.

#### `ClientFrame` variants

`Submit { req_id, slot, max_tokens, temperature, top_p, prompt }`, `Stop`, `Cancel`, `Subscribe { mask }`, `Ping`.

#### `ServerFrame` variants

`Hello`, `Hwinfo`, `Tiers`, `ExpertMap`, `ExpertHits`, `ProfTurn`, `Token`, `Accept`, `Done`, `Scheduler`, `Error`, `Pong`.

### `install` surface (`feature = "install"`)

Re-exported as `colibri_sys::model::install` (also `pub use model::install` at crate root).

| Item | Role |
|------|------|
| `InstallSource` | `HuggingFace { repo_id, revision, allow_patterns }` \| `LocalPath` |
| `InstallOptions` | dest, prefer_cli, min_free, inspect_after, register |
| `install_model` / `install_model_with` | Orchestration + injectable `HfCliRunner` |
| `SystemHfCli` | Real `hf download` process |
| `download_via_hf_hub` | **private** fn; only fallback when CLI missing/skipped |
| helpers | `ensure_space`, pattern filter, `detect_incomplete_download`, `materialize_snapshot`, `convert_subprocess` |

### `ffi` stub status

```rust
// src/lib.rs, feature "ffi"
pub mod ffi {
    pub fn ffi_available() -> bool { false }
}
```

- No `build.rs`, no bindgen, no `libcolibri` link search.
- Design spike only: `docs/ffi-phase-d.md`.
- Prior C inventory: `.agents/reports/explore-c-engine-ffi.md` (engines are executables with `main`; only experimental DeepSeek V4 C API in headers; product ABI is serve mux).

---

## 2. hf-hub 0.4 call sites → 1.x map

**Pin today:** `Cargo.toml` comment and dep:

```toml
# install (hf-hub 1.x is a full API rewrite; stay on 0.4 until install is ported)
hf-hub = { version = "0.4", optional = true }
```

**Only real crate usage:** `src/model/install.rs`, function `download_via_hf_hub` (lines ~413–473). Prefer path still uses system `hf` CLI via `SystemHfCli` / `HfCliRunner`; hub crate is the **else** branch when `prefer_cli && cli.available()` is false.

### Exact 0.4 symbols used

| Call | Purpose |
|------|---------|
| `hf_hub::api::sync::Api::new()` | Sync client |
| `hf_hub::Repo::with_revision(id, RepoType::Model, rev)` | Revision-bound repo |
| `api.repo(r)` | ApiRepo handle |
| `api.model(repo_id)` | Default-revision model repo |
| `repo.info()` | List siblings |
| `info.siblings` → `s.rfilename` | File names |
| `repo.get(name)` | Download one file to hub cache; returns `PathBuf` |
| `std::fs::copy(cached, dest.join(name))` | Materialize into install dest |

**Not used:** async 0.4 API, progress callbacks from hub, auth builders, snapshot helper as a single call (manual per-file get).

### What to change for 1.x (mapping only; implementer bumps)

Upstream 1.0 ([docs.rs/hf-hub](https://docs.rs/hf-hub/latest/hf_hub/), accessed 2026-08-10) rewrote the crate around **`HFClient`** (async default) and **`HFClientSync`** (`features = ["blocking"]`). Old `hf_hub::api::sync::Api` / `Repo` path is gone.

| 0.4 (current) | 1.x direction (sync install stays blocking) |
|---------------|-----------------------------------------------|
| `Api::new()` | `HFClientSync::new()` with `hf-hub = { version = "1", features = ["blocking"] }`, **or** `HFClient::new()` + async runtime if install is made async |
| `api.model(repo_id)` with full `"owner/name"` | `split_id(repo_id)` → `(owner, name)` then `client.model(owner, name)` |
| `Repo::with_revision(..., rev)` | Per-request revision on builders (`.revision(...)` style on info/download; confirm exact builder methods at port time) |
| `repo.info()` + `siblings[].rfilename` | `model.info().send()?` (shape differs) and/or `list_tree().recursive(true)` stream for file paths — **do not assume `siblings` field name survives** |
| `repo.get(name)` → cache path | `download_file().filename(name).send()?` (optional `.local_dir(dest)` to skip manual copy) |
| Manual `fs::copy` into `opts.dest` | Prefer 1.x `local_dir` on download **or** keep copy from cache if multi-pattern filter still needs pre-list + selective get |
| Allow patterns | Still host-side `filter_by_allow_patterns` **or** use hub globs if 1.x download APIs accept them; keep host filter if unsure |
| Error type | Map `HFError` → `Error::Install(...)` |

**Cargo / feature notes for implementer:**

1. Bump optional dep to `1` and almost certainly enable `blocking` so `install_model` stays sync (matches current API: no async install public surface).
2. `tokio` is already a default feature of colibri-sys; async `HFClient` is an alternative if install becomes async later.
3. Prefer-cli path (`hf download`) needs **no** hf-hub change.
4. Tests: mocked CLI path does not hit hub; only `live_hf_snapshot_tiny` (ignored) and real hub fallback need 1.x.

**Single function to rewrite:** `download_via_hf_hub` in `install.rs`. No other crate files import `hf_hub`.

---

## 3. Runtime honesty: process IPC, not in-process FFI

**For the operator claim “sys FFI not REST/RPC/IPC”:**

| Layer | What it is today |
|-------|------------------|
| **Inference embed** | **Process IPC**: spawn C engine, **stdin/stdout serve mux** (line protocol). Not REST. Not gRPC. Not in-process FFI. |
| **Product HTTP OpenAI** | Still **outside** this crate: `c/openai_server.py` / `coli serve` talk the **same serve mux** to the engine and expose HTTP to clients. |
| **rkyv duplex** | Optional **typed frame codec** for a future/custom host↔gateway duplex. Not wired as the engine control plane. |
| **`ffi` feature** | Stub; `ffi_available() == false`. No `libcolibri`. |

Docs already say this: crate README, `docs/user-guide.md`, `docs/ffi-phase-d.md`, `lib.rs` module docs (“Inference stays in **C engine subprocesses**”).

**Closest product “ABI” without linking C:** `docs/serve_protocol.md` + `ServeClient`.

---

## 4. True in-process embed vs proving rkyv over the existing duplex

### A. True in-process embed (real FFI)

Needs product C work first (not just Rust):

1. **No-`main` library target** for at least one family (proposed `COLIBRI_NO_MAIN` / `libcolibri` in ffi-phase-d).
2. **Stable C exports:** open/close model, session, generate (+ cancel), optional visual poll (sketch in `docs/ffi-phase-d.md`).
3. DeepSeek V4 experimental API in `c/deepseek_v4.h` is a **shape reference only** (still beside CLI main; not a shipping shared lib).
4. **Rust:** real `ffi` module, bindgen/`cc` link, version check; **`COLIBRI_FORCE_PROCESS=1` kill-switch** and fallback to process spawn.
5. Crash isolation: subprocess death is recoverable; in-process fault kills the host (desktop must keep process path).
6. Acceptance from ffi-phase-d: CI library build, golden generate vs process path, kill-switch tests, operator OK on export set.

**This is not “turn on feature ffi”.** Stub exists so the feature name is reserved.

### B. Prove rkyv path over the **existing** process duplex (no FFI)

Cheaper, host-only vertical:

1. Keep engine on **serve mux** (stdin/stdout).
2. Add a **bridge** that:
   - Accepts `ClientFrame` (encode/decode already done).
   - Translates `Submit`/`Stop`/`Cancel`/`Subscribe` → `ServeClient` SUBMIT/STOP + telemetry interest.
   - Emits `ServerFrame` from mux events: DATA→`Token`, DONE→`Done`, HWINFO/TIERS/EMAP/HITS/PROF→ matching frames, READY→`Hello` (fill model/engine/kv from config).
3. Transport options for proof:
   - **In-process:** `duplex_pair` + task that owns `EngineHandle`/`ServeClient` (unit/integration).
   - **OS pipe / Unix socket:** same codec on both ends; host process vs harness.
4. Use `decode_frame_checked` for any untrusted peer; trusted local can use `decode_frame`.
5. Gaps to close for a real product duplex:
   - Grammar field on Submit (serve has grammar; `ClientFrame::Submit` does not).
   - Scheduler frame source (may be host-side only).
   - Continuous visual pushes vs snapshot-after-generate (current `EngineHandle` refreshes visual mainly after `generate`).
   - Multi-slot mux when `kv_slots > 1` (protocol allows; host concurrency policy still thin).

**Bottom line:** rkyv can be **proven end-to-end without FFI** by wrapping the process mux. True “sys FFI” needs a C library product.

---

## 5. GPUI / Rust desktop notes (2026) — no edits

**In this repo:** no `gpui` dependency. Desktop shell is **Tauri 2**:

- `/home/hunter/Projects/surmount/colibri/desktop/`
- `desktop/src-tauri/Cargo.toml`: `tauri = "2.11.5"`, `tauri-build = "2.6.3"`
- Scaffold wraps existing web UI in a native window (`desktop/README.md`).

**GPUI (Zed Industries) — knowledge / approach notes only:**

| Topic | Note |
|-------|------|
| What | GPU-oriented UI framework powering the [Zed](https://zed.dev) editor (`zed-industries/gpui`). Immediate-mode-ish elements + retained entity model. |
| Depend | Historically git/path on Zed’s monorepo; crates.io `gpui` may exist depending on Zed packaging—pin carefully, expect fast breakage. |
| Stack | Metal (macOS), Vulkan/GL paths on other OS as Zed supports; not “HTML in a webview.” |
| App shape | `Application` / window, `Entity`/`Context`, actions, GPUI’s own executor—not a drop-in for Tauri commands. |
| Ecosystem | Community `gpui-component` kits exist; less “batteries” than egui/Tauri for forms + OS chrome. |
| When to pick | Pure-Rust, editor-like high-refresh UI, willing to track Zed’s API. |
| When not | Ship a thin shell around existing React/Svelte Brain UI → **Tauri (already here)** or wry; quick tools → egui/iced; multi-platform product with web team → keep Tauri. |
| colibri-sys fit | Host crate is backend (probe/plan/mux). Desktop would **call** `EngineHandle` from a Tauri command or a GPUI task the same way; neither path is in-process FFI today. Process isolation remains the safe default for a desktop host that loads multi-GB models. |

**Other pure-Rust desktop options (context, not recommendations):** egui (eframe), iced, Dioxus (native), Slint, freya—each has different maturity and packaging story than GPUI.

---

## 6. File index (absolute paths)

| Path | Role |
|------|------|
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/Cargo.toml` | Features + deps (rkyv 0.8, hf-hub 0.4 optional) |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/lib.rs` | Public re-exports + ffi stub |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/engine/mod.rs` | `EngineHandle` |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/engine/serve.rs` | Serve mux client (process IPC) |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/engine/locate.rs` | Binary locate |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/stream/{frame,codec,session,mod}.rs` | rkyv duplex |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/model/install.rs` | **Only** hf-hub call sites |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/ffi-phase-d.md` | In-process FFI design spike |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/user-guide.md` | Embed + stream + install how-to |
| `/home/hunter/Projects/surmount/colibri/docs/serve_protocol.md` | Product serve wire (repo root) |
| `/home/hunter/Projects/surmount/colibri/.agents/reports/explore-c-engine-ffi.md` | C build / no libcolibri inventory |
| `/home/hunter/Projects/surmount/colibri/desktop/` | Tauri 2 shell (not GPUI) |

---

## 7. One-line takeaways

1. **Public embed path = `EngineHandle` + process serve mux**, not REST and not FFI.
2. **rkyv is a parallel typed frame stack**; prove it by bridging to `ServeClient`, not by claiming engine already speaks rkyv.
3. **hf-hub 1.x port is isolated to `download_via_hf_hub`**; prefer-cli path unchanged; use `HFClientSync` + `blocking` for least API churn.
4. **True in-process embed is blocked on C `libcolibri`**, with kill-switch; stub only.
5. **Desktop in-tree is Tauri 2**; GPUI is a pure-Rust alternative with no current vendored refs.
