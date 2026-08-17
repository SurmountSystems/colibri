# Recon: Doctor "Overall: Fail" with all visible checks `[pass]`

**Date:** 2026-08-11
**Scope:** `colibri-sys` doctor aggregation + `colibri-native` checklist formatting / wizard+Tools clip
**Mode:** explore only (no product edits)

---

## Verdict

**Not a pure overall-aggregation bug.**
`Overall: Fail` means the report’s overall `status` is `"error"`, which is set **only** when **at least one** check has `status == "fail"`.

**Primary UX bug:** the native Doctor panel **clips** the checklist (`max_h` + scroll). The first checks (path / config / tokenizer / persistence / in-process engine) are almost always pass for a real model leaf, so the visible fold can look “all green” while a **later** fail (very often `memory.ram`, sometimes `accelerator.cuda` or `model.shards`) still drives overall Fail.

**Secondary product harshness (not display):** `memory.ram` is a hard **fail** for conditions that the Memory plan panel only surfaces as **warnings**. That matches operator evidence: Memory plan “Warnings (RAM budget, VRAM in use)” + Overall Fail, with early checklist lines all pass.

**Not:** memory-plan panel text rewriting Overall. Plan is a separate `plan_text` path.

---

## 1. How overall pass / fail is set

### Report schema

`DoctorReport.status` is one of `ok` | `warning` | `error` (not Pass/Fail):

```59:68:crates/colibri-sys/src/doctor.rs
pub struct DoctorReport {
    pub schema_version: u32,
    /// `ok` | `warning` | `error`
    pub status: String,
    pub model: String,
    /// `standard` | `deep`
    pub mode: String,
    pub checks: Vec<DoctorCheck>,
    ...
}
```

### Aggregation (after all checks are built)

```1307:1314:crates/colibri-sys/src/doctor.rs
let statuses: HashSet<&str> = checks.iter().map(|c| c.status.as_str()).collect();
let status = if statuses.contains("fail") {
    "error"
} else if statuses.contains("warn") {
    "warning"
} else {
    "ok"
};
```

Rules:

| Any check status | Overall `report.status` |
|------------------|-------------------------|
| at least one `fail` | `error` |
| no fail, at least one `warn` | `warning` |
| only `pass` / `skip` | `ok` |

There is **no** path that sets overall from `plan.warnings` alone without also emitting check rows. Plan warnings become `placement.plan` with status **`warn`** (not fail):

```1247:1255:crates/colibri-sys/src/doctor.rs
if p.warnings.is_empty() {
    checks.push(check("placement.plan", "pass", "tier placement has no warnings"));
} else {
    checks.push(check("placement.plan", "warn", p.warnings.join("; ")));
}
```

### Checks that can fail after the “early green” five

Standard order in `run_doctor` (abbreviated):

1. `model.path`
2. `model.config`
3. `model.tokenizer`
4. `storage.persistence`
5. `engine.binary` ← operator’s “in-process engine is available”
6. `accelerator.cuda` (can **fail** if requested GPU missing or GPU runtime lib missing)
7. `model.shards`
8. `storage.disk` (warn if &lt; 1 GB free)
9. **`memory.ram`** ← hard **fail** when budget &gt; available **or** `cache_slots_per_layer < 1`
10. `placement.plan` (warn when plan has warnings)
11. `storage.ssd_probe` (skip/pass)

`memory.ram` fail branches:

```1232:1245:crates/colibri-sys/src/doctor.rs
let (ram_status, ram_summary) = if available_memory == 0 {
    ("warn", "available RAM could not be measured")
} else if ram.budget_bytes > available_memory {
    ("fail", "planned RAM budget exceeds available memory")
} else if ram.cache_slots_per_layer < 1 {
    ("fail", "RAM budget cannot hold one expert slot per sparse layer")
} else {
    ("pass", "RAM budget is viable")
};
checks.push(check("memory.ram", ram_status, ram_summary));
```

Same “cannot hold one expert slot” string is also pushed into **plan warnings** (warn-level only):

```278:280:crates/colibri-sys/src/plan.rs
if cap < 1 {
    warnings.push("RAM budget cannot hold one expert slot per sparse layer".into());
}
```

So one condition yields:

- checklist: `[fail] RAM budget cannot hold one expert slot…` → **Overall Fail**
- checklist: `[warn] …` on `placement.plan`
- Memory plan panel: “review warnings…” + `Warning: …`

### Budget floor that can force `budget > available`

