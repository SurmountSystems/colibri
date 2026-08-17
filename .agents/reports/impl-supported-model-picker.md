# Report: supported-model catalog + native picker

**Date:** 2026-08-11
**Scope:** `colibri-sys` static catalog API; `colibri-native` wizard Model step + Tools picker
**Git:** no commit (operator-owned)

## Honesty: registry vs catalog

Prior **`ModelRegistry`** is **local disk inventory only**: it scans configured store roots for directories with `config.json` (`open` / `refresh` / `entries`). It is **not** a product list of models Colibri supports, and not the SPA `/v1/models` list (that is live after server load).

The product gap “picker of all models Colibri supports” is fixed by a new **static supported catalog** seeded from the root **README.md** family table + engine `ModelFamily` routing + documented HF ids. Install UX uses that catalog; scan remains for already-installed local leaves.

## A. colibri-sys catalog

**File:** `crates/colibri-sys/src/model/catalog.rs`
**Exports:** `model` module + crate root (`SupportedModel`, `supported_models`, `supported_model_by_id`, `supported_model_by_hf_repo`).

| Field | Role |
|-------|------|
| `id` | Server-style id from `c/openai_server.py` (`glm-5.2-colibri`, `inkling-colibri`, `kimi-k3-colibri`, `deepseek-v4-colibri`) + `olmoe-colibri` for OLMoE |
| `display_name` | README product names |
| `family` | Existing `ModelFamily` |
| `hf_repo` | `owner/name` when documented; `None` for convert-only OLMoE |
| `notes` | Short operational notes (MXFP4, convert path, …) |
| `disk_hint` | README disk size strings when present |

**Seed entries (5):**

| id | HF repo | Notes |
|----|---------|--------|
| `glm-5.2-colibri` | `mastouri/GLM-5.2-colibri-int4-g64-with-int8-mtp` | |
| `inkling-colibri` | `nbeerbower/Inkling-colibri-int4` | |
| `kimi-k3-colibri` | `moonshotai/Kimi-K3` | native MXFP4 note |
| `deepseek-v4-colibri` | `deepseek-ai/DeepSeek-V4-Flash-0731` | from `docs/deepseek-v4.md` (README names checkpoint without full URL) |
| `olmoe-colibri` | *(none)* | convert-only: `c/tools/convert_olmoe_merged.py` |

No invented models beyond README / engine support / docs HF id for V4.

## B. colibri-native UX

**Pure helpers** (`host.rs`):

- `CatalogSelection` — mapped install form fields + status
- `catalog_selection_from_model` / `catalog_selection_by_id`
- `format_supported_model_row` — display name · disk · repo or convert-only
- `list_supported_models` — thin re-export of sys catalog

**UI:**

1. **Supported models** section always visible on wizard **Model** step and **Tools** (before freeform install form).
2. On select of installable entry: fill `repo_id` + dest (repo name segment); status `Ready to install {name}`; open download form (`show_download = true`).
3. Existing **Install model** uses those fields (unchanged install path).
4. **Scan models** still lists local registry leaves.
5. Freeform HF fields kept under “Show download options” / always on Tools as secondary.

**Copy:** operational English only. i18n keys `catalog.supported` / `catalog.supportedHelp` (en + it). Reused `rail.scanModels`, `rail.installBtn`, install field labels. No marketing slogans.

## C. TDD

- Sys: catalog non-empty; HF shape; known ids; lookup; OLMoE convert-only; family coverage.
- Native: GLM maps to form fields; OLMoE non-installable; unknown id; rows format; installable entries pass `validate_install_form`.

## D. Verify

| Step | Result |
|------|--------|
| `cargo fmt -p colibri-sys -p colibri-native` | ok |
| `cargo clippy -p colibri-sys -p colibri-native --all-targets -- -D warnings` | ok |
| `cargo test -p colibri-sys --lib` | **115** passed |
| `cargo test -p colibri-native --bin colibri-native` | **222** passed |

## Files touched

- `crates/colibri-sys/src/model/catalog.rs` (new)
- `crates/colibri-sys/src/model/mod.rs`
- `crates/colibri-sys/src/lib.rs`
- `crates/colibri-native/src/host.rs`
- `crates/colibri-native/src/main.rs`
- `crates/colibri-native/src/i18n.rs`
- `.agents/reports/impl-supported-model-picker.md` (this file)
