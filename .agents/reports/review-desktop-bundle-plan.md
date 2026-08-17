# Plan alignment review: desktop residuals bundle (steps 2–6)

**Date:** 2026-08-10
**Role:** plan-alignment specialist (L2), read-only
**Plan steps under review:** Native desktop residuals **2–6** (GPUI Stop through Phase D honesty)
**Impl summary:** [`.agents/reports/impl-desktop-residuals-bundle.md`](impl-desktop-residuals-bundle.md)
**Process mop:** [`.agents/reports/process-mop-desktop-bundle.md`](process-mop-desktop-bundle.md) (clean)
**Plan / recon SoT:** [`.agents/reports/recon-plan-four-gaps.md`](recon-plan-four-gaps.md) (recommended order + per-gap “what’s missing”), with step-1 boundary already locked in [`.agents/reports/review-stop-sys-plan.md`](review-stop-sys-plan.md) / r2
**Living residual:** [`.agents/RESIDUAL.md`](../RESIDUAL.md)
**Fidelity:** [`crates/colibri-desktop-gpui/docs/fidelity.md`](../../crates/colibri-desktop-gpui/docs/fidelity.md)

No product code edits in this review.

---

## Plan source and step map

There is no separate session `plan.md` under `.agents/plans/`. Authority for this bundle is the recon recommended order plus the deferred UI half of stop, with Phase D demoted to honesty (not true FFI).

| Recon order | Bundle step | Intent |
|-------------|-------------|--------|
| 1 Stop (sys **+** UI) | **1** sys (prior); **2** GPUI Stop | Button + live session; STOP semantics |
| 2 Live tiers + PROF strip | **3** Visual pump + tiers + PROF | Concurrent `pump_visual`; text strips |
| 3 Brain panel | **4** Brain | Grid + hits; atlas optional |
| 4 HF install picker | **5** HF install | Feature + form + progress + prefer-cli |
| 5 Phase D FFI (defer / separate) | **6** Phase D **honesty** only | Docs; no C extraction |

Renumbering (sys stop vs GPUI stop; Phase D → honesty) matches recon bottom line: process mux remains product path; do not block residual on `libcolibri`.

---

## Step checklists (required vs tree)

### Step 2 — GPUI Stop

| Plan item (recon §3 + step-1 deferral) | Evidence | Status |
|----------------------------------------|----------|--------|
| Stop control while `generating` | `main.rs` Stop button → `stop_generate` | **Met** |
| Session stays in slot (no `take()` orphan) | `EngineSession::generate_async` clones handle / req book; locks only to allocate id | **Met** |
| Mux `STOP` with **active** req id | `stop_active` → `with_client(\|c\| c.stop_request(req_id))` | **Met** |
| Prefer Stop → DONE (not disconnect CANCEL) | Uses `stop_request`, not `cancel_request` | **Met** |
| Status reflects stop on stream end | `stop_requested` → Done/Error status `stopped` / `stopped (…)` | **Met** |

### Step 3 — Visual pump + live tiers + PROF

| Plan item (recon §1 + order 2) | Evidence | Status |
|--------------------------------|----------|--------|
| Background visual pump while engine up (idle + mid-turn) | `ensure_visual_pump` ~500ms loop; `pump_session_visual` | **Met** |
| Concurrent with generate (same lock design as stop) | Handle clone + unlocked generate recv (step 1); session stays in mutex | **Met** |
| Live tiers from engine TIERS / snapshot (not plan-text only) | `format_live_tiers` + UI strip | **Met** |
| PROF history strip (table OK; web charts not required for strip MVP) | `format_profile_turns`, last 8 | **Met** |
| Data from mux / `EngineHandle`, not HTTP | `pump_visual` + `visual_snapshot` only | **Met** |

### Step 4 — Brain panel

| Plan item (recon §1 + order 3) | Evidence | Status |
|--------------------------------|----------|--------|
| Grid from EMAP (tier + heat) | `brain_view_from_map` + cell RGB | **Met** |
| Hits pulse on seq change | `brain_view_hit_pulse_on_seq_change` test + UI | **Met** |
| Scale strategy for large maps | `BRAIN_MAX_CELLS = 2048` stride sample; fidelity + residual | **Met** |
| Atlas / hover affinities | Explicitly optional for first native Brain | **Deferred (OPEN)** |
| Document limits | `fidelity.md` Brain section + residual `open:brain-full-atlas` | **Met** |

### Step 5 — HF install picker

| Plan item (recon §2 + order 4) | Evidence | Status |
|--------------------------------|----------|--------|
| Enable `install` on desktop crate | `Cargo.toml` `default = ["install"]` → `colibri-sys/install` | **Met** |
| Form: repo id, optional revision/dest, free-space **display**, progress, errors | `main.rs` HF install panel + channel drain | **Met** |
| Background `install_model` | `install_async` worker thread | **Met** |
| `prefer_cli: true` default | `InstallOptions { prefer_cli: true, … }` | **Met** |
| On success: set model path | Done handler updates `model_input` | **Met** |
| Mid-download cancel | Not first-class in sys; recon optional | **Deferred (OPEN)** |
| Registry scan picker | Separate fidelity gap | **Deferred (OPEN)** |

### Step 6 — Phase D honesty (not true FFI)

| Plan item (recon §4 + order 5) | Evidence | Status |
|--------------------------------|----------|--------|
| Do **not** extract / ship `libcolibri` | No engine C library work in this bundle | **Met** |
| Keep `ffi_available() == false` | Stub unchanged | **Met** |
| Host in-process ≠ engine in-process wording | UI honesty strip; README; `fidelity.md` architecture table; `ffi-phase-d.md` honesty block | **Met** |
| Process mux remains product path | Residual architecture reminder; fidelity matrix FFI **missing** | **Met** |

