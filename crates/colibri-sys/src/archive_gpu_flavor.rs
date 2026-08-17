//! Detect leftover HIP/CUDA objects in a `libcolibri.a` (or override) archive.
//!
//! `feature = "ffi"` is CPU-only. A prior `ffi-hip` / `ffi-cuda` clippy run can
//! leave `backend_cuda.o` in `c/libcolibri.a`. Those objects reference vendor
//! symbols such as `__hipUnregisterFatBinary` that CPU-only link does not
//! satisfy. This helper is shared by `build.rs` (include) and crate tests.
//!
//! Detection is a byte scan of unique fatbin register/unregister names so
//! tests can use canned `nm` text (or any blob) instead of binary fixtures.

use std::fmt;
use std::path::Path;

/// Unique HIP fatbin unregister (hipcc-packed `backend_cuda.o`).
pub const HIP_FATBIN_UNREGISTER: &[u8] = b"__hipUnregisterFatBinary";
/// Unique HIP fatbin register (same objects; some tables omit unregister).
pub const HIP_FATBIN_REGISTER: &[u8] = b"__hipRegisterFatBinary";
/// Unique CUDA fatbin unregister (nvcc-packed `backend_cuda.o`).
pub const CUDA_FATBIN_UNREGISTER: &[u8] = b"__cudaUnregisterFatBinary";
/// CUDA fatbin register (some toolchains omit unregister in the table).
pub const CUDA_FATBIN_REGISTER: &[u8] = b"__cudaRegisterFatBinary";

/// GPU vendor objects packed into a static archive, or none (CPU-only / missing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuArchiveFlavor {
    None,
    Hip,
    Cuda,
}

impl GpuArchiveFlavor {
    /// True when the archive carries HIP or CUDA objects.
    pub const fn is_gpu(self) -> bool {
        !matches!(self, Self::None)
    }
}

impl fmt::Display for GpuArchiveFlavor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::None => "cpu",
            Self::Hip => "HIP",
            Self::Cuda => "CUDA",
        })
    }
}

fn contains_ascii(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Infer flavor from archive bytes or from `nm` / `llvm-nm` / `readelf` text.
///
/// HIP wins when both vendor markers appear (HIP CUDA-compat objects often
/// mention `__cudaRegisterFatBinary` as well as `__hipUnregisterFatBinary`).
pub fn flavor_from_bytes(data: &[u8]) -> GpuArchiveFlavor {
    if contains_ascii(data, HIP_FATBIN_UNREGISTER) || contains_ascii(data, HIP_FATBIN_REGISTER) {
        return GpuArchiveFlavor::Hip;
    }
    if contains_ascii(data, CUDA_FATBIN_UNREGISTER) || contains_ascii(data, CUDA_FATBIN_REGISTER) {
        return GpuArchiveFlavor::Cuda;
    }
    GpuArchiveFlavor::None
}

/// Same as [`flavor_from_bytes`] for a canned `nm -g` (or similar) listing.
///
/// Used by unit tests. `build.rs` includes this file and classifies archives
/// via [`archive_gpu_flavor`] / [`flavor_from_bytes`] only.
#[cfg_attr(not(test), allow(dead_code))]
pub fn flavor_from_nm_lines(text: &str) -> GpuArchiveFlavor {
    flavor_from_bytes(text.as_bytes())
}

/// Read `path` and classify it. Missing or empty files are [`GpuArchiveFlavor::None`].
pub fn archive_gpu_flavor(path: &Path) -> GpuArchiveFlavor {
    match std::fs::read(path) {
        Ok(data) if data.is_empty() => GpuArchiveFlavor::None,
        Ok(data) => flavor_from_bytes(&data),
        Err(_) => GpuArchiveFlavor::None,
    }
}

#[cfg(test)]
mod tests {
    use super::{GpuArchiveFlavor, archive_gpu_flavor, flavor_from_bytes, flavor_from_nm_lines};
    use std::path::Path;

    #[test]
    fn missing_file_is_none() {
        assert_eq!(
            archive_gpu_flavor(Path::new("/no/such/colibri/libcolibri.a")),
            GpuArchiveFlavor::None
        );
    }

    #[test]
    fn empty_bytes_are_none() {
        assert_eq!(flavor_from_bytes(b""), GpuArchiveFlavor::None);
        assert_eq!(flavor_from_nm_lines(""), GpuArchiveFlavor::None);
    }

    #[test]
    fn empty_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.a");
        std::fs::write(&path, b"").unwrap();
        assert_eq!(archive_gpu_flavor(&path), GpuArchiveFlavor::None);
    }

    #[test]
    fn cpu_archive_text_is_none() {
        let nm = "\
0000000000000000 T coli_glm_open
0000000000000100 T coli_glm_generate
                 U memcpy
";
        assert_eq!(flavor_from_nm_lines(nm), GpuArchiveFlavor::None);
        assert!(!GpuArchiveFlavor::None.is_gpu());
    }

    #[test]
    fn hip_unregister_from_nm_lines() {
        let nm = "\
backend_cuda.o:
0000000000000000 T coli_cuda_init
                 U __hipUnregisterFatBinary
";
        assert_eq!(flavor_from_nm_lines(nm), GpuArchiveFlavor::Hip);
        assert!(GpuArchiveFlavor::Hip.is_gpu());
    }

    #[test]
    fn cuda_unregister_from_nm_lines() {
        let nm = "\
backend_cuda.o:
0000000000000000 T coli_cuda_init
                 U __cudaUnregisterFatBinary
";
        assert_eq!(flavor_from_nm_lines(nm), GpuArchiveFlavor::Cuda);
        assert!(GpuArchiveFlavor::Cuda.is_gpu());
    }

    #[test]
    fn hip_wins_when_both_vendor_markers_present() {
        let blob = b"pad __cudaRegisterFatBinary __hipUnregisterFatBinary pad";
        assert_eq!(flavor_from_bytes(blob), GpuArchiveFlavor::Hip);
    }

    #[test]
    fn display_names_are_plain() {
        assert_eq!(GpuArchiveFlavor::None.to_string(), "cpu");
        assert_eq!(GpuArchiveFlavor::Hip.to_string(), "HIP");
        assert_eq!(GpuArchiveFlavor::Cuda.to_string(), "CUDA");
    }
}
