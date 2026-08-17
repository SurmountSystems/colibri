# Recon: native and colibri-sys config formats (JSON vs TOML)

Date: 2026-08-11
Workspace: `/home/hunter/Projects/surmount/colibri`
Mode: read-only (no product edits)

## Operator ask (restated)

Clarify what is **JSON** vs **TOML** today, where files live, and design for:

- **Native config in TOML** as the primary write path
- **JSON compatibility** only where operators may already have JSON on disk
- Scope: UI prefs vs engine/runtime config vs model path

## Executive answer

| Layer | On-disk format today | Path | Who owns it |
|-------|----------------------|------|-------------|
| **Native UI prefs** | **TOML only** (already shipped) | `~/.config/colibri/native-ui.toml` | `colibri-native` `prefs` |
| **Engine / embed runtime** | **No app config file** | Process env + in-memory `ColibriConfig` | `colibri-sys` |
| **Model store root** | Directory of models (not a config file) | XDG data / env | `colibri-sys` `paths` |
| **Model identity** | HF **JSON** inside the model dir | `<model>/config.json` etc. | weights / HF layout |
| **Measured tune profiles** | **JSON** (Python `autotune`) | `~/.config/colibri/tuning/<fp>.json` | `c/autotune.py` / `coli tune` |
| **Web SPA prefs** | Browser **localStorage** (not a file) | keys `colibri.*` | `web/` |
| **Expert atlas** | Static **JSON** asset | `experts.json` / env path | Brain UI, not user settings |

**There is no `native-ui.json` (or any native UI prefs JSON) in this tree.** Operator line "native config is in TOML" matches code: `PREFS_FILE_NAME = "native-ui.toml"`.

**JSON compatibility for native UI is not required for existing native users** (TOML is already the only prefs format). JSON on disk that operators may have is almost entirely **model HF files**, **tune profiles**, and **atlas/fixture** files, not shell prefs.

---

## 1. Config paths inventory

### 1.1 Native UI prefs (TOML) — primary product "native config"

| Item | Value |
|------|--------|
| Module | [`crates/colibri-native/src/prefs.rs`](../../crates/colibri-native/src/prefs.rs) |
| File name | `native-ui.toml` (`PREFS_FILE_NAME`) |
| Schema version | `PREFS_VERSION = 1` |
| Linux/mac default | `$XDG_CONFIG_HOME/colibri/native-ui.toml` else `~/.config/colibri/native-ui.toml` |
| Windows default | `%LOCALAPPDATA%\colibri\native-ui.toml` (fallback `C:\colibri\native-ui.toml`) |
| Dep | direct `toml = "0.8"` in `colibri-native` |
| Load | `prefs::load()` → `load_from_path` + `apply_env_overrides` |
| Save | `NativePrefs::save` / `save_to_path` → `toml::to_string_pretty` |
| Corrupt / missing | silent **defaults** (no error to operator) |

**Struct `NativePrefs` (serialized keys):**

```text
version: u32              # default 1
first_run_done: bool      # wizard gate
theme: "doge" | "mint"    # ThemePref; unknown → doge
locale: "en" | "it"       # LocalePref; unknown → en
last_model_path: String   # optional absolute model dir; empty = unset
```

Loose deserialize via `RawNativePrefs` (all fields `Option`) so partial files work.

**Env (post-load, do not rewrite file on read):**

| Env | Effect |
|-----|--------|
| `COLIBRI_THEME` | Non-empty → override theme (`doge` / `mint`; unknown → doge) |
| `COLIBRI_SKIP_WIZARD` | Truthy `1` / `true` / `yes` → hide first-run wizard without writing prefs |

**Wiring today:** `DesktopApp::new` calls `prefs::load()`; seeds theme, `first_run_done`, model path from `last_model_path` after env model. Wizard Skip/Finish and Tools theme paths call `save()`. See [`main.rs`](../../crates/colibri-native/src/main.rs) ~190–530 and [`wizard.rs`](../../crates/colibri-native/src/wizard.rs).

Example disk file:

```toml
version = 1
first_run_done = true
theme = "mint"
locale = "en"
last_model_path = "/home/user/.local/share/colibri/models/my-glm"
```

### 1.2 colibri-sys `ColibriConfig` — no load/save file API

