# Implement: Doctor Overall Fail while checklist looks all pass

**Date:** 2026-08-11
**Scope:** `colibri-native` checklist UX + `colibri-sys` `memory.ram` severity
**Recon:** `.agents/reports/recon-doctor-overall-fail-all-pass.md`

---

## What changed

### A. Checklist UX (`crates/colibri-native/src/host.rs`)

`format_doctor_checklist`:

1. When overall status is `error` (UI **Fail**), the Overall line includes the first fail check summary, e.g.
   `Overall: Fail · RAM budget cannot hold one expert slot per sparse layer`
2. Check rows are sorted **fail → warn → pass → skip** (stable within each group) so short panels show problems first.

Optional clip heights (`crates/colibri-native/src/main.rs`):

- Tools shared `panel` body: `max_h` 140 → **220**
- Wizard doctor body: `max_h` 200 → **280**

### B. Severity alignment (`crates/colibri-sys/src/doctor.rs`)

`memory.ram` capacity branches are **`warn`**, not **`fail`**:

| Condition | Before | After |
|-----------|--------|-------|
| available RAM unmeasured | warn | warn (unchanged) |
| planned RAM budget exceeds available memory | fail | **warn** |
| RAM budget cannot hold one expert slot per sparse layer | fail | **warn** |

Product intent: **Fail** = broken install / model unreadable / no engine path. **Warning** = may run poorly (RAM/VRAM capacity). Model + engine green with only capacity tightness → **Overall: Warning**, not Fail.

Aggregation still requires a real `fail` check for Overall Fail; no invented Pass.

---

## TDD evidence

### Red (before product edit)

| Test | Command | Fail reason |
|------|---------|-------------|
| `host::tests::format_doctor_checklist_surfaces_fail_near_top` | `cargo test -p colibri-native format_doctor_checklist_surfaces_fail_near_top` | Overall line was bare `Overall: Fail`; fail row after five `[pass]` lines |
| `doctor::tests::memory_ram_capacity_tight_is_warn_not_fail` | `cargo test -p colibri-sys --lib memory_ram_capacity_tight_is_warn_not_fail` | `memory.ram` status was `"fail"`; expected `"warn"` for budget > available |

### Green (after product edit)

Same filters pass. Full suites:

```
cargo fmt -p colibri-sys -p colibri-native
cargo clippy -p colibri-sys -p colibri-native --all-targets -- -D warnings
cargo test -p colibri-sys --lib doctor
cargo test -p colibri-native
```

| Step | Exit |
|------|------|
| fmt | 0 |
| clippy `-D warnings` | 0 |
| `colibri-sys --lib doctor` (20 tests) | 0 |
| `colibri-native` (249 tests) | 0 |

---

## Files touched

| Path | Change |
|------|--------|
| `crates/colibri-native/src/host.rs` | Overall fail reason; sort rank; red/green checklist test |
| `crates/colibri-native/src/main.rs` | Tools panel max_h 220; wizard doctor max_h 280 |
| `crates/colibri-sys/src/doctor.rs` | RAM capacity fail→warn; capacity-tight warn test |

---

## Notes

- Existing `format_doctor_checklist_is_not_cli_dump` still green (`Overall: Fail` substring + marks).
- No test asserted `memory.ram` capacity as fail; new sys test locks the warn contract.
- Overall Pass is never forced while a fail check remains.
