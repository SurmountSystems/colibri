# Recon: colibri workspace layout, deps, docs, Grok harness gaps

**Date:** 2026-08-10
**Scope:** read-only inventory for a docs/edition/deps polish pass
**Repo:** `/home/hunter/Projects/surmount/colibri`
**Do not treat this file as product docs.**

---

## 1. Workspace layout

### Root workspace

| Item | Value | Cite |
|------|--------|------|
| Workspace manifest | `/home/hunter/Projects/surmount/colibri/Cargo.toml` | L1–L5 |
| Resolver | `"2"` | `Cargo.toml:2` |
| Members | **only** `crates/colibri-sys` | `Cargo.toml:3` |
| Explicit non-member | Desktop Tauri package kept **outside** the workspace so identity/edition/MSRV are not forced | `Cargo.toml:5–7` |

```1:7:/home/hunter/Projects/surmount/colibri/Cargo.toml
[workspace]
resolver = "2"
members = ["crates/colibri-sys"]

# Desktop (Tauri) stays a separate package under desktop/src-tauri so its
# identity and edition/MSRV are not forced by this workspace.
```

### Crates under `crates/`

| Crate path | Package name | Version | Edition | MSRV (`rust-version`) | In workspace? |
|------------|--------------|---------|---------|----------------------|---------------|
| `crates/colibri-sys/` | `colibri-sys` | `0.1.0` | **2024** | **1.85** | yes |
| *(none other)* | | | | | |

`crates/colibri-sys/Cargo.toml` package stanza:

```1:11:/home/hunter/Projects/surmount/colibri/crates/colibri-sys/Cargo.toml
[package]
name = "colibri-sys"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
description = "Embeddable Colibrì host crate: config, placement plan, model registry, engine process + serve mux, visual APIs, rkyv duplex stream"
license = "MIT"
readme = "README.md"
keywords = ["colibri", "llm", "inference", "moe"]
categories = ["api-bindings"]
repository = "https://github.com/SurmountSystems/colibri"
```

### Separate package (not workspace member)

| Path | Package | Version | Edition | MSRV |
|------|---------|---------|---------|------|
| `desktop/src-tauri/` | `colibri-desktop` | `0.1.0` | **2024** | **1.85** |

```1:17:/home/hunter/Projects/surmount/colibri/desktop/src-tauri/Cargo.toml
[package]
name = "colibri-desktop"
version = "0.1.0"
description = "Native desktop shell for the colibri inference engine"
authors = ["colibri contributors"]
edition = "2024"
rust-version = "1.85"
...
[dependencies]
tauri = { version = "2.11.5", features = [] }
```

Own lockfile: `desktop/src-tauri/Cargo.lock` (not the root workspace lock).

### Features (`colibri-sys`)

| Feature | Default | Gates | Cite |
|---------|---------|-------|------|
| `runtime` | on | empty feature (process spawn + serve mux) | `Cargo.toml:14–16` |
| `stream` | on | `dep:rkyv`, `dep:rancor` | `Cargo.toml:17–18` |
| `tokio` | on | `dep:tokio` + requires `runtime` | `Cargo.toml:19–20` |
| `install` | off | `dep:hf-hub`, `dep:indicatif` | `Cargo.toml:21–22` |
| `ffi` | off | stub only | `Cargo.toml:23–24` |

Examples: `embed_chat` (needs `runtime`+`tokio`), `plan_probe` (no features) — `Cargo.toml:67–73`.

### Project agent law files

| File | Present? |
|------|----------|
| `AGENTS.md` / `Agents.md` / `Claude.md` / `AGENT.md` under repo | **No** |
| `.agents/reports/` | Yes (this recon and prior impl/explore reports) |

### Non-Rust surface (context only)

Large C engine tree `c/`, Python host (`c/openai_server.py`, `c/doctor.py`, …), `web/` (Vite React), `desktop/` (Tauri shell over web), `docs/` (product C/CLI docs). Root `README.md` is the C product story; **no mention of `colibri-sys`** (grep of root `README.md` for `colibri-sys` / Rust host: empty).

---

## 2. Dependencies and resolved versions

