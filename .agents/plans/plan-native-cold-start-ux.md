# Plan: Native cold-start UX (doctor, scan, HF labels)

## Context

Operator screenshots of **colibri-native** cold start looked product-correct for chrome (rail, machine AMD probe, empty hero, inference + HF blocks), but three concrete gaps hurt trust:

| Issue | Root cause | Severity |
|-------|------------|----------|
| **Doctor Overall: Fail** with no model | Empty path → `"."` (cwd) → `config.json` fail → overall `"error"` | UX harshness; doctor is correct for a path, wrong for empty |
| **Scan finds no old GLM** | Inventory is **one store root**, **depth-1** children with `config.json` only; old downloads often live outside `~/.local/share/colibri/models` | Expected miss; product should make recovery obvious and optionally deepen scan under store |
| **HF field shows bare `1`** | `min_free_input` default `"1"`; placeholder **Min free GB** is hidden when content non-empty | Labeling bug |

**Recon:** `.agents/reports/recon-native-ui-doctor-scan.md`, `recon-native-hf-install-parity.md`, `recon-plan-native-ux-polish.md`.

### Non-goals

- Tauri/React install parity (those surfaces have **no** HF install UI).
- Auto-scanning `~/.cache/huggingface/hub` by default (noisy, huge).
- Changing default min free from 1 GiB.
- Full i18n of every doctor checklist string from sys.
- Rewriting `coli doctor` CLI semantics for empty path.

### Assumptions

1. Empty-model doctor is a **host** policy: do not invent a fake pass; show idle/skip, not Fail against cwd.
2. Finding old GLM is primarily **path paste / store env / move-or-symlink**; scan improvements are under-store depth + clearer empty copy, not whole-disk search.
3. HF labels go through **i18n** EN+IT for install form fields.

---

## Approach

**Order: doctor empty → HF labels → scan discoverability → copy/residual.**

### 1. Doctor empty-model UX (host-first)

When model path is empty (or only `.` after trim):

- **Do not** call `run_doctor` / deep doctor with `PathBuf::from(".")`.
- Show a fixed friendly checklist, e.g.:
  - Overall: **Idle** (or **Skip**) — not Fail
  - Model: (none selected)
  - Info/skip line: set a model path, use Scan models, or install from Hugging Face
- Keep real doctor for any non-empty path (including deliberate `.` if operator types it).
- Bootstrap: same empty branch (no auto-fail on launch).
- Soften hero / empty copy if it implies doctor “works” as overall Pass without a model.

Optional later: sys `DoctorOptions` for no-model mode. **Not required** if host never sends empty path.

### 2. HF install field labels

- Always-visible caption **Min free disk (GB)** (EN) / IT equivalent beside or above `min_free_input`.
- Same pattern for repo / revision / dest if those are placeholder-only today.
- Keys in `i18n.rs` (`install.minFree`, etc.).
- Default value remains `1`; `0` still turns gate off (document in caption or help line).

### 3. Scan discoverability (store first, depth second)

**Tier A (must ship):** Clear empty-scan message:

- Store path
- Rule: one-level dirs with `config.json`
- Recovery: paste full path that contains `config.json`, or set `COLIBRI_MODEL` / `COLIBRI_MODEL_STORE`, or install into the store

**Tier B (recommended in this plan):** Bounded deeper scan **under the model store only**:

- Walk depth 2–3 (or find `config.json` under store with a small max depth + max entries cap)
- Dedupe; still require `config.json`
- Tests: `store/m/config.json` and `store/owner/name/config.json` both found; junk dirs without config ignored

**Tier C (out of scope unless operator asks):** extra scan roots list, HF hub cache, file-picker dialog for arbitrary folders.

### 4. Copy consistency

- Align Plan empty hint with Doctor empty (“set path first”).
- Status after scan: list count + store path; if empty, short recovery text (not only “No models under …”).

---

## Critical files

