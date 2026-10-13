//! Lower OS scheduling priority for heavy compute (engine serve children, FFI workers).
//!
//! The **GPUI / UI thread stays at default priority**. Heavy work is demoted
//! either in a child `pre_exec` or on the calling **thread** after spawn.
//!
//! | Platform | Mechanism |
//! |----------|-----------|
//! | Linux / macOS / other Unix | `setpriority(PRIO_PROCESS, 0, ENGINE_CHILD_NICE)` from the worker (Linux: that thread only) or in child `pre_exec` |
//! | Windows | `BELOW_NORMAL_PRIORITY_CLASS` via process creation flags (children) |
//!
//! Do **not** `setpriority` the whole `colibri-native` pid and do not walk
//! `/proc/self/task`. Either would demote GPUI. Call [`set_current_thread_nice`]
//! only from compute workers (engine start, generate, install children).

use std::process::Command;

/// Unix niceness applied to engine serve (and similar heavy) children.
///
/// Default nice is 0. Values from 1 to 19 lower priority (higher niceness).
/// **10** is a clear step below the UI without the extreme lag of 19 under light
/// desktop load. Unprivileged processes may always *raise* their own niceness;
/// lowering it back requires privileges, which we never do from the host.
pub const ENGINE_CHILD_NICE: i32 = 10;

/// Intended Unix niceness for heavy children (pure helper for tests and docs).
#[inline]
pub fn engine_child_nice() -> i32 {
    ENGINE_CHILD_NICE
}

/// Whether the configured Unix nice is an elevated (lower-priority) value.
///
/// Contract: heavy children must request niceness strictly greater than 0 and
/// at most 19 (Linux/macOS portable range for positive nice).
#[inline]
pub fn engine_child_nice_is_elevated(nice: i32) -> bool {
    nice > 0 && nice <= 19
}

/// Windows process priority class flag: below-normal (not idle).
///
/// See [Process creation flags](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags).
#[cfg(windows)]
pub const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

