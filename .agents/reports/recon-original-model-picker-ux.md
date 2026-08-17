# Recon: original model picker / HF install / registry UX

**Repo:** `/home/hunter/Projects/surmount/colibri`
**Date:** 2026-08-11
**Scope:** Original web SPA (`web/`), Tauri desktop shell (`desktop/`), product docs/README, vs what native already invented.
**Question:** How does *original* product present model choice, Hugging Face install, and registry? What labels and curated lists exist for native reuse?

---

## Executive summary

| Surface | Model choice | HF install | Registry / store scan |
|---------|--------------|------------|------------------------|
| **Web SPA** (`web/`) | OpenAI-style **dropdown of API model ids** after **Probe server** | **None** | **None** (server already running; no local store) |
| **Tauri desktop** (`desktop/`) | Same SPA only | Explicitly **out of scope** (no download, no engine) | None |
| **Docs / README / Docker** | Path + `COLI_MODEL`; curated **HF repo table** | **CLI / browser**: `hf_download`, HF web page, `coli convert` | N/A (filesystem path) |
| **colibri-native** (GPUI) | Folder path + scan + install | Freeform HF form (repo / rev / dest / min free) | Scan model store for `config.json` dirs |

**Bottom line for native:** the original SPA is a **remote client** of an already-served engine. It never downloads weights and never browses a local model store. Curated HF repos and install copy live in **README / quickstart / docker**, not in the React UI. Native’s freeform install + store scan are **new product surface**; SPA labels worth reusing are mostly **Model / ACTIVE MODEL / Inference** and the **API model-id defaults**, not a curated picker.

---

## 1. Web SPA (`web/`)

### Controls

| Control | Where | Behavior |
|---------|-------|----------|
| **API endpoint** | Sidebar → Connection | Default `http://127.0.0.1:8000/v1` (or same-origin `/v1` when served by engine) |
| **API key** | Connection | Optional; memory-only |
| **Probe server** | Connection | `GET …/v1/models` → fills model list; also loads `/health` |
| **Model** `<select>` | Sidebar → **Inference** | Options = ids from last successful probe; if empty list, single option = current value |
| **ACTIVE MODEL** | Top bar | Display-only of selected id |

No free-text model path, no “Install”, no “Scan models”, no HF repo field.

### Source

- UI: [`web/src/App.tsx`](../../web/src/App.tsx) — default model `"glm-5.2-colibri"`; select at Inference section; `listModels` on connect.
- API: [`web/src/lib/api.ts`](../../web/src/lib/api.ts) — `listModels` → `GET {base}/models` (OpenAI list).
- Persist: [`web/src/lib/storage.ts`](../../web/src/lib/storage.ts) — `colibri.model`, `colibri.baseUrl` (not API key).
- README: [`web/README.md`](../../web/README.md) — “use **Probe server** to load its models.”

### Labels (English, SPA i18n)

From [`web/src/i18n/en.ts`](../../web/src/i18n/en.ts):

| Key | String |
|-----|--------|
| `sidebar.inference` | **Inference** |
| `sidebar.model` | **Model** |
| `topbar.activeModel` | **ACTIVE MODEL** |
| `sidebar.probe` | **Probe server** |
| `sidebar.connection` | Connection |
| `sidebar.endpoint` | API endpoint |
| `status.connected` | Engine reachable |
| `status.notConnected` | Not connected |
| `brand.tagline` | local giant, tiny footprint |
| `hero.title` | COLIBRÌ ENGINE |
| `hero.subtitle` / `tagline` | Ask the giant. / Keep the machine yours. |

Locales: `en`, `it`, `zh-CN`, `zh-TW` (same keys; model label = “Model” / “Modello” / “模型”).

### Curated list in UI?

**No.** The dropdown is **only** whatever the server returns from `/v1/models`. There is no hard-coded HF catalog in React.

Fallback when not yet probed: default / last-stored string **`glm-5.2-colibri`**.

### Server-side model ids (what the SPA typically shows)

[`c/openai_server.py`](../../c/openai_server.py) advertises **one** id (not multi-model registry). Auto defaults by arch:

