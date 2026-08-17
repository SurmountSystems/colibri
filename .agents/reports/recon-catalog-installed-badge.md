# Recon: catalog "Installed" badge (wizard Choose a model)

**Date:** 2026-08-11
**Scope:** colibri-native wizard step 3 catalog rows vs local `ModelRegistry` scan
**Goal:** detect already-installed models on Supported models rows; solid white + "Installed" on the right

---

## Executive summary

| Layer | Role today | Installed badge? |
|-------|------------|------------------|
| Static `SupportedModel` catalog | Product list (5 rows) | **No** local state |
| `ModelRegistry` / `scan_model_registry` | Disk inventory under store (depth ≤2) | Finds leaves like `…/DeepSeek-V4-Flash-0731` |
| Wizard catalog paint | Selection green / unselected secondary | **No** install crosswalk |
| Catalog → install form | Fills HF repo + dest = **repo name segment** | Dest path is the best link to disk |

**Gap:** catalog paint never consults `registry_entries`. Startup already scans the store into `registry_entries`, so data is usually present on step 3 without an extra click; match logic is missing.

---

## 1. Catalog list UI (wizard Choose a model)

| Piece | Path |
|-------|------|
| Step enum | `crates/colibri-native/src/wizard.rs` — `WizardStep::Model` (index 2), title key `wizard.model.title` |
| Paint | `crates/colibri-native/src/main.rs` — `WizardStep::Model` branch (~3469+) calls `supported_catalog_panel` then path + Scan + registry list |
| Catalog panel | `main.rs` `supported_catalog_panel` (~1439–1507) |
| Row label pure | `host.rs` `format_supported_model_row` |
| Catalog map | `host.rs` `catalog_selection_from_model` / `catalog_selection_by_id` |
| i18n | `crates/colibri-native/src/i18n.rs` — `wizard.model.title` = "Choose a model"; `catalog.supported` / `catalog.supportedHelp`; scan `rail.scanModels` |
| Theme tokens | `crates/colibri-native/src/theme.rs` — `DOGE_WHITE` / palette roles |

**Current row paint** (`supported_catalog_panel`):

- Fill: selected → `p.primary` (green), else `p.secondary` (black DOGE / dark mint).
- Text: selected → `p.primary_fg`, else `p.text`.
- Single child: one label string (`format_supported_model_row`).
- Click → `select_supported_model` fills install form + opens download; does **not** set model path from disk.

**Registry list** (below catalog, same step) is separate: `format_registry_entry`, `p.secondary` rows, click → `select_registry_entry` sets **model path**.

---

## 2. Supported catalog → install paths

**Catalog source:** `crates/colibri-sys/src/model/catalog.rs`

| id | display_name | hf_repo | catalog `dest` (name segment) |
|----|--------------|---------|-------------------------------|
| `glm-5.2-colibri` | GLM-5.2 | `mastouri/GLM-5.2-colibri-int4-g64-with-int8-mtp` | `GLM-5.2-colibri-int4-g64-with-int8-mtp` |
| `inkling-colibri` | Inkling | `nbeerbower/Inkling-colibri-int4` | `Inkling-colibri-int4` |
| `kimi-k3-colibri` | Kimi K3 | `moonshotai/Kimi-K3` | `Kimi-K3` |
| `deepseek-v4-colibri` | DeepSeek V4 Flash | `deepseek-ai/DeepSeek-V4-Flash-0731` | **`DeepSeek-V4-Flash-0731`** |
| `olmoe-colibri` | OLMoE | *(none)* | *(none; convert-only)* |

**Dest mapping** (`host.rs` `catalog_selection_from_model`):

- Installable: `dest = hf_repo` after last `/` (repo leaf name).
- On select, dest is written into the install form override.

**Actual on-disk install dest** (`host.rs` `resolve_install_dest` / `validate_install_form`):

| Form dest override | Resolved path under store |
|--------------------|---------------------------|
| Non-empty relative (catalog default) | `store/<dest>` e.g. `~/.local/share/colibri/models/DeepSeek-V4-Flash-0731` |
| Empty | `store/{owner}__{name}` e.g. `deepseek-ai__DeepSeek-V4-Flash-0731` |
| Absolute under store | as given |

So a catalog-driven install lands at **name segment**, not `owner__name`. Manual empty-dest installs use `owner__name`. Operators may also place models as `store/owner/name` (HF layout, registry depth 2).

**Default store:** `colibri-sys` `paths::default_model_store_path` → env `COLIBRI_MODEL_STORE` / `COLI_MODEL_STORE`, else `$XDG_DATA_HOME/colibri/models` or `~/.local/share/colibri/models`.

---

## 3. Scan / registry inspect (already finds DeepSeek)

| API | Path | Behavior |
|-----|------|----------|
| `ModelRegistry::refresh` | `colibri-sys/src/model/registry.rs` | Walk roots depth ≤ **2**; dir with `config.json` = model leaf; no recurse into leaf |
| `classify_path` | same | `ModelInfo::inspect` when possible; family from `model_type`; status Present / Incomplete / … |
| `scan_model_registry` | `colibri-native/src/host.rs` | `ModelRegistry::open(roots).refresh()` → `Vec<ModelEntry>` |
| `registry_scan_roots` | host | default store (+ optional extra) |
| Cold start | `main.rs` `DesktopApp::new` | `ensure_model_directory` + scan → **`registry_entries`** seeded |
| Wizard Scan button | `scan_registry` | Rescan; may auto-set path if exactly one usable model |

**Leaf example operator cares about:**
`~/.local/share/colibri/models/DeepSeek-V4-Flash-0731`
(depth 1 under store; folder name == HF repo leaf; family via `config.json` `model_type` → `DeepseekV4`.)

