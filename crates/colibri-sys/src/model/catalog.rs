//! Static catalog of models Colibri supports.
//!
//! Seeded from the root README supported-models table and engine family routing.
//! This is **not** local disk inventory ([`super::ModelRegistry`]) and not the
//! live `/v1/models` list after a server load. Hosts use this for install
//! pickers and docs-aligned product names.

use super::ModelFamily;

/// One product-supported model entry (static, compile-time).
///
/// `id` matches server-style model ids from `c/openai_server.py` where defined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupportedModel {
    /// Server-style id (e.g. `glm-5.2-colibri`).
    pub id: &'static str,
    /// Plain product name from the README table.
    pub display_name: &'static str,
    /// Engine family for routing / templates.
    pub family: ModelFamily,
    /// Hugging Face `owner/name` when a snapshot install is documented.
    /// `None` when convert-only or no public snapshot in docs.
    pub hf_repo: Option<&'static str>,
    /// Short operational note (e.g. convert-only).
    pub notes: Option<&'static str>,
    /// Disk size hint from README when documented (e.g. `~372 GB`).
    pub disk_hint: Option<&'static str>,
}

/// All models Colibri documents as supported (README family table).
///
/// Order matches the README table: GLM, Inkling, Kimi, DeepSeek V4, OLMoE.
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

const SUPPORTED_MODELS: &[SupportedModel] = &[
    SupportedModel {
        id: "glm-5.2-colibri",
        display_name: "GLM-5.2",
        family: ModelFamily::Glm,
        hf_repo: Some("mastouri/GLM-5.2-colibri-int4-g64-with-int8-mtp"),
        notes: None,
        disk_hint: Some("~372 GB"),
    },
    SupportedModel {
        id: "inkling-colibri",
        display_name: "Inkling",
        family: ModelFamily::Inkling,
        hf_repo: Some("nbeerbower/Inkling-colibri-int4"),
        notes: None,
        disk_hint: Some("~469 GB"),
    },
    SupportedModel {
        id: "kimi-k3-colibri",
        display_name: "Kimi K3",
        family: ModelFamily::Kimi,
        hf_repo: Some("moonshotai/Kimi-K3"),
        notes: Some("original checkpoint; routed experts stay native MXFP4"),
        disk_hint: Some("~1.6 TB"),
    },
    SupportedModel {
        id: "deepseek-v4-colibri",
        display_name: "DeepSeek V4 Flash",
        family: ModelFamily::DeepseekV4,
        // Official HF id from docs/deepseek-v4.md (README table names the
        // checkpoint without a full URL).
        hf_repo: Some("deepseek-ai/DeepSeek-V4-Flash-0731"),
        notes: Some("official sharded checkpoint; native fp4 experts, fp8-e4m3 dense"),
        disk_hint: Some("~167 GB"),
    },
    SupportedModel {
        id: "olmoe-colibri",
        display_name: "OLMoE",
        family: ModelFamily::Olmoe,
        hf_repo: None,
        notes: Some("convert-only: use c/tools/convert_olmoe_merged.py"),
        disk_hint: Some("~4 GB"),
    },
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
    }

    #[test]
    fn installable_entries_have_hf_repo() {
        let installable: Vec<_> = supported_models()
            .iter()
            .filter(|m| m.hf_repo.is_some())
            .collect();
        assert!(installable.len() >= 4, "README lists 4 HF-backed models");
        for m in installable {
            assert_ne!(m.family, ModelFamily::Olmoe);
        }
    }

    #[test]
    fn families_cover_engine_enum() {
        let mut seen = [false; 5];
        for m in supported_models() {
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
}
