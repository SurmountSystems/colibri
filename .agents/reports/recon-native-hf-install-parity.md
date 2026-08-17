# Recon: native HF install vs Tauri/React desktop

**Date:** 2026-08-10
**Scope:** read-only; no product edits.

## Short answer

**Native HF install is not a port of a Tauri/React install screen.** The Tauri shell + React SPA have **no model install UI at all**. Native owns this surface and wires it to `colibri-sys` `model::install` (feature `install`). Screenshot matches that native form.

The field showing **`1`** is **min free GB** (default `DEFAULT_INSTALL_MIN_FREE_BYTES / GB` = 1), placeholder `min free GB (0 = off)`. Not concurrency or shard count.

## Surfaces compared

| Surface | HF install? |
|---------|-------------|
| `desktop/` Tauri | No. Thin webview; README says it does not download models. |
| `web/` React SPA | No install/repo/revision/dest fields; chat against OpenAI HTTP only. |
| `colibri-native` GPUI | Yes (`feature = "install"`, default on). |
| `colibri-sys` | Shared API both hosts *could* use; only native UI does. |

## Native form (matches screenshot)

From `main.rs` + `host.rs`:

1. **HF repo id (owner/name)** → `repo_input`
2. **revision (optional)** → `revision_input`
3. **dest under store (optional)** → `dest_input` (empty → `store/owner__name`; must stay under store; `..` rejected)
4. **min free GB** → `min_free_input`, default **`1`**
5. **Space line** → `dest {path} · free X.XX GB · min Y.Y GB` (or `min free: off`)
6. **Install model** / **Cancel**
7. Status: **Ready to install** → progress `[{phase}] {message}` (+ bytes/files) → done/error

Behavior: validate form → hard free-space gate → background `install_model_cancellable` → cancel requests kill CLI child / stop hub loop → success sets model path field.

## colibri-sys install API (shared library)

`crates/colibri-sys/src/model/install.rs` (feature `install`):

- **Sources:** `HuggingFace { repo_id, revision, allow_patterns }` · `LocalPath`
- **Options:** `dest`, `prefer_cli`, `min_free_bytes`, `inspect_after`, `register`
- **API:** `install_model` / `install_model_cancellable` + `InstallCancel`
- **Progress:** phase, message, bytes, file, files_done/total
- **Gate:** `ensure_space` before download; incomplete-download detection; optional inspect

Native host mapping:

- HF only (`allow_patterns: None`)
- `prefer_cli: true`, `inspect_after: true`, `register: false`
- UI min-GB → `min_free_bytes`; empty field → default 1 GiB; `0` → gate off

## Parity vs Tauri/React

| Concern | Tauri/React | Native | Verdict |
|---------|-------------|--------|---------|
| Install form | Missing | Full | **Native better** (new capability) |
| Progress / cancel | N/A | Text progress + cancel | Native only |
| Dest under store | N/A | Yes + path rules | Native only |
| Free space gate | N/A | Default 1 GB, editable | Native only |
| Shared sys API | Not used by desktop | Used | Library shared; UI not |
| HTTP chat / settings | SPA primary path | Separate native chat path | Different products |

**Fidelity claim in `colibri-native/docs/fidelity.md` (row “Model install (HF) = done”)** is about sys API + GPUI wiring, **not** pixel/feature equality with Tauri. That is accurate: desktop never had this panel.

## Gaps / notes (native vs full sys API, not vs Tauri)

- No UI for `allow_patterns`, `LocalPath`, or `register: true`.
- Idle space line refreshes against **model store** root; start-install rechecks against **resolved dest**.
- Progress is a scrollable status string, not a bar.
- Placeholder on min-free field is easy to miss → screenshot “1” looks mysterious without the placeholder text.

## Conclusion

User screenshot is **faithful to colibri-native**, not a degraded copy of Tauri/React. Tauri/React intentionally defer model management; native is **ahead** on HF install. The unclear **`1`** is min free GB.
