# Recon: wizard Ready (step 6) freeze / GNOME Not Responding

Read-only. No product edits. Evidence is file:line in this tree.

**Verdict:** Clicking **Start engine** on Ready (or the same control on the left rail) runs `EngineSession::start` **synchronously on the GPUI UI thread**. For a GLM-5.2-class leaf that is mmap + `model_init` / serve READY, that can last **minutes with no event-loop pump**. GNOME `"Unknown" Is Not Responding` is the expected host symptom. **Finish / Skip / Back do not start the engine** and are not a deadlock pair with start.

---

## 1. Step 6 (Ready) buttons and handlers

Wizard is 6 steps; Ready is last (`WizardStep::Ready`, `number() == 6`).

| Control | Element id | Handler | What it does |
|---------|------------|---------|--------------|
| **Back** | `wizard-btn-back` | `wizard_back` | `wizard.back()` + notify. No I/O. |
| **Skip** | `wizard-btn-skip` | `wizard_skip` | `complete_wizard` (first-run done + close) + `persist_prefs_status` (small TOML write). Status: `Setup skipped · you can open Setup anytime`. |
| **Finish** | `wizard-btn-next` (label flips to Finish on last step) | `wizard_next` → `wizard_finish` | Same persist+close as Skip. Status: `Setup complete`. **Does not call `start_engine`.** |
| **Start engine** (in-wizard) | `wizard-btn-start-engine` | `start_engine` | Same as rail. |
| **Start engine** (rail) | `btn-start-engine` | `start_engine` | Rail stays mounted while the wizard is open (`left_rail` is a sibling of the wizard column). |

Wiring:

- Footer Back / Skip / Next|Finish: `main.rs` 3612–3664.
- Last-step Finish: `wizard_next` 588–590 → `wizard_finish` 618–632.
- Ready-only Start engine: `main.rs` 4079–4092 → `this.start_engine(cx)`.
- Rail Start engine: `main.rs` 3012–3026 → same `start_engine`.
- `complete_wizard`: `wizard.rs` 161–165 (prefs flag + `wizard.close()` only).

Copy (faithful i18n): title `Ready`, body “You are set. Finish to use the dashboard. Starting the engine is optional.”, button `wizard.ready.start` = “Start engine” (`i18n.rs` 332–340).

---

## 2. Does Start / Finish / model load run on the UI thread?

**Yes for Start engine. Yes for Finish persist (tiny). Yes for the whole model open.**

`DesktopApp::start_engine` (`main.rs` 982–1032):

1. Early-out if `generating`.
2. Drop the live session under `self.session.lock()`.
3. Set `status = "Starting engine…"` and `cx.notify()`.
4. **Immediately** `EngineSession::start(&path)` on this thread.
5. Only then store the session, persist prefs, start the visual pump.

`cx.notify()` does **not** yield a frame. Paint happens after the mouse-up listener returns. Compare Doctor-step buttons, which **do** yield 16 ms so “Running doctor…” can paint (`dispatch_readiness_action`, `main.rs` 772–782). Start engine has no such yield.

`EngineSession::start` is documented **Blocking** (`host.rs` 1808–1814). Default native features are `install` + `ffi` (`colibri-native/Cargo.toml` 26–31). Resolution is FFI-first (`resolve_prefer_process_from_flags`, `host.rs` 1698–1703):

- `coli_ffi::open_engine` → `GlmEngine::open` → **`coli_glm_engine_open`** (`multi.rs` 201–215, 248–260).
- On FFI failure, **same UI call** falls through to `start_process`: `PlacementPlan::build` (full shard-header inspect) then `EngineHandle::start_with_plan` / `start_blocking` (`host.rs` 1884–1906).

`generate_async` already uses `thread::spawn` (`host.rs` 2019–2030). Start does not.

Finish/Skip: `NativePrefs::save` is a short TOML `fs::write` (`prefs.rs` 225–238). Not a minutes-long hang.

---

## 3. Huge GLM-5.2: what actually blocks, how long?

Default path (feature `ffi`, no `COLIBRI_FORCE_PROCESS`):

1. **`coli_glm_walk_disk`**: `stat` every `*.safetensors` (`colibri.c` 9566–9610). Seconds at worst.
2. **`model_init`** (`colibri.c` 1785+): `st_init_multi` (open/mmap shards), then **`qt_load` of embed, lm_head, every layer’s attention + shared-expert tensors**. Comment in-tree: embed+lm_head ~1.9 GB bf16 on a real GLM. Dense load is the long part. Experts stay on the mmap/store path (`COLI_MMAP` in `colibri.c`), but opening and mapping a many-hundred-GB tree plus paging/mlock can still run **minutes**. Comment on the FFI open: `model_init may abort process on fatal load errors` (`colibri.c` 9606–9607). Abort is a crash, not Not Responding.
3. If FFI fails after that wait: **do it again** as a child serve. `ServeClient::spawn` then **`wait_ready` with no timeout** (`serve.rs` 170–217, 440–456). The child prints `\x01\x01READY\x01\x01` only after its own `model_init`. The UI thread is stuck in `read_until` until READY or EOF. **Unbounded.**

