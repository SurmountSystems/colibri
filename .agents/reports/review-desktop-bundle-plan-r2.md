# Plan alignment review r2: desktop residuals bundle (steps 2–6)

**Date:** 2026-08-10
**Role:** plan-alignment specialist (L2), read-only
**Pass:** re-check after desktop fix (`impl-desktop-bundle-fix.md`)
**Plan steps under review:** Native desktop residuals **2–6** (GPUI Stop through Phase D honesty)
**Prior alignment (r1):** [`.agents/reports/review-desktop-bundle-plan.md`](review-desktop-bundle-plan.md) → **aligned**
**Impl (bundle):** [`.agents/reports/impl-desktop-residuals-bundle.md`](impl-desktop-residuals-bundle.md)
**Impl (fix pass):** [`.agents/reports/impl-desktop-bundle-fix.md`](impl-desktop-bundle-fix.md)
**Reviews fixed:** [`.agents/reports/review-desktop-bundle-general.md`](review-desktop-bundle-general.md), [`.agents/reports/review-desktop-bundle-tests.md`](review-desktop-bundle-tests.md) (both **Status: fixed**, 0 open)
**Plan / recon SoT:** [`.agents/reports/recon-plan-four-gaps.md`](recon-plan-four-gaps.md)
**Living residual:** [`.agents/RESIDUAL.md`](../RESIDUAL.md)
**Fidelity:** [`crates/colibri-desktop-gpui/docs/fidelity.md`](../../crates/colibri-desktop-gpui/docs/fidelity.md)

No product code edits in this review.

---

## What the fix pass changed (scope of this re-check)

Fix pass was **quality / contract honesty** inside already-delivered steps 2–6, not a new product step:

| Fix | Plan step touch | Plan miss risk? |
|-----|-----------------|-----------------|
| Brain heat curve → web `heat/24`, lum `0.35+0.65` | 4 (Brain fidelity vs web) | No; strengthens Brain MVP |
| `resolve_install_dest` / `path_is_under_store` | 5 (dest under store claim → real rule) | No; closes r1 Nit 3 claim gap |
| `ReqBook::begin` / `clear_matching` + host Stop/gen tests | 2 (Stop bookkeeping guards) | No; regression guards only |
| `status_after_gen_done`, stop race sets `stop_requested` | 2 (status `stopped`) | No; hardens required status contract |
| `saw_terminal` (no double Error) | 2 polish | No |
| `install_options_for_ui` + prefer_cli / space / tiers / PROF last-N tests | 3, 5 | No |
| Packed tier/heat decode asserts | 4 | No |

Reported verify: fmt + clippy `-D warnings` + **23** desktop tests (`--features install`). Residual OPEN list unchanged (no false CLOSED).

---

## Plan source and step map (unchanged from r1)

No separate session `plan.md` under `.agents/plans/`. Authority remains recon recommended order + deferred UI half of stop; Phase D is honesty only.

| Recon order | Bundle step | Intent |
|-------------|-------------|--------|
| 1 Stop (sys **+** UI) | **1** sys (prior); **2** GPUI Stop | Button + live session; STOP semantics |
| 2 Live tiers + PROF strip | **3** Visual pump + tiers + PROF | Concurrent `pump_visual`; text strips |
| 3 Brain panel | **4** Brain | Grid + hits; atlas optional |
| 4 HF install picker | **5** HF install | Feature + form + progress + prefer-cli |
| 5 Phase D FFI (defer) | **6** Phase D **honesty** only | Docs; no C extraction |

---

## Step checklists after fix

### Step 2 — GPUI Stop

| Plan item | Evidence post-fix | Status |
|-----------|---------------------|--------|
| Stop control while `generating` | `main.rs` Stop → `stop_session` / `stop_request` | **Met** |
| Session stays in slot (no `take()` orphan) | `generate_async` clones handle + book; no slot take | **Met** |
| Mux `STOP` with **active** req id | `stop_active` → `with_client(\|c\| c.stop_request(req_id))` | **Met** |
| Prefer Stop → DONE (not disconnect CANCEL) | `stop_request`, not `cancel_request` | **Met** |
| Status reflects stop on stream end | `status_after_gen_done` + UI race: `stop_requested = true` even when stop fails mid-allocate | **Met** (strengthened) |
| Host regression guards (review, not recon mandate) | `stop_session_empty_slot_errors`, `generate_async_errors_when_no_session`, `req_book_*`, `status_after_gen_done_respects_stop_requested` | **Met** (new) |

