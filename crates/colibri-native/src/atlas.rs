//! Expert atlas (`experts.json`) parse + Brain hover tip helpers.
//!
//! Web SPA loads `/experts.json` (static publish from `c/tools/expert_atlas`).
//! Native loads the same web-shaped file from env or cwd; missing file is empty
//! atlas with depth-role fallback (web parity).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// One expert's measured topic affinity (web Brain tooltip source).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AtlasEntry {
    pub affinity: HashMap<String, f32>,
    pub entropy: f32,
    pub top: String,
    pub label: String,
}

/// Parsed web-format experts atlas (`categories` + `"layer:expert"` keys).
#[derive(Debug, Clone, Default)]
pub struct ExpertAtlas {
    pub categories: Vec<String>,
    /// Keys are absolute MoE layer id and expert index within the layer.
    pub experts: HashMap<(u32, u32), AtlasEntry>,
}

impl ExpertAtlas {
    pub fn is_empty(&self) -> bool {
        self.experts.is_empty()
    }

    pub fn get(&self, layer: u32, expert: u32) -> Option<&AtlasEntry> {
        self.experts.get(&(layer, expert))
    }
}

#[derive(Debug, Deserialize)]
struct RawAtlasFile {
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    experts: HashMap<String, RawAtlasEntry>,
}

#[derive(Debug, Deserialize)]
struct RawAtlasEntry {
    #[serde(default)]
    affinity: HashMap<String, f32>,
    #[serde(default)]
    entropy: f32,
    #[serde(default)]
    top: String,
    #[serde(default)]
    label: String,
}

/// Parse web-shaped experts.json bytes. Bad keys are skipped; empty object ok.
pub fn parse_experts_json(bytes: &[u8]) -> Result<ExpertAtlas, String> {
    let raw: RawAtlasFile =
        serde_json::from_slice(bytes).map_err(|e| format!("experts.json: {e}"))?;
    let mut experts = HashMap::with_capacity(raw.experts.len());
    for (key, entry) in raw.experts {
        let Some((layer, expert)) = parse_layer_expert_key(&key) else {
            continue;
        };
        experts.insert(
            (layer, expert),
            AtlasEntry {
                affinity: entry.affinity,
                entropy: entry.entropy,
                top: entry.top,
                label: entry.label,
            },
        );
    }
    Ok(ExpertAtlas {
        categories: raw.categories,
        experts,
    })
}

/// `"layer:expert"` → (layer, expert). Invalid → None.
pub fn parse_layer_expert_key(key: &str) -> Option<(u32, u32)> {
    let (a, b) = key.split_once(':')?;
    let layer = a.parse().ok()?;
    let expert = b.parse().ok()?;
    Some((layer, expert))
}

/// Env override path: `COLIBRI_EXPERTS_JSON` then `COLI_EXPERTS_JSON`.
pub fn experts_json_path_from_env() -> Option<PathBuf> {
    std::env::var_os("COLIBRI_EXPERTS_JSON")
        .or_else(|| std::env::var_os("COLI_EXPERTS_JSON"))
        .map(PathBuf::from)
}

/// Load order (first readable file wins): env path, then cwd `experts.json`.
/// Missing / unreadable → empty atlas (depth-role fallback still works).
pub fn load_experts_atlas() -> ExpertAtlas {
    if let Some(path) = experts_json_path_from_env() {
        if let Ok(atlas) = load_experts_atlas_from_path(&path) {
            return atlas;
        }
    }
    let cwd = PathBuf::from("experts.json");
    load_experts_atlas_from_path(&cwd).unwrap_or_default()
}

/// Load and parse a path; Err on IO or JSON failure.
pub fn load_experts_atlas_from_path(path: &Path) -> Result<ExpertAtlas, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_experts_json(&bytes)
}

/// EMAP grid row → absolute MoE layer id (GLM-5.2 Brain.tsx convention).
///
/// Last grid row is the MTP head → layer 78. All other rows: `row + 3`
/// (dense layers 0..2 sit before sparse MoE experts on the canvas).
///
/// See web `Brain.tsx` hover mapping. Other families may differ; do not invent
/// multi-model atlas without evidence.
pub fn emap_row_to_layer(row: u32, rows: u32) -> (u32, bool) {
    let rows = rows.max(1);
    if row + 1 >= rows {
        (78, true)
    } else {
        (row.saturating_add(3), false)
    }
}

