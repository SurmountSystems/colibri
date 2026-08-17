# Recon: HF install cancel vs pause/resume

**Date:** 2026-08-11
**Repo:** `/home/hunter/Projects/surmount/colibri`
**Scope:** read-only inventory of install cancel, partial files, restart behavior, UI, and pause/resume feasibility.
**No code edits.**

---

## Summary

| Question | Answer (honest) |
|----------|-----------------|
| How install works | Feature `install` on `colibri-sys`; native UI spawns background job via `install_async` → `install_model_cancellable`. UI forces **hub path** (`prefer_cli: false`) for determinate counters. |
| Cancel hard or graceful? | **Mixed.** Hub: cooperative **between files** (current file finishes; no mid-HTTP cancel). CLI: **kill child** (hard abort). |
| Partial shards on disk | **Left in place.** No cleanup on cancel. Incomplete detector is heuristic only. |
| Restart same dest = resume? | **Not really on the UI hub path.** Per-file `download_file` + `local_dir` **re-downloads every file** (no exists-skip in that code path). Cache-mode resume exists in hf-hub but colibri bypasses it with `local_dir`. CLI may resume (not owned/tested in-tree). |
| UI | **Install** + **Cancel** only. Status strings + determinate strip. No Pause / Resume. |
| Graceful pause feasible? | **Yes as a product layer** on hub path: stop after current file + skip complete files on re-entry. **Not free from hf-hub.** Mid-file kill without waiting needs more work / different API. |

---

## 1. How install works today

### Feature and crates

| Piece | Location |
|-------|----------|
| Feature flag | `colibri-sys` `install` → deps `hf-hub` 1.x (`blocking`) + `indicatif` (`crates/colibri-sys/Cargo.toml`) |
| Native default | `colibri-native` `default = ["install"]` → `colibri-sys/install` |
| Core module | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/model/install.rs` |
| Re-export | `colibri_sys::install` when feature on (`lib.rs`) |
| Host glue | `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/host.rs` (`install_async`, `InstallEvent`, form validation) |
| GPUI UI | `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/main.rs` (Tools install form) |
| Progress math | `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/progress.rs` + host `progress_view_for_install` |
| Docs | `colibri-sys/docs/user-guide.md` §9; `colibri-native/docs/fidelity.md` “Model install (HF)” |

Web SPA (`web/`) has **no** HF install surface. Install is native-only.

### Public API (sys)

- `InstallSource::HuggingFace { repo_id, revision, allow_patterns }` or `LocalPath`
- `InstallOptions { dest, prefer_cli, min_free_bytes, inspect_after, register }`
- `InstallProgress { phase, message, bytes_done/total, file, files_done/total }`
- `InstallResult { dest, source, notes, model_info }`
- `InstallCancel` — `Arc<AtomicBool>`; `request` / `is_requested` / `clear`
- `install_model` / `install_model_cancellable` / `install_model_with` (injectable `HfCliRunner`)
- Constant `INSTALL_CANCELLED_MSG = "install cancelled"`

### Control flow (`install_model_with`)

1. `check_cancel`
2. `ensure_space(dest, min_free_bytes)` (0 = skip)
3. `create_dir_all(dest)`
4. HuggingFace branch:
   - Progress `phase: "download"`, message `fetching {repo_id}`
   - If `prefer_cli && cli.available()` → `HfCliRunner::download` (`hf download … --local-dir dest`, optional `--include`)
   - Else → `download_via_hf_hub` (below)
   - `detect_incomplete_download(dest)` → hard fail if issues
5. Optional `inspect` (`ModelInfo::inspect`) then optional registry `register`
6. Progress `phase: "done"`

### Hub path (`download_via_hf_hub`)

Concrete calls into **hf-hub 1.0.0** (registry source inspected):

1. `HFClientSync::new()`
2. `client.model(owner, name)`
3. `repo.list_tree().recursive(true).maybe_revision(...).send()` → file list + sizes
4. Filter with `filter_entries_with_sizes` / allow patterns
5. **For each file** (sequential):
   - `check_cancel(cancel)?`
   - emit progress (bytes_done of *completed* files only; current file name; files_done = index before this file)
   - `repo.download_file().filename(...).local_dir(dest).maybe_revision(...).send()`
6. Final progress with all files complete

**Important:** UI does **not** use prefer-cli. `install_options_for_ui` sets `prefer_cli: false` so the panel gets byte/file totals (CLI gives no structured counters). Comment in host: hub prefer for determinate progress.

```text
UI: start_install → install_async → install_options_for_ui(prefer_cli: false)
  → install_model_cancellable → download_via_hf_hub (unless tests force CLI)
```

### Cancel token + progress channel

```text
InstallCancel (cloneable AtomicBool)
  ├─ UI holds install_cancel; Cancel button → request()
  └─ background thread holds cancel_bg; install_model_* polls via check_cancel

mpsc::Sender<InstallEvent>
  Progress(InstallProgress)  // every hub file boundary / phase
  Done(InstallResult)
  Error(String)              // cancel normalized to INSTALL_CANCELLED_MSG