### `colibri-sys` direct `[dependencies]`

Source: `crates/colibri-sys/Cargo.toml:26–52`.
Resolved: root `Cargo.lock` (version format 4).

| Crate | Cargo.toml req | Optional / feature | Lock version | Lock cite |
|-------|----------------|--------------------|--------------|-----------|
| **thiserror** | `"2"` | always | **2.0.19** | `Cargo.lock` ~1510–1513 |
| **serde** | `"1"`, features `derive` | always | **1.0.229** | ~1306–1309 |
| **serde_json** | `"1"` | always | **1.0.151** | ~1336–1339 |
| **regex** | `"1"` | always | **1.13.1** | ~1083–1086 |
| **hex** | `"0.4"` | always | **0.4.3** | ~432–435 |
| **tracing** | `"0.1"` | always | **0.1.44** | ~1661–1664 |
| **parking_lot** | `"0.12"` | always | **0.12.5** | ~910–913 |
| **bytes** | `"1"` | always | **1.12.1** | ~74–77 |
| **rkyv** | `"0.8"`, features `bytecheck` | `stream` | **0.8.17** | ~1178–1181 |
| **rancor** | `"0.1"` | `stream` | **0.1.2** | ~1025–1028 |
| **tokio** | `"1"`, features `io-util,process,sync,rt,macros,time` | `tokio` | **1.53.1** | ~1555–1558 |
| **hf-hub** | `"0.4"` | `install` | **0.4.3** | ~438–441 |
| **indicatif** | `"0.17"` | `install` | **0.17.11** | ~691–694 |

Exact names for `cargo search` / crates.io:

```
thiserror serde serde_json regex hex tracing parking_lot bytes rkyv rancor tokio hf-hub indicatif
```

### `colibri-sys` `[dev-dependencies]`

| Crate | Cargo.toml req | Lock version |
|-------|----------------|--------------|
| **tempfile** | `"3"` | **3.27.0** (`Cargo.lock` ~1497–1500) |
| **tokio** | `"1"`, features include `rt-multi-thread` (+ same as prod) | **1.53.1** (same package entry) |

```
tempfile tokio
```

### `colibri-desktop` direct deps

| Crate | Role | Cargo.toml | Lock (`desktop/src-tauri/Cargo.lock`) |
|-------|------|------------|----------------------------------------|
| **tauri** | runtime | `2.11.5` | **2.11.5** |
| **tauri-build** | build-dep | `2.6.3` | **2.6.3** |

```
tauri tauri-build
```

Desktop intentionally does **not** depend on `colibri-sys` (crate README residual: “Desktop dep deferred”; `desktop/README.md:22–24` defers engine process management).

### Transitive note (not exhaustive)

Root lock pulls the usual graph under tokio, rkyv, hf-hub, etc. Polish pass should `cargo update -p <name>` / `cargo search` on the **direct** list above, not invent pins for every transitive.

### Publish / crates.io status

- No `publish = false` in `colibri-sys` Cargo.toml (default allows publish).
- No evidence of a published crates.io release in-repo; version is `0.1.0`.
- rustdoc sets `html_root_url = "https://docs.rs/colibri-sys"` (`lib.rs:77`) even though docs.rs may not exist yet → polish risk (broken docs badge / wrong assumption).

---

## 3. Existing docs inventory

### Root / product

| Path | Role |
|------|------|
| `/home/hunter/Projects/surmount/colibri/README.md` | Product landing (C engines, web UI). **No `colibri-sys` section.** |
| `README.zh-CN.md`, `README.zh-TW.md`, `README.it.md` | Locales |
| `CONTRIBUTING.md`, `CHANGELOG.md`, `SUMMARY.md`, `LICENSE` | Project meta |
| `GPU_BACKENDS.md` | Backend overview |
| `docs/*` | Product: `api.md`, `SETTINGS.md`, `ENVIRONMENT.md`, `serve_protocol.md`, family docs, experiments, `MAINTAINING-DOCS.md`, etc. |
| `desktop/README.md` | Tauri shell; engine not bundled |
| `web/README.md` | Frontend |
| `docker/README.md`, `docker/README.IT.md` | Containers |
| `c/tools/README.md`, `c/tests/README_efficiency.md` | C tooling |

