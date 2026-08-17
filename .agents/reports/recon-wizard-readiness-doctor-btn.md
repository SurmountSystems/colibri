# Recon: wizard step 4 "Check readiness" + Tools doctor button

Evidence-only. Workspace: `/home/hunter/Projects/surmount/colibri`.
No product edits in this recon.

## 1. Wizard step 4 (Readiness) path

### Step machine (`wizard.rs`)

| Product step | Enum | Index | Title key |
|--------------|------|-------|-----------|
| 1 Welcome | `WizardStep::Welcome` | 0 | `wizard.welcome.title` |
| 2 Your machine | `Machine` | 1 | `wizard.machine.title` |
| 3 Choose a model | `Model` | 2 | `wizard.model.title` |
| **4 Check readiness** | **`Readiness`** | **3** | **`wizard.readiness.title`** |
| 5 Look and feel | `LookAndFeel` | 4 | `wizard.look.title` |
| 6 Ready | `Ready` | 5 | `wizard.ready.title` |

Source: `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/wizard.rs` (`WizardStep::ALL`, `title_key` / `body_key`).

Copy (EN, `i18n.rs`):

- Title: **"Check readiness"**
- Body: quick health check + plain memory plan; fix anything red before start engine
- Labels: `wizard.readiness.doctor` = "Health check", `wizard.readiness.plan` = "Memory plan", `wizard.readiness.refresh` = "Run checks again"

### Render path (`main.rs`)

1. App holds `wizard: WizardState` on `DesktopApp`.
2. When wizard is open, shell renders `wizard_view` (around `wizard_view` ~2830).
3. Shared chrome: step label (`wizard.stepOf`), title, body, then step-specific `content`, then nav (Back / Skip / Next|Finish).
4. **Readiness arm** is the `WizardStep::Readiness => { ... }` match (~3050–3108).

Pure state lives in `wizard.rs`; **all UI + doctor/plan side effects** live in `main.rs` handlers and `wizard_view`.

---

## 2. How health check + memory plan run and show

### Shared state

- `doctor_text: SharedString` — checklist from host doctor formatters
- `plan_text` — placement / memory plan prose
- Both are **the same fields** Tools tab uses (one buffer each for the whole app)

### Host API (`host.rs`)

| Function | Role |
|----------|------|
| `run_shallow_doctor(path, machine)` | Quick doctor → `run_doctor_checks(..., deep: false)` |
| `run_deep_doctor(path, machine)` | Thorough → `deep: true` (tensor headers, shards, index, optional mirror) |
| `run_plan(path, machine)` | `PlacementPlan::build` → plain readiness text |
| `format_idle_doctor_checklist()` | Empty path: **Overall: Idle**, no sys doctor / no cwd `.` probe |
| `format_doctor_checklist(report)` | Overall Pass/Warn/Fail + model + depth + `[pass\|warn\|fail\|skip]` lines |

App methods wrapping host:

- `DesktopApp::run_doctor` → shallow + status `"Checks finished"`
- `DesktopApp::run_deep_doctor` → deep + `"Running thorough checks…"` then `"Thorough checks finished"`
- `DesktopApp::run_plan` → plan + `"Plan finished"`

### When readiness content is filled

| Trigger | What runs | Notes |
|---------|-----------|--------|
| Bootstrap panels | shallow doctor + plan (if path set) | App start; not wizard-only |
| **Leaving Model → entering Readiness** (`wizard_next` when current step is `Model`) | shallow doctor + plan | **Auto-refresh** before `advance()` to Readiness |
| **"Run checks again"** on Readiness | `run_doctor` + `run_plan` | Shallow only; no deep |
| Tools "Run checks" / "Thorough check" / "Plan memory" | same methods | Shared `doctor_text` / `plan_text` |

Critical leave-Model hook (`main.rs` ~483–492):

```text
if self.wizard.step == WizardStep::Model {
    path = model_path
    doctor_text = run_shallow_doctor(...)
    plan_text = empty-path message OR run_plan(...)
}
// then advance to Readiness (or finish if last — not this step)
```

### Readiness UI layout (today)

1. Single primary button: **`wizard-btn-readiness-refresh`** — label **"Run checks again"**
   - Click: `run_doctor(cx); run_plan(cx);` (shallow + plan together)
2. Section **"Health check"** + scroll body `wizard-doctor-body` (`doctor_text`, max height 160)
3. Section **"Memory plan"** + scroll body `wizard-plan-body` (`plan_text`, max height 160)

**No deep doctor control on the wizard step today.** No separate "Doctor" label on the refresh button; product name for the checklist section is "Health check".

---

## 3. Tools tab: Doctor already exists (reuse)

### Panel chrome

- Tools panel title key: `tools.doctor` → **"Check model"**
- Actions row next to the doctor panel body:

| Element id | Label key | Label (EN) | Handler |
|------------|-----------|------------|---------|
| `tools-btn-doctor` | `rail.runChecks` | **Run checks** | `run_doctor` (shallow) |
| `tools-btn-doctor-deep` | `rail.deepCheck` | **Thorough check** | `run_deep_doctor` |

Styling: shallow = primary fill; deep = `primary_wash` + body text color. Both `text_xs`, chip-sized padding.

