# Impl report: colibri-sys docs polish, edition, deps

**Date:** 2026-08-10
**Repo:** `/home/hunter/Projects/surmount/colibri`
**Scope:** docs polish (path dep / not crates.io / Grok Build), edition confirm, dep freshness for `colibri-sys`

---

## Docs files touched + what changed

| File | Change |
|------|--------|
| `/home/hunter/Projects/surmount/colibri/README.md` | Repo layout line for `crates/colibri-sys/`; new **Rust embed host (`colibri-sys`)** subsection (process-first, not crates.io, links to crate README + user guide). No crates.io publish claim. |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/README.md` | Rewrote for dependents: is/is not, edition/MSRV, **not on crates.io**, absolute + relative + workspace + git path forms, features table, quick start (probe/plan/store/doctor), examples (`plan_probe`, `embed_chat`), Grok pointer, residual. |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/README.md` | Index with relative links + **Local development / absolute paths** note (`COLIBRI_ROOT` / this machine prefix); not-on-crates.io callout. |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/user-guide.md` | Expanded **§1**: not on crates.io, absolute/relative/`$COLIBRI_ROOT`/workspace/git, features, engine still required; new **§1.1 Grok Build local integration**; §5 `exit_code` remains crate-root import (now re-exported); §15 checklist uses the canonical path-dep TOML snippet. |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/ffi-phase-d.md` | Light polish: point at user guide for process embed / path dep; demote agent report as non-API. |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/lib.rs` | Crate-level note: path/git until published; **removed** premature `html_root_url` → docs.rs; **`pub use doctor::exit_code`**. |
| `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/Cargo.toml` | `indicatif` `"0.17"` → `"0.18"`; comment that `hf-hub` stays on `0.4` (1.x is a full API rewrite). |

---

## Edition confirmation

| Package | Path | Edition | MSRV | Action |
|---------|------|---------|------|--------|
| `colibri-sys` | `crates/colibri-sys` | **2024** | **1.85** | Already correct; no change |
| `colibri-desktop` | `desktop/src-tauri` | **2024** | **1.85** | Already correct; **deps deferred** (see below) |
| Workspace root | `Cargo.toml` | N/A (per package) | | Member: only `crates/colibri-sys` |

---

## Dep version before → after

Operator registry is **menhera-cooldown** (`~/.cargo/config.toml` replaces crates-io). Resolved “latest” means latest **available on that index** for the Cargo.toml range, not necessarily crates.io edge.

| Crate | Cargo.toml req | Lock before | Lock after | Notes |
|-------|----------------|-------------|------------|-------|
| thiserror | `"2"` | 2.0.19 | **2.0.19** | crates.io search shows 2.0.20; menhera max 2.0.19 |
| serde | `"1"` + derive | 1.0.229 | **1.0.229** | already current on index |
| serde_json | `"1"` | 1.0.151 | **1.0.151** | already current |
| regex | `"1"` | 1.13.1 | **1.13.1** | already current |
| hex | `"0.4"` | 0.4.3 | **0.4.3** | already current |
| tracing | `"0.1"` | 0.1.44 | **0.1.44** | already current |
| parking_lot | `"0.12"` | 0.12.5 | **0.12.5** | already current |
| bytes | `"1"` | 1.12.1 | **1.12.1** | already current |
| rkyv | `"0.8"` + bytecheck | 0.8.17 | **0.8.17** | search shows 0.8.18; menhera max 0.8.17 |
| rancor | `"0.1"` | 0.1.2 | **0.1.2** | search shows 0.1.3; menhera max 0.1.2 |
| tokio | `"1"` (listed features) | 1.53.1 | **1.53.1** | already current |
| hf-hub | `"0.4"` optional | 0.4.3 | **0.4.3** | **kept on 0.4** on purpose; 1.0.0 exists but rewrites `Api` → `HFClient` (install would need a port) |
| indicatif | `"0.17"` → **`"0.18"`** | 0.17.11 | **0.18.6** (direct); 0.17.11 remains via hf-hub | Cargo.toml range bumped |
| tempfile | `"3"` dev | 3.27.0 | **3.27.0** | already current |

`cargo update` note: only unchanged package “behind latest compatible” was **hf-hub 0.4.3 (available: 1.0.0)** by design.

---

## Commands + exit codes

| Command | Exit |
|---------|------|
| `cargo update -p thiserror -p rkyv -p rancor -p indicatif` | **0** (indicatif 0.18.6 + console/unit-prefix; others already max on index) |
| `cargo update` / `cargo update -p colibri-sys` | **0** |
| `cargo fmt -p colibri-sys` | **0** |
| `cargo clippy -p colibri-sys --all-targets --features install -- -D warnings` | **0** |
| `cargo test -p colibri-sys` | **0** (47 unit + 2 plan + 1 ssd + 1 doctest; 1 ignored real engine) |
| `cargo test -p colibri-sys --features install` | **0** (54 unit with install; 1 ignored live HF) |

Precise bumps (`--precise 0.8.18` / `0.1.3` / `2.0.20`) **failed** with “no matching package” on menhera-cooldown (index lag). Documented, not forced via crates-io override.

---

## Final Grok Build local-path snippet (as in docs)

From crate README and user-guide §1 / §1.1 / §15 checklist:

```toml
# In the consumer Cargo.toml
colibri-sys = { path = "/home/hunter/Projects/surmount/colibri/crates/colibri-sys", features = ["runtime", "stream"] }
# or relative from consumer:
# colibri-sys = { path = "../colibri/crates/colibri-sys", default-features = true }
```

Also documented: optional `install` / `ffi` stub, `MachineInfo::probe` / `probe_for_config` / model store, engine binaries still required, `plan_probe` example, repo root `/home/hunter/Projects/surmount/colibri`, HTTP harness Path A/B in user-guide §15.

---

## Desktop status

**Deferred.** `desktop/src-tauri` is already edition **2024** / MSRV **1.85**, is **outside** the workspace (own `Cargo.lock`), and pins exact Tauri `2.11.5` / `tauri-build` `2.6.3`. No `colibri-sys` dep yet. Bumping Tauri without a dedicated green pass risks shell churn unrelated to this crate’s docs/host work. Left alone.

---

## Code fix tied to docs

- `exit_code` is now re-exported at crate root (`pub use doctor::{…, exit_code, run_doctor}`) so user-guide §5 is accurate.

---

## One-line summary

Docs now state **not on crates.io**, give copy-paste **path deps** for Grok Build and other consumers, root README points at `colibri-sys`; edition already 2024; only intentional range bump is **indicatif 0.18**; other deps already at menhera-max; **hf-hub stays 0.4**; fmt/clippy/tests green; desktop Tauri deps deferred.