| Path | Why |
|------|-----|
| `crates/colibri-native/src/main.rs` | `run_doctor` / `run_deep_doctor` empty → `.`; `scan_registry`; HF form panel |
| `crates/colibri-native/src/host.rs` | Doctor formatters; `registry_scan_roots`; install min-free parse/format |
| `crates/colibri-native/src/i18n.rs` | EN/IT install + empty-state strings |
| `crates/colibri-native/src/text_input.rs` | Placeholder only when empty (drives bare `1`) |
| `crates/colibri-sys/src/doctor.rs` | Leave behavior for real paths; no required change if host skips empty |
| `crates/colibri-sys/src/model/registry.rs` | Depth-1 `refresh` → bounded deeper walk for Tier B |
| `crates/colibri-sys/src` paths / store | Default store + env keys (docs only if needed) |
| `crates/colibri-native/docs/fidelity.md` / residual | Honesty if scan depth or doctor idle changes product claim |

---

## Reuse

| Piece | How |
|-------|-----|
| `format_doctor_checklist` / `doctor_overall_label` | New branch or pre-check for empty path |
| `ModelRegistry::refresh` | Extend walk; keep config.json gate |
| `parse_min_free_gb` / space line | Keep logic; add visible labels only |
| i18n EN/IT tables | Pattern from rail/hero keys |

---

## Steps

1. **Red: doctor empty** — host unit test: empty model path → checklist has no Overall Fail from cwd config; shows none-selected + idle/skip.
2. **Green: doctor empty** — branch in `run_doctor` / deep / bootstrap; formatter/host helpers as needed.
3. **Red/green: HF labels** — i18n keys present EN+IT; form shows Min free disk (GB) with value `1` still default.
4. **Scan Tier A** — empty message + recovery copy (tests on message builder if pure).
5. **Scan Tier B** — registry depth/cap + red/green nested fixture under temp store.
6. **Copy pass** — Plan/hero/status alignment; residual or fidelity one-liner if scan depth claim changes.
7. **Mop** — fmt, clippy, `cargo test -p colibri-sys` + `colibri-native` (install feature on).

---

## Risks

| Risk | Mitigation |
|------|------------|
| Tests expect Overall Fail on empty/`.` | Update only tests that encoded the bad empty UX; keep fail for real missing config |
| Deeper scan picks junk | Require `config.json`; depth + entry caps; no hub cache |
| IT lag | Ship EN+IT keys together for install labels |
| Operators still miss off-store GLM | Tier A recovery text is mandatory even if Tier B ships |

---

## Verification

| Slice | Proof |
|-------|--------|
| Doctor empty | Red then green: empty path → Idle/Skip, not Fail |
| Doctor with path | Existing or new: dir without config still overall Fail |
| HF labels | Visible min-free label; install still gates on 1 GiB default |
| Scan depth-1 | Still finds `store/m/config.json` |
| Scan depth-2 | Nested under store found after Tier B |
| Regression | `cargo test -p colibri-sys --lib`; `cargo test -p colibri-native`; clippy on both |

Manual: cold start → Doctor shows Idle; paste real GLM path → doctor pass; Scan with nested store layout lists models; HF form readable without guessing `1`.

---

## Open questions

- **Q1 — Scan depth:** Cap at depth 2 under store only, or depth 3? (Default if unanswered: **depth 2**, max N entries e.g. 64.)
- **Q2 — Overall label for empty doctor:** `Idle` vs `Skip` vs `Ready`? (Default: **Idle**.)
- **Q3 — Extra scan roots in this plan?** (Default: **no**; env store + path paste only.)

---

## Board after approval (seed)

| Id | Work |
|----|------|
| `feat:native-cold-start-ux` | Close cold-start doctor/scan/HF polish |
| `impl:doctor-empty-idle` | Empty path doctor Idle/Skip (host) |
| `impl:hf-install-labels` | Min free + install field i18n labels |
| `impl:scan-empty-copy` | Tier A empty-scan recovery copy |
| `impl:scan-depth-store` | Tier B bounded depth under store |
| `impl:native-ux-mop` | fmt/clippy/tests |

---

### Critical Files for Implementation

- `crates/colibri-native/src/main.rs` — empty doctor branch, scan message, HF form
- `crates/colibri-native/src/host.rs` — checklist format, registry roots, min-free
- `crates/colibri-native/src/i18n.rs` — install + empty-state strings
- `crates/colibri-sys/src/model/registry.rs` — bounded deeper scan
- `crates/colibri-sys/src/doctor.rs` — only if host needs shared empty report type