Live mux mid-stream STOP id still owned by colibri-sys (step 1). Desktop correctly does not re-encode wire tests.

### Step 3 — Visual pump + live tiers + PROF

| Plan item | Evidence post-fix | Status |
|-----------|---------------------|--------|
| Background visual pump while engine up | `ensure_visual_pump` ~500ms; `pump_session_visual` | **Met** |
| Concurrent with generate | Shared handle + unlocked stream recv (step 1) | **Met** |
| Live tiers from engine snapshot | `format_live_tiers` + strengthened RAM/GB asserts | **Met** |
| PROF last N turns | `format_profile_turns` + `format_profile_keeps_last_n_only` | **Met** (window now unit-pinned) |
| Data from mux / `EngineHandle` only | No HTTP | **Met** |
| Empty-slot pump | `pump_session_visual_none_when_slot_empty` | **Met** (guard) |

### Step 4 — Brain panel

| Plan item | Evidence post-fix | Status |
|-----------|---------------------|--------|
| Grid from EMAP (tier + heat) | `brain_view_from_map` + **web-aligned** `brain_cell_rgb` (`heat/24`) | **Met** (heat bug closed) |
| Hits pulse on seq change | Existing pulse + test | **Met** |
| Scale strategy ≤2048 | `BRAIN_MAX_CELLS` stride sample | **Met** |
| Packed decode honesty | Small + large map assert tier/heat at sample indices | **Met** (new) |
| Atlas / hover affinities | Still OPEN | **Deferred (OPEN)** |
| Document limits | `fidelity.md` Brain **partial**; residual `open:brain-full-atlas` | **Met** |

### Step 5 — HF install picker

| Plan item | Evidence post-fix | Status |
|-----------|---------------------|--------|
| Enable `install` on desktop crate | default feature → `colibri-sys/install` | **Met** |
| Form: repo, rev, dest, free-space display, progress, errors | form + channel; `format_install_space` tested | **Met** |
| **Dest under model store** | `resolve_install_dest` + `path_is_under_store`; reject `..` and absolute escape; absolute under store OK | **Met** (r1 claim gap closed) |
| Background `install_model` | `install_async` worker | **Met** |
| `prefer_cli: true` default | `install_options_for_ui` + unit test | **Met** (pinned) |
| On success: set model path | Done handler | **Met** |
| Mid-download cancel | Not first-class | **Deferred (OPEN)** |
| Registry scan picker | Separate gap | **Deferred (OPEN)** |

### Step 6 — Phase D honesty (not true FFI)

| Plan item | Evidence post-fix | Status |
|-----------|---------------------|--------|
| No `libcolibri` extract / ship | Fix pass did not touch C engine | **Met** |
| `ffi_available() == false` | Stub unchanged | **Met** |
| Host in-process ≠ engine in-process | README, fidelity architecture, `ffi-phase-d.md` honesty | **Met** |
| Process mux remains product path | Residual architecture reminder; fidelity FFI **missing** | **Met** |

---

## Scope check (over-scope / under-scope)

### Over-scope

| Forbidden / deferred | Introduced by fix pass? |
|----------------------|-------------------------|
| True Phase D C extract / ABI | **No** |
| OpenAI REST / SSE in GPUI | **No** |
| Tauri / SPA pixel parity | **No** |
| Full unsampled Brain + atlas as done | **No** (still OPEN) |
| Model registry picker as done | **No** |
| Multi-slot / GBNF ClientFrame | **No** |
| Install mid-download cancel as shipped | **No** |
| Product work outside desktop host/main + honesty docs | **No** (desktop-only fix) |

Supporting in-scope noise: pure helpers, unit tests (11 → 23), fidelity wording for heat + dest. Not a second campaign.

### Under-scope / plan misses

**No required plan items for steps 2–6 are missing.**

OPEN residual still matches recon deferrals / polish, not silent plan failure:

| OPEN id | Why not a step 2–6 plan miss |
|---------|------------------------------|
| `open:brain-full-atlas` | Atlas optional for first native Brain |
| `open:install-cancel` | Recon optional; API has no first-class cancel |
| `open:model-registry-ui` | Explicit separate missing row |
| `open:live-hwinfo-strip` | Fidelity still partial; not claimed done |
| `open:deep-doctor-ui` | Sys deep; GPUI shallow only |
| `open:multi-slot` / `open:grammar-submit` | Out of this bundle |
| `open:ffi-phase-d` | Step 6 is honesty only |
| `open:tauri-parity` / `open:openai-rest` | Explicit non-goals |
| `open:brain-pulse-decay` | Polish vs web multi-frame fade |
| `open:install-min-free-gate` | Free-space **display** required; hard threshold polish (`min_free=0`) |
| `open:visual-pump-idle-stop` | Process nit |