| Item | Value |
|------|--------|
| Module | [`crates/colibri-sys/src/config.rs`](../../crates/colibri-sys/src/config.rs) |
| Doc claim | "The product has no TOML/YAML app config; configuration is the process environment plus a model directory." |
| Persistence | **None.** Host builds `ColibriConfig` in memory. |
| Serde | `Serialize` + `Deserialize` present (usable for tests / future file), **not** used for on-disk app config |

**Struct highlights:** `model`, `model_store`, `engine`, `policy`, `ram_gb`, `ctx`, `cap`, `max_tokens`, `temperature`, `top_p`, `gpu_indices`, `vram_gb`, `cuda_enabled`, `kv_slots`, `mirror`, `extra_env` (`EnvMap`), `no_omp_tune`, `prefer_process`.

**Precedence for engine env (highest wins):**

1. Explicit `ColibriConfig` fields / `extra_env` the host set
2. Parent process environment (when merging)
3. Placement plan `environment_for_plan` setdefault
4. Engine built-in defaults

Also: `COLIBRI_FORCE_PROCESS` always forces process path over FFI.

### 1.3 Model store / registry paths (dirs, not config files)

| Item | Value |
|------|--------|
| Module | [`crates/colibri-sys/src/paths.rs`](../../crates/colibri-sys/src/paths.rs), [`model/registry.rs`](../../crates/colibri-sys/src/model/registry.rs) |
| Env | `COLIBRI_MODEL_STORE` then `COLI_MODEL_STORE` |
| Default Linux/mac | `$XDG_DATA_HOME/colibri/models` else `~/.local/share/colibri/models` |
| Default Windows | `%LOCALAPPDATA%\colibri\models` |
| Registry | In-memory scan of roots; **no registry DB file**. Model leaf = dir with `config.json`, depth ≤2, cap 64 entries |

**Active model path resolution (native host):**

1. `COLIBRI_MODEL` / `COLI_MODEL` (env)
2. else `native-ui.toml` `last_model_path`
3. else empty / operator paste / install dest

### 1.4 Measured tune profiles (JSON under config home)

| Item | Value |
|------|--------|
| Source | `c/autotune.py` (`coli tune`), docs [`docs/tuning.md`](../../docs/tuning.md) |
| Path | `$XDG_CONFIG_HOME/colibri/tuning/<fingerprint>.json` or `%LOCALAPPDATA%\colibri\tuning\<fingerprint>.json` |
| Format | **JSON** schema v1; fingerprint SHA-256 prefix of CPU/GPU/model files/engine mtime |
| Loaded by | `--auto-tier` (setdefault); explicit env always wins; `--no-tune-profile` bypass |
| Rust port | **Not** in `colibri-sys` today |

Same XDG **config** tree as native-ui (`…/colibri/`), different subdirectory (`tuning/` vs file `native-ui.toml`).

### 1.5 Model-directory JSON (HF layout; not app prefs)

Operators will have these on disk under every model:

| File | Role |
|------|------|
| `config.json` | Arch geometry / family routing (`model_type`) |
| `tokenizer.json` | Required preflight |
| `model.safetensors.index.json` | Optional weight map |
| `generation_config.json` | Sometimes present (HF) |
| `.coli_usage` / `.coli_kv` / `.coli_ssd` | Engine side state (not JSON prefs) |
| Mirror receipts | e.g. `.colibri-mirror.json` (partial mirror) |

These must stay JSON (HF / tool contracts). **Do not** migrate to TOML.

### 1.6 Static / fixture JSON (not user config)

- `web/public/experts.json` and cwd / `COLIBRI_EXPERTS_JSON` atlas for Brain tips
- `c/glm_tiny/config.json`, `ref_glm.json`, test fixtures
- Doctor / plan **output** can be JSON on stdout (`coli doctor --json`, `coli plan --json`)

### 1.7 Web SPA (localStorage, not native config)

| Key | Content |
|-----|---------|
| `colibri.baseUrl` | API base |
| `colibri.model` | Model id string for HTTP API |
| `colibri-locale` | i18n |
| legacy `colibri.apiKey` | deliberately **removed** on persist |

Not shared with `native-ui.toml`. Tauri desktop wraps the web SPA; GPUI native is separate.

### 1.8 Unrelated TOML

- Workspace / crate `Cargo.toml`
- Host Grok `~/.grok/config.toml` (operator IDE tool; documented in sys user-guide only as client config)
- **Not** product native config

