# Fix1: makefile-jobs test isolation

Workspace: `/home/hunter/Projects/surmount/colibri`

Product jobserver / circular / knobs were accepted. This round only tightens `c/tests/test_makefile_jobs.py`.

## Changes

1. **`_isolated_env`.** `_dry` no longer inherits parent `MAKEFLAGS` / `MFLAGS` / `GNUMAKEFLAGS`. Per-test overlay is applied after the pop. Default `-jN` asserts still require a positive `-j` (not rewritten to accept a missing `-j`).
2. **Windows alias lock.** `test_windows_colibri_alias_is_not_a_self_edge` dry-runs `TRIPLET=x86_64-w64-mingw32`: no Circular line, `-o colibri.exe` present, no `No rule to make target 'colibri'`.
3. **`HasBareMakeTests`.** True for `\tmake clean` and `\tmake -C foo bar`. False for `$(MAKE) clean`, quoted usage `make`, and `tools/make_deepseek_v4_tiny.py`. Live Makefile scan stays.
4. **Default `-jN`.** Parsed N must equal `just --evaluate make_jobs` and be `> 1` when `os.cpu_count() > 1`. Same pin for `c-check` and `c-test`.
5. **Jobserver-auth test.** Now requires `make -C c check` at end of output, not only the absence of `-j`.

## RED / GREEN

RED (Issue 1, before the env pop): `MAKEFLAGS=-j4 python3 -m unittest tests.test_makefile_jobs.JustCheckJobserverTests -v` failed `test_just_c_check_default_passes_dash_j`, `test_just_c_check_honors_explicit_make_jobs`, `test_just_c_test_default_passes_dash_j` because `make_j` stayed empty. Fail reason: inherited `-j4`.

GREEN: same command, 5 tests, exit 0. Also green under `MAKEFLAGS=--jobserver-auth=fifo:/tmp/coli-fake-jobserver`.

Issues 2-5 are lock-ins on already-correct product. No fake red.

## Post-impl verify

| Command | Exit |
|---------|------|
| `just --fmt --check` | 0 |
| `MAKEFLAGS=-j4 python3 -m unittest tests.test_makefile_jobs.JustCheckJobserverTests -v` | 0 |
| `MAKEFLAGS=--jobserver-auth=fifo:/tmp/coli-fake-jobserver python3 -m unittest tests.test_makefile_jobs.JustCheckJobserverTests -v` | 0 |
| `cd c && python3 -m unittest tests.test_makefile_jobs tests.test_makefile_platform tests.test_makefile_cuda_scope -v` | 0 (31 tests) |
