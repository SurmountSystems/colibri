//! OS desktop notifications for terminal install / generate events.
//!
//! Uses [`notify_rust`] (FreeDesktop / macOS / Windows toast). Fail soft when
//! the notification daemon is unavailable. Title/body helpers are pure and
//! unit-tested without a desktop bus.
//!
//! Fire only on meaningful ends: install success once; generate Done / user
//! stop / error end once (not per token or progress tick).

use std::path::Path;

/// App name shown by the notification daemon (matches brand.name tone).
pub const APP_NAME: &str = "colibrì";

/// Why a generate stream ended (gating + copy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferenceEndKind {
    /// Natural completion ([`crate::host::GenEvent::Done`] without user stop).
    Finished,
    /// User pressed Stop; stream ended (Done or Error with stop flag).
    StoppedByUser,
    /// Error end without user stop.
    Error,
}

/// Title + body for one notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifyCopy {
    pub title: String,
    pub body: String,
}

// ---------------------------------------------------------------------------
// Gating
// ---------------------------------------------------------------------------

/// Install success always notifies once (caller fires only on Done).
pub fn should_notify_install_complete() -> bool {
    true
}

/// Whether this inference end should raise an OS notification.
///
/// Finished, user stop, and error each notify once. Progress tokens never
/// call this path.
pub fn should_notify_inference_end(kind: InferenceEndKind) -> bool {
    matches!(
        kind,
        InferenceEndKind::Finished | InferenceEndKind::StoppedByUser | InferenceEndKind::Error
    )
}

/// Map UI `stop_requested` + whether the channel frame was Done vs Error.
pub fn inference_end_kind(stop_requested: bool, is_error: bool) -> InferenceEndKind {
    if stop_requested {
        InferenceEndKind::StoppedByUser
    } else if is_error {
        InferenceEndKind::Error
    } else {
        InferenceEndKind::Finished
    }
}

// ---------------------------------------------------------------------------
// Copy builders (plain operational English; no marketing slogans)
// ---------------------------------------------------------------------------

/// Last path component for install dest, or full display if missing.
pub fn model_label_from_dest(dest: &Path) -> String {
    dest.file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| dest.display().to_string())
}

/// Copy for a successful model install.
pub fn install_complete_copy(dest: &Path) -> NotifyCopy {
    let name = model_label_from_dest(dest);
    NotifyCopy {
        title: "Model install complete".into(),
        body: format!("Download finished: {name}"),
    }
}

/// Copy for generate end. `error_summary` used only for [`InferenceEndKind::Error`].
pub fn inference_end_copy(
    kind: InferenceEndKind,
    completion_tokens: Option<u64>,
    tokens_per_second: Option<f32>,
    error_summary: Option<&str>,
) -> NotifyCopy {
    match kind {
        InferenceEndKind::Finished => {
            let body = match (completion_tokens, tokens_per_second) {
                (Some(t), Some(r)) => format!("Reply finished · {t} tok · {r:.2} tok/s"),
                (Some(t), None) => format!("Reply finished · {t} tok"),
                _ => "Reply finished".into(),
            };
            NotifyCopy {
                title: "Reply finished".into(),
                body,
            }
        }
        InferenceEndKind::StoppedByUser => NotifyCopy {
            title: "Generation stopped".into(),
            body: "Stopped by user".into(),
        },
        InferenceEndKind::Error => {
            let raw = error_summary.unwrap_or("generate failed");
            NotifyCopy {
                title: "Generation failed".into(),
                body: truncate_plain(raw, 160),
            }
        }
    }
}

/// Truncate for notification body length (char count, ASCII `...` suffix).
pub fn truncate_plain(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    if max_chars <= 3 {
        return s.chars().take(max_chars).collect();
    }
    let take = max_chars - 3;
    let head: String = s.chars().take(take).collect();
    format!("{head}...")
}

// ---------------------------------------------------------------------------
// Send (thin wrapper; fail soft)
// ---------------------------------------------------------------------------

/// Send one OS notification. Never panics; logs and returns on failure.
pub fn send_os_notification(title: &str, body: &str) {
    match notify_rust::Notification::new()
        .appname(APP_NAME)
        .summary(title)
        .body(body)
        .show()
    {
        Ok(_) => {}
        Err(e) => {
            tracing::warn!(
                target: "colibri_native",
                error = %e,
                "OS notification failed"
            );
        }
    }
}

