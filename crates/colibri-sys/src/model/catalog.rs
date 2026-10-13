//! Static catalog of models Colibri supports.
//!
//! Seeded from `c/family_registry.py` and `c/setup_catalog.py` on this branch,
//! plus the August text ids the host tests already resolve. This is **not**
//! local disk inventory ([`super::ModelRegistry`]) and not the live
//! `/v1/models` list after a server load.
//!
//! [`CatalogLaunch`] says whether this window can start the row. Rows the
//! process launcher cannot start stay on the list and do not offer a text load.

use std::path::Path;

use serde_json::Value;

use super::ModelFamily;

/// How a catalog row is started from the native window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogLaunch {
    /// Existing text FFI or process engine (`glm`, `kimi`, `inkling`, `deepseek_v4`, `olmoe`).
    Text(ModelFamily),
    /// `qwenimage --model DIR --serve`. Not the text engine.
    Image,
    /// Laya or GLiNER. Decision servers, not this text window.
    Decision,
    /// Named on the 2.0 roster. This host has no process launch for it.
    NotStartedHere,
}

/// What a model directory is, for refusing a text-engine start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelDirKind {
    Text(ModelFamily),
    Image,
    Decision,
    Unsupported,
}

/// One product-supported model entry (static, compile-time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupportedModel {
    /// Stable id. August text ids stay (`glm-5.2-colibri`, …).
    pub id: &'static str,
    /// Plain product name.
    pub display_name: &'static str,
    /// Text family when [`Self::launch`] is [`CatalogLaunch::Text`].
    /// Other launches do not use this field to start an engine.
    pub family: ModelFamily,
    /// Whether the native window can start this row, and how.
    pub launch: CatalogLaunch,
    /// `family_registry.py` family id (`glm`, `qwen_image`, `laya`, …).
    pub roster_family: &'static str,
    /// Hugging Face `owner/name` when a snapshot is documented.
    /// Present on rows this window does not install, for display only.
    pub hf_repo: Option<&'static str>,
    /// Short operational note.
    pub notes: Option<&'static str>,
    /// Disk size hint from `setup_catalog.py` when documented.
    pub disk_hint: Option<&'static str>,
}

impl SupportedModel {
    /// True when Start engine / the text FFI can load this row.
    pub fn offers_text_load(self) -> bool {
        matches!(self.launch, CatalogLaunch::Text(_))
    }

    /// True when the install form may download this row.
    ///
    /// Image weights are included. Families this host cannot start are not.
    pub fn offers_install(self) -> bool {
        self.hf_repo.is_some()
            && matches!(self.launch, CatalogLaunch::Text(_) | CatalogLaunch::Image)
    }
}

/// All models on the 2.0 roster that the native window lists.
pub fn supported_models() -> &'static [SupportedModel] {
    SUPPORTED_MODELS
}

/// Lookup by server-style id (exact match).
pub fn supported_model_by_id(id: &str) -> Option<&'static SupportedModel> {
    SUPPORTED_MODELS.iter().find(|m| m.id == id)
}

/// Lookup by Hugging Face `owner/name` (exact match).
pub fn supported_model_by_hf_repo(repo: &str) -> Option<&'static SupportedModel> {
    SUPPORTED_MODELS
        .iter()
        .find(|m| m.hf_repo.is_some_and(|r| r == repo))
}