Plan is a **separate** Tools card (`tools.plan` / "Memory plan") with:

- `tools-btn-plan` → `rail.planBtn` ("Plan memory") → `run_plan`
- Plus scan registry, model path editor (when Tools owns the input site)

Source: `main.rs` Tools doctor panel ~2586–2622; plan row ~2671–2689.

### Reuse assessment

| Asset | Reusable as-is? |
|-------|-----------------|
| `run_doctor` / `run_deep_doctor` / `run_plan` | **Yes** — already used by wizard refresh and leave-Model |
| `doctor_text` buffer + checklist format | **Yes** — wizard body already binds it |
| i18n `rail.runChecks`, `rail.deepCheck` | **Yes** for parity labels; or add `wizard.readiness.*` keys if wizard wants different wording |
| Tools button **markup** | **Pattern** only (duplicate small `div().id(...).on_mouse_up(...)`); no shared GPUI widget extracted today |
| Deep doctor | **Already implemented** for Tools; wizard only needs a second button wired the same way |

No separate "Doctor" rail entry for wizard; rail has `rail.doctor` = "Check model" as lifecycle/tools naming, not a wizard control.

---

## 4. Minimal design: Doctor control on readiness

### Gap

Readiness already runs **shallow** doctor + plan on enter and on "Run checks again". Operators who need **thorough** validation (or a clearer "Doctor" affordance next to refresh) must leave the wizard and open **Tools**.

### Minimal product shape (recommended)

Keep one row of actions on Readiness, primary-first:

```text
[ Run checks again ]  [ Thorough check ]   // or [ Doctor ] if renaming
```

| Control | Behavior | Reuse |
|---------|----------|--------|
| **Run checks again** (keep) | `run_doctor` + `run_plan` | existing `wizard-btn-readiness-refresh` |
| **Thorough check** (add) | `run_deep_doctor` only (updates `doctor_text`; leave plan as-is or also re-run plan if you want one-shot consistency) | same as `tools-btn-doctor-deep` |

Alternative naming if product wants the word "Doctor":

- Button label: **"Run doctor"** or **"Doctor"** → new key e.g. `wizard.readiness.doctorBtn`, or reuse `rail.runChecks` / `tools.doctor`
- Section header stays "Health check" (`wizard.readiness.doctor`) unless copy pass renames it

### Implementation sketch (no edits in this recon)

1. In `WizardStep::Readiness` branch, wrap the existing refresh button in a `flex().flex_row().gap_2()` (match Tools doctor action row).
2. Add sibling:

   - id: `wizard-btn-readiness-deep` (or `wizard-btn-doctor-deep`)
   - style: mirror Tools deep (`primary_wash`, `text_xs`)
   - label: `self.tr("rail.deepCheck")` **or** new wizard key
   - listener: `|this, _, _, cx| this.run_deep_doctor(cx)`

3. Optional: if marketing wants **"Doctor"** as the shallow re-run, either rename refresh to `rail.runChecks` / "Run doctor" or add a third button; **avoid three near-duplicates** (Run checks again + Run checks + Thorough). Prefer **two**: combined shallow+plan refresh, plus thorough doctor.
4. i18n: EN + IT for any new keys; existing `rail.deepCheck` already has IT ("Controllo completo").
5. Tests (if any UI id contracts): none found that assert wizard button ids; host doctor idle/deep covered in `host.rs` unit tests. Optional: pure test that step still maps title key (already in `wizard.rs`).

### What not to do

- Do not reimplement doctor/plan in `wizard.rs` (keep pure step machine).
- Do not spawn a second doctor text buffer; shared `doctor_text` is correct.
- Do not auto-run **deep** on enter Readiness (slow; Tools keeps deep as explicit).
- Do not require Tools navigation for thorough checks once the button exists.

### Effort

~1 small UI change in `main.rs` Readiness arm + optional i18n key. No new host API. Wire existing `run_deep_doctor`.

---

## 5. File index

| Path | Role |
|------|------|
| `crates/colibri-native/src/wizard.rs` | Step enum, titles, open/close, pure navigation |
| `crates/colibri-native/src/main.rs` | `wizard_next` auto doctor/plan; `wizard_view` Readiness UI; Tools doctor buttons; `run_doctor` / `run_deep_doctor` / `run_plan` |
| `crates/colibri-native/src/host.rs` | `run_shallow_doctor`, `run_deep_doctor`, `run_plan`, checklist formatters |
| `crates/colibri-native/src/i18n.rs` | Wizard readiness + Tools/rail doctor strings (EN/IT) |
| `crates/colibri-native/README.md` | Documents Tools Doctor quick vs thorough |
| `.agents/reports/recon-native-ui-doctor-scan.md` | Older doctor empty-path / scan roots recon (idle path fixed since: empty → Idle, not cwd Fail) |

---

## 6. One-line conclusion

**Wizard step 4 already shows shallow doctor + plan** (auto on leave-Model, refresh button runs both). **Tools already has Run checks + Thorough check** wired to the same host path. **Minimal fix: add a Thorough check (Doctor deep) button beside "Run checks again"** on Readiness, calling `run_deep_doctor` — no new backend.
