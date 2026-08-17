# Recon: FFI load path vs last logout (mmap / RAM / isolation)

Read-only inventory. No product edits. Evidence is code plus
`.agents/reports/recon-journal-session-crash.md` (operator GLM-5.2 CPU FFI,
~74G RAM + 106G swap, systemd-oomd killed GNOME).

## What mmap / kernel techniques exist today

Default product path for GLM **does not mmap weights**. The shard reader is
explicitly pread-plus-fadvise, written to avoid a historical mmap RSS bug.

| Technique | Where | What it actually does |
|-----------|--------|------------------------|
| **pread + malloc** (default experts) | `c/colibri.c` `expert_load` / `expert_load_impl` (~2292–2432) | `posix_memalign` slab + `pread` / `mir_pread`. This is the FFI path today. |
| **pread + malloc** (dense) | `c/colibri.c` `qt_from_disk` / `qt_load` (~1669–1741); `c/st.h` `st_read_raw` / `st_read_f32` | Dense, shared, attn, embed, lm_head: allocate, copy, keep. `qt_load` passes `drop=0`, so file pages are **not** advised away. RSS + page cache can both hold the same dense bytes. |
| **posix_fadvise WILLNEED** | `c/st.h` `st_prefetch` / `st_prefetch_rep`; `c/colibri.c` `expert_prefetch`; `c/kimi_k3.c` | Async readahead before demand pread. |
| **posix_fadvise DONTNEED** | `c/st.h` `st_read_*` when `drop=1`; `c/colibri.c` expert path only if `g_drop` (`DROP=1`); `c/inkling.c` | Drops page-cache after a streaming read. **Off by default.** `g_drop` is set only in CLI `main()`, not in `coli_glm_engine_open`. |
| **O_DIRECT twin fds** | `c/st.h` `st_open_fd` / `st_direct_fd`; `c/colibri.c` `g_direct` (`DIRECT=1`); `c/kimi_k3.c` `K3_DIRECT` default 1 | Bypass page cache on expert demand reads. GLM default **OFF**. `g_direct` is set only in CLI `main()`. |
| **COLI_MMAP=1** (opt-in experts) | `c/colibri.c` `map_of_fd` (~2011–2029), `expert_load` mmap branch (~2248–2289) | `mmap(..., PROT_READ, MAP_SHARED, fd, 0)` of whole shards. Views into the file. **No `MAP_PRIVATE`, `MAP_POPULATE`, `MAP_NORESERVE`, `MAP_LOCKED`.** |
| **madvise MADV_WILLNEED + page touch** | same mmap branch (~2264–2275) | On **every** mmap expert load: `madvise(..., MADV_WILLNEED)` then touch every 4K. Forces the expert into RAM (page cache). Opposite of "do not populate." |
| **mlock / munlock** | `mem_should_wire`, `mem_wire`, `qt_wire_mmap`, `qt_unwire_mmap`, `pin_wire` (~7797–7876) | Pins **pinned** experts. Auto-on on macOS, **opt-in on Linux** (`MLOCK=1`). Only after CLI pin path. |
| **madvise MADV_DONTNEED** | `expert_host_release` (~2821) | Linux CUDA host-copy release of arena slabs. Not the CPU FFI default. |
| **io_uring mmap** | `c/uring.h` `coli_uring_init` | `MAP_SHARED\|MAP_POPULATE` of the **ring**, not model weights. CLI-only (`URING=1`); incompatible with `COLI_MMAP=1`. |
| **SSD probe mmap** | `c/colibri.c` `coli_ssd_probe_raw`; `c/tests/test_ssd_probe.c` | Address-space map for `mincore`. Not weight load. Metal+darwin. |
| **compat shims** | `c/compat.h` | `posix_fadvise` no-op on macOS/Windows; `compat_mlock` = `VirtualLock`; `compat_open_direct`. |

**Not present anywhere under `c/`:** `MAP_NORESERVE`, `MADV_HUGEPAGE`,
`MADV_SEQUENTIAL` / `MADV_RANDOM`, `posix_madvise`, `MAP_LOCKED`, `fallocate`,
transparent-huge-page control, hugepage mounts.

