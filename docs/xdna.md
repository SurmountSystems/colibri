# XDNA backend (AMD Ryzen AI NPU)

colibrì includes an **experimental, opt-in** backend that can run qualified
work on an AMD XDNA NPU using a **reduced-precision BF16 compute path**.
It is off by default, it is never selected automatically, and model output may
differ from the normal path.

Windows still loads `coli_xdna.dll` and still limits execution to the qualified
GLM shared-expert artifacts. Linux loads `libcoli_xdna.so` the same way
(beside the executable, absolute path, no PATH search). Dispatch selection
accepts every model on the 2.0 roster. A roster family also has one qualified
synthetic op: the public IRON npu2 single-core matmul, M=K=N=64, int16 in,
int32 out. That op is not a token generate. The dedicated GLM shared-expert
files (K=6144, N=2048) are still not in this tree. When the on-disk 64x64x64
program is present, expert GEMM at that shape tiles through it (`TILED_64`).
That is not a dedicated aiecc kernel for K=6144, and it is not a token generate.
No throughput number is claimed.

```bash
coli run --xdna "your prompt"
```

Without `--xdna` nothing changes: no helper is loaded, no device is opened, and
the engine produces exactly the results it produces today. Installing the
optional package does not enable anything on its own.

`--xdna` works by setting `COLI_XDNA=1` for the engine, and the engine reads that
variable directly — so if you export it yourself it has the same effect, with or
without the flag. That is the only other way in.

## What runs on the NPU

With `COLI_XDNA=1`, each owned engine main calls
`coli_xdna_engine_qualified_submit` once at session start and that submits
the 64x64x64 int16 program. The int32 result is not an expert GEMM output.
This is not a token generate. No throughput number is claimed.

When the on-disk 64x64x64 xclbin is present, expert GEMM at K=6144 N=2048
tiles through that program (`TILED_64`). It is not an immediate
`ARTIFACT_UNAVAILABLE` decline. The dedicated wa_F3 files are still not on
disk. Exact 64x64 is `PROGRAM_PRESENT`. Other positive K,N also tile
(`TILED_64`; remainders pad to 64). A live submit still needs permission, a
bound helper, and a device after the shape gate. `COLI_XDNA` off returns
`ENGINE_LANE_NOT_CALLED` and does not open the device.

The historical MoE shared-expert gate/up projection for GLM
(`K=6144, N=2048`, fmt=4 grouped int4, group size 64) is still a shape
`coli_xdna_try_matmul` knows. The dedicated artifacts for that row are not
in this tree. When the 64x64x64 xclbin is present, that expert GEMM tiles
through the on-disk program. This is not a token generate.

## Precision

The session submit that runs today is int16 in and int32 out. It is not BF16.
The historical GLM shared-expert path converts activations and weights to BF16
and accumulates in F32. The dedicated K=6144 N=2048 files for that path are
not on disk, so that BF16 path still declines. Tiling uses the on-disk
64x64x64 int16 program, not those files. BF16 on the dedicated path is not
the same arithmetic as the normal path, and it is not claimed to be equivalent:

- per-option scores on a small internal benchmark changed on every operation the
  NPU handled;
- in a separate experiment with free-running generation, 5 of 8 prompts produced
  **different text**.

Whether that matters for your use is a judgement you have to make. The mode
exists so the choice is explicit.

## Installing the optional package

The NPU backend needs two things beside the executable: a helper DLL and the
qualified kernel artifacts. They ship as a separate optional download, not in
the main archive.

Extract it over your colibrì directory so the layout is:

```
<colibri directory>\
    colibri.exe
    coli_xdna.dll
    xdna\
        wa_F3_M64_K6144_N2048.xclbin
        wa_F3_M64_K6144_N2048_insts.bin
        wa_F3_M256_K6144_N2048.xclbin
        wa_F3_M256_K6144_N2048_insts.bin
```

Both locations are resolved **relative to the executable**, by absolute path.
`PATH` and the working directory are not searched, so the package has to be in
that one place. Every artifact is checked against a hash built into the binary
before it is used.

## What the optional package needs installed

The sidecar is small because it ships only the helper and the kernels. Two
things have to already be on the machine for `coli_xdna.dll` to load:

