//! Default filesystem locations for colibri-sys host state.
//!
//! The C product (`c/coli`) requires an explicit `COLI_MODEL` / `--model` and has
//! no single hard-coded model tree. This crate defines a **discoverable default
//! model store** for install/registry/disk probes so hosts can size free space
//! before download without inventing a path each time.
//!
//! Override precedence (first wins):
//! 1. Explicit API argument (`Some(path)` / `ProbeOptions::model_store`), including
//!    values copied from `ColibriConfig::model_store` via
//!    [`crate::MachineInfo::probe_for_config`] / [`crate::ProbeOptions::from_config`]
//! 2. Environment `COLIBRI_MODEL_STORE` or `COLI_MODEL_STORE`
//! 3. Platform data dir: `$XDG_DATA_HOME/colibri/models` or `~/.local/share/colibri/models`
//!    (Windows: `%LOCALAPPDATA%\colibri\models`)
//!
//! `ColibriConfig` is not read inside `resolve_model_store` itself. Hosts pass the
//! config field into probe options (prefer `probe_for_config` so the override is
//! hard to miss).

use std::path::{Path, PathBuf};

/// Environment keys consulted for the model store root (in order).
pub const MODEL_STORE_ENV_KEYS: &[&str] = &["COLIBRI_MODEL_STORE", "COLI_MODEL_STORE"];

/// Expand a leading `~` / `~/` to the current user's home directory.
///
/// - `~` → `$HOME` (or `%USERPROFILE%` on Windows)
/// - `~/foo/bar` → `$HOME/foo/bar`
/// - other paths (including `~otheruser`) are returned unchanged
///
/// Used by doctor, plan, and open so `~/.models` is not treated as a literal
/// relative path under the process cwd.
pub fn expand_user_path(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    let s = path.as_os_str();
    if s.is_empty() {
        return path.to_path_buf();
    }
    // Prefer OsStr prefix checks so non-UTF-8 paths stay untouched.
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let bytes = s.as_bytes();
        if bytes == b"~" {
            return user_home_dir();
        }
        if let Some(rest) = bytes.strip_prefix(b"~/") {
            return user_home_dir().join(Path::new(std::ffi::OsStr::from_bytes(rest)));
        }
        path.to_path_buf()
    }
    #[cfg(not(unix))]
    {
        let lossy = path.to_string_lossy();
        if lossy == "~" {
            return user_home_dir();
        }
        if let Some(rest) = lossy.strip_prefix("~/") {
            return user_home_dir().join(rest);
        }
        if let Some(rest) = lossy.strip_prefix("~\\") {
            return user_home_dir().join(rest);
        }
        path.to_path_buf()
    }
}

/// Home directory for path expansion (`HOME` / `USERPROFILE`, else `.`).
fn user_home_dir() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

/// Resolve the default model store directory (no create).
///
/// See module docs for override order. Does not create the directory.
pub fn default_model_store_path() -> PathBuf {
    for key in MODEL_STORE_ENV_KEYS {
        if let Ok(v) = std::env::var(key) {
            let t = v.trim();
            if !t.is_empty() {
                return PathBuf::from(t);
            }
        }
    }
    platform_default_model_store()
}

/// Outcome of ensuring a model (or store) directory exists on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnsureModelDir {
    /// Path already existed (file or directory).
    AlreadyExists,
    /// Directory (and parents) were created by this call.
    Created,
}

/// Create `path` and its parents when missing.
///
/// **Security / scope:** only call this for the user/app model path or the
/// default model store — never for arbitrary untrusted system paths.
///
/// Returns [`EnsureModelDir::AlreadyExists`] when the path is already present
/// (does not check that it is a directory). Returns
/// [`EnsureModelDir::Created`] after a successful `create_dir_all`.
pub fn ensure_model_directory(path: impl AsRef<Path>) -> std::io::Result<EnsureModelDir> {
    let path = path.as_ref();
    if path.as_os_str().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "empty path",
        ));
    }
    if path.exists() {
        return Ok(EnsureModelDir::AlreadyExists);
    }
    std::fs::create_dir_all(path)?;
    Ok(EnsureModelDir::Created)
}