| Architecture | Default `--model-id` / `$COLI_MODEL_ID` |
|--------------|----------------------------------------|
| (default / GLM) | `glm-5.2-colibri` |
| inkling | `inkling-colibri` |
| kimi | `kimi-k3-colibri` |
| deepseek_v4 | `deepseek-v4-colibri` |

Documented in [`docs/SETTINGS.md`](../../docs/SETTINGS.md) (`--model-id`, default `glm-5.2-colibri`).

These are **API display names**, not Hugging Face repo ids and not local folder paths.

---

## 2. Tauri desktop (`desktop/`)

[`desktop/README.md`](../../desktop/README.md):

- Shell around **the same** `web/` SPA.
- Does **not** start the engine, download models, or add filesystem install.
- Model remains “external, user-selected” (path chosen outside this shell).

No separate picker UX beyond the SPA dropdown.

---

## 3. Docs / README: where install and curated models actually live

### Primary install story (not SPA)

| Path | Content |
|------|---------|
| [`README.md`](../../README.md) § “Get the model” | Curated HF table; prefer gs64 + int8 MTP; size ~372 GB; `coli convert` alternative |
| [`docs/quickstart.md`](../../docs/quickstart.md) §3 | Same GLM HF URL; convert path; point `COLI_MODEL` |
| [`docker/README.md`](../../docker/README.md) | `hf_download <repo> --local-dir .`; manual HF page |
| [`docs/SETTINGS.md`](../../docs/SETTINGS.md) | Flags for `--model`, `--model-id`, convert `--repo` default `zai-org/GLM-5.2-FP8` |

### Curated HF / family list (docs, not UI)

From README “Other supported models” table (authoritative **product catalog** for install presets if native wants suggestions):

| Family | Weights (HF or notes) | Rough disk |
|--------|----------------------|------------|
| **GLM-5.2** (reference) | `mastouri/GLM-5.2-colibri-int4-g64-with-int8-mtp` | ~372 GB |
| **Inkling** | `nbeerbower/Inkling-colibri-int4` | ~469 GB |
| **Kimi K3** | `moonshotai/Kimi-K3` (native MXFP4 experts; no convert) | ~1.6 TB |
| **DeepSeek V4 Flash** | official sharded checkpoint (fp4 / fp8) | ~167 GB |
| **OLMoE** | convert via `c/tools/convert_olmoe_merged.py` | ~4 GB |

**Hard warning (docs copy):** use **gs64** GLM container with **int8 MTP**, not older per-row int4 mirrors (`mateogrgic/…`, `jlnsrk/…`).

**CLI install freeform:**

```bash
hf_download mastouri/GLM-5.2-colibri-int4-g64-with-int8-mtp --local-dir .
# or
./coli convert --model /path/to/dest
```

Docker prose: “Download the model”, “Using Python (recommended)”, “Without Python… download manually from Hugging Face.”

There is **no** in-docs “model registry” UI concept; “registry” in docs often means **quant format registry** (`docs/FORMATS.md`), not a model picker.

---

## 4. Native (for contrast / reuse target)

[`crates/colibri-native/src/i18n.rs`](../../crates/colibri-native/src/i18n.rs) already owns install/registry copy that the SPA never had:

| Key | English |
|-----|---------|
| `rail.install` / `tools.install` | **Download model** |
| `rail.installBtn` | **Install model** |
| `rail.installing` | Installing… |
| `rail.scanModels` / `tools.scan` | **Scan models** |
| `rail.modelPath` / `tools.modelPath` | **Model folder** / Model folder path |
| `rail.modelUnset` | No model selected |
| `install.repo` | **Hugging Face repo (owner/name)** |
| `install.revision` | Revision (optional) |
| `install.dest` | Folder name under store (optional) |
| `install.minFree` | Min free disk (GB) |
| `wizard.model.title` | **Choose a model** |
| `wizard.model.body` | Paste a folder path, pick from the model store, or download from Hugging Face. |
| `wizard.readiness.scan` | Scan for models |
| `wizard.readiness.install` | Install a model |