`g_mmap` and `g_direct` are initialized in CLI `main()` only
(`c/colibri.c` ~8995, ~9088). `coli_glm_engine_open` never reads `COLI_MMAP` or
`DIRECT`. In-process GLM is always the pread+slab path unless someone later
wires those env knobs into the embed open.

## Rust FFI open path

1. Native host (`crates/colibri-native/src/host.rs` `EngineSession::start`):
   worker via `spawn_engine_start` (not the UI thread).
2. With Cargo `feature = "ffi"` and no `COLIBRI_FORCE_PROCESS`,
   `resolve_prefer_process()` → try FFI first.
3. `colibri_sys::ffi::open_engine` → `GlmEngine::open` →
   `coli_glm_engine_open` (`crates/colibri-sys/src/ffi/multi.rs`,
   `c/colibri.c` ~9631).
4. C open: walk disk for safetensors, `calloc` engine, **`model_init` only**.
   Default knobs: `cap=64`, `expert_bits=4`, `dense_bits=8`
   (`c/colibri_api.h`, `GlmOpenOptions::default()` zeros become those C defaults).
5. `model_init` (~1785): index shards, **malloc+pread every dense/shared/attn
   tensor**, allocate empty expert LRU arrays of size `cap`. Does **not** load
   routed experts yet.
6. On FFI open failure: process fallback (`start_process` +
   `PlacementPlan::build` + `EngineHandle::start_with_plan`).
7. Generate: `coli_glm_generate` → `step` / `spec_decode` → `expert_load`
   fills LRU up to `ecap`. `repin_pass` / `rss_guard` exist on that decode
   loop, but they are inert on FFI (see below).

**Process path contrast:** CLI `main()` snapshots `MemAvailable`, may pin,
then **always** calls `cap_for_ram` (~9428–9430). That function lowers or
raises `ecap` to 88% of available RAM, and **`exit(2)`** if even cap=1
projected peak exceeds boot `MemAvailable` (unless `COLI_RAM_OVERCOMMIT=1`).
Process serve inherits that because the child runs `main()`. Native process
start also applies `RAM_GB` from the placement plan
(`environment_for_plan`).

**FFI GLM skips all of that.** No `g_mem_avail_boot`, no `cap_for_ram`, no
AUTOPIN, no `rss_guard` budget (`g_ram_budget_gb` stays 0; `g_repin` stays 0
because `REPIN` is only read in `main()`). `rss_guard` returns immediately
when `lim<=0`.

Inkling embed is slightly more honest: `coli_ink_engine_open` passes cap 0
into `model_init`, and Inkling auto-sizes LRU from free RAM minus 20% + 4 GB
(`c/inkling.c` ~885–891). Kimi embed is `model_init` only, no RAM clamp
(`c/kimi_k3.c` `coli_kimi_engine_open`).

## Memory honesty today

| Surface | Behavior |
|---------|----------|
| Doctor `memory.ram` | **Warn only** if plan budget > available RAM, or cannot hold one slot/layer. Fail is reserved for broken install / missing engine. Test: `memory_ram_capacity_tight_is_warn_not_fail`. |
| Placement plan | Notes cold-disk overflow as **intended**, not a hard stop. `RAM_GB` = 88% of `MemAvailable` (min 8 GiB floor). Applied to **process** env, **not** to FFI open. |
| Native engine preflight | `preflight_model_for_engine_start`: leaf/path only. **No RAM vs model check.** |
| `coli_model_size_probe` | Disk bytes of `*.safetensors` only. Dense/expert split often 0 at C open. |
| `native.log` start line | `format_engine_start_log`: phase, model path, `kind=ffi\|process`, elapsed, error. **No pid, tid, cgroup, RSS, VmSwap, CPU-vs-HIP.** |
| Periodic RSS | None in the host. C `rss_gb()` prints on CLI STAT / PROF / RAM-GUARD only. |
| C banners in native.log | `stderr_tee.rs` appends lines that start with `[`. FFI never emits `[RAM]` / `[RAM-GUARD]` because those functions are not run. |

