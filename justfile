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

# Install colibri-native into ~/.cargo/bin.
# Default `auto`: HIP (`install,ffi-hip`) when hipcc is on PATH (same probe as
# clippy), else CPU (`install,ffi`). Prints the flavor. Refuses a CPU-only
# install when `c/libcolibri.a` already has HIP or CUDA objects, so that
# archive is not rebuilt as CPU. Override: `just install install,ffi-hip`
# (or `just install features=install,ffi-hip`). CPU: `just install install,ffi`.
# Process-only: `just install install`.
# Auto HIP when hipcc is on PATH, else CPU. Prints the flavor.
install features="auto":
    #!/usr/bin/env bash
    set -euo pipefail
    eval "$(just _install-native-features '{{ features }}')"
    echo "just install: installing ${FLAVOR} (features ${FEATURES})"
    if [[ "${FLAVOR}" == "HIP" ]]; then
      export COLIBRI_REQUIRE_FFI_HIP=1
    fi
    cargo install --path crates/colibri-native --locked --force --no-default-features --features "${FEATURES}"
    echo "just install: installed ${FLAVOR} (features ${FEATURES})"

# Resolve cargo features for `just install`. stdout is FEATURES= and FLAVOR=
# (eval-friendly). hip/cuda/archive: `live` probes this host; tests inject
# 0|1 and cpu|hip|cuda|none. Do not rebuild a GPU archive as CPU-only.
[private]
_install-native-features features="auto" hip="live" cuda="live" archive="live":
    #!/usr/bin/env bash
    set -euo pipefail
    features='{{ features }}'
    hip_probe='{{ hip }}'
    cuda_probe='{{ cuda }}'
    archive_probe='{{ archive }}'
    # `just install features=install,ffi-hip` passes that whole token as the
    # first argument. Strip the name prefix so the documented override works.
    if [[ $features == features=* ]]; then
      features=${features#features=}
    fi
    if [[ $hip_probe == hip=* ]]; then
      hip_probe=${hip_probe#hip=}
    fi
    if [[ $cuda_probe == cuda=* ]]; then
      cuda_probe=${cuda_probe#cuda=}
    fi
    if [[ $archive_probe == archive=* ]]; then
      archive_probe=${archive_probe#archive=}
    fi

    archive_flavor() {
      local path=$1
      if [[ ! -s $path ]]; then
        echo none
        return
      fi
      if grep -a -q -F -e '__hipUnregisterFatBinary' -e '__hipRegisterFatBinary' "$path"; then
        echo hip
      elif grep -a -q -F -e '__cudaUnregisterFatBinary' -e '__cudaRegisterFatBinary' "$path"; then
        echo cuda
      else
        echo none
      fi
    }

    refuse_cpu_wipe() {
      local kind=$1
      echo "error: refusing CPU-only native install because a ${kind} engine archive is present. That path rebuilds c/libcolibri.a as CPU and wipes ${kind} objects. Use HIP (hipcc on PATH, or just install install,ffi-hip)." >&2
      exit 1
    }

    case $archive_probe in
      live) arch_flavor=$(archive_flavor c/libcolibri.a) ;;
      hip|cuda|cpu|none) arch_flavor=$archive_probe ;;
      *) echo "error: unknown archive probe ${archive_probe} (live|hip|cuda|cpu|none)" >&2; exit 1 ;;
    esac
    if [[ $arch_flavor == cpu ]]; then
      arch_flavor=none
    fi

    run_hip=0
    if [[ $hip_probe == live ]]; then
      eval "$(just _gpu-ffi-compilers)"
      run_hip=${HIP}
    elif [[ $hip_probe == 1 ]]; then
      run_hip=1
    elif [[ $hip_probe == 0 ]]; then
      run_hip=0
    else
      echo "error: unknown hip probe ${hip_probe} (live|0|1)" >&2
      exit 1
    fi
    # Native has no ffi-cuda feature. cuda_probe is only for tests that inject
    # a leftover CUDA archive (still refuse a CPU wipe).
    if [[ $cuda_probe == live ]]; then
      :
    elif [[ $cuda_probe == 0 || $cuda_probe == 1 ]]; then
      :
    else
      echo "error: unknown cuda probe ${cuda_probe} (live|0|1)" >&2
      exit 1
    fi

    has_ffi=0
    has_hip=0
    if [[ $features == auto || -z $features ]]; then
      if [[ $run_hip -eq 1 ]]; then
        features=install,ffi-hip
      else
        features=install,ffi
      fi
    fi
    IFS=',' read -ra parts <<< "$features"
    for p in "${parts[@]}"; do
      case $p in
        ffi) has_ffi=1 ;;
        ffi-hip) has_hip=1; has_ffi=1 ;;
      esac
    done

    flavor=cpu
    if [[ $has_hip -eq 1 ]]; then
      flavor=HIP
    elif [[ $has_ffi -eq 1 ]]; then
      flavor=cpu
    else
      flavor=process
    fi

    if [[ $flavor == cpu && ( $arch_flavor == hip || $arch_flavor == cuda ) ]]; then
      if [[ $arch_flavor == hip ]]; then
        refuse_cpu_wipe HIP
      else
        refuse_cpu_wipe CUDA
      fi
    fi

    printf 'FEATURES=%s\nFLAVOR=%s\n' "$features" "$flavor"

# Recipe contract for install feature selection (no cargo install).
# Injected probes: HIP when hipcc would be present, CPU otherwise, refuse when
# a GPU archive would be wiped. Live probe must match `_gpu-ffi-compilers`.
# Feature-selection contract for `just install` (does not run cargo install).
test-install-native-features:
    #!/usr/bin/env bash
    set -euo pipefail
    fail=0
    expect_ok() {
      local label=$1 features=$2 hip=$3 cuda=$4 archive=$5 want_features=$6 want_flavor=$7
      local out rc
      set +e
      out=$(just _install-native-features "$features" "$hip" "$cuda" "$archive" 2>/dev/null)
      rc=$?
      set -e
      if [[ $rc -ne 0 ]]; then
        echo "FAIL ${label}: expected exit 0, got ${rc}" >&2
        fail=1
        return
      fi
      local got_features got_flavor
      got_features=$(printf '%s\n' "$out" | sed -n 's/^FEATURES=//p' | tail -n1)
      got_flavor=$(printf '%s\n' "$out" | sed -n 's/^FLAVOR=//p' | tail -n1)
      if [[ $got_features != "$want_features" || $got_flavor != "$want_flavor" ]]; then
        echo "FAIL ${label}: got FEATURES=${got_features} FLAVOR=${got_flavor}, want FEATURES=${want_features} FLAVOR=${want_flavor}" >&2
        fail=1
      else
        echo "ok ${label}: FEATURES=${got_features} FLAVOR=${got_flavor}"
      fi
    }
    expect_fail() {
      local label=$1 features=$2 hip=$3 cuda=$4 archive=$5
      local rc err
      set +e
      err=$(just _install-native-features "$features" "$hip" "$cuda" "$archive" 2>&1 >/dev/null)
      rc=$?
      set -e
      if [[ $rc -eq 0 ]]; then
        echo "FAIL ${label}: expected refuse, got exit 0" >&2
        fail=1
        return
      fi
      if [[ $err != *wipes* ]]; then
        echo "FAIL ${label}: refuse exit ${rc} but stderr lacked wipe notice: ${err}" >&2
        fail=1
      else
        echo "ok ${label}: refused CPU wipe (exit ${rc})"
      fi
    }

    expect_ok "auto hipcc" auto 1 0 cpu install,ffi-hip HIP
    expect_ok "auto hipcc keeps HIP archive" auto 1 0 hip install,ffi-hip HIP
    expect_ok "auto no toolkit" auto 0 0 cpu install,ffi cpu
    expect_ok "auto nvcc only is still CPU (native has no ffi-cuda)" auto 0 1 cpu install,ffi cpu
    expect_ok "explicit HIP" install,ffi-hip 0 0 cpu install,ffi-hip HIP
    expect_ok "features= prefix override" features=install,ffi-hip 0 0 cpu install,ffi-hip HIP
    expect_ok "explicit CPU" install,ffi 1 0 cpu install,ffi cpu
    expect_ok "process-only does not wipe" install 0 0 hip install process
    expect_fail "auto HIP archive without hipcc" auto 0 0 hip
    expect_fail "auto CUDA archive without GPU features" auto 0 0 cuda
    expect_fail "explicit CPU would wipe HIP" install,ffi 1 0 hip

    eval "$(just _gpu-ffi-compilers 2>/dev/null)"
    live=$(just _install-native-features auto 2>/dev/null)
    live_flavor=$(printf '%s\n' "$live" | sed -n 's/^FLAVOR=//p' | tail -n1)
    live_features=$(printf '%s\n' "$live" | sed -n 's/^FEATURES=//p' | tail -n1)
    if [[ ${HIP} -eq 1 ]]; then
      if [[ $live_flavor != HIP || $live_features != install,ffi-hip ]]; then
        echo "FAIL live: HIP=1 from _gpu-ffi-compilers but got FEATURES=${live_features} FLAVOR=${live_flavor}" >&2
        fail=1
      else
        echo "ok live: hipcc present → FEATURES=${live_features} FLAVOR=${live_flavor}"
      fi
    else
      if [[ $live_flavor == HIP ]]; then
        echo "FAIL live: HIP=0 from _gpu-ffi-compilers but flavor is HIP" >&2
        fail=1
      else
        echo "ok live: no hipcc → FEATURES=${live_features} FLAVOR=${live_flavor}"
      fi
    fi

    if ! just --show install | grep -q _install-native-features; then
      echo "FAIL just install recipe does not call _install-native-features" >&2
      fail=1
    else
      echo "ok just install uses _install-native-features"
    fi

    if [[ $fail -ne 0 ]]; then
      echo "test-install-native-features: FAILED" >&2
      exit 1
    fi
    echo "test-install-native-features: passed"

