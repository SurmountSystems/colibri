# Impl: install pause UI honesty + restart-safe Resume

**Date:** 2026-08-11
**Repo:** `/home/hunter/Projects/surmount/colibri`
**Scope:** pause status exclusivity + durable install checkpoint (native only)

---

## Root cause of dual messages

When the job reached **Paused** (or **Pausing**), the Tools panel painted **two** progress-related strings:

1. **Progress strip** always called `ProgressView::line()`, which kept the last active download label (`"Downloading..."`) plus percent and ETA.
2. **Form status line** was set to `paused_status_line()` / `pausing_status_line(...)`.

So the user could see e.g. `"Downloading... 16% · about 2 hours left"` and `"Paused. Resume to continue..."` at once. That is contradictory: one line claims an active download with ETA, the other claims paused.

No product rule hid the strip line by phase. Freezing the strip only stopped *updates*; it still showed the last active line.

---

## UI rules after fix

| Phase | Progress bar | Strip text under bar | Form status line |
|-------|--------------|----------------------|------------------|
| **Installing** | Fills with live % | Active `view.line()` (Downloading… / % / ETA) | Full line with file + byte pair |
| **Pausing** | Frozen last % | **Hidden** (`show_active_progress_line` false) | Exclusive: `Pausing. / .. / ... Waiting for current file to finish` |
| **Paused** | Frozen last % | **Hidden** | Exclusive: `Paused at N% · Resume to continue downloading remaining files.` (or without `at N%` if % unknown) |
| **Cancelling** | Frozen last % | **Hidden** | `Cancelling...` |
| **Idle** (Done strip) | 100% | `Done 100% · …` allowed | Done / ready copy |

Helpers in `install_ui.rs`:

- `show_active_progress_line(phase)` — true only for Installing / Idle
- `exclusive_status_for_phase(phase, percent, pause_tick)`
- `paused_status_line(percent: Option<u8>)` — never active `"Downloading..."`

`progress_strip_el(..., show_line)` paints bar-only when pause/cancel wait owns the prose.

---

## What is persisted + where

| Item | Value |
|------|--------|
| File | `install-checkpoint.toml` |
| Directory | Same config dir as `native-ui.toml` (XDG `~/.config/colibri/` or Windows LocalAppData `colibri/`) |
| Path helper | `install_ui::default_checkpoint_path()` |

Fields (`InstallCheckpoint`):

- `repo_id`, `revision`, `dest` (absolute when form validates)
- `min_free_gb` (form text as typed)
- `percent` (optional last trustworthy fill)

No secrets. Separate file from prefs so clear/save does not rewrite theme/locale.

### When written / cleared

| Event | Checkpoint |
|-------|------------|
| `InstallEvent::Paused` | **Save** (form + last %) |
| Install **Done** | **Clear** |
| **Cancel** (request or cancelled error) | **Clear** |
| Fresh **Start** from Idle (not Resume) | **Clear** (drop stale) |
| Non-cancel **Error** with checkpoint still on disk | **Keep**; UI re-enters **Paused** so Resume stays available |
| App **restart** with file present | Load → phase **Paused**, form prefilled, Resume primary |

Resume still uses hub **skip complete files** in `colibri-sys` (`local_file_is_complete`); checkpoint only restores UI form + phase across process death.

---

## Restart resume flow

1. User pauses → job stops between files → UI **Paused** + checkpoint written.
2. User quits app.
3. Next launch: `load_checkpoint_default()` → repo/revision/dest/min-free prefilled, `InstallUiPhase::Paused`, status `Paused at N%…`, progress bar at last %, chrome `"Install paused · Resume in Tools"`.
4. User clicks **Resume** → same install path as in-process resume; completed shards skipped on hub path.
5. Done clears checkpoint; Cancel abandons it.

---

## Tests (red → green)

Named contracts in `crates/colibri-native/src/install_ui.rs` (written as expectations, green after product):

| Test | Contract |
|------|----------|
| `show_active_progress_line_only_while_installing_or_idle` | Pausing/Paused/Cancelling hide active strip line |
| `paused_status_never_says_downloading` | Paused copy has no active `"Downloading..."`; includes `16%` when known |
| `exclusive_status_paused_vs_installing` | Installing → no exclusive line; Paused/Pausing exclusive and not active download |
| `active_download_line_detected` | Detector matches live download lines only |
| `checkpoint_round_trip_temp_dir` | save/load equality |
| `checkpoint_missing_is_none` | absent file |
| `checkpoint_corrupt_is_none` | bad TOML |
| `checkpoint_empty_repo_unusable` | empty repo not restored |
| `clear_checkpoint_removes_file` | clear + idempotent |
| `default_checkpoint_path_next_to_prefs_dir` | sibling of prefs dir |

Prior pause SM tests still green (`install_to_pause_to_resume`, etc.).

### Evidence

```text
cargo fmt -p colibri-native
cargo clippy -p colibri-native --features install --all-targets -- -D warnings   # clean
cargo test -p colibri-native --features install                                  # 285 passed
# install_ui filter: 18 passed (SM + exclusive + checkpoint)
```

---

## Files touched

- `crates/colibri-native/src/install_ui.rs` — exclusive status + checkpoint I/O + tests
- `crates/colibri-native/src/main.rs` — strip `show_line`, pause/paused/cancel wire, restore on startup, persist/clear

Did not touch ROCm/UMA (`probe.rs` / `plan.rs`) or `colibri-sys` this slice (skip-complete already landed).

---

## Residual

- Mid-file HTTP is still not aborted on Pause (cooperative after current file); Pausing wait copy covers that.
- No explicit “Abandon paused install” control while Idle/Paused without starting; Cancel only while busy. User can Resume then Cancel, or Start a different install after moving to Idle via cancel path.
- Checkpoint does not store allow-patterns / partial file inventory; hub re-list + size skip handles remaining work.
- No git commit (operator-owned).
