# Track SPA: native pixel parity — implement report

**Date:** 2026-08-10
**Residual:** `open:tauri-parity` **closed**
**Package:** `colibri-native`
**Scope:** GPUI shell visual system + layout density matching `web/` (not pixel-perfect CSS; not REST).

---

## Deliverables (B1–B8)

| # | Item | Status |
|---|------|--------|
| B1 | Mint design tokens (`#4ed6a5`, dark teal BG) | **done** `theme.rs` |
| B2 | Left rail + top tabs Chat \| Brain \| Profiling | **done** `main.rs` shell |
| B3 | Chat hero empty state + suggested prompts | **done** orb + 3 prompts |
| B4 | Topbar live badges (tokens, tok/s, TTFT) | **done** stream + Done metrics |
| B5 | Proportional tier bar | **done** VRAM/RAM/disk shares |
| B6 | Profiling page (share bars / path charts / table) | **done** web phase model |
| B7 | i18n English + second locale | **done** en + it |
| B8 | Docs / fidelity / residual close | **done** |

Native lifecycle kept: Machine, Doctor, Plan, Start engine, Install, Inference.

---

## Code

| Path | Role |
|------|------|
| `crates/colibri-native/src/theme.rs` | **New.** Mint tokens, phase colors, rail width |
| `crates/colibri-native/src/i18n.rs` | **New.** Locale en/it string tables + `t` / `t_fmt` |
| `crates/colibri-native/src/profiling_view.rs` | **New.** DerivedTurn, share bars, chart heights, tier shares, badge formatters |
| `crates/colibri-native/src/main.rs` | Shell rewrite: rail + tabs + hero + badges + tier bar + prof page + i18n; Brain full-page integrates atlas hover / Full grid |
| `crates/colibri-native/src/text_input.rs` | Cursor / selection / field colors → mint family |
| `crates/colibri-native/src/atlas.rs` | Unchanged (Atlas track); wired into Brain tab |
| `crates/colibri-native/docs/fidelity.md` | SPA shell / PROF page / tiers / Tauri chrome rows |
| `crates/colibri-native/README.md` | Visual system + surface table |
| `.agents/RESIDUAL.md` | `open:tauri-parity` closed |

### Layout (product chrome)

```
┌────── rail ~292px ──────┬──────────── main ────────────┐
│ brand (mint)            │ topbar: model · tabs · badges│
│ Lifecycle: Machine      │ Chat | Brain | Profiling     │
│ Doctor / Plan / Start   │                              │
│ Runtime: tier bar + HW  │ Chat: hero OR messages       │
│ Inference + Install     │ Brain: full page + hover tip │
│ locale · about · status │ Profiling: tiles/charts/table│
└─────────────────────────┴──────────────────────────────┘
```

### Behavior notes

- **Brand:** BG `#080b0d`, primary `#4ed6a5`, panels `#0d1215` (web `index.css`).
- **Badges:** live token estimate while streaming; Done overwrites with engine completion_tokens / tok/s; TTFT from first Token after Send.
- **Tier bar:** proportional flex bases from `tier_share_fractions(vram,ram,disk)`.
- **Profiling:** last 40 turns; phases I/O wait / matmul / attention / LM head / other (web colors); empty copy when no turns.
- **Brain tab:** calls existing `brain_view_from_map` / full max / `format_brain_tooltip` / `display_to_source`; does not reimplement atlas parse.
- **i18n:** English complete for shell surface; Italian second locale (rail footer cycle).
- **Not claimed:** pixel-identical spacing, webview CSS blur glass, SVG-exact charts, HTTP SPA connection sidebar.

---

## Tests / verify

```
cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets --features install -- -D warnings
cargo test -p colibri-native --tests
```

**Result:** clippy clean; **74** unit tests passed (host + atlas + i18n + profiling_view).

### New unit coverage (SPA)

- `i18n::tests::*` — nav keys, it locale, placeholders, locale cycle
- `profiling_view::tests::*` — derive other/toks, share segments, throughput/phase heights, recent window, badge formatters, tier shares, phase colors

---

## Coordination

- **Atlas track:** `atlas.rs` + host brain helpers left as-is; SPA re-wired hover + Full grid into full-page Brain tab after shell rewrite.
- **FFI track:** not touched.
- **Do not touch:** `c/**`, REST as engine path, Tauri/web deletion.

---

## Residual honesty

- `open:tauri-parity` removed from OPEN; listed CLOSED + production MVP note.
- Still open: `open:npu-inference`, `open:ffi-phase-d` (partial), `open:openai-rest`, `open:visual-pump-idle-stop`.
- Acceptance bar met: **same product family** (mint brand, rail+tabs, PROF charts, badges, i18n, layout density), not pixel-perfect.

---

*End report. No git mutations.*
