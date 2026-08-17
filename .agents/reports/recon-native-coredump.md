# Recon: systemd coredump for colibri-native

**When:** 2026-08-13  
**Tool:** `/usr/bin/coredumpctl` (ran as the operator user; no permission error)

## Result

systemd-coredump has **no** stored dump for `colibri-native` or `colibri`.

- `coredumpctl list colibri-native` → `No coredumps found.` (exit 1)
- `coredumpctl list colibri` → `No coredumps found.` (exit 1)
- Same filters with `--since=2026-08-12` are also empty.
- There is no dump at all after **2026-08-13 10:00 MDT**. The crash window in `native.log` is **20:44:41Z** (14:44:41 MDT). `coredumpctl list --since="2026-08-13 14:00:00"` is empty.

`coredumpctl info` was not run. There is no matching PID or cursor.

## What is in the journal instead

The newest dump on this host is unrelated:

| Time (MDT) | PID | Signal | Exe |
|---|---|---|---|
| Thu 2026-08-13 09:57:25 | 2057713 | SIGABRT | `/usr/bin/bash` |

Other recent dumps (last day) are grok-build test bins, `kodi-test`, Spotify, `gnome-keyring-daemon`, and bash. None live under `/home/hunter/Projects/surmount/colibri`.

## Meaning

This crash did not leave a systemd coredump. Common reasons that match "log ends at a successful FFI start, no panic line":

- The process was **SIGKILL** (no core).
- The process **exited** without a fatal signal.
- The process is still considered running, or only a child died without a dump.
- `RLIMIT_CORE` is 0, or the unit/user session does not collect cores.

This check does not identify the signal. It only shows systemd did not keep a core.
