//! Process-engine GPU linkage helpers (CUDA / HIP).
//!
//! Detect whether a **process** engine binary was built with a GPU runtime
//! without loading that runtime. Used by doctor and tests.
//!
//! - Linux: `ldd` for `libcudart` / `libamdhip64` (same contract as
//!   `c/doctor.py` `cuda_linkage`).
//! - Fallback / unit tests: scan file bytes for the same soname markers
//!   (HIP-linked ELF carries `libamdhip64` in dynamic string tables).
//! - Windows: sibling `coli_cuda.dll` / `coli_hip.dll` + CUDA mode marker.
//!
//! Process HIP build (Linux):
//! ```text
//! make -C c colibri HIP=1          # or: glm HIP=1
//! make -C c inkling HIP=1          # when inkling uses the CUDA object
//! # ROCM_HOME / ROCM_PATH default /opt/rocm
//! # HIP_ARCH=native (rocm_agent_enumerator) or explicit gfxNNNN
//! ldd c/colibri | grep libamdhip64
//! export COLI_ENGINE=$PWD/c/colibri   # or COLIBRI_ENGINE
//! ```

use std::path::Path;
use std::process::Command;

/// CUDA / HIP linkage of a process engine binary (no runtime load).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessGpuLinkage {
    pub linked: bool,
    pub missing: bool,
    /// `"cuda"`, `"hip"`, or empty when neither is linked / unknown.
    pub kind: String,
}

/// Parse one `ldd` stdout blob into CUDA/HIP linkage.
///
/// Pure helper for tests and for [`probe_process_gpu_linkage`].
pub fn parse_ldd_gpu_linkage(ldd_stdout: &str) -> ProcessGpuLinkage {
    let mut linked = false;
    let mut missing = false;
    let mut kind = String::new();
    for line in ldd_stdout.lines() {
        let is_cuda = line.contains("libcudart");
        let is_hip = line.contains("libamdhip64");
        if !is_cuda && !is_hip {
            continue;
        }
        if line.contains("not found") {
            missing = true;
        } else {
            linked = true;
        }
        if is_hip {
            kind = "hip".into();
        } else if is_cuda && kind.is_empty() {
            kind = "cuda".into();
        }
    }
    ProcessGpuLinkage {
        linked,
        missing,
        kind,
    }
}

/// Whether file bytes look like a HIP-linked process engine (soname marker).
///
/// Does not prove the dynamic linker will resolve the library; use
/// [`parse_ldd_gpu_linkage`] / live `ldd` for that. Useful when `ldd` is
/// unavailable or for unit tests with a fixture blob containing the marker.
pub fn bytes_mention_hip_runtime(bytes: &[u8]) -> bool {
    contains_ascii(bytes, b"libamdhip64")
}

/// Whether file bytes look like a CUDA-linked process engine (soname marker).
pub fn bytes_mention_cuda_runtime(bytes: &[u8]) -> bool {
    contains_ascii(bytes, b"libcudart")
}

/// Infer linkage from raw binary bytes when `ldd` is not used.
///
/// Prefer live `ldd` on Unix for missing-vs-linked accuracy. This path only
/// detects **kind** markers embedded in the file (typical for HIP/CUDA
/// NEEDED entries).
pub fn parse_bytes_gpu_markers(bytes: &[u8]) -> ProcessGpuLinkage {
    let hip = bytes_mention_hip_runtime(bytes);
    let cuda = bytes_mention_cuda_runtime(bytes);
    if hip {
        return ProcessGpuLinkage {
            linked: true,
            missing: false,
            kind: "hip".into(),
        };
    }
    if cuda {
        return ProcessGpuLinkage {
            linked: true,
            missing: false,
            kind: "cuda".into(),
        };
    }
    ProcessGpuLinkage::default()
}

