# Recon: doctor "engine is not built" and readiness UX

**Scope:** read-only. Workspace `/home/hunter/Projects/surmount/colibri`.
**Date:** 2026-08-11.

## 1. Where `[fail] engine is not built` comes from

### Source of the summary string

| Layer | File | Behavior |
|-------|------|----------|
| **colibri-sys doctor** | `crates/colibri-sys/src/doctor.rs` ~1036–1073 | Check id `engine.binary`, status `fail`, summary **`engine is not built`** when path is not a file (and not the “exists but not executable” branch). |
| **Python doctor** | `c/doctor.py` ~477–499 | Same id/summary (port origin). |
| **Native UI** | `crates/colibri-native/src/host.rs` ~233–258 | `format_doctor_checklist` renders each check as **`[{status}] {summary}`** (not the id). So the wizard/Tools panel shows **`[fail] engine is not built`**. |

Overall status becomes **`error`** if any check is `fail` (`doctor.rs` ~1221–1227), so host label is **Overall: Fail**.

Related (process locate, not doctor UI):

- `crates/colibri-sys/src/engine/locate.rs` ~90–92: `"{name} engine is not built or not on search path; set COLI_ENGINE or build with \`make -C c {name}\`"` on spawn/locate failure.
- `c/coli` ~260–264: hard exit `"{target} engine is not built. Run: make -C c {target}"` before chat/serve.

## 2. When doctor fails: binary path only (not FFI / lib)

### What `engine.binary` actually tests

In `run_doctor` (`doctor.rs` ~1036–1073):

1. `engine = opts.engine_path.clone().unwrap_or_else(|| PathBuf::from("colibri"))`
2. If `is_executable(&engine)` → ldd missing libs → fail with load message, else **pass** `"engine executable is ready"`.
3. Else if file but not executable → fail `"engine exists but is not executable"`.
4. Else → **fail `"engine is not built"`** with details `{ "path": ... }`.

`is_executable` (`doctor.rs` ~338–348): path **must be a file** with Unix execute bits. A bare name `"colibri"` only passes if **cwd**/`colibri` (or that relative path) exists as an executable file. **No** `locate_engine` search. **No** check for `libcolibri.a`, Cargo `feature = "ffi"`, or `ffi_link_available()`.

### Native host wiring

`run_doctor_checks` (`host.rs` ~267–286):

- Empty/whitespace model path → **idle checklist** (no sys doctor).
- Non-empty path → `run_doctor` with machine RAM/disk when available.
- Engine override **only** if `COLI_ENGINE` / `COLIBRI_ENGINE` env is set (`env_engine_path`, ~52–56).
- Otherwise doctor keeps default **`PathBuf::from("colibri")`**.

So a typical desktop run with no env and no `./colibri` in CWD always fails `engine.binary`, even when:

- static libs are built (`c/libcolibri.a`, etc.),
- `colibri-sys` was built with `feature = "ffi"`,
- in-process open would work.

### Operator misconception (accurate)

**Sys crate / FFI “engine is built” ≠ process binary on disk.**

| Artifact | Role | Doctor `engine.binary` |
|----------|------|-------------------------|
| `libcolibri.a` / `libkimi_k3.a` / … | Static embed for Cargo `ffi` | **Ignored** |
| Linked FFI (`ffi_link_available`) | In-process inference | **Ignored** |
| Process binary `colibri` / `kimi_k3` / … | Subprocess serve mux | **Only thing checked** |
| `locate_engine` search roots | Used at **session start** (process path) | **Not used by doctor** |

Python CLI is better at **family**: `c/coli` `cmd_doctor` passes `engine_path=engine_for(a.model)` (~757–762). Rust doctor default is always the basename **`colibri`**, not family-aware, unless the host sets `DoctorOptions.engine_path`.

## 3. What native actually uses for inference (ffi vs process)

### Product default (library vs native)

| Context | Default |
|---------|---------|
| `ColibriConfig` (sys library) | `prefer_process = true` → process path (`config.rs` ~370–394) |
| **colibri-native** with Cargo `feature = "ffi"` | `resolve_prefer_process_from_flags` → `prefer_process = false` (FFI-first) (`host.rs` ~1154–1187) |
| native **without** `feature = "ffi"` | process unless `COLIBRI_PREFER_FFI` (still cannot link FFI) |
| Kill-switch | `COLIBRI_FORCE_PROCESS` always wins (process) |

