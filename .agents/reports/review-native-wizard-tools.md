# Review (R2 of 3): colibri-native Tools + Setup wizard

**Repo:** `/home/hunter/Projects/surmount/colibri`
**Scope:** `wizard.rs`, `main.rs` Tools tab + slim rail, first-run gate, i18n wizard/tools keys, report `impl-tools-and-wizard.md`
**Mode:** read-only
**Date:** 2026-08-11
**Reviewer:** 2 of 3 (effort 3)

## Verdict

**Not ready as shipped.** The step machine, first-run gate, Tools tab, and EN/IT keys are real and mostly wired, but step 4 does not deliver plain readiness, Skip can look done while disk is stale, the rail is still dense, and re-open Setup is fragile (Setup buried in scroll + shared `model_input` mounted in two parents).

Impl claim of 153 green tests is not re-run here; unit coverage is thin for the main.rs wire path (in-memory wizard only).

---

## Findings (severity + file:line)

### 1. Step 4 readiness present as a step, product value largely missing

| Sev | Where | Issue |
|-----|--------|--------|
| **High** | `host.rs:339-367` (`format_plan`) | Wizard step 4 body promises a "plain memory plan" (`i18n.rs:285-287`) but the plan panel dumps lab fields: `version=`, `policy=`, `hit=`, `bottleneck=`, `expert_count=`, `dense_bytes=`, `ssd_probe_state=`, raw `warn:` / decision lines. That is the opposite of the plan’s plain-language pass/fail. |
| **High** | `main.rs:3025-3085` + `host.rs:318-324` | Empty/missing model still shows planner jargon (`plan: set a model path first (COLIBRI_MODEL / COLI_MODEL …)`). Step 4 is not a readiness gate with a clear yes/no. |
| **Med** | `main.rs:480-490`, `3038-3043` | Auto-refresh and "Run checks again" only run **shallow** doctor. Plan called for quick **+ optional deep** on step 4; deep exists only under Tools (`main.rs:2595-2609`). |
| **Low** | `wizard.rs:11-28`, `main.rs:3025` | The **step itself is not missing** from the enum or UI match. Advance path Model → Readiness is correct. The gap is content quality, not a dropped arm. |

**Not a missing enum step** — readiness is step 4 of 6 and renders doctor + plan bodies. The product gap is plain readiness, not `WizardStep::Readiness` absence.

---

### 2. Skip / Finish and `first_run_done` save (partially wrong)

| Sev | Where | Issue |
|-----|--------|--------|
| **High** | `main.rs:499-511`, `514-526` | After `persist_prefs_status`, status is **always** overwritten with hard-coded English (`"Setup skipped · …"` / `"Setup complete"`). Save failures set `"Could not save settings: …"` in `persist_prefs_status` (`main.rs:402-407`) then get wiped. User sees Skip success while **disk may still have `first_run_done = false`** → next launch re-traps in first-run wizard. Matches "skip not saving first_run" as an observable trap. |
| **Med** | `main.rs:499-511` vs `wizard.rs:161-165` | Skip builds a **local** `NativePrefs`, mutates it in `complete_wizard`, then **discards** it and saves from `self` via a second snapshot. Works only because `self.first_run_done = true` is set by hand. Easy to regress; tests never exercise this path. |
| **Med** | `wizard.rs:244-263` | Unit tests assert in-memory `first_run_done` only. **No** temp-dir test that Skip/Finish through the **main** persist path writes `first_run_done = true` to `native-ui.toml`. Impl report overclaims "skip / finish → first_run_done" as fully covered. |
| **Low** | Happy path | When `save()` succeeds, in-memory + disk should both be true. Logic is not unconditionally broken; error masking is the real bug. |

---

### 3. Jargon still in user-facing UI

Primary labels in wizard/tools i18n are improved. Residual jargon still hits first-run and daily shell:

