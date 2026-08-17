# Recon: rail Setup still green after Finish

Not a persist miss. Finish wrote `first_run_done`. The left-rail **Setup** slab is never gated.

## 1. When is the rail Setup button shown vs hidden?

**Shown always.** There is no `first_run_done`, prefs, or wizard-open check.

- Rail footer paints `btn-setup` unconditionally: [`crates/colibri-native/src/main.rs:3158`](crates/colibri-native/src/main.rs) through [`3182`](crates/colibri-native/src/main.rs). Fill is `p.primary` (the green slab), `text_sm` + semibold. Click always calls `open_setup_wizard`.
- Wizard taking the main column does not hide that rail footer.

**What *is* gated:** empty-chat **hero** Setup only.

- [`show_first_run_setup_cta`](crates/colibri-native/src/main.rs) at [`4622`](crates/colibri-native/src/main.rs): `!first_run_done`.
- Used in `chat_hero` at [`4236`](crates/colibri-native/src/main.rs). Comment at [`4620`](crates/colibri-native/src/main.rs) says rail Setup stays for re-entry.

First-run **auto-open** of the wizard is a third gate: `NativePrefs::should_show_wizard` at [`prefs.rs:214`](crates/colibri-native/src/prefs.rs) (`!first_run_done && !COLIBRI_SKIP_WIZARD`), used at [`main.rs:412`](crates/colibri-native/src/main.rs). That only skips opening the wizard on launch. It does not hide the rail button.

## 2. Did Finish persist `first_run_done`?

**Yes.**

- `wizard_finish` [`main.rs:628`](crates/colibri-native/src/main.rs): `complete_wizard` then `self.first_run_done = prefs.first_run_done` then `persist_prefs_status`.
- `complete_wizard` [`wizard.rs:162`](crates/colibri-native/src/wizard.rs): `prefs.first_run_done = true`; closes wizard.
- `persist_prefs_status` [`main.rs:504`](crates/colibri-native/src/main.rs) writes `shell_prefs_snapshot` via `NativePrefs::save`.

Host file `~/.config/colibri/native-ui.toml` (no secrets; model path is a local folder):

```text
version = 1
first_run_done = true
theme = "doge"
locale = "en"
last_model_path = "/home/hunter/.local/share/colibri/models/GLM-5.2-colibri-int4-g64-with-int8-mtp"
```

In-memory flag and disk agree. The green rail button is not leftover first-run state.

## 3. Product intent: always-available re-open vs first-run CTA

**Prior closed bug hid only the center hero CTA.** Rail Setup was left on purpose.

- Report [`.agents/reports/impl-post-setup-hide-first-run-cta.md`](.agents/reports/impl-post-setup-hide-first-run-cta.md): "Rail **Setup** (re-entry) is fine; the center first-run CTA was wrong." Table: "Rail Setup | Unchanged (intentional re-open)".
- Fidelity: [`crates/colibri-native/docs/fidelity.md:28`](crates/colibri-native/docs/fidelity.md): Skip/Finish set `first_run_done`; "re-open via **Setup** on rail".
- i18n `setup.reopen` [`i18n.rs:278`](crates/colibri-native/src/i18n.rs): "Open setup again anytime from the left rail."
- Tools footer [`main.rs:3671`](crates/colibri-native/src/main.rs): muted hint only. **No Tools button** that re-opens the wizard. The only re-open control is the giant green rail slab.

Skip status still says you can open Setup anytime [`wizard.rs:202`](crates/colibri-native/src/wizard.rs). That is copy for re-entry, not a reason to keep a first-run-sized green CTA on Chat after Finish.

**Mismatch:** last implementer treated rail Setup as the permanent re-open. Operator screenshot (Chat, live engine, rail footer still **Setup**) is that design, not a failed write. New intent: after Finish, do not keep a giant green first-run CTA on the rail.

## 4. Named TDD contract (plain English)

After wizard Finish (or Skip) has set `first_run_done` true:

- The first-run Setup CTA must not stay as a giant green rail button (`btn-setup` + `p.primary`).
- Re-open Setup, if we still offer it, belongs on a quieter Tools or menu path, not a primary rail slab on Chat with a live engine.

Suggested red (do not implement here): a pure helper such as `show_rail_setup_primary_cta(first_run_done)` is false when `first_run_done`; paint test or helper test that the rail footer does not use `p.primary` for Setup after first-run. Hero tests already cover the center CTA only (`show_first_run_setup_cta_false_when_first_run_done` at [`main.rs:5297`](crates/colibri-native/src/main.rs)).

No code changed in this recon.
