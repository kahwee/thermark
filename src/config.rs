//! User config: default printer address and connection prefs.
//!
//! File location (platform standard):
//! - **macOS:** `~/Library/Application Support/thermark/config.{toml,json}`
//! - **Linux:** `~/.config/thermark/config.{toml,json}`
//! - **Windows:** `%APPDATA%\thermark\config.{toml,json}`
//!
//! Formats: **TOML** (default) or **JSON** (path ends in `.json`, or `config set --format json`).
//!
//! Override path with env `THERMARK_CONFIG`.
//!
//! Address resolution order for CLI:
//! 1. `-a` / `--addr` flag
//! 2. `THERMARK_ADDR` env
//! 3. `addr` in this config file

use crate::errors::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const TOML_HEADER: &str = "\
# thermark user config — do not commit device serials to git
# Override path: THERMARK_CONFIG=/path/to/config.toml
# Override addr only: THERMARK_ADDR=B1-YourPrinter
";

/// On-disk config encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConfigFormat {
    #[default]
    Toml,
    Json,
}

impl ConfigFormat {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "toml" => Some(Self::Toml),
            "json" => Some(Self::Json),
            _ => None,
        }
    }

    pub fn from_path(path: &Path) -> Self {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref()
        {
            Some("json") => Self::Json,
            _ => Self::Toml,
        }
    }

    pub fn file_name(self) -> &'static str {
        match self {
            Self::Toml => "config.toml",
            Self::Json => "config.json",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Toml => "toml",
            Self::Json => "json",
        }
    }
}

impl std::fmt::Display for ConfigFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Preferred link type stored in config / resolved for the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnPref {
    #[default]
    Ble,
    Usb,
}

impl ConnPref {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ble => "ble",
            Self::Usb => "usb",
        }
    }

    /// Parse `"ble"`, `"usb"`, or `"serial"` (alias for usb). Unknown → Ble.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "usb" | "serial" => Self::Usb,
            _ => Self::Ble,
        }
    }
}

impl std::fmt::Display for ConnPref {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// On-disk / in-memory user preferences.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Default BLE name / UUID or serial device path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addr: Option<String>,
    /// `"ble"` or `"usb"` (also accepts `"serial"` when reading).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// Default model string, e.g. `"b1"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Default BLE scan seconds before connect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scan_secs: Option<u64>,
}

impl Config {
    /// Empty config (no saved printer).
    pub fn new() -> Self {
        Self::default()
    }

    /// True when nothing is configured.
    pub fn is_empty(&self) -> bool {
        self.addr.is_none()
            && self.connection.is_none()
            && self.model.is_none()
            && self.scan_secs.is_none()
    }

    /// Config directory (platform standard).
    pub fn config_dir() -> Result<PathBuf> {
        let dirs = directories::ProjectDirs::from("", "", "thermark")
            .ok_or_else(|| Error::msg("could not determine config directory for thermark"))?;
        Ok(dirs.config_dir().to_path_buf())
    }

    /// Path used by the CLI (`THERMARK_CONFIG`, else existing json/toml, else `config.toml`).
    pub fn default_path() -> Result<PathBuf> {
        if let Ok(p) = std::env::var("THERMARK_CONFIG") {
            let p = p.trim();
            if !p.is_empty() {
                return Ok(PathBuf::from(p));
            }
        }
        let dir = Self::config_dir()?;
        let json = dir.join(ConfigFormat::Json.file_name());
        let toml = dir.join(ConfigFormat::Toml.file_name());
        if json.exists() {
            return Ok(json);
        }
        if toml.exists() {
            return Ok(toml);
        }
        Ok(toml)
    }

    /// Default path forced to a format (for `config set --format json`).
    pub fn path_for_format(format: ConfigFormat) -> Result<PathBuf> {
        if let Ok(p) = std::env::var("THERMARK_CONFIG") {
            let p = p.trim();
            if !p.is_empty() {
                let mut path = PathBuf::from(p);
                // Align extension with requested format when env path is used.
                path.set_extension(format.as_str());
                return Ok(path);
            }
        }
        Ok(Self::config_dir()?.join(format.file_name()))
    }

    /// Load from the default path. Missing file → empty config (not an error).
    pub fn load() -> Result<Self> {
        Self::load_from(&Self::default_path()?)
    }

