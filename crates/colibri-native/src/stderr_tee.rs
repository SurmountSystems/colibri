//! Host stderr tee so libc `fprintf` banners reach `native.log` and the chip.
//!
//! Install **before** tracing init: `pipe` + `dup2` onto fd 2, then a drain
//! thread that echoes every line to the saved TTY fd and appends only C
//! banners (`[` ...) to the rotating native log. Tracing already has its own
//! file layer; teeing every stderr byte would double-write Rust lines.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::log_init::append_native_log_line;
use crate::prefill::{parse_prefill_line, store_prefill_progress};

/// C engine banners start with `[` (`[prefill]`, `[stop]`, …).
pub fn is_c_banner_line(line: &str) -> bool {
    line.trim_start().starts_with('[')
}

/// Parse a drained stderr line: file-append C banners only; store `[prefill]`.
pub fn handle_stderr_line(line: &str, log_path: Option<&Path>) {
    if is_c_banner_line(line) {
        if let Some(path) = log_path {
            let _ = append_native_log_line(path, line);
        }
    }
    if let Some(tick) = parse_prefill_line(line) {
        store_prefill_progress(tick);
    }
}

/// Remap host fd 2 through a line drain. Fail-soft if the pipe cannot be set up.
pub fn install_host_stderr_tee() {
    #[cfg(unix)]
    install_unix();
}

#[cfg(unix)]
fn install_unix() {
    let log_path = if colibri_sys::native_log_enabled() {
        Some(colibri_sys::default_native_log_path())
    } else {
        None
    };

    let mut fds = [0i32; 2];
    // SAFETY: `fds` is a two-int buffer as `pipe(2)` requires.
    if unsafe { pipe(fds.as_mut_ptr()) } != 0 {
        return;
    }
    let (read_fd, write_fd) = (fds[0], fds[1]);
    // SAFETY: dup the current stderr so the drain can echo to the real TTY.
    let saved = unsafe { dup(2) };
    if saved < 0 {
        unsafe {
            close(read_fd);
            close(write_fd);
        }
        return;
    }
    unsafe {
        fcntl(read_fd, F_SETFD, FD_CLOEXEC);
        fcntl(saved, F_SETFD, FD_CLOEXEC);
    }

    let spawn = std::thread::Builder::new()
        .name("colibri-stderr-tee".into())
        .spawn(move || drain_tee(read_fd, saved, log_path));
    if spawn.is_err() {
        unsafe {
            close(read_fd);
            close(write_fd);
            close(saved);
        }
        return;
    }

    // SAFETY: put the pipe write end on fd 2; libc fprintf and tracing stderr
    // both follow that fd. The extra write_fd copy is then closed.
    if unsafe { dup2(write_fd, 2) } < 0 {
        unsafe {
            close(write_fd);
        }
        return;
    }
    unsafe {
        close(write_fd);
    }
}

#[cfg(unix)]
fn drain_tee(read_fd: i32, echo_fd: i32, log_path: Option<PathBuf>) {
    use std::fs::File;
    use std::os::fd::FromRawFd;

    // SAFETY: exclusive ownership of the pipe read end and the saved TTY fd.
    let reader = unsafe { File::from_raw_fd(read_fd) };
    let mut echo = unsafe { File::from_raw_fd(echo_fd) };
    let mut buf = BufReader::new(reader);
    let mut line = String::new();
    loop {
        line.clear();
        match buf.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let _ = echo.write_all(line.as_bytes());
                let _ = echo.flush();
                let trimmed = line.trim_end_matches(['\n', '\r']);
                handle_stderr_line(trimmed, log_path.as_deref());
            }
            Err(_) => break,
        }
    }
}

#[cfg(unix)]
const F_SETFD: i32 = 2;
#[cfg(unix)]
const FD_CLOEXEC: i32 = 1;

#[cfg(unix)]
unsafe extern "C" {
    fn pipe(pipefd: *mut i32) -> i32;
    fn dup(fd: i32) -> i32;
    fn dup2(oldfd: i32, newfd: i32) -> i32;
    fn close(fd: i32) -> i32;
    fn fcntl(fd: i32, cmd: i32, arg: i32) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefill::{clear_prefill_progress, load_prefill_progress};

    #[test]
    fn handle_stderr_line_appends_c_banners_only() {
        clear_prefill_progress();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("native.log");
        handle_stderr_line("INFO colibri_native: generate begin", Some(&path));
        handle_stderr_line("[stop] 18 stop tokens: 1 2 3", Some(&path));
        handle_stderr_line("[prefill] layer 13/78 · 47 token · +21.80s", Some(&path));
        let body = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            !body.contains("generate begin"),
            "tracing-shaped lines must not be double-written: {body}"
        );
        assert!(body.contains("[stop] 18 stop tokens"), "{body}");
        assert!(body.contains("[prefill] layer 13/78"), "{body}");
        let snap = load_prefill_progress().expect("prefill tick stored");
        assert_eq!(snap.layer, 13);
        assert_eq!(snap.total, 78);
        assert_eq!(snap.tokens, 47);
        clear_prefill_progress();
    }
}
