# Recon: model registry / supported models for UI picker

**Date:** 2026-08-11
**Scope:** `colibri-sys` model APIs + native wizard Model step + SPA/desktop
**Goal:** catalog picker for all Colibri-supported models (not freeform HF only)

---

## Executive summary

| Layer | What it is today | Catalog of supported HF models? |
|-------|------------------|----------------------------------|
| `ModelRegistry` | **Local disk inventory** of installed model dirs | **No** |
| `install` feature | Generic HF snapshot download by `repo_id` | **No** curated list |
| Product docs (`README.md`) | Human table of families + HF URLs + size | **Yes (docs only)** |
| Native wizard Model step | Path field + **scan local store** + optional freeform install form | No remote catalog |
| SPA `web/` | OpenAI `/v1/models` ids after connect | No install; not HF catalog |
| Tauri `desktop/` | Shell only; no model picker | No |

**Gap:** there is **no** `list_supported_models()` / catalog type in `colibri-sys`. Hosts must invent a catalog (or parse docs) then call existing install + registry APIs.

---

## 1. `colibri-sys` model API surface

### 1.1 Modules / re-exports

| Path | Role |
|------|------|
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/model/mod.rs` | Family routing, `ModelInfo` inspect, `ModelSizeInfo` |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/model/registry.rs` | Local root scan inventory |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/model/install.rs` | Feature `install` (default **off** in sys; **on** in native) |
| Crate root `lib.rs` | Re-exports model types; `pub use model::install` when `install` |

**Public re-exports (always):**

```text
ModelEntry, ModelFamily, ModelInfo, ModelRegistry, ModelSizeInfo, ModelStatus
model_arch, model_arch_from_type, param_count_from_config
```

**Public when `features = ["install"]`:** entire `colibri_sys::model::install` (also `colibri_sys::install` via crate root).

### 1.2 Families (`ModelFamily`)

| Variant | `as_str()` | Engine basename | Notes |
|---------|------------|-----------------|-------|
| `Glm` | `glm` | `colibri` | Default |
| `Inkling` | `inkling` | `inkling` | |
| `Kimi` | `kimi` | `kimi_k3` | |
| `DeepseekV4` | `deepseek_v4` | `deepseek_v4` | |
| `Olmoe` | `olmoe` | `colibri` | Research; coli falls through to GLM binary |

Routing: `model_arch_from_type(&str)` / `model_arch(&Path)` from `config.json` `model_type` substrings (`inkling`, `kimi`, `deepseek`+`v4`, `olmoe`, else GLM).

### 1.3 Local registry (not a remote catalog)

```text
ModelStatus: Present | Incomplete | MissingTokenizer | MissingConfig | Unreadable

ModelEntry {
  path, family, engine_id, status,
  model_bytes, disk_bytes, param_count, shards, model_type, note
}

ModelRegistry::open(roots)
  .add_root(path)
  .register(path) -> Result<&ModelEntry>   // inspect one path
  .refresh() -> Result<()>                 // rescan roots
  .roots() / .entries() / .find(path)

REGISTRY_SCAN_MAX_DEPTH = 2   // store/m or store/owner/name
REGISTRY_SCAN_MAX_ENTRIES = 64
```

Scan rule: dirs with `config.json` are model leaves. Classify via `ModelInfo::inspect` when possible.

**No HF repo ids. No size hints for uninstalled models. No “supported product list.”**

### 1.4 Inspect / size

```text
ModelInfo::inspect(path) -> Result<ModelInfo>
  family, engine_id, model_type, shards,
  model_bytes / disk_bytes, param_count,
  dense/expert geometry, has_config, has_tokenizer, is_complete()
  size_info() -> ModelSizeInfo
  disk_gib()

ModelSizeInfo { path, family, engine_id, disk_bytes, …, optional plan tier fields }
  with_plan_tiers(&PlacementPlan)
```

Size after install/on disk only. Pre-download disk estimate is **not** in the API (must come from a catalog constant).

### 1.5 Install (feature `install`)

```text
InstallSource::HuggingFace { repo_id, revision, allow_patterns }
InstallSource::LocalPath { path }

InstallOptions { dest, prefer_cli, min_free_bytes, inspect_after, register }
InstallProgress { phase, message, bytes_*, files_*, file }
InstallResult { dest, source, notes, model_info: Option<ModelInfoSummary> }
InstallCancel { new, request, is_requested, clear }