# ---------------------------------------------------------------------------
# Comprehensive local CI: every Linux job in check.yml and ci.yml.
# Not invoked (other operating systems, not this x86_64 Linux host):
# check.yml windows (Windows UCRT64) and macos (macOS Metal / metal-test).
# Shared `make -C c check` still runs via c-check. Linux v4-tiny covers the
# shared deepseek-v4-tiny-check. ci.yml musl (Alpine) is not run. The macos
# and windows rows of engines-all-platforms are not run (Metal, XDNA=1,
# Windows backend-loader tests); the ubuntu row is engines-linux. ci.yml
# windows-cuda-build is not run. ci.yml windows-python-focused is not run as
# a Windows job; those three modules stay in the Linux python discover.
# ci.yml oracle-arm is not run. Missing nvcc skips only the CUDA syntax
# compile. If nvcc is on PATH and that compile fails, the gate fails.
# An unusable Docker API skips engine-hip-syntax and the image build.
# If docker info works and either of those fails, the gate fails.
# ---------------------------------------------------------------------------

# One invocation. Rust, then CUDA/HIP syntax, Docker, web, GGUF, engines,
# tiny oracles, sanitizers, Vulkan, then c-check and Python.
# c-check stays immediately before python-test: `make check` cleans, and
# python-test rebuilds the engines the python CI job builds.
# (python-test "1") sets COLI_OMIT_TOK_S_TIMING so the tok/s tests that
# already ran inside c-check are not measured again.
check: rust-fmt rust-clippy rust-nextest \
    engine-cuda-syntax engine-hip-syntax docker web-check gguf \
    engine engines-linux \
    k3-tiny mimo-tiny v4-tiny glm53-oracle \
    qwen36-tiny-check olmoe-tiny-check dsv41-tiny-check qwen38-tiny-check \
    laya-tiny-check gliner-decide-tiny-check clef-tiny-check jev-sdk-check qwenimage-tiny-check \
    inkling-oracle efficiency cluster-parity \
    sanitizers vulkan vulkan-engines \
    c-check (python-test "1")
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
# Parent OMP_NUM_THREADS is a build cap, not an operator engine override.
# Unset it so C CLI tests see physical cores (test_other_engines_get_physical_cores).
c-check:
    #!/usr/bin/env bash
    set -euo pipefail
    unset OMP_NUM_THREADS
    make -C c check {{ make_j }}

# C unit binaries only (faster than full `make check` when iterating on C).
c-test:
    make -C c test-c {{ make_j }}

# ---------------------------------------------------------------------------
# Python (c/tests — same suite as ci.yml python job)
# ---------------------------------------------------------------------------

# Discover and run all c/tests/test_*.py (stdlib unittest; no extra pip deps).
# omit_tok_s=1 is only for `just check`, after c-check already measured
# test_tiny_tok_s_floor and test_cpu_vs_cpu_tok_s_stability once.
# Same discover from c/. A bare `just python-test` still runs both.
python-test omit_tok_s="0":
    #!/usr/bin/env bash
    set -euo pipefail
    # Parent OMP_NUM_THREADS is a build cap, not an operator engine override.
    unset OMP_NUM_THREADS
    # Same engines as ci.yml job python, before discover. A bare
    # `just python-test` still builds them and still runs the tok/s tests.
    make -C c {{ make_j }} kimi_k3 inkling colibri olmoe qwen38 qwen36 deepseek_v41 qwenimage
    cd c
    if [[ "{{ omit_tok_s }}" == "1" ]]; then
      export COLI_OMIT_TOK_S_TIMING=1
    fi
    python3 -m unittest discover -s tests -p 'test_*.py' -v

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
# Linux CI jobs (check.yml + ci.yml). Venvs live under target/just-check-all
# so a full gate does not fill /tmp. One venv per requirements set.
# ---------------------------------------------------------------------------

# Create target/just-check-all/venvs/<name> and install each argument.
# A path that exists is `pip install -r`. Anything else is a package name.
# Rebuild when a requirements file is newer than the stamp, or the arg list changed.
[private]
_venv name *reqs:
    #!/usr/bin/env bash
    set -euo pipefail
    root=$(pwd)
    name="{{ name }}"
    # shellcheck disable=SC2206
    reqs=({{ reqs }})
    work="$root/target/just-check-all"
    venv="$work/venvs/$name"
    stamp="$venv/.requirements.stamp"
    mkdir -p "$work/pip-cache" "$work/tmp"
    export PIP_CACHE_DIR="$work/pip-cache"
    export TMPDIR="$work/tmp"
    sig="$name"
    for a in "${reqs[@]}"; do
      sig+=$'\n'"$a"
      if [[ -f "$a" ]]; then
        sig+=$'\n'"$(stat -c %Y "$a")"
      fi
    done
    if [[ -x "$venv/bin/python3" && -f "$stamp" && "$(cat "$stamp")" == "$sig" ]]; then
      exit 0
    fi
    rm -rf "$venv"
    python3 -m venv "$venv"
    "$venv/bin/python3" -m pip install --upgrade pip
    for a in "${reqs[@]}"; do
      if [[ -f "$a" ]]; then
        "$venv/bin/python3" -m pip install -r "$a"
      else
        "$venv/bin/python3" -m pip install "$a"
      fi
    done
    printf '%s\n' "$sig" > "$stamp"

# ci.yml job engine.
engine:
    #!/usr/bin/env bash
    set -euo pipefail
    make -C c {{ make_j }} colibri inkling
    make -C c {{ make_j }} test-c

# Ubuntu row of ci.yml engines-all-platforms. One engine failure does not hide
# the next. No XDNA=1 and no METAL=1. deepseek-v4 is included because the
# ubuntu row sets v4=1.
engines-linux:
    #!/usr/bin/env bash
    set -euo pipefail
    cd c
    rc=0
    ENGINES="colibri glm53 inkling kimi_k3 olmoe qwen36 qwen38 deepseek_v41 mimo qwenimage laya gliner_decide"
    ENGINES="$ENGINES deepseek-v4"
    for t in $ENGINES; do
      echo "==> $t"
      make {{ make_j }} "$t" || { echo "FAILED: $t"; rc=1; }
    done
    make tests/test_compat_env && ./tests/test_compat_env || rc=1
    for t in tests/test_omp_tune tests/test_qwen36_ctx; do
      echo "==> $t"
      make "$t" && ./"$t" || rc=1
    done
    exit "$rc"

# ci.yml sanitizers. test-asan cleans.
sanitizers:
    #!/usr/bin/env bash
    set -euo pipefail
    make -C c {{ make_j }} test-asan
    make -C c {{ make_j }} fuzz-rans

