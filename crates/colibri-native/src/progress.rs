//! Determinate progress helpers (percent + ETA) for install and generate.
//!
//! Pure math only: no GPUI. The native shell paints a thick fill row + line
//! from [`ProgressView`] during install and generate.
//!
//! **Honesty:** when percent or ETA is unknown or untrustworthy, use `None`
//! and omit it from the status line. Never invent `0%` or multi-day ETAs.

/// Snapshot for a progress strip: optional filled fraction, optional time left,
/// short label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressView {
    /// Trustworthy 0..=100 when known; `None` when total unknown or no work
    /// has landed yet (do not paint or print a fake 0%).
    pub percent: Option<u8>,
    /// Estimated seconds remaining; `None` when rate is unknown, non-positive,
    /// absurd, or no work has landed yet. Omit entirely from the line (no
    /// "Calculating..." filler).
    pub eta_secs: Option<u64>,
    /// Short status phrase (caller-owned plain English), e.g. "Downloading...".
    pub label: String,
}

impl ProgressView {
    pub fn new(percent: Option<u8>, eta_secs: Option<u64>, label: impl Into<String>) -> Self {
        Self {
            percent: percent.map(|p| p.min(100)),
            eta_secs,
            label: label.into(),
        }
    }

    /// One-line copy: label, optional `N%`, optional ETA. Omits unknown pieces.
    pub fn line(&self) -> String {
        format_progress_line(&self.label, self.percent, self.eta_secs)
    }

    /// Filled track fraction for paint; `0.0` (empty track) when percent is
    /// `None`. Same number as the label percent when `Some`.
    pub fn fill_fraction(&self) -> f32 {
        fill_fraction(self.percent)
    }
}

/// Track fill width as a fraction of parent width (0.0..=1.0).
///
/// - `None` → `0.0` (empty / indeterminate track; do not invent a fill)
/// - `Some(p)` → same capped percent as the status line (`p/100`)
pub fn fill_fraction(percent: Option<u8>) -> f32 {
    match percent {
        None => 0.0,
        Some(p) => f32::from(p.min(100)) / 100.0,
    }
}

/// Percent complete: `done / total` as 0..=100.
///
/// - `total == 0` → `0`
/// - `done > total` → `100` (capped)
///
/// Prefer [`install_percent`] for install UI: that returns `Option` and omits
/// untrustworthy zeros.
pub fn percent_done(done: u64, total: u64) -> u8 {
    if total == 0 {
        return 0;
    }
    let pct = (done as u128)
        .saturating_mul(100)
        .checked_div(total as u128)
        .unwrap_or(0);
    pct.min(100) as u8
}

/// Trustworthy install/download percent, or `None` when it would be nonsense.
///
/// - `total == 0` → `None`
/// - `done == 0` → `None` (no work landed; do not show `0%`)
/// - `done >= total` → `Some(100)`
/// - else → `Some(percent)` with a floor of **1** once any work has landed so
///   integer rounding never prints `0%` mid-transfer
pub fn install_percent(done: u64, total: u64) -> Option<u8> {
    if total == 0 {
        return None;
    }
    if done == 0 {
        return None;
    }
    if done >= total {
        return Some(100);
    }
    Some(percent_done(done, total).max(1))
}

/// Longest ETA we surface in the UI (7 days).
///
/// Beyond this, rates from early / unstable samples produce multi-week noise
/// (e.g. "about 1445 hours left"). Hide as unknown instead.
pub const MAX_ETA_SECS: u64 = 7 * 24 * 3600;

