# Recon: that run used CPU FFI, not HIP/ROCm

**That run: CPU-only in-process FFI.** Not HIP FFI. Not CUDA. Not the process engine.

The host has ROCm. The binary that ran did not link it and cannot launch HIP kernels.

## What they ran

`which colibri-native` is `/home/hunter/.cargo/bin/colibri-native`. Same bytes as `target/release/colibri-native` (sha256 `4a4009b37f8fa775b04468fccd373638a779426f1b0d2197101dfbcc6e400a70`).

- mtime `2026-08-13 11:10:04 MDT`
- first `native.log` open `2026-08-13T17:10:09.171467Z` (11:10:09 MDT), five seconds later
- second process `20:42:58Z` (14:42:58 MDT); binary mtime unchanged

Cargo install record (`~/.cargo/.crates2.json`):

- features: `ffi`, `install`
- `no_default_features`: true
- profile: release

That is the default `just install` recipe (`features="install,ffi"`). HIP is a different install: `just install features=install,ffi-hip`. They did not do that.

`crates/colibri-native/Cargo.toml` default is also `install` + `ffi`. Comment in that file: default `ffi` is CPU-only kernels. `ffi-hip` is opt-in.

## Linkage (this binary is not HIP)

`readelf` `NEEDED` / `ldd`: `libfreetype`, `libxcb`, `libxkbcommon`, `libxkbcommon-x11`, `libm`, **`libgomp`**, `libgcc_s`, `libc`. No `libamdhip64`. No `libcudart`.

`nm --undefined-only`: no `hip*` / `cuda*` / `hsa*` imports. GOMP symbols are imported (CPU OpenMP).

Product GPU-object scan (same names `archive_gpu_flavor` uses): **absent** in both the installed ELF and `c/libcolibri.a`:

- `__hipUnregisterFatBinary` / `__hipRegisterFatBinary`
- `__cudaUnregisterFatBinary` / `__cudaRegisterFatBinary`

`libamdhip64` appears only as **probe/doctor strings** (how to detect a HIP process engine, and the hint to rebuild with `ffi-hip`). That is not a dynamic `NEEDED` entry.

`colibri-sys` `build.rs` output for this release (`target/release/build/colibri-sys-b981953d7e104845/output`):

- compiled GLM with `gcc … -DCOLIBRI_NO_MAIN -c colibri.c`
- packed `libcolibri.a` from `colibri.lib.o` only (11:09:51 MDT)
- linked `gomp`
- **no** `hipcc`, **no** `HIP=1`, **no** `cargo:rustc-cfg=ffi_hip_linked`

A HIP FFI binary would `NEEDED` `libamdhip64` and carry those fatbin names. This one does neither.

## What `kind=ffi` means (and what it does not)

`native.log` last line:

```
engine start end … kind=ffi elapsed_ms=4805
```

In `host.rs`, `kind` is only **in-process vs process**:

```
let kind = if session.is_ffi() { "ffi" } else { "process" };
```

`is_ffi()` is `EnginePathKind::Ffi`. It does **not** name CPU vs HIP vs CUDA.

That run wrote **no** GPU flavor / linkage line. The file is still 8 tracing lines, 1250 bytes. No `panic:`, no `generate begin`, no `[prefill]` C banner. The stderr tee only appends C lines that start with `[`. Nothing like that landed.

Doctor/probe can say “AMD GPU detected but the engine is CPU-only (build with HIP=1)”. That text is in the binary. It was **not** written to this log.

## Journal `14:40–14:52` MDT (`20:40–20:52` UTC)

No kernel `amdgpu` / `kfd` / `hip` / `rocm` / pageflip / reset / hang in that twelve-minute window.

The only `amdgpu` userspace line is **after** session death: `14:51:36` MDT, new `gnome-shell` adding `/dev/dri/card1`. Display bind on re-login.

Boot of that session (`14:28` MDT) has normal Strix `amdgpu` init (4096M VRAM, ~46G GTT). That is device bring-up, not this engine run.

Kernel silence is **not** proof that no GPU kernel ever ran. Successful HIP launches often leave no journal line. The **binary** is what rules HIP out: this ELF cannot call the HIP runtime.

## Host ROCm (present, unused by this run)

- `/opt/rocm` exists. Version `7.2.4`. `hipconfig` `7.2.53211-9999`.
- `hipcc` and `rocminfo` are `/opt/rocm/bin/…`.
- `libamdhip64.so.7` is under `/opt/rocm/lib`.
- `/dev/kfd` exists. `rocminfo` (cheap enum only; no model): CPU 16 CU, GPU **gfx1102** “AMD Radeon 860M Graphics” 8 CU, plus RyzenAI NPU.

ROCm is on the machine. `just install` still built CPU `ffi` unless someone passed `ffi-hip`.

## Lag: what is proved vs what is not

**Proved (same window as the second start):** memory pressure and swap, then `systemd-oomd` killed GNOME Shell at `14:50:17` MDT. Journald flushed caches. libinput logged “your system is too slow.” Largest scope: `app-gnome-Alacritty-14969` **74.4G RAM peak / 106G swap peak**, **24 min 28 s CPU** in **8 min 44 s** wall. That is about 2.8 cores of CPU time on average, plus huge anonymous memory. See `recon-journal-session-crash.md`.

The journal still does not name the child as `colibri-native`. Timing still lines up (scope start ~14:41:38, log open 14:42:58).

**Not proved:** a full-CPU peg (no per-core series). GPU compute as the lag source (this binary has no HIP/CUDA). What the process did in the 5 min 36 s after `engine start end` (no generate line; open already returned).

Loading a ~400G on-disk GLM tree through CPU FFI into ~90 GiB RAM plus swap matches the memory picture. That is consistent, not a second measurement.

## Honest answers

| Question | Answer |
|----------|--------|
| CPU FFI, HIP FFI, or unknown? | **CPU FFI.** |
| Did `kind=ffi` tell us HIP vs CPU? | **No.** In-process vs process only. |
| Did that log write a GPU flavor? | **No.** Eight lines, none of them linkage. |
| Is ROCm on the host? | **Yes.** Unused by this install. |
| Was lag a ROCm hang? | **No matching GPU hang/reset.** Lag that we can see is **RAM/swap pressure**. |
| CPU saturation vs GPU? | **Not GPU for this binary.** CPU time in that Alacritty scope is real. We do not have a saturation trace. |
