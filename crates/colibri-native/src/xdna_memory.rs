//! One status sentence for the XDNA button.
//!
//! The NPU on this APU has no graphics VRAM pool. Weights and the 64 by 64 by
//! 64 check share system RAM with the CPU. The check is not token generate.
//! XDNA stays off until the button asks the next engine start to set
//! `COLI_XDNA`.

const NPU_SHARES_SYSTEM_MEMORY: &str = "The NPU uses this machine's system memory together with the CPU. There is no separate graphics VRAM pool.";

/// `on` is the XDNA button. `probe_completed` is the 64 by 64 by 64 NPU check:
/// missing if it has not run, `Some(false)` if it failed, `Some(true)` if it
/// finished. A finished check is not token generate and is not a claim that
/// expert GEMM or generation is using the NPU. When `on` is false the probe
/// value is ignored.
pub fn xdna_memory_sentence(on: bool, probe_completed: Option<bool>) -> String {
    if !on {
        return "XDNA is off. The next engine start leaves COLI_XDNA unset.".to_string();
    }
    match probe_completed {
        None => format!(
            "The next engine start will request the NPU check and set COLI_XDNA. {NPU_SHARES_SYSTEM_MEMORY} The check has not run."
        ),
        Some(false) => {
            format!("The NPU check requested with COLI_XDNA failed. {NPU_SHARES_SYSTEM_MEMORY}")
        }
        Some(true) => format!(
            "The 64 by 64 by 64 check requested with COLI_XDNA completed. It is not token generate. {NPU_SHARES_SYSTEM_MEMORY}"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_leaves_coli_xdna_unset_and_does_not_say_completed() {
        let expected = "XDNA is off. The next engine start leaves COLI_XDNA unset.";
        for probe in [None, Some(false), Some(true)] {
            let s = xdna_memory_sentence(false, probe);
            assert_eq!(s, expected);
            assert!(s.contains("COLI_XDNA"), "{s}");
            assert!(!s.contains("completed"), "{s}");
        }
    }

    #[test]
    fn on_without_probe_says_check_has_not_run() {
        let s = xdna_memory_sentence(true, None);
        assert_eq!(
            s,
            "The next engine start will request the NPU check and set COLI_XDNA. The NPU uses this machine's system memory together with the CPU. There is no separate graphics VRAM pool. The check has not run."
        );
        assert!(s.contains("system memory"), "{s}");
        assert!(s.contains("COLI_XDNA"), "{s}");
        assert!(!s.contains("completed"), "{s}");
    }

    #[test]
    fn on_failed_probe_still_names_system_memory() {
        let s = xdna_memory_sentence(true, Some(false));
        assert_eq!(
            s,
            "The NPU check requested with COLI_XDNA failed. The NPU uses this machine's system memory together with the CPU. There is no separate graphics VRAM pool."
        );
        assert!(s.contains("system memory"), "{s}");
        assert!(s.contains("COLI_XDNA"), "{s}");
        assert!(s.contains("failed"), "{s}");
    }

    #[test]
    fn on_completed_probe_is_the_small_check_not_token_generate() {
        let s = xdna_memory_sentence(true, Some(true));
        assert_eq!(
            s,
            "The 64 by 64 by 64 check requested with COLI_XDNA completed. It is not token generate. The NPU uses this machine's system memory together with the CPU. There is no separate graphics VRAM pool."
        );
        assert!(s.contains("system memory"), "{s}");
        assert!(s.contains("COLI_XDNA"), "{s}");
        assert!(s.contains("64 by 64 by 64"), "{s}");
        assert!(s.contains("completed"), "{s}");
        assert!(s.contains("not token generate"), "{s}");
    }
}
