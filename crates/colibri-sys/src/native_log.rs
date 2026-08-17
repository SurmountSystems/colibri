//! Native / sys log policy (paths live in [`crate::paths`]).
//!
//! Default is **on** at `info` for `colibri_native` and `colibri_sys`.
//! `COLIBRI_LOG=off` or `0` skips the file. `RUST_LOG` overrides the filter.

use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

/// Env key: `off` / `0` (and `false` / `no`) skip native file logging.
pub const COLIBRI_LOG_ENV: &str = "COLIBRI_LOG";

/// Default tracing filter when `RUST_LOG` is unset.
pub const DEFAULT_NATIVE_LOG_FILTER: &str = "colibri_native=info,colibri_sys=info";

/// Whether native file logging should run (`COLIBRI_LOG` process env).
pub fn native_log_enabled() -> bool {
    native_log_enabled_from(std::env::var(COLIBRI_LOG_ENV).ok().as_deref())
}

/// Parse `COLIBRI_LOG`. Unset / empty → on. `off` / `0` / `false` / `no` → off.
pub fn native_log_enabled_from(colibri_log: Option<&str>) -> bool {
    match colibri_log.map(str::trim) {
        None | Some("") => true,
        Some(v) => !matches!(
            v.to_ascii_lowercase().as_str(),
            "off" | "0" | "false" | "no"
        ),
    }
}

/// Filter spec: `RUST_LOG` when non-empty, else [`DEFAULT_NATIVE_LOG_FILTER`].
pub fn native_log_filter_from(rust_log: Option<&str>) -> String {
    match rust_log.map(str::trim) {
        Some(v) if !v.is_empty() => v.to_string(),
        _ => DEFAULT_NATIVE_LOG_FILTER.to_string(),
    }
}

fn secret_token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)(hf_[A-Za-z0-9]{8,}|sk-[A-Za-z0-9_\-]{8,})").expect("token regex")
    })
}

fn secret_assignment_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b((?:HF_TOKEN|HUGGING_FACE_HUB_TOKEN|OPENAI_API_KEY|API_KEY|COLIBRI_API_KEY)\s*=\s*)\S+",
        )
        .expect("assignment regex")
    })
}

/// Redact common secret tokens so a log line never stores them.
///
/// Does not claim to catch every secret. Callers must still not pass prompts,
/// generate tokens, HF tokens, or API keys into tracing fields.
pub fn sanitize_log_text(s: &str) -> String {
    let without_tokens = secret_token_re().replace_all(s, "[redacted]");
    secret_assignment_re()
        .replace_all(&without_tokens, "${1}[redacted]")
        .into_owned()
}

/// Link flavor for this binary: `cpu`, `HIP`, or `CUDA`.
///
/// Matches [`crate::archive_gpu_flavor::GpuArchiveFlavor`] Display names.
/// Uses compile-time Cargo features (what this process is linked as), not a
/// live archive scan.
pub fn linked_engine_flavor() -> &'static str {
    if cfg!(feature = "ffi-hip") {
        "HIP"
    } else if cfg!(feature = "ffi-cuda") {
        "CUDA"
    } else {
        "cpu"
    }
}

/// RSS / swap sample parsed from `/proc/self/status` (or a fixture blob).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProcStatusVm {
    pub rss_kb: u64,
    pub vmswap_kb: u64,
}

/// Parse `VmRSS` / `VmSwap` kilobyte fields. Missing swap is 0. Does not panic.
pub fn parse_proc_status_vm(text: &str) -> ProcStatusVm {
    let mut out = ProcStatusVm::default();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            out.rss_kb = parse_status_kb(rest);
        } else if let Some(rest) = line.strip_prefix("VmSwap:") {
            out.vmswap_kb = parse_status_kb(rest);
        }
    }
    out
}

