# Fix2: circular dry-run always-make (`-B`)

Workspace: `/home/hunter/Projects/surmount/colibri`

Only `c/tests/test_makefile_jobs.py` changed. Product makefile / justfile / knobs untouched.

## Change

`MakefileCircularColibriTests._dry` now runs `make --no-print-directory -B -n`, matching `test_makefile_platform.py` and `test_makefile_cuda_scope.py`. After `portable` leaves an up-to-date `colibri.exe`, the Windows alias test still prints the `colibri.exe` recipe. Kept no-Circular, `-o colibri.exe`, and no `No rule to make target`.

## RED

Dummy `c/colibri.exe` newer than prerequisites. Command: `python3 -m unittest tests.test_makefile_jobs.MakefileCircularColibriTests.test_windows_colibri_alias_is_not_a_self_edge -v`

Fail: `'-o colibri.exe' not found` in `Nothing to be done for 'colibri'.`

## GREEN

Same command after `-B`, with dummy still present: 1 test, OK, exit 0. Dummy deleted after.

## Post-impl verify

| Command | Exit |
|---------|------|
| Windows alias test with dummy `c/colibri.exe` | 0 |
| `cd c && python3 -m unittest tests.test_makefile_jobs tests.test_makefile_platform tests.test_makefile_cuda_scope -v` | 0 (31 tests) |
| `just --fmt --check` | 0 |
| Dummy removed | gone |
