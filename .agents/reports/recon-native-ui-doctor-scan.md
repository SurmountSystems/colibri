# Recon: native UI doctor + model scan (empty model / old GLM)

Evidence-only. Paths under `/home/hunter/Projects/surmount/colibri`.

## A) Doctor: config.json fail expected vs bug

**Expected when no model is selected**, not a deep-check-only bug.

1. Empty model field → path becomes **`.`** (cwd) for both quick and thorough:

```376:398:crates/colibri-native/src/main.rs
    fn run_doctor(...) {
        let path = if path.as_os_str().is_empty() {
            PathBuf::from(".")
        } else { path };
        ...
    fn run_deep_doctor(...) {
        let path = if path.as_os_str().is_empty() {
            PathBuf::from(".")
        } else { path };
```

Same on bootstrap (`bootstrap_panels`, ~2644–2649).

2. Doctor always checks `model.join("config.json")`. Missing/invalid → **fail**. Any fail → overall `"error"` → UI **Overall: Fail**:

```978:990:crates/colibri-sys/src/doctor.rs
    let config = model.join("config.json");
    let valid_config = ...;
    checks.push(check(..., if valid_config { "pass" } else { "fail" },
        ... "config.json is missing or invalid"));
```

```1221:1223:crates/colibri-sys/src/doctor.rs
    let status = if statuses.contains("fail") { "error" } else if ...
```

3. If cwd is a readable dir without a model container: **[pass] model directory is readable** + **[fail] config.json…** matches the screenshot. Deep only adds safetensors/shard checks after standard ones; it does **not** require a selected path beyond that `.` fallback.

4. UI labels empty/`.` as none selected:

```217:225:crates/colibri-native/src/host.rs
    let model_line = if report.model.is_empty() || report.model == "." {
        "Model: (none selected)".to_string()
```

**Bug?** Product claim is “Doctor … work without a model” (`i18n` hero), but empty state still runs checks against `.` and overall-Fails. Checklist is honest; overall Fail for “no model” is a **UX harshness**, not a broken config validator.

## B) Model scan roots and contract

| Item | Evidence |
|------|----------|
| Scan roots | `registry_scan_roots(store, empty)` → one root: `machine.model_store.path` or `default_model_store_path()` (`host.rs` 709–724; `main.rs` 798–799) |
| Default store | `~/.local/share/colibri/models` (`$XDG_DATA_HOME/colibri/models`); Windows `%LOCALAPPDATA%\colibri\models` (`paths.rs` 40–63) |
| Env store | **`COLIBRI_MODEL_STORE` / `COLI_MODEL_STORE` only** (`MODEL_STORE_ENV_KEYS`). **No `COLIBRI_MODELS`.** |
| Model path env | **`COLIBRI_MODEL` / `COLI_MODEL`** fill the path field only (`env_model_path`); **not** scan roots |
| What counts | Dir with **`config.json` file** |
| Depth | **Immediate children only** (+ root if root has `config.json`). **No recursion.** |

```85:105:crates/colibri-sys/src/model/registry.rs
    /// Rescan all roots: each immediate child that looks like a model dir, plus
    /// the root itself if it contains `config.json`.
    ...
                if p.is_dir() && p.join("config.json").is_file() {
```

Empty message hardcodes the store path: `No models under {} (dirs with config.json)` (`main.rs` 803–806). Store path is platform-default hardcoding of `…/colibri/models`, not a magic constant elsewhere.

## C) Where old GLM downloads likely live / why scan misses them

Python/`coli` **never** used `~/.local/share/colibri/models`. Docs require explicit **`COLI_MODEL`** at operator paths:

- `/nvme/glm52_i4`, `/fast/glm52_i4`, `/d/glm52_i4` (`README.md`, `docs/quickstart.md`)
- `hf download … --local-dir D:\glm52_i4` or `~/Models/inkling_i4` (`docs/windows.md`, `docs/inkling.md`)
- HF hub cache (typical `~/.cache/huggingface/hub/…`) is **not** a scan root

Miss cases for scan:

1. Model outside the store (old HF/local-dir) → not listed; paste path or set `COLIBRI_MODEL`.
2. Nested under store (`…/models/foo/bar/config.json`) → **depth-1 only**, missed.
3. Incomplete download without top-level `config.json` → not a model.
4. Store env pointed elsewhere → different root.

Install into UI goes **under** the store (`resolve_install_dest` in `host.rs`); old manual downloads usually did not.

## D) Empty-state UI (brief)

Mostly product-correct: path placeholder `COLIBRI_MODEL / COLI_MODEL`, registry “Scan model store…”, Plan says choose a folder when empty. Doctor’s **Overall Fail** + path-readable-on-`.` while hero says doctor works without a model is the main awkwardness; config fail itself is expected until a real model dir is set.

## Operator fix for the GLM screenshot

Point the model field (or `COLIBRI_MODEL`) at the real GLM folder that contains `config.json`, **or** put/link that folder as a **direct child** of `~/.local/share/colibri/models` (or set `COLIBRI_MODEL_STORE` to the parent of that folder), then Scan again.
