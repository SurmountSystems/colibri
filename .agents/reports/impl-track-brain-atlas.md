# Track Atlas: full Brain atlas — implement report

**Date:** 2026-08-10
**Residual:** `open:brain-full-atlas` **closed**
**Package:** `colibri-native`

---

## Deliverables (C1–C7)

| # | Item | Status |
|---|------|--------|
| 1 | Parse web-shaped experts.json (`"layer:expert"` keys) | **done** `atlas::parse_experts_json` |
| 2 | Load order: `COLIBRI_EXPERTS_JSON` / `COLI_EXPERTS_JSON` → cwd `experts.json` | **done** `load_experts_atlas` |
| 3 | `emap_row_to_layer` (GLM: row+3, last row MTP→78) + tests | **done** |
| 4 | Tooltip builder specialist/generalist/entropy/top-3 + depth roles + tests | **done** `format_brain_tooltip` |
| 5 | GPUI hover hit-test; display→source under stride | **done** per-cell `on_hover` + `display_to_source` |
| 6 | Full-res toggle / env; default sample documented | **done** UI **Full grid** + `COLIBRI_BRAIN_FULL` |
| 7 | Residual + fidelity close for atlas | **done** |

Out of scope (untouched): `c/**`, colibri-sys FFI, SPA chrome / mint / i18n / PROF charts.

---

## Code

| Path | Role |
|------|------|
| `crates/colibri-native/src/atlas.rs` | **New.** Atlas types, parse, load, layer map, depth roles, tip text |
| `crates/colibri-native/src/host.rs` | `BrainView` strides + `max_cells`; `brain_view_from_map_with_max`; `display_to_source`; `env_brain_full` |
| `crates/colibri-native/src/main.rs` | Atlas load on start; hover tip state; Full grid toggle; tip panel under grid |
| `crates/colibri-native/Cargo.toml` | `serde` + `serde_json` |
| `crates/colibri-native/docs/fidelity.md` | Brain row **done**; limits → full section |
| `crates/colibri-native/README.md` | Brain row mentions atlas + toggle |
| `.agents/RESIDUAL.md` | Closed `open:brain-full-atlas`; CLOSED table + MVP note |

### Behavior notes

- **Default paint:** still stride-samples at `BRAIN_MAX_CELLS` (2048). Full 19k-div paint is opt-in (toggle or env). Full mode raises grid max height and shrinks cell px when cols are large; not a canvas virtualizer (honest div grid).
- **Atlas missing:** empty atlas; tips use English depth-role strings (web `brain.*` meaning, ASCII dashes).
- **Hover:** GPUI `on_hover` on each cell; tip rebuilt from source indices under current strides so sampled grids still name the sampled expert.
- **No multi-MB JSON in binary/tests:** tiny inline fixture in `atlas::tests`.

### Operator load path

```bash
# optional atlas
export COLIBRI_EXPERTS_JSON=/path/to/web/public/experts.json
# optional full paint from start
export COLIBRI_BRAIN_FULL=1
cargo run -p colibri-native
```

Or place `experts.json` in cwd; or click **Full grid** in the Brain panel header.

---

## Tests / verify

```
cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets -- -D warnings   # clean
cargo test -p colibri-native                                  # 57 passed
```

### New / extended unit coverage

- `atlas::tests::parse_web_shaped_experts_json`
- `atlas::tests::parse_empty_and_invalid_json`
- `atlas::tests::parse_layer_expert_key_accepts_web_keys`
- `atlas::tests::emap_row_to_layer_glm_convention`
- `atlas::tests::depth_role_bands_and_mtp`
- `atlas::tests::tooltip_with_atlas_specialist_top3`
- `atlas::tests::tooltip_generalist_and_depth_fallback`
- `atlas::tests::load_from_path_fixture_file`
- `host::tests::brain_view_full_res_mode_no_sample_on_large_map`
- `host::tests::display_to_source_matches_brain_view_sampling`
- Existing heat/pulse/sample tests remain green

---

## Residual honesty

- `open:brain-full-atlas` removed from OPEN; listed CLOSED + production MVP note.
- Related still open: `open:tauri-parity` (3-D galaxy / full SPA), `open:npu-inference`, `open:ffi-phase-d`.

---

*End report. No git mutations.*
