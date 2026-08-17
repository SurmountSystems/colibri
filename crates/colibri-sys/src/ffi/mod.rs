//! Optional multi-family in-process engines (`feature = "ffi"`).
//!
//! Links static libraries built without CLI `main`:
//! - DeepSeek V4: `libdeepseek_v4.a` (`COLI_V4_SKIP_GENERATE_MAIN`)
//! - GLM: `libcolibri.a` (`COLIBRI_NO_MAIN`; **CPU by default**)
//! - Kimi K3: `libkimi_k3.a` (`KIMI_NO_MAIN`)
//! - Inkling: `libinkling.a` (`INKLING_NO_MAIN`, CPU only)
//!
//! ## GPU (opt-in Linux CUDA or HIP for GLM; one vendor per binary)
//!
//! Default `feature = "ffi"` archives are **CPU-only**. Opt-in:
//! - Cargo `ffi-cuda` (implies `ffi`), and/or env `COLIBRI_FFI_CUDA=1` at build
//! - Cargo `ffi-hip` (implies `ffi`), and/or env `COLIBRI_FFI_HIP=1` at build
//! - `make -C c libcolibri CUDA=1` or `HIP=1` packs `backend_cuda.o`
//!
//! When the matching toolkit is present, rustc cfg `ffi_cuda_linked` or
//! `ffi_hip_linked` is set. Without a toolkit, build.rs falls back to
//! CPU-only (see feature-enabled vs linked helpers). **CUDA and HIP cannot
//! both be linked** into one host binary (`build.rs` panics if both forced).
//! NPU, Metal, and Vulkan remain out of the FFI static matrix.
//!
//! ## Availability split
//!
//! | Function | Meaning |
//! |----------|---------|
//! | [`ffi_link_available`] | Linked static engines (always true under `feature = "ffi"`). |
//! | [`ffi_available`] | Link available and `COLIBRI_FORCE_PROCESS` is not forcing process. |
//! | [`ffi_family_available`] | Same, for a given [`FfiFamily`]. |
//! | [`ffi_cuda_feature_enabled`] | Build requested CUDA embed (`ffi-cuda` / env). |
//! | [`ffi_cuda_linked`] | GLM actually linked with CUDA backend + cudart. |
//! | [`ffi_hip_feature_enabled`] | Build requested HIP embed (`ffi-hip` / env). |
//! | [`ffi_hip_linked`] | GLM actually linked with HIP backend + amdhip64. |
//! | [`open_engine`] | Family-selected open; may fail on missing weights / kill-switch. |
//!
//! Process serve mux remains the **default** product path
//! (`ColibriConfig::prefer_process = true`). See crate `docs/ffi-phase-d.md`.
//!
//! Model **size** for hosts: use [`crate::ModelInfo::size_info`] /
//! [`crate::ModelSizeInfo`] (always available without `ffi`) or
//! [`FfiEngine::size_info`] after open.

mod bindings;
mod multi;
mod v4;

#[cfg(test)]
use std::sync::atomic::{AtomicU32, Ordering};
#[cfg(test)]
use std::sync::{Mutex, MutexGuard};

#[cfg(test)]
static APPLY_FFI_COMPUTE_NICENESS_CALLS: AtomicU32 = AtomicU32::new(0);

/// Serializes tests that touch C `g_embed_stop` or `COLI_TEST_MEM_AVAIL_*`.
/// Default cargo test threads can race those process globals across modules.
#[cfg(test)]
static FFI_PROCESS_GLOBAL_TEST: Mutex<()> = Mutex::new(());

#[cfg(test)]
pub(crate) fn lock_ffi_process_global_test() -> MutexGuard<'static, ()> {
    FFI_PROCESS_GLOBAL_TEST
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Demote this thread and the persistent OpenMP team.
///
/// Call only from FFI start / generate workers. The OpenMP master is the
/// caller, so this must not run on the GPUI thread.
pub(crate) fn apply_ffi_compute_niceness() {
    #[cfg(test)]
    APPLY_FFI_COMPUTE_NICENESS_CALLS.fetch_add(1, Ordering::SeqCst);
    let _ = crate::process_priority::set_current_thread_nice(crate::ENGINE_CHILD_NICE);
    unsafe {
        bindings::coli_nice_compute_threads(crate::ENGINE_CHILD_NICE);
    }
}