# ci.yml docker. Build Dockerfile.slim. Lint docker/Dockerfile (not built).
# A missing or permission-denied Docker API skips this job. A build or
# hadolint failure still fails the gate when `docker info` succeeds.
docker:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! docker info >/dev/null 2>&1; then
      echo "warning: docker API is not usable; skipping image build and hadolint" >&2
      exit 0
    fi
    docker build -t colibri:ci -f docker/Dockerfile.slim .
    docker run --rm colibri:ci --version
    docker run --rm colibri:ci --help > /dev/null
    docker run --rm --entrypoint python3 colibri:ci -c "import openai_server; print('gateway imports')"
    docker run --rm --entrypoint /app/colibri colibri:ci --help 2>&1 | head -5 || true
    docker run --rm -i hadolint/hadolint < docker/Dockerfile

# ci.yml gguf. The four modules that need requirements-gguf.txt.
gguf: (_venv "gguf" "c/tools/requirements-gguf.txt")
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="$PWD/target/just-check-all/venvs/gguf/bin:$PATH"
    cd c
    python3 -m unittest -v \
      tests.test_gguf_reader \
      tests.test_gguf_dequant \
      tests.test_gguf_olmoe_profile \
      tests.test_convert_gguf_to_olmoe

# check.yml k3-tiny.
k3-tiny: (_venv "kimi-k3-tiny" "c/tools/requirements-kimi-k3-tiny.txt")
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="$PWD/target/just-check-all/venvs/kimi-k3-tiny/bin:$PATH"
    make -C c {{ make_j }} kimi-k3-tiny-check

# check.yml mimo-tiny, including the ASan/UBSan block.
mimo-tiny: (_venv "mimo-tiny" "c/tools/requirements-mimo-tiny.txt")
    #!/usr/bin/env bash
    set -euo pipefail
    root=$(pwd)
    export PATH="$root/target/just-check-all/venvs/mimo-tiny/bin:$PATH"
    make -C c mimo-tiny-check
    make -C c mimo-tiny-serve-check
    (
    cd "$root"
    make -C c clean >/dev/null
    make -C c mimo EXTRA_CFLAGS="-fsanitize=address,undefined -fno-omit-frame-pointer -g"
    # After the link. See c/tools/asan-preload.sh.
    source "$root/c/tools/asan-preload.sh"
    ASAN_OPTIONS=detect_leaks=0:detect_stack_use_after_return=0 UBSAN_OPTIONS=halt_on_error=1:print_stacktrace=1 \
      python c/tests/mimo_tiny_harness.py --binary c/mimo --fixture c/mimo_tiny
    cd c && ASAN_OPTIONS=detect_leaks=0:detect_stack_use_after_return=0 UBSAN_OPTIONS=halt_on_error=1:print_stacktrace=1 \
      python -m unittest tests.test_mimo_prefix_serve tests.test_brio_serve.MimoBrioServe \
      tests.test_systemone_api.MimoSystemOneEndToEnd
    )

# check.yml v4-tiny.
v4-tiny: (_venv "deepseek-v4-tiny" "c/tools/requirements-deepseek-v4-tiny.txt")
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="$PWD/target/just-check-all/venvs/deepseek-v4-tiny/bin:$PATH"
    make -C c {{ make_j }} deepseek-v4-tiny-check

# check.yml glm53-oracle. Each CI step starts at the repo root.
# Fixtures live under target/just-check-all/glm53, not /tmp. /tmp is a small
# memory disk and still holds the previous run. The recipe removes that
# directory before generating, so a second local run does not need --force.
# CI still generates into a fresh /tmp and does not use this recipe body.
glm53-oracle: (_venv "glm53-tiny" "c/tools/requirements-glm53-tiny.txt")
    #!/usr/bin/env bash
    set -euo pipefail
    root=$(pwd)
    export PATH="$root/target/just-check-all/venvs/glm53-tiny/bin:$PATH"
    fix="$root/target/just-check-all/glm53"
    rm -rf "$fix"
    (
    cd "$root"
    python c/tools/make_glm53_tiny.py --output "$fix/glm53_tiny"
    python c/tools/check_glm53_checkpoint.py --model "$fix/glm53_tiny"
    )
    (
    cd "$root"
    make -C c glm53
    python c/tools/make_glm53_multimodal_tiny.py --output "$fix/glm53_mm"
    python c/tools/make_glm53_streaming_pair.py --fixture "$fix/glm53_mm" --output "$fix/glm53_stream"
    python c/tools/make_edge_tiny_tokenizer.py --vocab-size 281 "$fix/glm53_stream-i4"
    cd c && COLI_GLM53_FIXTURE="$fix/glm53_stream-i4" python -m unittest -v tests.test_glm53_dashboard
    COLI_GLM53_FIXTURE="$fix/glm53_stream-i4" python -m unittest -v tests.test_glm53_context_exceeded
    COLI_GLM53_FIXTURE="$fix/glm53_stream-i4" python -m unittest -v tests.test_glm53_hostile_weights
    COLI_GLM53_FIXTURE="$fix/glm53_mm" python -m unittest -v tests.test_glm53_hostile_weights.Glm53MatrixShapeTest
    )
    (
    cd "$root"
    cd c && GLM53_TINY="$fix/glm53_tiny" GLM53_MM_TINY="$fix/glm53_mm" python -m unittest -v tests.test_glm53_oracles
    GLM53_TINY="$fix/glm53_tiny" COLI_GLM53_FIXTURE="$fix/glm53_stream-i4" python -m unittest -v tests.test_glm53_prefill_batch
    )

# ci.yml laya-tiny-check.
laya-tiny-check: (_venv "numpy" "numpy")
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="$PWD/target/just-check-all/venvs/numpy/bin:$PATH"
    make -C c laya-tiny-check

# ci.yml gliner-decide-tiny-check. Same numpy-only set as laya.
gliner-decide-tiny-check: (_venv "numpy" "numpy")
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="$PWD/target/just-check-all/venvs/numpy/bin:$PATH"
    make -C c gliner-decide-tiny-check

# ci.yml clef-tiny-check.
clef-tiny-check: (_venv "numpy-oracle" "numpy" "c/tools/oracle-requirements.txt")
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="$PWD/target/just-check-all/venvs/numpy-oracle/bin:$PATH"
    make -C c clef-tiny-check

# ci.yml jev-sdk-check. numpy is in this venv; the decide Vulkan family is not.
jev-sdk-check: (_venv "jev-sdk" "c/tools/requirements-jev-sdk.txt" "numpy" "c/tools/oracle-requirements.txt")
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="$PWD/target/just-check-all/venvs/jev-sdk/bin:$PATH"
    (cd c/tools/jev-sdk && npm ci)
    make -C c jev-sdk-check

# ci.yml olmoe-tiny-check. Shared oracle venv.
# make_olmoe_tiny.py exits when its output directory already exists.
# A second local run leaves c/olmoe_tiny and c/olmoe_tiny_c behind.
# Remove each one before generating only when git ls-files lists no
# tracked file under that path. Do not pass --force.
olmoe-tiny-check: (_venv "oracle" "c/tools/oracle-requirements.txt")
    #!/usr/bin/env bash
    set -euo pipefail
    root=$(pwd)
    export PATH="$root/target/just-check-all/venvs/oracle/bin:$PATH"
    for rel in c/olmoe_tiny c/olmoe_tiny_c; do
      tracked=$(git -C "$root" ls-files -- "$rel")
      if [[ -n "$tracked" ]]; then
        echo "skip $rel: git ls-files shows a tracked file" >&2
        continue
      fi
      if [[ -e "$root/$rel" ]]; then
        echo "removing untracked $rel"
        rm -rf "$root/$rel"
      fi
    done
    (
    cd "$root"
    cd c
    python3 tools/make_olmoe_tiny.py --output olmoe_tiny
    python3 tools/convert_olmoe_merged.py --model olmoe_tiny --out olmoe_tiny_c
    python3 tools/make_edge_tiny_tokenizer.py --vocab-size 128 olmoe_tiny_c
    )
    (
    cd "$root"
    cd c
    make olmoe
    SNAP=olmoe_tiny_c ./olmoe 8 8 olmoe_tiny/ref_olmoe.json | tee oracle.log
    grep -qE 'Matching tokens: ([0-9]+)/\1$' oracle.log
    )
    (
    cd "$root"
    cd c
    COLI_OLMOE_FIXTURE=olmoe_tiny_c COLI_OLMOE_REFERENCE=olmoe_tiny/ref_olmoe.json \
      python3 -m unittest -v tests.test_olmoe_dense_shape
    )
    (
    cd "$root"
    cd c
    COLI_OLMOE_FIXTURE=olmoe_tiny_c python3 -m unittest -v tests.test_olmoe_cap_budget
    )
    (
    cd "$root"
    cd c
    OLMOE_TINY=olmoe_tiny_c python3 -m unittest -v tests.test_olmoe_dashboard_hits
    )
    (
    cd "$root"
    cd c
    COLI_OLMOE_FIXTURE=olmoe_tiny_c python3 -m unittest -v tests.test_olmoe_context_exceeded
    )
    (
    cd "$root"
    cd c
    COLI_OLMOE_FIXTURE=olmoe_tiny_c python3 -m unittest -v tests.test_olmoe_reference_bounds
    )
    (
    cd "$root"
    cd c
    OLMOE_TINY=olmoe_tiny_c python3 -m unittest -v tests.test_olmoe_prefix_serve
    )
    (
    cd "$root"
    cd c
    OLMOE_TINY=olmoe_tiny_c python3 -m unittest -v tests.test_brio_serve
    )