- **AMD XRT runtime** (`xrt_coreutil.dll`), from AMD's Ryzen AI / XRT install.
- **Microsoft Visual C++ Redistributable** (x64). The helper is built with MSVC
  — that is how it reaches XRT's C++ interface — so it imports `MSVCP140.dll`,
  `VCRUNTIME140.dll` and `VCRUNTIME140_1.dll`.

Neither is bundled. If either is missing the helper simply will not load, and
`--xdna` reports it and continues on the normal path:

```
[XDNA] requested, and the artifact package is valid, but the XDNA helper
       (coli_xdna.dll) is not usable beside the executable: continuing on the
       normal path
```

**The ordinary colibrì host needs neither of these.** It imports only Windows
system libraries, with or without the NPU capability compiled in — the external
dependencies belong to the optional package alone.

Also required: Windows and an XDNA2-class NPU.

## When something is missing

`--xdna` always tells you what happened and always keeps running on the normal
path:

```
[XDNA] requested, but this build has no XDNA support: continuing on the normal path
[XDNA] requested, but the XDNA package was not found at <dir>\xdna: continuing on the normal path
[XDNA] requested, but the XDNA package is incomplete (missing <file>) in <dir>\xdna: ...
[XDNA] requested, but an XDNA artifact does not match its expected hash (<file>):
       refusing to use it, continuing on the normal path
[XDNA] requested, and the artifact package is valid, but the XDNA helper
       (coli_xdna.dll) is not usable beside the executable: ...
```

An artifact whose bytes do not match the expected hash is refused even though
you asked for the NPU — bytes that are not the qualified bytes are not the
qualified kernel.

When everything is in place you get the activation notice instead, once:

```
[XDNA] experimental: qualified GLM shared expert operations will use the
[XDNA] native NPU with a reduced-precision BF16 compute path. Model output
[XDNA] may differ from the normal path, and generated text has been observed
[XDNA] to diverge. Operations this lane does not support continue to use the
[XDNA] normal path.
```

All of this goes to stderr, so it never mixes into machine-readable output.


## Producing the optional package (maintainers)

CI cannot build this package. The helper needs the XRT SDK and the artifacts
need the AIE toolchain, and the release runners have neither — which is why the
Windows job builds an XDNA-capable host and stops there. The sidecar is produced
on a qualified machine and attached to the same release.

Everything below is owned by `c/tools/build_xdna_package.py`. It reads the
expected artifact names and hashes out of `c/backend_xdna.c`, so the package can
never drift from what the engine will accept at runtime.

```bash
python tools/build_xdna_package.py \
    --helper   <path to the built coli_xdna.dll> \
    --artifacts <directory holding the four qualified artifacts> \
    --out      dist/xdna-pkg \
    --release --dist dist
```

That verifies every input against the compiled registry *before* writing
anything, then produces three files named from `version.py`:

```
colibri-<tag>-windows-x86_64-xdna.zip            the sidecar
colibri-<tag>-windows-x86_64-xdna.manifest.txt   sizes, hashes, roles, version
colibri-<tag>-windows-x86_64-xdna.sha256         sha256sum format
```

The name keeps the core archive's stem and adds `-xdna`, so it sits beside
`colibri-<tag>-windows-x86_64.zip`, is matched by the release job's own
`sha256sum colibri-*`, and cannot be mistaken for the core download.

The archive is reproducible: the same helper and the same qualified artifacts
produce the same bytes, and therefore the same sha256, wherever they were staged
from and whatever mtimes those files carry. If you rebuild the sidecar and the
checksum moves, something in the *content* changed — that is a signal, not noise.

Before publishing, re-verify the built assets:

```bash
python tools/build_xdna_package.py --verify-release dist/colibri-<tag>-windows-x86_64-xdna.zip
```

This refuses to pass if the archive does not match its manifest, if the manifest
names a different release version than the file does, or if any artifact digest
disagrees with the compiled registry. Attach only after it prints `release
assets OK` — the command to do so is printed by `--release`, and uses the same
`gh release upload ... --clobber` the release workflow itself uses.

**Never bundle** `xrt_core.dll`, `xrt_coreutil.dll`, the MSVC redistributable, the
AIE toolchain or any SDK header. Those are the user's to install; the sidecar is
only colibrì's own helper and artifacts.

To confirm the ordinary download is unaffected, install the core archive on its
own and run normally: no helper is loaded, no device is opened, and nothing
about the default path changes.

## Linux

