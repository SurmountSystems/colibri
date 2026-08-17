# Recon: native.log vs journal (session death)

Clocks agree. `native.log` is UTC (`Z`). The journal `short-iso` timestamps are local America/Denver (MDT, UTC-6). NTP was synchronized when this was read. Add six hours to a journal `-06:00` stamp to get UTC.

`native.log` did **not** grow. Still 8 lines, 1250 bytes, mtime `2026-08-13 14:44:41.715 MDT`. No `native.log.1` or `.2`. Last product line is still FFI engine start end at `2026-08-13T20:44:41.715971Z`.

The journal never names `colibri-native`. Correlation is by time only.

## Last product events

Two process starts in one file, same model path
`~/.local/share/colibri/models/GLM-5.2-colibri-int4-g64-with-int8-mtp` (on-disk tree about 400G):

1. `17:10:09Z` / `11:10:09 MDT` log opened. Rail Start engine. FFI start end `17:10:51Z` (`elapsed_ms=7400`).
2. `20:42:58Z` / `14:42:58 MDT` log opened again. Rail Start engine `20:44:36Z`. FFI start end `20:44:41Z` (`elapsed_ms=4805`).

Nothing after that. No `panic:`, no `generate begin`, no `[prefill]`.

## First window (`17:10–17:20 UTC` / `11:10–11:20 MDT`)

GNOME Shell stayed up. No `systemd-oomd` kill. No kernel OOM. No GPU reset. No `colibri` process line.

Only compositor noise: repeated `gnome-shell` `meta_window_set_stack_position_no_sync` assertions, including one at `11:10:09 MDT` next to the first log open.

That graphical session ran until an **operator reboot**: `reboot requested from client PID … ('reboot')` at `14:27:26 MDT` (`20:27:26Z`). Not a compositor crash.

## Second window (`20:40–20:55 UTC` / `14:40–14:55 MDT`)

This is the session death.

| Local MDT | UTC | What |
|-----------|-----|------|
| 14:28:13 | 20:28:13Z | New boot after the 14:27 reboot. GNOME Shell PID 1762 starts. |
| 14:41:20 / 14:41:38 | 20:41:20 / 20:41:38Z | Two `Started Application launched by gnome-shell` (new Alacritty scopes). |
| 14:42:58 | 20:42:58Z | `native.log` second process. |
| 14:42:59 | 20:42:59Z | `gnome-shell` stack-position assertion. |
| 14:44:36–14:44:41 | 20:44:36–20:44:41Z | Last product lines: Start engine, FFI start end. |
| 14:46:03–14:49:38 | 20:46–20:49Z | `gnome-shell` / libinput: `your system is too slow` (lag 22ms to 1534ms). |
| 14:49:29–14:49:45 | 20:49:29–20:49:45Z | `systemd-journald`: `Under memory pressure, flushing caches.` |
| **14:50:17** | **20:50:17Z** | **`systemd-oomd` SIGKILL of GNOME Shell. Graphical session dies.** |
| 14:50:19 | 20:50:19Z | LightDM greeter. |
| 14:50:22 | 20:50:22Z | `app-gnome-Alacritty-14969.scope` stop timed out and was killed. |
| 14:51:35 | 20:51:35Z | Hunter session opens again. New `gnome-shell` PID 20431. |
| 14:51:51 | 20:51:51Z | `gnome-session-s` requested reboot. Orderly shutdown. |
| 14:53:08 | 20:53:08Z | Current boot starts. |

### What killed the compositor

Not a GPU hang. Not the kernel OOM killer. Not a `gnome-shell` coredump.

`systemd-oomd` at `14:50:17 MDT` / `20:50:17 UTC`:

```
Marked …/session.slice/org.gnome.Shell@user.service for killing
due to memory pressure for …/session.slice being 83.72% > 80.00%
for > 20s with reclaim activity
```

- Pressure on that unit: Avg10 83.38, Avg60 60.85, Avg300 26.75. Current memory of the unit at kill time: 1.1G.
- `org.gnome.Shell@user.service`: main process `code=killed, status=9/KILL`, result `oom-kill`. 1467 processes in that unit were killed.
- That unit's accounting: 9.1G memory peak, 6.5G swap peak, about 21 minutes wall time (this boot only).
- Clients then logged `Lost connection to Wayland compositor.`
- LightDM closed the hunter session and opened a greeter.