fn contains_ascii(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Probe process-engine GPU linkage at `engine_path`.
///
/// Returns default (unlinked) when the path is missing or probing fails.
pub fn probe_process_gpu_linkage(engine_path: &Path) -> ProcessGpuLinkage {
    if !engine_path.is_file() {
        return ProcessGpuLinkage::default();
    }
    #[cfg(unix)]
    {
        let out = Command::new("ldd").arg(engine_path).output();
        if let Ok(out) = out {
            if out.status.success() || !out.stdout.is_empty() {
                let text = String::from_utf8_lossy(&out.stdout);
                let from_ldd = parse_ldd_gpu_linkage(&text);
                if from_ldd.linked || from_ldd.missing || !from_ldd.kind.is_empty() {
                    return from_ldd;
                }
            }
        }
        // Fallback: ELF/string markers (still no "missing" accuracy).
        if let Ok(bytes) = std::fs::read(engine_path) {
            return parse_bytes_gpu_markers(&bytes);
        }
        ProcessGpuLinkage::default()
    }
    #[cfg(windows)]
    {
        let bytes = match std::fs::read(engine_path) {
            Ok(b) => b,
            Err(_) => return ProcessGpuLinkage::default(),
        };
        let parent = engine_path.parent().unwrap_or_else(|| Path::new("."));
        let cuda_dll = parent.join("coli_cuda.dll").is_file();
        let hip_dll = parent.join("coli_hip.dll").is_file();
        let built_cuda = bytes
            .windows(b"[CUDA] mode: routed experts".len())
            .any(|w| w == b"[CUDA] mode: routed experts");
        if hip_dll {
            return ProcessGpuLinkage {
                linked: true,
                missing: false,
                kind: "hip".into(),
            };
        }
        if built_cuda || cuda_dll {
            return ProcessGpuLinkage {
                linked: cuda_dll,
                missing: built_cuda && !cuda_dll,
                kind: "cuda".into(),
            };
        }
        ProcessGpuLinkage::default()
    }
    #[cfg(all(not(unix), not(windows)))]
    {
        let _ = engine_path;
        ProcessGpuLinkage::default()
    }
}

/// One-line operational next step when AMD is present but the process engine
/// is CPU-only (not HIP-linked).
///
/// Plain English for doctor details. Prefers process `HIP=1`; mentions
/// Cargo `ffi-hip` only as an alternate when that feature is the in-process path.
pub fn hip_process_rebuild_next_step(engine_basename: &str) -> String {
    let name = if engine_basename.is_empty() {
        "colibri"
    } else {
        engine_basename
    };
    format!(
        "Rebuild the process engine with HIP: `make -C c {name} HIP=1` \
         (ROCM_HOME/ROCM_PATH default /opt/rocm; HIP_ARCH=native or explicit \
         gfxNNNN, e.g. gfx1152 for some APUs). Then set COLI_ENGINE or \
         COLIBRI_ENGINE to that binary (or leave locate to find c/{name}). \
         Alternate: rebuild native with Cargo feature ffi-hip for in-process HIP."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parse_ldd_hip_linked() {
        let text = "\
\tlinux-vdso.so.1 (0x0000)
\tlibamdhip64.so.6 => /opt/rocm/lib/libamdhip64.so.6 (0x0000)
\tlibc.so.6 => /usr/lib/libc.so.6 (0x0000)
";
        let l = parse_ldd_gpu_linkage(text);
        assert!(l.linked);
        assert!(!l.missing);
        assert_eq!(l.kind, "hip");
    }

    #[test]
    fn parse_ldd_hip_missing() {
        let text = "\tlibamdhip64.so.6 => not found\n";
        let l = parse_ldd_gpu_linkage(text);
        assert!(!l.linked);
        assert!(l.missing);
        assert_eq!(l.kind, "hip");
    }

    #[test]
    fn parse_ldd_cuda_linked() {
        let text = "\tlibcudart.so.12 => /usr/local/cuda/lib64/libcudart.so.12 (0x0)\n";
        let l = parse_ldd_gpu_linkage(text);
        assert!(l.linked);
        assert_eq!(l.kind, "cuda");
    }

    #[test]
    fn parse_ldd_cpu_only() {
        let text = "\tlibgomp.so.1 => /usr/lib/libgomp.so.1 (0x0)\n\tlibc.so.6 => /usr/lib/libc.so.6 (0x0)\n";
        let l = parse_ldd_gpu_linkage(text);
        assert!(!l.linked);
        assert!(!l.missing);
        assert!(l.kind.is_empty());
    }

    #[test]
    fn bytes_marker_hip() {
        let blob = b"ELF\0pad libamdhip64.so.7 more";
        assert!(bytes_mention_hip_runtime(blob));
        assert!(!bytes_mention_cuda_runtime(blob));
        let l = parse_bytes_gpu_markers(blob);
        assert!(l.linked);
        assert_eq!(l.kind, "hip");
    }

    #[test]
    fn bytes_marker_cpu() {
        let blob = b"#!/bin/sh\necho cpu only\n";
        assert!(!bytes_mention_hip_runtime(blob));
        assert_eq!(parse_bytes_gpu_markers(blob), ProcessGpuLinkage::default());
    }

    #[test]
    fn probe_fixture_with_hip_marker() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fake-hip-engine");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"#!/bin/sh\n# NEEDED libamdhip64.so.6\nexit 0\n")
            .unwrap();
        drop(f);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
        }
        // ldd on a shell script usually yields nothing useful; bytes fallback
        // should still report hip when the marker is present.
        let l = probe_process_gpu_linkage(&path);
        assert_eq!(l.kind, "hip", "{l:?}");
        assert!(l.linked, "{l:?}");
    }

    #[test]
    fn next_step_names_make_hip_and_env() {
        let s = hip_process_rebuild_next_step("colibri");
        assert!(s.contains("HIP=1"), "{s}");
        assert!(s.contains("make -C c colibri"), "{s}");
        assert!(
            s.contains("COLI_ENGINE") || s.contains("COLIBRI_ENGINE"),
            "{s}"
        );
        assert!(s.contains("ROCM"), "{s}");
        // Process path is primary; ffi-hip is optional alternate wording.
        assert!(s.contains("ffi-hip"), "{s}");
    }
}