---

## Scope check (over-scope / under-scope)

### Over-scope (must not have landed in this bundle)

| Forbidden / deferred | In this slice? |
|----------------------|----------------|
| True Phase D C extract / ABI | **No** |
| OpenAI REST / SSE gateway in GPUI | **No** |
| Tauri / SPA pixel parity | **No** |
| Full unsampled Brain + atlas tooltips as done | **No** (OPEN residual) |
| Model registry picker UI as done | **No** (OPEN residual) |
| Multi-slot / GBNF ClientFrame | **No** |
| Install mid-download cancel as shipped | **No** (OPEN residual) |
| Product code under unrelated crates beyond honesty docs | **No** (desktop host/main + fidelity/README + `ffi-phase-d` honesty + residual/report) |

Supporting in-scope noise (allowed):

- Host unit tests for tiers/PROF formatters, brain sample/pulse, install form validate
- Fidelity matrix row updates and residual CLOSED/OPEN rewrite
- Subscribe mask at engine start for visual interest

None of that is a second product campaign.

### Under-scope / plan misses

**No required plan items for steps 2–6 are missing.**

Honest OPEN items in `RESIDUAL.md` match recon deferrals and optional polish, not silent plan failure:

| OPEN id | Why not a step 2–6 plan miss |
|---------|------------------------------|
| `open:brain-full-atlas` | Atlas optional for first native Brain |
| `open:install-cancel` | Not first-class in install API; recon optional |
| `open:model-registry-ui` | Explicit separate missing row |
| `open:live-hwinfo-strip` | Recon runtime strip partial; fidelity still partial |
| `open:ffi-phase-d` | Strategic defer; step 6 is honesty only |
| `open:brain-pulse-decay` | Polish vs web multi-frame fade |
| `open:install-min-free-gate` | Free-space **display** required; hard `min_free_bytes` threshold polish (UI uses 0) |
| `open:visual-pump-idle-stop` | Process nit; pump exits when slot empty |

---

## Fidelity / residual honesty cross-check

| Claim surface | Aligned with impl? |
|---------------|--------------------|
| Fidelity: Stop **done** | Yes |
| Fidelity: tiers **done**, PROF **done** | Yes (text strip, not SPA charts) |
| Fidelity: Brain **partial** (sample + no atlas) | Yes |
| Fidelity: install **done** (no mid cancel) | Yes |
| Fidelity: FFI **missing** | Yes |
| Fidelity: HWINFO **partial** | Yes; not claimed done |
| `RESIDUAL.md` CLOSED rows vs bundle steps 2–6 | Matches impl report outcomes |
| Architecture reminder (host ≠ engine) | Matches step 6 |

No CLOSED claim invents true FFI, full Brain atlas, registry picker, or install cancel.

---

## Nits (not plan misses)

### Nit 1 — free-space gate is display-only

- **Where:** `main.rs` install path sets `min_free = 0` with comment that space is informational.
- **Plan:** recon form list requires free-space **display**; API row mentions `min_free_bytes` gate capability.
- **Read:** MVP form met; hard threshold is polish (`open:install-min-free-gate`).
- **Suggestion:** Leave OPEN; do not re-open step 5 as incomplete.

### Nit 2 — install is HF-only form

- **Where:** `InstallSource::HuggingFace` only; `register: false`; no LocalPath field.
- **Plan:** intro mentioned HF or local folder; concrete “what’s missing” is HF form + separate registry UI.
- **Read:** Aligned with form MVP + `open:model-registry-ui`.

### Nit 3 — impl report “dest under store” is slightly loose

- **Where:** `validate_install_form` allows absolute `dest` outside the store.
- **Impact:** Claim wording only; not a missing plan feature.
- **Suggestion:** Residual or future tighten if operators want store-rooted installs only.

### Nit 4 — hits pulse is one-shot, not web-style multi-frame decay

- Tracked as `open:brain-pulse-decay`. First native Brain required flash on seq change; decay animation is polish.

### Nit 5 — desktop tests are host helpers, not full Stop/pump integration

- Eleven unit tests cover formatters, brain sample/pulse, install validation.
- Concurrent Stop + DONE is covered at **sys** (step 1), not a GPUI window test.
- Acceptable for fidelity-demo host; not a recon-mandated desktop integration suite.

---

## Issues (plan misses / over-scope)

**None** at plan-miss or over-scope severity.

---

## Verdict

**aligned**

Steps 2–6 deliver the recon residual path for GPUI Stop, visual pump with live tiers and PROF, sampled Brain, HF install form with prefer-cli, and Phase D honesty without true FFI. Deferred items are named OPEN residual and match fidelity partial/missing rows. No required plan item is missing; no out-of-scope campaign (real libcolibri, REST, Tauri parity, full atlas-as-done) leaked into the bundle.

---

## References (absolute)

- `/home/hunter/Projects/surmount/colibri/.agents/reports/impl-desktop-residuals-bundle.md`
- `/home/hunter/Projects/surmount/colibri/.agents/reports/recon-plan-four-gaps.md`
- `/home/hunter/Projects/surmount/colibri/.agents/RESIDUAL.md`
- `/home/hunter/Projects/surmount/colibri/crates/colibri-desktop-gpui/docs/fidelity.md`
- `/home/hunter/Projects/surmount/colibri/crates/colibri-desktop-gpui/src/host.rs`
- `/home/hunter/Projects/surmount/colibri/crates/colibri-desktop-gpui/src/main.rs`
- `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/ffi-phase-d.md`