`ModelEntry` has: `path`, `family`, `engine_id`, `status`, sizes, `model_type`, `note`. **No `hf_repo` field.**

Usable for badge: `Present` (and optionally Incomplete / MissingTokenizer if weights partially there; prefer **Present only** for solid "Installed").

---

## 4. Recommended match rule (catalog id ↔ local leaf)

Pure helper (suggested in `host.rs`, unit-tested):

```text
fn catalog_is_installed(model: &SupportedModel, entries: &[ModelEntry]) -> Option<&ModelEntry>
```

**Priority (first hit wins; case-sensitive path components as on disk; normalize only for string compares where noted):**

1. **Folder name == HF repo leaf**
   `entry.path.file_name() == hf_repo.rsplit('/').next()`
   Covers catalog dest and the DeepSeek case: `DeepSeek-V4-Flash-0731`.

2. **Folder name == `owner__name`**
   Empty-override install layout: `deepseek-ai__DeepSeek-V4-Flash-0731`.

3. **Path ends with `owner/name` components**
   Nested HF layout: `store/deepseek-ai/DeepSeek-V4-Flash-0731` (depth 2 scan already finds this).

4. **Family-only fallback (weak)**
   Only if **exactly one** usable entry has `entry.family == model.family` **and** that family is unique in the catalog (or unique among installable rows).
   Avoid multi-GLM ambiguity later; for today's 1:1 family↔catalog installables it works but is brittle. Prefer path rules 1–3 first; skip family-only for convert-only OLMoE unless a leaf is clearly olmoe.

5. **Optional:** if `selected_catalog_id` maps to install dest under store and that path exists as a registry leaf, treat installed (redundant if 1 works).

**Do not require** exact full-path equality to `store/dest` only; folder-name + nested owner/name cover real layouts.

**Status filter:** badge when matched entry is `ModelStatus::Present` (strict). Incomplete can show a different muted state later; out of scope for solid white "Installed".

**Refresh timing:** use `self.registry_entries` in `supported_catalog_panel` (already cold-started). After successful install completion, ensure registry rescan so badge flips without leaving the step.

---

## 5. UI paint approach (theme white)

**Operator ask:** installed rows = **solid white** styling + **"Installed"** on the **right** of the box.

### Layout

Row becomes horizontal flex:

- Left: existing label (`format_supported_model_row` or slightly shorter when badge present).
- Right: badge text `"Installed"` (plain operational English; optional i18n key `catalog.installed` en/it later; native-only operational is fine).

```text
.div row: flex, flex_row, items_center, justify_between, w_full
  .child(label)  // min_w_0, truncate if needed
  .child("Installed") when installed
```

### Colors (DOGE default)

| State | fill | text | border |
|-------|------|------|--------|
| Installed, not selected | **`DOGE_WHITE` / `0xFFFFFF`** (`p` has no dedicated "installed" role; use `theme::DOGE_WHITE` or add `installed_fill` later) | **`DOGE_BLACK`** (readable on white) | white or black thin border |
| Selected (whether installed or not) | `p.primary` | `p.primary_fg` | `p.primary` |
| Default uninstalled | `p.secondary` | `p.text` | `p.border` |

**Selected + installed:** keep **primary (green)** for selection affordance; still show **"Installed"** on the right so status is not lost. Do not replace selection green with white when selected.

**Mint:** solid white fill + dark text (`primary_fg` or near-black) still works on dark chrome; avoid using mint `p.text` (light) on white.

**DOGE purity:** white and black are already in the eight pure DOGE colors (`DOGE_WHITE`, `DOGE_BLACK` in `theme.rs`). No invented third palette for this badge.

### Click behavior (recommendation, product)

- Installed row click: set **model path** to matched leaf (like registry row) **and/or** keep install form fill for re-download; prefer path set so Doctor/engine work without a second click.
- Uninstalled installable: keep current install-form fill.
- Out of scope unless implementer expands: hide Install when Present.

---

## 6. Critical files (implement touch list)

| File | Why |
|------|-----|
| `crates/colibri-sys/src/model/catalog.rs` | Static catalog + `hf_repo` / ids (read-only unless helper moves to sys) |
| `crates/colibri-sys/src/model/registry.rs` | Scan depth, `ModelEntry`, status |
| `crates/colibri-sys/src/paths.rs` | Default store roots |
| `crates/colibri-native/src/host.rs` | **Match helper + tests**; `catalog_selection_*`; `scan_model_registry` |
| `crates/colibri-native/src/main.rs` | `supported_catalog_panel` paint + use `registry_entries`; optional select path when installed |
| `crates/colibri-native/src/theme.rs` | `DOGE_WHITE` / black for installed fill/fg |
| `crates/colibri-native/src/i18n.rs` | Optional `catalog.installed` = "Installed" |
| `crates/colibri-native/src/wizard.rs` | Step only; no paint |

Prior reports: `impl-supported-model-picker.md`, `recon-model-registry-picker.md`.

---

## 7. TDD sketch (implementer)

1. **Red:** pure tests in `host` — given `SupportedModel` deepseek + `ModelEntry` path `…/DeepSeek-V4-Flash-0731` Present → match; `owner__name` path → match; nested `owner/name` → match; unrelated folder → no match; Incomplete → no badge if Present-only.
2. **Green:** implement match helper.
3. Paint/wiring: optional snapshot-style assert hard; prefer pure paint-state helper `catalog_row_style(installed, selected) -> fill/fg` if tests should avoid GPUI.

---

## 8. Non-goals

- Changing install dest layout or HF download path.
- SPA `/v1/models` (live server ids, not wizard catalog).
- Inventing marketing copy; badge is operational **"Installed"** only.