| Sev | file:line | Copy / surface |
|-----|-----------|----------------|
| **High** | `host.rs:339-367` | Step 4 plan body (see finding 1). |
| **High** | `main.rs:684-685` | Status after engine start: `"Brain / PROF use embed visual poll (GLM)."` — decoder-ring, not product English. |
| **Med** | `host.rs:299-306`, `318-324` | Scan empty + plan empty mention `COLIBRI_MODEL` / `COLIBRI_MODEL_STORE` / `COLI_MODEL` as primary recovery text. |
| **Med** | `i18n.rs:149-153` | Topbar: `ACTIVE MODEL`, `TTFT {{n}} ms`, `tok/s` (power-user fine on badges; still not plain for cold start). |
| **Med** | `i18n.rs:187`, `205` | `Expert Cortex`, `Expert matmul` in Brain/Profiling (in scope of "no jargon in primary labels"). |
| **Med** | `main.rs:210` | Grammar placeholder `"GBNF (optional)"` on Tools advanced. |
| **Low** | `i18n.rs:132`, `343` | `Where experts live` / IT `Dove vivono gli expert` — still MoE lab framing on the tier strip. |
| **Low** | `i18n.rs:250` | Theme label `DOGE` is intentional brand (OK); keep. |

Wizard/tools **keys** themselves (`wizard.*`, `tools.*`, `setup.*`) are largely plain EN/IT. Failure is **bodies fed into** those panels (plan/doctor/status), not missing i18n keys.

---

### 4. Rail still cluttered

Plan target: brand · model **summary** · Start/Stop · live tiers when running · temp + max tokens · status · Setup.

| Sev | file:line | Issue |
|-----|-----------|--------|
| **Med** | `main.rs:2256-2318` | Still a full **editable** `model_input` entity, summary line, **and** Start/Stop in one card — denser than "model summary". Path editing also lives on Tools (`main.rs:2634`). |
| **Med** | `main.rs:2331` + `1988-2020` | Always-on Chat settings (temperature + max tokens) keep the rail tall even after doctor/install moved to Tools. Plan allowed this, but combined with full path field it still feels pre-slim. |
| **Med** | `theme.rs:281` | `RAIL_WIDTH = 292.0` unchanged from pre-slim recon. |
| **Low** | `main.rs:2320-2330` | Live placement + `live_hwinfo_text` when engine up is intended; copy is mostly plain via host formatters. |
| **Good** | Tools vs rail | Machine details, doctor, plan/scan, install, theme, language, About, advanced chat moved off the rail into Tools (`main.rs:2506+`). That part of the IA is real. |

---

### 5. Re-open Setup broken / fragile

| Sev | Where | Issue |
|-----|--------|--------|
| **High** | `main.rs:2274`, `2634`, `2943` | Same `Entity<TextInput>` for `model_input` is **parented in more than one place** in one frame: always rail + Tools when Tools tab is active; rail + wizard on Model step. Shared entity double-mount is a classic GPUI failure mode (input missing, wrong tree, or re-open path into Model step broken). Re-open Setup that lands users on step 3 is the sharp edge. |
| **Med** | `main.rs:2332-2358` + `2213` | Setup sits under `mt_auto` inside an `overflow_scroll` rail. When content exceeds viewport (path + inference + live strips), Setup is **below the fold** — users must scroll the rail to re-open. Feels "Setup broken" when the button is not visible. |
| **Med** | `main.rs:468-471` | `open_setup_wizard` always resets to Welcome (good). No guard against re-entry while open; re-clicking Setup mid-wizard only resets step (acceptable). No exclusive modal; rail remains interactive under full-main wizard — can Start engine while wizard is open (confusing). |
| **Low** | `main.rs:3240-3253` | Hero Setup CTA is fine when chat empty and wizard closed. Not the failure path. |

---

## What looks solid

