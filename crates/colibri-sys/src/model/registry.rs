//! Model registry: scan configured roots, classify family, report status.
//!
//! Upstream has no multi-model DB; the unit of install is a model directory.
//! This registry is host-side inventory on top of that contract.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{ModelFamily, ModelInfo, model_arch};
use crate::error::Result;

/// Completeness / health class for a registered model path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelStatus {
    Present,
    Incomplete,
    MissingTokenizer,
    MissingConfig,
    Unreadable,
}

/// One inventory entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    pub path: PathBuf,
    pub family: ModelFamily,
    /// Engine binary basename for this family.
    #[serde(default)]
    pub engine_id: String,
    pub status: ModelStatus,
    pub model_bytes: u64,
    /// Weight size on disk (bytes); same as `model_bytes` when known.
    #[serde(default)]
    pub disk_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param_count: Option<u64>,
    pub shards: usize,
    pub model_type: Option<String>,
    /// Optional short note (e.g. inspect error).
    pub note: Option<String>,
}

/// Max directory depth under a scan root when looking for `config.json`.
///
/// Root itself is depth 0. Depth 1 is an immediate child (`store/m`); depth 2 is
/// a grandchild (`store/owner/name`). Deeper layouts are ignored.
pub const REGISTRY_SCAN_MAX_DEPTH: u32 = 2;

/// Soft cap on model entries returned from one [`ModelRegistry::refresh`].
pub const REGISTRY_SCAN_MAX_ENTRIES: usize = 64;

/// Scans operator-configured roots for model directories.
#[derive(Debug, Clone, Default)]
pub struct ModelRegistry {
    roots: Vec<PathBuf>,
    entries: Vec<ModelEntry>,
}