/// Configure `cmd` so the **spawned** process runs at reduced scheduling priority.
///
/// Safe to call on every heavy-work spawn. Does not change the calling process.
pub fn apply_low_compute_priority(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: pre_exec runs once in the child after fork, before exec.
        // setpriority on self (who=0) is async-signal-safe on Linux and macOS.
        // Failure is ignored: a missing demotion must not prevent engine start.
        unsafe {
            cmd.pre_exec(|| {
                let _ = set_current_thread_nice(ENGINE_CHILD_NICE);
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(BELOW_NORMAL_PRIORITY_CLASS);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = cmd;
    }
}

/// Set niceness of the **calling thread** (Unix).
///
/// On Linux, `setpriority(PRIO_PROCESS, 0, nice)` applies to the calling
/// thread, not every thread in the process. Call this from a compute worker
/// (or from child `pre_exec`) so GPUI stays at default nice.
///
/// Also used from child `pre_exec` after fork: the child has one thread, so
/// the same syscall nices the process that `exec` will keep.
#[cfg(unix)]
pub fn set_current_thread_nice(nice: i32) -> std::io::Result<()> {
    // PRIO_PROCESS = 0, who = 0: Linux treats this as the calling thread.
    // See setpriority(2) (accessed: 2026-08-13).
    let rc = unsafe { libc_setpriority(0, 0, nice) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// No-op on non-Unix (Windows children use [`apply_low_compute_priority`]).
#[cfg(not(unix))]
pub fn set_current_thread_nice(nice: i32) -> std::io::Result<()> {
    let _ = nice;
    Ok(())
}

#[cfg(unix)]
unsafe extern "C" {
    fn setpriority(which: i32, who: u32, prio: i32) -> i32;
}

#[cfg(unix)]
#[inline]
unsafe fn libc_setpriority(which: i32, who: u32, prio: i32) -> i32 {
    unsafe { setpriority(which, who, prio) }
}

/// Read niceness of `pid` (Unix). `pid == 0` means the calling thread on Linux.
#[cfg(unix)]
pub fn get_process_nice(pid: u32) -> std::io::Result<i32> {
    // getpriority can return -1 as a valid nice; clear errno first.
    unsafe {
        *errno_location() = 0;
        let n = getpriority(0, pid);
        let err = *errno_location();
        if n == -1 && err != 0 {
            Err(std::io::Error::from_raw_os_error(err))
        } else {
            Ok(n)
        }
    }
}

#[cfg(unix)]
unsafe extern "C" {
    fn getpriority(which: i32, who: u32) -> i32;
}

#[cfg(all(unix, target_os = "linux"))]
unsafe fn errno_location() -> *mut i32 {
    unsafe extern "C" {
        fn __errno_location() -> *mut i32;
    }
    unsafe { __errno_location() }
}

#[cfg(all(
    unix,
    any(target_os = "macos", target_os = "ios", target_os = "freebsd")
))]
unsafe fn errno_location() -> *mut i32 {
    unsafe extern "C" {
        fn __error() -> *mut i32;
    }
    unsafe { __error() }
}

#[cfg(all(
    unix,
    not(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd"
    ))
))]
unsafe fn errno_location() -> *mut i32 {
    // Best-effort for other Unix: thread-local errno via libc convention.
    unsafe extern "C" {
        fn __errno_location() -> *mut i32;
    }
    unsafe { __errno_location() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_child_nice_is_elevated_constant() {
        let n = engine_child_nice();
        assert_eq!(n, ENGINE_CHILD_NICE);
        assert!(
            engine_child_nice_is_elevated(n),
            "engine child nice {n} must be in 1..=19 so it is lower priority than default 0"
        );
        // Documented choice: 10, not idle-starved 19.
        assert_eq!(
            n, 10,
            "ENGINE_CHILD_NICE should stay at 10 unless deliberately retuned"
        );
    }

    #[test]
    fn elevated_predicate_rejects_default_and_out_of_range() {
        assert!(!engine_child_nice_is_elevated(0));
        assert!(!engine_child_nice_is_elevated(-5));
        assert!(!engine_child_nice_is_elevated(20));
        assert!(engine_child_nice_is_elevated(1));
        assert!(engine_child_nice_is_elevated(19));
    }

    /// Nicing a worker thread must not change the caller's niceness (GPUI).
    ///
    /// On Linux, `setpriority(PRIO_PROCESS, 0, nice)` from a thread demotes
    /// that thread only. The UI / test caller must stay at its original nice.
    #[cfg(unix)]
    #[test]
    fn set_current_thread_nice_does_not_change_other_thread() {
        use std::thread;

        let caller_before = get_process_nice(0).expect("caller nice before");
        let worker_nice = thread::spawn(|| {
            set_current_thread_nice(ENGINE_CHILD_NICE).expect("worker set nice");
            get_process_nice(0).expect("worker get nice")
        })
        .join()
        .expect("worker join");
        let caller_after = get_process_nice(0).expect("caller nice after");
        assert_eq!(
            worker_nice, ENGINE_CHILD_NICE,
            "worker thread must be niced to ENGINE_CHILD_NICE"
        );
        assert_eq!(
            caller_before, caller_after,
            "set_current_thread_nice on a worker must not change the caller thread"
        );
    }

    /// OpenMP team hook nices libgomp workers from a compute thread, not GPUI.
    ///
    /// The hook must be invoked from a spawned thread: the calling thread is
    /// the OpenMP master and would be niced if we ran this on the test thread.
    #[cfg(all(unix, feature = "ffi"))]
    #[test]
    fn coli_nice_compute_threads_nices_openmp_team_not_caller() {
        use std::thread;

        let caller_before = get_process_nice(0).expect("caller nice before");
        let (team_ok, worker_nice) = thread::spawn(|| {
            let rc = crate::ffi::coli_nice_compute_threads(ENGINE_CHILD_NICE);
            assert_eq!(rc, 0, "coli_nice_compute_threads must succeed");
            let worker = get_process_nice(0).expect("worker nice after hook");
            let team_ok = crate::ffi::coli_openmp_team_all_at_nice(ENGINE_CHILD_NICE);
            (team_ok, worker)
        })
        .join()
        .expect("worker join");
        assert_eq!(
            worker_nice, ENGINE_CHILD_NICE,
            "OpenMP master (the worker) must be niced"
        );
        assert!(
            team_ok,
            "every OpenMP team member must be at ENGINE_CHILD_NICE"
        );
        let caller_after = get_process_nice(0).expect("caller nice after");
        assert_eq!(
            caller_before, caller_after,
            "OpenMP niceness hook must not change the caller thread (GPUI)"
        );
        assert_eq!(
            crate::ffi::COLI_COMPUTE_NICE,
            ENGINE_CHILD_NICE,
            "C COLI_COMPUTE_NICE must match ENGINE_CHILD_NICE"
        );
    }

    /// Applying low priority mutates only the Command; parent niceness unchanged.
    #[cfg(unix)]
    #[test]
    fn apply_does_not_demote_parent_process() {
        let before = get_process_nice(0).expect("parent nice before");
        let mut cmd = Command::new("true");
        apply_low_compute_priority(&mut cmd);
        let after = get_process_nice(0).expect("parent nice after apply");
        assert_eq!(
            before, after,
            "apply_low_compute_priority must not call setpriority on the UI/host process"
        );
    }

    /// Child process starts at elevated niceness (integration; needs spawn).
    #[cfg(unix)]
    #[test]
    fn spawned_child_has_elevated_nice() {
        use std::process::Stdio;

        // Parent reads getpriority(child_pid) after pre_exec demotion.
        let mut cmd = Command::new("sleep");
        cmd.arg("5")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        apply_low_compute_priority(&mut cmd);
        let mut child = cmd.spawn().expect("spawn sleep with low priority");
        let pid = child.id();
        let nice = get_process_nice(pid).expect("getpriority on child");
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            engine_child_nice_is_elevated(nice),
            "child nice was {nice}, expected elevated ({ENGINE_CHILD_NICE})"
        );
        assert_eq!(
            nice, ENGINE_CHILD_NICE,
            "child should match ENGINE_CHILD_NICE exactly when setpriority succeeds"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_below_normal_flag_is_non_zero() {
        assert_ne!(BELOW_NORMAL_PRIORITY_CLASS, 0);
        assert_eq!(BELOW_NORMAL_PRIORITY_CLASS, 0x4000);
    }
}