Native `ffi` feature: `crates/colibri-native/Cargo.toml` ~27–32 (`ffi = ["colibri-sys/ffi"]`). **Default features are only `install`**, not `ffi`. So default `cargo run -p colibri-native` is **process-only** unless built with `--features ffi`.

### Start path (`EngineSession::start`, `host.rs` ~1278–1406)

1. Require model **directory**.
2. Build `ColibriConfig` with `prefer_process(resolve_prefer_process())`, optional engine from env.
3. `should_try_ffi_open` (`host.rs` ~1190–1209): config prefers FFI **and** (with `feature = "ffi"`) `FfiFamily::from_model_family` + `ffi_family_available`.
4. On FFI: `coli_ffi::open_engine(family, model)` → `LiveEngine::Ffi` (no process binary).
5. On FFI fail: note + fall through to **process**.
6. Process: `EngineHandle::start_with_plan` / `start_blocking` → **`locate_engine`** if no `config.engine` (`engine/mod.rs` ~83–91).

`locate_engine` order (`locate.rs` ~51–92): override → search roots → cwd/`c/<name>`, exe dir, `/usr/local/libexec/colibri/…`, etc. Failure message: “not built or not on search path…”.

**Doctor does not share this path.** Ready for chat with FFI can still show doctor Fail on engine.binary.

## 4. Wizard readiness: check IDs, messages, model path

### When readiness runs

- Wizard step **Readiness** shows `doctor_text` + plan (`main.rs` ~3050–3094).
- Entering readiness from Model step: `run_shallow_doctor` + `run_plan` (`main.rs` ~483–492).
- Refresh button: same (`main.rs` ~3065–3067).
- Bootstrap on app start also runs shallow doctor for current path (`bootstrap_panels` ~3760–3762).

Formatting: summary only (see §1). Status marks: pass / warn / fail / skip.

### Idle vs fail spam (model path)

| Input | Behavior |
|-------|----------|
| Empty / whitespace model path | Host **`format_idle_doctor_checklist`**: `Overall: Idle`, no sys doctor (`host.rs` ~216–230, 267–269). Tests assert no `[fail]`. |
| Non-empty path (including bad `~/.models`) | Full `run_doctor` → real fails (path missing, engine, etc.). |
| Deliberate `.` | **Not** idle; real doctor on cwd. |

**No tilde expansion** anywhere under `crates/` (no `expand_tilde` / `shellexpand`). `PathBuf::from("~/.models")` is a **literal** path under cwd-ish semantics: almost always **`model.path` fail** “model directory does not exist”.

### Default model **store** (not the model path field)

`default_model_store_path` (`paths.rs` ~28–63):

1. `COLIBRI_MODEL_STORE` / `COLI_MODEL_STORE`
2. Else `$XDG_DATA_HOME/colibri/models` or **`~/.local/share/colibri/models`** (not `~/.models`)

Model path seed (`main.rs` ~194–204): `COLIBRI_MODEL` / `COLI_MODEL`, else prefs `last_model_path`, else **empty** (idle doctor). Store path is for scan/install free-space, not auto-filled into the model field.

### Standard-mode check inventory (wizard shallow doctor)

Order and typical summaries from `run_doctor` (+ plan branch). Deep mode adds more (below).

| id | Typical pass / fail / warn / skip summaries |
|----|-----------------------------------------------|
| `model.path` | pass: “model directory is readable”; fail: “does not exist” / “not readable” |
| `model.config` | pass: “config.json is valid”; fail: “missing or invalid” |
| `model.tokenizer` | pass: “tokenizer.json found”; fail: “missing” |
| `storage.persistence` | pass: writable dir; warn: read-only; skip: not a dir |
| `engine.binary` | pass: “engine executable is ready”; fail: **“engine is not built”** / not executable / cannot load shared libs |
| `accelerator.cuda` | skip: no GPU / GPU disabled; pass/warn: CUDA/HIP linked; warn: GPU but CPU-only engine; fail: missing runtime / requested GPU missing |
| `model.shards` | pass: “safetensors headers are valid”; fail: plan build error string |
| `storage.disk` | pass/warn free space; skip if plan failed |
| `memory.ram` | pass / fail budget; skip if plan failed |
| `placement.plan` | pass no warnings; warn with plan warnings; skip if plan failed |
| `storage.ssd_probe` | pass with GB/s; skip pending / no probe |

**Deep only** (`opts.deep` / Tools “Deep check”): `model.container`, `model.shard_sequence`, `model.required`, `model.index`, `storage.mirror` (`push_deep_checks` / `push_deep_fail` ~853–931).

