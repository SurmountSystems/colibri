# Implementation summary: C make jobserver, circular colibri, quant.h knobs

Workspace: `/home/hunter/Projects/surmount/colibri`

## What changed

### Files

| File | Change |
|------|--------|
| `justfile` | Default `make_jobs := num_cpus()`. `make_j` is `-jN` unless `MAKEFLAGS` already has `-j` or `--jobserver`. `c-check` and `c-test` pass `{{ make_j }}`. |
| `c/Makefile` | On Unix, do not define `colibri: colibri` or mark `colibri` phony. On Windows (`EXE=.exe`), keep the phony `colibri` alias to `colibri.exe`. `glm` stays a phony alias. Nested `check` / `portable` still use `$(MAKE)`. |
| `c/quant.h` | Removed file-scope `static int g_idot` / `g_i4s` / `g_xexp`. Kernel inlines stay. |
| `c/colibri.c` | Those knobs now live next to the other engine policy statics, right after `#include "quant.h"`. Env `IDOT` / `I4S` / `XEXP` unchanged. |
| `c/tests/test_makefile_jobs.py` | New structural tests for the three named contracts. |

### Design

1. **Jobserver.** Do not force `-j` inside `c/Makefile` (that would start a new jobserver and ignore the caller). `just check` is the path that used implicit `-j1`. It now passes `-j$(num_cpus())` at the top-level `make -C c check`. Nested recipes already used `$(MAKE)` (`clean`, then `portable`, then `test`), so they inherit `MAKEFLAGS` / `--jobserver-auth`. `check` still runs those three as sequential recipe lines so `clean` finishes before `portable`. Independent `tests/test_*$(EXE)` rules stay separate targets; there is no global `.NOTPARALLEL`. Override: `just c-check make_jobs=4`. Caller `MAKEFLAGS=-j2` or an existing jobserver leaves `make_j` empty.

2. **Circular colibri.** On Unix `EXE` is empty, so `colibri: colibri$(EXE)` was `colibri: colibri`. GNU make dropped that self-edge and printed `Circular colibri <- colibri dependency dropped.` The binary target is already named `colibri`; only Windows needs a phony `colibri` -> `colibri.exe` alias.

3. **Unused knobs.** `g_idot` / `g_i4s` / `g_xexp` are engine policy, not kernel implementation. They were `static` in `quant.h`, so every TU that only needed types/inlines (`test_e8_kernel.c`, `test_fp8_passthrough.c`) compiled unused objects. Moved the definitions into `colibri.c` (tests that `#include "../colibri.c"` still see one copy). Did not add `-Wno-unused-variable`. Did not delete the knobs.

## RED (before product edit)

Command: `cd c && python3 -m unittest tests.test_makefile_jobs -v`

Observed failures (same test bodies as GREEN):

| Test | Named contract | Fail reason |
|------|----------------|-------------|
| `JustCheckJobserverTests.test_just_c_check_default_passes_dash_j` | `just check` / `c-check` must not invoke make at implicit `-j1` | dry-run was `make -C c check` with no `-j` |
| `JustCheckJobserverTests.test_just_c_check_honors_explicit_make_jobs` | explicit caller `-j` / `make_jobs` | `just --set make_jobs 3` failed: variable not in justfile |
| `JustCheckJobserverTests.test_just_c_test_default_passes_dash_j` | same for `make test-c` | dry-run was `make -C c test-c` with no `-j` |
| `MakefileCircularColibriTests.test_make_colibri_has_no_circular_dependency` | no circular self-edge | `make -n colibri` printed `Circular colibri <- colibri` |
| `MakefileCircularColibriTests.test_make_portable_has_no_circular_dependency` | same for `portable` | `make[1]: Circular colibri <- colibri` |
| `MakefileCircularColibriTests.test_linux_named_binary_has_no_self_edge` | Unix `EXE` empty is the self-edge | same circular line |
| `QuantHeaderKnobTests.test_quant_h_does_not_define_engine_knob_statics` | knobs must not be header statics | `static int g_idot` still in `quant.h` |
| `QuantHeaderKnobTests.test_e8_kernel_tu_has_no_unused_knob_warnings` | `test_e8_kernel.c` must not warn unused knobs | `-Werror=unused-variable` on `g_xexp` / `g_i4s` / `g_idot` |
| `QuantHeaderKnobTests.test_fp8_passthrough_tu_has_no_unused_knob_warnings` | same for `test_fp8_passthrough.c` | same three unused statics |