#[cfg(test)]
pub(crate) fn apply_ffi_compute_niceness_call_count() -> u32 {
    APPLY_FFI_COMPUTE_NICENESS_CALLS.load(Ordering::SeqCst)
}

#[cfg(test)]
pub(crate) fn coli_nice_compute_threads(nice: i32) -> i32 {
    unsafe { bindings::coli_nice_compute_threads(nice) }
}

#[cfg(test)]
pub(crate) fn coli_openmp_team_all_at_nice(nice: i32) -> bool {
    unsafe { bindings::coli_openmp_team_all_at_nice(nice) != 0 }
}

pub use crate::config::{FORCE_PROCESS_ENV, env_force_process, force_process_from_env};
pub use bindings::COLI_COMPUTE_NICE;
pub use multi::{FfiEngine, FfiFamily, FfiGenerateOptions, GlmOpenOptions, open_engine, open_glm};

/// Cooperative embed stop. Does not take the engine mutex. C decode/prefill
/// checks this between tokens and prefill chunks.
pub fn request_embed_stop() {
    unsafe { bindings::coli_embed_request_stop() }
}

/// Clear embed stop at the start of a generate.
pub fn clear_embed_stop() {
    unsafe { bindings::coli_embed_clear_stop() }
}

/// Whether C decode/prefill should stop (reads `g_embed_stop` plus mux/SIGINT).
pub fn embed_should_stop() -> bool {
    unsafe { bindings::coli_embed_should_stop() != 0 }
}
pub use v4::{
    V4Engine, V4EngineOpenOptions, V4GenerateOptions, V4GenerateStats, V4Session,
    V4SessionCreateOptions, V4TokenEvent,
};

/// True when this crate was built with `feature = "ffi"` and static engines linked.
#[inline]
pub fn ffi_link_available() -> bool {
    true
}

/// Families linked in this build (all product engines under `ffi`).
pub fn linked_families() -> &'static [FfiFamily] {
    &[
        FfiFamily::Glm,
        FfiFamily::Kimi,
        FfiFamily::Inkling,
        FfiFamily::DeepseekV4,
    ]
}

/// True when in-process may be used for any family (link + env kill-switch).
#[inline]
pub fn ffi_available() -> bool {
    ffi_link_available() && !force_process_from_env()
}

/// True when a specific family may be opened in-process.
#[inline]
pub fn ffi_family_available(family: FfiFamily) -> bool {
    ffi_available() && linked_families().contains(&family)
}

/// True when this build **requested** GLM CUDA embed (`feature = "ffi-cuda"`).
///
/// Does **not** mean cudart is linked: without a CUDA toolkit, build.rs falls
/// back to CPU-only. Use [`ffi_cuda_linked`] for the actual link matrix.
#[inline]
pub fn ffi_cuda_feature_enabled() -> bool {
    cfg!(feature = "ffi-cuda")
}

/// True when GLM embed was built with Linux CUDA objects and linked against
/// `cudart` (rustc cfg `ffi_cuda_linked` from `build.rs`).
///
/// Default `feature = "ffi"` (no `ffi-cuda`) is always false. With `ffi-cuda`
/// but no toolkit at build time, also false (CPU fallback).
#[inline]
pub fn ffi_cuda_linked() -> bool {
    cfg!(ffi_cuda_linked)
}

/// True when this build **requested** GLM HIP embed (`feature = "ffi-hip"`).
///
/// Does **not** mean amdhip64 is linked: without ROCm, build.rs falls back
/// to CPU-only. Use [`ffi_hip_linked`] for the actual link matrix.
#[inline]
pub fn ffi_hip_feature_enabled() -> bool {
    cfg!(feature = "ffi-hip")
}

/// True when GLM embed was built with Linux HIP objects and linked against
/// `amdhip64` (rustc cfg `ffi_hip_linked` from `build.rs`).
///
/// Default `feature = "ffi"` (no `ffi-hip`) is always false. With `ffi-hip`
/// but no toolkit at build time, also false (CPU fallback).
#[inline]
pub fn ffi_hip_linked() -> bool {
    cfg!(ffi_hip_linked)
}

