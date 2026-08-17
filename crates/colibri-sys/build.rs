//! Build script for optional multi-family static FFI (`feature = "ffi"`).
//!
//! Links:
//! - `libdeepseek_v4.a` (`c/Makefile.deepseek-v4` target `libdeepseek-v4`)
//! - `libcolibri.a` (GLM; `make libcolibri` / `COLIBRI_NO_MAIN`)
//! - `libkimi_k3.a` (`make libkimi_k3` / `KIMI_NO_MAIN`)
//! - `libinkling.a` (`make libinkling` / `INKLING_NO_MAIN`, CPU only)
//!
//! ## GPU (opt-in, Linux only; one vendor per binary)
//!
//! Default `feature = "ffi"` is **CPU-only**. Opt-in GLM GPU embed:
//! - Cargo feature `ffi-cuda` (implies `ffi`), and/or env `COLIBRI_FFI_CUDA=1`
//! - Cargo feature `ffi-hip` (implies `ffi`), and/or env `COLIBRI_FFI_HIP=1`
//!
//! **Mutual exclusion:** CUDA and HIP cannot both be linked into one binary.
//! If both are requested (features and/or env), `build.rs` panics with a clear
//! error. Pick one vendor for the host build.
//!
//! ### CUDA (`ffi-cuda`)
//! When the CUDA toolkit (`nvcc`) is present, `build.rs` runs
//! `make libcolibri CUDA=1` (packs `backend_cuda.o`) and links `cudart` +
//! `stdc++`, then sets rustc cfg `ffi_cuda_linked`.
//!
//! Without a toolkit, the build **falls back to CPU-only** GLM (cargo warning)
//! unless `COLIBRI_REQUIRE_FFI_CUDA=1` is set (hard fail). Default CI stays green.
//!
//! ### HIP (`ffi-hip`)
//! When ROCm (`hipcc` + `libamdhip64`) is present, `build.rs` runs
//! `make libcolibri HIP=1` and links `amdhip64` + `stdc++` with rpath, then
//! sets rustc cfg `ffi_hip_linked`.
//!
//! Env: `ROCM_HOME` / `ROCM_PATH` (default `/opt/rocm`), `HIPCC`, `HIP_ARCH`
//! (default Makefile `native` or pass explicit `gfxXXXX`).
//! Without a toolkit, CPU fallback unless `COLIBRI_REQUIRE_FFI_HIP=1`.
//!
//! Prebuilt overrides (optional):
//! - `COLIBRI_V4_STATIC_LIB`, `COLIBRI_GLM_STATIC_LIB`, `COLIBRI_KIMI_STATIC_LIB`,
//!   `COLIBRI_INKLING_STATIC_LIB`
//!
//! ## CPU-only `ffi` vs leftover GPU archives
//!
//! Default `feature = "ffi"` must not link a `libcolibri.a` that still contains
//! HIP or CUDA objects (for example after `ffi-hip` clippy packed
//! `backend_cuda.o`). `build.rs` scans the archive (or `COLIBRI_GLM_STATIC_LIB`)
//! for `__hipUnregisterFatBinary` / `__cudaUnregisterFatBinary`. In-tree leftover
//! archives are deleted and rebuilt with `make libcolibri HIP=0 CUDA=0`. A
//! prebuilt override that is still GPU-flavored is a hard error (do not paper
//! over by linking `amdhip64` / `cudart` on CPU-only ffi).

#[path = "src/archive_gpu_flavor.rs"]
mod archive_gpu_flavor;

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use archive_gpu_flavor::{GpuArchiveFlavor, archive_gpu_flavor};

