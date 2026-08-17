# Recon: Doctor Overall Fail on missing external engine vs FFI

**Repo:** `/home/hunter/Projects/surmount/colibri`
**Date:** 2026-08-11
**Mode:** read-only (no code edits)
**Screenshot match:** Overall Fail with hard fail
`[fail] external engine program not found at deepseek_v4; set COLIBRI_ENGINE or COLI_ENGINE, or build with make...`
Model path / weights / tokenizer pass; native “often prefers FFI.”

**Related prior work (already landed, still relevant):**

- `.agents/reports/recon-doctor-engine-built.md` (older wording “engine is not built”)
- `.agents/reports/impl-doctor-engine-readiness.md` (current wording + FFI-aware `engine.binary`)
- Residual closed: `feat:doctor-engine-readiness` in `.agents/RESIDUAL.md`

---

## Executive answer

| Question | Answer |
|----------|--------|
| Where is the engine binary check? | `colibri-sys` `doctor.rs`: `engine_binary_check` + `run_doctor` push of `engine.binary` |
| How is Overall computed? | **Any check with status `fail` → report.status `"error"` → UI Overall Fail** |
| Is FFI probed per family for doctor? | **No.** Global `ffi_available()` (or host override). Per-family exists only for start (`ffi_family_available` / `should_try_ffi_open`) |
| How does native wizard run doctor? | `run_shallow_doctor` / `run_deep_doctor` → `run_doctor_checks` → `run_doctor` |
| Smallest product fix? | **Logic for “FFI available → pass, not fail” is already in tree.** Screenshot is consistent with a **process-only** native binary (`feature = "ffi"` **off**, which is the **Cargo default**), or `COLIBRI_FORCE_PROCESS` killing FFI. Not with a missing doctor branch. |

---

## 1. Where doctor checks the engine binary

### Core types and options

**File:** `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/doctor.rs`

| Item | Lines (approx) | Role |
|------|----------------|------|
| `DoctorCheck` | 47–55 | One line: `id`, `status` (`pass`/`warn`/`fail`/`skip`), `summary`, optional `details` |
| `DoctorReport` | 58–68 | `status` overall (`ok`/`warning`/`error`), `checks[]`, `mode` |
| `DoctorOptions` | 72–91 | Includes `engine_path: Option<PathBuf>`, **`in_process_engine: Option<bool>`** |
| `in_process_engine` docs | 87–90 | `None` = detect from Cargo `ffi` + `COLIBRI_FORCE_PROCESS` (same idea as native prefer-FFI) |

### Decision helpers

| Function | Lines (approx) | Behavior |
|----------|----------------|----------|
| `doctor_in_process_available(opts)` | 359–372 | If `opts.in_process_engine` is `Some(v)` → that. Else: `#[cfg(feature = "ffi")]` → `crate::ffi::ffi_available()`; without `ffi` → **`false`** |
| `resolve_doctor_engine_path(model, opts)` | 374–393 | Explicit `opts.engine_path` → else family-aware `locate_engine` (when `feature = "runtime"`) → else **`PathBuf::from(family.engine_basename())`** |
| `external_engine_not_found_summary(path)` | 395–401 | Exact fail copy matching the screenshot (path + `COLIBRI_ENGINE` / `COLI_ENGINE` / `make -C c <engine>`) |
| **`engine_binary_check(engine, in_process_available)`** | 403–455 | Pure decision for check id **`engine.binary`** |

### `engine_binary_check` matrix (current code)

| Condition | status | summary |
|-----------|--------|---------|
| Executable file, shared libs OK | `pass` | `"engine executable is ready"` |
| Executable, missing shared libs | `fail` | `"engine cannot load: …"` |
| File exists, not executable | `fail` | `"engine exists but is not executable"` |
| Missing + **`in_process_available`** | **`pass`** | **`"in-process engine is available"`** (details `mode: "ffi"`) |
| Missing + no in-process | **`fail`** | **`external_engine_not_found_summary`** → screenshot text |

### Call site inside `run_doctor`

**Function:** `pub fn run_doctor(model, opts)` — ~1040–1307

```text
// ~1138–1143
let engine = resolve_doctor_engine_path(model, opts);
checks.push(engine_binary_check(
    &engine,
    doctor_in_process_available(opts),
));
```

