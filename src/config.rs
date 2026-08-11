//! User config: default printer address and connection prefs (**JSON only**).
//!
//! File location (platform standard):
//! - **macOS:** `~/Library/Application Support/thermark/config.json`
//! - **Linux:** `~/.config/thermark/config.json`
//! - **Windows:** `%APPDATA%\thermark\config.json`
//!
//! Override path with env `THERMARK_CONFIG`.
//!
//! Address resolution order for CLI:
//! 1. `-a` / `--addr` flag
//! 2. `THERMARK_ADDR` env
//! 3. `addr` in this config file

use crate::errors::{Error, Result};
use crate::geometry::SafeArea;

/// Fallback label size when neither the CLI nor the config names one.
pub const DEFAULT_LABEL: &str = "50x30";
use crate::protocol::Model;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Preferred link type stored in config / resolved for the CLI.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Hash, clap::ValueEnum, Serialize, Deserialize,
)]
#[value(rename_all = "lower")]
#[serde(rename_all = "lowercase")]
pub enum ConnPref {
    #[default]
    Ble,
    #[serde(alias = "serial")]
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
        f.pad(self.as_str())
    }
}

/// On-disk / in-memory user preferences (JSON).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Default BLE name / UUID or serial device path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addr: Option<String>,
    /// Link type (`ble` / `usb`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<ConnPref>,
    /// Default model (`b1`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<Model>,
    /// Default BLE scan seconds before connect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scan_secs: Option<u64>,
    /// Content/registration insets for this printer + media, in pixels.
    /// Set it from `thermark calibrate`; see `thermark config safe-area`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_area: Option<SafeArea>,
    /// Default label size, e.g. `"50x30"`. Saves repeating `--label` on media
    /// you use every day.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
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
            && self.safe_area.is_none()
            && self.label.is_none()
    }

    /// Config directory (platform standard).
    pub fn config_dir() -> Result<PathBuf> {
        let dirs = directories::ProjectDirs::from("", "", "thermark")
            .ok_or_else(|| Error::msg("could not determine config directory for thermark"))?;
        Ok(dirs.config_dir().to_path_buf())
    }

    /// Path used by the CLI (`THERMARK_CONFIG` or `…/config.json`).
    pub fn default_path() -> Result<PathBuf> {
        let env = std::env::var("THERMARK_CONFIG").ok();
        Self::default_path_with(env.as_deref())
    }

    /// Resolve the config path from an explicit override.
    ///
    /// Separated from the environment read so it is testable without
    /// `set_var`, which is `unsafe` in edition 2024.
    pub fn default_path_with(env: Option<&str>) -> Result<PathBuf> {
        if let Some(p) = Self::nonempty(env) {
            return Ok(PathBuf::from(p));
        }
        Ok(Self::config_dir()?.join("config.json"))
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
        Self::parse_json(&text)
            .map_err(|e| Error::msg(format!("parse config {}: {e}", path.display())))
    }

    /// Parse JSON body (no file I/O).
    pub fn parse_json(text: &str) -> Result<Self> {
        serde_json::from_str(text).map_err(|e| Error::msg(format!("invalid JSON: {e}")))
    }

    /// Pretty-print JSON for display / file.
    pub fn to_json_pretty(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| Error::msg(format!("serialize JSON: {e}")))
    }

    /// Write to the default path (creates parent dirs).
    pub fn save(&self) -> Result<PathBuf> {
        let path = Self::default_path()?;
        self.save_to(&path)?;
        Ok(path)
    }

    /// Write JSON to an explicit path.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| Error::msg(format!("create config dir {}: {e}", parent.display())))?;
        }
        let body = format!("{}\n", self.to_json_pretty()?);
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        // A unique sibling avoids concurrent writers sharing `config.json.tmp`.
        // Persisting within the same directory keeps the final rename atomic.
        let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(|e| {
            Error::msg(format!(
                "create temporary config in {}: {e}",
                parent.display()
            ))
        })?;
        tmp.write_all(body.as_bytes())
            .map_err(|e| Error::msg(format!("write temporary config: {e}")))?;
        tmp.as_file()
            .sync_all()
            .map_err(|e| Error::msg(format!("flush temporary config: {e}")))?;
        tmp.persist(path)
            .map_err(|e| Error::msg(format!("replace config {}: {}", path.display(), e.error)))?;
        Ok(())
    }

    /// Delete the default config file if it exists.
    pub fn clear() -> Result<bool> {
        Self::clear_at(&Self::default_path()?)
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
        model: Option<Model>,
        scan_secs: Option<u64>,
    ) {
        self.addr = Some(addr.into().trim().to_string());
        self.connection = Some(connection);
        if let Some(m) = model {
            self.model = Some(m);
        }
        if let Some(s) = scan_secs {
            self.scan_secs = Some(s);
        }
    }

    fn nonempty(s: Option<&str>) -> Option<&str> {
        s.map(str::trim).filter(|s| !s.is_empty())
    }

    /// Resolve printer address from explicit inputs.
    ///
    /// Takes the environment value rather than reading it, so tests need not
    /// mutate process-wide state. `std::env::set_var` is `unsafe` in edition
    /// 2024 precisely because another thread reading the environment at the
    /// same time is undefined behaviour — and the test suite runs in parallel
    /// with other tests that read it.
    pub fn resolve_addr_with(&self, cli: Option<&str>, env: Option<&str>) -> Result<String> {
        if let Some(a) = Self::nonempty(cli) {
            return Ok(a.to_string());
        }
        if let Some(a) = Self::nonempty(env) {
            return Ok(a.to_string());
        }
        if let Some(a) = Self::nonempty(self.addr.as_deref()) {
            return Ok(a.to_string());
        }
        Err(Error::msg(
            "no printer address: pass -a \"B1-YourPrinter\" (full name), set THERMARK_ADDR, \
             or: thermark scan --save / thermark config set -a \"B1-YourPrinter\"",
        ))
    }

    /// Resolve printer address: CLI flag → `THERMARK_ADDR` → config.
    ///
    /// Reads the environment here, at the edge, and defers the logic to
    /// [`Self::resolve_addr_with`] so it stays testable without `set_var`.
    pub fn resolve_addr(&self, cli_addr: Option<&str>) -> Result<String> {
        let env = std::env::var("THERMARK_ADDR").ok();
        self.resolve_addr_with(cli_addr, env.as_deref())
    }

    /// Prefer CLI connection when provided; else config; else BLE.
    pub fn resolve_connection(&self, cli: Option<ConnPref>) -> ConnPref {
        cli.or(self.connection).unwrap_or_default()
    }

    /// Prefer CLI model when provided; else config; else [`Model::B1`].
    pub fn resolve_model(&self, cli: Option<Model>) -> Model {
        cli.or(self.model).unwrap_or_default()
    }

    /// Label size: CLI flag, else saved default, else 50x30.
    pub fn resolve_label(&self, cli: Option<&str>) -> String {
        Self::nonempty(cli)
            .map(str::to_string)
            .or_else(|| Self::nonempty(self.label.as_deref()).map(str::to_string))
            .unwrap_or_else(|| DEFAULT_LABEL.to_string())
    }

    /// Measured safe area if one was saved, else the built-in default.
    pub fn resolve_safe_area(&self) -> SafeArea {
        self.safe_area.unwrap_or_default()
    }

    /// Prefer CLI scan seconds when provided; else config; else 4.
    pub fn resolve_scan_secs(&self, cli: Option<u64>) -> u64 {
        cli.or(self.scan_secs).unwrap_or(4).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_json_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let cfg = Config {
            addr: Some("B1-YourPrinter".into()),
            connection: Some(ConnPref::Ble),
            model: Some(Model::B1),
            scan_secs: Some(6),
            safe_area: None,
            label: None,
        };
        cfg.save_to(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.trim_start().starts_with('{'));
        assert!(text.contains("B1-YourPrinter"));
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded, cfg);
    }

    #[test]
    fn missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nope.json");
        let cfg = Config::load_from(&path).unwrap();
        assert!(cfg.is_empty());
    }

    #[test]
    fn concurrent_saves_use_distinct_atomic_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = std::sync::Arc::new(dir.path().join("config.json"));
        let threads: Vec<_> = (0..8)
            .map(|n| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let cfg = Config {
                        addr: Some(format!("B1-{n}")),
                        ..Config::default()
                    };
                    cfg.save_to(&path)
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap().unwrap();
        }
        let loaded = Config::load_from(&path).unwrap();
        assert!(loaded.addr.unwrap().starts_with("B1-"));
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
    fn apply_set_merges_without_clobbering_unspecified() {
        let mut cfg = Config {
            addr: Some("old".into()),
            connection: Some(ConnPref::Usb),
            model: Some(Model::B21),
            scan_secs: Some(9),
            safe_area: None,
            label: None,
        };
        cfg.apply_set("B1-New", ConnPref::Ble, None, None);
        assert_eq!(cfg.addr.as_deref(), Some("B1-New"));
        assert_eq!(cfg.connection, Some(ConnPref::Ble));
        assert_eq!(cfg.model, Some(Model::B21));
        assert_eq!(cfg.scan_secs, Some(9));
    }

    #[test]
    fn apply_set_trims_addr() {
        let mut cfg = Config::default();
        cfg.apply_set("  B1-Spaced  ", ConnPref::Ble, None, None);
        assert_eq!(cfg.addr.as_deref(), Some("B1-Spaced"));
    }

    #[test]
    fn resolve_addr_priority() {
        // No `set_var`: the env value is a parameter, so this cannot race with
        // any other test that reads the environment.
        let cfg = Config {
            addr: Some("from-config".into()),
            ..Default::default()
        };
        assert_eq!(
            cfg.resolve_addr_with(Some("from-cli"), Some("from-env"))
                .unwrap(),
            "from-cli"
        );
        assert_eq!(
            cfg.resolve_addr_with(None, Some("from-env")).unwrap(),
            "from-env"
        );
        assert_eq!(cfg.resolve_addr_with(None, None).unwrap(), "from-config");
        // Blank values fall through instead of resolving to an empty address.
        assert_eq!(
            cfg.resolve_addr_with(Some("  "), Some("  ")).unwrap(),
            "from-config"
        );
    }

    #[test]
    fn resolve_addr_missing_errors() {
        let err = Config::default()
            .resolve_addr_with(None, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("config set"), "{err}");
    }

    #[test]
    fn resolve_label_prefers_cli_then_config_then_default() {
        let mut cfg = Config::default();
        assert_eq!(cfg.resolve_label(None), DEFAULT_LABEL);
        assert_eq!(cfg.resolve_label(Some("40x20")), "40x20");

        cfg.label = Some("30x15".into());
        assert_eq!(cfg.resolve_label(None), "30x15");
        assert_eq!(cfg.resolve_label(Some("40x20")), "40x20");
        // Blank flags fall through rather than producing an empty size.
        assert_eq!(cfg.resolve_label(Some("   ")), "30x15");
    }

    #[test]
    fn resolve_connection_and_scan() {
        let cfg = Config {
            connection: Some(ConnPref::Usb),
            scan_secs: Some(8),
            model: Some(Model::B21),
            ..Default::default()
        };
        assert_eq!(cfg.resolve_connection(None), ConnPref::Usb);
        assert_eq!(cfg.resolve_connection(Some(ConnPref::Ble)), ConnPref::Ble);
        assert_eq!(cfg.resolve_model(None), Model::B21);
        assert_eq!(cfg.resolve_model(Some(Model::B1)), Model::B1);
        assert_eq!(cfg.resolve_scan_secs(None), 8);
        assert_eq!(cfg.resolve_scan_secs(Some(0)), 1);
    }

    #[test]
    fn clear_at_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        Config {
            addr: Some("x".into()),
            ..Default::default()
        }
        .save_to(&path)
        .unwrap();
        assert!(Config::clear_at(&path).unwrap());
        assert!(!path.exists());
        assert!(!Config::clear_at(&path).unwrap());
    }

    #[test]
    fn default_path_honors_thermark_config_env() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("custom.json");
        assert_eq!(Config::default_path_with(path.to_str()).unwrap(), path);
        // Blank or absent falls back to the platform config directory.
        assert!(
            Config::default_path_with(Some("  "))
                .unwrap()
                .ends_with("config.json")
        );
        assert!(
            Config::default_path_with(None)
                .unwrap()
                .ends_with("config.json")
        );
    }

    #[test]
    fn parse_json_object() {
        let cfg = Config::parse_json(r#"{"addr":"B1-Y","connection":"usb","model":"b1"}"#).unwrap();
        assert_eq!(cfg.addr.as_deref(), Some("B1-Y"));
        assert_eq!(cfg.connection, Some(ConnPref::Usb));
        assert_eq!(cfg.model, Some(Model::B1));
        // alias
        let cfg = Config::parse_json(r#"{"connection":"serial"}"#).unwrap();
        assert_eq!(cfg.connection, Some(ConnPref::Usb));
    }

    #[test]
    fn to_json_pretty_roundtrip() {
        let cfg = Config {
            addr: Some("B1-X".into()),
            ..Default::default()
        };
        let s = cfg.to_json_pretty().unwrap();
        let back = Config::parse_json(&s).unwrap();
        assert_eq!(back, cfg);
    }
}
