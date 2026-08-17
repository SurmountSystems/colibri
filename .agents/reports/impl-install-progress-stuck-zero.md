# Install UI stuck at 0% with absurd ETA

**Date:** 2026-08-11
**Status:** done
**Board:** `bug:install-progress-stuck-zero`

## Problem (operator)

Wizard download for `mastouri/GLM-5.2-colibri-int4-g64-with-int8-mtp` (~372 GB):

- Status: Installing... (cyan), Pause/Cancel available
- Progress bar empty; labels `Downloading... 0% · about 1445 hours left · out-00000.safetensors`
- Host network ~52 MiB/s (bytes actually moving)
- Footer stayed `native · Ready to install GLM-5.2` while install was running

## Root cause

1. **File-boundary only progress.** `download_via_hf_hub` emitted `InstallProgress` at file start/skip/end only. During a multi-GB first shard (`out-00000.safetensors`), `bytes_done` stayed at completed prior files (often 0) for a long time → percent 0, empty bar.

2. **hf-hub ProgressHandler unused.** hf-hub 1.x already streams per-chunk and aggregate download events via `.progress(...)`. We never wired it.

3. **ETA from junk when done is 0.** With a positive `rate_per_sec` and huge remaining, UI could show multi-day hour counts. Zero completed bytes should never invent an ETA. Estimates above 7 days are suppressed.

4. **Stale chrome status.** Catalog select set `self.status` to `Ready to install …` and install only updated `install_status`, so the footer stayed stale.

## Fix

### colibri-sys (`model/install.rs`)

| Piece | Role |
|-------|------|
| `aggregate_download_bytes` / `download_progress_event` | Pure multi-shard + mid-file composition |
| `InstallLiveProgress` | Shared atomics + snapshot for UI polls during blocked hub download |
| `HubDownloadProgress` | Implements `hf_hub::progress::ProgressHandler`; updates live bytes on each tick (throttled full publish ~250ms) |
| `download_via_hf_hub` | Passes `.progress(handler)` on every `download_file` |
| `install_model_cancellable_live` / `install_model_with(..., live)` | Optional live handle for hosts |

### colibri-native

| Piece | Role |
|-------|------|
| `progress.rs` | ETA hard rules: no ETA when `done == 0`; hide ETA > 7 days; days phrasing instead of “1445 hours” |
| `install_progress_with_partial` | Pure multi-file + partial tests |
| `host::install_async` | Returns `(InstallCancel, Arc<InstallLiveProgress>)` |
| `host::format_install_bytes_pair` | `12.5/372.0 GiB` style counters on status line |
| `main.rs` drain | Polls `install_live.snapshot()` every 80ms while Installing so bar moves mid-file; updates footer `Installing · file · N%` |

## TDD (red → green)

Red contracts first (then implementation made them pass):

| Test | Contract |
|------|----------|
| `install_zero_done_no_eta_even_with_positive_rate` | 0 bytes done → no multi-day ETA / “Calculating...” |
| `install_zero_total_bytes_no_div_by_zero` | total 0 safe |
| `install_partial_file_advances_percent` | prior + partial → real % |
| `install_multi_file_completed_only_file_boundary` | boundary math still works |
| `install_absurd_eta_hidden` | tiny rate / huge remaining → None |
| `eta_tiny_rate_above_max_is_none` | > 7 days → None |
| `format_eta_days_not_absurd_hour_count` | days phrasing, not huge hours |
| `aggregate_download_bytes_*` (sys + native) | sum / saturate |
| `download_progress_event_mid_file_partial` | mid-file event not stuck at 0% |
| `live_progress_publish_and_snapshot` | atomics round-trip |
| `progress_view_zero_done_no_absurd_eta` (host) | view line contract |
| `progress_view_mid_file_partial_advances` | 50/500 MiB → 10% |

## Verify

```text
cargo fmt -p colibri-sys -p colibri-native
cargo clippy -p colibri-sys -p colibri-native --all-targets -- -D warnings  # clean
cargo test -p colibri-sys --lib --features install   # 147 passed, 1 ignored
cargo test -p colibri-native --bin colibri-native    # 268 passed
```

## Files touched

- `crates/colibri-sys/src/model/install.rs`
- `crates/colibri-native/src/progress.rs`
- `crates/colibri-native/src/host.rs`
- `crates/colibri-native/src/main.rs`

## Not changed

- Prefer-cli path still coarse (no free byte stream from `hf` CLI)
- Marketing / i18n product copy (operational status only)
- No git commit (operator-owned)

## Expected UX after rebuild

- Bar and percent advance during large shards
- Status shows file name + `done/total GiB` while % is still low
- ETA “Calculating...” until some bytes completed; then sane hours/days, never multi-week garbage from 0 done
- Footer: `Installing · out-00000.safetensors · N%` (not stale Ready to install)
