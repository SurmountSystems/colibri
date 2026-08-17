//! Native UI preferences: first-run gate, theme, locale, last model path.
//!
//! Primary file is TOML at `~/.config/colibri/native-ui.toml` (XDG config home),
//! or `%LOCALAPPDATA%\colibri\native-ui.toml` on Windows (same colibri folder
//! family as the model store). Saves always write TOML. If only a legacy
//! `native-ui.json` exists in that directory, load accepts it (same field
//! names); the next save creates TOML and leaves JSON in place. Missing or
//! corrupt files load defaults.
//!
//! Load order: valid `native-ui.toml` if present, else valid `native-ui.json`
//! in the same directory, else defaults. Then env overrides (`COLIBRI_THEME`).
//!
//! Environment:
//! - `COLIBRI_THEME` — overrides theme after load (`doge` | `mint`; unknown → doge)
//! - `COLIBRI_SKIP_WIZARD=1` — suppress first-run wizard without writing the file
//!
//! Public API is for wizard / Tools slices; unit tests cover behavior until then.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// TOML file name under the colibri config directory (primary write path).
pub const PREFS_FILE_NAME: &str = "native-ui.toml";
/// Legacy JSON file name (read-only compatibility when TOML is absent or invalid).
pub const PREFS_JSON_FILE_NAME: &str = "native-ui.json";
/// Schema version written on save.
pub const PREFS_VERSION: u32 = 1;
/// Env key that overrides the theme preference.
pub const THEME_ENV: &str = "COLIBRI_THEME";
/// Env key that skips the first-run wizard when set truthy (`1` / `true` / `yes`).
pub const SKIP_WIZARD_ENV: &str = "COLIBRI_SKIP_WIZARD";

/// Theme id persisted in native UI prefs (`"doge"` | `"mint"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePref {
    /// DOGE palette (product default).
    #[default]
    Doge,
    /// Mint SPA-family palette.
    Mint,
}

impl ThemePref {
    /// Parse a theme id. Empty and unknown values become [`ThemePref::Doge`].
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "mint" => Self::Mint,
            "doge" | "" => Self::Doge,
            _ => Self::Doge,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Doge => "doge",
            Self::Mint => "mint",
        }
    }
}

/// Locale id persisted in native UI prefs (`"en"` | `"it"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LocalePref {
    #[default]
    En,
    It,
}

impl LocalePref {
    /// Parse a locale code. Unknown values become English.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "it" => Self::It,
            _ => Self::En,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::It => "it",
        }
    }
}

/// User preferences for the native shell (TOML on disk; JSON load compatible).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NativePrefs {
    pub version: u32,
    pub first_run_done: bool,
    pub theme: ThemePref,
    pub locale: LocalePref,
    /// Optional absolute model path; empty means unset.
    pub last_model_path: String,
}

impl Default for NativePrefs {
    fn default() -> Self {
        Self {
            version: PREFS_VERSION,
            first_run_done: false,
            theme: ThemePref::Doge,
            locale: LocalePref::En,
            last_model_path: String::new(),
        }
    }
}

/// Loose on-disk shape so unknown theme/locale strings do not fail the whole file.
/// Shared by TOML and JSON (same field names).
#[derive(Debug, Deserialize)]
struct RawNativePrefs {
    version: Option<u32>,
    first_run_done: Option<bool>,
    theme: Option<String>,
    locale: Option<String>,
    last_model_path: Option<String>,
}

impl From<RawNativePrefs> for NativePrefs {
    fn from(raw: RawNativePrefs) -> Self {
        Self {
            version: raw.version.unwrap_or(PREFS_VERSION),
            first_run_done: raw.first_run_done.unwrap_or(false),
            theme: raw
                .theme
                .as_deref()
                .map(ThemePref::parse)
                .unwrap_or_default(),
            locale: raw
                .locale
                .as_deref()
                .map(LocalePref::parse)
                .unwrap_or_default(),
            last_model_path: raw.last_model_path.unwrap_or_default(),
        }
    }
}