### `colibri-sys` human docs

| Path | Purpose | Notes |
|------|---------|--------|
| `crates/colibri-sys/README.md` | Crate overview, features, quick start, residual | Points at user guide §15 for Grok harness (`README.md:16`) |
| `crates/colibri-sys/docs/README.md` | Doc index + machine-local absolute paths | L9 harness callout |
| `crates/colibri-sys/docs/user-guide.md` | Full user guide (§1 depend … §15 Grok harness) | **Primary path-dep + harness SoT** |
| `crates/colibri-sys/docs/ffi-phase-d.md` | Future in-process FFI design (proposed only) | Status: design only |

### Crate-level / module rustdocs

| Path | Coverage |
|------|----------|
| `src/lib.rs` L1–L75 | Crate-level `//!` doc: features, quick start, `MachineInfo` field table, Python origins |
| `src/paths.rs` | Discoverable model store precedence |
| `src/probe.rs` | Hardware probe / SSD grammar |
| `src/plan.rs` | Placement plan v2 |
| `src/doctor.rs` | Doctor standard + deep |
| `src/model/mod.rs` | Inspect / family / registry |
| `src/engine/mod.rs` | Process embed + serve mux |
| `src/stream/mod.rs` | rkyv duplex |
| `src/visual.rs` | Telemetry snapshots |
| `src/config.rs`, `error.rs` | (module docs present or minimal; inventory via rustdoc) |

### Agent reports (not user API)

Under `.agents/reports/`: `impl-colibri-sys.md`, `impl-colibri-sys-followups.md`, explore-*, process-mop-*, verify-*. User guide explicitly demotes them (`user-guide.md:539`).

### Generated rustdoc on disk

`target/doc/colibri_sys/index.html` referenced by crate docs (exists after prior `cargo doc` in this tree).

---

## 4. What Grok Build local-path / harness guidance already exists

### In-repo (`colibri-sys` user guide)

**§1 Add the dependency** (`user-guide.md:42–83`):

- Absolute path dep example for this machine:
  `colibri-sys = { path = "/home/hunter/Projects/surmount/colibri/crates/colibri-sys" }` (L46–49)
- Workspace-member pattern: add app to members + relative `path = "../colibri-sys"` (L51–61)
- Feature selection: default-features false; `features = ["install"]` (L73–83)

**§15 Grok Build–style completion harness** (`user-guide.md:543–740`):

| Subsection | Content |
|------------|---------|
| Intro | Colibrì as OpenAI Chat Completions for Grok `api_backend = "chat_completions"` |
| Path A | Product Python `c/openai_server.py` / `coli serve` + sample `~/.grok/config.toml` `[model.colibri-local]` block (L559–605) |
| Path B | Rust harness binary on top of `colibri-sys` (HTTP, not link Grok → crate) (L609–708) |
| Layering | Grok → HTTP → harness (axum/hyper/…) → `EngineHandle` |
| Contract table | `/v1/chat/completions`, `/v1/models`, SSE, auth, errors (L627–638) |
| Lifecycle | probe → plan → doctor → start → generate → stop (L667–676) |
| Checklist | new binary crate + path dep absolute path again (L686–695) |
| Pointers | Host Grok docs: `~/.grok/docs/user-guide/11-custom-models.md`; config `~/.grok/config.toml` (L24–25, L547–550, L739–740) |

**Explicit guidance already present:**

- Prefer **HTTP custom model**, do **not** link Grok Build to `colibri-sys` unless in-tree Grok feature (`user-guide.md:625`).
- Path dependency examples for **external** consumers (absolute + relative workspace).
- Features for harness: default `runtime`+`tokio`; optional `install` (L690–691).

### Host Grok user-guide (outside repo)

| File | Relevance |
|------|-----------|
| `/home/hunter/.grok/docs/user-guide/11-custom-models.md` | How to register HTTP models (`base_url`, `api_backend`, credentials). **No Cargo / path-dep / colibri-sys text** (grep empty). |
| Other user-guide chapters | “Harness” mostly means Claude/Cursor compat (`05-configuration.md`), not Cargo path deps. |

