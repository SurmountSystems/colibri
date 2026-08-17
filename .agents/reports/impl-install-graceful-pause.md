# Impl: graceful install Pause + waiting spinner + Resume

**Date:** 2026-08-11
**Repo:** `/home/hunter/Projects/surmount/colibri`
**Recon:** `.agents/reports/recon-install-pause-resume.md`

---

## What landed

### A. colibri-sys (`crates/colibri-sys/src/model/install.rs`)

1. **Skip complete files** (`local_file_is_complete`)
   - Before each hub `download_file`, if dest already has the file:
     - `expected_size > 0`: skip only when `metadata.len() == expected_size`
     - `expected_size == 0`: skip when file exists and is **non-empty**
   - Zero-length files never count as complete
   - Progress: `skip {name} (already complete)` and bytes advance
   - **Honesty:** size/name heuristic only; no content hash. A wrong file of the right size would still skip.

2. **Pause vs cancel**
   - `InstallStopKind::{Cancel, Pause}`
   - `InstallCancel::request()` → cancel (`INSTALL_CANCELLED_MSG`)
   - `InstallCancel::request_pause()` → pause (`INSTALL_PAUSED_MSG`)
   - Same cooperative between-file stop on hub; CLI still kills child
   - `check_cancel` returns distinct error strings so hosts can treat pause as non-red

3. **Tests (red→green)**
   - `local_file_is_complete_matches_size`
   - `local_file_is_complete_nested_and_zero_size_heuristic`
   - `request_pause_returns_paused_message`
   - `pause_mid_download_via_mock_runner`
   - Existing cancel tests still green

### B. colibri-native

1. **Pure state machine** `crates/colibri-native/src/install_ui.rs`
   - Phases: Idle → Installing → Pausing → Paused → (Resume) Installing
   - Cancel path: Installing/Pausing → Cancelling → Idle
   - Helpers: `pausing_status_line(tick)` (pulsing dots), `paused_status_line`, button visibility
   - Unit tests for full pause/resume and cancel flows

2. **Host**
   - `InstallEvent::Paused` when job ends with `INSTALL_PAUSED_MSG`
   - Cancel/errors remain `InstallEvent::Error`

3. **UI (Tools install row)**
   - While **Installing**: **Pause** next to **Cancel**
   - On Pause: phase **Pausing**, status `Pausing… Waiting for current file to finish` (dots pulse on 80ms poll), progress strip **frozen** at last %
   - When job stops for pause: **Paused** + Resume primary label; partials kept
   - Resume re-calls install with same form fields (hub skip-complete does the rest)
   - Cancel: **Cancelling…** then `Install cancelled` (not the same as paused)
   - Resume only offered on Pause path (not after cancel)

4. **Copy**
   - i18n: `rail.pause` / `rail.resume` / `rail.pausing` (en + it), marked native-only operational
   - Status lines plain English; no brand slogans

### C. Docs

- `colibri-sys/docs/user-guide.md` §9: pause message + skip-complete note
- `colibri-native/docs/fidelity.md`: Pause / Resume row

---

## Verify

```text
cargo fmt -p colibri-sys -p colibri-native
cargo clippy -p colibri-sys --features install --all-targets -- -D warnings   # ok
cargo clippy -p colibri-native --features install --all-targets -- -D warnings # ok
cargo test -p colibri-sys --features install --lib model::install              # 16 passed, 1 ignored
cargo test -p colibri-native --features install                                # 237 passed
```

---

## Honesty (operator-facing)

| Claim | Truth |
|-------|--------|
| Pause is graceful | Yes **after the current file** finishes. Mid-file HTTP is not aborted. |
| Spinner covers wait | Yes: frozen last % + pulsing “Pausing… Waiting for current file to finish” |
| Resume continues multi-shard | Yes on **hub path** via size match skip; UI uses hub (`prefer_cli: false`) |
| Cancel vs pause | Distinct status and events; cancel is not Resume-friendly in UI |
| Hash integrity | Not claimed; size heuristic only |

No git commit (operator owns VCS).
