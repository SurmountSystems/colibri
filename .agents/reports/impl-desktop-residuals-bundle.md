# Implement: desktop residuals bundle (plan steps 2–6)

**Date:** 2026-08-10
**Repo:** `/home/hunter/Projects/surmount/colibri`
**Role:** implementer (L2). Step 1 (stop/cancel sys) already done.
**Role-swap note:** this feature’s primary implementer is this agent; general review owned stop-sys previously.

## Scope delivered

| Step | Outcome |
|------|---------|
| **2. GPUI Stop** | Stop button; session stays in slot; STOP with active req_id; status `stopped` |
| **3. Visual pump + tiers + PROF** | 500ms pump while engine up; live tiers strip; last N PROF turns |
| **4. Brain panel** | Tier/heat grid + hits pulse; sample ≤2048 cells; limits documented |
| **5. HF install picker** | Feature `install` default; form + progress; prefer-cli; set model path on success |
| **6. Phase D honesty** | Docs only; `ffi_available()` still false; host vs engine wording |

## How stop works

1. `EngineSession` holds `EngineHandle` (Clone/Arc) and `Arc<Mutex<ReqBook>>` with `next_req` / `active_req`.
2. `generate_async` **does not** take the session out of the UI mutex. It clones the handle, allocates `req_id`, sets `active_req`, runs `EngineDuplex::handle_with(Submit)` on a worker thread.
3. **Stop** locks the session, reads `active_req`, calls `engine.with_client(|c| c.stop_request(id))` (mux `STOP {id}`). Concurrent with generate because step 1 unlocked the handle mutex during stream recv.
4. UI sets `stop_requested`; on `GenEvent::Done` / `Error`, status becomes `stopped` (or `stopped (…)`), `generating` cleared.

## How visual pump works

1. After engine start (or generate), `ensure_visual_pump` spawns a GPUI timer loop (~500ms).
2. Each tick: `pump_session_visual` → `EngineHandle::pump_visual` + `visual_snapshot`.
3. UI updates: live tiers text, PROF table (`format_profile_turns`, last 8), Brain via `brain_view_from_map`.
4. No HTTP. Data comes only from mux telemetry already absorbed by the serve client / snapshot.
5. Loop stops when the session slot is empty.

## Install path

1. Cargo feature `install` (default) enables `colibri-sys/install`.
2. Form: HF repo id, optional revision, optional dest under model store.
3. `validate_install_form` (unit-tested, no network): owner/name shape; dest always under store (relative join, or absolute only if already a store descendant; `..` rejected).
4. Free space line via `disk_free_bytes` / probe model store.
5. Worker: `install_async` → `install_model` with **prefer_cli: true**, progress → `InstallEvent` channel.
6. On success: set model path field to install dest; status “install complete”.
7. Mid-download cancel is **not** implemented (OPEN residual).

## Brain limits

- `BRAIN_MAX_CELLS = 2048`. Larger maps are stride-sampled; panel shows `src` vs `disp` and “sampled”.
- Full atlas + hover affinities remain OPEN (`open:brain-full-atlas`).

## Files touched

| Path | Change |
|------|--------|
| `crates/colibri-desktop-gpui/src/host.rs` | Shared session, stop, pump, Brain helpers, install helpers + tests |
| `crates/colibri-desktop-gpui/src/main.rs` | Stop UI, pump, tiers/PROF/Brain panels, install form |
| `crates/colibri-desktop-gpui/Cargo.toml` | Feature `install` → `colibri-sys/install` (default) |
| `crates/colibri-desktop-gpui/README.md` | Surface list, honesty, features |
| `crates/colibri-desktop-gpui/docs/fidelity.md` | Matrix rows for stop, Brain, PROF, tiers, install, FFI |
| `crates/colibri-sys/docs/ffi-phase-d.md` | Host in-process vs engine process honesty |
| `.agents/RESIDUAL.md` | CLOSED / OPEN residual list |
| `.agents/reports/impl-desktop-residuals-bundle.md` | This report |

## Residual path

`/home/hunter/Projects/surmount/colibri/.agents/RESIDUAL.md`

## Commands + exit codes

```text
cargo fmt -p colibri-desktop-gpui -p colibri-sys
# exit 0

cargo test -p colibri-desktop-gpui
# exit 0
# 11 passed; 0 failed

cargo test -p colibri-sys --lib
# exit 0
# 72 passed; 0 failed

cargo clippy -p colibri-sys --all-targets -- -D warnings
# exit 0

cargo clippy -p colibri-desktop-gpui --all-targets --features install -- -D warnings
# exit 0
```

## Tests added (desktop host)

- `format_live_tiers_line`
- `format_profile_empty_and_nonempty`
- `brain_view_samples_large_map`
- `brain_view_full_small_map`
- `brain_view_hit_pulse_on_seq_change`
- `brain_cell_rgb_differs_by_tier`
- `validate_install_rejects_bad_repo` / `validate_install_accepts_owner_name` (feature `install`)

## Not done (honest)

- Full unsampled Brain + atlas tooltips
- Install cancel mid-download
- Model registry picker UI
- True libcolibri FFI (Phase D campaign)
- Dedicated live HWINFO strip UI
- Multi-slot / grammar on ClientFrame
