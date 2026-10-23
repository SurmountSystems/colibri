//! One status sentence for the XDNA button.
//!
//! The NPU on this APU has no graphics VRAM pool. Weights and the 64 by 64 by
//! 64 check share system RAM with the CPU. The check is not token generate.
//! XDNA stays off until the button asks the next engine start to set
//! `COLI_XDNA`.

use std::sync::Mutex;

const NPU_SHARES_SYSTEM_MEMORY: &str = "The NPU uses this machine's system memory together with the CPU. There is no separate graphics VRAM pool.";

/// In-memory outcome of this window's session-start 64 by 64 by 64 check.
/// `None` until the start path records it. Not written to prefs.
static SESSION_PROBE: Mutex<Option<bool>> = Mutex::new(None);

fn session_probe_slot() -> std::sync::MutexGuard<'static, Option<bool>> {
    SESSION_PROBE.lock().unwrap_or_else(|p| p.into_inner())
}

/// Persist the session-start 64³ check for this running window.
/// `Some(true)` completed, `Some(false)` declined or missing device,
/// `None` has not run. Process start and engine start call this through
/// [`record_session_start_probe`].
pub fn set_session_probe_completed(probe_completed: Option<bool>) {
    *session_probe_slot() = probe_completed;
}

/// Stored session-start check. `None` until [`set_session_probe_completed`].
pub fn session_probe_completed() -> Option<bool> {
    *session_probe_slot()
}

/// Store the live 64³ check when XDNA is requested at process start or engine
/// start. Missing device or missing helper is a finished fail, not "has not
/// run". XDNA off clears the stored check. Does not run expert GEMM.
pub fn record_session_start_probe(xdna_requested: bool) {
    if !xdna_requested {
        set_session_probe_completed(None);
        return;
    }
    set_session_probe_completed(Some(live_npu_check_completed()));
}

fn live_npu_check_completed() -> bool {
    let helper = std::env::current_exe()
        .ok()
        .as_deref()
        .and_then(colibri_sys::xdna_lane::linux_helper_beside);
    colibri_sys::xdna_lane::lane_status_live(Some("1"), helper.as_deref()).is_available()
}

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

/// Config sentence for the live XDNA row. Uses the stored session-start check.
pub fn xdna_config_sentence(on: bool) -> String {
    xdna_memory_sentence(on, session_probe_completed())
}

#[cfg(test)]
pub(crate) fn with_session_probe<R>(f: impl FnOnce() -> R) -> R {
    static SESSION_TEST_LOCK: Mutex<()> = Mutex::new(());
    let _guard = SESSION_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    f()
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

    #[test]
    fn completed_probe_does_not_print_has_not_run() {
        for probe in [Some(true), Some(false)] {
            let s = xdna_memory_sentence(true, probe);
            assert!(
                !s.contains("has not run"),
                "completed probe still printed has not run: {s}"
            );
            assert!(!s.contains("generate on"), "{s}");
        }
    }

    #[test]
    fn missing_probe_still_prints_has_not_run() {
        let s = xdna_memory_sentence(true, None);
        assert!(s.contains("has not run"), "{s}");
        assert!(!s.contains("generate on"), "{s}");
    }

    #[test]
    fn config_sentence_uses_stored_session_probe_not_always_none() {
        with_session_probe(|| {
            set_session_probe_completed(None);
            let missing = xdna_config_sentence(true);
            assert!(
                missing.contains("has not run"),
                "None must still print has not run: {missing}"
            );

            set_session_probe_completed(Some(true));
            let done = xdna_config_sentence(true);
            assert!(
                !done.contains("has not run"),
                "stored completed probe must not print has not run: {done}"
            );
            assert!(done.contains("64 by 64 by 64"), "{done}");
            assert!(done.contains("not token generate"), "{done}");
            assert!(!done.contains("generate on"), "{done}");

            set_session_probe_completed(Some(false));
            let failed = xdna_config_sentence(true);
            assert!(
                !failed.contains("has not run"),
                "stored failed probe must not print has not run: {failed}"
            );
            assert!(failed.contains("failed"), "{failed}");
            assert!(!failed.contains("generate on"), "{failed}");

            set_session_probe_completed(None);
            assert!(xdna_config_sentence(true).contains("has not run"));
            assert!(!xdna_config_sentence(false).contains("has not run"));
        });
    }

    #[test]
    fn record_session_start_probe_on_is_not_has_not_run() {
        with_session_probe(|| {
            record_session_start_probe(false);
            assert!(
                xdna_config_sentence(true).contains("has not run"),
                "XDNA off must leave the check unrun"
            );
            record_session_start_probe(true);
            let s = xdna_config_sentence(true);
            assert!(
                !s.contains("has not run"),
                "session-start record must not print has not run: {s}"
            );
            assert!(!s.contains("generate on"), "{s}");
            record_session_start_probe(false);
        });
    }
}
