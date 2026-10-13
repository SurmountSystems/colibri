//! Parse C `[prefill]` banners and format the native status chip.
//!
//! Copy is native-only operational English (the SPA has no live prefill chip).
//! Source format: `c/colibri.c` `layers_forward_rows` fprintf, same line the
//! Python `c/coli` spinner tails. C prints singular `token`; the chip uses
//! plural `tokens`.

use std::sync::Mutex;

/// One parsed `[prefill] layer N/M · S token · +T.TTs` tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrefillProgress {
    pub layer: u32,
    pub total: u32,
    pub tokens: u32,
    pub elapsed_s: f32,
}

/// Snapshot written by the stderr tee. Not the FFI engine mutex.
static LAST_PREFILL: Mutex<Option<PrefillProgress>> = Mutex::new(None);

/// Parse a C `[prefill]` banner. Other stderr lines (including `[stop]`) are `None`.
pub fn parse_prefill_line(line: &str) -> Option<PrefillProgress> {
    let line = line.trim();
    let rest = line.strip_prefix("[prefill]")?.trim_start();
    let rest = rest.strip_prefix("layer")?.trim_start();
    let (layer_total, rest) = rest.split_once('·')?;
    let (layer_s, total_s) = layer_total.trim().split_once('/')?;
    let layer: u32 = layer_s.trim().parse().ok()?;
    let total: u32 = total_s.trim().parse().ok()?;
    let rest = rest.trim();
    let (tok_part, time_part) = rest.split_once('·')?;
    let tok_part = tok_part.trim();
    let mut tok_words = tok_part.split_whitespace();
    let tokens: u32 = tok_words.next()?.parse().ok()?;
    let unit = tok_words.next()?;
    if unit != "token" && unit != "tokens" {
        return None;
    }
    if tok_words.next().is_some() {
        return None;
    }
    let time = time_part.trim();
    let time = time.strip_prefix('+')?;
    let time = time.strip_suffix('s')?;
    let elapsed_s: f32 = time.parse().ok()?;
    Some(PrefillProgress {
        layer,
        total,
        tokens,
        elapsed_s,
    })
}

/// Status-chip phrase. Native-only operational English; not an i18n key.
pub fn format_prefill_status(layer: u32, total: u32, tokens: u32) -> String {
    format!("Prefill layer {layer}/{total} · {tokens} tokens")
}

/// Apply a prefill snapshot to the status chip when generate has no decode tokens.
///
/// Must not lock `engine` (that is the FFI generate mutex). The tee / UI poll
/// read a snapshot that is not this mutex.
pub fn apply_prefill_status<T>(
    generating: bool,
    live_token_count: u64,
    snapshot: Option<PrefillProgress>,
    _engine: &Mutex<T>,
) -> Option<String> {
    if !generating || live_token_count != 0 {
        return None;
    }
    let p = snapshot?;
    Some(format_prefill_status(p.layer, p.total, p.tokens))
}

/// Store a tick from the stderr tee. Does not take the FFI engine mutex.
pub fn store_prefill_progress(p: PrefillProgress) {
    if let Ok(mut g) = LAST_PREFILL.lock() {
        *g = Some(p);
    }
}

/// Latest tee snapshot, if any tick has been seen since the last clear.
pub fn load_prefill_progress() -> Option<PrefillProgress> {
    LAST_PREFILL.lock().ok().and_then(|g| *g)
}

/// Drop a stale tick (new generate). Does not take the FFI engine mutex.
pub fn clear_prefill_progress() {
    if let Ok(mut g) = LAST_PREFILL.lock() {
        *g = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex, mpsc};
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn parse_prefill_line_extracts_layer_total_tokens() {
        let p = parse_prefill_line("[prefill] layer 13/78 · 47 token · +21.80s")
            .expect("singular token banner");
        assert_eq!(p.layer, 13);
        assert_eq!(p.total, 78);
        assert_eq!(p.tokens, 47);
        assert!(
            (p.elapsed_s - 21.80).abs() < 0.001,
            "elapsed_s={}",
            p.elapsed_s
        );

        let plural = parse_prefill_line("[prefill] layer 13/78 · 47 tokens · +21.80s")
            .expect("plural tokens banner");
        assert_eq!(plural.layer, 13);
        assert_eq!(plural.total, 78);
        assert_eq!(plural.tokens, 47);
        assert!((plural.elapsed_s - 21.80).abs() < 0.001);

        let first =
            parse_prefill_line("[prefill] layer 1/78 · 47 token · +0.00s").expect("first tick");
        assert_eq!(first.layer, 1);
        assert_eq!(first.total, 78);
        assert_eq!(first.tokens, 47);
        assert!(first.elapsed_s.abs() < 0.001);
    }

    #[test]
    fn parse_prefill_line_rejects_stop_banner() {
        assert!(
            parse_prefill_line("[stop] 18 stop tokens: 1 2 3 4 5").is_none(),
            "stop banner is not a prefill tick"
        );
        assert!(parse_prefill_line("Generating... 0%").is_none());
        assert!(parse_prefill_line("").is_none());
        assert!(parse_prefill_line("garbage").is_none());
    }

    #[test]
    fn format_prefill_status_is_plain_operational_english() {
        assert_eq!(
            format_prefill_status(13, 78, 47),
            "Prefill layer 13/78 · 47 tokens"
        );
    }

    #[test]
    fn apply_prefill_status_does_not_take_engine_mutex() {
        let engine = Arc::new(Mutex::new(0u32));
        let held = Arc::clone(&engine);
        let (ready_tx, ready_rx) = mpsc::channel();
        let join = thread::spawn(move || {
            let _g = held.lock().unwrap();
            ready_tx.send(()).unwrap();
            thread::sleep(Duration::from_millis(400));
        });
        ready_rx.recv().unwrap();
        let snap = Some(PrefillProgress {
            layer: 13,
            total: 78,
            tokens: 47,
            elapsed_s: 21.80,
        });
        let start = Instant::now();
        let out = apply_prefill_status(true, 0, snap, &engine);
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(80),
            "apply must not lock the engine mutex, took {elapsed:?}"
        );
        assert_eq!(
            out.as_deref(),
            Some("Prefill layer 13/78 · 47 tokens"),
            "prefill status is a phrase, not a generate percent"
        );
        join.join().unwrap();
        let free = Mutex::new(());
        assert!(
            apply_prefill_status(false, 0, snap, &free).is_none(),
            "idle must not paint prefill"
        );
        assert!(
            apply_prefill_status(true, 1, snap, &free).is_none(),
            "first decode token leaves prefill; generate % owns the chip"
        );
    }
}