The lane stays off until `COLI_XDNA` parses as a non-zero integer. Unset,
empty, `0`, and non-numeric values leave it off. A device node by itself does
not turn it on, and a missing helper is reported as unavailable rather than
available.

Place the helper beside the executable:

```
<colibri directory>/
    colibri
    libcoli_xdna.so
    xdna/
        ...qualified artifacts, when you have them...
```

The host resolves that path from `/proc/self/exe` and passes the absolute path
to `dlopen`. It does not search `PATH` or `LD_LIBRARY_PATH` for the helper.
The default CPU build does not link XRT. XRT is linked only into
`libcoli_xdna.so`, and only when you build that helper.

The kernel driver is `amdxdna`. A present device is a node under `/dev/accel/`
whose sysfs driver is `amdxdna` (on this class of machine, `/dev/accel/accel0`).
No such node means the device is absent. The report says so. It does not invent
a device and it does not invent a generate.

Userspace is AMD's XRT plus the XDNA SHIM (`libxrt_coreutil.so` and
`libxrt_driver_xdna.so`). Upstream documents the install prefix as
`/opt/xilinx/xrt/lib` and the kernel module as `amdxdna.ko`. See
[AMD XDNA Driver for Linux](https://github.com/amd/xdna-driver/blob/main/README.md)
(accessed: 2026-10-12). A distribution package may install the same libraries
under `/usr/lib` and the headers under `/usr/include/xrt`. Either location is
the user's install. Colibrì does not bundle them.

Build the helper on a machine that has those headers and `libxrt_coreutil`:

```bash
make -C c/xdna
```

That writes `c/xdna/libcoli_xdna.so`. Copy it beside the executable. The
ordinary `c/Makefile` build does not do this and does not link XRT.

Linux dispatch selection accepts every id from the 2.0 roster (`glm`, `kimi`,
`inkling`, `deepseek_v4`, `olmoe`, `qwen_image`, `glm53`, `qwen36`, `qwen38`,
`deepseek_v41`, `mimo`, `laya`, `gliner_decide`). A non-GLM model is not
refused just because it is not the old GLM shared-expert shape.

Each of those families has a registry row for one program:
`mlir_aie_single_core_matmul_npu2.xclbin` plus its instruction file, under
`c/xdna/kernels`. The shape is M=K=N=64, int16 inputs, int32 output. The
helper copies the weight into a host-only buffer on kernel argument group 4,
which is the argument layout of that `MLIR_AIE` program (opcode 3, instructions
on group 1, buffers on groups 3, 4, and 5). `coli_xdna_linux_roster_op` runs
that program with synthetic buffers and keeps the result only when every int32
matches the host reference. That is one matmul per family. It is not a
downloaded-weight generate.

The two GLM shared-expert rows (K=6144, N=2048, M64 and M256) are still in
the registry. Their dedicated files are not shipped. When the on-disk
64x64x64 xclbin is present, `coli_xdna_try_matmul` at K=6144 N=2048 tiles
through that program (`TILED_64`). It is not an immediate
`ARTIFACT_UNAVAILABLE` decline and it is not a dedicated aiecc kernel for
K=6144. Exact 64x64 is `PROGRAM_PRESENT`. Other positive K,N also tile
(`TILED_64`). With `COLI_XDNA=1`, each owned engine main calls
`coli_xdna_engine_qualified_submit` once at session start and that submits
the 64x64x64 int16 program. That session-start int32 result is not an expert
GEMM output. `COLI_XDNA` off returns `ENGINE_LANE_NOT_CALLED` and does not
open the device.
Work with no matching artifact declines to the normal path. Acceptance
of a model id does not mean a generate ran. This is not a token generate.
No throughput number is claimed.

## What this backend does not do

- It is not enabled by having the hardware, the helper, or the package.
- It does not choose between the NPU and the normal path on speed or cost.
- No speed claim is made here.
- Windows execution stays the qualified GLM shared-expert lane.
- Linux selection covers the 2.0 roster. A family with no row still declines.
  The synthetic 64x64x64 int16 matmul is not a token generate, and a declined
  operation is not reported as a run.
- It does not claim the dedicated GLM K=6144 N=2048 gate/up ran. Those files
  are absent. When the 64x64x64 xclbin is present, expert GEMM at that shape
  tiles through the on-disk program (`TILED_64`). That is not a token generate.
  `COLI_XDNA` off returns `ENGINE_LANE_NOT_CALLED` and does not open the device.