- `WizardStep` linear machine, 6 steps, title/body keys (`wizard.rs`).
- First-run open: `prefs.should_show_wizard()` → `WizardState::open_at_start()` (`main.rs:303-304` area).
- `complete_wizard` sets `first_run_done` and closes (`wizard.rs:161-165`).
- Tools tab + `nav.tools` EN/IT; theme picker; locale cycle; rail Setup id `btn-setup`.
- Shallow doctor checklist formatting is much plainer than raw CLI (`host.rs:224-258`).
- Theme switch temp-dir test in wizard module is a real contract (`wizard.rs:266-286`).

---

## Suggested tests (red-first)

1. **`skip_persist_writes_first_run_done_to_temp_toml`**
   Drive the same sequence as `wizard_skip` should: `first_run_done=false` → `complete_wizard` + `shell_prefs_snapshot` + `save_to_path` → reload → `first_run_done == true`. Assert disk, not only memory.

2. **`finish_persist_writes_first_run_done_to_temp_toml`**
   Same after walking to Ready.

3. **`persist_status_must_not_mask_save_error`** (behavior contract)
   Document that Skip/Finish status must retain save-error text when `save` fails (today broken at `main.rs:510` / `525`). Integration or extracted helper: `complete_and_persist(...) -> Result` must surface `Err`.

4. **`readiness_plan_copy_is_plain_english`**
   Fixture placement plan → formatter asserts: no `ssd_probe_state=`, no bare `dense_bytes=`, includes human labels ("Memory on GPU", "System RAM", pass/fail or clear bottleneck sentence). Red against current `format_plan`.

5. **`wizard_next_from_model_refreshes_doctor_and_plan`**
   Pure or host-level: after Model→Next, doctor/plan strings change for a given path (or at least plan not empty-idle when path set).

6. **`model_input_single_parent_per_frame`** (layout contract / smoke)
   Document invariant: at most one of {rail path editor, tools path editor, wizard path editor} mounts `model_input` per frame. Prefer separate display summary on rail; edit only on Tools/wizard. Test via small pure "where is path editor active?" helper if extractable.

7. **`should_show_wizard_false_after_skip_reload`**
   Prefs round-trip already partially covered; add explicit Skip story in prefs/wizard tests.

8. **Jargon guard (i18n / status)**
   Grep-style unit test: product status strings used after start_engine must not contain `PROF`, `EMAP`, `HWINFO`, `embed visual poll`. Red on `main.rs:684`.

9. **Step 4 deep optional** (if product still wants it)
   Wizard readiness exposes deep check or documents Tools-only; test for button id `wizard-btn-doctor-deep` if added.

---

## Priority order for fix implementer

1. Plain `format_plan` (and empty-plan copy) for step 4 + Tools plan panel.
2. Single-parent `model_input` (rail summary vs Tools/wizard editor).
3. Skip/Finish: save first, never overwrite save-error status; prefer saving the `complete_wizard` prefs snapshot.
4. Pin Setup (sticky footer outside scroll) or shorten rail so Setup stays visible.
5. Scrub start-engine status and remaining primary jargon.
6. Optional deep doctor on readiness; sticky first-run save tests.

---

## Files reviewed

| Path | Role |
|------|------|
| `crates/colibri-native/src/wizard.rs` | Step machine, complete, tests |
| `crates/colibri-native/src/main.rs` | Tools, rail, wizard UI, first-run, skip/finish |
| `crates/colibri-native/src/i18n.rs` | wizard/tools/setup/theme keys EN+IT |
| `crates/colibri-native/src/prefs.rs` | first_run_done, should_show_wizard, save |
| `crates/colibri-native/src/host.rs` | doctor/plan formatters (step 4 content) |
| `crates/colibri-native/src/theme.rs` | RAIL_WIDTH, ThemeId::to_pref |
| `.agents/reports/impl-tools-and-wizard.md` | Impl claims |
| `.agents/plans/plan-native-wizard-tools-theme.md` | Acceptance shape |

No product edits in this review.