Process-only (`COLIBRI_FORCE_PROCESS=1` or `--no-default-features`): skip FFI, still block on plan inspect + spawn + `wait_ready`.

`PlacementPlan::build` → `ModelInfo::inspect` reads **every shard header** (`model/mod.rs` 143–201). Already paid when the user planned; paid again on the process fallback during start.

**Expected hang:** GNOME’s Not Responding is typically ~5 s of no X events. A GLM-5.2 open is routinely **tens of seconds to many minutes**, so the dialog is expected, not a mystery deadlock.

---

## 4. Watchdog / “still starting” / event-loop pump?

**None during start.**

- Status string `"Starting engine…"` is set but **will not paint** until start returns.
- `ensure_visual_pump` (`main.rs` 1194+) starts only **after** a successful start (`1024`).
- `wait_ready` has **no timeout**.
- Ready step does not paint `progress_strip_el`. That strip is install/generate only (`progress_strip_el` 1995–2031; install form 3234–3243).
- Doctor’s 16 ms yield is **not** reused here.

---

## 5. Finish + Start together / deadlock?

**No mutex deadlock in code.** Both handlers run on the same UI thread; they cannot interleave.

- Finish never starts the engine. Product copy says start is optional.
- If **Start is first:** the thread stays inside `EngineSession::start`. Finish/Skip/Back sit in the GPUI queue until start returns (or the process dies). Looks frozen; not a lock cycle.
- If **Finish is first:** wizard closes; in-wizard Start is gone. Rail Start is still there and can then block the same way.
- `session.lock()` is held only to drop/store the session, not across open. Visual pump is not running yet. Generate is gated off (`generating` early-out).
- Double-click Start: second click runs **after** the first open finishes, drops the new session, and opens again. Wasteful, not a deadlock.

Persist + start cannot “both fire” from Finish. They are separate buttons.

---

## 6. Screenshot strings

**`native · Plan finished` (operator wrote a hyphen):** rail footer is `"{brand.native} · {status}"` (`main.rs` 3085–3088). `brand.native` = `"native"` (`i18n.rs` 101). `"Plan finished"` is set only by `DesktopApp::run_plan` (`main.rs` 967–970). On the wizard path that is a **Doctor-step registry row click** (`main.rs` 3999–4005), or Tools “Plan memory” if they were on Tools earlier. Wizard Next from Model runs `run_plan(...)` into `plan_text` but **does not** set that status (`581–586`). Quick check is required **not** to bury doctor under `"Plan finished"` (`wizard.rs` 587–588). So leftover `"Plan finished"` on Ready is consistent with a prior plan click, and it **stays** during a Start hang because `"Starting engine…"` never paints.

**Green “full bar”:** Ready’s Start engine is a **solid `p.ok` (green) button**, full content width, not a progress widget (`main.rs` 4079–4092). Rail Start uses the same ok fill when the engine is not live (`start_button_paint` 4489–4496). Ready does **not** show the install/generate progress strip. A full green slab labeled “Start engine” is the control, not a load meter.

---

## Named contract (later TDD, do not implement here)

**When the user clicks Start engine (wizard Ready or left rail), or Finish on Ready:**

1. **Finish** only marks first-run done, saves prefs, closes the wizard, and shows “Setup complete” (or the save-error line). It must **not** open the model or block the UI for more than a quick disk write.
2. **Start engine** must **not** run mmap / FFI `coli_glm_engine_open` / process spawn / READY wait on the GPUI UI thread.
3. On click, the UI must **paint immediately** (same turn or one short yield, like Doctor): status becomes “Starting engine…”, the Start control disables or shows in-progress, and the event loop keeps pumping so GNOME does not show Not Responding even if load takes **many minutes**.
4. While starting, show a living “still starting” line (elapsed time is enough). No silent full green bar that never moves.
5. A second Start / Finish / chat send must not start a second open on the UI thread. Cancel/Stop during start is a product choice; the floor is “window stays responsive.”
6. When start finishes, then show “Engine ready …” / the existing error line. If FFI falls back to process, keep the same “still starting” UI across both phases.

Contrast already in-tree: `generate_async` + thread; Doctor `cx.spawn` + 16 ms timer before blocking host work.
