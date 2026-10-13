//! Process binary for a model directory.
//!
//! `model_arch` folds several 2.0 text types onto GLM or DeepSeek V4. The
//! serve spawn reads `config.json` `model_type` first and uses the registry
//! process name. Image, Laya, and GLiNER stay on that older classification.

use std::path::Path;

use crate::model::model_arch;

use super::locate::engine_basename;

/// Upstream KV mux width for every text family in the registry.
pub const TEXT_ROSTER_MAX_KV_SLOTS: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessRoute {
    pub binary: &'static str,
    /// Expert-cache argument passed when config leaves cap unset.
    /// `None` keeps the historical spawn default for that [`crate::model::ModelFamily`].
    pub implicit_cap: Option<u32>,
    pub max_kv_slots: u32,
}

fn route(binary: &'static str, implicit_cap: u32) -> ProcessRoute {
    ProcessRoute {
        binary,
        implicit_cap: Some(implicit_cap),
        max_kv_slots: TEXT_ROSTER_MAX_KV_SLOTS,
    }
}

/// Registry `model_type` to process binary. Longer names are exact matches
/// so `deepseek_v41` is not `deepseek_v4` and `glm5_next` is not `colibri`.
pub fn text_roster_for_model_type(model_type: &str) -> Option<ProcessRoute> {
    match model_type.trim().to_ascii_lowercase().as_str() {
        "deepseek_v41" | "deepseek_v41_text" => Some(route("deepseek_v41", 8)),
        "glm5_next" | "glm5_next_text" => Some(route("glm53", 0)),
        "qwen4_exp" | "qwen4_exp_text" => Some(route("qwen38", 1)),
        "qwen3_5_moe" | "qwen3_5_moe_text" | "qwen3_5" | "qwen3_5_text" | "qwen3_moe" => {
            Some(route("qwen36", 8))
        }
        "mimo_v2" => Some(route("mimo", 16)),
        "olmoe" => Some(route("olmoe", 0)),
        _ => None,
    }
}

fn read_model_type(model: &Path) -> Option<String> {
    let text = std::fs::read_to_string(model.join("config.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("model_type")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
}

/// Binary and expert cap for a model directory.
pub fn process_route_for_model(model: &Path) -> ProcessRoute {
    if let Some(mt) = read_model_type(model) {
        if let Some(route) = text_roster_for_model_type(&mt) {
            return route;
        }
    }
    let family = model_arch(model);
    ProcessRoute {
        binary: engine_basename(family),
        implicit_cap: None,
        max_kv_slots: TEXT_ROSTER_MAX_KV_SLOTS,
    }
}

/// Host mux width. Zero becomes 1. Values above the engine max clamp to that
/// max, and the max itself stays at most 16.
pub fn clamp_kv_slots(requested: u32, family_max: u32) -> u32 {
    let max = family_max.clamp(1, TEXT_ROSTER_MAX_KV_SLOTS);
    if requested == 0 {
        1
    } else {
        requested.clamp(1, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ColibriConfig;
    use crate::model::ModelFamily;
    use std::fs;
    use std::path::PathBuf;

    fn with_type(name: &str, model_type: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("colibri-roster-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(
            dir.join("config.json"),
            format!(r#"{{"model_type":"{model_type}"}}"#),
        )
        .expect("config");
        dir
    }

    #[test]
    fn process_binary_qwen36_is_not_colibri() {
        let dir = with_type("qwen36", "qwen3_5_moe");
        let route = process_route_for_model(&dir);
        assert_eq!(route.binary, "qwen36");
        assert_ne!(route.binary, "colibri");
        assert_eq!(route.implicit_cap, Some(8));
        assert_eq!(
            text_roster_for_model_type("qwen3_5_text").unwrap().binary,
            "qwen36"
        );
        assert_eq!(
            text_roster_for_model_type("qwen3_moe").unwrap().binary,
            "qwen36"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn process_binary_deepseek_v41_is_not_v4() {
        let dir = with_type("v41", "deepseek_v41");
        let route = process_route_for_model(&dir);
        assert_eq!(route.binary, "deepseek_v41");
        assert_ne!(route.binary, "deepseek_v4");
        assert_eq!(route.implicit_cap, Some(8));
        assert_eq!(
            text_roster_for_model_type("deepseek_v41_text")
                .unwrap()
                .binary,
            "deepseek_v41"
        );
        let v4 = with_type("v4", "deepseek_v4");
        assert_eq!(process_route_for_model(&v4).binary, "deepseek_v4");
        assert!(text_roster_for_model_type("deepseek_v4").is_none());
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&v4);
    }

    #[test]
    fn process_binary_olmoe_is_olmoe() {
        assert_eq!(engine_basename(ModelFamily::Olmoe), "olmoe");
        let dir = with_type("olmoe", "olmoe");
        let route = process_route_for_model(&dir);
        assert_eq!(route.binary, "olmoe");
        assert_eq!(route.implicit_cap, Some(0));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn process_binary_glm53_qwen38_mimo() {
        assert_eq!(
            text_roster_for_model_type("glm5_next").unwrap().binary,
            "glm53"
        );
        assert_eq!(
            text_roster_for_model_type("glm5_next")
                .unwrap()
                .implicit_cap,
            Some(0)
        );
        assert_eq!(
            text_roster_for_model_type("qwen4_exp").unwrap().binary,
            "qwen38"
        );
        assert_eq!(
            text_roster_for_model_type("qwen4_exp_text")
                .unwrap()
                .implicit_cap,
            Some(1)
        );
        assert_eq!(
            text_roster_for_model_type("mimo_v2").unwrap().binary,
            "mimo"
        );
        assert_eq!(
            text_roster_for_model_type("mimo_v2").unwrap().implicit_cap,
            Some(16)
        );
    }

    #[test]
    fn image_laya_gliner_are_not_text_roster() {
        assert!(text_roster_for_model_type("qwen_image").is_none());
        assert!(text_roster_for_model_type("qwenimage").is_none());
        assert!(text_roster_for_model_type("laya").is_none());
        assert!(text_roster_for_model_type("gliner_decide").is_none());
    }

    #[test]
    fn text_roster_kv_slots_match_upstream() {
        for t in [
            "glm5_next",
            "olmoe",
            "qwen3_5_moe",
            "qwen4_exp",
            "deepseek_v41",
            "mimo_v2",
        ] {
            let route = text_roster_for_model_type(t).unwrap();
            assert_eq!(route.max_kv_slots, 16, "{t}");
        }
    }

    #[test]
    fn kv_slots_clamp_follows_engine_max() {
        assert_eq!(clamp_kv_slots(0, 16), 1);
        assert_eq!(clamp_kv_slots(1, 16), 1);
        assert_eq!(clamp_kv_slots(16, 16), 16);
        assert_eq!(clamp_kv_slots(32, 16), 16);
        let high = ColibriConfig::default().kv_slots(100);
        assert_eq!(high.kv_slots, 16);
        let low = ColibriConfig::default().kv_slots(0);
        assert_eq!(low.kv_slots, 1);
    }
}