### Why the path reads as bare `deepseek_v4`

1. Model family from `config.json` via `model_arch` → `ModelFamily::DeepseekV4`
   (`crates/colibri-sys/src/model/mod.rs`, `engine_basename` → `"deepseek_v4"`).
2. Host / doctor try `locate_engine` for that family; no process binary found.
3. Fallback path is the basename only: **`deepseek_v4`** (not a full searched list).
   That is exactly the path in the fail summary.

Family basename table (`ModelFamily::engine_basename`):

| Family | Basename |
|--------|----------|
| Glm / Olmoe | `colibri` |
| Inkling | `inkling` |
| Kimi | `kimi_k3` |
| DeepseekV4 | **`deepseek_v4`** |

---

## 2. How Overall pass/fail is computed

### Sys report status

**Function:** end of `run_doctor` (~1290–1298)

```text
let statuses: HashSet<&str> = checks.iter().map(|c| c.status.as_str()).collect();
let status = if statuses.contains("fail") {
    "error"
} else if statuses.contains("warn") {
    "warning"
} else {
    "ok"
};
```

**Rule:** **any single `fail` → overall `"error"`**. Warns alone → `"warning"`. No fails and no warns → `"ok"`. Skips do not affect overall.

**Exit helper:** `exit_code(report)` (~1309–1311): `1` if `status == "error"`, else `0`.

### Native UI label

**File:** `crates/colibri-native/src/host.rs`

| Function | Lines (approx) | Mapping |
|----------|----------------|---------|
| `doctor_overall_label` | 228–238 | `"ok"` → **Pass**, `"warning"` → **Warning**, `"error"` → **Fail** |
| `doctor_check_mark` | 241–254 | check status → `[pass]` / `[warn]` / `[fail]` / `[skip]` |
| `format_doctor_checklist` | 434–458 | `Overall: {label}\nModel: …\nDepth: …\n` then `[{mark}] {summary}` per check |

So: **one hard `engine.binary` fail alone forces Overall Fail**, even if every model/storage check passes. That matches the screenshot.

---

## 3. FFI availability: global vs per family

### Doctor (no per-family probe)

`doctor_in_process_available` only:

- host/test override `DoctorOptions.in_process_engine`, or
- **`ffi_available()`** when built with `feature = "ffi"`.

It does **not** call `ffi_family_available(FfiFamily::DeepseekV4)` and does **not** open the engine.

### Sys FFI surface (when `feature = "ffi"`)

**File:** `crates/colibri-sys/src/ffi/mod.rs`

| Function | Meaning |
|----------|---------|
| `ffi_link_available()` | Always `true` under feature (static engines linked) |
| `ffi_available()` | Link + **not** `COLIBRI_FORCE_PROCESS` |
| `ffi_family_available(family)` | `ffi_available()` && family in `linked_families()` |
| `linked_families()` | **All four:** Glm, Kimi, Inkling, **DeepseekV4** |

**File:** `crates/colibri-sys/src/ffi/multi.rs` — `FfiFamily::from_model_family`:

| `ModelFamily` | `FfiFamily` |
|---------------|-------------|
| Glm, Olmoe | Glm |
| Kimi | Kimi |
| Inkling | Inkling |
| DeepseekV4 | DeepseekV4 |

Under the current link matrix, **DeepSeek V4 is always included when `ffi` is built**, so global vs per-family would not diverge for V4 today. Gap is only if a future build drops a family from `linked_families` while doctor still uses global `ffi_available()`.

### Module gated

`lib.rs`: `#[cfg(feature = "ffi")] pub mod ffi;`
Without the feature, doctor cannot see FFI at all (`doctor_in_process_available` → false).

---

## 4. How native wizard / Tools run doctor (options: `in_process`?)

### Call graph