/// Classify `config.json` `model_type` without reading a directory.
///
/// `deepseek_v41` is checked before `deepseek_v4` because the V4.1 token
/// contains the V4 substring. Unknown types that are not a roster family
/// this host refuses still fall through to GLM, matching `model_arch_from_type`.
pub fn kind_from_model_type(model_type: &str) -> ModelDirKind {
    let t = model_type.to_ascii_lowercase();
    if t.contains("glm5_next") || t.contains("qwen") || t.contains("mimo") {
        return ModelDirKind::Unsupported;
    }
    if t.contains("deepseek_v41")
        || t.contains("deepseek-v4.1")
        || t.contains("deepseek_v4_1")
        || t.contains("deepseekv41")
    {
        return ModelDirKind::Unsupported;
    }
    if t.contains("extractor") || t.contains("gliner") {
        return ModelDirKind::Decision;
    }
    if t.contains("inkling") {
        return ModelDirKind::Text(ModelFamily::Inkling);
    }
    if t.contains("kimi") {
        return ModelDirKind::Text(ModelFamily::Kimi);
    }
    if t.contains("deepseek_v4") || (t.contains("deepseek") && t.contains("v4")) {
        return ModelDirKind::Text(ModelFamily::DeepseekV4);
    }
    if t.contains("olmoe") {
        return ModelDirKind::Text(ModelFamily::Olmoe);
    }
    ModelDirKind::Text(ModelFamily::Glm)
}