Probe already reads swap (`crates/colibri-sys/src/probe.rs`
`linux_memory_snapshot`). Nothing uses swap as a start gate.

## Crash / fault isolation

- **No sandbox, seccomp, landlock, `setrlimit`, or fork-before-open.**
- FFI lives in the GPUI process. `SIGSEGV` / `abort` in C kills the UI.
  Documented and accepted: `crates/colibri-sys/docs/ffi-phase-d.md`
  (Isolation policy). Residual repeats it.
- Isolation control is **`COLIBRI_FORCE_PROCESS=1`** (or build without
  `feature=ffi`). Process fallback after **open** failure does not help a
  successful open that later fills RAM.
- Process mode isolates **faults**, not **memory pressure**. A SERVE child
  in the same Alacritty `app.slice` can still fill RAM+swap. Last logout
  was `systemd-oomd` killing **GNOME Shell** (`session.slice` > 80% memory
  pressure), not a segfault of `colibri-native`.
- Niceness (`COLI_COMPUTE_NICE=10`) does not reduce RSS. Residual already
  says it will not stop oomd.

## What the last logout actually needed

From `recon-journal-session-crash.md` (verified journal, not guessed mmap):

- Model: `GLM-5.2-colibri-int4-g64-with-int8-mtp` (~400G on disk).
- Host: ~90 GiB RAM, ~185 GiB swap.
- Last product line: FFI engine start **end** (~4.8s). No `generate begin`,
  no `[prefill]`, no panic. File silent for 5 min 36 s, then oomd.
- Largest scope at tear-down: Alacritty ~**74.4G memory + 106G swap**.
- Killer: userspace oomd on the compositor, not kernel OOM, not GPU reset.

**Open itself** (4–8s) is dense `model_init` only. Expert slabs appear when
decode/prefill runs `expert_load`. A filled `cap=64` LRU on this family is
the right order of magnitude: `expert_bytes_probe` uses the **widest** slot
(~38 MB for GLM-5.2 int8 MTP, `c/telemetry.h`). Roughly
`64 × ~77 sparse/MTP rows × ~38 MB ≈ 180 GB` if the cache fills. That
matches 74G + 106G swap. We **do not know** generate ran (no log line). We
**do know** FFI leaves `ecap=64` with no clamp, and nothing logs RSS while
it grows.

Kernel mmap flags would not have stopped this run:

- Default path is **not mmap**.
- Opt-in mmap **faults every expert in** (`MADV_WILLNEED` + touch).
- `MAP_NORESERVE` only changes accounting of untouched maps. These maps
  (or slabs) get touched.

What was missing: **refuse or clamp before the working set can eat RAM and
swap**, plus **pid/RSS/swap lines in `native.log`** so the next silent
5-minute hole is diagnosable. Process isolation alone would not have kept
the desktop session.

## Recommended product slices (smallest real vs theater)

Do **not** invent NPU, OpenAI REST, Sentry, or generate-% redesign.

### 1. Run the existing RAM clamp on FFI GLM open (real)

Call the same sequence CLI already has after `model_init`: snapshot
`MemAvailable`, `cap_for_ram` (and refuse when floor peak exceeds RAM).
Optionally honor `RAM_GB` / a host-computed cap from `PlacementPlan`.

**Tradeoff:** cold experts hit disk more; decode is slower; the session
stays up. Inkling already auto-caps. GLM embed is the hole that matched
this logout.

Theater: flipping `COLI_MMAP=1` on FFI without removing the touch loop.

### 2. Host preflight: RAM+swap vs projected working set (real)

Before `coli_glm_engine_open`, plan dense + cap-floor experts + slack
against `MemAvailable` (and decide whether swap counts). **Refuse start**
with a plain status. Doctor can stay warn for "may run poorly"; **Start
engine** should not.

**Tradeoff:** false refuse if the projection is pessimistic (CLI already
lives with that; `COLI_RAM_OVERCOMMIT` exists). Better than logging the
user out.

Theater: warn-only doctor row the operator can ignore and still press
Start.