Placeholders in code: `HF repo id (owner/name)`, `revision (optional)`.

**No curated HF preset list** in native UI either: freeform repo string only. Docs table is the natural source if native adds “suggested models.”

SPA strings **already mirrored** in native where chat shell matches: `topbar.activeModel` = ACTIVE MODEL; hero title/subtitle/tagline; brand tagline.

---

## 5. Strings / patterns to reuse for native

### High value (match original product language)

| Reuse | Why |
|-------|-----|
| **Model** (section field) | SPA `sidebar.model`; users already see it for “which model” |
| **ACTIVE MODEL** | SPA top bar; native already uses same key |
| **Inference** (or native **Chat settings**) | SPA groups model with temp / max tokens; native rail uses “Chat settings” for that cluster |
| **Probe server** / **Engine reachable** | Only if native still talks to a remote OpenAI gateway; less relevant for in-process engine |
| API ids: **`glm-5.2-colibri`**, **`inkling-colibri`**, **`kimi-k3-colibri`**, **`deepseek-v4-colibri`** | What Grok / clients send as `model` when using serve; good secondary label next to folder path |
| Hero: **COLIBRÌ ENGINE**, **Ask the giant.**, **Keep the machine yours.** | SPA empty state; brand continuity |
| Docs catalog wording: **Download model**, **group-scaled (gs64)**, **int8 MTP head**, size callouts | Install success / warning copy |

### Do **not** invent as “original SPA behavior”

| Not in original SPA | |
|---------------------|--|
| Curated HF dropdown in the React UI | Docs only |
| Local model store scan | Native-only |
| Freeform HF install form | Native-only (plus CLI docs) |
| Multi-model simultaneous serve list | Server exposes one `model_id` |

### Suggested native preset labels (if adding curated install chips)

Map docs table → human short names + repo (not SPA, but original *product* copy):

1. **GLM-5.2 int4 (recommended)** → `mastouri/GLM-5.2-colibri-int4-g64-with-int8-mtp` (~372 GB)
2. **Inkling int4** → `nbeerbower/Inkling-colibri-int4` (~469 GB)
3. **Kimi K3** → `moonshotai/Kimi-K3` (large; no convert)
4. Freeform remains: **Hugging Face repo (owner/name)** (native install.repo)

Optional convert source default from SETTINGS: `zai-org/GLM-5.2-FP8` (FP8 source for `coli convert`, not the ready int4 container).

---

## 6. UX flow comparison (one glance)

```
Original SPA:
  User starts engine + serve outside UI
    → open web → set endpoint → Probe server
    → Model <select> shows server model id(s)
    → chat with that id

Original docs install:
  Download HF (curated URL or hf_download freeform) OR coli convert
    → COLI_MODEL=/path ./coli serve|web|chat

Native:
  Path paste | Scan store | freeform HF install into store
    → Doctor / plan → Start engine → chat
  (SPA model <select> not the centerpiece)
```

---

## 7. File index

| Path | Role |
|------|------|
| `web/src/App.tsx` | Model state, select, probe |
| `web/src/i18n/en.ts` (it, zh-*) | SPA labels |
| `web/src/lib/api.ts` | `listModels` |
| `web/README.md` | Probe-server instruction |
| `desktop/README.md` | Tauri = SPA only; no install |
| `README.md` / `docs/quickstart.md` | Curated HF + convert |
| `docker/README.md` | `hf_download` steps |
| `docs/SETTINGS.md` | `--model-id`, convert `--repo` |
| `c/openai_server.py` | Single model id defaults by arch |
| `crates/colibri-native/src/i18n.rs` | Native install/scan/wizard copy |

---

## 8. Gaps / open for product (not claimed decided)

- Whether native should show **API model-id** next to folder path for OpenAI client parity.
- Whether curated **preset chips** from the README table should appear above freeform HF (SPA never had this; docs imply recommended first).
- OLMoE / convert-from-FP8 as first-class install paths vs “weights already colibri-shaped.”

End of recon.
