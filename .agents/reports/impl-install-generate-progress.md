# Report: `impl:install-generate-progress`

Date: 2026-08-11
Scope: `colibri-native` install + generate determinate progress UI; `colibri-sys` hub download byte/file fill

## Goal

Show a determinate progress strip (thick fill + percent + ETA) during **generate** and **install**, using existing pure math in `progress.rs`. Plain English labels. Hide when idle. Improve sys hub progress to report bytes when list_tree sizes are available.

## Done

### 1. Generate (native)

- Track `gen_max_tokens` from controls and `gen_progress: Option<ProgressView>`.
- On send: start strip at 0% with label **Generating...** (rate unknown → Calculating...).
- On each token: `progress_view_for_generate(live_tokens, max, tok/s)`.
- On Done: force 100% via `progress_view_generate_done()`, then clear strip when idle.
- Status line mirrors the progress line while streaming; footer keeps `status_after_gen_done` on completion.
- UI: thick fill row under the chat composer (`#generate-progress`), only while `gen_progress` is set.

### 2. Install (native)

- Track `install_started`, `install_progress: Option<ProgressView>`.
- On progress events: `progress_view_for_install` (phase → plain English, rate from elapsed wall time).
- Labels: Downloading..., Checking files..., Registering..., Done, Working...
- Status text is the progress line (+ current file name when present), not raw `[phase] message` codes alone.
- UI: same strip above install status (`#install-progress`) while installing; cleared when idle.
- Done → 100% strip briefly, then hide; status keeps the completion summary.

### 3. Host helpers (`host.rs`)

| Helper | Role |
|--------|------|
| `progress_rate` | done / elapsed_secs |
| `install_phase_label` | plain English phase |
| `install_rate_from_progress` | bytes/s preferred, else files/s |
| `progress_view_for_install` | wires InstallProgress → ProgressView; done → 100% |
| `progress_view_for_generate` / `progress_view_generate_done` | generate strip |

Unit tests cover rate, generate midway/done, install bytes/files/done, phase labels.

### 4. Sys install (`model/install.rs`)

- `download_via_hf_hub` now keeps Hub **file sizes** from `RepoTreeEntry::File`.
- Progress events fill `bytes_done` / `bytes_total` when total size &gt; 0, plus existing `files_done` / `files_total`.
- `filter_entries_with_sizes` + unit test for allow-pattern + byte sum.
- Prefer-cli path still coarse (phase/message only); no free byte stream from `hf` CLI without parsing.

### 5. `progress.rs`

- Removed “wire later / dead_code” note; module is live from host + UI.
- Existing 34 math tests unchanged and green.

## Verify

```text
cargo fmt -p colibri-native -p colibri-sys
cargo clippy -p colibri-native --all-targets -- -D warnings   # ok
cargo clippy -p colibri-sys --all-targets --features install -- -D warnings  # ok
cargo test -p colibri-sys --lib --features install            # 105 passed, 1 ignored
cargo test -p colibri-native --bin colibri-native progress    # 39 passed (math + host progress_*)
cargo test -p colibri-native --bin colibri-native host::tests # 67 passed
```

## Files touched

| Path | Change |
|------|--------|
| `crates/colibri-native/src/progress.rs` | Live module docs; drop dead_code allow |
| `crates/colibri-native/src/host.rs` | Progress helpers + tests |
| `crates/colibri-native/src/main.rs` | State, drain wiring, `progress_strip_el`, composer + install UI |
| `crates/colibri-sys/src/model/install.rs` | Hub path byte progress; filter helper + test |

## Not in this slice

- Full Setup wizard
- Prefer-cli live byte parsing
- Per-chunk mid-file byte callbacks (file-granularity only on hub path)

No git commit (operator-owned).