/// Depth-role English copy when atlas has no entry (web `brain.*` en strings).
pub fn depth_role(row: u32, rows: u32, is_mtp: bool) -> &'static str {
    if is_mtp {
        return "MTP head - drafts the next token for speculative decoding";
    }
    let denom = rows.saturating_sub(1).max(1) as f32;
    let f = row as f32 / denom;
    if f < 0.2 {
        "early layers - surface features: tokens, spelling, local syntax"
    } else if f < 0.45 {
        "lower-middle - phrase structure, word relations, simple facts"
    } else if f < 0.7 {
        "upper-middle - semantics, long-range context, reasoning steps"
    } else if f < 0.9 {
        "late layers - planning the answer, style, coherence"
    } else {
        "final layers - output shaping: picks the actual next-token distribution"
    }
}

/// Tier display name for tip (disk / system RAM / GPU).
pub fn tier_name(tier: u8) -> &'static str {
    match tier {
        2 => "GPU",
        1 => "System RAM",
        _ => "Disk",
    }
}

/// Heat line: never routed vs ~2^heat selections (web copy).
pub fn heat_line(heat: u8) -> String {
    if heat == 0 {
        "Heat: never routed".into()
    } else {
        format!("Heat: ~2^{heat} selections")
    }
}

/// Top-N affinity topics by value, formatted `topic NN% · …`.
pub fn top_affinities(entry: &AtlasEntry, n: usize) -> String {
    let mut pairs: Vec<(&str, f32)> = entry
        .affinity
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect();
    pairs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    pairs
        .into_iter()
        .take(n)
        .map(|(k, v)| format!("{} {}%", k, (v * 100.0).round() as i32))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Build multi-line hover tip for a source cell (atlas or depth-role fallback).
pub fn format_brain_tooltip(
    src_row: u32,
    src_col: u32,
    src_rows: u32,
    tier: u8,
    heat: u8,
    atlas: &ExpertAtlas,
) -> String {
    let (layer, is_mtp) = emap_row_to_layer(src_row, src_rows);
    let mtp_bit = if is_mtp { " (MTP)" } else { "" };
    let mut lines = Vec::with_capacity(5);
    lines.push(format!("Layer {layer}{mtp_bit} · Expert {src_col}"));
    lines.push(format!("Tier: {}", tier_name(tier)));
    lines.push(heat_line(heat));

    if let Some(entry) = atlas.get(layer, src_col) {
        let role = if entry.label.starts_with("specialist") {
            format!("Specialist: {}", entry.top)
        } else {
            "Generalist".to_string()
        };
        // Entropy formatting: web shows raw number; keep short decimal.
        lines.push(format!("{role} (entropy {:.2})", entry.entropy));
        let aff = top_affinities(entry, 3);
        if !aff.is_empty() {
            lines.push(aff);
        }
    } else {
        lines.push(depth_role(src_row, src_rows, is_mtp).to_string());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
  "categories": ["code_python", "poetry", "math_proof"],
  "experts": {
    "3:0": {
      "affinity": {"code_python": 0.4, "poetry": 0.12, "math_proof": 0.05},
      "entropy": 1.23,
      "top": "code_python",
      "label": "specialist: code_python"
    },
    "10:7": {
      "affinity": {"poetry": 0.2, "code_python": 0.15},
      "entropy": 2.8,
      "top": "poetry",
      "label": "generalist"
    },
    "bad-key": {
      "affinity": {},
      "entropy": 0.0,
      "top": "x",
      "label": "generalist"
    }
  }
}"#;

    #[test]
    fn parse_web_shaped_experts_json() {
        let atlas = parse_experts_json(FIXTURE.as_bytes()).expect("parse");
        assert_eq!(
            atlas.categories,
            vec!["code_python", "poetry", "math_proof"]
        );
        assert_eq!(atlas.experts.len(), 2, "bad keys skipped");
        let e = atlas.get(3, 0).expect("3:0");
        assert_eq!(e.top, "code_python");
        assert!(e.label.starts_with("specialist"));
        assert!((e.entropy - 1.23).abs() < 1e-5);
        assert!((e.affinity["code_python"] - 0.4).abs() < 1e-5);
        let g = atlas.get(10, 7).expect("10:7");
        assert_eq!(g.label, "generalist");
    }

    #[test]
    fn parse_empty_and_invalid_json() {
        let empty = parse_experts_json(br#"{"experts":{}}"#).unwrap();
        assert!(empty.is_empty());
        let missing = parse_experts_json(br#"{}"#).unwrap();
        assert!(missing.is_empty());
        assert!(parse_experts_json(br#"not json"#).is_err());
    }

    #[test]
    fn parse_layer_expert_key_accepts_web_keys() {
        assert_eq!(parse_layer_expert_key("3:0"), Some((3, 0)));
        assert_eq!(parse_layer_expert_key("78:255"), Some((78, 255)));
        assert_eq!(parse_layer_expert_key("nope"), None);
        assert_eq!(parse_layer_expert_key("3:"), None);
    }

    #[test]
    fn emap_row_to_layer_glm_convention() {
        // 76-row GLM-style map: rows 0..74 → layers 3..77; row 75 → MTP 78.
        assert_eq!(emap_row_to_layer(0, 76), (3, false));
        assert_eq!(emap_row_to_layer(1, 76), (4, false));
        assert_eq!(emap_row_to_layer(10, 76), (13, false));
        assert_eq!(emap_row_to_layer(74, 76), (77, false));
        assert_eq!(emap_row_to_layer(75, 76), (78, true));
        // Single-row map: that row is MTP.
        assert_eq!(emap_row_to_layer(0, 1), (78, true));
    }

    #[test]
    fn depth_role_bands_and_mtp() {
        assert!(depth_role(0, 76, true).contains("MTP"));
        assert!(depth_role(0, 76, false).contains("early"));
        assert!(depth_role(20, 76, false).contains("lower-middle"));
        assert!(depth_role(40, 76, false).contains("upper-middle"));
        assert!(depth_role(60, 76, false).contains("late"));
        assert!(depth_role(74, 76, false).contains("final"));
    }

    #[test]
    fn tooltip_with_atlas_specialist_top3() {
        let atlas = parse_experts_json(FIXTURE.as_bytes()).unwrap();
        // src_row 0 → layer 3; expert 0 specialist
        let tip = format_brain_tooltip(0, 0, 76, 2, 12, &atlas);
        assert!(tip.contains("Layer 3"), "{tip}");
        assert!(tip.contains("Expert 0"), "{tip}");
        assert!(tip.contains("Specialist: code_python"), "{tip}");
        assert!(tip.contains("entropy 1.23"), "{tip}");
        // Top-3 order by affinity
        assert!(tip.contains("code_python 40%"), "{tip}");
        assert!(tip.contains("poetry 12%"), "{tip}");
        assert!(tip.contains("math_proof 5%"), "{tip}");
        // Order: code_python before poetry
        let pos_code = tip.find("code_python 40%").unwrap();
        let pos_poetry = tip.find("poetry 12%").unwrap();
        assert!(pos_code < pos_poetry);
    }

    #[test]
    fn tooltip_generalist_and_depth_fallback() {
        let atlas = parse_experts_json(FIXTURE.as_bytes()).unwrap();
        // layer 10 → src_row 7
        let tip = format_brain_tooltip(7, 7, 76, 1, 0, &atlas);
        assert!(tip.contains("Layer 10"), "{tip}");
        assert!(tip.contains("Generalist"), "{tip}");
        assert!(tip.contains("never routed"), "{tip}");

        // No atlas entry → depth role (row 0 early)
        let empty = ExpertAtlas::default();
        let tip2 = format_brain_tooltip(0, 5, 76, 0, 3, &empty);
        assert!(tip2.contains("early layers"), "{tip2}");
        assert!(tip2.contains("~2^3 selections"), "{tip2}");

        // Last row MTP fallback
        let tip_mtp = format_brain_tooltip(75, 10, 76, 2, 1, &empty);
        assert!(tip_mtp.contains("Layer 78 (MTP)"), "{tip_mtp}");
        assert!(tip_mtp.contains("MTP head"), "{tip_mtp}");
    }

    #[test]
    fn load_from_path_fixture_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("experts.json");
        fs::write(&path, FIXTURE).unwrap();
        let atlas = load_experts_atlas_from_path(&path).unwrap();
        assert!(atlas.get(3, 0).is_some());
        // Missing path errors
        assert!(load_experts_atlas_from_path(Path::new("/no/such/experts.json")).is_err());
    }
}
