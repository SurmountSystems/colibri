# colibri-sys documentation

Local documentation for the embeddable Colibrì host crate.

## Local development / absolute paths

On the Surmount operator machine the clone lives at:

```text
/home/hunter/Projects/surmount/colibri
```

Crate root: `/home/hunter/Projects/surmount/colibri/crates/colibri-sys`

Other clones: replace that prefix with your own repo root (or set a shell
variable such as `COLIBRI_ROOT` and expand paths in docs/scripts yourself).
In-tree markdown links below stay **relative** so they work from any checkout.

| Document | Relative (from this file) | Purpose |
|----------|---------------------------|---------|
| [**User guide**](user-guide.md) | `user-guide.md` | Depend, probe, plan, doctor, embed, stream, install, **Grok Build harness (§15)** |
| [**Crate README**](../README.md) | `../README.md` | Overview, features, path dep, quick start |
| [**Phase D FFI**](ffi-phase-d.md) | `ffi-phase-d.md` | Multi-family CPU static opt-in (GLM/Kimi/Inkling/V4) + size API; process serve still default |
| API rustdoc | (generated) | After `cargo doc -p colibri-sys --open` |

Absolute paths on this machine (optional, for operators and `file://` open):

| Document | Absolute path |
|----------|---------------|
| User guide | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/user-guide.md` |
| Crate README | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/README.md` |
| Phase D FFI | `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/ffi-phase-d.md` |
| rustdoc | `/home/hunter/Projects/surmount/colibri/target/doc/colibri_sys/index.html` |

## Open locally

```bash
cd /home/hunter/Projects/surmount/colibri
# or: cd "$COLIBRI_ROOT"

less crates/colibri-sys/docs/user-guide.md
cargo doc -p colibri-sys --no-deps --features install --open
```

`file://` user guide (this machine):

`file:///home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/user-guide.md`

## Not on crates.io

Dependents use a **path** or **git** dependency. See [user-guide.md §1](user-guide.md)
and the crate [README](../README.md). Do not write `colibri-sys = "0.1"` expecting
the registry until a publish exists.

## Related product docs (repo root)

These live outside the crate and describe the C/Python product the host talks to:

| Topic | Path from repo root |
|-------|---------------------|
| Serve mux protocol | `docs/serve_protocol.md` |
| CLI settings | `docs/SETTINGS.md` |
| Engine environment | `docs/ENVIRONMENT.md` |
| Telemetry packing | `c/telemetry.h` |
| Quality / placement doctrine | crate README “Quality doctrine” |
