# C engine → Rust FFI map (colibri-sys planning)

Read-only inventory of how the C inference engine is built and what can be linked from Rust. Paths are under repo root unless noted.

## 1. Build: how `coli` / `colibri` is produced

- **Top `Makefile`**: only forwards `all glm deepseek-v4 portable test check cuda-test clean install uninstall` into `c/` (lines 1–4).
- **`c/setup.sh`**: deps check (gcc/clang + OpenMP), then `make -s colibri ARCH=${ARCH:-native}`; optional tiny oracle self-test with `SNAP=./glm_tiny TF=1 ./colibri …`. No library install.
- **`c/Makefile` primary product**: **executable only**
  - `all` → `colibri$(EXE)` (phony alias `glm` same binary).
  - Link recipe (~549–550): single TU `colibri.c` + optional `backend_*.o` → `colibri`.
  - **No** `libcolibri.a` / `libcolibri.so` / `libcolibri.dylib` target for the main engine.
- **Other engine binaries** (separate mains, same header soup):
  - `olmoe`, `kimi_k3`, `inkling` (sibling `.c` files).
  - `deepseek-v4` via `Makefile.deepseek-v4` (amalgamated units of `deepseek_v4.c`).
- **Install** (~1000–1020): `coli` → `$(PREFIX)/bin`; engines + Python helpers → `$(PREFIX)/libexec/colibri`. Still **bins**, not linkable libs.
- **Only shared libs today**:
  - Optional tool codecs: `make iq3` → `tools/libiq3.so` (or `.dylib`/`.dll`); `make rans` → `tools/librans_c.so` (ctypes bridge, not inference).
  - Windows GPU: `make cuda-dll` / `hip-dll` → `coli_cuda.dll` / `coli_hip.dll` (host still is `colibri.exe` + runtime loader).
- **Default link**: `-lm -fopenmp -pthread` (macOS: libomp if Homebrew; Windows: `-static -lpsapi`). **CPU-only, zero GPU deps** unless flags set.

### GPU build flags (link requirements)

| Flag | Where | Defines | Extra link / artifacts |
|------|--------|---------|-------------------------|
| `CUDA=1` | Linux only | `-DCOLI_CUDA` | `backend_cuda.o`, `-lcudart -lstdc++`, rpath `CUDA_HOME` |
| `HIP=1` | Linux only | `-DCOLI_CUDA` | same `.cu` via hipcc, `-lamdhip64 -lstdc++` |
| `CUDA_DLL=1` / `HIP_DLL=1` | Windows | `-DCOLI_CUDA` [+ `COLI_HIP_DLL`] | host links `backend_loader.o` only; GPU in separate DLL |
| `METAL=1` | macOS | `-DCOLI_METAL` | `backend_metal.o`, `-framework Metal -framework Foundation -lc++` |
| `VK=1` | any Vulkan 1.2 | `-DCOLI_VULKAN` | `backend_vulkan.o`, `-lvulkan`, SPIR-V under `c/shaders/` (runtime path `COLI_VK_SHADERS`) |

See `GPU_BACKENDS.md`, `docs/cuda.md`, `docs/metal.md`, `docs/vulkan.md`.

## 2. `colibri.c`: CLI process, not a library

- ~9.5k-line amalgamation: model structs, load, forward, serve, chat-ish text path all in one file + many `#include "*.h"` headers (no separate `lib/` tree).
- **`int main`** at `c/colibri.c:8864`. First action can **re-exec self** for OpenMP tuning (`COLI_OMP_TUNED` / `COLI_NO_OMP_TUNE`) — important for embedders (fork/exec assumptions, or set kill-switch before process start).
- **No exported load/generate C API** for GLM. Control is env + argv:
  - Required: `SNAP=<model_dir>` (else exit with `SNAP=<dir>`).
  - Modes (after load, ~9413–9422): `ABLATE_SCORE`, `SCORE`, `SERVE` (+ optional `SERVE_BATCH`), `PROMPT`/`run_text`, teacher-force `TF`, oracle replay, default oracle compare.
