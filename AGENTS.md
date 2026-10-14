# Colibri project agent rules

Standing process and product law for this repo. **Important process rules live
here**, not in `.agents/RESIDUAL.md`. Residual is the open product backlog only
(D0). Chat is not authority after compaction; this file is for process pins.

Host global rules still apply: `~/.grok/AGENTS.md`.

---

## Where law lives

| Kind | File |
|------|------|
| Standing process / product process pins | **this file** (`AGENTS.md`) |
| Open product residual (closed vs open work) | `.agents/RESIDUAL.md` |
| Plans / implement reports | `.agents/plans/`, `.agents/reports/` |

Do **not** put durable process rules only in residual. If residual temporarily
held a pin before this file existed, migrate it here and leave residual as a
one-line pointer if useful.

---

## Heavy work yields the machine (pinned 2026-10-13)

The Operator is using this machine while agents run. Always remember.

Any long or CPU-heavy command an agent starts runs under `nice -n 19`. That
includes benchmarks, test suites, model checks, compiles, and Nix builds.

If a tool takes a thread count or a job count, use at most half the machine's
cores. Read the count with `nproc` in that turn. On a 16-core machine the cap
is 8. Set that cap on the command: `OMP_NUM_THREADS`, `MAKEFLAGS=-jN`,
`CARGO_BUILD_JOBS`, and the tool's own jobs flag. Do not leave a parallel
default that takes every core.

Do not renice or taskset the Operator's own apps. Apply this to the agent job
and the processes it spawns. A job already running when this pin lands gets
`renice -n 19` and a CPU set of at most half the cores, without a restart,
unless it is still saturating the machine.

Do not use a bare `make -j` or an uncapped cargo build. When a command has its
own jobs flag, that flag is at most half the cores.

---

## Product copy fidelity

**Pinned:** 2026-08-11 (operator: keep copy faithful to original Colibri; no
agent invention).

1. **Faithful to original Colibri.** User-visible product copy (i18n, wizard,
   hero, rail, Tools, status, Brain/Profiling labels) must stay faithful to
   original Colibri sources:
   - Primary: `web/src/i18n/en.ts` (and `it` / other locales when present)
   - Original desktop/SPA strings already in-tree (not new Surmount marketing)

2. **Do not invent brand theater.** Agents must not invent marketing slogans,
   taglines, hero lines, brand voice, or witty copy that is not in the original
   product. Prefer porting **exact keys and values** from `web/src/i18n/`.

3. **Necessary new functional copy only.** When a native-only surface needs a
   string the SPA never had (next step after setup, error recovery, install
   status, doctor readiness), use **plain operational English**. Match the tone
   of adjacent original strings when possible. Not brand theater.

4. **Cite source when non-obvious.** When adding i18n keys, note the source path
   (web key or “native-only operational”) in a short comment or implement report.

5. **Do not mass-rewrite i18n** unless a string clearly diverges from `web/` for
   the same key/intent. Divergence notes go in reports, not silent rewrites.

---

## Native crash logs (test runs)

**Pinned:** 2026-08-13 (operator crashed `colibri-native` while using it and
asked for a log; chat does not get dumps unless someone reads the file).
**Extended:** 2026-08-13 (operator: crash killed the whole windowing session;
journalctl around that time is in scope).

1. **First artifacts.** After an operator-reported native crash or hang, read
   the tail of `~/.local/share/colibri/logs/native.log` and any rotated
   siblings (`native.log.1`, and so on). Chat still does not automatically
   receive dumps. When the windowing session, compositor, or display died,
   also read `journalctl` around those timestamps (user and system: gnome-shell,
   mutter, gdm, drm/gpu, kernel oops/OOM). Do not answer from unit tests or
   chat memory as if that were the last-run dump.

2. **Default-on file logging stays.** Panic hooks, tracing, and C banners that
   the product already tees must still reach that file. If a crash class cannot
   appear there (SIGKILL, or SIGSEGV with no hook), say so in the implement
   report. Do not claim we have logs for a signal that never writes.

3. **Real-app verify.** When the operator just ran the real app, implementer
   verify for crash or hang UX is not done on parse-only unit tests. Targeted
   cargo tests stay required (TDD). They do not replace reading the last-run
   log.

4. **Contract tests.** Product tests that exercise logging must fail if the
   log path, panic capture, or banner-tee contract is broken. Do not add
   Sentry or another crash reporter under this pin.

5. **Journal is allowed for session death.** The host rule against a journalctl
   safari is for keyring / Secret Service archaeology and supply-chain fishing.
   It does not forbid journal correlation when the desktop session died or the
   operator asked for the journal.

6. **native.log is not a full session story.** Product `native.log` will not
   survive a compositor death as a complete account of the session. Correlate
   clocks (UTC in `native.log` versus the journal). If the journal has no match,
   say so.