Native does **not** show check ids in the checklist (only summaries). Python `format_doctor` shows id + summary (`c/doctor.py` ~656–665).

## 5. Recommended product fix (plain English)

### Root problem

Doctor treats “engine ready” as **“process binary file exists at this path”**. Product reality (especially with FFI): **engine ready** can mean **in-process static engines linked**, with process binary only as fallback / force-process.

### Fix order (concrete)

1. **Doctor: readiness for engine mode, not “not built” for missing process file**
   - In `colibri-sys` `run_doctor` (or host pre-fill of `DoctorOptions` + new check id if needed):
     - If Cargo `ffi` linked and family available (and not force-process): **pass** (or dedicated) message like **“in-process engine (FFI) is available”**; do **not** fail the overall report solely because `colibri` is missing from CWD.
     - If process-only (or force-process / fallback required): resolve path via **`locate_engine` + model family** (match `engine_for` / spawn), then check that path.
     - Fail copy when process path needed and missing: **“external engine binary not found”** (say where searched / `COLI_ENGINE` / `make -C c <target>`). Reserve **“not built”** only when you mean “rebuild required,” or drop that phrase for install/desktop UX.
   - Optional second line: process binary missing but FFI ok → **warn** “process fallback binary not found,” overall still ok if FFI is the product path.

2. **Native host: feed doctor the same engine resolution as start**
   - Before `run_doctor`, set `engine_path` from `env_engine_path` **or** `locate_engine(EngineLocate { family: model_arch(model), … })` when model dir exists.
   - Pass `gpus` / linkage from probe when useful so accelerator check does not depend only on a missing binary’s ldd.

3. **Default `colibri-native` + docs alignment**
   - If desktop is intended FFI-first, consider making `ffi` a **default feature** (or document that readiness Fail without `./colibri` is expected until process binary or `--features ffi`). Today default bin is process-only, so doctor Fail is “correct” for process path, but the **message** is still wrong-sounding.

4. **Model path UX (empty / tilde)**
   - Keep empty path as **Idle** (already good).
   - Expand `~` / `$HOME` when reading the model field and prefs, or reject with a clear **“~ is not expanded; use an absolute path”** instead of fake `model.path` fail.
   - Do not equate user mental model **`~/.models`** with product store **`~/.local/share/colibri/models`**; if UI copy mentions a store, use the real default.

5. **Tests (TDD)**
   - Sys: doctor with no process binary but `feature = "ffi"` → not overall `error` solely from `engine.binary` (contract as chosen).
   - Sys/native: missing process + process-required → message **“external engine binary not found”** (or agreed wording), not “not built.”
   - Native: `~` path expands or soft-fails clearly; empty path stays Idle without `[fail]`.

### Non-goals / keep

- Kill-switch `COLIBRI_FORCE_PROCESS` still requires a real process binary.
- Deep tensor checks unchanged.
- Library embeds may keep process-default `ColibriConfig`; only doctor + desktop host need mode-aware wording.

## 6. File:line cite index

| Topic | Location |
|-------|----------|
| Fail summary | `crates/colibri-sys/src/doctor.rs:1068-1073` |
| Default engine name | `crates/colibri-sys/src/doctor.rs:1036-1040` |
| Checklist format | `crates/colibri-native/src/host.rs:233-258` |
| Idle empty path | `crates/colibri-native/src/host.rs:216-230, 267-269` |
| Doctor env engine only | `crates/colibri-native/src/host.rs:52-56, 280-282` |
| prefer_ffi / start | `crates/colibri-native/src/host.rs:1154-1346` |
| locate_engine | `crates/colibri-sys/src/engine/locate.rs:51-92` |
| Process always spawn | `crates/colibri-sys/src/engine/mod.rs:68-94` |
| FFI availability | `crates/colibri-sys/src/ffi/mod.rs:49-75` |
| Model store default | `crates/colibri-sys/src/paths.rs:28-63` |
| Wizard readiness UI | `crates/colibri-native/src/main.rs:483-492, 3050-3094` |
| Python family engine for doctor | `c/coli:757-762` (`engine_for`) |
| Python same fail string | `c/doctor.py:499` |

## 7. One-line diagnosis

**Doctor fails “engine is not built” when a process executable is missing at a weak default path (`colibri` in CWD / env override), while “sys/FFI built” means static libs and optional in-process inference; readiness UX currently conflates those two engines.**