```

Poll loop: `schedule_install_poll` every ~80 ms → `drain_install`.

Tests for cancel: `pre_set_cancel_aborts_before_download`, `cancel_mid_download_via_mock_runner` in `install.rs`.

---

## 2. Cancel: hard abort vs graceful; partials on disk

### Semantics by path

| Path | On `InstallCancel::request` | Granularity |
|------|----------------------------|-------------|
| **Hub** (`download_via_hf_hub`) | Loop checks **between** files only. In-flight `download_file().send()` runs to completion (or network error). Then next `check_cancel` returns `Err(Install("install cancelled"))`. | Graceful **per file**, not mid-stream |
| **CLI** (`SystemHfCli::download`) | Poll loop every 50 ms; on cancel → `child.kill()` + `wait` | Hard process kill; mid-transfer shred |
| **Phases** after download | Still checked before incomplete detect, inspect, register | Cancel can stop post-download work |
| **LocalPath** | Checks only; no download | N/A |

Doc string on `InstallCancel::request`: *"CLI child is killed and hub path stops between files."* User-guide §9 matches.

### What is left on disk

- **No cleanup** on cancel or error. Dest dir and any completed shards remain.
- Hub non-xet `local_dir` write path (`stream_response_to_file_with_progress` in hf-hub): `File::create(dest)` then stream. Mid-kill (only if something killed the thread; normal cancel does not interrupt this) would leave a **truncated final name**, not necessarily `*.incomplete`.
- Hub **cache** mode (not used by colibri UI) uses `*.incomplete` then rename.
- Xet local_dir path downloads straight to final path (also not incomplete-marker in the same way as cache blobs).

`detect_incomplete_download` (post-download success gate only) flags:

- names ending `.incomplete`, `.tmp`, `.download`
- dirs named `.cache` or ending `.lock`
- **zero-length** `*.safetensors`

It does **not** detect a half-written shard of correct non-zero size. After CLI kill, a partial multi-GB `.safetensors` can sit under dest and **pass** incomplete detection if non-empty, then inspect may fail or worse accept bad weights. Residual honesty: incomplete detection is a partial safety net, not full integrity.

After cancel the install job returns error before incomplete check, so cancel does not run that detector; partials just sit.

---

## 3. Restart same dest: resume or from zero?

### UI hub path (product default)

`download_via_hf_hub` always:

1. Re-lists the tree
2. Calls `download_file` + **`local_dir(dest)` for every matched file**

In hf-hub 1.0.0, `download_file_to_local_dir`:

- Does **not** check “dest file already exists → skip”
- Always HEAD + GET (or xet) and writes the path again

hf-hub docs: **cache** is the content-addressed resume path (`HF_HUB_CACHE`, etag, skip existing blobs). Setting `.local_dir(...)` **bypasses** that cache. Snapshot-download helpers in the same crate *do* skip existing files under `local_dir` (`if dest.exists() && !force`), but colibri does **not** call snapshot download; it uses the per-file API without a skip.

**Conclusion for native Install button:** restarting install on the same dest is **re-download from zero** for hub files (re-transfer bandwidth; completed shards rewritten). Not a true resume of remaining shards only.

Optional improvements (not present):

- Before `download_file`, skip if local size matches hub size (or hash)
- Or download without `local_dir` into hub cache, then materialize/symlink into dest
- Or use snapshot API with exists-skip

### CLI path (`prefer_cli: true`)

`hf download --local-dir` is Hugging Face’s CLI. Industry expectation is resume/skip for already-present files, but **this repo does not unit-test resume** and the UI does not use CLI by default. Do not claim CLI resume as product behavior without a live check.

### Incomplete leftovers

If prior run left `*.incomplete` under dest, a **successful** re-run still fails at `detect_incomplete_download` until those markers are removed. Restart does not auto-scrub incomplete markers.

---

## 4. UI: cancel only; install status states

### Controls (Tools install panel)

| Control | i18n / label | Behavior |
|---------|--------------|----------|
| Primary button | `rail.installBtn` “Install model” / while busy `rail.installing` “Installing…” | `start_install` |
| Secondary | `rail.cancel` “Cancel” | `cancel_install` → `InstallCancel::request`; status **“Cancelling...”** (hardcoded English, not i18n) |
| Progress strip | determinate bar % / ETA | shown while `install_progress` is `Some` |
| Status line | `install_status` | freeform string |

No Pause, Resume, or “delete partials” control.

Wizard “Install a model” only navigates to the install form (`wizard_open_install`); same Cancel/Install UX.

### State fields (`main.rs` `App`)

| Field | Role |
|-------|------|
| `installing: bool` | Mutex for one job; blocks second start (“install already running”) |
| `install_cancel: Option<InstallCancel>` | live handle |
| `install_rx: Option<Receiver<InstallEvent>>` | progress drain |
| `install_started: Option<Instant>` | rate / ETA |
| `install_progress: Option<ProgressView>` | strip; cleared on error/cancel; kept at 100% on Done until next install |
| `install_status: SharedString` | human line |

### Status lifecycle (observed strings)

| When | `install_status` (approx) |
|------|---------------------------|
| Idle init | `"Ready to install"` |
| Validation / space fail | `"install invalid: …"` / `"install refused: …"` |
| Start | progress line + `repo -> dest` |
| Progress | `progress_view_for_install` line + optional ` · {file}` |
| User cancel click | `"Cancelling..."` (until Error event) |
| Cancelled / other fail | `"Install error: install cancelled"` or other error |
| Success | Done line + dest + notes; model path set; app status `"Install complete · model path set"` |

There is **no** enum of install states in product code; only `installing` bool + string status + optional progress view. Phases on the wire: `"download" | "inspect" | "register" | "done" | "register"` (local).

Cancel button is always painted; when not installing it no-ops with `"no install in progress"`.

---

## 5. Feasibility of graceful pause + resume

### What already matches “pause after current file”

Hub cancel is already **cooperative after the current file**. Naming it Pause instead of Cancel is mostly UX + not treating the job as a hard failure + keeping dest for continue.

Gap vs pause product:

1. **No mid-file interrupt.** Pause request during a large shard waits until that shard finishes (can be many minutes). UI can show “Pausing…” (indeterminate or sticky last %) while waiting; that maps cleanly to today’s flag + post-file stop.
2. **Cancel is terminal Error.** Resume needs either a distinct `Paused` event / status, or “cancel” that is expected and leaves `installing = false` with dest intact (already true on disk).
3. **Resume must not re-download completed files.** Today re-calling `install_async` on same repo/dest **re-downloads everything** on hub path. Pause without skip-complete is almost useless for multi-hundred-GB models.
4. **Partial current file.** If you ever hard-stop mid-file, you need incomplete cleanup or size/hash validation; today cancel does not mid-stop hub HTTP.
5. **CLI path.** Kill is not graceful pause; resume would depend on `hf` CLI. UI defaults hub, so pause design can target hub first.

### Minimal product design (feasible without inventing hf-hub APIs)

**Pause (graceful):**

1. UI: Pause → same `InstallCancel::request` (or rename handle to `stop_after_file`)
2. Status: `"Pausing..."` indeterminate spinner / phase floor until Error or custom `Paused` event
3. Sys: after current file, return a distinct error/result e.g. `install paused` (not only `"install cancelled"`) so UI can set `paused` not “error red”
4. Leave completed files on disk

**Resume:**

1. Same form fields (repo, revision, dest)
2. Sys hub loop: for each file, if local path exists and `metadata.len() == hub_size` (or stronger hash), skip download and count toward `bytes_done`
3. Re-enter `install_model_cancellable` / `install_async`
4. Optional: clear only mismatched / zero-length shards

**Not claimed as free from hf-hub:**

- True mid-HTTP cancel with Range resume of the same incomplete blob is **not** wired; colibri does not pass a cancel into `download_file`, and `local_dir` non-xet path does not use the cache incomplete+rename protocol.
- hf-hub progress events exist but colibri does not subscribe; mid-file bytes remain open residual (`open:hub-mid-file-byte-progress` in `.agents/RESIDUAL.md`).

### Effort sketch (for planners, not an implement commit)

| Slice | Work |
|-------|------|
| UX Pause + “Pausing...” + Resume button | native `main.rs` / i18n operational strings; state: idle / installing / pausing / paused / error |
| Distinct paused terminal event | `install.rs` + host `InstallEvent` |
| Skip-if-complete on hub resume | `download_via_hf_hub` before each `download_file` |
| Scrub incomplete markers | optional helper before resume |
| Mid-file cancel + byte resume | larger; need hf-hub cancel/stream or own HTTP |

---

## 6. File / function index (quick)

| Symbol | File |
|--------|------|
| `InstallCancel`, `check_cancel`, `install_model_cancellable`, `download_via_hf_hub`, `detect_incomplete_download`, `SystemHfCli` | `crates/colibri-sys/src/model/install.rs` |
| `install_async`, `install_options_for_ui`, `InstallEvent`, `progress_view_for_install`, `validate_install_form` | `crates/colibri-native/src/host.rs` |
| `start_install`, `cancel_install`, `drain_install`, install panel buttons | `crates/colibri-native/src/main.rs` |
| `install_progress` / views | `crates/colibri-native/src/progress.rs` |
| `rail.cancel`, `rail.installBtn`, `rail.installing` | `crates/colibri-native/src/i18n.rs` |
| hf-hub `download_file` / `local_dir` / cache skip | `~/.cargo/registry/.../hf-hub-1.0.0/src/repository/download.rs` |

---

## 7. Honesty checklist

- Cancel on **UI default path** is **between-file stop**, not kill of HTTP mid-shard.
- Cancel on **CLI path** is **process kill**.
- Partials are **not deleted**; incomplete detector is **heuristic**.
- **Re-install same dest over hub rewrites all files**; do not call that “resume.”
- **Pause/Resume is product work** (state + skip complete files + copy), not a hidden hf-hub switch already on.
- Web SPA is out of scope for this install UX.
- Live network resume behavior of `hf` CLI not verified in this recon.
