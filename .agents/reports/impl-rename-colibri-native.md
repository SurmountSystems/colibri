# Implement: rename colibri-desktop-gpui → colibri-native

**Date:** 2026-08-10
**Scope:** Workspace crate rename only. Tauri `desktop/` and `colibri-desktop` product shell untouched.

## What changed

| Action | Detail |
|--------|--------|
| Directory | `crates/colibri-desktop-gpui` → `crates/colibri-native` |
| Package / bin | `colibri-native` (`[[bin]] name = "colibri-native"`) |
| Workspace | Root `Cargo.toml` members + comment |
| Product strings | Plan-warn log → `colibri-native:`; main crate doc mentions package; window title / chrome already `colibrì (native)` |
| Docs | Root README tree + link; crate README commands; `.agents/RESIDUAL.md` scope and CLOSED paths |

Historical `.agents/reports/*` left as-is (past implement/review notes).

## Verify

```
cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets -- -D warnings   # exit 0
cargo test -p colibri-native                                  # 23 passed
cargo build -p colibri-native                                 # exit 0 → target/debug/colibri-native
```

`rg colibri-desktop-gpui` clean under `crates/`, root README, workspace `Cargo.toml`, `Cargo.lock`, and living residual (old names remain only in historical reports under `.agents/reports/`).

## Run

```bash
cargo run -p colibri-native
```

## Not renamed

- Tauri shell under `desktop/`
- Any `colibri-desktop` product identity