# ci.yml inkling-oracle. Shared oracle venv.
inkling-oracle: (_venv "oracle" "c/tools/oracle-requirements.txt")
    #!/usr/bin/env bash
    set -euo pipefail
    root=$(pwd)
    export PATH="$root/target/just-check-all/venvs/oracle/bin:$PATH"
    (
    cd "$root"
    cd c && make inkling
    )
    (
    cd "$root"
    cd c
    python3 tools/make_tiny_inkling.py tiny_inkling
    for cap in 1 2 8; do
      SNAP=tiny_inkling ./inkling "$cap" 0 tiny_inkling/ref_inkling.json
    done
    )
    (
    cd "$root"
    cd c
    INKLING_TINY=tiny_inkling python3 -m unittest -v tests.test_inkling_prefix_serve
    )
    (
    cd "$root"
    cd c
    INKLING_TINY=tiny_inkling python3 -m unittest -v tests.test_inkling_dashboard_hits
    )

# ci.yml efficiency. Shared oracle venv. The integer-kernel binary is
# target/just-check-all/tike, not /tmp/tike. --fp8 stays after the bf16 assertions.
efficiency: (_venv "oracle" "c/tools/oracle-requirements.txt")
    #!/usr/bin/env bash
    set -euo pipefail
    root=$(pwd)
    export PATH="$root/target/just-check-all/venvs/oracle/bin:$PATH"
    mkdir -p "$root/target/just-check-all"
    make -C c colibri
    (
    cd "$root"
    cd c
    gcc -O3 -march=native -fopenmp -I. tests/test_int_kernel_exact.c -o "$root/target/just-check-all/tike" -lm
    "$root/target/just-check-all/tike"
    )
    (
    cd "$root"
    cd c && python3 tools/make_glm_oracle.py
    )
    (
    cd "$root"
    cd c
    SNAP=./glm_tiny TF=1 COLI_TEMP=0 ORACLE_STRICT=1 ORACLE_TF_MAX_MISMATCHES=2 ./colibri 64 16 16
    SNAP=./glm_tiny COLI_TEMP=0 ORACLE_STRICT=1 ./colibri 64 16 16
    )
    (
    cd "$root"
    cd c && python3 tests/test_glm_oracle.py
    )
    (
    cd "$root"
    cd c
    python3 -m unittest -v \
      tests.test_inefficiency.TinyEfficiencyTest.test_telemetry_parses \
      tests.test_inefficiency.TinyEfficiencyTest.test_profile_phases_present_and_nonneg \
      tests.test_inefficiency.TinyEfficiencyTest.test_disk_wait_not_dominant \
      tests.test_inefficiency.TinyEfficiencyTest.test_cpu_vs_cpu_determinism
    )
    (
    cd "$root"
    cd c
    python3 tools/make_glm_oracle.py --fp8
    python3 tools/convert_fp8_to_int4.py --indir glm_tiny --outdir glm_tiny_i4 --ebits 4 --io-bits 4
    cp ref_glm.json glm_tiny_i4/ref_glm.json
    )
    (
    cd "$root"
    cd c && python3 -m unittest -v tests.test_trunk_resident_layers
    )

# ci.yml cluster-parity. Shared oracle venv.
cluster-parity: (_venv "oracle" "c/tools/oracle-requirements.txt")
    #!/usr/bin/env bash
    set -euo pipefail
    root=$(pwd)
    export PATH="$root/target/just-check-all/venvs/oracle/bin:$PATH"
    make -C c colibri
    (
    cd "$root"
    cd c && python3 tools/make_glm_oracle.py --fmt6 && python3 tools/make_glm_oracle.py --fmt4
    )
    (
    cd "$root"
    cd c && python3 -m unittest -v tests.test_cluster_sharding
    )

# ci.yml qwenimage-tiny-check. torch 2.14, its own venv.
qwenimage-tiny-check: (_venv "qwenimage-tiny" "c/tools/requirements-qwenimage-tiny.txt")
    #!/usr/bin/env bash
    set -euo pipefail
    root=$(pwd)
    export PATH="$root/target/just-check-all/venvs/qwenimage-tiny/bin:$PATH"
    (
    cd "$root"
    cd c
    python3 tools/make_qwenimage_tiny.py qwenimage_tiny
    make qwenimage tests/test_qi_gemm tests/test_qwenimage_vae
    tests/test_qi_gemm
    COLI_IMG_BITS=16 COLI_IMG_ACT8=0 ./qwenimage --model qwenimage_tiny --ref qwenimage_tiny/ref
    )
    (
    cd "$root"
    cd c
    python3 tools/make_qwenimage_vae_tiny.py qwenimage_vae_tiny
    tests/test_qwenimage_vae qwenimage_vae_tiny
    )
    (
    cd "$root"
    cd c
    ./qwenimage --model qwenimage_tiny --prompt "a red fox in the snow" \
                --width 256 --height 256 --steps 2 --seed 1 --out tiny.png
    python3 -c "import sys; sys.path.insert(0, '.'); import image_engine as e; w, h, c, px = e.decode_png(open('tiny.png', 'rb').read()); assert (w, h, c) == (256, 256, 4), (w, h, c)"
    QWENIMAGE_TINY=qwenimage_tiny python3 -m unittest -v tests.test_qwenimage_engine_serve
    )
    (
    cd "$root"
    cd c
    make -B qwenimage EXTRA_CFLAGS="-fsanitize=address,undefined -fno-omit-frame-pointer -g"
    source "$root/c/tools/asan-preload.sh"
    export ASAN_OPTIONS=detect_leaks=0:detect_stack_use_after_return=0 UBSAN_OPTIONS=print_stacktrace=1 OMP_NUM_THREADS=2
    COLI_IMG_BITS=16 COLI_IMG_ACT8=0 ./qwenimage --model qwenimage_tiny --ref qwenimage_tiny/ref > qwenimage-san.log 2>&1 || true
    QWENIMAGE_TINY=qwenimage_tiny python3 -m unittest tests.test_qwenimage_engine_serve >> qwenimage-san.log 2>&1 || true
    if grep -qE "ERROR: AddressSanitizer|runtime error:" qwenimage-san.log; then
      cat qwenimage-san.log
      exit 1
    fi
    grep -q "all stages within tolerance" qwenimage-san.log
    )