---

## 2. What is JSON today that an operator might have on disk?

| Category | Path pattern | Migrate to TOML? |
|----------|--------------|------------------|
| HF model metadata | `<model>/config.json`, `tokenizer.json`, index | **No** |
| Tune profiles | `~/.config/colibri/tuning/*.json` | **No** (unless a deliberate future port; keep JSON for Python parity) |
| Expert atlas | `experts.json` | **No** |
| Native UI prefs | *(none in JSON)* | N/A; already TOML |
| `ColibriConfig` export | *(none written by product)* | N/A |

**Conclusion for "preserve JSON compatibility for existing users":** for **native shell prefs**, there are no JSON users. Compatibility work is only needed if:

1. You later add a **host settings file for engine knobs** and someone already hand-wrote JSON, or
2. You want one loader that accepts both shapes for a **new** unified settings file, or
3. You confuse HF `config.json` with app config (do not).

---

## 3. What already uses TOML?

| Consumer | Use |
|----------|-----|
| **`colibri-native` prefs** | Load/save `native-ui.toml` (primary write path already) |
| Cargo / build tooling | Workspace manifests (not product runtime) |

`colibri-sys` has **no** `toml` dependency and no TOML I/O. Older recon (`recon-plan-revise-wizard-progress-toml.md`) is **stale** on "no direct toml dep"; native now depends on `toml = "0.8"`.

---

## 4. Design: TOML primary, optional JSON compatibility

### 4.1 Scope split (keep separate)

| Scope | Format | File / mechanism | Notes |
|-------|--------|------------------|-------|
| **A. UI prefs** | **TOML** primary | `native-ui.toml` | theme, locale, first_run, last_model_path |
| **B. Engine / runtime** | Env + in-memory `ColibriConfig` | no required file | Optional future: `engine.toml` or section in a host file; env still wins |
| **C. Model path** | String path | env → prefs `last_model_path` → store scan | Model **contents** stay HF JSON |
| **D. Tune profiles** | JSON | `tuning/<fp>.json` | Leave alone; not "native config" |
| **E. Registry** | No file | scan roots | Optional future inventory cache is product-new |

Operator phrase **"native config is in TOML"** maps to **A**, not B–E.

### 4.2 Recommended load order — native UI prefs (product path today + optional JSON)

Current product order:

1. Read `default_prefs_path()` → `native-ui.toml`
2. Missing/corrupt → `NativePrefs::default()`
3. Apply env: `COLIBRI_THEME` (and wizard gate reads `COLIBRI_SKIP_WIZARD` separately)
4. Host uses prefs; model field still loses to `COLIBRI_MODEL` / `COLI_MODEL`

**If adding JSON compatibility** (only if product wants a dual format; not required by current users):

1. Prefer **`native-ui.toml`** if it exists and parses
2. Else try **`native-ui.json`** (same dir; same field names) if present
3. Else defaults
4. Env overrides after merge
5. **Write path always TOML** (`native-ui.toml`); optional migrate-on-write: after successful save of TOML, leave JSON alone or delete only if `migrate_remove_json=true` (default: leave JSON, stop reading it once TOML exists so TOML wins)

Do **not** invent JSON write for prefs; that fights "TOML primary."

### 4.3 Optional future engine host file (only if residual asks)

If sys ever persists embed knobs:

```text
~/.config/colibri/engine.toml   # or [engine] table in a host.toml
```

Load order (align with existing env doctrine):

1. Process env (always wins over file)
2. Explicit API fields on `ColibriConfig`
3. File (`engine.toml` preferred; optional `.json` fallback)
4. Plan setdefault
5. Engine defaults

Keep **out of** `native-ui.toml` (UI shell vs runtime). Same config **directory** (`colibri/`), different files.

### 4.4 What not to mix

- Do not put theme/locale into model `config.json`
- Do not rewrite tune profile JSON as TOML without a Python dual-reader
- Do not use Tauri `tauri.conf.json` as user prefs

---

## 5. COLIBRI_* / COLI_* env map (native + sys surface)

### Native host / prefs-adjacent