/// Seconds remaining given `remaining` work units and `rate_per_sec` (units/sec).
///
/// Returns `None` when rate is not positive or not finite, or when the estimate
/// exceeds [`MAX_ETA_SECS`] (absurd / unreliable). Zero remaining → `Some(0)`.
pub fn eta_secs(remaining: u64, rate_per_sec: f64) -> Option<u64> {
    if !rate_per_sec.is_finite() || rate_per_sec <= 0.0 {
        return None;
    }
    if remaining == 0 {
        return Some(0);
    }
    let secs = (remaining as f64 / rate_per_sec).ceil();
    if !secs.is_finite() || secs < 0.0 {
        return None;
    }
    // Cap to u64::MAX without panicking on huge f64.
    let secs_u = if secs >= u64::MAX as f64 {
        u64::MAX
    } else {
        secs as u64
    };
    if secs_u > MAX_ETA_SECS {
        return None;
    }
    Some(secs_u)
}

/// Plain-English time-left phrase, or `None` when ETA is unknown.
///
/// - `None` input → `None` (caller omits; no "Calculating..." filler)
/// - under 60s → `"about Ns left"`
/// - under 1 hour → `"about N min left"` (at least 1 min when ≥ 60s)
/// - under 48 hours → `"about N hour(s) left"`
/// - else → `"about N days left"` (still within [`MAX_ETA_SECS`])
pub fn format_eta(secs: Option<u64>) -> Option<String> {
    let s = secs?;
    if s < 60 {
        return Some(format!("about {s}s left"));
    }
    if s < 3600 {
        let mins = (s / 60).max(1);
        return Some(format!("about {mins} min left"));
    }
    if s < 48 * 3600 {
        let hours = (s / 3600).max(1);
        if hours == 1 {
            return Some("about 1 hour left".into());
        }
        return Some(format!("about {hours} hours left"));
    }
    let days = (s / (24 * 3600)).max(2);
    Some(format!("about {days} days left"))
}

/// Status line: `status`, optional ` N%`, optional ` · {eta}`.
///
/// Unknown percent and unknown ETA are **omitted** (never `0%` filler, never
/// "Calculating...").
///
/// Examples:
/// - `(..., None, None)` → `"Downloading..."`
/// - `(..., Some(42), None)` → `"Downloading... 42%"`
/// - `(..., Some(42), Some(60))` → `"Downloading... 42% · about 1 min left"`
pub fn format_progress_line(status: &str, percent: Option<u8>, eta: Option<u64>) -> String {
    let mut out = status.to_string();
    if let Some(p) = percent {
        out.push_str(&format!(" {}%", p.min(100)));
    }
    if let Some(eta_text) = format_eta(eta) {
        out.push_str(" · ");
        out.push_str(&eta_text);
    }
    out
}

/// Footer / chrome status during install: omit `N%` when percent is unknown.
///
/// - file + percent → `"Installing · {file} · {n}%"`
/// - file only → `"Installing · {file}"`
/// - percent only → `"Installing · {n}%"`
/// - neither → `"Installing"`
pub fn format_install_chrome_status(file: Option<&str>, percent: Option<u8>) -> String {
    match (file, percent) {
        (Some(f), Some(p)) => format!("Installing · {f} · {}%", p.min(100)),
        (Some(f), None) => format!("Installing · {f}"),
        (None, Some(p)) => format!("Installing · {}%", p.min(100)),
        (None, None) => "Installing".into(),
    }
}

/// Generate / inference progress against configured max output tokens.
///
/// Percent is `generated / max_output` (capped). ETA uses remaining tokens / tok/s.
/// When `tok_per_sec <= 0`, ETA is `None` (omitted from the line).
/// When `max_output == 0`, percent is `None` (no trustworthy fraction).
pub fn generate_progress(
    generated: u32,
    max_output: u32,
    tok_per_sec: f64,
) -> (Option<u8>, Option<u64>) {
    if max_output == 0 {
        // No configured max → fraction is untrustworthy.
        return (None, None);
    }
    // max_output > 0: token counts are trustworthy even at 0 generated.
    let percent = Some(percent_done(generated as u64, max_output as u64));
    let remaining = max_output.saturating_sub(generated) as u64;
    let eta = eta_secs(remaining, tok_per_sec);
    (percent, eta)
}

