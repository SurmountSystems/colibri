# Report: Tools panel + Setup wizard + plain EN/IT copy

**Package:** `colibri-native`
**Date:** 2026-08-11
**Plan:** `.agents/plans/plan-native-wizard-tools-theme.md` (shell UX slice)

## Delivered

### 1. Main tabs
- `MainView::Tools` next to Chat | Brain | Profiling
- Topbar tab uses `nav.tools` (EN: Tools, IT: Strumenti)

### 2. Slim left rail
**Kept:** brand, model folder path + summary, Start / Stop engine, live placement bar + hardware strip when engine is up, temperature + max tokens, status, **Setup** button.

**Moved to Tools:** machine details, Check model (doctor), memory plan + scan/registry, download/install form, theme picker, language, About, advanced chat options (reasoning, session slots, grammar).

### 3. Setup wizard
New module: [`crates/colibri-native/src/wizard.rs`](../../crates/colibri-native/src/wizard.rs)

| Piece | Behavior |
|-------|----------|
| Steps | Welcome → Your machine → Choose a model → Check readiness → Look and feel → Ready |
| Nav | Back / Next / Skip / Finish |
| First-run | `prefs.should_show_wizard()` (`!first_run_done && !COLIBRI_SKIP_WIZARD`) opens wizard on launch |
| Skip / Finish | `complete_wizard` sets `first_run_done = true`, saves prefs, closes wizard |
| Re-open | Setup button on rail (and hero Setup) anytime |
| Model path | Prefs `last_model_path` loaded into model field at start (already in prefs slice) |
| Step 3 | Scan + pick list; optional download expand (`feature = "install"`) |
| Step 4 | Doctor + memory plan plain summary; refresh button; auto-refresh when Next from model step |
| Step 5 | DOGE / Mint; live theme + save prefs |
| Step 6 | Summary + optional Start engine; Finish exits |

Full-main wizard replaces the main column while open; slim rail stays.

### 4. i18n
[`i18n.rs`](../../crates/colibri-native/src/i18n.rs): EN + IT for `wizard.*`, `tools.*`, `setup.*`, `theme.doge`, `theme.mint`, rail Setup/Stop, plain tier/doctor labels (no PROF/EMAP/HWINFO in primary copy).

### 5. Prefs / theme wire
- `ThemeId::to_pref`
- `shell_prefs_snapshot` + `apply_theme` + save on theme switch, locale cycle, engine start, wizard complete
- Text inputs re-palette on theme change

### 6. Tests
| Area | Result |
|------|--------|
| Wizard step advance / skip / finish → `first_run_done` | unit tests in `wizard::tests` |
| Theme switch saves prefs (temp dir) | `theme_switch_saves_prefs_temp_dir` |
| i18n wizard/tools keys EN+IT | `wizard_and_tools_keys_en_it` |
| Full package | **153 passed** |

### 7. Verify

```bash
cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets -- -D warnings   # green
cargo test -p colibri-native --bin colibri-native             # 153 ok
```

## Files touched

| Path | Change |
|------|--------|
| `crates/colibri-native/src/wizard.rs` | **New** step machine + complete + prefs helpers + tests |
| `crates/colibri-native/src/main.rs` | Tools view, slim rail, wizard UI, theme/locale/setup handlers |
| `crates/colibri-native/src/i18n.rs` | Plain EN/IT keys |
| `crates/colibri-native/src/theme.rs` | `ThemeId::to_pref` |

## Not in this slice
- Tauri/web parity
- Brain/Profiling further changes (already themed earlier)
- Progress widget changes (already shipped)
- Git commit