/// Classify a model directory. `None` when it is not a model leaf yet.
///
/// Image is `model_index.json` with `_class_name` containing `qwenimage` and
/// no root `config.json` (a text checkpoint that also has an index stays text).
/// Laya is `rl_agent_config.json`. GLiNER is `model_type` `extractor`.
pub fn classify_model_dir(path: &Path) -> Option<ModelDirKind> {
    if path.join("rl_agent_config.json").is_file() {
        return Some(ModelDirKind::Decision);
    }
    let config = path.join("config.json");
    if config.is_file() {
        let text = std::fs::read_to_string(&config).ok()?;
        let value: Value = serde_json::from_str(&text).ok()?;
        let model_type = value
            .get("model_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        return Some(kind_from_model_type(model_type));
    }
    let index = path.join("model_index.json");
    if index.is_file() {
        let text = std::fs::read_to_string(&index).ok()?;
        let value: Value = serde_json::from_str(&text).ok()?;
        let class = value
            .get("_class_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let norm: String = class
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        if norm.to_ascii_lowercase().contains("qwenimage") {
            return Some(ModelDirKind::Image);
        }
    }
    None
}

/// Plain refusal when Start engine must not load this folder as a text model.
/// `None` means the text path may proceed (including a folder that is not a leaf yet).
pub fn text_start_refusal(path: &Path) -> Option<&'static str> {
    match classify_model_dir(path) {
        Some(ModelDirKind::Image) => {
            Some("This folder is an image model. Use image chat, not Start engine.")
        }
        Some(ModelDirKind::Decision) => Some(
            "This folder is a decision model. This window does not start it through the text engine.",
        ),
        Some(ModelDirKind::Unsupported) => Some("This host cannot start this model family."),
        Some(ModelDirKind::Text(_)) | None => None,
    }
}

const NOT_STARTED: &str = "not started from this window";

/// Notes and disk hint are one argument so `row` stays within the const-fn limit.
struct NotesAndDisk {
    notes: Option<&'static str>,
    disk_hint: Option<&'static str>,
}

const fn row(
    id: &'static str,
    display_name: &'static str,
    family: ModelFamily,
    launch: CatalogLaunch,
    roster_family: &'static str,
    hf_repo: Option<&'static str>,
    aside: NotesAndDisk,
) -> SupportedModel {
    SupportedModel {
        id,
        display_name,
        family,
        launch,
        roster_family,
        hf_repo,
        notes: aside.notes,
        disk_hint: aside.disk_hint,
    }
}

const fn text(
    id: &'static str,
    display_name: &'static str,
    family: ModelFamily,
    roster_family: &'static str,
    hf_repo: Option<&'static str>,
    notes: Option<&'static str>,
    disk_hint: Option<&'static str>,
) -> SupportedModel {
    row(
        id,
        display_name,
        family,
        CatalogLaunch::Text(family),
        roster_family,
        hf_repo,
        NotesAndDisk { notes, disk_hint },
    )
}

const SUPPORTED_MODELS: &[SupportedModel] = &[
    // Text families the August FFI / process path already starts.
    text(
        "glm-5.2-colibri",
        "GLM-5.2",
        ModelFamily::Glm,
        "glm",
        Some("mastouri/GLM-5.2-colibri-int4-g64-with-int8-mtp"),
        None,
        Some("429.3 GB"),
    ),
    text(
        "glm-5.3",
        "GLM-5.3",
        ModelFamily::Glm,
        "glm",
        Some("Justvugg/GLM-5.3-colibri-int4-g64"),
        None,
        Some("419.3 GB"),
    ),
    text(
        "inkling-colibri",
        "Inkling",
        ModelFamily::Inkling,
        "inkling",
        Some("nbeerbower/Inkling-colibri-int4"),
        None,
        Some("514.1 GB"),
    ),
    text(
        "kimi-k3-colibri",
        "Kimi K3",
        ModelFamily::Kimi,
        "kimi",
        Some("moonshotai/Kimi-K3"),
        Some("original checkpoint; routed experts stay native MXFP4"),
        Some("1561 GB"),
    ),
    text(
        "deepseek-v4-colibri",
        "DeepSeek V4 Flash",
        ModelFamily::DeepseekV4,
        "deepseek_v4",
        Some("deepseek-ai/DeepSeek-V4-Flash-0731"),
        Some("official sharded checkpoint; native fp4 experts, fp8-e4m3 dense"),
        Some("166.9 GB"),
    ),
    text(
        "deepseek-v4-flash-reap",
        "DeepSeek V4 Flash REAP 150B",
        ModelFamily::DeepseekV4,
        "deepseek_v4",
        Some("puwaer/DeepSeek-V4-Flash-0731-reap-150b"),
        Some("same deepseek_v4 engine as DeepSeek V4 Flash"),
        Some("84.7 GB"),
    ),
    text(
        "olmoe-colibri",
        "OLMoE",
        ModelFamily::Olmoe,
        "olmoe",
        None,
        Some("convert-only: use c/tools/convert_olmoe_merged.py"),
        Some("~4 GB"),
    ),
    // Image. Launch is qwenimage --serve, not the text FFI.
    row(
        "qwen-image-2.1-colibri",
        "Qwen-Image-2.1",
        ModelFamily::Glm,
        CatalogLaunch::Image,
        "qwen_image",
        Some("Qwen/Qwen-Image-2.1"),
        NotesAndDisk {
            notes: Some("image chat uses the qwenimage engine, not the text engine"),
            disk_hint: Some("33.1 GB"),
        },
    ),
    // On the roster. No process launch in this host.
    row(
        "glm-5.3-flash-colibri",
        "GLM-5.3-Flash",
        ModelFamily::Glm,
        CatalogLaunch::NotStartedHere,
        "glm53",
        None,
        NotesAndDisk {
            notes: Some(NOT_STARTED),
            disk_hint: None,
        },
    ),
    row(
        "qwen36-35b",
        "Qwen3.6-35B-A3B",
        ModelFamily::Glm,
        CatalogLaunch::NotStartedHere,
        "qwen36",
        Some("Kreuzzelg/qwen36-35b-a3b-colibri-i4-gs64"),
        NotesAndDisk {
            notes: Some(NOT_STARTED),
            disk_hint: Some("23.1 GB"),
        },
    ),
    row(
        "qwen3-coder-30b",
        "Qwen3-Coder-30B-A3B",
        ModelFamily::Glm,
        CatalogLaunch::NotStartedHere,
        "qwen36",
        Some("Justvugg/Qwen3-Coder-30B-A3B-colibri-int4"),
        NotesAndDisk {
            notes: Some(NOT_STARTED),
            disk_hint: Some("19.4 GB"),
        },
    ),
    row(
        "qwen3.8-27b-colibri",
        "Qwen3.8-27B",
        ModelFamily::Glm,
        CatalogLaunch::NotStartedHere,
        "qwen36",
        None,
        NotesAndDisk {
            notes: Some(NOT_STARTED),
            disk_hint: None,
        },
    ),
    row(
        "qwen3-coder-reap-25b-a3b-colibri",
        "Qwen3-Coder-REAP-25B-A3B",
        ModelFamily::Glm,
        CatalogLaunch::NotStartedHere,
        "qwen36",
        None,
        NotesAndDisk {
            notes: Some(NOT_STARTED),
            disk_hint: None,
        },
    ),
    row(
        "qwen3.8-2.4t-a95b",
        "Qwen3.8-2.4T-A95B",
        ModelFamily::Glm,
        CatalogLaunch::NotStartedHere,
        "qwen36",
        None,
        NotesAndDisk {
            notes: Some(NOT_STARTED),
            disk_hint: None,
        },
    ),
    row(
        "qwen38-flash-next",
        "Qwen3.8-Flash-Next",
        ModelFamily::Glm,
        CatalogLaunch::NotStartedHere,
        "qwen38",
        Some("Qwen/Qwen3.8-Flash-Next-FP8"),
        NotesAndDisk {
            notes: Some(NOT_STARTED),
            disk_hint: Some("185.6 GB"),
        },
    ),
    row(
        "deepseek-v4.1-flash-colibri",
        "DeepSeek V4.1 Flash",
        ModelFamily::Glm,
        CatalogLaunch::NotStartedHere,
        "deepseek_v41",
        None,
        NotesAndDisk {
            notes: Some(NOT_STARTED),
            disk_hint: None,
        },
    ),
    row(
        "mimo-v2.6-flash",
        "MiMo-V2.6 Flash",
        ModelFamily::Glm,
        CatalogLaunch::NotStartedHere,
        "mimo",
        Some("XiaomiMiMo/MiMo-V2.6-Flash-MOPD"),
        NotesAndDisk {
            notes: Some(NOT_STARTED),
            disk_hint: Some("171.8 GB"),
        },
    ),
    row(
        "mimo-v2.6-pro",
        "MiMo-V2.6 Pro",
        ModelFamily::Glm,
        CatalogLaunch::NotStartedHere,
        "mimo",
        Some("XiaomiMiMo/MiMo-V2.6-Pro-MOPD"),
        NotesAndDisk {
            notes: Some(NOT_STARTED),
            disk_hint: Some("563.6 GB"),
        },
    ),
    // Decision models. Upstream serves them on the gateway, not coli run / text FFI.
    row(
        "laya",
        "Laya",
        ModelFamily::Glm,
        CatalogLaunch::Decision,
        "laya",
        None,
        NotesAndDisk {
            notes: Some(NOT_STARTED),
            disk_hint: None,
        },
    ),
    row(
        "laya-multilingual",
        "Laya multilingual",
        ModelFamily::Glm,
        CatalogLaunch::Decision,
        "laya",
        None,
        NotesAndDisk {
            notes: Some(NOT_STARTED),
            disk_hint: None,
        },
    ),
    row(
        "gliner2.5-decide",
        "GLiNER2.5-Decide",
        ModelFamily::Glm,
        CatalogLaunch::Decision,
        "gliner_decide",
        None,
        NotesAndDisk {
            notes: Some(NOT_STARTED),
            disk_hint: None,
        },
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_non_empty() {
        assert!(!supported_models().is_empty());
    }

    #[test]
    fn every_hf_repo_has_owner_name_shape() {
        for m in supported_models() {
            let Some(repo) = m.hf_repo else {
                continue;
            };
            let parts: Vec<_> = repo.split('/').collect();
            assert_eq!(
                parts.len(),
                2,
                "{} hf_repo {repo:?} must be owner/name",
                m.id
            );
            assert!(!parts[0].is_empty(), "{} empty owner", m.id);
            assert!(!parts[1].is_empty(), "{} empty name", m.id);
            assert!(!repo.contains(".."), "{} path traversal in repo", m.id);
        }
    }

    #[test]
    fn known_ids_present() {
        for id in [
            "glm-5.2-colibri",
            "inkling-colibri",
            "kimi-k3-colibri",
            "deepseek-v4-colibri",
            "olmoe-colibri",
            "glm-5.3",
            "deepseek-v4-flash-reap",
            "qwen-image-2.1-colibri",
            "laya",
            "gliner2.5-decide",
        ] {
            assert!(
                supported_model_by_id(id).is_some(),
                "missing catalog id {id}"
            );
        }
    }

    #[test]
    fn known_display_names_from_readme() {
        let names: Vec<_> = supported_models().iter().map(|m| m.display_name).collect();
        for want in [
            "GLM-5.2",
            "Inkling",
            "Kimi K3",
            "DeepSeek V4 Flash",
            "OLMoE",
        ] {
            assert!(names.contains(&want), "missing display name {want}");
        }
    }

    #[test]
    fn lookup_by_id_returns_matching_entry() {
        let m = supported_model_by_id("glm-5.2-colibri").expect("glm");
        assert_eq!(m.family, ModelFamily::Glm);
        assert_eq!(m.launch, CatalogLaunch::Text(ModelFamily::Glm));
        assert!(m.offers_text_load());
        assert_eq!(
            m.hf_repo,
            Some("mastouri/GLM-5.2-colibri-int4-g64-with-int8-mtp")
        );
        assert!(supported_model_by_id("no-such-model").is_none());
    }

    #[test]
    fn lookup_by_hf_repo() {
        let m = supported_model_by_hf_repo("moonshotai/Kimi-K3").expect("kimi");
        assert_eq!(m.id, "kimi-k3-colibri");
        assert!(supported_model_by_hf_repo("nobody/nothing").is_none());
    }

    #[test]
    fn olmoe_is_convert_only() {
        let m = supported_model_by_id("olmoe-colibri").expect("olmoe");
        assert!(m.hf_repo.is_none());
        assert!(m.notes.is_some_and(|n| n.contains("convert")));
        assert_eq!(m.family, ModelFamily::Olmoe);
        assert!(m.offers_text_load());
        assert!(!m.offers_install());
    }

    #[test]
    fn installable_entries_have_hf_repo() {
        let installable: Vec<_> = supported_models()
            .iter()
            .filter(|m| m.offers_install())
            .collect();
        assert!(installable.len() >= 4, "text HF models stay installable");
        for m in installable {
            assert!(m.hf_repo.is_some(), "{}", m.id);
            assert_ne!(m.family, ModelFamily::Olmoe);
            assert!(m.offers_text_load() || matches!(m.launch, CatalogLaunch::Image));
        }
    }

    #[test]
    fn families_cover_engine_enum() {
        let mut seen = [false; 5];
        for m in supported_models() {
            if !m.offers_text_load() {
                continue;
            }
            let i = match m.family {
                ModelFamily::Glm => 0,
                ModelFamily::Inkling => 1,
                ModelFamily::Kimi => 2,
                ModelFamily::DeepseekV4 => 3,
                ModelFamily::Olmoe => 4,
            };
            seen[i] = true;
        }
        assert!(seen.iter().all(|&x| x), "every ModelFamily in catalog");
    }

    #[test]
    fn setup_hf_repos_are_listed() {
        for repo in [
            "Kreuzzelg/qwen36-35b-a3b-colibri-i4-gs64",
            "Justvugg/Qwen3-Coder-30B-A3B-colibri-int4",
            "puwaer/DeepSeek-V4-Flash-0731-reap-150b",
            "deepseek-ai/DeepSeek-V4-Flash-0731",
            "Qwen/Qwen3.8-Flash-Next-FP8",
            "XiaomiMiMo/MiMo-V2.6-Flash-MOPD",
            "mastouri/GLM-5.2-colibri-int4-g64-with-int8-mtp",
            "Justvugg/GLM-5.3-colibri-int4-g64",
            "XiaomiMiMo/MiMo-V2.6-Pro-MOPD",
            "nbeerbower/Inkling-colibri-int4",
            "moonshotai/Kimi-K3",
            "Qwen/Qwen-Image-2.1",
        ] {
            assert!(
                supported_model_by_hf_repo(repo).is_some(),
                "missing setup repo {repo}"
            );
        }
    }

    #[test]
    fn roster_families_cover_registry() {
        let mut seen = std::collections::BTreeSet::new();
        for m in supported_models() {
            seen.insert(m.roster_family);
        }
        for id in [
            "glm53",
            "glm",
            "inkling",
            "kimi",
            "olmoe",
            "qwen36",
            "qwen38",
            "deepseek_v4",
            "deepseek_v41",
            "mimo",
            "qwen_image",
            "laya",
            "gliner_decide",
        ] {
            assert!(seen.contains(id), "missing roster family {id}");
        }
    }

    #[test]
    fn unstartable_rows_do_not_offer_text_load_or_install() {
        for id in [
            "qwen36-35b",
            "qwen38-flash-next",
            "mimo-v2.6-flash",
            "glm-5.3-flash-colibri",
            "deepseek-v4.1-flash-colibri",
            "laya",
            "gliner2.5-decide",
        ] {
            let m = supported_model_by_id(id).unwrap_or_else(|| panic!("{id}"));
            assert!(!m.offers_text_load(), "{id}");
            assert!(!m.offers_install(), "{id}");
        }
        let image = supported_model_by_id("qwen-image-2.1-colibri").expect("image");
        assert!(!image.offers_text_load());
        assert!(image.offers_install());
        assert_eq!(image.launch, CatalogLaunch::Image);
        let reap = supported_model_by_id("deepseek-v4-flash-reap").expect("reap");
        assert!(reap.offers_text_load());
        assert_eq!(reap.launch, CatalogLaunch::Text(ModelFamily::DeepseekV4));
    }

    #[test]
    fn kind_from_model_type_guards_v41_before_v4() {
        assert_eq!(
            kind_from_model_type("deepseek_v41"),
            ModelDirKind::Unsupported
        );
        assert_eq!(
            kind_from_model_type("deepseek_v4"),
            ModelDirKind::Text(ModelFamily::DeepseekV4)
        );
        assert_eq!(kind_from_model_type("glm5_next"), ModelDirKind::Unsupported);
        assert_eq!(
            kind_from_model_type("glm_moe_dsa"),
            ModelDirKind::Text(ModelFamily::Glm)
        );
        assert_eq!(
            kind_from_model_type("qwen3_5_moe_text"),
            ModelDirKind::Unsupported
        );
        assert_eq!(
            kind_from_model_type("olmoe"),
            ModelDirKind::Text(ModelFamily::Olmoe)
        );
        assert_eq!(
            kind_from_model_type(""),
            ModelDirKind::Text(ModelFamily::Glm)
        );
        assert_eq!(kind_from_model_type("extractor"), ModelDirKind::Decision);
    }

    #[test]
    fn classify_model_dir_image_decision_and_v41() {
        let root =
            std::env::temp_dir().join(format!("colibri-catalog-classify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("image")).unwrap();
        std::fs::write(
            root.join("image/model_index.json"),
            r#"{"_class_name":"QwenImage21Pipeline"}"#,
        )
        .unwrap();
        assert_eq!(
            classify_model_dir(&root.join("image")),
            Some(ModelDirKind::Image)
        );

        std::fs::create_dir_all(root.join("laya")).unwrap();
        std::fs::write(root.join("laya/rl_agent_config.json"), "{}").unwrap();
        assert_eq!(
            classify_model_dir(&root.join("laya")),
            Some(ModelDirKind::Decision)
        );

        std::fs::create_dir_all(root.join("v41")).unwrap();
        std::fs::write(
            root.join("v41/config.json"),
            r#"{"model_type":"deepseek_v41"}"#,
        )
        .unwrap();
        assert_eq!(
            classify_model_dir(&root.join("v41")),
            Some(ModelDirKind::Unsupported)
        );
        assert_eq!(
            text_start_refusal(&root.join("v41")),
            Some("This host cannot start this model family.")
        );
        assert_eq!(
            text_start_refusal(&root.join("image")),
            Some("This folder is an image model. Use image chat, not Start engine.")
        );
        assert!(text_start_refusal(&root.join("missing")).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