/// Install / download progress from optional byte and file counters.
///
/// Prefer **bytes** when both `bytes_done` and a positive `bytes_total` are known
/// (even if files are also known). Else use **files** when both file counters
/// are known with a positive total. Else percent `None` and no ETA.
///
/// `rate_per_sec` is in the unit of the chosen dimension (bytes/s or files/s).
/// When neither dimension yields a total, rate is ignored.
///
/// **Honesty rules:**
/// - No positive total → percent `None`, ETA `None` (not `0%`).
/// - `done == 0` with work remaining → percent `None`, ETA `None`.
/// - Estimates over [`MAX_ETA_SECS`] → ETA `None` (see [`eta_secs`]).
pub fn install_progress(
    bytes_done: Option<u64>,
    bytes_total: Option<u64>,
    files_done: Option<u32>,
    files_total: Option<u32>,
    rate_per_sec: f64,
) -> (Option<u8>, Option<u64>) {
    if let (Some(done), Some(total)) = (bytes_done, bytes_total) {
        if total > 0 {
            let percent = install_percent(done, total);
            let remaining = total.saturating_sub(done);
            let eta = install_eta_for_counters(done, remaining, rate_per_sec);
            return (percent, eta);
        }
    }
    if let (Some(done), Some(total)) = (files_done, files_total) {
        if total > 0 {
            let percent = install_percent(done as u64, total as u64);
            let remaining = (total as u64).saturating_sub(done as u64);
            let eta = install_eta_for_counters(done as u64, remaining, rate_per_sec);
            return (percent, eta);
        }
    }
    (None, None)
}

/// ETA for install counters: suppress when no work has landed yet.
fn install_eta_for_counters(done: u64, remaining: u64, rate_per_sec: f64) -> Option<u64> {
    if remaining == 0 {
        // Complete: zero remaining is a real ETA only when work finished.
        return if done > 0 { Some(0) } else { None };
    }
    // Zero completed → any positive rate is unreliable (or caller passed junk).
    if done == 0 {
        return None;
    }
    eta_secs(remaining, rate_per_sec)
}

/// Aggregate multi-shard download bytes: completed prior files + partial current.
///
/// Pure helper for hosts / tests. Does not clamp to total (caller may not know it).
/// Used by install UI math tests and available for hosts that stitch partials.
#[allow(dead_code)] // exercised in unit tests; host uses sys aggregate + live snapshot
pub fn aggregate_download_bytes(completed_prior: u64, current_file_partial: u64) -> u64 {
    completed_prior.saturating_add(current_file_partial)
}

/// Build an install progress snapshot from multi-file counters + optional partial.
///
/// `completed_prior_bytes` = sum of finished files; `current_partial` = bytes of
/// the in-flight file (0 if unknown). Prefer this over raw file-boundary totals
/// so the bar advances during a multi-GB first shard.
#[allow(dead_code)] // pure contract tests; live path uses InstallProgress + install_progress
pub fn install_progress_with_partial(
    completed_prior_bytes: u64,
    current_partial: u64,
    bytes_total: Option<u64>,
    files_done: Option<u32>,
    files_total: Option<u32>,
    rate_per_sec: f64,
) -> (Option<u8>, Option<u64>) {
    let bytes_done = aggregate_download_bytes(completed_prior_bytes, current_partial);
    install_progress(
        Some(bytes_done),
        bytes_total,
        files_done,
        files_total,
        rate_per_sec,
    )
}

/// Build a [`ProgressView`] for install using the same preference rules as
/// [`install_progress`].
pub fn install_progress_view(
    status: impl Into<String>,
    bytes_done: Option<u64>,
    bytes_total: Option<u64>,
    files_done: Option<u32>,
    files_total: Option<u32>,
    rate_per_sec: f64,
) -> ProgressView {
    let (percent, eta) = install_progress(
        bytes_done,
        bytes_total,
        files_done,
        files_total,
        rate_per_sec,
    );
    ProgressView::new(percent, eta, status)
}