### 3. Log pid, tid, cgroup, RSS, VmSwap, flavor (real, cheap)

On engine start and on a slow timer (and on generate begin): write into
`native.log` (and keep C `[RAM]` banners teed). Include `kind=ffi|process`,
CPU vs `ffi-hip` / `ffi-cuda` if linked.

Does not prevent oomd. Would have answered "what grew for 5 minutes."

### 4. Optional process default when projected RSS is huge (partial)

If estimated working set exceeds a fraction of RAM, skip FFI and spawn
SERVE. **Does not isolate memory.** The child still uses the same machine
and the same user slice. Only useful **together with** slice 1 on the
child (`main()` already has `cap_for_ram`). Useful for **SIGSEGV**
isolation, not for this logout.

Honest line: CPU FFI in-process **cannot** be crash-isolated without
leaving the process.

### 5. mmap polish (not the logout fix)

Only after 1–3, and only if someone wants file-backed experts:

- Stop touching every page on load (that *is* populate-all).
- Sequential `madvise` / `posix_fadvise` for cold misses, not WILLNEED of
  the whole expert unless you intend residency.
- `MAP_NORESERVE` is optional accounting; it is not a safety net once
  matmul faults pages.

Do not treat hugepages / THP / `O_DIRECT` / io_uring as the desktop
fix.

## Critical files

- `/home/hunter/Projects/surmount/colibri/c/colibri.c` — `model_init`,
  `expert_load`, `map_of_fd`, `cap_for_ram`, `rss_guard`,
  `coli_glm_engine_open`, CLI `main` env knobs
- `/home/hunter/Projects/surmount/colibri/c/st.h` — pread, fadvise,
  O_DIRECT twins, mmap-rss comment
- `/home/hunter/Projects/surmount/colibri/c/telemetry.h` —
  `expert_bytes_probe` (widest-slot, OOM history)
- `/home/hunter/Projects/surmount/colibri/c/colibri_api.h` — FFI open
  defaults
- `/home/hunter/Projects/surmount/colibri/c/inkling.c` — embed auto-cap
  contrast
- `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/ffi/multi.rs`
- `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/host.rs`
- `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/plan.rs`
- `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/doctor.rs`
- `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/native_log.rs`
- `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/docs/ffi-phase-d.md`
- `/home/hunter/Projects/surmount/colibri/.agents/reports/recon-journal-session-crash.md`

## TDD contracts to add (do not implement here)

1. **FFI open clamps cap.** Fixture or seam: `coli_glm_engine_open` /
   a extracted `cap_for_ram` with injected `MemAvailable` much smaller
   than `64 * nsp * expert_bytes` must leave `ecap` at the budget, not 64.
   Same numbers as CLI.

2. **FFI open refuses when floor peak exceeds RAM.** Injected available
   RAM below dense + one slot/layer + slack → open returns error (not
   `exit(2)` if embed must stay non-abort; match product choice). Override
   env still documented.

3. **Native start preflight refuses.** `EngineSession::start` / host
   helper: plan projection over available RAM → `Err` with plain English;
   does not call `coli_glm_engine_open`. Doctor may still warn.

4. **Start log fields.** `format_engine_start_log` (or a sibling) includes
   pid and kind; a unit test locks the format. Optional: RSS/VmSwap
   parse-from-string helper with a `/proc` fixture.

5. **Env knobs on embed.** If `COLI_MMAP` / `DIRECT` / `DROP` remain
   CLI-only, a test that embed open does not silently honor them (or does,
   once wired). Today they are dead on FFI.

6. **Do not weaken** `memory_ram_capacity_tight_is_warn_not_fail` unless
   the product decision is "doctor Fail too." Prefer a **separate** start
   gate test so doctor stay-warn and start-refuse can both be true.

## Not claimed

- Identity of the 74G Alacritty scope as `colibri-native` (journal never
  names the binary; clocks line up).
- That generate ran in the silent 5 minutes.
- That mmap is unused in the whole tree (it exists, opt-in / probe /
  uring).
- That process mode would have saved GNOME without the RAM clamp.
