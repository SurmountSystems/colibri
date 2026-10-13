# Colibri local recipes.
# Run `just` (or `just --list`) to print every recipe.

set shell := ["bash", "-euo", "pipefail", "-c"]
set dotenv-load := false

# Default: list all recipes (what bare `just` does).
default:
    @just --list

# ---------------------------------------------------------------------------
# Install
# ---------------------------------------------------------------------------

# Install colibri-native into ~/.cargo/bin (default: install + ffi).
# AMD/ROCm: `just install features=install,ffi-hip`
# Process-only (no static FFI libs): `just install features=install`
install features="install,ffi":
    cargo install --path crates/colibri-native --locked --force --no-default-features --features '{{ features }}'

# ---------------------------------------------------------------------------
# Comprehensive local CI (mirrors .github/workflows check.yml + ci.yml where practical)
# Skips GPU-only / Docker-only jobs that need toolkits or large images.
# ---------------------------------------------------------------------------

# Full local gate: Rust (fmt → clippy → nextest), C `make check`, Python, web.
check: rust-fmt rust-clippy rust-nextest c-check python-test web-check
    @echo ""
    @echo "just check: all local gates passed."

# ---------------------------------------------------------------------------
# Rust (workspace: colibri-sys + colibri-native)
# Order is fixed: 1) fmt --all  2) clippy all targets/features  3) nextest
# ---------------------------------------------------------------------------

# 1. rustfmt every workspace file; fail if anything would change.
rust-fmt:
    cargo fmt --all -- --check

# Shared HIP/CUDA compiler probe for rust-clippy and rust-nextest.
# stdout: HIP=0|1 and CUDA=0|1. Warnings go to stderr. Does not fail when a
# toolkit or card is missing (same warn-and-skip rules as clippy).
[private]
_gpu-ffi-compilers:
    #!/usr/bin/env bash
    set -euo pipefail
    have_hipcc=0
    have_amd_gpu=0
    have_nvcc=0
    have_nvidia_gpu=0
    if command -v hipcc >/dev/null 2>&1; then have_hipcc=1; fi
    if [[ -e /dev/kfd ]] || { command -v rocminfo >/dev/null 2>&1 && rocminfo >/dev/null 2>&1; }; then
      have_amd_gpu=1
    fi
    if command -v nvcc >/dev/null 2>&1; then have_nvcc=1; fi
    if command -v nvidia-smi >/dev/null 2>&1 && nvidia-smi -L >/dev/null 2>&1; then
      have_nvidia_gpu=1
    fi

    # Compile HIP/CUDA feature sets when the compiler is on PATH. Hardware
    # without a toolkit cannot clippy those cfgs; toolkit without a card still can.
    run_hip=0
    run_cuda=0
    if [[ $have_hipcc -eq 1 ]]; then
      run_hip=1
    elif [[ $have_amd_gpu -eq 1 ]]; then
      echo "warning: AMD GPU visible but hipcc is not on PATH; skipping ffi-hip" >&2
    else
      echo "warning: no AMD ROCm/HIP hardware or hipcc; skipping ffi-hip" >&2
    fi
    if [[ $have_nvcc -eq 1 ]]; then
      run_cuda=1
    elif [[ $have_nvidia_gpu -eq 1 ]]; then
      echo "warning: NVIDIA GPU visible but nvcc is not on PATH; skipping ffi-cuda" >&2
    else
      echo "warning: no NVIDIA CUDA hardware or nvcc; skipping ffi-cuda" >&2
    fi
    printf 'HIP=%s\nCUDA=%s\n' "$run_hip" "$run_cuda"

# 2. clippy all targets + all compileable features; warn+skip HIP/CUDA if missing.
# Fail on warnings (`-D warnings`). HIP/CUDA feature sets run only when the
# matching compiler is on PATH. Missing NVIDIA or AMD stack: warn and skip
# (do not fail the gate). colibri-sys cannot take literal `--all-features`:
# `ffi-cuda` and `ffi-hip` are mutually exclusive (build.rs panics).
rust-clippy:
    #!/usr/bin/env bash
    set -euo pipefail
    deny=(-D warnings)
    sys_cpu=(runtime,stream,tokio,install,ffi)
    eval "$(just _gpu-ffi-compilers)"

    native_features="install,ffi"
    if [[ ${HIP} -eq 1 ]]; then
      native_features="install,ffi,ffi-hip"
    fi
    echo "==> clippy colibri-native --all-targets --features ${native_features}"
    cargo clippy -p colibri-native --all-targets --features "${native_features}" -- "${deny[@]}"

    echo "==> clippy colibri-sys --all-targets --features ${sys_cpu[*]}"
    cargo clippy -p colibri-sys --all-targets --features "${sys_cpu[*]}" -- "${deny[@]}"

    if [[ ${HIP} -eq 1 ]]; then
      echo "==> clippy colibri-sys --all-targets --features ${sys_cpu[*]},ffi-hip"
      cargo clippy -p colibri-sys --all-targets --features "${sys_cpu[*]},ffi-hip" -- "${deny[@]}"
    fi
    if [[ ${CUDA} -eq 1 ]]; then
      echo "==> clippy colibri-sys --all-targets --features ${sys_cpu[*]},ffi-cuda"
      cargo clippy -p colibri-sys --all-targets --features "${sys_cpu[*]},ffi-cuda" -- "${deny[@]}"
    fi

