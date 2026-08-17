# impl: colibri-sys install path → hf-hub 1.x

**Date:** 2026-08-10
**Repo:** `/home/hunter/Projects/surmount/colibri`
**Scope:** `crates/colibri-sys` install feature only. Prefer-cli path unchanged.

---

## Summary

Ported the optional `hf-hub` fallback in `download_via_hf_hub` from **0.4** (`Api` / `Repo` / `get`) to **1.0** (`HFClientSync` + `blocking`, recursive `list_tree`, selective `download_file` into `opts.dest` via `local_dir`). Removed the Cargo.toml “stay on 0.4” pin. Menhera-cooldown has `hf-hub 1.0.0`; no crates.io fallback needed.

---

## Before / after API

### Cargo

| | Before | After |
|--|--------|-------|
| dep | `hf-hub = { version = "0.4", optional = true }` | `hf-hub = { version = "1", optional = true, features = ["blocking"] }` |
| comment | “1.x is a full API rewrite; stay on 0.4 until install is ported” | “hf-hub 1.x blocking sync client” |

Resolved: `hf-hub v0.4.3` → `v1.0.0` (menhera-cooldown index).

### `download_via_hf_hub` call graph

| Step | 0.4 | 1.x |
|------|-----|-----|
| Client | `hf_hub::api::sync::Api::new()` | `HFClientSync::new()` |
| Repo id | `api.model(repo_id)` or `Repo::with_revision(..., Model, rev)` + `api.repo(r)` | `split_id(repo_id)` → `(owner, name)`; reject empty owner/name; `client.model(owner, name)` |
| List files | `repo.info()` → `siblings[].rfilename` | `repo.list_tree().recursive(true).maybe_revision(rev).send()` → `RepoTreeEntry::File { path, .. }` only |
| Allow patterns | host `filter_by_allow_patterns` | same host filter (unchanged) |
| Download | `repo.get(name)` → cache `PathBuf` + `fs::copy` into `dest` | `repo.download_file().filename(name).local_dir(dest).maybe_revision(rev).send()` (materializes under `dest` with repo path) |
| Errors | `Error::Install(e.to_string())` / `hf-hub get {name}: …` | `Error::Install(...)` for client, list_tree, download_file, invalid id |

Prefer-cli branch (`SystemHfCli` / `HfCliRunner` / `hf download --local-dir`) **unchanged**.

### Docs

- `docs/user-guide.md`: “hf-hub sibling snapshot” → “hf-hub 1.x (`list_tree` + selective `download_file`)”
- Module docs on `install.rs`: siblings/`info` wording → recursive tree list

No other product files referenced `hf-hub 0.4`.

---

## Commands and exit codes

| Command | Exit | Notes |
|---------|------|--------|
| `cargo fmt -p colibri-sys` | **0** | |
| `cargo clippy -p colibri-sys --all-targets --features install -- -D warnings` | **0** | |
| `cargo test -p colibri-sys --features install` | **0** | lib: 65 passed, 1 ignored; integration + doctests green |

Earlier mid-port: package briefly failed compile/clippy on concurrent untracked `engine/duplex.rs` (edition-2024 reserved `gen`, private `stream::frame` import, clippy lints). That file was fixed outside this install slice; final verify is green with default features + `install`.

---

## Files touched (this port)

| Path | Change |
|------|--------|
| `crates/colibri-sys/Cargo.toml` | hf-hub `1` + `blocking`; drop 0.4 stay pin |
| `crates/colibri-sys/src/model/install.rs` | rewrite `download_via_hf_hub`; module note; install notes string |
| `crates/colibri-sys/docs/user-guide.md` | install bullet wording |
| `Cargo.lock` | lock update via cargo (hf-hub 1.0.0 + transitive) |

---

## Evidence notes

- Registry: host default is menhera-cooldown; source already present at
  `~/.cargo/registry/src/index.crates.menhera.org-*/hf-hub-1.0.0/`.
- Upstream 1.x docs (docs.rs/hf-hub/1.0.0, accessed 2026-08-10): `HFClientSync`, `split_id`, blocking `list_tree` / `download_file` with `local_dir` / `revision`.
- Live network test `live_hf_snapshot_tiny` remains `#[ignore]`; unit install tests use mocked CLI and do not hit the hub.

No git commit (operator-owned).