/// Platform config path without env override: XDG config or LocalAppData.
pub fn platform_default_prefs_path() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(base) = std::env::var("LOCALAPPDATA") {
            let t = base.trim();
            if !t.is_empty() {
                return PathBuf::from(t).join("colibri").join(PREFS_FILE_NAME);
            }
        }
        return PathBuf::from(r"C:\colibri").join(PREFS_FILE_NAME);
    }
    #[cfg(not(windows))]
    {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            let t = xdg.trim();
            if !t.is_empty() {
                return PathBuf::from(t).join("colibri").join(PREFS_FILE_NAME);
            }
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home)
            .join(".config")
            .join("colibri")
            .join(PREFS_FILE_NAME)
    }
}

/// Default prefs path used by [`load`] / [`save`] (always the TOML path).
pub fn default_prefs_path() -> PathBuf {
    platform_default_prefs_path()
}

/// Sibling legacy JSON path next to a TOML prefs path.
pub fn json_prefs_path_beside(toml_path: &Path) -> PathBuf {
    match toml_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(PREFS_JSON_FILE_NAME),
        _ => PathBuf::from(PREFS_JSON_FILE_NAME),
    }
}

/// Truthy values for `COLIBRI_SKIP_WIZARD` (and similar flags).
pub fn is_skip_wizard_value(s: &str) -> bool {
    let t = s.trim();
    t == "1" || t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("yes")
}

/// Whether the process env asks to skip the first-run wizard.
pub fn skip_wizard_from_env() -> bool {
    std::env::var(SKIP_WIZARD_ENV)
        .ok()
        .as_deref()
        .is_some_and(is_skip_wizard_value)
}

impl NativePrefs {
    /// Apply `COLIBRI_THEME` when set (non-empty). Unknown → doge.
    pub fn apply_env_overrides(&mut self) {
        if let Ok(v) = std::env::var(THEME_ENV) {
            let t = v.trim();
            if !t.is_empty() {
                self.theme = ThemePref::parse(t);
            }
        }
    }

    /// Whether the setup wizard should open for this prefs snapshot and skip env.
    ///
    /// Pure helper for tests and hosts that already resolved the env flag.
    pub fn should_show_wizard_with_skip(&self, skip_env: bool) -> bool {
        !self.first_run_done && !skip_env
    }

    /// Product gate: first-run and `COLIBRI_SKIP_WIZARD`.
    pub fn should_show_wizard(&self) -> bool {
        self.should_show_wizard_with_skip(skip_wizard_from_env())
    }

    /// Write this prefs snapshot as TOML to `path` (creates parent dirs).
    /// Always TOML; does not delete a sibling `native-ui.json` if present.
    pub fn save_to_path(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let text = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        fs::write(path, text)
    }

    /// Write TOML to the platform default prefs path.
    pub fn save(&self) -> std::io::Result<()> {
        self.save_to_path(&default_prefs_path())
    }
}

fn parse_toml_text(text: &str) -> Option<NativePrefs> {
    toml::from_str::<RawNativePrefs>(text)
        .ok()
        .map(NativePrefs::from)
}

fn parse_json_text(text: &str) -> Option<NativePrefs> {
    serde_json::from_str::<RawNativePrefs>(text)
        .ok()
        .map(NativePrefs::from)
}

fn try_load_toml_file(path: &Path) -> Option<NativePrefs> {
    let text = fs::read_to_string(path).ok()?;
    parse_toml_text(&text)
}

fn try_load_json_file(path: &Path) -> Option<NativePrefs> {
    let text = fs::read_to_string(path).ok()?;
    parse_json_text(&text)
}

/// Load prefs from the config directory that owns `toml_path`.
///
/// Order:
/// 1. Prefer `native-ui.toml` at `toml_path` when present and valid.
/// 2. Else prefer sibling `native-ui.json` when present and valid.
/// 3. Else defaults.
///
/// Does **not** apply env overrides.
pub fn load_from_path(toml_path: &Path) -> NativePrefs {
    if toml_path.is_file() {
        if let Some(prefs) = try_load_toml_file(toml_path) {
            return prefs;
        }
        // Present but corrupt: fall through to JSON, then defaults.
    }
    let json_path = json_prefs_path_beside(toml_path);
    if json_path.is_file() {
        if let Some(prefs) = try_load_json_file(&json_path) {
            return prefs;
        }
    }
    NativePrefs::default()
}