```214:223:crates/colibri-sys/src/plan.rs
let ram_budget = if opts.ram_gb > 0.0 {
    (opts.ram_gb * GB as f64) as u64
} else {
    (available_memory as f64 * 0.88) as u64
};
let ram_budget = if ram_budget < 4 * GB {
    8 * GB
} else {
    ram_budget
};
```

If free RAM is under 4 GiB, budget is raised to **8 GiB**, then doctor fails with “planned RAM budget exceeds available memory”. That is a real fail row, not a ghost overall.

Host doctor also forces `ram_gb` from **available** memory (100% of free), while Memory plan (`run_plan`) leaves `ram_gb` default and uses the 88% branch:

```527:531:crates/colibri-native/src/host.rs
if let Some(m) = machine {
    opts.available_memory = Some(m.available_memory);
    opts.available_disk = Some(m.model_store.free_bytes);
    opts.ram_gb = m.available_memory as f64 / GB as f64;
    opts.gpus = Some(m.gpus.clone());
}
```

---

## 2. How native UI prints "Overall: Fail/Pass"

### Mapping

```228:238:crates/colibri-native/src/host.rs
fn doctor_overall_label(status: &str) -> &'static str {
    match status {
        "ok" => "Pass",
        "warning" => "Warning",
        "error" => "Fail",
        ...
    }
}
```

### Checklist body (all checks, no filter)

```434:458:crates/colibri-native/src/host.rs
pub fn format_doctor_checklist(report: &colibri_sys::DoctorReport) -> String {
    let overall = doctor_overall_label(&report.status);
    ...
    let mut out = format!("Overall: {overall}\n{model_line}\n{depth}\n");
    ...
    for c in &report.checks {
        let mark = doctor_check_mark(&c.status);
        ...
        out.push_str(&format!("[{mark}] {label}\n"));
    }
}
```

Display does **not** drop fail rows. It also does **not** sort fails first or annotate “N failed checks under Overall”.

### Clip heights (why only the green five show)

| Surface | Clip | File |
|---------|------|------|
| Tools Doctor panel body | `max_h(px(140.))` + `overflow_scroll` | `main.rs` ~1818–1819 (`panel`) |
| Wizard readiness doctor body | `max_h(px(200.))` + `overflow_scroll` | `main.rs` ~3703–3704 |

Typical first screenful:

```
Overall: Fail
Model: …/DeepSeek-V4-Flash-0731
Depth: quick

[pass] model directory is readable
[pass] config.json is valid
[pass] tokenizer.json found
[pass] model directory can store usage and KV state
[pass] in-process engine is available
```

That matches operator screenshots **without** scrolling. Fail lines (e.g. `memory.ram`) sit **below the fold**.

### Memory plan is separate

`run_plan` → `format_plan_readiness`; warnings only change plan copy (“review warnings before start”), not doctor overall.

---

## 3. Exact condition for Overall Fail + all *visible* passes

**Logical condition (truthful overall):**
`checks.iter().any(|c| c.status == "fail")` ⇒ `report.status == "error"` ⇒ `Overall: Fail`.

**Observed UI condition (operator screenshot):**
A real model leaf + in-process engine pass + **at least one later fail** (most likely for DeepSeek + tight RAM/VRAM evidence):

1. **`memory.ram` fail** — budget &gt; available, **or** zero expert cache slots per layer
   (aligned with Memory plan RAM warnings; expert-slot message is shared with plan)
2. and/or **`accelerator.cuda` fail** — missing CUDA/HIP runtime when GPUs are present
3. and/or **`model.shards` fail** — rare if scan/path looks healthy

**Plus** the short Doctor viewport so the fail row is not in the first paint.

**Cannot happen by design:** overall `error` with **zero** fail checks in the same report (aggregation + format share one `DoctorReport`). If every check including scrolled ones is truly `pass`/`skip`/`warn`, overall must be Pass or Warning, not Fail.

Status bar “Running doctor...” is independent (set at start of `run_doctor` / deep; cleared to “Checks finished” / “Doctor finished” when recovery returns). Mid-run screenshot or a separate stamp race is possible; it does not invent Overall Fail.

---

## 4. Classification

| Hypothesis | Result |
|------------|--------|
| Overall aggregation invents Fail with zero fails | **No** — code requires a `fail` check |
| Memory plan panel couples into Overall | **No** — separate strings; plan warnings only add `placement.plan` **warn** |
| Display / scroll hides the real fail | **Yes — primary UX root cause** |
| Severity mismatch (RAM treated as fail while plan is warn) | **Yes — secondary product issue**; explains Fail + “Memory plan has Warnings” together |
| `ram_budget` 8 GiB floor ⇒ budget &gt; available | **Possible contributing fail producer** on low free RAM |