# ci.yml qwen38-tiny-check, including FP8, int4, MTP, ASan, the malformed
# container, and the dashboard unittests.
qwen38-tiny-check: (_venv "qwen38-tiny" "c/tools/requirements-qwen38-tiny.txt")
    #!/usr/bin/env bash
    set -euo pipefail
    root=$(pwd)
    export PATH="$root/target/just-check-all/venvs/qwen38-tiny/bin:$PATH"
    make -C c qwen38-tiny-check
    make -C c qwen38-tiny-fp8-check qwen38-tier-engine-check qwen38-dn-gpu-check
    make -C c qwen38-tiny-int4-check
    make -C c qwen38-tiny-mtp-check
    (
    cd "$root"
    cd c
    make -B qwen38 EXTRA_CFLAGS="-fsanitize=address,undefined -fno-omit-frame-pointer -g"
    source "$root/c/tools/asan-preload.sh"
    ASAN_OPTIONS=detect_leaks=0:detect_stack_use_after_return=0 UBSAN_OPTIONS=print_stacktrace=1 \
      OMP_NUM_THREADS=2 SNAP=./qwen38_tiny \
      ./qwen38 1 8 ./qwen38_tiny/ref.json > qwen38-san.log 2>&1
    ASAN_OPTIONS=detect_leaks=0:detect_stack_use_after_return=0 UBSAN_OPTIONS=print_stacktrace=1 \
      Q38_EXPERT_INT4=1 OMP_NUM_THREADS=2 SNAP=./qwen38_tiny_int4 \
      ./qwen38 1 8 ./qwen38_tiny_int4/ref_int4.json > qwen38-san-int4.log 2>&1
    if grep -qE "ERROR: AddressSanitizer|runtime error:" qwen38-san.log qwen38-san-int4.log; then
      cat qwen38-san.log qwen38-san-int4.log
      exit 1
    fi
    grep -q "Matching tokens: 8/8" qwen38-san.log
    grep -q "Matching tokens: 8/8" qwen38-san-int4.log
    ASAN_OPTIONS=detect_leaks=0:detect_stack_use_after_return=0 UBSAN_OPTIONS=print_stacktrace=1:halt_on_error=1 \
      python3 tests/qwen38_mtp_harness.py --engine ./qwen38 --bf16 ./qwen38_tiny_mtp \
      --fp8 ./qwen38_tiny_fp8_mtp --int4 ./qwen38_tiny_int4_mtp
    )
    (
    cd "$root"
    cd c
    cp -r qwen38_tiny qwen38_tiny_bad
    python3 - <<'PY'
    import json
    path = "qwen38_tiny_bad/config.json"
    config = json.load(open(path, encoding="utf-8"))
    config["hc_count"] = 5
    json.dump(config, open(path, "w", encoding="utf-8"))
    PY
    # Same ASan qwen38 as the previous subshell. No rebuild.
    source "$root/c/tools/asan-preload.sh"
    if SNAP=./qwen38_tiny_bad ./qwen38 1 8 ./qwen38_tiny/ref.json > qwen38-bad.log 2>&1; then
      echo "FAIL: malformed Qwen3.8 container was accepted"
      cat qwen38-bad.log
      exit 1
    fi
    grep -qE "elements, expected|gated residual dimensions" qwen38-bad.log
    )
    (
    cd "$root"
    cd c
    python3 tools/make_edge_tiny_tokenizer.py --vocab-size "$(python3 -c 'import json;print(json.load(open("qwen38_tiny/config.json"))["vocab_size"])')" qwen38_tiny
    make qwen38 >/dev/null
    QWEN38_TINY=qwen38_tiny python3 -m unittest -v tests.test_qwen38_dashboard tests.test_qwen38_brio
    python3 tools/make_edge_tiny_tokenizer.py --vocab-size 64 qwen38_tiny_fp8
    QWEN38_TINY=qwen38_tiny_fp8 python3 -m unittest -v tests.test_qwen38_brio
    )

# ci.yml dsv41-tiny-check. Own venv: oracle requirements plus sympy and pillow.
# Do not install those into the shared oracle venv.
dsv41-tiny-check: (_venv "dsv41" "c/tools/oracle-requirements.txt" "sympy" "pillow")
    #!/usr/bin/env bash
    set -euo pipefail
    root=$(pwd)
    export PATH="$root/target/just-check-all/venvs/dsv41/bin:$PATH"
    (
    cd "$root"
    cd c
    python3 tools/make_dsv41_tiny.py --out dsv41_tiny --emit-ref dsv41_tiny/ref.json
    )
    (
    cd "$root"
    cd c
    make deepseek_v41
    for cap in 1 2 8; do
      echo "::group::cap=$cap"
      SNAP=dsv41_tiny ./deepseek_v41 "$cap" dsv41_tiny/ref.json
      echo "::endgroup::"
    done
    )
    (
    cd "$root"
    cd c
    mkdir -p dsv41_mirror
    cp dsv41_tiny/model.safetensors dsv41_mirror/
    SNAP=dsv41_tiny ./deepseek_v41 8 dsv41_tiny/ref.json > primary.txt 2>/dev/null
    SNAP=dsv41_tiny COLI_MODEL_MIRROR=dsv41_mirror ./deepseek_v41 8 dsv41_tiny/ref.json > mirror.txt 2>/dev/null
    SNAP=dsv41_tiny COLI_MODEL_MIRROR=dsv41_mirror V41_DIRECT=0 ./deepseek_v41 8 dsv41_tiny/ref.json > buffered.txt 2>/dev/null
    cmp primary.txt mirror.txt
    cmp primary.txt buffered.txt
    mkdir -p dsv41_split_meta dsv41_split_shard
    cp dsv41_tiny/config.json dsv41_tiny/tokenizer.json dsv41_tiny/dsv41_engram.json dsv41_split_meta/
    cp dsv41_tiny/model.safetensors dsv41_split_shard/
    SNAP=dsv41_split_meta COLI_MODEL_DIRS=dsv41_split_shard ./deepseek_v41 8 dsv41_tiny/ref.json > split.txt 2>split.err
    cmp primary.txt split.txt
    grep -q "\[SPLIT\] model across 2 dir(s)" split.err
    mkdir -p dsv41_divergent
    python3 - <<'PY'
    import pathlib
    src = pathlib.Path("dsv41_tiny/model.safetensors")
    data = bytearray(src.read_bytes())
    data[8] ^= 0x01
    pathlib.Path("dsv41_divergent/model.safetensors").write_bytes(bytes(data))
    PY
    SNAP=dsv41_tiny COLI_MODEL_MIRROR=dsv41_divergent ./deepseek_v41 8 dsv41_tiny/ref.json > divergent.txt 2>divergent.err
    cmp primary.txt divergent.txt
    grep -q "header differs from the primary copy" divergent.err
    )
    (
    cd "$root"
    cd c
    DSV41_TINY=dsv41_tiny python3 -m unittest -v tests.test_dsv41_prefix_serve
    )
    (
    cd "$root"
    cd c
    python3 tools/make_dsv41_tiny.py --out dsv41_long --emit-ref dsv41_long/ref.json \
      --prompt-len 40 --max-new 6
    for cap in 2 8; do
      echo "::group::long prompt, cap=$cap"
      SNAP=dsv41_long ./deepseek_v41 "$cap" dsv41_long/ref.json
      echo "::endgroup::"
    done
    )
    (
    cd "$root"
    cd c
    if SNAP=dsv41_tiny V41_INDEX_OWNER=1 ./deepseek_v41 8 dsv41_tiny/ref.json > /dev/null 2>&1; then
      echo "FAIL: V41_INDEX_OWNER=1 reproduced the vendor's tokens; one of the two"
      echo "      paths is not doing what its comment says"
      exit 1
    fi
    echo "the two index-key policies differ, as documented"
    )
    (
    cd "$root"
    cd c
    for force in 1 2 3 4 5; do
      for cap in 1 8; do
        echo "::group::V41_SPEC_FORCE=$force cap=$cap"
        SNAP=dsv41_tiny V41_DSPARK=1 V41_SPEC_FORCE=$force ./deepseek_v41 "$cap" dsv41_tiny/ref.json
        echo "::endgroup::"
      done
    done
    SNAP=dsv41_tiny V41_DSPARK=1 V41_SPEC_FORCE=2 V41_DSPARK_MAX=1 ./deepseek_v41 8 dsv41_tiny/ref.json
    )
    (
    cd "$root"
    cd c
    COLI_DSV41_FIXTURE=dsv41_tiny python3 -m unittest -v tests.test_dsv41_dashboard
    )
    (
    cd "$root"
    cd c
    COLI_DSV41_FIXTURE=dsv41_tiny python3 -m unittest -v \
      tests.test_dsv41_image_serve tests.test_dsv41_dspark_serve
    )
    (
    cd "$root"
    cd c
    python3 -m unittest -v tests.test_openai_tools_v41_e2e
    )

