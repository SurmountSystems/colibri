# Implement: catalog Installed badge (wizard Supported models)

**Date:** 2026-08-11
**Scope:** `colibri-native` wizard / Tools Supported models rows vs local registry
**Recon:** `.agents/reports/recon-catalog-installed-badge.md`

---

## Outcome

Supported models catalog rows that are **Present** on disk paint as **solid white** (unselected) with **Installed** on the right. Selected installed rows keep primary green selection and still show the badge. Not-installed rows stay the hollow/dark secondary style.

DeepSeek path covered: leaf folder `DeepSeek-V4-Flash-0731` matches catalog `deepseek-ai/DeepSeek-V4-Flash-0731` via repo leaf name; also `owner__name` and nested `owner/name`.

---

## What changed

| File | Change |
|------|--------|
| `crates/colibri-native/src/host.rs` | `catalog_is_installed`, `path_ends_with_owner_name`, `CatalogRowStyle`, `catalog_row_style` + unit tests |
| `crates/colibri-native/src/main.rs` | `supported_catalog_panel` match + flex row + badge; select sets model path when Present; enter Model step rescans if registry empty; install Done rescans (status preserved) |
| `crates/colibri-native/src/i18n.rs` | `catalog.installed` EN "Installed" / IT "Installato"; core-surface test |

---

## Match rules (`catalog_is_installed`)

Present only. Convert-only (`hf_repo` None) never matches.

1. Folder name == HF repo leaf
2. Folder name == `owner__name`
3. Path ends with `owner/name` components

---

## Paint (`catalog_row_style`)

| State | fill | text | badge |
|-------|------|------|-------|
| Selected (installed or not) | primary | primary_fg | if installed |
| Installed, not selected | `DOGE_WHITE` | `DOGE_BLACK` | yes |
| Default | secondary | text | no |

---

## TDD

**Red (observed):** stub returned `None` / secondary always.

- `catalog_is_installed_matches_leaf_name_deepseek` FAIL
- `catalog_is_installed_matches_owner_double_underscore_name` FAIL
- `catalog_is_installed_matches_nested_owner_name` FAIL
- `catalog_row_style_installed_unselected_is_solid_white` FAIL
- `catalog_row_style_selected_keeps_primary_and_badge` FAIL

**Green:** real match + style; same filters pass.

---

## Verify

```text
cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets -- -D warnings   # ok
cargo test -p colibri-native                                  # 258 passed
```

---

## Product notes

- Cold start already seeds `registry_entries`; if empty when advancing onto Model, one `scan_registry` runs so badges appear without Scan first.
- Click installed catalog row: sets model path to matched leaf (and still fills install form when installable).
- After install Done: deferred registry rescan so badge flips without leaving the step; install-complete status kept.
- Badge copy is operational only (i18n `catalog.installed`), not brand theater.