# 3. cargo nextest for the whole workspace (lib + bins + tests).
# GPU extras match rust-clippy on this machine: hipcc → ffi-hip, else nvcc →
# ffi-cuda, else install,ffi only. Never both vendors. Same warn-and-skip as
# clippy (missing toolkit does not fail the gate).
rust-nextest:
    #!/usr/bin/env bash
    set -euo pipefail
    eval "$(just _gpu-ffi-compilers)"
    features="install,ffi"
    if [[ ${HIP} -eq 1 ]]; then
      features="install,ffi,ffi-hip"
    elif [[ ${CUDA} -eq 1 ]]; then
      features="install,ffi,ffi-cuda"
    fi
    echo "==> nextest --workspace --all-targets --features ${features}"
    cargo nextest run --workspace --all-targets --features "${features}"

# ---------------------------------------------------------------------------
# C engine (dependency-free CPU gate used by check.yml)
# ---------------------------------------------------------------------------

# Parallel make jobs for the C engine. Default: one per logical CPU.
# Override: `just c-check make_jobs=4`
# If MAKEFLAGS already has -j or a jobserver, do not add another -j (that
# would start a new jobserver and ignore the caller). Nested recipes in
# c/Makefile use $(MAKE) so they share this jobserver.
make_jobs := num_cpus()
make_flags := env_var_or_default("MAKEFLAGS", "")
make_j := if make_flags =~ "--jobserver" { "" } else if make_flags =~ "-j" { "" } else { "-j" + make_jobs }

# Official portable gate: clean + portable CPU build + C unit suites + Python stdlib tests.
c-check:
    make -C c check {{ make_j }}

# C unit binaries only (faster than full `make check` when iterating on C).
c-test:
    make -C c test-c {{ make_j }}

# ---------------------------------------------------------------------------
# Python (c/tests — same suite as ci.yml python job)
# ---------------------------------------------------------------------------

# Discover and run all c/tests/test_*.py (stdlib unittest; no extra pip deps).
python-test:
    cd c && python3 -m unittest discover -s tests -p 'test_*.py' -v

# ---------------------------------------------------------------------------
# Web UI (TypeScript / Vite / Vitest)
# ---------------------------------------------------------------------------

# npm ci + production build + vitest (matches ci.yml web job).
web-check: web-install
    cd web && npm run build
    cd web && npm test

web-install:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ ! -d web/node_modules ]]; then
      (cd web && npm ci)
    else
      echo "web/node_modules present; skip npm ci (run \`just web-install-force\` to refresh)"
    fi

web-install-force:
    cd web && npm ci

# ---------------------------------------------------------------------------
# Optional extras (not part of default `just check`)
# ---------------------------------------------------------------------------

# Build every process engine binary (colibri, inkling, kimi_k3, olmoe). Slow.
engines-build:
    #!/usr/bin/env bash
    set -euo pipefail
    cd c
    rc=0
    for t in colibri inkling kimi_k3 olmoe; do
      echo "==> make $t"
      make "$t" || { echo "FAILED: $t"; rc=1; }
    done
    exit "$rc"

# CUDA .cu syntax-only compile if nvcc is on PATH (skips cleanly otherwise).
cuda-syntax:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! command -v nvcc >/dev/null 2>&1; then
      echo "warning: no nvcc; skipping CUDA syntax check" >&2
      exit 0
    fi
    cd c
    nvcc -O2 -std=c++17 -arch=sm_80 -c backend_cuda.cu -o /dev/null -Xcompiler=-Wall,-Wextra
    nvcc -O2 -std=c++17 -arch=sm_80 -c backend_cuda_ink.cu -o /dev/null -Xcompiler=-Wall,-Wextra
    echo "CUDA syntax check passed"

# Extended gate: default check plus engines build and optional CUDA syntax.
check-extra: check engines-build cuda-syntax
    @echo "just check-extra: extended gates passed."