# ci.yml qwen36-tiny-check. Shared oracle venv. `make clean` is required.
# Each block is one CI step and starts at the repo root.
qwen36-tiny-check: (_venv "oracle" "c/tools/oracle-requirements.txt")
    #!/usr/bin/env bash
    set -euo pipefail
    root=$(pwd)
    export PATH="$root/target/just-check-all/venvs/oracle/bin:$PATH"
    (
    cd "$root"
    cd c
    python3 tools/make_qwen36_tiny.py --out qwen36_tiny --ref-mode full \
            --emit-ref qwen36_tiny/ref_full.json
    python3 tools/convert_qwen36.py --model qwen36_tiny --out qwen36_tiny_c --ebits 8
    )
    (
    cd "$root"
    cd c
    make qwen36
    for cap in 1 2 8; do
      echo "::group::cap=$cap"
      COLI_DENSE_I8=0 SNAP=qwen36_tiny_c \
        ./qwen36 "$cap" 8 qwen36_tiny/ref_full.json
      echo "::endgroup::"
    done
    )
    (
    cd "$root"
    cd c
    python3 tools/make_qwen36_tiny.py --geometry qwen38-2p4t --seed 3 \
            --out qwen38_2p4t_tiny --ref-mode full \
            --emit-ref qwen38_2p4t_tiny/ref_full.json
    python3 tools/convert_qwen36.py --model qwen38_2p4t_tiny \
            --out qwen38_2p4t_tiny_c --ebits 8 | tee convert_2p4t.log
    grep -q "skipping 19 \`mtp.\*\` tensor" convert_2p4t.log
    grep -q "tensor kinds: 1587 in 92 layers, 3 globals" convert_2p4t.log
    python3 - <<'EOF'
    import glob
    from safetensors import safe_open
    for path in glob.glob("qwen38_2p4t_tiny_c/*.safetensors"):
        with safe_open(path, "pt") as f:
            assert not [k for k in f.keys() if k.startswith("mtp.")], path
    EOF
    for cap in 1 2 512; do
      echo "::group::2.4T geometry cap=$cap"
      COLI_DENSE_I8=0 SNAP=qwen38_2p4t_tiny_c \
        ./qwen36 "$cap" 8 qwen38_2p4t_tiny/ref_full.json
      echo "::endgroup::"
    done
    )
    (
    cd "$root"
    cd c
    python3 tools/make_qwen36_tiny.py --geometry qwen3-coder-30b \
            --out qwen3_coder_tiny --ref-mode full \
            --emit-ref qwen3_coder_tiny/ref_full.json
    python3 tools/convert_qwen36.py --model qwen3_coder_tiny \
            --out qwen3_coder_tiny_c --ebits 8
    python3 -c "import json; m = json.load(open('qwen3_coder_tiny_c/qwen36_meta.json')); \
                assert (m['shared_inter'], m['zero_centered_norms'], m['partial_rotary_factor'], \
                        set(m['layer_types'])) == (0, False, 1.0, {'full_attention'}), m"
    for cap in 1 8; do
      echo "::group::Qwen3-Coder geometry cap=$cap"
      COLI_DENSE_I8=0 SNAP=qwen3_coder_tiny_c \
        ./qwen36 "$cap" 8 qwen3_coder_tiny/ref_full.json
      echo "::endgroup::"
    done
    )
    (
    cd "$root"
    cd c
    python3 tools/make_qwen36_tiny.py --geometry qwen38-27b-dense \
            --out qwen38_27b_tiny --ref-mode full \
            --emit-ref qwen38_27b_tiny/ref_full.json
    python3 tools/convert_qwen36.py --model qwen38_27b_tiny \
            --out qwen38_27b_tiny_c --ebits 8
    python3 -c "import json; m = json.load(open('qwen38_27b_tiny_c/qwen36_meta.json')); \
                assert (m['num_experts'], m['topk'], m['shared_inter']) == (0, 0, 128), m"
    for cap in 1 8; do
      echo "::group::27B dense geometry cap=$cap"
      COLI_DENSE_I8=0 SNAP=qwen38_27b_tiny_c \
        ./qwen36 "$cap" 8 qwen38_27b_tiny/ref_full.json
      echo "::endgroup::"
    done
    python3 tools/make_qwen36_vl_tiny.py --out qwen38_27b_vl_tiny
    python3 tools/convert_qwen36.py --model qwen38_27b_vl_tiny \
            --out qwen38_27b_vl_tiny_c --ebits 8 | tee convert_vl.log
    grep -q "\[vision\] model-vision.safetensors" convert_vl.log
    for cap in 1 8; do
      echo "::group::27B vision cap=$cap"
      COLI_DENSE_I8=0 SNAP=qwen38_27b_vl_tiny_c \
        ./qwen36 "$cap" 8 qwen38_27b_vl_tiny/ref.json
      echo "::endgroup::"
    done
    QWEN36_VL_TINY=qwen38_27b_vl_tiny_c QWEN36_VL_REF=qwen38_27b_vl_tiny/ref.json \
      python3 -m unittest -v tests.test_qwen36_vision_serve
    )
    (
    cd "$root"
    cd c
    make clean >/dev/null 2>&1 || true
    make qwen36 EXTRA_CFLAGS="-fsanitize=address,undefined -fno-omit-frame-pointer -g"
    source "$root/c/tools/asan-preload.sh"
    for fx in qwen36_tiny qwen38_2p4t_tiny; do
      ASAN_OPTIONS=detect_leaks=0:detect_stack_use_after_return=0 UBSAN_OPTIONS=print_stacktrace=1 \
        COLI_DENSE_I8=0 SNAP=${fx}_c PILOT=1 WIDE=2 \
        ./qwen36 1 8 $fx/ref_full.json > san.log 2>&1 || true
      if grep -qE "ERROR: AddressSanitizer|runtime error:" san.log; then
        echo "FAIL: sanitizer diagnostic under PILOT ($fx)"; cat san.log; exit 1
      fi
      echo "sanitizers clean ($fx):"; grep -E "Matching tokens" san.log || true
    done
    ASAN_OPTIONS=detect_leaks=0:detect_stack_use_after_return=0 QWEN36_TINY_C=qwen36_tiny_c \
      QWEN36_TINY_REF=qwen36_tiny/ref_full.json \
      python3 -m unittest -v tests.test_qwen36_hostile_weights
    )
    (
    cd "$root"
    cd c
    make clean >/dev/null 2>&1 || true
    make qwen36
    QWEN36_TINY=qwen36_tiny_c python3 -m unittest -v tests.test_qwen36_prefix_serve
    )
    (
    cd "$root"
    cd c
    make clean >/dev/null 2>&1 || true
    make qwen36
    python3 tools/make_qwen36_tiny.py --out qwen36_tiny64 --ref-mode full --inter 64 \
            --emit-ref qwen36_tiny64/ref_full.json
    python3 tools/convert_qwen36.py --model qwen36_tiny64 --out qwen36_tiny64_c --ebits 4 --gs 64
    for cap in 1 2 8 16; do
      for k in 0 1; do
        QWEN_EXPERT_KERNEL=$k COLI_DENSE_I8=0 SNAP=qwen36_tiny64_c DUMP=logits_${cap}_$k.f32 \
          ./qwen36 "$cap" 4 qwen36_tiny64/ref_full.json > ab_${cap}_$k.log 2>&1 || true
        grep -E "^C engine" ab_${cap}_$k.log > ids_${cap}_$k.txt
      done
      grep -q "expert kernel: planar int4" ab_${cap}_1.log || { echo "FAIL: kernel did not engage at cap=$cap"; cat ab_${cap}_1.log; exit 1; }
      grep -q "expert kernel: planar int4" ab_${cap}_0.log && { echo "FAIL: QWEN_EXPERT_KERNEL=0 still engaged the kernel"; exit 1; }
      test -s ids_${cap}_1.txt || { echo "FAIL: no ids at cap=$cap"; cat ab_${cap}_1.log; exit 1; }
      cmp ids_${cap}_0.txt ids_${cap}_1.txt || { echo "FAIL: ids differ at cap=$cap"; cat ids_${cap}_0.txt ids_${cap}_1.txt; exit 1; }
      echo "cap=$cap: identical ($(cat ids_${cap}_1.txt))"
    done
    for k in 0 1; do
      for cap in 1 2 8; do
        test -s logits_${cap}_$k.f32 || { echo "FAIL: no logits dump at cap=$cap kernel=$k"; exit 1; }
        cmp logits_${cap}_$k.f32 logits_16_$k.f32 || { echo "FAIL: kernel=$k logits at cap=$cap differ from cap=16"; exit 1; }
      done
      echo "kernel=$k: identical logits at cap 1/2/8/16"
    done
    make tests/test_expert_ffn && ./tests/test_expert_ffn
    )
    (
    cd "$root"
    cd c
    python3 tools/convert_qwen36.py --model qwen36_tiny64 --out qwen36_tiny64_d8 --ebits 4 --gs 64 --down-bits 8
    python3 - <<'PY'
    import json; m = json.load(open("qwen36_tiny64_d8/qwen36_meta.json"))
    assert m["expert_down_bits"] == 8 and m["expert_down_gs"] == 0 and m["expert_gs"] == 64, m
    PY
    for cap in 1 2 8; do
      COLI_DENSE_I8=0 SNAP=qwen36_tiny64_d8 ./qwen36 "$cap" 4 qwen36_tiny64/ref_full.json > mixed_$cap.log 2>&1 || true
      grep -q "expert format on disk: int4 gate/up + int8 down" mixed_$cap.log || { echo "FAIL: mixed layout not detected at cap=$cap"; cat mixed_$cap.log; exit 1; }
      grep -E "^C engine" mixed_$cap.log > mixed_ids_$cap.txt
      test -s mixed_ids_$cap.txt || { echo "FAIL: no ids at cap=$cap"; cat mixed_$cap.log; exit 1; }
    done
    cmp mixed_ids_1.txt mixed_ids_8.txt && cmp mixed_ids_2.txt mixed_ids_8.txt || { echo "FAIL: ids differ across caps"; cat mixed_ids_*.txt; exit 1; }
    echo "mixed layout: identical ids at cap 1/2/8 ($(cat mixed_ids_8.txt))"
    COLI_CUDA=1 COLI_DENSE_I8=0 SNAP=qwen36_tiny64_d8 ./qwen36 8 4 qwen36_tiny64/ref_full.json > mixed_cuda.log 2>&1 || true
    grep -q "COLI_CUDA=1 ignored: the VRAM expert tier does not take the mixed layout" mixed_cuda.log || { echo "FAIL: tier refusal line missing"; cat mixed_cuda.log; exit 1; }
    )
    (
    cd "$root"
    cd c
    B=./qwen36
    run() {
      local tag=$1 k=$2 cap=$3; shift 3
      env "$@" QWEN_EXPERT_KERNEL=$k COLI_DENSE_I8=0 SNAP=qwen36_tiny64_c DUMP=pin_$tag.f32 \
        $B "$cap" 4 qwen36_tiny64/ref_full.json > pin_$tag.log 2>&1 || true
      test -s pin_$tag.f32 || { echo "FAIL: no logits dump ($tag)"; cat pin_$tag.log; exit 1; }
    }
    fail=0
    for k in 0 1; do
      run ref_$k $k 16
      for cap in 2 3 4; do
        for hot in 1 2 3; do
          run k${k}_cap${cap}_hot$hot $k $cap HOT=$hot
          grep -q "^\[HOT\] Pinned [1-9]" pin_k${k}_cap${cap}_hot$hot.log || { echo "FAIL: HOT pinned nothing"; exit 1; }
        done
        for r in 1 2; do
          run k${k}_cap${cap}_res$r $k $cap COLIBRI_RESIDENT=$r
          grep -q "^\[RESIDENT\] Pinned [1-9]" pin_k${k}_cap${cap}_res$r.log || { echo "FAIL: RESIDENT pinned nothing"; exit 1; }
        done
      done
      for i in $(seq 10); do
        run k${k}_pilot1_$i $k 2 PILOT=1
        run k${k}_pilot3_$i $k 2 PILOT=3 WIDE=4
      done
      n=0 bad=0
      for f in pin_k${k}_*.f32; do
        n=$((n+1)); cmp -s "$f" pin_ref_$k.f32 || { echo "kernel=$k: ${f#pin_k${k}_} differs from cap=16"; bad=$((bad+1)); }
      done
      echo "kernel=$k: $((n-bad))/$n runs give the cap=16 logits"
      [ $bad -eq 0 ] || fail=1
    done
    exit $fail
    )
    (
    cd "$root"
    cd c
    cp -r qwen36_tiny_c qwen36_tiny_bad
    python3 - <<'PY'
    import json
    p = "qwen36_tiny_bad/config.json"
    cfg = json.load(open(p))
    cfg["num_hidden_layers"] = 4
    json.dump(cfg, open(p, "w"))
    PY
    if COLI_DENSE_I8=0 SNAP=qwen36_tiny_bad ./qwen36 8 8 \
         qwen36_tiny/ref_full.json > bad.log 2>&1; then
      echo "FAIL: engine accepted a container whose two config files disagree"
      cat bad.log; exit 1
    fi
    grep -q "config.json says 4 layers" bad.log || {
      echo "FAIL: refused, but not for the reason under test"; cat bad.log; exit 1; }
    echo "refused as expected:"; grep '^\[cfg\]' bad.log
    )
    (
    cd "$root"
    cd c
    python3 tools/make_edge_tiny_tokenizer.py --vocab-size 320 qwen36_tiny_c
    make qwen36 >/dev/null
    COLI_QWEN36_FIXTURE=qwen36_tiny_c python3 -m unittest -v tests.test_qwen36_dashboard
    )
    (
    cd "$root"
    cd c
    SNAP=qwen36_tiny_c ./qwen36 1 8 qwen36_tiny/ref_full.json 2> probe8.log >/dev/null || true
    grep -q 'expert format on disk: int8' probe8.log || { echo "FAIL: int8 container probed as"; grep 'expert format' probe8.log; exit 1; }
    python3 tools/convert_qwen36.py --model qwen36_tiny --out qwen36_tiny_c4 --ebits 4 >/dev/null
    SNAP=qwen36_tiny_c4 ./qwen36 1 4 qwen36_tiny/ref_full.json 2> probe4.log >/dev/null || true
    grep -q 'expert format on disk: int4' probe4.log || { echo "FAIL: int4 container probed as"; grep 'expert format' probe4.log; exit 1; }
    echo "probe: int8 -> fmt=1, int4 -> fmt=4"
    )