/// True when any in-process GPU vendor was actually linked (CUDA or HIP).
#[inline]
pub fn ffi_gpu_linked() -> bool {
    ffi_cuda_linked() || ffi_hip_linked()
}

#[cfg(test)]
mod cuda_gate_tests {
    use super::{
        ffi_cuda_feature_enabled, ffi_cuda_linked, ffi_gpu_linked, ffi_hip_feature_enabled,
        ffi_hip_linked, ffi_link_available,
    };

    /// Documents the product contract: default `feature = "ffi"` is CPU-only.
    #[test]
    fn default_ffi_without_gpu_features_is_cpu_only() {
        assert!(ffi_link_available());
        #[cfg(not(feature = "ffi-cuda"))]
        {
            assert!(
                !ffi_cuda_feature_enabled(),
                "ffi-cuda feature must be off under --features ffi alone"
            );
            assert!(
                !ffi_cuda_linked(),
                "default ffi must not set ffi_cuda_linked (CPU-only archives)"
            );
        }
        #[cfg(not(feature = "ffi-hip"))]
        {
            assert!(
                !ffi_hip_feature_enabled(),
                "ffi-hip feature must be off under --features ffi alone"
            );
            assert!(
                !ffi_hip_linked(),
                "default ffi must not set ffi_hip_linked (CPU-only archives)"
            );
        }
        // When neither GPU feature is on, no vendor runtime is linked.
        // With ffi-cuda or ffi-hip, ffi_gpu_linked tracks actual toolkit link.
        assert_eq!(
            ffi_gpu_linked(),
            ffi_cuda_linked() || ffi_hip_linked(),
            "ffi_gpu_linked must match cuda||hip linked flags"
        );
        #[cfg(all(not(feature = "ffi-cuda"), not(feature = "ffi-hip")))]
        {
            assert!(
                !ffi_gpu_linked(),
                "default ffi must not link any GPU vendor runtime"
            );
        }
    }

    /// When `ffi-cuda` is on, the feature flag is visible; link is host-gated.
    #[test]
    fn ffi_cuda_feature_reports_request_not_necessarily_link() {
        #[cfg(feature = "ffi-cuda")]
        {
            assert!(
                ffi_cuda_feature_enabled(),
                "ffi-cuda feature must report enabled"
            );
            // Linked only if nvcc was present at build time (this host may not).
            // Do not require ffi_cuda_linked() so default CI without CUDA stays green.
            let _ = ffi_cuda_linked();
        }
        #[cfg(not(feature = "ffi-cuda"))]
        {
            assert!(!ffi_cuda_feature_enabled());
            assert!(!ffi_cuda_linked());
        }
    }

    /// When `ffi-hip` is on, the feature flag is visible; link is host-gated.
    #[test]
    fn ffi_hip_feature_reports_request_not_necessarily_link() {
        #[cfg(feature = "ffi-hip")]
        {
            assert!(
                ffi_hip_feature_enabled(),
                "ffi-hip feature must report enabled"
            );
            // Linked only if hipcc/ROCm was present at build time.
            // Do not require ffi_hip_linked() so CI without ROCm stays green.
            let _ = ffi_hip_linked();
        }
        #[cfg(not(feature = "ffi-hip"))]
        {
            assert!(!ffi_hip_feature_enabled());
            assert!(!ffi_hip_linked());
        }
    }

    /// Hosts with a CUDA toolkit can opt into a smoke that requires real link.
    /// Default CI skips (ignore) so machines without CUDA stay green.
    #[test]
    #[ignore = "requires feature ffi-cuda + CUDA toolkit at build (COLIBRI_REQUIRE_FFI_CUDA=1)"]
    fn ffi_cuda_linked_when_toolkit_present() {
        assert!(
            ffi_cuda_feature_enabled(),
            "enable --features ffi-cuda for this smoke"
        );
        assert!(
            ffi_cuda_linked(),
            "expected ffi_cuda_linked after building with nvcc; \
             set COLIBRI_REQUIRE_FFI_CUDA=1 if build fell back to CPU"
        );
    }