**Conclusion:** Local-path + harness guidance for Colibrì lives almost entirely in **`crates/colibri-sys/docs/user-guide.md` §1 and §15**, not in Grok’s global user-guide. Grok side only documents the HTTP custom-model contract.

---

## 5. Gaps for a polish pass

### 5.1 Missing or thin “local path without crates.io” section

| Gap | Detail |
|-----|--------|
| No dedicated section title | Path dep is under “§1 Add the dependency” but does **not** say in one place: *crate is not (yet) on crates.io; path/git only; do not write `colibri-sys = "0.1"` expecting registry.* |
| Absolute machine paths committed | Many docs hardcode `/home/hunter/Projects/surmount/colibri/...` (user-guide paths table L11–25, README L12–18, docs/README L5–12). Portable clones need relative + “replace with your clone” pattern. |
| `git` dependency form missing | No `colibri-sys = { git = "…", rev = "…" }` example for out-of-tree consumers. |
| crates.io / docs.rs mismatch | `html_root_url` points at docs.rs (`lib.rs:77`) while package may be unpublished; polish should either document “local rustdoc only” or drop/adjust the attribute until publish. |
| Root product README silent | Root `README.md` does not link `crates/colibri-sys` for embedders (inventory gap for discoverability). |

Suggested polish slice (implementer, not done here): short § or README subsection:

1. Not on crates.io (or status if published later).
2. Path dep (relative from your crate).
3. Optional git dep.
4. Feature matrix.
5. Engine binary still required (`c/colibri` build).
6. Link to §15 for Grok HTTP harness (not in-process link).

### 5.2 Edition / MSRV

| Package | Edition | Status |
|---------|---------|--------|
| `colibri-sys` | 2024 | **Already target edition** — no edition bump needed |
| `colibri-desktop` | 2024 | Same |
| Workspace | N/A (edition per package) | Comment explains desktop isolation |

No `edition != 2024` crates found among the three manifests.

### 5.3 Stale / loose dependency specs (candidates to check on crates.io)

Semver-loose reqs are normal; polish = compare lock to latest major/minor, not necessarily pin.

| Name | Req | Locked | Action for implementer |
|------|-----|--------|------------------------|
| thiserror | 2 | 2.0.19 | `cargo search thiserror` |
| serde | 1 | 1.0.229 | check |
| serde_json | 1 | 1.0.151 | check |
| regex | 1 | 1.13.1 | check |
| hex | 0.4 | 0.4.3 | check |
| tracing | 0.1 | 0.1.44 | check |
| parking_lot | 0.12 | 0.12.5 | check |
| bytes | 1 | 1.12.1 | check |
| rkyv | 0.8 | 0.8.17 | check 0.8.x / 0.9 if intentional |
| rancor | 0.1 | 0.1.2 | keep aligned with rkyv |
| tokio | 1 | 1.53.1 | check; note dev uses `rt-multi-thread` |
| hf-hub | 0.4 | 0.4.3 | check 0.4 / 0.5 |
| indicatif | 0.17 | 0.17.11 | check 0.17 / 0.18 |
| tempfile | 3 | 3.27.0 | check |
| tauri | 2.11.5 | pinned exact | check Tauri 2.x latest |
| tauri-build | 2.6.3 | pinned exact | keep in sync with tauri |

No `path =` dependencies in `colibri-sys` or desktop manifests (all crates.io registry).

### 5.4 Broken / inaccurate doc links and API claims

| Issue | Cite | Severity |
|-------|------|----------|
| User guide §5 imports `exit_code` from crate root: `use colibri_sys::{…, exit_code}` | `user-guide.md:252` | **API doc bug**: `exit_code` is `pub fn` in `doctor.rs:1120` but **not** re-exported in `lib.rs:110` (`pub use doctor::{DoctorCheck, DoctorOptions, DoctorReport, run_doctor}` only). Fix: re-export **or** document `colibri_sys::doctor::exit_code`. |
| `html_root_url` → docs.rs | `lib.rs:77` | May 404 if unpublished |
| Repo URL Surmount vs product README JustVugg | `Cargo.toml:11` vs root README badges | Org/fork identity polish if public |
| Relative product links from crate docs | e.g. `docs/serve_protocol.md` from crate README L136 | Correct from **repo root** only; fine if README assumes clone root |
| Machine-local `file://` URLs | docs/README, user-guide | Break on other machines (document as examples) |
| Desktop residual “not wired” | crate README L150; user-guide L527 | Accurate; keep unless wiring lands |