# ci.yml vulkan. Skip apt when the headers, glslc, and Lavapipe ICD are already
# installed. /tmp/vkprobe stays; it is a tiny config.
vulkan:
    #!/usr/bin/env bash
    set -euo pipefail
    root=$(pwd)
    if [[ -f /usr/include/vulkan/vulkan.h ]] && command -v glslc >/dev/null 2>&1 && [[ -f /usr/share/vulkan/icd.d/lvp_icd.json ]]; then
      echo "Vulkan headers, glslc, and Lavapipe ICD already present"
    else
      sudo -n apt-get update || { echo "Vulkan toolchain is missing and sudo -n apt-get update failed"; exit 1; }
      sudo -n apt-get install -y --no-install-recommends libvulkan-dev glslc mesa-vulkan-drivers \
        || { echo "Vulkan toolchain is missing and sudo -n apt-get install failed"; exit 1; }
    fi
    (
    cd "$root"
    cd c
    make colibri VK=1
    ls -l shaders/qmatmul.spv shaders/qmatmul_gate_up.spv \
          shaders/attention_absorb.spv shaders/rmsnorm.spv
    if ldd colibri | grep -q libvulkan; then echo "FAIL: colibri links libvulkan"; ldd colibri; exit 1; fi
    grep -q libvulkan.so.1 colibri || { echo "FAIL: built without the Vulkan backend"; exit 1; }
    )
    (
    cd "$root"
    cd c
    mkdir -p /tmp/vkprobe
    echo '{"model_type":"glm_moe_dsa"}' > /tmp/vkprobe/config.json
    VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \
    COLI_NO_OMP_TUNE=1 COLI_VULKAN=1 SNAP=/tmp/vkprobe \
      timeout 120 ./colibri > vk.log 2>&1 || true
    cat vk.log
    grep -q "\[VK\] ready:" vk.log || {
      echo "FAIL: the Vulkan backend did not initialise"; exit 1; }
    grep -q "expert tier active" vk.log || {
      echo "FAIL: Vulkan initialised but the expert tier did not activate"; exit 1; }
    echo "OK: shaders compiled, device negotiated, expert tier active"
    COLI_VK_LOADER=/nonexistent/libvulkan.so.1 COLI_NO_OMP_TUNE=1 COLI_VULKAN=1 SNAP=/tmp/vkprobe \
      timeout 120 ./colibri > vk0.log 2>&1 || true
    cat vk0.log
    grep -q "\[VK\] no Vulkan loader (/nonexistent/libvulkan.so.1)" vk0.log || {
      echo "FAIL: no line for a missing Vulkan loader"; exit 1; }
    )
    (
    cd "$root"
    cd c
    bash tests/vulkan_engines.sh shader
    )
    (
    cd "$root"
    cd c
    bash tests/vulkan_engines.sh staged
    COLI_VK_STAGED=1 bash tests/vulkan_engines.sh shader
    )