| Env | Role |
|-----|------|
| `COLIBRI_THEME` | Override prefs theme |
| `COLIBRI_SKIP_WIZARD` | Suppress wizard |
| `COLIBRI_MODEL` / `COLI_MODEL` | Active model dir |
| `COLIBRI_ENGINE` / `COLI_ENGINE` | Engine binary override |
| `COLIBRI_MODEL_STORE` / `COLI_MODEL_STORE` | Install / scan root |
| `COLIBRI_KV_SLOTS` / `KV_SLOTS` | Mux slots |
| `COLIBRI_FORCE_PROCESS` | Always process engine |
| `COLIBRI_PREFER_FFI` | Prefer FFI (mostly redundant under native `feature=ffi`) |
| `COLIBRI_BRAIN_FULL` / `COLI_BRAIN_FULL` | Full expert grid |
| `COLIBRI_EXPERTS_JSON` / `COLI_EXPERTS_JSON` | Atlas path |
| `COLIBRI_WINDOWED` | Windowed launch vs default window mode |
| `COLIBRI_FFI_CUDA` / `COLIBRI_REQUIRE_FFI_CUDA` | Build-time / link GPU FFI |
| `COLIBRI_*_STATIC_LIB` | Prebuilt static libs for FFI build |

### Sys / engine spawn (subset; full list in `docs/ENVIRONMENT.md`)

`SNAP`, `COLI_MODEL`, `COLI_ENGINE`, `CTX`, `NGEN`, `RAM_GB`, `KV_SLOTS`, `COLI_TEMP`, `COLI_POLICY`, `COLI_CUDA`, `COLI_GPU(S)`, `COLI_NO_OMP_TUNE`, `COLI_MODEL_MIRROR`, plan-applied OMP/PIPE keys, etc.

`ColibriConfig` maps the typed subset into these env keys for serve/plan.

---

## 6. Structs quick reference

### `NativePrefs` (`colibri-native`)

Path helpers: `platform_default_prefs_path`, `default_prefs_path`, `load` / `load_from_path`, `save` / `save_to_path`.

### `ColibriConfig` (`colibri-sys`)

In-memory only; `apply_plan`, `serve_env`, `serve_env_with_plan`, `resolved_model_store`, `must_use_process`.

### `ModelRegistry` / `ModelEntry`

Scan roots; entry fields include path, family, status, disk_bytes, shards; leaf marker `config.json`.

### Autotune profile (Python JSON)

`schema_version`, `fingerprint`, `accepted`, `winner.env` restricted to tunable keys (`OMP_NUM_THREADS`, `COLI_NUMA`, `PIPE`, `DIRECT`, `COLI_CUDA_PIPE`, `COLI_CUDA_ASYNC`).

---

## 7. Recommended product policy (concise)

1. **Native UI config = TOML** at `~/.config/colibri/native-ui.toml` (already true). Keep that as the only write path for shell prefs.
2. **Do not** claim JSON prefs compatibility is needed for native UI unless a real `native-ui.json` user base appears.
3. **Engine runtime** stays env + `ColibriConfig`; file persistence is optional future, separate from UI prefs.
4. **Model path** is not a "config format": it is a directory path resolved by env → prefs → store.
5. **HF and tune JSON stay JSON.**
6. If dual-format is ever added for a **new** host settings file: **prefer TOML on read if both exist; always write TOML; env overrides file.**

---

## 8. Code anchors

| Concern | Path |
|---------|------|
| Native prefs I/O | `crates/colibri-native/src/prefs.rs` |
| Prefs at launch / save | `crates/colibri-native/src/main.rs` |
| Wizard prefs helpers | `crates/colibri-native/src/wizard.rs` |
| Sys typed config | `crates/colibri-sys/src/config.rs` |
| Model store paths | `crates/colibri-sys/src/paths.rs` |
| Registry scan | `crates/colibri-sys/src/model/registry.rs` |
| Tune JSON | `c/autotune.py`, `docs/tuning.md` |
| Env / CLI surface | `docs/ENVIRONMENT.md`, `docs/SETTINGS.md` |
| Native fidelity row for TOML prefs | `crates/colibri-native/docs/fidelity.md` |
| Prior prefs impl report | `.agents/reports/impl-native-prefs-toml.md` |

---

## 9. Gaps / residual (informational only)

- No `native-ui.json` reader (and no need for current users).
- No `ColibriConfig` file load/save in sys (by design).
- Tune profiles JSON not ported to Rust.
- Web localStorage and native TOML remain separate worlds.
- Stale note in older recon about missing `toml` crate dependency for native.
