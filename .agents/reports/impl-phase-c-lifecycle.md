# Implement: Phase C model lifecycle (registry + install cancel + min free)

**Date:** 2026-08-10
**Scope:** `colibri-sys` install/registry + `colibri-native` GPUI
**Residual closed:** `open:model-registry-ui`, `open:install-cancel`, `open:install-min-free-gate`

---

## Summary

Production-parity Phase C for the local embed journey:

1. **Registry picker** – scan model store via `ModelRegistry`; list entries; click sets model path.
2. **Install cancel** – cooperative `InstallCancel` kills the `hf` CLI child and stops the hf-hub loop between files; native Cancel button during install.
3. **Min free space gate** – default 1 GB (decimal `GB`); UI field; refuse install with a clear message when free space is below the threshold.

`prefer_cli` install path is preserved (still the default).

---

## colibri-sys

### Install cancel (`model/install.rs`)

| API | Role |
|-----|------|
| `InstallCancel` | `Arc<AtomicBool>` handle; `request` / `is_requested` / `clear` |
| `INSTALL_CANCELLED_MSG` | Stable error substring (`"install cancelled"`) |
| `install_model_cancellable` | Public wrapper with cancel handle |
| `install_model_with(..., cancel: Option<&InstallCancel>, ...)` | Injected cancel for tests/hosts |
| `HfCliRunner::download(..., cancel)` | Extra cancel param; mockable |

**CLI path (`SystemHfCli`):** `spawn` + poll `try_wait` every 50ms; on cancel, `kill` + `wait`, return cancelled.

**Hub path (`download_via_hf_hub`):** `check_cancel` before list, between files, and after loop. In-flight single `download_file` is not interruptible mid-HTTP (cooperative between files only).

**Space gate:** existing `ensure_space` unchanged; still runs at start of `install_model_with`.

### Registry (`model/registry.rs`)

Existing `ModelRegistry::open` / `refresh` / `entries` / `find` unchanged. Extra unit tests for multi-root scan and refresh clearing stale dirs.

### Tests (install feature)

| Test | Contract |
|------|----------|
| `pre_set_cancel_aborts_before_download` | Cancel before job → no mock CLI call |
| `cancel_mid_download_via_mock_runner` | Blocking mock + concurrent `request` → cancelled |
| `ensure_space_refuses_when_below_threshold` | Absurd min free → insufficient disk space |
| `ensure_space_skips_when_zero` | 0 min free always ok |
| `registry_scans_multiple_temp_dirs` | Two roots, two models |
| `registry_refresh_clears_stale_entries` | Delete model dir → empty after refresh |

Existing prefer-cli multishard mock and incomplete-download tests still green with the new `cancel` param (`None`).

---

## colibri-native

### Host helpers (`host.rs`)

| Helper | Role |
|--------|------|
| `registry_scan_roots` | Model store (+ optional extras), deduped |
| `scan_model_registry` | `ModelRegistry::refresh` → `Vec<ModelEntry>` |
| `format_registry_entry` | One-line picker label |
| `DEFAULT_INSTALL_MIN_FREE_BYTES` | `GB` (1e9) |
| `parse_min_free_gb` | Empty → default; `0` → off; else GB → bytes |
| `check_install_free_space` | UI hard gate message |
| `format_install_space_with_min` | Free + min line for panel |
| `install_async` → returns `InstallCancel` | Uses `install_model_cancellable` |

### UI (`main.rs`)

- **Scan models** button on Plan / model panel; status line; clickable registry rows set model path.
- Install form: **min free GB** field (default `1`); space line shows free + min.
- **Cancel** button next to Install; requests cancel on the live handle.
- Start install refuses when free &lt; min (status `install refused: not enough free space: ...`) before spawning the background job; sys still enforces `min_free_bytes` as well.

---

## Verify

```text
cargo fmt -p colibri-sys -p colibri-native
cargo clippy -p colibri-sys --features install --all-targets -- -D warnings   # ok
cargo clippy -p colibri-native --all-targets -- -D warnings                 # ok
cargo test -p colibri-sys --features install --lib                          # 94 passed, 1 ignored (live HF)
cargo test -p colibri-native                                                # 31 passed
```

---

## Files touched

| Path | Change |
|------|--------|
| `crates/colibri-sys/src/model/install.rs` | Cancel API, CLI kill loop, hub checks, tests |
| `crates/colibri-sys/src/model/registry.rs` | Multi-root / refresh tests |
| `crates/colibri-native/src/host.rs` | Registry + min free + cancellable install_async |
| `crates/colibri-native/src/main.rs` | Picker UI, Cancel, min free field |
| `crates/colibri-native/docs/fidelity.md` | Install + registry rows → done |
| `.agents/RESIDUAL.md` | Close three Phase C open ids |

---

## Honesty / limits

- Hub cancel is **between files**, not mid-single-file HTTP.
- CLI cancel races: kill may surface as non-zero status; cancelled message preferred when flag is set.
- Default min free is 1 GB so tiny metadata-only pulls need operator to set **0** in the field.
- Registry scan is immediate children with `config.json` under store roots (existing sys contract).
- No git commit (operator-owned).