/// Resolve the default model store and create it when missing.
///
/// Same path rules as [`default_model_store_path`]. Safe for cold-start once;
/// do not call on every keystroke.
pub fn ensure_default_model_store() -> std::io::Result<(PathBuf, EnsureModelDir)> {
    let path = default_model_store_path();
    let outcome = ensure_model_directory(&path)?;
    Ok((path, outcome))
}

/// Native log file name under the log directory.
pub const NATIVE_LOG_FILE_NAME: &str = "native.log";

/// Platform data directory (`$XDG_DATA_HOME/colibri` or `~/.local/share/colibri`).
///
/// No create. Model store is [`platform_default_model_store`]; logs are
/// [`default_log_dir`].
pub fn platform_data_dir() -> PathBuf {
    platform_data_dir_from(
        std::env::var("XDG_DATA_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
        std::env::var("LOCALAPPDATA").ok().as_deref(),
    )
}

/// Resolve the platform data dir from explicit env values (testable).
///
/// Unix: non-empty `xdg_data_home` wins, else `home/.local/share/colibri`,
/// else `./.local/share/colibri`. Windows: `local_app_data\colibri`, else
/// `C:\colibri`.
pub fn platform_data_dir_from(
    xdg_data_home: Option<&str>,
    home: Option<&str>,
    local_app_data: Option<&str>,
) -> PathBuf {
    #[cfg(windows)]
    {
        let _ = (xdg_data_home, home);
        if let Some(base) = local_app_data.map(str::trim).filter(|s| !s.is_empty()) {
            return PathBuf::from(base).join("colibri");
        }
        PathBuf::from(r"C:\colibri")
    }
    #[cfg(not(windows))]
    {
        let _ = local_app_data;
        if let Some(xdg) = xdg_data_home.map(str::trim).filter(|s| !s.is_empty()) {
            return PathBuf::from(xdg).join("colibri");
        }
        let home = home.map(str::trim).filter(|s| !s.is_empty()).unwrap_or(".");
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("colibri")
    }
}

/// Platform XDG / LocalAppData path without env override.
pub fn platform_default_model_store() -> PathBuf {
    platform_data_dir().join("models")
}

/// Default native log directory (`<data>/logs`). No create.
pub fn default_log_dir() -> PathBuf {
    default_log_dir_from(
        std::env::var("XDG_DATA_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
        std::env::var("LOCALAPPDATA").ok().as_deref(),
    )
}

/// Testable log directory from explicit env values.
pub fn default_log_dir_from(
    xdg_data_home: Option<&str>,
    home: Option<&str>,
    local_app_data: Option<&str>,
) -> PathBuf {
    platform_data_dir_from(xdg_data_home, home, local_app_data).join("logs")
}

/// Default native log file (`<data>/logs/native.log`). No create.
pub fn default_native_log_path() -> PathBuf {
    default_native_log_path_from(
        std::env::var("XDG_DATA_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
        std::env::var("LOCALAPPDATA").ok().as_deref(),
    )
}

/// Testable native log path from explicit env values.
pub fn default_native_log_path_from(
    xdg_data_home: Option<&str>,
    home: Option<&str>,
    local_app_data: Option<&str>,
) -> PathBuf {
    default_log_dir_from(xdg_data_home, home, local_app_data).join(NATIVE_LOG_FILE_NAME)
}

/// Create the native log directory (and parents) when missing.
pub fn ensure_log_directory(path: impl AsRef<Path>) -> std::io::Result<EnsureModelDir> {
    ensure_model_directory(path)
}

/// How the model store path was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelStoreSource {
    /// Explicit `Some(path)` on the probe/config API.
    Override,
    /// `COLIBRI_MODEL_STORE` / `COLI_MODEL_STORE`.
    Environment,
    /// XDG / LocalAppData default.
    PlatformDefault,
}

/// Resolve path + source for a probe call.
pub fn resolve_model_store(explicit: Option<&std::path::Path>) -> (PathBuf, ModelStoreSource) {
    if let Some(p) = explicit {
        return (p.to_path_buf(), ModelStoreSource::Override);
    }
    for key in MODEL_STORE_ENV_KEYS {
        if let Ok(v) = std::env::var(key) {
            let t = v.trim();
            if !t.is_empty() {
                return (PathBuf::from(t), ModelStoreSource::Environment);
            }
        }
    }
    (
        platform_default_model_store(),
        ModelStoreSource::PlatformDefault,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_default_is_absolute_or_relative_home() {
        let p = platform_default_model_store();
        assert!(p.components().count() >= 2, "{p:?}");
        assert!(
            p.ends_with("colibri/models")
                || p.ends_with(r"colibri\models")
                || p.to_string_lossy().contains("colibri"),
            "{p:?}"
        );
    }

    #[test]
    fn resolve_override_wins() {
        let (p, src) = resolve_model_store(Some(std::path::Path::new("/tmp/my-models")));
        assert_eq!(p, PathBuf::from("/tmp/my-models"));
        assert_eq!(src, ModelStoreSource::Override);
    }

    #[test]
    fn expand_user_path_tilde_home() {
        let home = user_home_dir();
        let alone = expand_user_path("~");
        assert_eq!(alone, home, "bare ~ expands to home");

        let nested = expand_user_path("~/.models");
        assert_eq!(nested, home.join(".models"));
        assert!(
            !nested.to_string_lossy().starts_with('~'),
            "tilde must not remain literal: {nested:?}"
        );

        let sub = expand_user_path("~/foo/bar");
        assert_eq!(sub, home.join("foo").join("bar"));
    }

    #[test]
    fn expand_user_path_leaves_other_paths() {
        assert_eq!(expand_user_path("/abs/model"), PathBuf::from("/abs/model"));
        assert_eq!(expand_user_path("rel/model"), PathBuf::from("rel/model"));
        // Only bare ~ and ~/…; ~otheruser is left alone.
        assert_eq!(
            expand_user_path("~other/models"),
            PathBuf::from("~other/models")
        );
        assert_eq!(expand_user_path(""), PathBuf::from(""));
    }

    #[test]
    fn ensure_model_directory_creates_missing_path() {
        let root = std::env::temp_dir().join(format!(
            "colibri-ensure-model-dir-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let path = root.join("nested").join("models");
        assert!(!path.exists());
        let outcome = ensure_model_directory(&path).expect("create");
        assert_eq!(outcome, EnsureModelDir::Created);
        assert!(path.is_dir(), "{path:?}");
        let again = ensure_model_directory(&path).expect("already exists");
        assert_eq!(again, EnsureModelDir::AlreadyExists);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ensure_model_directory_fails_when_parent_is_file() {
        let root = std::env::temp_dir().join(format!(
            "colibri-ensure-model-fail-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let blocker = root.join("not-a-dir");
        std::fs::write(&blocker, b"x").unwrap();
        let want = blocker.join("models");
        let err = ensure_model_directory(&want).expect_err("parent is a file");
        assert!(!want.exists());
        assert!(!err.to_string().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ensure_model_directory_rejects_empty_path() {
        let err = ensure_model_directory(Path::new("")).expect_err("empty");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn native_log_path_uses_xdg_data_home() {
        let p = default_native_log_path_from(Some("/tmp/xdg-data"), None, None);
        assert_eq!(p, PathBuf::from("/tmp/xdg-data/colibri/logs/native.log"));
    }

    #[test]
    fn native_log_path_uses_home_local_share() {
        let p = default_native_log_path_from(None, Some("/home/hunter"), None);
        assert_eq!(
            p,
            PathBuf::from("/home/hunter/.local/share/colibri/logs/native.log")
        );
    }

    #[test]
    fn native_log_path_suffix_is_colibri_logs_native() {
        let p = default_native_log_path_from(Some("/var/data"), Some("/home/x"), None);
        assert!(
            p.ends_with("colibri/logs/native.log") || p.ends_with(r"colibri\logs\native.log"),
            "{p:?}"
        );
        assert_eq!(p.file_name().and_then(|s| s.to_str()), Some("native.log"));
    }

    #[test]
    fn ensure_log_directory_creates_missing_path() {
        let root = std::env::temp_dir().join(format!(
            "colibri-ensure-log-dir-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let path = root.join("colibri").join("logs");
        assert!(!path.exists());
        let outcome = ensure_log_directory(&path).expect("create log dir");
        assert_eq!(outcome, EnsureModelDir::Created);
        assert!(path.is_dir(), "{path:?}");
        let again = ensure_log_directory(&path).expect("already exists");
        assert_eq!(again, EnsureModelDir::AlreadyExists);
        let _ = std::fs::remove_dir_all(&root);
    }
}