### 5.5 Incomplete inventory docs

| Gap | Notes |
|-----|--------|
| Root README | No Rust embed / `colibri-sys` pointer |
| No `AGENTS.md` | Process lives in host `~/.grok/AGENTS.md` only for this tree |
| Dependency inventory | No `docs/DEPS.md` or crate “Dependencies” table with lock versions |
| Workspace membership | Only one member; desktop/docs should keep stating “separate package + lock” (desktop README does) |
| Publish checklist | License/readme/keywords present; no authors field on `colibri-sys`; no `exclude` for huge `c/` (not needed for path consumers) |
| §15 Path B | Checklist names hypothetical `crates/colibri-openai` — crate does not exist yet (expected residual, not a broken link) |
| Grok global guide | No reverse link from `11-custom-models.md` to colibri (optional; host-doc scope) |

### 5.6 What is already in good shape (do not thrash)

- Edition **2024** + MSRV **1.85** already set on both Rust packages.
- User guide is substantial (probe/plan/doctor/embed/stream/install + Grok §15).
- Feature table repeated consistently (README, lib.rs, user-guide, docs/README).
- Path A/B harness design is clear: HTTP first; no false claim of in-process Grok link.
- `ffi` stub + Phase D doc correctly marked design-only.

---

## 6. Concrete polish checklist for implementer

1. **Docs: local path without crates.io** — expand §1 (or new short section) with: unpublished status, relative path, optional git dep, “no registry version yet”, engine binary still required. Soften or dual-write absolute `/home/hunter/...` paths.
2. **Docs: root discoverability** — one paragraph + link from root `README.md` → `crates/colibri-sys/README.md`.
3. **Docs: fix `exit_code` import** — re-export from `lib.rs` **or** fix user-guide example (`user-guide.md:252` vs `lib.rs:110`).
4. **Docs: docs.rs / html_root_url** — align with actual publish state.
5. **Deps: audit** — run `cargo search` / crates.io on the exact names in §2; update Cargo.toml ranges only with intentional bumps; re-lock with workspace `cargo update` and desktop lock separately.
6. **Edition** — no bump required (already 2024).
7. **Optional** — `publish = false` until first release; authors field; CONTRIBUTING note for Rust host tests (`cargo test -p colibri-sys`).

---

## 7. File index (absolute paths touched by this recon)

```
/home/hunter/Projects/surmount/colibri/Cargo.toml
/home/hunter/Projects/surmount/colibri/Cargo.lock
/home/hunter/Projects/surmount/colibri/README.md
/home/hunter/Projects/surmount/colibri/crates/colibri-sys/Cargo.toml
/home/hunter/Projects/surmount/colibri/crates/colibri-sys/README.md
/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/README.md
/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/user-guide.md
/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/ffi-phase-d.md
/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/lib.rs
/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/doctor.rs
/home/hunter/Projects/surmount/colibri/desktop/src-tauri/Cargo.toml
/home/hunter/Projects/surmount/colibri/desktop/src-tauri/Cargo.lock
/home/hunter/Projects/surmount/colibri/desktop/README.md
/home/hunter/.grok/docs/user-guide/11-custom-models.md
```

---

## 8. One-line summary

Workspace is **single-member** (`colibri-sys` **0.1.0 / edition 2024 / MSRV 1.85**); desktop is a **separate** 2024 Tauri package; direct deps are the 13+2 names in §2 with lock versions listed; human docs for path deps + Grok harness already live in **user-guide §1 + §15**, but polish still needs a clear **not-on-crates.io** story, **portable path** wording, **root README** discoverability, **`exit_code` re-export vs docs**, and a **dep freshness** pass against crates.io.