/// Install Done → one system notification.
pub fn notify_install_complete(dest: &Path) {
    if !should_notify_install_complete() {
        return;
    }
    let copy = install_complete_copy(dest);
    send_os_notification(&copy.title, &copy.body);
}

/// Generate terminal event → one system notification when gated.
pub fn notify_inference_end(
    kind: InferenceEndKind,
    completion_tokens: Option<u64>,
    tokens_per_second: Option<f32>,
    error_summary: Option<&str>,
) {
    if !should_notify_inference_end(kind) {
        return;
    }
    let copy = inference_end_copy(kind, completion_tokens, tokens_per_second, error_summary);
    send_os_notification(&copy.title, &copy.body);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn install_complete_always_gated_on() {
        assert!(should_notify_install_complete());
    }

    #[test]
    fn inference_gating_covers_terminal_kinds() {
        assert!(should_notify_inference_end(InferenceEndKind::Finished));
        assert!(should_notify_inference_end(InferenceEndKind::StoppedByUser));
        assert!(should_notify_inference_end(InferenceEndKind::Error));
    }

    #[test]
    fn inference_end_kind_from_flags() {
        assert_eq!(inference_end_kind(false, false), InferenceEndKind::Finished);
        assert_eq!(
            inference_end_kind(true, false),
            InferenceEndKind::StoppedByUser
        );
        assert_eq!(
            inference_end_kind(true, true),
            InferenceEndKind::StoppedByUser
        );
        assert_eq!(inference_end_kind(false, true), InferenceEndKind::Error);
    }

    #[test]
    fn model_label_uses_file_name() {
        let p = PathBuf::from("/models/glm-5.2-colibri");
        assert_eq!(model_label_from_dest(&p), "glm-5.2-colibri");
        let bare = PathBuf::from("repo-name");
        assert_eq!(model_label_from_dest(&bare), "repo-name");
    }

    #[test]
    fn install_complete_copy_is_operational() {
        let p = PathBuf::from("/store/MyModel");
        let c = install_complete_copy(&p);
        assert_eq!(c.title, "Model install complete");
        assert_eq!(c.body, "Download finished: MyModel");
        // No invented marketing taglines.
        assert!(!c.title.contains('!'));
        assert!(!c.body.to_lowercase().contains("amazing"));
    }

    #[test]
    fn inference_finished_copy_includes_stats() {
        let c = inference_end_copy(InferenceEndKind::Finished, Some(42), Some(3.5), None);
        assert_eq!(c.title, "Reply finished");
        assert!(c.body.contains("42"), "{:?}", c.body);
        assert!(c.body.contains("3.50"), "{:?}", c.body);
    }

    #[test]
    fn inference_finished_without_stats() {
        let c = inference_end_copy(InferenceEndKind::Finished, None, None, None);
        assert_eq!(c.title, "Reply finished");
        assert_eq!(c.body, "Reply finished");
    }

    #[test]
    fn inference_stopped_copy() {
        let c = inference_end_copy(InferenceEndKind::StoppedByUser, Some(10), Some(1.0), None);
        assert_eq!(c.title, "Generation stopped");
        assert_eq!(c.body, "Stopped by user");
    }

    #[test]
    fn inference_error_copy_truncates() {
        let long = "x".repeat(200);
        let c = inference_end_copy(InferenceEndKind::Error, None, None, Some(&long));
        assert_eq!(c.title, "Generation failed");
        assert!(c.body.chars().count() <= 160);
        assert!(c.body.ends_with("..."));
    }

    #[test]
    fn inference_error_default_summary() {
        let c = inference_end_copy(InferenceEndKind::Error, None, None, None);
        assert_eq!(c.body, "generate failed");
    }

    #[test]
    fn truncate_plain_edges() {
        assert_eq!(truncate_plain("hi", 10), "hi");
        assert_eq!(truncate_plain("hello world", 8), "hello...");
        assert_eq!(truncate_plain("abc", 0), "");
        assert_eq!(truncate_plain("ab", 2), "ab");
        // Multi-byte: count chars not bytes.
        let s = "éééé";
        assert_eq!(truncate_plain(s, 2), "éé");
    }
}