fn main() {
    // Always advertise optional cfgs so `#[cfg(ffi_*_linked)]` does not warn.
    println!("cargo:rustc-check-cfg=cfg(ffi_cuda_linked)");
    println!("cargo:rustc-check-cfg=cfg(ffi_hip_linked)");
    println!("cargo:rerun-if-env-changed=COLIBRI_FFI_CUDA");
    println!("cargo:rerun-if-env-changed=COLIBRI_REQUIRE_FFI_CUDA");
    println!("cargo:rerun-if-env-changed=COLIBRI_FFI_HIP");
    println!("cargo:rerun-if-env-changed=COLIBRI_REQUIRE_FFI_HIP");
    println!("cargo:rerun-if-env-changed=CUDA_HOME");
    println!("cargo:rerun-if-env-changed=CUDA_PATH");
    println!("cargo:rerun-if-env-changed=NVCC");
    println!("cargo:rerun-if-env-changed=ROCM_HOME");
    println!("cargo:rerun-if-env-changed=ROCM_PATH");
    println!("cargo:rerun-if-env-changed=HIPCC");
    println!("cargo:rerun-if-env-changed=HIP_ARCH");
    println!("cargo:rerun-if-env-changed=COLI_HIP_NO_WMMA");
    println!("cargo:rerun-if-env-changed=COLIBRI_V4_STATIC_LIB");
    println!("cargo:rerun-if-env-changed=COLIBRI_GLM_STATIC_LIB");
    println!("cargo:rerun-if-env-changed=COLIBRI_KIMI_STATIC_LIB");
    println!("cargo:rerun-if-env-changed=COLIBRI_INKLING_STATIC_LIB");
    println!("cargo:rerun-if-env-changed=COLIBRI_V4_LINK_STDCXX");

    if env::var_os("CARGO_FEATURE_FFI").is_none() {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let c_dir = manifest_dir
        .join("..")
        .join("..")
        .join("c")
        .canonicalize()
        .unwrap_or_else(|_| manifest_dir.join("../../c"));

    for rel in [
        "Makefile",
        "Makefile.deepseek-v4",
        "Makefile.deepseek-v4.units",
        "deepseek_v4.c",
        "deepseek_v4.h",
        "colibri.c",
        "kimi_k3.c",
        "inkling.c",
        "colibri_api.h",
        "backend_cuda.cu",
        "backend_cuda.h",
        "backend_gpu_compat.h",
    ] {
        println!("cargo:rerun-if-changed={}", c_dir.join(rel).display());
    }

    let jobs = env::var("NUM_JOBS")
        .ok()
        .or_else(|| {
            env::var("CARGO_MAKEFLAGS")
                .ok()
                .and_then(parse_jobs_from_makeflags)
        })
        .unwrap_or_else(|| "4".into());

    let want_cuda = want_ffi_cuda();
    let want_hip = want_ffi_hip();
    if want_cuda && want_hip {
        panic!(
            "ffi-cuda and ffi-hip are mutually exclusive: one GPU vendor link \
             mode per binary. Enable only one of feature `ffi-cuda` / \
             COLIBRI_FFI_CUDA or feature `ffi-hip` / COLIBRI_FFI_HIP."
        );
    }

    let cuda_plan = if want_cuda { resolve_cuda_plan() } else { None };
    let hip_plan = if want_hip { resolve_hip_plan() } else { None };

    // V4
    if let Ok(pre) = env::var("COLIBRI_V4_STATIC_LIB") {
        link_prebuilt(&pre, "deepseek_v4");
    } else {
        run_make(
            &c_dir,
            &[
                "-f",
                "Makefile.deepseek-v4",
                "libdeepseek-v4",
                "LTO=0",
                &format!("-j{jobs}"),
            ],
        );
        expect_lib(&c_dir, "libdeepseek_v4.a");
        println!("cargo:rustc-link-search=native={}", c_dir.display());
        println!("cargo:rustc-link-lib=static=deepseek_v4");
    }

    // GLM (CPU default; optional Linux CUDA or HIP when toolkit present)
    if let Ok(pre) = env::var("COLIBRI_GLM_STATIC_LIB") {
        let pre_path = PathBuf::from(&pre);
        if !want_cuda && !want_hip {
            refuse_gpu_archive_for_cpu_ffi(&pre_path, "COLIBRI_GLM_STATIC_LIB");
        }
        link_prebuilt(&pre, "colibri");
        if want_cuda {
            // Prebuilt path: host must supply a CUDA-capable archive if they want
            // ffi_cuda_linked. We only set the cfg when the toolkit is also
            // present so -lcudart can be linked.
            if let Some(ref plan) = cuda_plan {
                link_cuda_runtime(plan);
                println!("cargo:rustc-cfg=ffi_cuda_linked");
            } else if require_ffi_cuda() {
                panic!(
                    "COLIBRI_GLM_STATIC_LIB set with ffi-cuda/COLIBRI_FFI_CUDA, \
                     but nvcc/CUDA toolkit not found (COLIBRI_REQUIRE_FFI_CUDA=1)"
                );
            } else {
                println!(
                    "cargo:warning=COLIBRI_GLM_STATIC_LIB + ffi-cuda: CUDA toolkit not \
                     found; linking prebuilt without -lcudart (set COLIBRI_REQUIRE_FFI_CUDA=1 \
                     to fail hard)"
                );
            }
        } else if want_hip {
            if let Some(ref plan) = hip_plan {
                link_hip_runtime(plan);
                println!("cargo:rustc-cfg=ffi_hip_linked");
            } else if require_ffi_hip() {
                panic!(
                    "COLIBRI_GLM_STATIC_LIB set with ffi-hip/COLIBRI_FFI_HIP, \
                     but hipcc/ROCm not found (COLIBRI_REQUIRE_FFI_HIP=1)"
                );
            } else {
                println!(
                    "cargo:warning=COLIBRI_GLM_STATIC_LIB + ffi-hip: ROCm/hipcc not \
                     found; linking prebuilt without -lamdhip64 (set COLIBRI_REQUIRE_FFI_HIP=1 \
                     to fail hard)"
                );
            }
        }
    } else if let Some(ref plan) = cuda_plan {
        // Opt-in CUDA embed: pack backend_cuda.o into libcolibri.a.
        run_make(
            &c_dir,
            &[
                "libcolibri",
                "CUDA=1",
                "LTO=0",
                &format!("CUDA_HOME={}", plan.cuda_home.display()),
                &format!("-j{jobs}"),
            ],
        );
        expect_lib(&c_dir, "libcolibri.a");
        println!("cargo:rustc-link-search=native={}", c_dir.display());
        println!("cargo:rustc-link-lib=static=colibri");
        link_cuda_runtime(plan);
        println!("cargo:rustc-cfg=ffi_cuda_linked");
    } else if let Some(ref plan) = hip_plan {
        // Opt-in HIP embed: pack backend_cuda.o (hipcc) into libcolibri.a.
        let mut args: Vec<String> = vec![
            "libcolibri".into(),
            "HIP=1".into(),
            "LTO=0".into(),
            format!("ROCM_HOME={}", plan.rocm_home.display()),
            format!("-j{jobs}"),
        ];
        if let Some(ref arch) = plan.hip_arch {
            args.push(format!("HIP_ARCH={arch}"));
        }
        if let Some(ref hipcc) = plan.hipcc {
            args.push(format!("HIPCC={}", hipcc.display()));
        }
        if plan.force_no_wmma {
            args.push("COLI_HIP_NO_WMMA=1".into());
            println!(
                "cargo:warning=ffi-hip: rocWMMA headers not found under {}; \
                 building portable HIP kernels (COLI_HIP_NO_WMMA=1). Install \
                 rocwmma-dev for tensor-core paths, or set HIP_ARCH to a \
                 non-WMMA arch listed in c/Makefile NO_WMMA_ARCHS.",
                plan.rocm_home.display()
            );
        }
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        run_make(&c_dir, &arg_refs);
        expect_lib(&c_dir, "libcolibri.a");
        println!("cargo:rustc-link-search=native={}", c_dir.display());
        println!("cargo:rustc-link-lib=static=colibri");
        link_hip_runtime(plan);
        println!("cargo:rustc-cfg=ffi_hip_linked");
    } else {
        if want_cuda {
            if require_ffi_cuda() {
                panic!(
                    "feature ffi-cuda / COLIBRI_FFI_CUDA requested but CUDA toolkit \
                     (nvcc) not found. Install CUDA or unset the flag. \
                     COLIBRI_REQUIRE_FFI_CUDA=1 forbids CPU fallback."
                );
            }
            println!(
                "cargo:warning=feature ffi-cuda / COLIBRI_FFI_CUDA requested but nvcc \
                 not found; building CPU-only libcolibri (default CI path). Install \
                 CUDA toolkit for GPU embed, or set COLIBRI_REQUIRE_FFI_CUDA=1 to fail hard."
            );
        }
        if want_hip {
            if require_ffi_hip() {
                panic!(
                    "feature ffi-hip / COLIBRI_FFI_HIP requested but ROCm (hipcc / \
                     libamdhip64) not found. Install ROCm or unset the flag. \
                     COLIBRI_REQUIRE_FFI_HIP=1 forbids CPU fallback."
                );
            }
            println!(
                "cargo:warning=feature ffi-hip / COLIBRI_FFI_HIP requested but hipcc \
                 not found; building CPU-only libcolibri (default CI path). Install \
                 ROCm for GPU embed, or set COLIBRI_REQUIRE_FFI_HIP=1 to fail hard."
            );
        }
        ensure_cpu_only_libcolibri(&c_dir, &jobs);
        println!("cargo:rustc-link-search=native={}", c_dir.display());
        println!("cargo:rustc-link-lib=static=colibri");
    }

    // Kimi (CPU / Vulkan process path; no CUDA/HIP objects in FFI archive)
    if let Ok(pre) = env::var("COLIBRI_KIMI_STATIC_LIB") {
        link_prebuilt(&pre, "kimi_k3");
    } else {
        run_make(&c_dir, &["libkimi_k3", &format!("-j{jobs}")]);
        expect_lib(&c_dir, "libkimi_k3.a");
        println!("cargo:rustc-link-search=native={}", c_dir.display());
        println!("cargo:rustc-link-lib=static=kimi_k3");
    }

    // Inkling (CPU only)
    if let Ok(pre) = env::var("COLIBRI_INKLING_STATIC_LIB") {
        link_prebuilt(&pre, "inkling");
    } else {
        run_make(&c_dir, &["libinkling", "LTO=0", &format!("-j{jobs}")]);
        expect_lib(&c_dir, "libinkling.a");
        println!("cargo:rustc-link-search=native={}", c_dir.display());
        println!("cargo:rustc-link-lib=static=inkling");
    }

    link_system_libs();
}

/// Whether the build asked for GLM CUDA embed (feature and/or env).
fn want_ffi_cuda() -> bool {
    env::var_os("CARGO_FEATURE_FFI_CUDA").is_some() || env_truthy("COLIBRI_FFI_CUDA")
}

fn require_ffi_cuda() -> bool {
    env_truthy("COLIBRI_REQUIRE_FFI_CUDA")
}

/// Whether the build asked for GLM HIP embed (feature and/or env).
fn want_ffi_hip() -> bool {
    env::var_os("CARGO_FEATURE_FFI_HIP").is_some() || env_truthy("COLIBRI_FFI_HIP")
}

fn require_ffi_hip() -> bool {
    env_truthy("COLIBRI_REQUIRE_FFI_HIP")
}

fn env_truthy(name: &str) -> bool {
    match env::var(name) {
        Ok(v) => {
            let t = v.trim();
            !t.is_empty()
                && !matches!(
                    t.to_ascii_lowercase().as_str(),
                    "0" | "false" | "no" | "off"
                )
        }
        Err(_) => false,
    }
}

struct CudaPlan {
    cuda_home: PathBuf,
    lib_dir: PathBuf,
}

struct HipPlan {
    rocm_home: PathBuf,
    lib_dir: PathBuf,
    hipcc: Option<PathBuf>,
    /// Explicit arch for make (None → Makefile default / native).
    hip_arch: Option<String>,
    /// When true, pass `COLI_HIP_NO_WMMA=1` (no rocWMMA headers → portable kernels).
    force_no_wmma: bool,
}

/// Locate CUDA toolkit for Linux direct-link embed. Returns None if missing
/// or not on a Linux target (one-platform slice).
fn resolve_cuda_plan() -> Option<CudaPlan> {
    if !is_linux_target() {
        let target = env::var("TARGET").unwrap_or_default();
        let host = env::var("HOST").unwrap_or_default();
        println!(
            "cargo:warning=ffi-cuda is Linux-only in this slice; target={target:?} host={host:?}"
        );
        return None;
    }

    let nvcc = find_nvcc()?;
    let cuda_home = nvcc
        .parent()
        .and_then(|bin| bin.parent())
        .map(|p| p.to_path_buf())
        .or_else(|| env::var_os("CUDA_HOME").map(PathBuf::from))
        .or_else(|| env::var_os("CUDA_PATH").map(PathBuf::from))?;

    let lib_dir = [
        cuda_home.join("lib64"),
        cuda_home.join("lib"),
        PathBuf::from("/usr/local/cuda/lib64"),
        PathBuf::from("/usr/lib/x86_64-linux-gnu"),
    ]
    .into_iter()
    .find(|d| d.join("libcudart.so").exists() || d.join("libcudart.a").exists())
    .unwrap_or_else(|| cuda_home.join("lib64"));

    Some(CudaPlan { cuda_home, lib_dir })
}

/// Locate ROCm/HIP for Linux direct-link embed. Returns None if missing
/// or not on a Linux target.
fn resolve_hip_plan() -> Option<HipPlan> {
    if !is_linux_target() {
        let target = env::var("TARGET").unwrap_or_default();
        let host = env::var("HOST").unwrap_or_default();
        println!(
            "cargo:warning=ffi-hip is Linux-only in this slice; target={target:?} host={host:?}"
        );
        return None;
    }

    let hipcc = find_hipcc()?;
    let rocm_home = hipcc
        .parent()
        .and_then(|bin| bin.parent())
        .map(|p| p.to_path_buf())
        .or_else(|| env::var_os("ROCM_HOME").map(PathBuf::from))
        .or_else(|| env::var_os("ROCM_PATH").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/opt/rocm"));

    let lib_dir = [
        rocm_home.join("lib"),
        rocm_home.join("lib64"),
        PathBuf::from("/opt/rocm/lib"),
        PathBuf::from("/usr/lib/x86_64-linux-gnu"),
    ]
    .into_iter()
    .find(|d| {
        d.join("libamdhip64.so").exists()
            || d.join("libamdhip64.so.6").exists()
            || d.join("libamdhip64.so.7").exists()
            || d.join("libamdhip64.a").exists()
    })
    .unwrap_or_else(|| rocm_home.join("lib"));

    // Require the runtime lib so we do not set ffi_hip_linked without linkable amdhip64.
    if !(lib_dir.join("libamdhip64.so").exists()
        || lib_dir.join("libamdhip64.so.6").exists()
        || lib_dir.join("libamdhip64.so.7").exists()
        || lib_dir.join("libamdhip64.a").exists())
    {
        println!(
            "cargo:warning=ffi-hip: hipcc found but libamdhip64 not under {}; \
             CPU fallback",
            lib_dir.display()
        );
        return None;
    }

    let hip_arch = env::var("HIP_ARCH")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // rocWMMA is optional: without headers, backend_gpu_compat.h hard-errors
    // unless -DCOLI_HIP_NO_WMMA (Makefile COLI_HIP_NO_WMMA=1 or NO_WMMA_ARCHS).
    let force_no_wmma = env_truthy("COLI_HIP_NO_WMMA")
        || !rocm_home
            .join("include")
            .join("rocwmma")
            .join("rocwmma.hpp")
            .is_file();

    Some(HipPlan {
        rocm_home,
        lib_dir,
        hipcc: Some(hipcc),
        hip_arch,
        force_no_wmma,
    })
}

fn is_linux_target() -> bool {
    let target = env::var("TARGET").unwrap_or_default();
    let host = env::var("HOST").unwrap_or_default();
    target.contains("linux") || (target.is_empty() && host.contains("linux"))
}

fn find_nvcc() -> Option<PathBuf> {
    if let Ok(p) = env::var("NVCC") {
        let pb = PathBuf::from(&p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    for key in ["CUDA_HOME", "CUDA_PATH"] {
        if let Ok(home) = env::var(key) {
            let candidate = PathBuf::from(&home).join("bin").join("nvcc");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    // PATH
    if let Ok(path) = env::var("PATH") {
        for dir in env::split_paths(&path) {
            let candidate = dir.join("nvcc");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    // Common install roots
    for root in ["/usr/local/cuda", "/usr/cuda"] {
        let candidate = PathBuf::from(root).join("bin").join("nvcc");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn find_hipcc() -> Option<PathBuf> {
    if let Ok(p) = env::var("HIPCC") {
        let pb = PathBuf::from(&p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    for key in ["ROCM_HOME", "ROCM_PATH"] {
        if let Ok(home) = env::var(key) {
            let candidate = PathBuf::from(&home).join("bin").join("hipcc");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    if let Ok(path) = env::var("PATH") {
        for dir in env::split_paths(&path) {
            let candidate = dir.join("hipcc");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    for root in ["/opt/rocm", "/usr/local/rocm"] {
        let candidate = PathBuf::from(root).join("bin").join("hipcc");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn link_cuda_runtime(plan: &CudaPlan) {
    println!("cargo:rustc-link-search=native={}", plan.lib_dir.display());
    // Match process Makefile: rpath so the host finds cudart without env gymnastics.
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", plan.lib_dir.display());
    println!("cargo:rustc-link-lib=cudart");
    println!("cargo:rustc-link-lib=stdc++");
}

fn link_hip_runtime(plan: &HipPlan) {
    println!("cargo:rustc-link-search=native={}", plan.lib_dir.display());
    // Match process Makefile HIP=1: rpath for libamdhip64.
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", plan.lib_dir.display());
    println!("cargo:rustc-link-lib=amdhip64");
    println!("cargo:rustc-link-lib=stdc++");
}

/// CPU-only `feature=ffi` must not link a leftover HIP/CUDA `libcolibri.a`.
///
/// Delete and remake with explicit `HIP=0 CUDA=0` so a prior clippy `ffi-hip`
/// archive (or a leaked `HIP=1` environment) cannot ride through `ar rcs`.
fn ensure_cpu_only_libcolibri(c_dir: &Path, jobs: &str) {
    let path = c_dir.join("libcolibri.a");
    if path.is_file() {
        let flavor = archive_gpu_flavor(&path);
        if flavor.is_gpu() {
            println!(
                "cargo:warning=existing {} is a {flavor} archive; rebuilding \
                 CPU-only libcolibri (no HIP=1/CUDA=1) for feature=ffi",
                path.display()
            );
            if let Err(e) = std::fs::remove_file(&path) {
                panic!(
                    "could not remove leftover {flavor} {} before CPU-only remake: {e}",
                    path.display()
                );
            }
        }
    }
    run_make(
        c_dir,
        &["libcolibri", "HIP=0", "CUDA=0", &format!("-j{jobs}")],
    );
    expect_lib(c_dir, "libcolibri.a");
    let flavor = archive_gpu_flavor(&path);
    if flavor.is_gpu() {
        panic!(
            "{} still contains {flavor} objects after CPU-only `make libcolibri \
             HIP=0 CUDA=0`. feature=ffi must not link HIP/CUDA objects (that \
             needs ffi-hip or ffi-cuda so the vendor runtime is linked). Delete \
             the archive and rebuild, or enable the matching GPU feature.",
            path.display()
        );
    }
}

fn refuse_gpu_archive_for_cpu_ffi(path: &Path, source: &str) {
    match archive_gpu_flavor(path) {
        GpuArchiveFlavor::None => {}
        flavor => panic!(
            "{source} at {} contains {flavor} objects, but this is a CPU-only \
             `feature=ffi` build (no ffi-cuda / ffi-hip). Rebuild that archive \
             with `make libcolibri` (no HIP=1/CUDA=1), or enable the matching \
             GPU feature so the host links the vendor runtime. Do not link \
             amdhip64/cudart on CPU-only ffi.",
            path.display()
        ),
    }
}

fn link_prebuilt(path: &str, default_name: &str) {
    let p = PathBuf::from(path);
    let dir = p.parent().unwrap_or_else(|| Path::new("."));
    let name = p
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(default_name)
        .trim_start_matches("lib");
    println!("cargo:rustc-link-search=native={}", dir.display());
    println!("cargo:rustc-link-lib=static={name}");
}

fn run_make(c_dir: &Path, args: &[&str]) {
    let status = Command::new("make").args(args).current_dir(c_dir).status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => panic!("make {:?} failed with {s} in {}", args, c_dir.display()),
        Err(e) => panic!("failed to run make {:?} in {}: {e}", args, c_dir.display()),
    }
}

fn expect_lib(c_dir: &Path, name: &str) {
    let p = c_dir.join(name);
    if !p.is_file() {
        panic!("expected static library at {} after make", p.display());
    }
}

fn link_system_libs() {
    println!("cargo:rustc-link-lib=m");
    println!("cargo:rustc-link-lib=gomp");
    println!("cargo:rustc-link-lib=pthread");
    if env::var_os("COLIBRI_V4_LINK_STDCXX").is_some() {
        println!("cargo:rustc-link-lib=stdc++");
    }
}

fn parse_jobs_from_makeflags(flags: String) -> Option<String> {
    let parts: Vec<&str> = flags.split_whitespace().collect();
    for i in 0..parts.len() {
        if parts[i] == "-j" {
            if let Some(n) = parts.get(i + 1) {
                if n.chars().all(|c| c.is_ascii_digit()) {
                    return Some((*n).to_string());
                }
            }
        } else if let Some(rest) = parts[i].strip_prefix("-j") {
            if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
                return Some(rest.to_string());
            }
        }
    }
    None
}