impl ModelRegistry {
    pub fn open(roots: impl IntoIterator<Item = impl Into<PathBuf>>) -> Self {
        Self {
            roots: roots.into_iter().map(Into::into).collect(),
            entries: Vec::new(),
        }
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    pub fn entries(&self) -> &[ModelEntry] {
        &self.entries
    }

    /// Add a root directory to scan (does not refresh until [`Self::refresh`]).
    pub fn add_root(&mut self, root: impl Into<PathBuf>) {
        self.roots.push(root.into());
    }

    /// Register a specific model path (inspect immediately).
    pub fn register(&mut self, path: impl AsRef<Path>) -> Result<&ModelEntry> {
        let entry = classify_path(path.as_ref());
        // Replace if same path.
        if let Some(i) = self.entries.iter().position(|e| e.path == entry.path) {
            self.entries[i] = entry;
            return Ok(&self.entries[i]);
        }
        self.entries.push(entry);
        Ok(self.entries.last().unwrap())
    }

    /// Rescan all roots for directories that contain `config.json`.
    ///
    /// Walks each root to depth [`REGISTRY_SCAN_MAX_DEPTH`] (default 2 under the
    /// store). A directory with `config.json` is treated as a model leaf (no
    /// further descent). Cap: [`REGISTRY_SCAN_MAX_ENTRIES`]. Dedupes by path.
    pub fn refresh(&mut self) -> Result<()> {
        let mut found = Vec::new();
        for root in &self.roots {
            if !root.is_dir() {
                continue;
            }
            collect_models(
                root,
                0,
                REGISTRY_SCAN_MAX_DEPTH,
                &mut found,
                REGISTRY_SCAN_MAX_ENTRIES,
            );
            if found.len() >= REGISTRY_SCAN_MAX_ENTRIES {
                break;
            }
        }
        // Dedupe by path.
        found.sort_by(|a, b| a.path.cmp(&b.path));
        found.dedup_by(|a, b| a.path == b.path);
        if found.len() > REGISTRY_SCAN_MAX_ENTRIES {
            found.truncate(REGISTRY_SCAN_MAX_ENTRIES);
        }
        self.entries = found;
        Ok(())
    }

    pub fn find(&self, path: &Path) -> Option<&ModelEntry> {
        self.entries.iter().find(|e| e.path == path)
    }
}

/// Collect model dirs under `dir` (depth from this root). Stop at max depth /
/// entry cap. Dirs with `config.json` are models (no recurse into them).
fn collect_models(
    dir: &Path,
    depth: u32,
    max_depth: u32,
    found: &mut Vec<ModelEntry>,
    max_entries: usize,
) {
    if found.len() >= max_entries {
        return;
    }
    if dir.join("config.json").is_file() {
        found.push(classify_path(dir));
        return;
    }
    if depth >= max_depth {
        return;
    }
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for ent in rd.flatten() {
        if found.len() >= max_entries {
            return;
        }
        let p = ent.path();
        if p.is_dir() {
            collect_models(&p, depth + 1, max_depth, found, max_entries);
        }
    }
}

fn classify_path(path: &Path) -> ModelEntry {
    let path_buf = path.to_path_buf();
    if !path.join("config.json").is_file() {
        return ModelEntry {
            path: path_buf,
            family: ModelFamily::Glm,
            engine_id: ModelFamily::Glm.engine_basename().into(),
            status: ModelStatus::MissingConfig,
            model_bytes: 0,
            disk_bytes: 0,
            param_count: None,
            shards: 0,
            model_type: None,
            note: Some("missing config.json".into()),
        };
    }
    let family = model_arch(path);
    match ModelInfo::inspect(path) {
        Ok(info) => {
            let status = if !info.has_tokenizer {
                ModelStatus::MissingTokenizer
            } else if info.is_complete() {
                ModelStatus::Present
            } else {
                ModelStatus::Incomplete
            };
            let fam = info.family.unwrap_or(family);
            ModelEntry {
                path: info.path,
                family: fam,
                engine_id: if info.engine_id.is_empty() {
                    fam.engine_basename().into()
                } else {
                    info.engine_id
                },
                status,
                model_bytes: info.model_bytes,
                disk_bytes: info.disk_bytes,
                param_count: info.param_count,
                shards: info.shards,
                model_type: info.model_type,
                note: None,
            }
        }
        Err(e) => ModelEntry {
            path: path_buf,
            family,
            engine_id: family.engine_basename().into(),
            status: ModelStatus::Unreadable,
            model_bytes: 0,
            disk_bytes: 0,
            param_count: None,
            shards: 0,
            model_type: None,
            note: Some(e.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn registry_scans_temp_model() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("m1");
        std::fs::create_dir(&model).unwrap();
        let mut f = std::fs::File::create(model.join("config.json")).unwrap();
        f.write_all(br#"{"model_type":"glm_moe_dsa"}"#).unwrap();
        // empty safetensors-less → Unreadable / incomplete on inspect fail
        let mut reg = ModelRegistry::open([dir.path()]);
        reg.refresh().unwrap();
        assert_eq!(reg.entries().len(), 1);
        assert!(matches!(
            reg.entries()[0].status,
            ModelStatus::Unreadable | ModelStatus::Incomplete | ModelStatus::MissingTokenizer
        ));
    }

    #[test]
    fn registry_scans_multiple_temp_dirs() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        for (root, name, mt) in [
            (a.path(), "glm-a", "glm_moe_dsa"),
            (b.path(), "kimi-b", "kimi_k3"),
        ] {
            let model = root.join(name);
            std::fs::create_dir(&model).unwrap();
            let mut f = std::fs::File::create(model.join("config.json")).unwrap();
            write!(f, r#"{{"model_type":"{mt}"}}"#).unwrap();
        }
        // Nested without config is ignored; root-level without config is ignored.
        std::fs::create_dir(a.path().join("empty-skip")).unwrap();

        let mut reg = ModelRegistry::open([a.path(), b.path()]);
        reg.refresh().unwrap();
        assert_eq!(reg.entries().len(), 2);
        let paths: Vec<_> = reg
            .entries()
            .iter()
            .map(|e| e.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(paths.contains(&"glm-a".into()));
        assert!(paths.contains(&"kimi-b".into()));
        assert_eq!(
            reg.find(&a.path().join("glm-a")).unwrap().family.as_str(),
            "glm"
        );
        assert_eq!(
            reg.find(&b.path().join("kimi-b")).unwrap().family.as_str(),
            "kimi"
        );
    }

    #[test]
    fn registry_refresh_clears_stale_entries() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("gone");
        std::fs::create_dir(&model).unwrap();
        std::fs::write(model.join("config.json"), br#"{"model_type":"glm"}"#).unwrap();
        let mut reg = ModelRegistry::open([dir.path()]);
        reg.refresh().unwrap();
        assert_eq!(reg.entries().len(), 1);
        std::fs::remove_dir_all(&model).unwrap();
        reg.refresh().unwrap();
        assert!(reg.entries().is_empty());
    }

    #[test]
    fn registry_scans_depth_one_and_two_under_store() {
        let dir = tempfile::tempdir().unwrap();
        // Depth 1: store/m/config.json
        let m = dir.path().join("m");
        std::fs::create_dir(&m).unwrap();
        std::fs::write(m.join("config.json"), br#"{"model_type":"glm_moe_dsa"}"#).unwrap();
        // Depth 2: store/owner/name/config.json
        let nested = dir.path().join("owner").join("name");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            nested.join("config.json"),
            br#"{"model_type":"glm_moe_dsa"}"#,
        )
        .unwrap();
        // Junk without config ignored
        std::fs::create_dir(dir.path().join("junk")).unwrap();
        // Depth 3: store/a/b/c/config.json must not be found
        let deep = dir.path().join("a").join("b").join("c");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("config.json"), br#"{"model_type":"glm"}"#).unwrap();

        let mut reg = ModelRegistry::open([dir.path()]);
        reg.refresh().unwrap();
        assert_eq!(reg.entries().len(), 2, "{:?}", reg.entries());
        let paths: Vec<_> = reg.entries().iter().map(|e| e.path.clone()).collect();
        assert!(paths.contains(&m), "{paths:?}");
        assert!(paths.contains(&nested), "{paths:?}");
        assert!(!paths.iter().any(|p| p.ends_with("c")), "{paths:?}");
    }
}