---

## 5. Smallest product fix (recommended) + red/green contracts

### Recommended fix (small, product-honest)

Do **both** of these; they are independent and both small:

#### A. Checklist UX (must-have for this report)

In `format_doctor_checklist` (`host.rs`):

1. Under Overall, add a one-line reason when not Pass, e.g.
   `Overall: Fail · 1 failed check`
   or first fail summary:
   `Overall: Fail · planned RAM budget exceeds available memory`
2. **Sort / emit fail checks first** (then warn, then pass/skip), **or** keep order but always prefix a short “Failed:” block listing fail summaries.

Optional UI: raise Tools/wizard doctor `max_h` slightly, but fail-first / Overall reason is enough even in a short panel.

#### B. Severity alignment for RAM (optional but matches operator mental model)

In `doctor.rs` `memory.ram`:

- Keep **fail** only for clearly impossible hard stops if product wants them (or demote both branches to **warn** so overall becomes **Warning** when RAM is tight but model files + engine pass).
- Smallest severity-only change: change `cache_slots_per_layer < 1` and/or `budget_bytes > available_memory` from `"fail"` → `"warn"` so overall becomes **Warning** (same as `placement.plan` / Memory plan panel), **not** Fail, when the only problems are capacity warnings.

Prefer product intent: **“Fail” = broken install / unreadable model / no engine**; **“Warning” = may run poorly (RAM/VRAM)**. That matches the green early checks the operator already trusts.

**Do not** “fix” by lying in Overall while a fail check remains; change either the check severity or the presentation of fails.

### Red / green test contracts (do not implement here)

**Contract 1 — Overall tracks fails; fails not invisible in string**

```text
GIVEN a DoctorReport with early checks pass and memory.ram fail
WHEN format_doctor_checklist(report)
THEN output contains "Overall: Fail"
AND output contains "[fail]" with the ram summary
AND (after fix A) the Overall line or first body lines mention the fail reason
   without requiring the reader to scroll past five [pass] lines
```

Unit home: `colibri-native` `host` tests next to `format_doctor_checklist_is_not_cli_dump`.

**Contract 2 — no fail ⇒ not Fail**

```text
GIVEN only pass + warn checks (e.g. placement.plan warn, memory.ram pass)
WHEN run_doctor / format
THEN report.status == "warning" and checklist "Overall: Warning"
AND never "Overall: Fail"
```

**Contract 3 — severity intent (if fix B lands)**

```text
GIVEN a model whose plan sets cache_slots_per_layer = 0 but path/config/tokenizer/engine pass
WHEN run_doctor with ample disk and in-process engine
THEN memory.ram is warn (not fail) OR remains fail only if product still wants hard fail
AND Overall matches the chosen severity (Warning vs Fail)
AND placement.plan still surfaces the same warning text
```

**Contract 4 — aggregation unit (sys)**

```text
GIVEN synthetic checks: five pass + one fail(memory.ram)
WHEN statuses aggregation in run_doctor (or pure helper if extracted)
THEN status == "error"
GIVEN five pass + only warn
THEN status == "warning"
```

Red first: build a report/fixture that mirrors the screenshot (DeepSeek-shaped or tiny fixture with `available_memory` tiny / slots 0), assert today’s string has Fail while the **first** N lines after header are all `[pass]`; green after fail-first or Overall reason line.

---

## 6. File map (absolute)

| Role | Path |
|------|------|
| Aggregation + memory.ram | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/doctor.rs` (~1232–1255, ~1307–1323) |
| Plan budget floor + warnings | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/plan.rs` (~214–223, ~278–307) |
| Overall label + checklist format | `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/host.rs` (~228–254, ~434–558) |
| Host doctor opts (ram_gb = available) | `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/host.rs` (~523–559) |
| Tools panel clip 140px | `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/main.rs` (~1815–1822) |
| Wizard doctor clip 200px | `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/main.rs` (~3697–3711) |
| Memory plan readiness copy | `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/host.rs` (~832–863) |

---

## Bottom line

Overall Fail is almost certainly **correct for the full report** (a later check fails, most likely **`memory.ram`** under DeepSeek + RAM pressure). The operator-visible contradiction is a **display fold / ordering problem**, amplified by treating RAM capacity as overall **error** while Memory plan only shows **warnings**. Fix: surface fails above the fold (or on the Overall line), and optionally demote pure capacity checks from fail → warn so Overall becomes Warning when every install/engine check is green.
