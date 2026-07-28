//! User config: default printer address and connection prefs.
//!
//! File location (platform standard):
//! - **macOS:** `~/Library/Application Support/thermark/config.toml`
//! - **Linux:** `~/.config/thermark/config.toml`
//! - **Windows:** `%APPDATA%\thermark\config.toml`
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

/// On-disk / in-memory user preferences.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Default BLE name / UUID or serial device path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addr: Option<String>,
    /// `"ble"` or `"usb"`.
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

    /// Path used by the CLI (`THERMARK_CONFIG` or platform config dir).
    pub fn default_path() -> Result<PathBuf> {
        if let Ok(p) = std::env::var("THERMARK_CONFIG") {
            return Ok(PathBuf::from(p));
        }
        let dirs = directories::ProjectDirs::from("", "", "thermark")
            .ok_or_else(|| Error::msg("could not determine config directory for thermark"))?;
        Ok(dirs.config_dir().join("config.toml"))
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
        toml::from_str(&text)
            .map_err(|e| Error::msg(format!("parse config {}: {e}", path.display())))
    }

    /// Write TOML to the default path (creates parent dirs).
    pub fn save(&self) -> Result<PathBuf> {
        let path = Self::default_path()?;
        self.save_to(&path)?;
        Ok(path)
    }

    /// Write TOML to an explicit path.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| Error::msg(format!("create config dir {}: {e}", parent.display())))?;
        }
        let text = toml::to_string_pretty(self)
            .map_err(|e| Error::msg(format!("serialize config: {e}")))?;
        let body = format!(
            "# thermark user config — do not commit secrets/device serials to git\n\
             # Override path: THERMARK_CONFIG=/path/to/config.toml\n\
             # Override addr only: THERMARK_ADDR=B1-YourPrinter\n\
             {text}"
        );
        fs::write(path, body)
            .map_err(|e| Error::msg(format!("write config {}: {e}", path.display())))?;
        Ok(())
    }

    /// Delete the default config file if it exists.
    pub fn clear() -> Result<bool> {
        let path = Self::default_path()?;
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|e| Error::msg(format!("remove config {}: {e}", path.display())))?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Resolve printer address: CLI flag → `THERMARK_ADDR` → config.
    pub fn resolve_addr(&self, cli_addr: Option<&str>) -> Result<String> {
        if let Some(a) = cli_addr.map(str::trim).filter(|s| !s.is_empty()) {
            return Ok(a.to_string());
        }
        if let Ok(a) = std::env::var("THERMARK_ADDR") {
            let a = a.trim();
            if !a.is_empty() {
                return Ok(a.to_string());
            }
        }
        if let Some(a) = self
            .addr
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Ok(a.to_string());
        }
        Err(Error::msg(
            "no printer address: pass -a \"B1-Name\", set THERMARK_ADDR, \
             or save one with: thermark config set -a \"B1-Name\"",
        ))
    }

    /// Prefer CLI connection string when provided; else config (`ble` default).
    pub fn resolve_connection(&self, cli: Option<&str>) -> String {
        if let Some(c) = cli.map(str::trim).filter(|s| !s.is_empty()) {
            return c.to_ascii_lowercase();
        }
        self.connection
            .as_deref()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "ble".into())
    }

    pub fn resolve_scan_secs(&self, cli: Option<u64>) -> u64 {
        cli.or(self.scan_secs).unwrap_or(4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize env mutations across tests in this module.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn roundtrip_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let cfg = Config {
            addr: Some("B1-YourPrinter".into()),
            connection: Some("ble".into()),
            model: Some("b1".into()),
            scan_secs: Some(6),
        };
        cfg.save_to(&path).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.addr.as_deref(), Some("B1-YourPrinter"));
        assert_eq!(loaded.connection.as_deref(), Some("ble"));
        assert_eq!(loaded.scan_secs, Some(6));
    }

    #[test]
    fn missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nope.toml");
        let cfg = Config::load_from(&path).unwrap();
        assert!(cfg.addr.is_none());
    }

    #[test]
    fn resolve_addr_priority() {
        let _g = ENV_LOCK.lock().unwrap();
        // Clear env that tests might leave behind
        std::env::remove_var("THERMARK_ADDR");
        let cfg = Config {
            addr: Some("from-config".into()),
            ..Default::default()
        };
        assert_eq!(cfg.resolve_addr(Some("from-cli")).unwrap(), "from-cli");
        assert_eq!(cfg.resolve_addr(None).unwrap(), "from-config");

        std::env::set_var("THERMARK_ADDR", "from-env");
        assert_eq!(cfg.resolve_addr(None).unwrap(), "from-env");
        assert_eq!(cfg.resolve_addr(Some("from-cli")).unwrap(), "from-cli");
        std::env::remove_var("THERMARK_ADDR");
    }

    #[test]
    fn resolve_addr_missing_errors() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("THERMARK_ADDR");
        let cfg = Config::default();
        let err = cfg.resolve_addr(None).unwrap_err().to_string();
        assert!(err.contains("config set"), "{err}");
    }
}