/// Load from the platform default path and apply env overrides (`COLIBRI_THEME`).
pub fn load() -> NativePrefs {
    let mut prefs = load_from_path(&default_prefs_path());
    prefs.apply_env_overrides();
    prefs
}

/// Ensure `native-ui.toml` exists at `path` (writes defaults, or values from a
/// sibling JSON file when only JSON is present). Does not overwrite an existing
/// TOML file. Returns `true` when this call wrote the file.
pub fn ensure_prefs_file_if_missing(path: &Path) -> std::io::Result<bool> {
    if path.is_file() {
        return Ok(false);
    }
    // Prefer values already loadable (JSON sibling or defaults), then write TOML.
    let prefs = load_from_path(path);
    prefs.save_to_path(path)?;
    Ok(true)
}

/// Ensure the platform default `native-ui.toml` exists (best effort for doctor).
///
/// Returns `true` when this call wrote the file. Errors are returned to the
/// caller; hosts may ignore them so doctor still reports the model path.
pub fn ensure_default_prefs_file() -> std::io::Result<bool> {
    ensure_prefs_file_if_missing(&default_prefs_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no-such-native-ui.toml");
        let prefs = load_from_path(&path);
        assert_eq!(prefs, NativePrefs::default());
        assert_eq!(prefs.version, PREFS_VERSION);
        assert!(!prefs.first_run_done);
        assert_eq!(prefs.theme, ThemePref::Doge);
        assert_eq!(prefs.locale, LocalePref::En);
        assert!(prefs.last_model_path.is_empty());
    }

    #[test]
    fn defaults_when_file_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PREFS_FILE_NAME);
        fs::write(&path, "this is not { valid toml [[[").unwrap();
        let prefs = load_from_path(&path);
        assert_eq!(prefs, NativePrefs::default());
    }

    #[test]
    fn round_trip_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("colibri").join(PREFS_FILE_NAME);
        let original = NativePrefs {
            version: PREFS_VERSION,
            first_run_done: true,
            theme: ThemePref::Mint,
            locale: LocalePref::It,
            last_model_path: "/models/demo".into(),
        };
        original.save_to_path(&path).unwrap();
        assert!(path.is_file());
        let loaded = load_from_path(&path);
        assert_eq!(loaded, original);
    }

    #[test]
    fn unknown_theme_becomes_doge() {
        assert_eq!(ThemePref::parse("neon"), ThemePref::Doge);
        assert_eq!(ThemePref::parse("DOGE"), ThemePref::Doge);
        assert_eq!(ThemePref::parse("mint"), ThemePref::Mint);
        assert_eq!(ThemePref::parse("  Mint  "), ThemePref::Mint);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PREFS_FILE_NAME);
        fs::write(
            &path,
            r#"
version = 1
first_run_done = true
theme = "neon"
locale = "en"
last_model_path = "/x"
"#,
        )
        .unwrap();
        let prefs = load_from_path(&path);
        assert_eq!(prefs.theme, ThemePref::Doge);
        assert!(prefs.first_run_done);
        assert_eq!(prefs.last_model_path, "/x");
    }

    #[test]
    fn skip_wizard_env_parse_and_gate() {
        assert!(is_skip_wizard_value("1"));
        assert!(is_skip_wizard_value(" true "));
        assert!(is_skip_wizard_value("YES"));
        assert!(!is_skip_wizard_value("0"));
        assert!(!is_skip_wizard_value("false"));
        assert!(!is_skip_wizard_value(""));

        let fresh = NativePrefs::default();
        assert!(fresh.should_show_wizard_with_skip(false));
        assert!(
            !fresh.should_show_wizard_with_skip(true),
            "COLIBRI_SKIP_WIZARD=1 suppresses wizard"
        );

        let done = NativePrefs {
            first_run_done: true,
            ..Default::default()
        };
        assert!(!done.should_show_wizard_with_skip(false));
        assert!(!done.should_show_wizard_with_skip(true));
    }

    #[test]
    fn theme_env_override_via_parse() {
        // Product path: apply_env_overrides uses ThemePref::parse on COLIBRI_THEME.
        let mint = NativePrefs {
            theme: ThemePref::parse("mint"),
            ..Default::default()
        };
        assert_eq!(mint.theme, ThemePref::Mint);
        let unknown = NativePrefs {
            theme: ThemePref::parse("unknown-theme"),
            ..Default::default()
        };
        assert_eq!(unknown.theme, ThemePref::Doge);
    }

    #[test]
    fn locale_parse_and_unknown() {
        assert_eq!(LocalePref::parse("it"), LocalePref::It);
        assert_eq!(LocalePref::parse("EN"), LocalePref::En);
        assert_eq!(LocalePref::parse("fr"), LocalePref::En);
    }

    #[test]
    fn platform_path_contains_colibri_and_filename() {
        let p = platform_default_prefs_path();
        let s = p.to_string_lossy();
        assert!(s.contains("colibri") && s.contains(PREFS_FILE_NAME), "{s}");
    }

    #[test]
    fn partial_toml_fills_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PREFS_FILE_NAME);
        fs::write(&path, "first_run_done = true\n").unwrap();
        let prefs = load_from_path(&path);
        assert!(prefs.first_run_done);
        assert_eq!(prefs.theme, ThemePref::Doge);
        assert_eq!(prefs.version, PREFS_VERSION);
    }

    #[test]
    fn load_json_prefs_when_no_toml() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(PREFS_FILE_NAME);
        let json_path = dir.path().join(PREFS_JSON_FILE_NAME);
        fs::write(
            &json_path,
            r#"{
  "version": 1,
  "first_run_done": true,
  "theme": "mint",
  "locale": "it",
  "last_model_path": "/models/from-json"
}"#,
        )
        .unwrap();
        assert!(!toml_path.exists());
        let prefs = load_from_path(&toml_path);
        assert!(prefs.first_run_done);
        assert_eq!(prefs.theme, ThemePref::Mint);
        assert_eq!(prefs.locale, LocalePref::It);
        assert_eq!(prefs.last_model_path, "/models/from-json");
        assert_eq!(prefs.version, PREFS_VERSION);
    }

    #[test]
    fn load_toml_prefs_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(PREFS_FILE_NAME);
        fs::write(
            &toml_path,
            r#"
version = 1
first_run_done = true
theme = "mint"
locale = "en"
last_model_path = "/models/from-toml"
"#,
        )
        .unwrap();
        let prefs = load_from_path(&toml_path);
        assert!(prefs.first_run_done);
        assert_eq!(prefs.theme, ThemePref::Mint);
        assert_eq!(prefs.last_model_path, "/models/from-toml");
    }

    #[test]
    fn both_present_toml_wins_over_json() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(PREFS_FILE_NAME);
        let json_path = dir.path().join(PREFS_JSON_FILE_NAME);
        fs::write(
            &toml_path,
            r#"
version = 1
first_run_done = true
theme = "doge"
locale = "en"
last_model_path = "/from-toml"
"#,
        )
        .unwrap();
        fs::write(
            &json_path,
            r#"{
  "version": 1,
  "first_run_done": true,
  "theme": "mint",
  "locale": "it",
  "last_model_path": "/from-json"
}"#,
        )
        .unwrap();
        let prefs = load_from_path(&toml_path);
        assert_eq!(prefs.theme, ThemePref::Doge);
        assert_eq!(prefs.locale, LocalePref::En);
        assert_eq!(prefs.last_model_path, "/from-toml");
    }

    #[test]
    fn corrupt_toml_falls_back_to_json() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(PREFS_FILE_NAME);
        let json_path = dir.path().join(PREFS_JSON_FILE_NAME);
        fs::write(&toml_path, "not { valid [[[ toml").unwrap();
        fs::write(
            &json_path,
            r#"{"first_run_done": true, "theme": "mint", "last_model_path": "/json-fallback"}"#,
        )
        .unwrap();
        let prefs = load_from_path(&toml_path);
        assert!(prefs.first_run_done);
        assert_eq!(prefs.theme, ThemePref::Mint);
        assert_eq!(prefs.last_model_path, "/json-fallback");
    }

    #[test]
    fn save_always_writes_toml_and_leaves_json() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(PREFS_FILE_NAME);
        let json_path = dir.path().join(PREFS_JSON_FILE_NAME);
        fs::write(
            &json_path,
            r#"{
  "version": 1,
  "first_run_done": true,
  "theme": "mint",
  "locale": "it",
  "last_model_path": "/from-json"
}"#,
        )
        .unwrap();
        let mut prefs = load_from_path(&toml_path);
        assert_eq!(prefs.theme, ThemePref::Mint);
        prefs.theme = ThemePref::Doge;
        prefs.save_to_path(&toml_path).unwrap();

        assert!(toml_path.is_file(), "save must create native-ui.toml");
        let text = fs::read_to_string(&toml_path).unwrap();
        assert!(
            text.contains("theme") && text.contains("doge"),
            "saved body should be TOML, got: {text}"
        );
        assert!(
            !text.trim_start().starts_with('{'),
            "save must not write JSON: {text}"
        );
        assert!(
            json_path.is_file(),
            "legacy JSON should remain after TOML save"
        );
        let reloaded = load_from_path(&toml_path);
        assert_eq!(reloaded.theme, ThemePref::Doge);
        assert_eq!(reloaded.last_model_path, "/from-json");
    }

    #[test]
    fn ensure_prefs_file_if_missing_writes_toml_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("colibri").join(PREFS_FILE_NAME);
        assert!(!path.exists());
        assert!(ensure_prefs_file_if_missing(&path).unwrap());
        assert!(path.is_file(), "must create native-ui.toml");
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("first_run_done") || text.contains("version"),
            "expected default prefs TOML, got: {text}"
        );
        assert!(!text.trim_start().starts_with('{'), "must be TOML not JSON");
        // Second call does not rewrite (idempotent).
        assert!(!ensure_prefs_file_if_missing(&path).unwrap());
    }

    #[test]
    fn ensure_prefs_file_if_missing_promotes_json_values() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(PREFS_FILE_NAME);
        let json_path = dir.path().join(PREFS_JSON_FILE_NAME);
        fs::write(
            &json_path,
            r#"{"first_run_done": true, "theme": "mint", "last_model_path": "/from-json"}"#,
        )
        .unwrap();
        assert!(ensure_prefs_file_if_missing(&toml_path).unwrap());
        let loaded = load_from_path(&toml_path);
        assert!(loaded.first_run_done);
        assert_eq!(loaded.theme, ThemePref::Mint);
        assert_eq!(loaded.last_model_path, "/from-json");
    }

    #[test]
    fn public_api_surface_smoke() {
        // Keep product entry points live until wizard/Tools wire them at startup.
        assert_eq!(THEME_ENV, "COLIBRI_THEME");
        assert_eq!(SKIP_WIZARD_ENV, "COLIBRI_SKIP_WIZARD");
        assert_eq!(PREFS_JSON_FILE_NAME, "native-ui.json");
        assert_eq!(ThemePref::Doge.as_str(), "doge");
        assert_eq!(ThemePref::Mint.as_str(), "mint");
        assert_eq!(LocalePref::En.as_str(), "en");
        assert_eq!(LocalePref::It.as_str(), "it");

        let _: fn() -> PathBuf = default_prefs_path;
        let _: fn() -> bool = skip_wizard_from_env;
        let _: fn() -> NativePrefs = load;
        let _: fn(&NativePrefs) -> std::io::Result<()> = NativePrefs::save;
        let _: fn(&Path) -> std::io::Result<bool> = ensure_prefs_file_if_missing;

        let mut prefs = NativePrefs::default();
        prefs.apply_env_overrides();
        let _ = prefs.should_show_wizard();
        let _ = skip_wizard_from_env();
        let _ = default_prefs_path();
        // load() only reads; missing file → defaults (may pick up real env theme).
        let loaded = load();
        assert_eq!(loaded.version, PREFS_VERSION);
    }
}
