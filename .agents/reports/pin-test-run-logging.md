# Pin: native crash / test-run logging

**Date:** 2026-08-13. Process only. No product edits.

`AGENTS.md` had no crash-log pin. Added **Native crash logs (test runs)** after
Product copy fidelity. Native-logs product work is already in tree.

Pins: first artifact is `~/.local/share/colibri/logs/native.log` (and rotates);
keep default-on panic/tracing/C-banner tee; be honest when a signal never
writes; last-run log plus TDD, not parse-only tests after a real-app crash;
logging contract tests must fail if that path breaks. No Sentry.

Residual left unchanged. No git add or commit.
