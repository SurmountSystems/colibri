# Implement report: Doctor-step buttons look dead

**Repo:** `/home/hunter/Projects/surmount/colibri`
**Date:** 2026-08-11
**Ask:** Run doctor / Quick check (and related) still do nothing on wizard Doctor step.

## Root cause (exact)

Handlers **were attached** (`on_mouse_up` → `run_deep_doctor` / `run_doctor`+`run_plan` / scan / install). Clicks were not a zero-size hitbox or missing id.

What made them *look* dead:

1. **Identical Health check body on re-click.** Path already empty / not-a-model → same `Overall: Needs model` text every time. No timestamp, no "Last run".
2. **Quick check finished as `"Plan finished"`.** Shallow doctor ran, then `run_plan` overwrote status. Rail showed `native · Plan finished` with static Needs model copy. Felt like doctor never ran.
3. **No paint of intermediate status.** `run_deep_doctor` set `"Running doctor..."` then immediately finished in the same event turn, so the rail never showed work in progress. Shallow `run_doctor` did not even set a running status.
4. **No single action map.** Button ids and handlers were duplicated inline; a disconnect would not fail a unit test.

Side effects (mkdir, `colibri.toml`, recovery scan) already lived in `run_doctor_with_recovery` / `run_doctor_checks`. The bug was feedback + status bury, not missing host work.

## Fix

### Pure wire-up (`wizard.rs`)

| Piece | Role |
|-------|------|
| `WIZARD_BTN_DOCTOR` / `_QUICK_CHECK` / `_SCAN` / `_INSTALL` | Same strings as GPUI `.id(...)` |
| `WizardReadinessAction` | RunDoctor, QuickCheck, ScanModels, InstallModel |
| `readiness_action_for_button_id` | Id → action (regression surface) |
| `readiness_running_status` / `readiness_done_status` | Immediate + done rail copy with clock |
| `stamp_doctor_last_run` / `readiness_click_outcome` | Health box always changes; Quick check no longer ends as bare Plan finished |

### UI / host (`main.rs`)

- Doctor-step buttons use **constants + `on_click`** → `handle_wizard_readiness_button(id)` → `dispatch_readiness_action`.
- Immediate status: `"Running doctor..."` / `"Quick check..."` / `"Scanning..."`.
- `cx.spawn` + 16ms yield so that status can paint before blocking host work.
- Host path unchanged in substance: recovery doctor, scan, plan, install jump.
- After host work: stamp `Last run: HH:MM:SS` on Health check; status e.g. `Doctor finished · Last run 15:04:05` or `Quick check finished · Last run …`.
- `min_h(28)` on buttons for a solid hit target.
- Tools `run_doctor` also sets `"Running doctor..."` before shallow work.

## Acceptance

| Action | Result |
|--------|--------|
| Run doctor | Status → Running doctor… then Doctor finished · Last run …; Health box updates (stamp + any mkdir/toml copy); empty path still scaffolds folder + colibri.toml |
| Quick check | Status → Quick check… then Quick check finished · Last run … (not Plan finished); doctor + plan refresh |
| Scan | Status → Scanning… then Scan finished · Last run …; registry + recovery doctor + plan |
| Install | Opens Model step install form (unchanged product path) |

## Tests (red → green contracts)

| Contract | Test |
|----------|------|
| Button ids map to actions | `wizard::readiness_button_ids_map_to_actions` |
| UI constants match map | `chrome_tests::wizard_doctor_button_ids_dispatch_via_action_map` |
| Stamp + status for Run doctor / Quick check | `wizard::readiness_click_outcome_updates_status_and_doctor` |
| Stamp replaces prior footer | `wizard::stamp_doctor_last_run_replaces_prior_footer` |
| Host mkdir + toml + stamp path | `chrome_tests::readiness_run_doctor_path_mutates_and_stamps` |
| Existing host mkdir/toml | `run_shallow_doctor_creates_missing_path`, `run_shallow_doctor_empty_dir_writes_colibri_toml` |

## Verify

```text
cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets -- -D warnings   # ok
cargo test -p colibri-native                                    # 208 passed
```

## Files

| Path | Change |
|------|--------|
| `crates/colibri-native/src/wizard.rs` | Action map, stamp, outcome helpers, unit tests |
| `crates/colibri-native/src/main.rs` | Dispatch, on_click wire-up, chrome tests |
| `.agents/reports/impl-doctor-buttons-dead.md` | This report |

## Operator check

1. Open Setup → Doctor step with a non-model path (e.g. empty store or missing folder).
2. Click **Run doctor**: rail shows Running doctor… then `Doctor finished · Last run HH:MM:SS`; Health check gains `Last run: …` (and Created folder / colibri.toml copy when path was empty).
3. Click **Quick check**: rail shows Quick check… then `Quick check finished · Last run …` (not Plan finished alone); Health check stamp updates again.
4. **Scan for models** / **Install a model** still scan or open install.