r1 Nit 3 (dest-under-store wording vs code) is **resolved by product enforce**, not left as loose claim.

---

## Fidelity / residual honesty cross-check

| Claim surface | Aligned after fix? |
|---------------|--------------------|
| Fidelity: Stop **done** | Yes; host guards + status helper strengthen |
| Fidelity: tiers **done**, PROF **done** | Yes (text strip; last-N tested) |
| Fidelity: Brain **partial** (sample + no atlas; heat matches web) | Yes; heat/24 noted in matrix notes |
| Fidelity: install **done** (dest under store rules; no mid cancel) | Yes; wording matches `resolve_install_dest` |
| Fidelity: FFI **missing** | Yes |
| Fidelity: HWINFO **partial** | Yes; not claimed done |
| `RESIDUAL.md` CLOSED rows for steps 2–6 | Matches bundle + fix (Stop, pump, tiers, PROF, Brain, install, Phase D honesty) |
| `RESIDUAL.md` OPEN | Unchanged correctly; heat/dest closed in code+docs, not falsely left OPEN |
| Architecture reminder (host ≠ engine) | Unchanged; still correct |

No CLOSED claim invents true FFI, full Brain atlas, registry picker, or install cancel.

---

## Nits (not plan misses)

### Nit 1 — free-space gate remains display-only

- UI still uses `min_free_bytes = 0`; free space is informational.
- Plan required free-space **display**; hard threshold is `open:install-min-free-gate`.
- Unchanged from r1; not re-opened as step 5 incomplete.

### Nit 2 — install remains HF-only form

- `InstallSource::HuggingFace` only; no LocalPath field; registry UI OPEN.
- Aligned with recon form MVP.

### Nit 3 — dest containment is lexical, not filesystem-canonical

- `path_is_under_store` is component-prefix (no symlink/`canonicalize` resolve).
- Adequate for MVP “under store” product rule; not a recon miss. Symlink escape is future polish if operators care.

### Nit 4 — hits pulse still one-shot

- `open:brain-pulse-decay`. First native Brain required flash on seq change only.

### Nit 5 — no full GPUI integration / mock `stop_request(id)` desktop test

- Host now guards bookkeeping, empty slot, and stopped status text.
- Live `stop_request(active_id)` wire remains sys. Acceptable split for fidelity-demo host; not a recon-mandated desktop integration suite.

### Nit 6 — r1 Nit 3 closed

- Former “impl report dest wording slightly loose” is no longer accurate; code + fidelity + tests enforce under-store.

---

## Issues (plan misses / over-scope)

**None** at plan-miss or over-scope severity.

Fix pass did not demote any required step 2–6 item, did not close OPEN residual falsely, and did not expand into Phase D FFI or other deferred campaigns.

---

## Verdict

**aligned**

Steps 2–6 remain fully delivered relative to recon residual intent. The desktop fix pass **tightens** Brain heat fidelity (step 4), **enforces** install dest under store (step 5), and **guards** Stop/status/PROF/pump contracts (steps 2–3) without adding out-of-scope product. Deferred OPEN rows still match fidelity partial/missing. No required plan item missing; no over-scope leakage.

---

## References (absolute)

- `/home/hunter/Projects/surmount/colibri/.agents/reports/review-desktop-bundle-plan.md` (r1)
- `/home/hunter/Projects/surmount/colibri/.agents/reports/impl-desktop-residuals-bundle.md`
- `/home/hunter/Projects/surmount/colibri/.agents/reports/impl-desktop-bundle-fix.md`
- `/home/hunter/Projects/surmount/colibri/.agents/reports/review-desktop-bundle-general.md`
- `/home/hunter/Projects/surmount/colibri/.agents/reports/review-desktop-bundle-tests.md`
- `/home/hunter/Projects/surmount/colibri/.agents/reports/recon-plan-four-gaps.md`
- `/home/hunter/Projects/surmount/colibri/.agents/RESIDUAL.md`
- `/home/hunter/Projects/surmount/colibri/crates/colibri-desktop-gpui/docs/fidelity.md`
- `/home/hunter/Projects/surmount/colibri/crates/colibri-desktop-gpui/src/host.rs`
- `/home/hunter/Projects/surmount/colibri/crates/colibri-desktop-gpui/src/main.rs`
- `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/ffi-phase-d.md`