install_model(source, opts, on_progress)
install_model_cancellable(source, opts, cancel, on_progress)
install_model_with(..., HfCliRunner, Option<&mut ModelRegistry>, …)

ensure_space, detect_incomplete_download, convert_subprocess, …
```

Install is **generic**: any `owner/name`. Post-install optional inspect + `ModelRegistry::register`. No mapping from family → default repo.

---

## 2. Documented “supported” models (human SoT, not code)

From `/home/hunter/Projects/surmount/colibri/README.md` (“Other supported models”):

| Family | HF repo (product weights) | Disk (docs) | Engine build |
|--------|---------------------------|-------------|--------------|
| GLM-5.2 | `mastouri/GLM-5.2-colibri-int4-g64-with-int8-mtp` | ~372 GB | `make -C c glm` → `colibri` |
| Inkling | `nbeerbower/Inkling-colibri-int4` | ~469 GB | `inkling` |
| Kimi K3 | `moonshotai/Kimi-K3` | ~1.6 TB | `kimi_k3` |
| DeepSeek V4 Flash | `deepseek-ai/DeepSeek-V4-Flash-0731` (from `docs/deepseek-v4.md`) | ~167 GB | `deepseek_v4` |
| OLMoE | convert via `c/tools/convert_olmoe_merged.py` | ~4 GB | `olmoe` / GLM engine |

Notes:

- GLM: prefer **gs64 + int8 MTP** container; older per-row mirrors discouraged in README.
- Inkling also mentions upstream `thinkingmachines/Inkling` and dense-int4 convert tools.
- Kimi: native MXFP4 experts, no convert.
- OLMoE: not a simple HF one-click install in docs.

**None of this table lives in `registry.rs` or install.**

---

## 3. Native wizard Model step (current UX)

**State:** `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/wizard.rs`
`WizardStep::Model` (“Choose a model”), `WizardState.show_download`.

**UI:** `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/main.rs` (~3222+)

1. **Model folder** text field (`model_input`) — free path / `~` expand.
2. **Scan models** → `scan_registry` → `host::scan_model_registry` → list of **already-on-disk** `ModelEntry`.
3. Click row → `select_registry_entry` → fills path only.
4. **Show download options** (feature `install`, default on) → freeform form:
   - HF repo `owner/name` (`repo_input`)
   - revision, dest override, min free GB
   - Install / Cancel → `validate_install_form` → `install_async` → `install_model_cancellable`

**Host helpers** (`host.rs`):

```text
registry_scan_roots, scan_model_registry, format_registry_entry
format_empty_registry_scan, format_registry_scan_status
usable_registry_models, pick_single_usable_model
validate_install_form → (repo_id, revision, dest under store)
resolve_install_dest → default store/owner__name
install_options_for_ui (prefer_cli: false for hub progress)
install_async, progress_view_for_install, …
```

**Default dest:** `model_store/owner__name` (not `owner/name` nested).

**Readiness step** reuses scan/install CTAs when path is empty/invalid; still no remote catalog.

---

## 4. SPA / Tauri

### SPA `web/`

- `/home/hunter/Projects/surmount/colibri/web/src/App.tsx`: model `<select>` from `listModels` → `GET {base}/models` (OpenAI ids, default `glm-5.2-colibri`).
- Chat client only. **No** HF install, **no** local registry, **no** family catalog.

### Tauri `desktop/`

- `/home/hunter/Projects/surmount/colibri/desktop/src-tauri/src/lib.rs`: empty shell.
- README: model is external; no native install/picker.

---

## 5. Gaps vs “picker for all supported models”

| Need | Status |
|------|--------|
| Export list of product-supported HF models | **Missing** in sys |
| HF repo id + family + display name + size hint | **Docs only** |
| Install from registry/catalog entry | Install takes freeform `InstallSource`; no entry type |
| Local installed list for “use existing” | **Present** (`ModelRegistry` + native scan UI) |
| Families complete in routing | **Yes** (5 families incl. Olmoe) |
| Catalog complete for product HF set | **Incomplete as code** (5 families documented; 4 main HF one-shots + OLMoE convert) |
| Register after install from native UI | `install_options_for_ui` sets `register: false`; path filled on Done elsewhere / rescan |

---

## 6. Recommended wire-up: wizard “Choose a model”

### 6.1 Add a **supported catalog** in `colibri-sys` (small, static)

New types (suggested; not present today), e.g. in `model/catalog.rs` or next to registry:

```text
pub struct SupportedModel {
  pub id: &'static str,              // stable key, e.g. "glm-5.2-gs64"
  pub display_name: &'static str,
  pub family: ModelFamily,
  pub hf_repo_id: &'static str,      // owner/name
  pub revision: Option<&'static str>,
  pub approx_disk_bytes: Option<u64>,
  pub notes: &'static str,           // e.g. "prefer gs64 + int8 MTP"
  pub installable: bool,             // false for OLMoE convert-only if desired
}