This is **5 minutes 36 seconds after** the last `native.log` line (`20:44:41Z`). The product file is silent for that gap. SIGKILL does not write to `native.log`.

### Memory next to that kill (same boot, user systemd)

Host: about **90 GiB RAM** (`MemTotal` 94366088 kB), about **185 GiB swap**.

Largest scopes when the session tore down:

| Unit | Wall | Memory peak | Swap peak |
|------|------|-------------|-----------|
| `app-gnome-Alacritty-14969.scope` | 8 min 44 s | **74.4G** | **106G** |
| `app.slice` (all apps) | 21 min | 78.3G | 109.3G |
| `app-gnome-Alacritty-5609.scope` | 16 min | 15.3G | 718M |
| `org.gnome.Shell@user.service` | 21 min | 9.1G | 6.5G |
| `session.slice` | 21 min | 9.3G | 6.5G |
| Chromium | 16 min | 2.3G | 1.2G |

`Alacritty-14969` started about `14:41:38 MDT` (wall time back-dated from the 14:50:22 stop). `native.log` opened 80 seconds later. That scope used 24 min CPU in 8 min 44 s wall, then **failed to stop** (`Stopping timed out. Killing.`). The journal does **not** say that scope's child was `colibri-native`. Timing lines up with the second native process. It does not prove identity.

`oomd` printed only one candidate path (GNOME Shell). It said it considered 33 cgroups. The 74.4G Alacritty lives under `app.slice`, not `session.slice`. The kill target was the pressured `session.slice` compositor.

### GPU / kernel / cores

- Kernel: **no** `invoked oom-killer`, **no** `Out of memory:`, **no** `Killed process`.
- Kernel: **no** `GPU hang`, `gpu reset`, `ring timeout`, `pageflip timeout`, `Oops`, or `BUG:` in this window. `amdgpu` lines on the current boot are normal device init (Strix, 4096M VRAM).
- `coredumpctl` since `2026-08-13 11:00`: **no** dumps. None for `colibri`, `gnome-shell`, `mutter`, `Xorg`, or `Xwayland`.
- One unrelated segfault at `14:51:51 MDT`: `gnome-software` in `libgtk-4` during the reboot after re-login.

## Last two hours / current boot

Current boot started `14:53:08 MDT`. No second `systemd-oomd` kill of GNOME Shell. `native.log` was not written again. No later product run is in that file.

## Honest answers

**Can we correlate `native.log` with the journal?** Yes, on the UTC vs MDT clocks above. Product timestamps match journal events in the same minute for window open and engine start. The session death is **not** at `20:44:41Z`. It is at `20:50:17Z`.

**Did GNOME Shell / mutter / GDM die near `20:44:41Z`?** GNOME Shell (mutter Wayland) died **5 min 36 s later**, at `20:50:17Z`, by `systemd-oomd` SIGKILL. LightDM (this host is not GDM) then dropped to a greeter. A new hunter session came back at `20:51:35Z`, then `gnome-session` requested a full reboot at `20:51:51Z`.

**Did the GPU driver reset or hang?** No matching kernel line in these windows.

**Did the kernel OOM-kill anything?** No. Userspace `systemd-oomd` killed the compositor cgroup. That is a different mechanism.

**What we still do not know**

- Whether `Alacritty-14969` (74.4G / 106G swap) actually hosted `colibri-native`. Journal never names the binary.
- What the process did in the 5 min 36 s after FFI start end. `native.log` has no generate or panic line. The machine was already logging input lag and journal memory pressure before the kill.
- Why `oomd` chose GNOME Shell over the huge `app.slice` scope. Only the chosen candidate was printed.
- Whether a first-session SIGKILL happened around `17:10Z`. The journal in `17:10–17:20 UTC` does not show one. That session lasted until the 14:27 reboot.
- Whether the 14:51 reboot after re-login was the operator or GNOME session recovery. The requester comm was `gnome-session-s`.