- **Positionals**: historic `./colibri <cap> [expert_bits] [dense_bits]` for cache sizing (self-test / direct engine use).
- User-facing entry is **`c/coli` (Python)**: sets env (`SNAP`/`COLI_MODEL`, `SERVE`, sampling, plan flags), spawns `colibri` or sibling engine; `COLI_ENGINE` override. HTTP is **not** in C: `openai_server.py` talks the **serve wire protocol** to the engine process (`docs/api.md`, `docs/serve_protocol.md`).

## 3. “Public” surfaces (what exists today)

### A. DeepSeek V4 experimental C API (best existing embed shape)

Header: **`c/deepseek_v4.h`** (explicitly *experimental, may change*, lines 72–76).

| Piece | Symbols |
|-------|---------|
| Config | `coli_v4_config_parse` / `coli_v4_config_load`, `ColiDeepSeekV4Config` |
| Prompt | `coli_v4_prompt_build`, modes chat/thinking/raw |
| Engine | `coli_v4_engine_open` / `destroy`, memory summary, config accessors |
| Session | `coli_v4_session_create` / `destroy` / `generate` (token callback `ColiV4SessionTokenFn`) / `generated_text` |

Opaque `ColiV4Engine` / `ColiV4Session`. Still **linked only into the `deepseek_v4` binary** (`Makefile.deepseek-v4` amalgamates units with `-DCOLI_V4_UNIT_*`). Tests: `tests/test_v4_ownership.c`, `tests/test_deepseek_v4.c`. **Not** a shipped shared library.

### B. Serve protocol (de facto process ABI for main GLM engine)

`docs/serve_protocol.md`: stdin/stdout line protocol after `SERVE=1` [+ `SERVE_BATCH=1` for mux].

- Handshake: `\x01\x01READY\x01\x01`, `STAT`, `HWINFO`, …
- Mux: `SUBMIT` / `STOP` / `CANCEL` → `DATA` / `DONE` / `ERROR …`
- Used by `openai_server.py`, `coli chat` / `coli serve`. Forward-compat rule: ignore unknown line kinds.

This is the **stable product integration path** without linking C.

### C. GPU backend ABI (internal, not app-facing)

`c/backend_cuda.h`: `coli_cuda_init`, tensor upload, matmul, expert_mlp, expert_group, attention_absorb, … Marked `COLI_CUDA_DLLEXPORT` for Windows DLL. Host resolves via `backend_loader.c` GetProcAddress on Windows. **For embedding the full engine, you get this transitively; you do not call it as the app API.**

### D. Tool ctypes bridges only

`c/tools/rans_ctypes.c`, `tools/iq3_encode.c`: small `RC_EXPORT` / `-fPIC -shared` for Python offline tools. Pattern for a future engine FFI, not inference itself.

### E. Headers are not a stable public SDK

`st.h`, `tok.h`, `quant.h`, `tensor.h`, `json.h`, `compat.h`, … are **implementation headers** included into the amalgamation / unit builds. No versioned soname, no `COLIBRI_API` export macro on the main engine.

## 4. Model load: paths, formats, env

- **Directory model** (not a single GGUF): `SNAP` / `COLI_MODEL` points at a dir with:
  - `config.json` (and often `generation_config.json` for stops)
  - `*.safetensors` shards (largest shard used as index entry point; multi-shard layout)
  - tokenizer assets (engine-specific; GLM path uses in-engine tok + JSON)
- **Split disks**: `COLI_MODEL_DIRS`, mirrors via `COLI_MODEL_MIRROR` / CLI (`c/coli` help).
- **Quant formats**: internal `QT.fmt` registry in `docs/FORMATS.md` (int4/int8/fp8/E8/…); conversion tools under `c/tools/`.
- **Runtime knobs**: large set documented in `docs/ENVIRONMENT.md` (`RAM_GB`, `CTX`, `NGEN`, `COLI_TEMP`, `KV_SLOTS`, GPU toggles `COLI_CUDA`/`COLI_METAL`/`COLI_VULKAN`, `PIN`, `PIPE`, …). `coli` maps flags → env (`docs/SETTINGS.md`).
- **Sibling engines** do **not** share one knob set (ENVIRONMENT.md table: colibri vs kimi_k3 vs inkling vs olmoe).

## 5. C ABI stability / version symbols / embed docs