# ci.yml vulkan-engines, ubuntu matrix order. npm ci only before family decide.
# Same requirements set shares one venv.
vulkan-engines: \
    (_venv "qwen38-tiny" "c/tools/requirements-qwen38-tiny.txt") \
    (_venv "qwen38-pillow" "c/tools/requirements-qwen38-tiny.txt" "pillow") \
    (_venv "qwen38-numpy-pillow" "c/tools/requirements-qwen38-tiny.txt" "numpy" "pillow") \
    (_venv "oracle" "c/tools/oracle-requirements.txt") \
    (_venv "qwenimage-tiny" "c/tools/requirements-qwenimage-tiny.txt") \
    (_venv "deepseek-sympy-pillow" "c/tools/requirements-deepseek-v4-tiny.txt" "sympy" "pillow") \
    (_venv "kimi-k3-tiny" "c/tools/requirements-kimi-k3-tiny.txt") \
    (_venv "mimo-qwenimage" "c/tools/requirements-mimo-tiny.txt" "c/tools/requirements-qwenimage-tiny.txt") \
    (_venv "mimo-tiny" "c/tools/requirements-mimo-tiny.txt") \
    (_venv "glm53-tiny" "c/tools/requirements-glm53-tiny.txt") \
    (_venv "glm53-pillow" "c/tools/requirements-glm53-tiny.txt" "pillow") \
    (_venv "kimi-oracle-pillow" "c/tools/requirements-kimi-k3-tiny.txt" "c/tools/oracle-requirements.txt" "pillow") \
    (_venv "jev-oracle" "c/tools/requirements-jev-sdk.txt" "c/tools/oracle-requirements.txt")
    #!/usr/bin/env bash
    set -euo pipefail
    root=$(pwd)
    venvs="$root/target/just-check-all/venvs"
    orig_path="$PATH"
    families=(
      qwen qwen-sanitize qwen-chain qwen-chain-sanitize
      qwen-spec qwen-spec-sanitize qwen-staged qwen-chain-staged
      staged-faults staged-faults-sanitize
      inkling-olmoe inkling-olmoe-sanitize inkling-olmoe-chain inkling-olmoe-chain-sanitize
      mimo-qwenimage
      deepseek deepseek-sanitize
      kimi kimi-mimo-sanitize
      mimo-chain mimo-chain-sanitize
      glm glm-sanitize glm-chain glm-chain-sanitize
      deepseek-chain deepseek-chain-sanitize
      kimi-chain kimi-chain-sanitize
      dense-only-qwen38 dense-only-qwen38-staged dense-only-qwen38-sanitize
      partial-qwen38 partial-qwen38-sanitize
      dense-only-qwen36-olmoe dense-only-qwen36-olmoe-sanitize
      partial-qwen36-olmoe partial-qwen36-olmoe-sanitize
      dense-only-inkling-mimo-kimi dense-only-inkling-mimo-kimi-sanitize
      dense-only-glm dense-only-glm-sanitize
      partial-glm partial-glm-sanitize
      dense-only-deepseek dense-only-deepseek-sanitize
      partial-deepseek partial-deepseek-sanitize
      dev2 dev2-deepseek-kimi-mimo dev2-sanitize
      partial-dsk partial-dsk-sanitize
      partial-inkling-mimo partial-inkling-mimo-sanitize
      prefill-qwen prefill-qwen-sanitize prefill-inkling-olmoe prefill-mimo-kimi prefill-glm prefill-deepseek
      kv-split kv-split-sanitize kv-split-cold kv-split-cold-sanitize
      kv-split-deepseek kv-split-deepseek-sanitize
      layers-dev2 layers-dev2-sanitize layers-dev2-mla layers-dev2-mla-sanitize
      layers-dev2-deepseek layers-dev2-deepseek-sanitize
      decide decide-sanitize
      mux mux-sanitize mux-deepseek mux-deepseek-sanitize
    )
    for family in "${families[@]}"; do
      case "$family" in
        qwen|qwen-sanitize|qwen-chain|qwen-chain-sanitize|qwen-spec|qwen-spec-sanitize|qwen-staged|qwen-chain-staged|staged-faults|staged-faults-sanitize|dense-only-qwen38|dense-only-qwen38-staged|dense-only-qwen38-sanitize|partial-qwen38|partial-qwen38-sanitize|dense-only-qwen36-olmoe|dense-only-qwen36-olmoe-sanitize|partial-qwen36-olmoe|partial-qwen36-olmoe-sanitize|prefill-qwen|prefill-qwen-sanitize)
          venv=qwen38-tiny ;;
        dense-only-glm|dense-only-glm-sanitize)
          venv=qwen38-pillow ;;
        kv-split|kv-split-sanitize|kv-split-cold|kv-split-cold-sanitize|layers-dev2|layers-dev2-sanitize|layers-dev2-mla|layers-dev2-mla-sanitize|mux|mux-sanitize)
          venv=qwen38-numpy-pillow ;;
        inkling-olmoe|inkling-olmoe-sanitize|inkling-olmoe-chain|inkling-olmoe-chain-sanitize|prefill-inkling-olmoe)
          venv=oracle ;;
        mimo-qwenimage)
          venv=qwenimage-tiny ;;
        deepseek|deepseek-sanitize|deepseek-chain|deepseek-chain-sanitize|dense-only-deepseek|dense-only-deepseek-sanitize|partial-deepseek|partial-deepseek-sanitize|dev2-deepseek-kimi-mimo|dev2-sanitize|partial-dsk|partial-dsk-sanitize|prefill-deepseek|kv-split-deepseek|kv-split-deepseek-sanitize|layers-dev2-deepseek|layers-dev2-deepseek-sanitize|mux-deepseek|mux-deepseek-sanitize)
          venv=deepseek-sympy-pillow ;;
        kimi|kimi-chain|kimi-chain-sanitize)
          venv=kimi-k3-tiny ;;
        kimi-mimo-sanitize)
          venv=mimo-qwenimage ;;
        mimo-chain|mimo-chain-sanitize|prefill-mimo-kimi)
          venv=mimo-tiny ;;
        glm|glm-sanitize|glm-chain|glm-chain-sanitize|dev2|prefill-glm)
          venv=glm53-tiny ;;
        partial-glm|partial-glm-sanitize)
          venv=glm53-pillow ;;
        dense-only-inkling-mimo-kimi|dense-only-inkling-mimo-kimi-sanitize|partial-inkling-mimo|partial-inkling-mimo-sanitize)
          venv=kimi-oracle-pillow ;;
        decide|decide-sanitize)
          venv=jev-oracle ;;
        *)
          echo "no venv for vulkan family $family" >&2
          exit 1 ;;
      esac
      if [[ "$family" == "decide" ]]; then
        (cd "$root/c/tools/jev-sdk" && npm ci)
      fi
      export PATH="$venvs/$venv/bin:$orig_path"
      echo "==> vulkan-engines $family ($venv)"
      bash c/tests/vulkan_engines.sh "$family"
    done

# ci.yml engine-hip-syntax. The ROCm docker compile is the gate when the
# Docker API accepts this user. A missing or permission-denied API skips.
# A compile or relocation failure still fails the gate when `docker info` works.
# The CI sudo rm of /usr/share/dotnet and other runner directories is not run.
engine-hip-syntax:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! docker info >/dev/null 2>&1; then
      echo "warning: docker API is not usable; skipping HIP syntax check" >&2
      exit 0
    fi
    docker run --rm -v "$PWD":/src -w /src rocm/dev-ubuntu-24.04:6.2 bash -c '
      apt-get update && apt-get install -y --no-install-recommends make gcc g++ rocwmma-dev binutils &&
      make -C c gpu-compile HIP=1 HIP_ARCH=gfx1100 &&
      if readelf -r c/backend_cuda.o | grep -qE "R_X86_64_32S?"; then
        echo "backend_cuda.o has absolute 32-bit relocations: the PIE link of colibri will fail (HIPCCFLAGS needs -fPIE or -fPIC)" >&2
        exit 1
      fi'
    echo "HIP syntax check passed"

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
# Missing nvcc does not fail the gate. If nvcc exists, a compile error does.
engine-cuda-syntax:
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

# Same gate as engine-cuda-syntax.
cuda-syntax: engine-cuda-syntax

# check already builds the engines and runs the CUDA syntax compile.
check-extra: check
    @echo "just check-extra: extended gates passed."

# Contract for `just check`: one unittest, no engine build.
check-contract:
    #!/usr/bin/env bash
    set -euo pipefail
    python3 -m unittest discover -s c/tests -p test_just_check_covers_ci.py -v