`test_just_c_check_does_not_override_caller_makeflags` and the jobserver-auth test were already green (no extra `-j` to override). They lock the edge path after the default `-j` was added.

## GREEN (same command, same test bodies)

Command: `cd c && python3 -m unittest tests.test_makefile_jobs -v`

Result: **17 tests, OK, exit 0.**

## Test-expectation change

`_has_bare_make` was tightened after the first red run. The first draft treated the word `make` inside `echo "usage: make deepseek-v4-oracle ..."` as a jobserver-dropping invocation. Named contract is: **recipe commands** must use `$(MAKE)`, not that the word "make" cannot appear in a usage string. Quoted strings are now stripped before the scan. Stronger/equal check for the real contract, not a looser product fit.

## How `just check` / nested make share the jobserver

1. `just check` depends on `c-check`.
2. `c-check` runs `make -C c check -j<N>` where `N = num_cpus()`, unless `MAKEFLAGS` already contains `-j` or `--jobserver`.
3. `c/Makefile` `check` is three sequential `$(MAKE)` lines: `clean`, then `portable`, then `test`. Each child inherits `MAKEFLAGS` (including `--jobserver-auth` / `--jobserver-fds`).
4. `portable` is `$(MAKE) colibri$(EXE) ARCH=$(PORTABLE_ARCH)` (another jobserver-aware child).
5. `test` -> `test-c: $(TEST_BINS)`. Independent `gcc … tests/test_*.c` recipes can run concurrently under that jobserver.
6. No extra `-j` on nested `$(MAKE)` lines, so they do not start a second jobserver.

## How unused `g_xexp` / `g_i4s` / `g_idot` were fixed

Moved the three file-scope statics out of `quant.h` into `colibri.c`. Architecture-dependent `g_i4s` defaults and the existing comments moved with them. Header-only kernel TUs no longer compile those objects. Engine TU still owns `IDOT` / `I4S` / `XEXP`. CFLAGS still do not contain `-Wno-unused-variable`.

## Post-impl verify (command + exit code)

| Command | Exit |
|---------|------|
| `cd c && python3 -m unittest tests.test_makefile_jobs -v` | 0 |
| `cd c && python3 -m unittest tests.test_makefile_jobs tests.test_makefile_platform tests.test_makefile_cuda_scope -v` | 0 |
| `just --fmt --check` | 0 |
| `just -n c-check` -> `make -C c check -j16` | 0 |
| `just --set make_jobs 3 -n c-check` -> `make -C c check -j3` | 0 |
| `MAKEFLAGS=-j2 just -n c-check` -> `make -C c check` (no extra `-j`) | 0 |
| `just -n check` includes `make -C c check -j16` | 0 |
| `make -C c -n colibri portable` (no `Circular` line) | 0 |
| `make -C c tests/test_e8_kernel tests/test_fp8_passthrough -j$(nproc)` | 0 |
| `./tests/test_e8_kernel` | 0 |
| `./tests/test_fp8_passthrough` | 0 |
| `make -C c colibri` (engine rebuild with moved knobs) | 0 |
| Recompile those two TUs with the makefile warning set; `rg` for `g_idot\|g_i4s\|g_xexp\|unused-variable` | no matches |

No wall-clock race was used as a CI gate. Parallelism is asserted by jobserver / `$(MAKE)` / independent targets / just `-j` shape.