    /// Hosts with ROCm can opt into a smoke that requires real HIP link.
    /// Default CI skips (ignore) so machines without ROCm stay green.
    #[test]
    #[ignore = "requires feature ffi-hip + ROCm/hipcc at build (COLIBRI_REQUIRE_FFI_HIP=1)"]
    fn ffi_hip_linked_when_toolkit_present() {
        assert!(
            ffi_hip_feature_enabled(),
            "enable --features ffi-hip for this smoke"
        );
        assert!(
            ffi_hip_linked(),
            "expected ffi_hip_linked after building with hipcc; \
             set COLIBRI_REQUIRE_FFI_HIP=1 if build fell back to CPU"
        );
    }
}

#[cfg(test)]
mod embed_stop_tests {
    use super::*;

    #[test]
    fn coli_embed_should_stop_reads_request_flag() {
        let _g = lock_ffi_process_global_test();
        unsafe {
            bindings::coli_embed_clear_stop();
            assert_eq!(
                bindings::coli_embed_should_stop(),
                0,
                "clear must reset the C flag"
            );
            bindings::coli_embed_request_stop();
            assert_eq!(
                bindings::coli_embed_should_stop(),
                1,
                "request must set the C flag that spec_decode reads"
            );
            bindings::coli_embed_clear_stop();
            assert_eq!(
                bindings::coli_embed_should_stop(),
                0,
                "clear after request must reset"
            );
        }
    }

    #[test]
    fn coli_prefill_should_run_leftover_skips_when_stopped() {
        let _g = lock_ffi_process_global_test();
        unsafe {
            bindings::coli_embed_clear_stop();
            assert_eq!(
                bindings::coli_prefill_should_run_leftover(10),
                1,
                "default path leftover layers_forward runs when not stopped"
            );
            bindings::coli_embed_request_stop();
            assert_eq!(
                bindings::coli_prefill_should_run_leftover(10),
                0,
                "chunk-loop break must not run leftover layers_forward"
            );
            bindings::coli_embed_clear_stop();
        }
    }
}

#[cfg(test)]
mod glm_ram_sample_tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::PathBuf;

    struct RestoreEnv {
        keys: Vec<(&'static str, Option<OsString>)>,
    }

    impl RestoreEnv {
        fn capture(keys: &[&'static str]) -> Self {
            Self {
                keys: keys.iter().map(|k| (*k, std::env::var_os(k))).collect(),
            }
        }
    }

    impl Drop for RestoreEnv {
        fn drop(&mut self) {
            for (k, v) in &self.keys {
                unsafe {
                    match v {
                        Some(val) => std::env::set_var(k, val),
                        None => std::env::remove_var(k),
                    }
                }
            }
        }
    }

    /// C auto budget is 88% of the 64 GiB pre-load inject (`[RAM_GB=56.3 auto]`).
    fn names_preload_64gib_88pct_budget(text: &str) -> bool {
        text.to_ascii_lowercase().contains("ram_gb=56.3")
    }

    /// Pre-load MemAvailable (64 GiB) fits; leftover after load (0.5 GiB) would
    /// false-refuse if sampled after `model_init`.
    #[test]
    fn glm_tiny_open_uses_preload_mem_not_leftover_after_init() {
        let _g = lock_ffi_process_global_test();
        let _restore = RestoreEnv::capture(&[
            "COLI_TEST_MEM_AVAIL_GB",
            "COLI_TEST_MEM_AVAIL_AFTER_GB",
            "RAM_GB",
            "COLI_RAM_OVERCOMMIT",
        ]);
        unsafe {
            std::env::remove_var("RAM_GB");
            std::env::remove_var("COLI_RAM_OVERCOMMIT");
            std::env::set_var("COLI_TEST_MEM_AVAIL_GB", "64");
            std::env::set_var("COLI_TEST_MEM_AVAIL_AFTER_GB", "0.5");
        }
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../c/glm_tiny");
        assert!(
            root.join("model.safetensors").is_file(),
            "c/glm_tiny/model.safetensors is required; this test must not skip"
        );
        assert!(
            !force_process_from_env(),
            "this test opens the in-process engine; unset COLIBRI_FORCE_PROCESS"
        );
        match open_engine(FfiFamily::Glm, &root) {
            Ok(_) => {}
            Err(e) => {
                let s = e.to_string();
                assert!(
                    names_preload_64gib_88pct_budget(&s),
                    "open failed without proving the 64 GiB / 88% inject (Ok or banner): {s}"
                );
            }
        }
    }
}