    /// Load from an explicit path. Missing file → empty config.
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(path)
            .map_err(|e| Error::msg(format!("read config {}: {e}", path.display())))?;
        Self::parse_str(&text, ConfigFormat::from_path(path))
            .map_err(|e| Error::msg(format!("parse config {}: {e}", path.display())))
    }

    /// Parse config body (TOML or JSON).
    pub fn parse_str(text: &str, format: ConfigFormat) -> Result<Self> {
        match format {
            ConfigFormat::Toml => {
                toml::from_str(text).map_err(|e| Error::msg(format!("invalid TOML: {e}")))
            }
            ConfigFormat::Json => {
                serde_json::from_str(text).map_err(|e| Error::msg(format!("invalid JSON: {e}")))
            }
        }
    }

    /// Parse TOML body (no file I/O).
    pub fn parse_toml(text: &str) -> Result<Self> {
        Self::parse_str(text, ConfigFormat::Toml)
    }

    /// Parse JSON body (no file I/O).
    pub fn parse_json(text: &str) -> Result<Self> {
        Self::parse_str(text, ConfigFormat::Json)
    }

    /// Serialize for display / file.
    pub fn to_string_pretty(&self, format: ConfigFormat) -> Result<String> {
        match format {
            ConfigFormat::Toml => {
                let text = toml::to_string_pretty(self)
                    .map_err(|e| Error::msg(format!("serialize TOML: {e}")))?;
                Ok(format!("{TOML_HEADER}{text}"))
            }
            ConfigFormat::Json => serde_json::to_string_pretty(self)
                .map_err(|e| Error::msg(format!("serialize JSON: {e}"))),
        }
    }

    /// Write to the default path (creates parent dirs).
    pub fn save(&self) -> Result<PathBuf> {
        let path = Self::default_path()?;
        self.save_to(&path)?;
        Ok(path)
    }

    /// Write using a chosen format under the platform config dir.
    pub fn save_as(&self, format: ConfigFormat) -> Result<PathBuf> {
        let path = Self::path_for_format(format)?;
        self.save_to(&path)?;
        Ok(path)
    }

    /// Write to an explicit path (format from extension: `.json` vs TOML).
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| Error::msg(format!("create config dir {}: {e}", parent.display())))?;
        }
        let body = self.to_string_pretty(ConfigFormat::from_path(path))?;
        fs::write(path, body)
            .map_err(|e| Error::msg(format!("write config {}: {e}", path.display())))?;
        Ok(())
    }

    /// Delete default config file(s). Returns true if anything was removed.
    pub fn clear() -> Result<bool> {
        let mut removed = false;
        if let Ok(p) = std::env::var("THERMARK_CONFIG") {
            let p = PathBuf::from(p.trim());
            if !p.as_os_str().is_empty() {
                return Self::clear_at(&p);
            }
        }
        if let Ok(dir) = Self::config_dir() {
            removed |= Self::clear_at(&dir.join(ConfigFormat::Toml.file_name()))?;
            removed |= Self::clear_at(&dir.join(ConfigFormat::Json.file_name()))?;
        }
        Ok(removed)
    }

    /// Delete config at an explicit path.
    pub fn clear_at(path: &Path) -> Result<bool> {
        if path.exists() {
            fs::remove_file(path)
                .map_err(|e| Error::msg(format!("remove config {}: {e}", path.display())))?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Update fields for `config set` (only overwrites provided values).
    pub fn apply_set(
        &mut self,
        addr: impl Into<String>,
        connection: ConnPref,
        model: Option<String>,
        scan_secs: Option<u64>,
    ) {
        self.addr = Some(addr.into().trim().to_string());
        self.connection = Some(connection.as_str().into());
        if let Some(m) = model {
            let m = m.trim();
            if !m.is_empty() {
                self.model = Some(m.to_string());
            }
        }
        if let Some(s) = scan_secs {
            self.scan_secs = Some(s);
        }
    }

    fn nonempty(s: Option<&str>) -> Option<&str> {
        s.map(str::trim).filter(|s| !s.is_empty())
    }

    /// Resolve printer address: CLI flag → `THERMARK_ADDR` → config.
    pub fn resolve_addr(&self, cli_addr: Option<&str>) -> Result<String> {
        if let Some(a) = Self::nonempty(cli_addr) {
            return Ok(a.to_string());
        }
        if let Ok(a) = std::env::var("THERMARK_ADDR") {
            if let Some(a) = Self::nonempty(Some(a.as_str())) {
                return Ok(a.to_string());
            }
        }
        if let Some(a) = Self::nonempty(self.addr.as_deref()) {
            return Ok(a.to_string());
        }
        Err(Error::msg(
            "no printer address: pass -a \"B1-Name\", set THERMARK_ADDR, \
             or save one with: thermark config set -a \"B1-Name\"",
        ))
    }

    /// Prefer CLI connection when provided; else config; else BLE.
    pub fn resolve_connection(&self, cli: Option<&str>) -> ConnPref {
        if let Some(c) = Self::nonempty(cli) {
            return ConnPref::parse(c);
        }
        self.connection
            .as_deref()
            .map(ConnPref::parse)
            .unwrap_or_default()
    }

    /// Prefer CLI scan seconds when provided; else config; else 4.
    pub fn resolve_scan_secs(&self, cli: Option<u64>) -> u64 {
        cli.or(self.scan_secs).unwrap_or(4).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialize env mutations (resolve_addr uses THERMARK_ADDR).
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_clean_env<T>(f: impl FnOnce() -> T) -> T {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("THERMARK_ADDR");
        std::env::remove_var("THERMARK_CONFIG");
        f()
    }

    #[test]
    fn roundtrip_toml_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let cfg = Config {
            addr: Some("B1-YourPrinter".into()),
            connection: Some("ble".into()),
            model: Some("b1".into()),
            scan_secs: Some(6),
        };
        cfg.save_to(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("thermark user config"));
        assert!(text.contains("B1-YourPrinter"));

        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded, cfg);
    }

    #[test]
    fn missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nope.toml");
        let cfg = Config::load_from(&path).unwrap();
        assert!(cfg.is_empty());
        assert!(cfg.addr.is_none());
    }

    #[test]
    fn invalid_toml_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "addr = [unterminated").unwrap();
        let err = Config::load_from(&path).unwrap_err().to_string();
        assert!(
            err.contains("parse config") || err.contains("TOML"),
            "{err}"
        );
    }

    #[test]
    fn roundtrip_json_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let cfg = Config {
            addr: Some("B1-YourPrinter".into()),
            connection: Some("ble".into()),
            model: Some("b1".into()),
            scan_secs: Some(5),
        };
        cfg.save_to(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.trim_start().starts_with('{'));
        assert!(text.contains("B1-YourPrinter"));
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded, cfg);
    }

    #[test]
    fn invalid_json_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "{not json").unwrap();
        let err = Config::load_from(&path).unwrap_err().to_string();
        assert!(err.contains("parse") || err.contains("JSON"), "{err}");
    }

    #[test]
    fn to_string_pretty_json() {
        let cfg = Config {
            addr: Some("B1-X".into()),
            ..Default::default()
        };
        let s = cfg.to_string_pretty(ConfigFormat::Json).unwrap();
        let back: Config = serde_json::from_str(&s).unwrap();
        assert_eq!(back.addr.as_deref(), Some("B1-X"));
    }

    #[test]
    fn apply_set_merges_without_clobbering_unspecified() {
        let mut cfg = Config {
            addr: Some("old".into()),
            connection: Some("usb".into()),
            model: Some("b21".into()),
            scan_secs: Some(9),
        };
        cfg.apply_set("B1-New", ConnPref::Ble, None, None);
        assert_eq!(cfg.addr.as_deref(), Some("B1-New"));
        assert_eq!(cfg.connection.as_deref(), Some("ble"));
        // model + scan_secs kept when not provided
        assert_eq!(cfg.model.as_deref(), Some("b21"));
        assert_eq!(cfg.scan_secs, Some(9));

        cfg.apply_set("B1-New", ConnPref::Ble, Some("b1".into()), Some(3));
        assert_eq!(cfg.model.as_deref(), Some("b1"));
        assert_eq!(cfg.scan_secs, Some(3));
    }

    #[test]
    fn apply_set_trims_addr() {
        let mut cfg = Config::default();
        cfg.apply_set("  B1-Spaced  ", ConnPref::Ble, None, None);
        assert_eq!(cfg.addr.as_deref(), Some("B1-Spaced"));
    }

    #[test]
    fn resolve_addr_priority() {
        with_clean_env(|| {
            let cfg = Config {
                addr: Some("from-config".into()),
                ..Default::default()
            };
            assert_eq!(cfg.resolve_addr(Some("from-cli")).unwrap(), "from-cli");
            assert_eq!(cfg.resolve_addr(Some("  from-cli  ")).unwrap(), "from-cli");
            assert_eq!(cfg.resolve_addr(None).unwrap(), "from-config");
            assert_eq!(cfg.resolve_addr(Some("")).unwrap(), "from-config");
            assert_eq!(cfg.resolve_addr(Some("   ")).unwrap(), "from-config");

            std::env::set_var("THERMARK_ADDR", "from-env");
            assert_eq!(cfg.resolve_addr(None).unwrap(), "from-env");
            assert_eq!(cfg.resolve_addr(Some("from-cli")).unwrap(), "from-cli");
            std::env::remove_var("THERMARK_ADDR");
        });
    }

    #[test]
    fn resolve_addr_missing_errors() {
        with_clean_env(|| {
            let cfg = Config::default();
            let err = cfg.resolve_addr(None).unwrap_err().to_string();
            assert!(err.contains("config set"), "{err}");
        });
    }

    #[test]
    fn resolve_connection_and_scan() {
        let cfg = Config {
            connection: Some("USB".into()),
            scan_secs: Some(8),
            ..Default::default()
        };
        assert_eq!(cfg.resolve_connection(None), ConnPref::Usb);
        assert_eq!(cfg.resolve_connection(Some("ble")), ConnPref::Ble);
        assert_eq!(cfg.resolve_connection(Some("serial")), ConnPref::Usb);
        assert_eq!(Config::default().resolve_connection(None), ConnPref::Ble);

        assert_eq!(cfg.resolve_scan_secs(None), 8);
        assert_eq!(cfg.resolve_scan_secs(Some(2)), 2);
        assert_eq!(Config::default().resolve_scan_secs(None), 4);
        // floor at 1
        assert_eq!(cfg.resolve_scan_secs(Some(0)), 1);
    }

    #[test]
    fn conn_pref_parse() {
        assert_eq!(ConnPref::parse("ble"), ConnPref::Ble);
        assert_eq!(ConnPref::parse("BLE"), ConnPref::Ble);
        assert_eq!(ConnPref::parse("usb"), ConnPref::Usb);
        assert_eq!(ConnPref::parse("serial"), ConnPref::Usb);
        assert_eq!(ConnPref::parse("???"), ConnPref::Ble);
        assert_eq!(ConnPref::Ble.as_str(), "ble");
        assert_eq!(ConnPref::Usb.to_string(), "usb");
    }

    #[test]
    fn clear_at_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        Config {
            addr: Some("x".into()),
            ..Default::default()
        }
        .save_to(&path)
        .unwrap();
        assert!(path.exists());
        assert!(Config::clear_at(&path).unwrap());
        assert!(!path.exists());
        assert!(!Config::clear_at(&path).unwrap());
    }

    #[test]
    fn default_path_honors_thermark_config_env() {
        with_clean_env(|| {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("custom.toml");
            std::env::set_var("THERMARK_CONFIG", path.as_os_str());
            assert_eq!(Config::default_path().unwrap(), path);
            std::env::remove_var("THERMARK_CONFIG");
        });
    }

    #[test]
    fn parse_toml_empty_object() {
        let cfg = Config::parse_toml("").unwrap();
        assert!(cfg.is_empty());
        let cfg = Config::parse_toml("addr = \"B1-X\"\n").unwrap();
        assert_eq!(cfg.addr.as_deref(), Some("B1-X"));
    }

    #[test]
    fn parse_json_object() {
        let cfg = Config::parse_json(r#"{"addr":"B1-Y","connection":"usb"}"#).unwrap();
        assert_eq!(cfg.addr.as_deref(), Some("B1-Y"));
        assert_eq!(cfg.resolve_connection(None), ConnPref::Usb);
    }

    #[test]
    fn format_from_path() {
        assert_eq!(
            ConfigFormat::from_path(Path::new("x.json")),
            ConfigFormat::Json
        );
        assert_eq!(
            ConfigFormat::from_path(Path::new("x.toml")),
            ConfigFormat::Toml
        );
    }
}
