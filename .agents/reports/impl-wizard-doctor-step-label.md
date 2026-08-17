# Report: wizard Doctor step + Doctor button labels

## Prior miss

Operator asked for a **doctor step / doctor button** on wizard readiness.
We shipped step title **"Check readiness"** and a secondary **"Thorough check"**
button (via `rail.deepCheck`) instead. That naming was wrong relative to the ask.

## What shipped now

Surgical label/CTA fix only. No wizard redesign. Enum stays `WizardStep::Readiness`
(internal); product copy is Doctor.

### Wizard step 4 (Readiness)

| Surface | EN | IT |
|---------|----|----|
| Step title (`wizard.readiness.title`) | **Doctor** | **Doctor** |
| Body (`wizard.readiness.body`) | unchanged (health check + memory plan) | unchanged |
| Section label doctor (`wizard.readiness.doctor`) | Health check | Controllo salute |
| Section label plan | Memory plan | Piano memoria |

### Buttons on readiness

| Control | ID | Style | Action | EN label | IT label |
|---------|----|-------|--------|----------|----------|
| **Doctor** (primary) | `wizard-btn-doctor` | primary fill | `run_deep_doctor` | **Doctor** | **Doctor** |
| Quick check | `wizard-btn-readiness-refresh` | wash | shallow doctor + plan | **Quick check** | **Controllo rapido** |

i18n keys:

- `wizard.readiness.runDoctor` → Doctor / Doctor
- `wizard.readiness.refresh` → Quick check / Controllo rapido

Status strings after deep run: `Running doctor...` / `Doctor finished`.

### Intentionally unchanged

- Tools rail still uses `rail.runChecks` ("Run checks") and `rail.deepCheck`
  ("Thorough check") for the Tools tab doctor controls. Scope was wizard
  readiness naming only.
- Host doctor/plan paths, hard edges, themes, engine-built / tilde / prefs:
  not touched.

## Files

- `crates/colibri-native/src/i18n.rs` — EN/IT strings + test asserts for Doctor
- `crates/colibri-native/src/main.rs` — readiness arm button order, IDs, handlers,
  deep status copy

## Verify

```text
cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets -- -D warnings   # ok
cargo test -p colibri-native                                  # 180 passed
```

Contract covered in `i18n::tests::wizard_and_tools_keys_en_it`:
`wizard.readiness.title` and `wizard.readiness.runDoctor` resolve to `"Doctor"`
in EN and IT; refresh is Quick check / Controllo rapido.
