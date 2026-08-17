//! SSD cache grammar vectors shared with C/Python.
//!
//! Source: `c/tests/fixtures/ssd_cache_vectors.txt` (copied under tests/fixtures).

use colibri_sys::{SsdCacheParse, parse_ssd_cache};

fn unescape_payload(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'n' => out.push(b'\n'),
                b'r' => out.push(b'\r'),
                b't' => out.push(b'\t'),
                b'0' => out.push(0),
                b's' => out.push(b' '),
                b'\\' => out.push(b'\\'),
                other => {
                    out.push(b'\\');
                    out.push(other);
                }
            }
            i += 2;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

fn load_vectors() -> String {
    // Prefer crate-local copy; fall back to repo fixture when run from workspace.
    let candidates = [
        "tests/fixtures/ssd_cache_vectors.txt",
        "crates/colibri-sys/tests/fixtures/ssd_cache_vectors.txt",
        "c/tests/fixtures/ssd_cache_vectors.txt",
    ];
    for p in candidates {
        if let Ok(s) = std::fs::read_to_string(p) {
            return s;
        }
    }
    panic!("ssd_cache_vectors.txt not found");
}

#[test]
fn ssd_cache_vectors_match_grammar() {
    let text = load_vectors();
    let mut cases = 0;
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        match fields[0] {
            "v2" => {
                assert!(fields.len() >= 4, "v2 line needs 4 fields: {line}");
                let expect_gbs: f64 = fields[1].parse().unwrap();
                let expect_dev: u64 = fields[2].parse().unwrap();
                let payload = unescape_payload(fields[3]);
                match parse_ssd_cache(&payload) {
                    SsdCacheParse::V2 { gbs, st_dev } => {
                        assert!(
                            (gbs - expect_gbs).abs() < 1e-9,
                            "gbs mismatch for {line}: {gbs} vs {expect_gbs}"
                        );
                        assert_eq!(st_dev, expect_dev, "st_dev mismatch for {line}");
                    }
                    other => panic!("expected v2 for {line:?}, got {other:?}"),
                }
                cases += 1;
            }
            "legacy" => {
                assert!(fields.len() >= 3, "legacy line needs 3 fields: {line}");
                let expect_gbs: f64 = fields[1].parse().unwrap();
                let payload = unescape_payload(fields[2]);
                match parse_ssd_cache(&payload) {
                    SsdCacheParse::Legacy { gbs } => {
                        assert!(
                            (gbs - expect_gbs).abs() < 1e-9,
                            "legacy gbs mismatch for {line}"
                        );
                    }
                    other => panic!("expected legacy for {line:?}, got {other:?}"),
                }
                cases += 1;
            }
            "garbage" => {
                let payload = if fields.len() >= 2 {
                    unescape_payload(fields[1])
                } else {
                    Vec::new()
                };
                assert_eq!(
                    parse_ssd_cache(&payload),
                    SsdCacheParse::Garbage,
                    "expected garbage for payload of line {line:?}"
                );
                cases += 1;
            }
            other => panic!("unknown vector kind {other}"),
        }
    }
    assert!(cases >= 40, "expected many vectors, got {cases}");
}