| Item | Status |
|------|--------|
| Engine shared library | **None** for inference |
| C version symbol | **None** (Python `c/version.py` → `__version__ = "1.5.0"`) |
| V4 engine/session API | Documented as **experimental** in-header |
| Serve protocol | Documented product contract (`docs/serve_protocol.md`) |
| GPU `coli_cuda_*` | DLL export surface for backends; versioned by build, not semver |
| Dedicated “embedding” docs | **None** found |

## 6. Recommended FFI boundary for `colibri-sys` / Rust

### Prefer short term (no fork of engine layout)

1. **Subprocess + serve mux protocol** (or `coli serve` + HTTP OpenAI API).
   - Matches existing product design; survives re-exec OMP tuning; isolation for huge RSS / OpenMP / GPU drivers.
   - Rust crate owns framing (`READY`, `SUBMIT`, `DATA`, `DONE`), not matmul kernels.
2. Optional: spawn `libexec/colibri` with `COLI_ENGINE` / same env as `c/coli`.

### Medium term (true link-time embed)

1. **Copy the V4 shape** for the main GLM engine: opaque `ColiEngine` / `ColiSession`, `open` / `generate(token_fn)` / `destroy`, error buffers, explicit options structs (not hundreds of getenv sites).
2. **Extract from CLI** (work that does not exist yet):
   - Factor `main` into: OMP re-exec policy (or disable for library), env→options parse (or pure options API), model load, then mode dispatch.
   - Make serve / prompt / TF call library entry points; keep `main` thin.
   - Guard re-exec: library builds must use `COLI_NO_OMP_TUNE` or a dedicated `coli_runtime_init()` that does not `execv`.
   - Stop relying on process-global state where possible (today many `static` / globals in `colibri.c`).
3. **Build product**:
   - `libcolibri.a` (static) first; optional `.so` later with an explicit export list.
   - Keep GPU objects optional: same `CUDA=1`/`METAL=1`/`VK=1` feature matrix as Make.
4. **Do not** treat linking raw `colibri.c` + calling into statics as the public ABI without the extraction above (undefined symbol surface, `main` collision, re-exec, OpenMP, thread-local globals).

### V4-only crate note

If scope is DeepSeek-V4 only, `deepseek_v4.h` is already a plausible bindgen target **after** adding a static/shared library target that **omits** `main` (today `deepseek_v4.c` still contains CLI mains). Stability still experimental.

## 7. Build integration options for Rust

| Approach | Fit | Notes |
|----------|-----|--------|
| **`build.rs` → `make -C c colibri …`** | Best first step for **binary path** or vendored artifact | Reuse ARCH/CUDA/METAL/VK matrix, `.build-config` stamp; then spawn binary or ship next to crate |
| **`cc` crate compile `colibri.c`** | Feasible for static link after **`#ifndef COLIBRI_NO_MAIN`** (or split TU) | Must mirror Makefile CFLAGS (`-O3`, `-fopenmp`, `-pthread`, arch), include path `c/`, optional CUDA via `cuda` crate / custom link; macOS libomp paths fragile |
| **cmake** | Overkill | Upstream is Make-only; wrapping Make is lower friction |
| **bindgen on `deepseek_v4.h`** | Good for V4 experimental API | Only after a lib target without `main` |
| **bindgen on whole of `colibri.c` headers** | Poor | No clean API; macros/statics dominate |

**Practical `colibri-sys` phases:**

1. **sys as process wrapper**: find/build engine, set env, optional serve framing crate.
2. **Optional `links = "colibri"`** static: after upstream (or fork) adds `libcolibri` + header + no-main.
3. **Feature flags** mirror Make: `cuda`, `metal`, `vulkan`, `hip` with platform `cfg` gates matching Makefile `$(error …)` rules.

## 8. Bottom line

- Today the C inference stack is a **family of binaries** (primarily `c/colibri` from one amalgamation) driven by **env + stdin serve protocol**, with a **Python CLI/HTTP shell**.
- **It cannot be linked as a library from Rust without new work**: no shared/static inference lib, no GLM C API, `main` + re-exec + process globals.
- Closest real C embed surface is **experimental DeepSeek V4** (`deepseek_v4.h`); closest production integration surface is the **serve protocol** (or HTTP gateway).
- Recommended Rust path: **protocol/subprocess first**; treat a V4-style engine/session C API + `libcolibri` as the real FFI boundary for in-process embedding later.
