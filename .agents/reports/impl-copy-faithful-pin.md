# Report: product-copy fidelity process pin

**Date:** 2026-08-11
**Task:** Pin standing law: keep product copy faithful to original Colibri; no invented marketing.

## What changed

| Path | Change |
|------|--------|
| `/home/hunter/Projects/surmount/colibri/.agents/RESIDUAL.md` | Standing process pin § *Product copy fidelity*; Open residual one-liner; header updated |
| No project `AGENTS.md` | Repo has none; full rule lives in residual Open / standing section |

## Rule summary (on disk)

- User-visible copy (i18n, wizard, hero, rail, Tools, status) must stay **faithful to original Colibri**, primarily `web/src/i18n/en.ts` (and `it` / other locales) and original desktop/SPA strings already in-tree.
- Agents must **not invent** marketing slogans, taglines, hero lines, or brand voice. Prefer exact port of web i18n keys/values.
- Native-only functional strings (setup next step, error recovery, readiness) stay **plain operational English**, matching adjacent original tone when possible.
- When adding keys, cite source path in a short comment or report if non-obvious.
- No mass i18n rewrite unless a same-key divergence from `web/` is clear.

## Optional divergence scan

Not run this turn (out of scope unless inventing marketing is found). Next UI/i18n edit should diff native `crates/colibri-native/src/i18n.rs` against `web/src/i18n/en.ts` for hero/wizard/rail keys if copy is touched.

## Git

No commit (operator-owned).