fn parse_status_kb(rest: &str) -> u64 {
    rest.split_whitespace()
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

/// Last path component of a `/proc/self/cgroup` blob (oomd names cgroups).
pub fn cgroup_leaf(text: &str) -> &str {
    let mut chosen = "";
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let path = line.rsplit(':').next().unwrap_or(line);
        if line.starts_with("0::") || chosen.is_empty() {
            chosen = path;
        }
    }
    chosen.rsplit('/').find(|s| !s.is_empty()).unwrap_or(chosen)
}

/// Identity fields for heartbeat / start lines. No prompts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionIdentity {
    pub pid: u32,
    pub comm: String,
    pub cgroup_leaf: String,
    pub flavor: String,
    pub rss_kb: u64,
    pub vmswap_kb: u64,
}

/// Read pid / comm / cgroup / RSS / swap / flavor from this process.
pub fn session_identity_now() -> SessionIdentity {
    let comm = read_proc_trimmed("/proc/self/comm").unwrap_or_else(|| "unknown".into());
    let cgroup = std::fs::read_to_string("/proc/self/cgroup").unwrap_or_default();
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let vm = parse_proc_status_vm(&status);
    SessionIdentity {
        pid: std::process::id(),
        comm,
        cgroup_leaf: cgroup_leaf(&cgroup).to_string(),
        flavor: linked_engine_flavor().to_string(),
        rss_kb: vm.rss_kb,
        vmswap_kb: vm.vmswap_kb,
    }
}

fn read_proc_trimmed(path: &str) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn append_identity_fields(line: &mut String, ident: &SessionIdentity) {
    line.push_str(&format!(" pid={}", ident.pid));
    if !ident.comm.is_empty() {
        line.push_str(" comm=");
        line.push_str(&ident.comm);
    }
    if !ident.cgroup_leaf.is_empty() {
        line.push_str(" cgroup=");
        line.push_str(&ident.cgroup_leaf);
    }
    line.push_str(" flavor=");
    line.push_str(&ident.flavor);
    line.push_str(&format!(
        " rss_kb={} vmswap_kb={}",
        ident.rss_kb, ident.vmswap_kb
    ));
}

/// One short heartbeat line. No prompts.
pub fn format_session_heartbeat_line(kind: Option<&str>, ident: &SessionIdentity) -> String {
    let mut line = format!(
        "heartbeat pid={} comm={} cgroup={} flavor={} rss_kb={} vmswap_kb={}",
        ident.pid, ident.comm, ident.cgroup_leaf, ident.flavor, ident.rss_kb, ident.vmswap_kb
    );
    if let Some(k) = kind {
        line.push_str(" kind=");
        line.push_str(k);
    }
    line
}

/// Operational start-line formatter (model path + kind + elapsed). No prompts.
pub fn format_engine_start_log(
    phase: &str,
    model: &Path,
    kind: Option<&str>,
    elapsed_ms: Option<u64>,
    error: Option<&str>,
) -> String {
    let mut line = format!("engine start {phase} model={}", model.display());
    if let Some(k) = kind {
        line.push_str(" kind=");
        line.push_str(k);
    }
    append_identity_fields(&mut line, &session_identity_now());
    if let Some(ms) = elapsed_ms {
        line.push_str(&format!(" elapsed_ms={ms}"));
    }
    if let Some(e) = error {
        line.push_str(" error=");
        line.push_str(&sanitize_log_text(e));
    }
    line
}

