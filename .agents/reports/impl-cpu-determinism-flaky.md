# Implement report: flaky CPU determinism tok/s bound

**Status:** done
**Date:** 2026-08-12
**Scope:** `c/tests/test_inefficiency.py` check-path split; CI structural list; optional windll probe guard.

## Named contract

Two identical greedy REPLAY runs with the same seed must produce **identical structural telemetry** (hit-rate and same counts). That is determinism.

The **tok/s within 25%** assert is not determinism. It is a throughput-stability bound. The tiny replay is about 15 ms. A quiet local box already measured about 38.8% tok/s swing, so 0.25 is flaky. Do not loosen 0.25 to 0.30.

Red was already observed:

```
FAIL: test_cpu_vs_cpu_determinism
AssertionError: 0.2616402701461307 not less than 0.25
```

Engine C code was not changed. Hit-rate identity was not the failure.

## What changed

### Check-path determinism (`c/tests/test_inefficiency.py`)

Kept the method name `TinyEfficiencyTest.test_cpu_vs_cpu_determinism` so `make check` / unittest discover still collect it.

- Still asserts both runs exit 0.
- Still asserts `hit_pct` identity, and now also that both hit-rates are present (`assertIsNotNone`).
- Also asserts identity of `loads_per_tok` and `experts_loaded` when either run parsed them.
- **Removed** the tok/s 25% bound from this test and from the default suite. No replacement perf gate.

`test_tiny_tok_s_floor` is unchanged (absolute floor, not run-to-run stability). CI already omits that floor.

### CI

`.github/workflows/ci.yml` efficiency job now includes `test_cpu_vs_cpu_determinism` in the structural list. Comment updated: the tok/s bound is gone from the test, so the structural hit-rate check belongs here.

### Docs

- `c/tests/README_efficiency.md`: determinism line now says hit-rate / counts, not tok/s.
- Module docstring in `test_inefficiency.py` matches that.

### windll warning (touched)

`[plan] warning: Windows core probe failed: module 'ctypes' has no attribute 'windll'` was not a production Linux probe taking the Windows path by accident. `physical_cpu_count()` is correctly gated on `sys.platform == "win32"`. The warning came from `tests.test_env_defaults` mocking `sys.platform` to `"win32"` on Linux CPython (no `ctypes.windll`), then `coli.env_for` calling `physical_cpu_count()`.

Small guard in `c/resource_plan.py`: if `ctypes.windll` is missing, skip the WinAPI probe and fall through to `lscpu` with no warning.

Pinned by `PhysicalCpuCountTest.test_win32_without_windll_falls_through_to_lscpu`.

## Commands and exit codes

| Command | Exit |
|---------|------|
| `cd c && python3 -m unittest tests.test_inefficiency.TinyEfficiencyTest.test_cpu_vs_cpu_determinism -v` | **0** (1 test, ok) |
| `cd c && python3 -m unittest tests.test_inefficiency.TinyEfficiencyTest -v` | **0** (5 tests, ok) |
| `cd c && python3 -m unittest tests.test_resource_plan.PhysicalCpuCountTest -v` | **0** (6 tests, ok) |
| `cd c && python3 -m unittest tests.test_env_defaults -v` | **0** (8 tests, no windll warning) |
| `make -C c test-python` | **0** (350 tests, 28 skipped) |

No engine C edits. No new Python helper scripts. No git add/commit.
