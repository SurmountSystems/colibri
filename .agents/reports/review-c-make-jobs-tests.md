# Review: C make jobserver / circular colibri / quant.h knobs (tests), r3

**Role:** tests specialist. No product code changes.
**Date:** 2026-08-13
**Round:** 3 (fix2 always-make on circular dry-run)
**Scope:** test coverage and quality for the three named operator contracts, plus whether r1/r2 test issues stayed fixed. Not style, naming, or architecture. Not the cold-expert plan.rs track.
**SoT:** `/tmp/grok-1000/grok-review-c-make-jobs.md`, fix2 `/tmp/grok-1000/grok-impl-summary-c-make-jobs-fix2.md`, mop `/tmp/grok-1000/grok-process-mop-c-make-jobs-fix2.md`, `c/tests/test_makefile_jobs.py`

## Verdict

**clean.** R2 Issue 1 is fixed: `MakefileCircularColibriTests._dry` now runs `make --no-print-directory -B -n`. With a current dummy `colibri.exe` present, `test_windows_colibri_alias_is_not_a_self_edge` still prints `-o colibri.exe` and passes. R1 isolation, `_has_bare_make` both ways, default `-jN` pin, and jobserver-auth positive line remain green under parent `MAKEFLAGS=-j4`. No wall-clock or CPU-saturation race is used as a CI gate. No new test holes in scope.

## R2 issue

| R2 | Status | Notes |
|----|--------|--------|
| Issue 1 (bug): Windows alias dry-run missing `-B` | **fixed** | `_dry` at `c/tests/test_makefile_jobs.py:303-304` passes `-B` with `-n`, same as `test_makefile_platform.py`. Kept no-Circular, `-o colibri.exe`, and no `No rule to make target`. Re-ran this review with a future-dated dummy `c/colibri.exe`: 1 test OK. Dummy removed. No leftover exe. |

R1 Issues 1-5 stay fixed (MAKEFLAGS isolation, Windows alias lock, HasBareMake both ways, default `-jN` vs `just --evaluate make_jobs`, jobserver-auth positive line). Not re-listed.

## Named contracts vs tests

| Contract | Guard | Strength |
|----------|--------|----------|
| `just check` / `just c-check` pass jobserver `-jN`, not implicit `-j1` | Isolated `just -n`; N pinned to `make_jobs`; caller `-j2` and `--jobserver-auth` keep a second `-j` off | **Strong.** Re-checked under `MAKEFLAGS=-j4`. |
| Nested recipes use `$(MAKE)`; `clean` then `portable` then `test` stay serial | Recipe shape + `HasBareMakeTests` | **Strong.** |
| No Unix `Circular colibri <- colibri` | Three `make -B -n` tests including Linux triplet | **Strong.** `-B` does not hide a Circular parse warning. |
| Windows `colibri` → `colibri.exe` alias | `test_windows_colibri_alias_is_not_a_self_edge` with `-B` | **Strong.** Recipe line is printed even when the exe is current. |
| Knobs out of `quant.h`, still live, no `-Wno-unused-variable` | Header / `getenv` / two `-Werror=unused-variable` compiles / Makefile text | **Strong.** |

## Observed this review

- Dummy current `colibri.exe` + Windows alias test: OK, then dummy deleted.
- `cd c && python3 -m unittest tests.test_makefile_jobs tests.test_makefile_platform tests.test_makefile_cuda_scope -v`: 31 tests, OK.
- `MAKEFLAGS=-j4 python3 -m unittest tests.test_makefile_jobs.JustCheckJobserverTests -v`: 5 tests, OK.
- No leftover `c/colibri.exe`.

## Issues

0 issues.