```text
UI (wizard step readiness / Tools / bootstrap)
  DesktopApp::run_doctor_with_recovery(deep)     main.rs ~785+
  bootstrap_panels_with_machine                  main.rs ~4493–4514
      │
      ├─ run_shallow_doctor(path, machine)       host.rs ~563–564
      └─ run_deep_doctor(path, machine)          host.rs ~568–569
            │
            └─ run_doctor_checks(model, machine, deep)   host.rs ~494–559
                  │
                  ├─ idle / create dir / not-a-model early returns
                  ├─ build DoctorOptions { deep, RAM/disk/GPUs from machine, … }
                  ├─ engine_path: COLI_ENGINE/COLIBRI_ENGINE or locate_engine(family)
                  ├─ #[cfg(feature = "ffi")] if !resolve_prefer_process() {
                  │       opts.in_process_engine = Some(true);
                  │   }
                  └─ run_doctor(&model, &opts) → format_doctor_checklist
```

### `run_doctor_checks` option detail (`host.rs` ~523–555)

1. `DoctorOptions { deep, ..Default::default() }`
2. Machine inject: `available_memory`, `available_disk`, `ram_gb`, `gpus`
3. Engine path:
   - `env_engine_path()` (`COLI_ENGINE` / `COLIBRI_ENGINE`), else
   - `locate_engine(EngineLocate { family: model_arch(&model), … })` on success
4. **FFI hint to doctor (only when native built with `ffi`):**
   - If `!resolve_prefer_process()` → **`in_process_engine = Some(true)`**
   - If process is preferred (e.g. force-process) → leave `None` (doctor then uses `ffi_available()`, which is false under force-process)

### Prefer-process resolution (native)

**File:** `host.rs` ~1613–1675

| Function | Behavior |
|----------|----------|
| `resolve_prefer_process_from_flags(force, prefer_ffi)` | Force → process; with **`feature = "ffi"`** → prefer_process **false** (FFI-first); without ffi → process unless prefer_ffi env |
| `resolve_prefer_process()` | Env: `COLIBRI_FORCE_PROCESS`, `COLIBRI_PREFER_FFI` |
| `should_try_ffi_open(cfg, family)` | Start path only: config prefers FFI + **`ffi_family_available`** for that family |

Wizard and Tools **do not** pass a separate doctor flag; they only call shallow/deep wrappers. **No `DoctorOptions` is built in `main.rs`.**

### Cargo: native does **not** default-enable FFI

**File:** `crates/colibri-native/Cargo.toml`

```toml
[features]
default = ["install"]
ffi = ["colibri-sys/ffi"]
```

**Default `cargo run -p colibri-native` / default binary = process-only.**
Docs (`fidelity.md`, `ffi-phase-d.md`, README): FFI-first only when built with **`feature = "ffi"`**.

That is the strongest explanation of the screenshot on a default-built desktop: model checks pass; process binary `deepseek_v4` missing; `in_process_engine` never true; Overall Fail.

---

## 5. Smallest fix vs what is already implemented

### Already implemented (do not re-implement blindly)

From `impl-doctor-engine-readiness.md` / current `doctor.rs`:

- Missing process + in-process available → **`pass`** (“in-process engine is available”)
- Missing process + no in-process → **fail** with **external engine program not found** (not “not built”)
- Unit tests:
  - `engine_missing_without_ffi_says_external_not_not_built`
  - `engine_missing_with_ffi_passes_in_process`
  - `doctor_engine_missing_no_ffi_message_contract`
  - `doctor_engine_missing_with_in_process_passes`
  - `doctor_ffi_feature_defaults_to_in_process_when_binary_missing` (`#[cfg(feature = "ffi")]`)

When `feature = "ffi"` is on and force-process is off, **Overall should not Fail solely on missing external binary.**

### Root cause classes for the operator screenshot

| # | Situation | `engine.binary` | Overall |
|---|-----------|-----------------|---------|
| A | **Native built without `ffi` (default)** | fail external not found at `deepseek_v4` | **Fail** |
| B | Built with `ffi` + `COLIBRI_FORCE_PROCESS=1` | fail (process required, binary missing) | Fail |
| C | Built with `ffi`, no force-process | **pass** in-process | Not Fail from engine alone |
| D | Process binary present and loadable | pass executable ready | Not Fail from engine alone |

Screenshot text and path **`deepseek_v4`** match **A** (or B) after the readiness wording ship.

### Smallest *product* fixes (if intent is “native often prefers FFI ⇒ doctor should not fail”)

Pick by product intent (recon only; not implementing):