/// Operational generate-line formatter (kind + req + elapsed). No prompt, no tokens.
pub fn format_generate_log(
    phase: &str,
    kind: Option<&str>,
    req_id: Option<u64>,
    elapsed_ms: Option<u64>,
    error: Option<&str>,
) -> String {
    let mut line = format!("generate {phase}");
    if let Some(k) = kind {
        line.push_str(" kind=");
        line.push_str(k);
    }
    append_identity_fields(&mut line, &session_identity_now());
    if let Some(id) = req_id {
        line.push_str(&format!(" req_id={id}"));
    }
    if let Some(ms) = elapsed_ms {
        line.push_str(&format!(" elapsed_ms={ms}"));
    }
    if let Some(e) = error {
        line.push_str(" error=");
        line.push_str(&sanitize_log_text(e));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn native_log_enabled_default_on() {
        assert!(native_log_enabled_from(None));
        assert!(native_log_enabled_from(Some("")));
        assert!(native_log_enabled_from(Some("1")));
        assert!(native_log_enabled_from(Some("info")));
    }

    #[test]
    fn native_log_disabled_for_off_and_zero() {
        assert!(!native_log_enabled_from(Some("off")));
        assert!(!native_log_enabled_from(Some("OFF")));
        assert!(!native_log_enabled_from(Some("0")));
        assert!(!native_log_enabled_from(Some("false")));
        assert!(!native_log_enabled_from(Some("no")));
    }

    #[test]
    fn native_log_filter_default_is_native_and_sys_info() {
        assert_eq!(native_log_filter_from(None), DEFAULT_NATIVE_LOG_FILTER);
        assert_eq!(native_log_filter_from(Some("")), DEFAULT_NATIVE_LOG_FILTER);
        assert!(
            native_log_filter_from(None).contains("colibri_native=info"),
            "{}",
            native_log_filter_from(None)
        );
        assert!(
            native_log_filter_from(None).contains("colibri_sys=info"),
            "{}",
            native_log_filter_from(None)
        );
    }

    #[test]
    fn native_log_filter_honors_rust_log() {
        assert_eq!(
            native_log_filter_from(Some("colibri_native=debug")),
            "colibri_native=debug"
        );
        assert_eq!(native_log_filter_from(Some("debug")), "debug");
    }

    #[test]
    fn sanitize_log_text_redacts_hf_token_and_api_key() {
        let raw = "model=/home/user/.models/glm hf_AbCdEfGhIjKlMnOpQrStUvWx token=sk-SECRETKEYVALUE123 HF_TOKEN=hf_AbCdEfGhIjKlMnOpQrStUvWx";
        let clean = sanitize_log_text(raw);
        assert!(
            clean.contains("/home/user/.models/glm"),
            "model path must remain: {clean}"
        );
        assert!(
            !clean.contains("hf_AbCdEfGhIjKlMnOpQrStUvWx"),
            "HF token must not remain: {clean}"
        );
        assert!(
            !clean.contains("sk-SECRETKEYVALUE123"),
            "API key must not remain: {clean}"
        );
        assert!(
            !clean.contains("HF_TOKEN=hf_"),
            "HF_TOKEN assignment must be redacted: {clean}"
        );
    }

    #[test]
    fn engine_start_log_line_has_path_not_secrets() {
        let line = format_engine_start_log(
            "begin",
            Path::new("/home/user/.models/glm-5.2"),
            Some("ffi"),
            Some(12),
            None,
        );
        assert!(line.contains("/home/user/.models/glm-5.2"), "{line}");
        assert!(line.contains("begin") || line.contains("start"), "{line}");
        let lower = line.to_ascii_lowercase();
        assert!(!lower.contains("prompt="), "{line}");
        assert!(!line.contains("hf_"), "{line}");
        assert!(!lower.contains("api_key"), "{line}");
        assert!(!line.contains("SECRET"), "{line}");
    }

    #[test]
    fn engine_start_log_includes_pid_kind_and_flavor() {
        let line = format_engine_start_log(
            "begin",
            Path::new("/models/glm"),
            Some("ffi"),
            Some(12),
            None,
        );
        let pid = std::process::id();
        assert!(
            line.contains(&format!("pid={pid}")),
            "start line must include pid: {line}"
        );
        assert!(line.contains("kind=ffi"), "{line}");
        assert!(
            line.contains("flavor=cpu")
                || line.contains("flavor=HIP")
                || line.contains("flavor=CUDA"),
            "start line must include link flavor: {line}"
        );
        assert!(
            line.contains("rss_kb="),
            "start line must include rss: {line}"
        );
        assert!(
            line.contains("vmswap_kb="),
            "start line must include swap: {line}"
        );
        let lower = line.to_ascii_lowercase();
        assert!(!lower.contains("prompt="), "{line}");
    }

    #[test]
    fn generate_log_includes_pid_kind_and_flavor() {
        let line = format_generate_log("begin", Some("ffi"), Some(3), None, None);
        let pid = std::process::id();
        assert!(
            line.contains(&format!("pid={pid}")),
            "generate line must include pid: {line}"
        );
        assert!(line.contains("kind=ffi"), "{line}");
        assert!(
            line.contains("flavor=cpu")
                || line.contains("flavor=HIP")
                || line.contains("flavor=CUDA"),
            "generate line must include link flavor: {line}"
        );
        assert!(line.contains("rss_kb="), "{line}");
        assert!(line.contains("vmswap_kb="), "{line}");
        let lower = line.to_ascii_lowercase();
        assert!(!lower.contains("prompt="), "{line}");
    }

    #[test]
    fn parse_proc_status_vm_reads_fixture_rss_and_swap() {
        let blob = "\
Name:\tcolibri-native
Pid:\t14969
VmRSS:\t  761856 kB
VmSwap:\t 108544 kB
";
        let parsed = parse_proc_status_vm(blob);
        assert_eq!(parsed.rss_kb, 761856, "{parsed:?}");
        assert_eq!(parsed.vmswap_kb, 108544, "{parsed:?}");
    }

    #[test]
    fn parse_proc_status_vm_missing_swap_is_zero() {
        let blob = "Name:\tfoo\nVmRSS:\t10 kB\n";
        let parsed = parse_proc_status_vm(blob);
        assert_eq!(parsed.rss_kb, 10);
        assert_eq!(parsed.vmswap_kb, 0);
    }

    #[test]
    fn cgroup_leaf_takes_last_path_component() {
        let blob = "0::/user.slice/user-1000.slice/session.slice/app-gnome-Alacritty-14969.scope\n";
        assert_eq!(cgroup_leaf(blob), "app-gnome-Alacritty-14969.scope");
    }

    #[test]
    fn heartbeat_line_includes_pid_flavor_rss() {
        let ident = SessionIdentity {
            pid: 14969,
            comm: "colibri-native".into(),
            cgroup_leaf: "app-gnome-Alacritty-14969.scope".into(),
            flavor: "cpu".into(),
            rss_kb: 761856,
            vmswap_kb: 108544,
        };
        let line = format_session_heartbeat_line(Some("ffi"), &ident);
        assert!(line.contains("heartbeat"), "{line}");
        assert!(line.contains("pid=14969"), "{line}");
        assert!(line.contains("comm=colibri-native"), "{line}");
        assert!(
            line.contains("cgroup=app-gnome-Alacritty-14969.scope"),
            "{line}"
        );
        assert!(line.contains("flavor=cpu"), "{line}");
        assert!(line.contains("kind=ffi"), "{line}");
        assert!(line.contains("rss_kb=761856"), "{line}");
        assert!(line.contains("vmswap_kb=108544"), "{line}");
        let lower = line.to_ascii_lowercase();
        assert!(!lower.contains("prompt="), "{line}");
    }

    #[test]
    fn generate_log_line_has_kind_not_prompt_or_tokens() {
        let begin = format_generate_log("begin", Some("ffi"), Some(3), None, None);
        assert!(begin.contains("generate begin"), "{begin}");
        assert!(begin.contains("kind=ffi"), "{begin}");
        assert!(begin.contains("req_id=3"), "{begin}");
        let lower = begin.to_ascii_lowercase();
        assert!(!lower.contains("prompt="), "{begin}");
        assert!(!lower.contains("token="), "{begin}");

        let end = format_generate_log(
            "end",
            Some("ffi"),
            Some(3),
            Some(1200),
            Some("stopped HF_TOKEN=hf_AbCdEfGhIjKlMnOp"),
        );
        assert!(end.contains("generate end"), "{end}");
        assert!(end.contains("elapsed_ms=1200"), "{end}");
        assert!(end.contains("[redacted]"), "{end}");
        assert!(!end.contains("hf_AbCdEfGhIjKlMnOp"), "{end}");
        let lower = end.to_ascii_lowercase();
        assert!(!lower.contains("prompt="), "{end}");
    }
}