pub fn supported_models() -> &'static [SupportedModel]
pub fn supported_model_by_id(id: &str) -> Option<&'static SupportedModel>
// optional: default_install_source(m) -> InstallSource
// optional: default_dest(store, m) -> PathBuf  // reuse owner__name rule
```

Seed from README + `docs/deepseek-v4.md` (table in §2). Keep OLMoE as non-installable or “local convert” note.

**Why sys:** one SoT for native, future SPA, docs tests, FFI later. Not the local `ModelRegistry` (that stays disk inventory).

### 6.2 Wizard Model step UX flow

```text
[Catalog]  list supported_models()
             · show name, family, ~size, notes
             · badge if matching installed entry under store (path contains repo leaf / scan by family)

Select catalog row
  → prefill repo_input (repo_id), optional revision
  → prefill dest (store/owner__name) via validate_install_form / resolve_install_dest
  → show approx_disk vs install_free_bytes / check_install_free_space

[Install]  install_async(repo_id, rev, dest, min_free, …)
  → on Done: set model_input to result.dest; optional register + scan_registry

[Or use installed]
  → existing scan list (ModelEntry rows) → select_registry_entry

[Advanced]
  → keep freeform owner/name collapse (current form)
```

Concrete host glue already usable:

| Step | API |
|------|-----|
| List remote product set | **new** `supported_models()` |
| Size gate | `check_install_free_space`, `DEFAULT_INSTALL_MIN_FREE_BYTES`, or catalog `approx_disk_bytes` as min |
| Install | `InstallSource::HuggingFace { … }` + `install_model_cancellable` / native `install_async` |
| Family/engine after install | `ModelInfo::inspect` / `ModelEntry.family` / `engine_id` |
| List local | `ModelRegistry::open` + `refresh` / `scan_model_registry` |
| Set active path | fill `model_input` (prefs `last_model_path` on finish) |

### 6.3 Minimal change path (if deferring sys catalog)

Hardcode the README table as `const CATALOG: &[…]` in `colibri-native` only, still call `validate_install_form` + `install_async`. Prefer sys for single SoT.

### 6.4 Do not overload `ModelRegistry`

- **Registry** = what is on disk under roots.
- **Catalog** = what Colibri product supports downloading/running.
Picker UI shows both: catalog for Install, registry for “already have.”

---

## 7. Quick reference: files

| File | Relevance |
|------|-----------|
| `crates/colibri-sys/src/model/registry.rs` | Local inventory only |
| `crates/colibri-sys/src/model/mod.rs` | Family + inspect |
| `crates/colibri-sys/src/model/install.rs` | HF install pipeline |
| `crates/colibri-sys/docs/user-guide.md` §3, §9 | Host docs |
| `crates/colibri-native/src/wizard.rs` | Model step machine |
| `crates/colibri-native/src/main.rs` | Model UI + install form |
| `crates/colibri-native/src/host.rs` | scan/install/validate helpers |
| `README.md` § “Other supported models” | Product HF list + sizes |
| `docs/deepseek-v4.md`, `docs/inkling.md`, `docs/kimi_k3.md` | Family-specific repos |
| `web/src/App.tsx` | Serve-side model id select only |
| `desktop/` | No model picker |

---

## 8. Acceptance sketch for implementer

1. `supported_models()` returns ≥4 installable entries (GLM, Inkling, Kimi, DeepSeek V4) with correct `hf_repo_id` + `ModelFamily`.
2. Wizard Model step shows catalog rows (not only freeform HF).
3. Select → fields filled → Install uses existing `install_async` path → `model_input` = dest.
4. Local scan list still works for installed models.
5. Freeform advanced form remains optional.