1. **Enable FFI for the product desktop default (most aligned with “often prefers FFI”)**
   - e.g. `colibri-native` `default = ["install", "ffi"]`, or document/require `cargo run -p colibri-native --features ffi` for the operator binary.
   - With that, existing `doctor_in_process_available` + `engine_binary_check` already pass without process binary.

2. **Tighten doctor to family (optional, small, only needed if link matrix splits)**
   - In `run_doctor` / `doctor_in_process_available`, map `model_arch(model)` → `FfiFamily::from_model_family` → `ffi_family_available`.
   - Today all four families are linked together; little behavior change for V4.

3. **Host: always set `in_process_engine` from the same predicate as start**
   - e.g. `opts.in_process_engine = Some(should_try_ffi_open(&cfg, family))` or `Some(ffi_family_available(...))` when feature=ffi.
   - Today host only sets `Some(true)` when `!resolve_prefer_process()`; when force-process, leaving `None` + `ffi_available()==false` is already correct.
   - **Does not help default no-ffi builds.**

4. **Warn instead of fail when process missing but product is FFI-capable**
   - Only useful if product wants Overall Pass/Warning while still nagging for optional process binary. Current contract is **pass** when FFI available, **fail** when not.

5. **Do not weaken Overall aggregation**
   - Changing “any fail → error” would hide real model/RAM fails. Prefer fixing `engine.binary` status (already done for FFI-on).

### What not to do

- Re-introduce “engine is not built” as the desktop fail copy (already scrubbed).
- Assume doctor “ignores FFI” in current tree: it does **not**, when `feature = "ffi"` is compiled in.
- Claim per-family doctor probe exists: it does **not**; start path has it, doctor does not.

---

## 6. Path index (absolute)

| Path | Symbols |
|------|---------|
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/doctor.rs` | `DoctorOptions`, `doctor_in_process_available`, `resolve_doctor_engine_path`, `engine_binary_check`, `run_doctor`, overall status fold, unit tests ~1813–1928 |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/ffi/mod.rs` | `ffi_available`, `ffi_family_available`, `linked_families` |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/ffi/multi.rs` | `FfiFamily`, `from_model_family`, `open_engine` |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/model/mod.rs` | `ModelFamily::engine_basename`, `model_arch` |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/engine/locate.rs` | `locate_engine`, locate error “not built or not on search path” (spawn path; not doctor UI summary) |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/lib.rs` | `#[cfg(feature = "ffi")] pub mod ffi`, re-exports `run_doctor` |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/host.rs` | `run_doctor_checks`, `run_shallow_doctor`, `run_deep_doctor`, `format_doctor_checklist`, `doctor_overall_label`, `resolve_prefer_process*`, `should_try_ffi_open` |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/main.rs` | `run_doctor_with_recovery`, wizard readiness, `bootstrap_panels_with_machine` → `run_shallow_doctor` |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-native/Cargo.toml` | **default features omit `ffi`** |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/Cargo.toml` | `ffi` / `ffi-cuda` optional |

---

## 7. Recommended verify (for an implementer later)

1. Confirm how the operator binary was built:
   `cargo build -p colibri-native` vs `cargo build -p colibri-native --features ffi`
   (and whether `COLIBRI_FORCE_PROCESS` is set in the shell).
2. Red/green already exist under `colibri-sys` doctor tests for both branches; with `--features ffi`, missing process must not force `engine.binary` fail when force-process is off.
3. If product decides native default includes FFI: add a host-level test that `run_doctor_checks` with a real model leaf and no process binary does **not** emit `external engine program not found` under `feature = "ffi"`.

---

## Bottom line

- **Overall Fail rule:** any check `fail` → report `error` → UI **Overall: Fail**.
- **Screenshot fail** is `engine.binary` from `engine_binary_check` after `resolve_doctor_engine_path` fell back to basename **`deepseek_v4`**.
- **Intended FFI pass path exists** (`in_process_available` → pass). It is **compile-gated** on Cargo **`ffi`**, which **colibri-native does not enable by default**.
- **Smallest real fix for “native often prefers FFI” doctor UX** is almost certainly **ship/run native with `feature = "ffi"`** (or make it default), not rewrite overall aggregation. Family-scoped doctor probe is a small optional tighten on top of that.