/// Build a [`ProgressView`] for generate.
pub fn generate_progress_view(
    status: impl Into<String>,
    generated: u32,
    max_output: u32,
    tok_per_sec: f64,
) -> ProgressView {
    let (percent, eta) = generate_progress(generated, max_output, tok_per_sec);
    ProgressView::new(percent, eta, status)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- percent_done -------------------------------------------------------

    #[test]
    fn percent_zero_total_is_zero() {
        assert_eq!(percent_done(0, 0), 0);
        assert_eq!(percent_done(50, 0), 0);
        assert_eq!(percent_done(u64::MAX, 0), 0);
    }

    #[test]
    fn percent_zero_done() {
        assert_eq!(percent_done(0, 100), 0);
        assert_eq!(percent_done(0, 1), 0);
    }

    #[test]
    fn percent_exact_half_and_full() {
        assert_eq!(percent_done(50, 100), 50);
        assert_eq!(percent_done(100, 100), 100);
        assert_eq!(percent_done(1, 1), 100);
        assert_eq!(percent_done(3, 4), 75);
    }

    #[test]
    fn percent_done_exceeds_total_caps_at_100() {
        assert_eq!(percent_done(150, 100), 100);
        assert_eq!(percent_done(u64::MAX, 1), 100);
    }

    #[test]
    fn percent_large_values_no_overflow() {
        // Would overflow u64 if done * 100 without widening.
        let done = u64::MAX / 2;
        let total = u64::MAX;
        let p = percent_done(done, total);
        assert!(p <= 100);
        // roughly 50%
        assert!((49..=50).contains(&p), "p={p}");
    }

    // --- install_percent (Option honesty) -----------------------------------

    #[test]
    fn install_percent_none_when_total_zero_or_done_zero() {
        assert_eq!(install_percent(0, 0), None);
        assert_eq!(install_percent(10, 0), None);
        assert_eq!(install_percent(0, 100), None);
        assert_eq!(install_percent(0, 1), None);
    }

    #[test]
    fn install_percent_midway_and_full() {
        assert_eq!(install_percent(50, 100), Some(50));
        assert_eq!(install_percent(100, 100), Some(100));
        assert_eq!(install_percent(150, 100), Some(100));
    }

    #[test]
    fn install_percent_tiny_done_never_shows_zero() {
        // 1 byte of 1 GiB would integer-divide to 0%; honesty floors to 1%.
        assert_eq!(install_percent(1, 1024 * 1024 * 1024), Some(1));
    }

    // --- eta_secs -----------------------------------------------------------

    #[test]
    fn eta_none_when_rate_non_positive() {
        assert_eq!(eta_secs(100, 0.0), None);
        assert_eq!(eta_secs(100, -1.0), None);
        assert_eq!(eta_secs(0, 0.0), None);
        assert_eq!(eta_secs(10, f64::NEG_INFINITY), None);
    }

    #[test]
    fn eta_none_when_rate_not_finite() {
        assert_eq!(eta_secs(10, f64::NAN), None);
        assert_eq!(eta_secs(10, f64::INFINITY), None);
    }

    #[test]
    fn eta_zero_remaining_with_positive_rate() {
        assert_eq!(eta_secs(0, 5.0), Some(0));
        assert_eq!(eta_secs(0, 0.001), Some(0));
    }

    #[test]
    fn eta_basic_division_ceils() {
        // 10 units at 3/s → 3.333... → ceil 4
        assert_eq!(eta_secs(10, 3.0), Some(4));
        assert_eq!(eta_secs(100, 10.0), Some(10));
        assert_eq!(eta_secs(1, 2.0), Some(1)); // 0.5 → 1
    }

    #[test]
    fn eta_tiny_rate_above_max_is_none() {
        // 1e6 units at 1e-9 /s → absurd multi-year estimate → hide.
        assert_eq!(eta_secs(1_000_000, 1e-9), None);
        // Just inside the cap still returns a finite value.
        let e = eta_secs(MAX_ETA_SECS, 1.0);
        assert_eq!(e, Some(MAX_ETA_SECS));
        assert_eq!(eta_secs(MAX_ETA_SECS + 1, 1.0), None);
    }

    // --- format_eta ---------------------------------------------------------

    #[test]
    fn format_eta_unknown_is_none() {
        // Honesty: no "Calculating..." filler when ETA is unknown.
        assert_eq!(format_eta(None), None);
    }

    #[test]
    fn format_eta_seconds() {
        assert_eq!(format_eta(Some(0)).as_deref(), Some("about 0s left"));
        assert_eq!(format_eta(Some(1)).as_deref(), Some("about 1s left"));
        assert_eq!(format_eta(Some(30)).as_deref(), Some("about 30s left"));
        assert_eq!(format_eta(Some(59)).as_deref(), Some("about 59s left"));
    }

    #[test]
    fn format_eta_minutes() {
        assert_eq!(format_eta(Some(60)).as_deref(), Some("about 1 min left"));
        assert_eq!(format_eta(Some(90)).as_deref(), Some("about 1 min left"));
        assert_eq!(format_eta(Some(120)).as_deref(), Some("about 2 min left"));
        assert_eq!(format_eta(Some(3599)).as_deref(), Some("about 59 min left"));
    }

    #[test]
    fn format_eta_hours() {
        assert_eq!(format_eta(Some(3600)).as_deref(), Some("about 1 hour left"));
        assert_eq!(
            format_eta(Some(7200)).as_deref(),
            Some("about 2 hours left")
        );
        assert_eq!(
            format_eta(Some(3600 * 5 + 10)).as_deref(),
            Some("about 5 hours left")
        );
        assert_eq!(
            format_eta(Some(47 * 3600)).as_deref(),
            Some("about 47 hours left")
        );
    }

    #[test]
    fn format_eta_days_not_absurd_hour_count() {
        // Multi-day stays in days, never "about 1445 hours left".
        assert_eq!(
            format_eta(Some(48 * 3600)).as_deref(),
            Some("about 2 days left")
        );
        assert_eq!(
            format_eta(Some(3 * 24 * 3600)).as_deref(),
            Some("about 3 days left")
        );
    }

    // --- format_progress_line (omit unknown pieces) -------------------------

    #[test]
    fn format_progress_line_with_eta() {
        let line = format_progress_line("Generating...", Some(42), Some(60));
        assert_eq!(line, "Generating... 42% · about 1 min left");
    }

    #[test]
    fn format_progress_line_unknown_omits_percent_and_eta() {
        // Operator contract: no fake 0%, no "Calculating...", no "about … left".
        let line = format_progress_line("Downloading...", None, None);
        assert_eq!(line, "Downloading...");
        assert!(!line.contains('%'), "{line}");
        assert!(!line.contains("Calculating"), "{line}");
        assert!(!line.contains("hours left"), "{line}");
        assert!(!line.contains("about "), "{line}");
    }

    #[test]
    fn format_progress_line_percent_only_no_eta_filler() {
        let line = format_progress_line("Downloading...", Some(12), None);
        assert_eq!(line, "Downloading... 12%");
        assert!(!line.contains("Calculating"), "{line}");
        assert!(!line.contains("about "), "{line}");
    }

    #[test]
    fn format_progress_line_caps_percent() {
        let line = format_progress_line("Done", Some(200), Some(0));
        assert_eq!(line, "Done 100% · about 0s left");
    }

    #[test]
    fn format_install_chrome_status_omits_unknown_percent() {
        assert_eq!(
            format_install_chrome_status(Some("out-00000.safetensors"), None),
            "Installing · out-00000.safetensors"
        );
        assert_eq!(
            format_install_chrome_status(Some("out-00000.safetensors"), Some(4)),
            "Installing · out-00000.safetensors · 4%"
        );
        assert_eq!(
            format_install_chrome_status(None, Some(10)),
            "Installing · 10%"
        );
        assert_eq!(format_install_chrome_status(None, None), "Installing");
        assert!(!format_install_chrome_status(Some("f"), None).contains('%'));
    }

    // --- generate_progress --------------------------------------------------

    #[test]
    fn generate_midway_with_rate() {
        let (p, eta) = generate_progress(50, 100, 10.0);
        assert_eq!(p, Some(50));
        assert_eq!(eta, Some(5)); // 50 remaining / 10 tok/s
    }

    #[test]
    fn generate_zero_rate_eta_none() {
        let (p, eta) = generate_progress(10, 100, 0.0);
        assert_eq!(p, Some(10));
        assert_eq!(eta, None);
    }

    #[test]
    fn generate_progress_zero_tokens_is_zero_percent() {
        let (p, eta) = generate_progress(0, 4096, 0.0);
        assert_eq!(
            p,
            Some(0),
            "0 generated / N max is honest 0%, not a fake floor"
        );
        assert_eq!(eta, None);
    }

    #[test]
    fn generate_max_zero() {
        let (p, eta) = generate_progress(0, 0, 5.0);
        assert_eq!(p, None);
        assert_eq!(eta, None);
    }

    #[test]
    fn generate_done_over_max_caps() {
        let (p, eta) = generate_progress(150, 100, 20.0);
        assert_eq!(p, Some(100));
        assert_eq!(eta, Some(0));
    }

    #[test]
    fn generate_full_completion() {
        let (p, eta) = generate_progress(100, 100, 1.0);
        assert_eq!(p, Some(100));
        assert_eq!(eta, Some(0));
    }

    #[test]
    fn generate_ceil_eta() {
        // 3 remaining at 2 tok/s → 1.5 → ceil 2
        let (p, eta) = generate_progress(7, 10, 2.0);
        assert_eq!(p, Some(70));
        assert_eq!(eta, Some(2));
    }

    // --- install_progress ---------------------------------------------------

    #[test]
    fn install_prefers_bytes_when_both_known() {
        // Bytes 25%, files would be 50% — must prefer bytes.
        let (p, eta) = install_progress(Some(25), Some(100), Some(1), Some(2), 25.0);
        assert_eq!(p, Some(25));
        // remaining 75 bytes / 25 B/s = 3s
        assert_eq!(eta, Some(3));
    }

    #[test]
    fn install_falls_back_to_files() {
        let (p, eta) = install_progress(None, None, Some(3), Some(10), 1.0);
        assert_eq!(p, Some(30));
        assert_eq!(eta, Some(7)); // 7 files left at 1/s
    }

    #[test]
    fn install_bytes_total_zero_falls_to_files() {
        let (p, eta) = install_progress(Some(0), Some(0), Some(2), Some(4), 1.0);
        assert_eq!(p, Some(50));
        assert_eq!(eta, Some(2));
    }

    #[test]
    fn install_bytes_partial_none_uses_files() {
        // done without total → not usable; files win
        let (p, _) = install_progress(Some(50), None, Some(1), Some(4), 0.0);
        assert_eq!(p, Some(25));
    }

    #[test]
    fn install_neither_totals_is_none_no_eta() {
        let (p, eta) = install_progress(None, None, None, None, 100.0);
        assert_eq!(p, None);
        assert_eq!(eta, None);
        let line = format_progress_line("Downloading...", p, eta);
        assert_eq!(line, "Downloading...");
        assert!(!line.contains('%'), "{line}");
        assert!(!line.contains("about "), "{line}");
        assert!(!line.contains("hours left"), "{line}");
    }

    #[test]
    fn install_files_total_zero_is_none() {
        let (p, eta) = install_progress(None, None, Some(5), Some(0), 1.0);
        assert_eq!(p, None);
        assert_eq!(eta, None);
    }

    #[test]
    fn install_bytes_done_over_total() {
        let (p, eta) = install_progress(Some(200), Some(100), None, None, 10.0);
        assert_eq!(p, Some(100));
        assert_eq!(eta, Some(0));
    }

    #[test]
    fn install_zero_rate_has_percent_no_eta() {
        let (p, eta) = install_progress(Some(40), Some(100), None, None, 0.0);
        assert_eq!(p, Some(40));
        assert_eq!(eta, None);
        let line = format_progress_line("Downloading...", p, eta);
        assert_eq!(line, "Downloading... 40%");
        assert!(!line.contains("about "), "{line}");
    }

    #[test]
    fn install_zero_done_no_percent_no_eta() {
        // Operator: done==0 → no `%`, no "about … left" / hours left.
        let total = 372_u64 * 1024 * 1024 * 1024; // ~372 GiB model
        let (p, eta) = install_progress(
            Some(0),
            Some(total),
            Some(0),
            Some(80),
            50.0 * 1024.0 * 1024.0,
        );
        assert_eq!(p, None, "0 bytes done must not claim 0%");
        assert_eq!(eta, None, "0 bytes done must not show a multi-day ETA");
        let line = format_progress_line("Downloading...", p, eta);
        assert_eq!(line, "Downloading...");
        assert!(!line.contains('%'), "{line}");
        assert!(!line.contains("Calculating"), "{line}");
        assert!(!line.contains("hours left"), "{line}");
        assert!(!line.contains("about "), "{line}");
    }

    #[test]
    fn install_zero_total_bytes_no_div_by_zero() {
        let (p, eta) = install_progress(Some(10), Some(0), None, None, 1.0);
        // total 0 → fall through; no files → None, no ETA
        assert_eq!(p, None);
        assert_eq!(eta, None);
    }

    #[test]
    fn install_partial_file_advances_percent() {
        // Multi-shard: 1 file done (100 B) + current file 25 of 100, total 400.
        let (p, eta) = install_progress_with_partial(
            100, // completed prior
            25,  // current partial
            Some(400),
            Some(1),
            Some(4),
            25.0, // B/s
        );
        // 125/400 = 31%
        assert_eq!(p, Some(31));
        // remaining 275 / 25 = 11s
        assert_eq!(eta, Some(11));
        let line = format_progress_line("Downloading...", p, eta);
        assert!(line.contains("31%"), "{line}");
        assert!(line.contains("about "), "{line}");
    }

    #[test]
    fn install_multi_file_completed_only_file_boundary() {
        // File-boundary style: two of four 100 B files done, no mid-file yet.
        let (p, eta) = install_progress(Some(200), Some(400), Some(2), Some(4), 50.0);
        assert_eq!(p, Some(50));
        assert_eq!(eta, Some(4)); // 200 remaining / 50 B/s
    }

    #[test]
    fn install_absurd_eta_hidden() {
        // Tiny done, huge remaining, tiny rate → multi-week ETA → None.
        let (p, eta) =
            install_progress(Some(1), Some(u64::from(u32::MAX) * 1000), None, None, 1e-6);
        assert_eq!(p, Some(1)); // floor 1% once any byte landed
        assert_eq!(eta, None);
        let line = format_progress_line("Downloading...", p, eta);
        assert!(line.contains("1%"), "{line}");
        assert!(!line.contains("about "), "{line}");
        assert!(!line.contains("hours left"), "{line}");
    }

    #[test]
    fn aggregate_download_bytes_sums_and_saturates() {
        assert_eq!(aggregate_download_bytes(0, 0), 0);
        assert_eq!(aggregate_download_bytes(100, 50), 150);
        assert_eq!(aggregate_download_bytes(u64::MAX, 10), u64::MAX);
    }

    // --- fill_fraction (bar width must match label percent) -----------------

    #[test]
    fn fill_fraction_none_is_empty_track() {
        assert_eq!(fill_fraction(None), 0.0);
    }

    #[test]
    fn fill_fraction_zero_four_full() {
        assert_eq!(fill_fraction(Some(0)), 0.0);
        assert!((fill_fraction(Some(4)) - 0.04).abs() < f32::EPSILON);
        assert_eq!(fill_fraction(Some(100)), 1.0);
    }

    #[test]
    fn fill_fraction_caps_above_100() {
        assert_eq!(fill_fraction(Some(200)), 1.0);
        assert_eq!(fill_fraction(Some(u8::MAX)), 1.0);
    }

    #[test]
    fn label_percent_and_fill_fraction_are_identical() {
        // Contract: bar paint width ratio equals the percent printed in the line.
        for pct in [0_u8, 1, 4, 25, 42, 50, 99, 100, 150] {
            let v = ProgressView::new(Some(pct), Some(3600), "Downloading...");
            let capped = pct.min(100);
            assert_eq!(v.percent, Some(capped));
            assert!(
                (v.fill_fraction() - fill_fraction(Some(capped))).abs() < f32::EPSILON,
                "view fill vs helper at {pct}"
            );
            assert!(
                (v.fill_fraction() - f32::from(capped) / 100.0).abs() < f32::EPSILON,
                "fill fraction must be percent/100 at {pct}"
            );
            let line = v.line();
            assert!(
                line.contains(&format!("{capped}%")),
                "line must show same percent as fill: line={line:?} capped={capped}"
            );
            // Bar width ratio equals label percent (0..=1).
            assert!((v.fill_fraction() * 100.0 - f32::from(capped)).abs() < 0.001);
        }
        // Unknown percent: empty track, no `%` in line.
        let unknown = ProgressView::new(None, None, "Downloading...");
        assert_eq!(unknown.fill_fraction(), 0.0);
        assert!(!unknown.line().contains('%'));
    }

    #[test]
    fn install_view_four_percent_fill_matches_line() {
        // Operator screenshot case: "Downloading... 4% · about 1 hour left"
        let v = install_progress_view(
            "Downloading...",
            Some(4),
            Some(100),
            None,
            None,
            0.0, // no rate → omit ETA
        );
        assert_eq!(v.percent, Some(4));
        assert!((v.fill_fraction() - 0.04).abs() < f32::EPSILON);
        assert!(v.line().contains("4%"), "line={}", v.line());
        assert!(!v.line().contains("about "), "line={}", v.line());
        // Explicit bar/track width ratio used by paint.
        let track_w = 400.0_f32;
        let fill_w = track_w * v.fill_fraction();
        assert!((fill_w - 16.0).abs() < 0.001, "fill_w={fill_w}");
    }

    // --- ProgressView -------------------------------------------------------

    #[test]
    fn progress_view_line_and_cap() {
        let v = ProgressView::new(Some(150), Some(30), "Working");
        assert_eq!(v.percent, Some(100));
        assert_eq!(v.line(), "Working 100% · about 30s left");
        assert_eq!(v.fill_fraction(), 1.0);
    }

    #[test]
    fn progress_view_unknown_line_is_label_only() {
        let v = ProgressView::new(None, None, "Downloading...");
        assert_eq!(v.percent, None);
        assert_eq!(v.eta_secs, None);
        assert_eq!(v.line(), "Downloading...");
        assert_eq!(v.fill_fraction(), 0.0);
    }

    #[test]
    fn generate_progress_view_wires_label() {
        let v = generate_progress_view("Generating...", 42, 100, 0.0);
        assert_eq!(v.percent, Some(42));
        assert_eq!(v.eta_secs, None);
        assert_eq!(v.line(), "Generating... 42%");
    }

    #[test]
    fn install_progress_view_bytes() {
        let v = install_progress_view("Downloading...", Some(50), Some(100), None, None, 25.0);
        assert_eq!(v.percent, Some(50));
        assert_eq!(v.eta_secs, Some(2)); // 50 / 25
        assert!(v.line().starts_with("Downloading... 50%"));
        assert!(v.line().contains("about "), "{}", v.line());
    }
}
