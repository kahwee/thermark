//! Command-line surface: the clap types and shared argument groups.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use thermark::config::{Config, ConnPref};
use thermark::label::{TextAlign, TextSide};
use thermark::print_task::PrintTask;
use thermark::protocol::Model;
use thermark::transport::BleMatchMode;
use thermark::types::{Density, Rotation, Threshold};
use thermark::wifi::WifiSecurity;

#[derive(Parser, Debug)]
#[command(
    name = "thermark",
    version,
    about = "Local sticker printing over BLE/USB — guest Wi‑Fi, QR, inventory (no vendor app)"
)]
pub struct Cli {
    /// Verbose logging (`RUST_LOG` still overrides when set)
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

// ─── Shared argument groups ─────────────────────────────────────────────────

/// How to reach the printer. Flattened into every command that connects.
#[derive(Debug, Clone, clap::Args)]
pub struct ConnArgs {
    /// Connection type (default: config or ble)
    #[arg(short = 'c', long, value_enum)]
    pub conn: Option<ConnPref>,
    /// BLE advertising name or peripheral id (exact match), or serial path
    /// (default: config / THERMARK_ADDR). Use the full name from `thermark scan`.
    #[arg(short, long)]
    pub addr: Option<String>,
    /// BLE scan time before connect (seconds; default: config or 4)
    #[arg(long)]
    pub scan_secs: Option<u64>,
    /// Allow substring BLE name matching (default: exact name or id only)
    #[arg(long, default_value_t = false)]
    pub fuzzy: bool,
}

/// Which on-wire print sequence to use. Flattened into every printing command.
#[derive(Debug, Clone, clap::Args)]
pub struct TaskArgs {
    /// Force simple 1-byte PrintStart (plain-form; experimental)
    #[arg(long, default_value_t = false)]
    pub simple_start: bool,
    /// Print task: b1 (tested), b21v1, d110, simple (experimental)
    #[arg(long, value_enum)]
    pub task: Option<PrintTask>,
    /// Allow experimental print tasks (non-B1); required for b21v1/d110/simple
    #[arg(long, default_value_t = false)]
    pub allow_experimental: bool,
}

/// Text rendering options. Flattened into the label-composing commands.
#[derive(Debug, Clone, clap::Args)]
pub struct FontArgs {
    /// Path to a .ttf / .ttc font file
    #[arg(long)]
    pub font: Option<PathBuf>,
    /// Named system font: helvetica, times, arial, courier, …
    #[arg(long)]
    pub font_name: Option<String>,
    /// Text size in px (e.g. 11 = small). Default: auto-fit largest.
    #[arg(long)]
    pub font_size: Option<f32>,
}

/// Connection settings after applying config / env defaults.
pub struct ResolvedConn {
    pub conn: ConnPref,
    pub addr: String,
    pub scan_secs: u64,
    pub match_mode: BleMatchMode,
}

impl ConnArgs {
    pub fn resolve(&self, cfg: &Config) -> Result<ResolvedConn> {
        Ok(ResolvedConn {
            conn: cfg.resolve_connection(self.conn),
            addr: cfg.resolve_addr(self.addr.as_deref())?,
            scan_secs: cfg.resolve_scan_secs(self.scan_secs),
            match_mode: BleMatchMode::from_fuzzy(self.fuzzy),
        })
    }
}

// ─── Value parsers ──────────────────────────────────────────────────────────

pub fn parse_density(s: &str) -> std::result::Result<Density, String> {
    s.parse::<Density>().map_err(|e| e.to_string())
}

pub fn parse_rotation(s: &str) -> std::result::Result<Rotation, String> {
    s.parse::<Rotation>().map_err(|e| e.to_string())
}

pub fn parse_threshold(s: &str) -> std::result::Result<Threshold, String> {
    s.parse::<Threshold>().map_err(|e| e.to_string())
}

// ─── Commands ───────────────────────────────────────────────────────────────

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Scan for thermal label printers over Bluetooth LE
    Scan {
        /// How long to scan (seconds)
        #[arg(short, long, default_value_t = 5)]
        seconds: u64,
        /// Save the best match into config.json as the default printer
        #[arg(long, default_value_t = false)]
        save: bool,
        /// When saving, prefer a device whose name contains this (case-insensitive)
        #[arg(long)]
        name: Option<String>,
    },
    /// List USB serial ports
    Ports,
    /// Query printer info (serial, battery, versions)
    Info {
        #[command(flatten)]
        conn: ConnArgs,
        /// Printer model (default: config or b1)
        #[arg(short, long, value_enum)]
        model: Option<Model>,
    },
    /// Print an image (PNG/JPEG/…)
    Print {
        #[command(flatten)]
        conn: ConnArgs,
        #[command(flatten)]
        task: TaskArgs,
        /// Image path
        #[arg(short, long)]
        image: PathBuf,
        /// Printer model (default: config or b1)
        #[arg(short, long, value_enum)]
        model: Option<Model>,
        /// Print density 1..=5 (default 3 = normal; use 4 for denser/darker)
        #[arg(short, long, default_value = "3", value_parser = parse_density)]
        density: Density,
        /// Rotate clockwise: 0, 90, 180, 270
        #[arg(short, long, default_value = "0", value_parser = parse_rotation)]
        rotate: Rotation,
        /// Black/white threshold after invert (0–255)
        #[arg(long, default_value = "127", value_parser = parse_threshold)]
        threshold: Threshold,
        /// Scale image down to fit printhead width only
        #[arg(long, default_value_t = false)]
        fit: bool,
        /// Physical label size in mm, e.g. 50x30 (width x height). Scales content to this canvas.
        #[arg(long)]
        label: Option<String>,
        /// Cover the label (may crop). Default on. Use --no-fill to fit the whole image centered.
        #[arg(long, default_value_t = true)]
        fill: bool,
        /// Fit the whole image on the label with white margins (no crop). Best for photos.
        #[arg(long, default_value_t = false)]
        no_fill: bool,
        /// White margin inset in pixels (each side). Avoids edge bleed; good with photos.
        #[arg(long, default_value_t = 0)]
        margin: u32,
        /// Floyd–Steinberg dither instead of hard B/W (recommended for photographs)
        #[arg(long, default_value_t = false)]
        dither: bool,
    },
    /// Print a full-bleed calibration pattern for a label size (find true print area)
    Calibrate {
        #[command(flatten)]
        conn: ConnArgs,
        #[command(flatten)]
        task: TaskArgs,
        /// Printer model (default: config or b1)
        #[arg(short, long, value_enum)]
        model: Option<Model>,
        /// Label size mm, e.g. 50x30
        #[arg(long, default_value = "50x30")]
        label: String,
        /// Density 1..=5 (default 4 = darker for full-bleed calibration)
        #[arg(short, long, default_value = "4", value_parser = parse_density)]
        density: Density,
    },
    /// Print a text-only sticker (no QR) — auto-fitted to fill the label
    Text {
        #[command(flatten)]
        conn: ConnArgs,
        #[command(flatten)]
        task: TaskArgs,
        #[command(flatten)]
        font: FontArgs,
        /// Printer model (default: config or b1)
        #[arg(short, long, value_enum)]
        model: Option<Model>,
        /// Text to print (use \\n for new lines)
        #[arg(long)]
        text: String,
        /// Horizontal alignment: left, center (default), right
        #[arg(long, value_enum, default_value_t = TextAlign::Center)]
        align: TextAlign,
        /// Label size mm, e.g. 50x30
        #[arg(long, default_value = "50x30")]
        label: String,
        /// Draw a 1px outer border
        #[arg(long, default_value_t = false)]
        border: bool,
        /// Density 1..=5 (default 4 = darker for crisp text)
        #[arg(short, long, default_value = "4", value_parser = parse_density)]
        density: Density,
        /// Also save PNG to this path
        #[arg(long)]
        save: Option<PathBuf>,
        /// Only generate PNG, do not print
        #[arg(long, default_value_t = false)]
        no_print: bool,
    },
    /// Design + print a square QR with side text (fills the label)
    Qr {
        #[command(flatten)]
        conn: ConnArgs,
        #[command(flatten)]
        task: TaskArgs,
        #[command(flatten)]
        font: FontArgs,
        /// Printer model (default: config or b1)
        #[arg(short, long, value_enum)]
        model: Option<Model>,
        /// URL or text encoded in the QR
        #[arg(long, default_value = "https://example.com")]
        url: String,
        /// Text drawn beside the QR (use \\n for new lines)
        #[arg(long, default_value = "ABC\nHELLO")]
        text: String,
        /// Put text on left or right of the square QR
        #[arg(long, value_enum, default_value_t = TextSide::Right)]
        text_side: TextSide,
        /// Label size mm, e.g. 50x30
        #[arg(long, default_value = "50x30")]
        label: String,
        /// Draw a 1px outer border (usually unnecessary)
        #[arg(long, default_value_t = false)]
        border: bool,
        /// Density 1..=5 (default 4 = darker for small QR/text)
        #[arg(short, long, default_value = "4", value_parser = parse_density)]
        density: Density,
        /// Also save PNG to this path
        #[arg(long)]
        save: Option<PathBuf>,
        /// Only generate PNG, do not print
        #[arg(long, default_value_t = false)]
        no_print: bool,
    },
    /// Guest Wi‑Fi sticker: scan-to-join QR + clear network name
    ///
    /// QR uses the standard WIFI: payload (phones join on scan). Side text shows
    /// the SSID large; password stays in the QR unless --show-password.
    /// Do not commit real credentials — print locally or --save outside the repo.
    Wifi {
        #[command(flatten)]
        conn: ConnArgs,
        #[command(flatten)]
        task: TaskArgs,
        #[command(flatten)]
        font: FontArgs,
        /// Printer model (default: config or b1)
        #[arg(short, long, value_enum)]
        model: Option<Model>,
        /// Network name (SSID) — shown on the sticker
        #[arg(long)]
        ssid: String,
        /// Wi‑Fi password (or set THERMARK_WIFI_PASSWORD — preferred, avoids shell history)
        #[arg(long, default_value = "")]
        password: String,
        /// Security: wpa (default), wep, nopass
        #[arg(long, value_enum, default_value_t = WifiSecurity::Wpa)]
        security: WifiSecurity,
        /// Hidden SSID
        #[arg(long, default_value_t = false)]
        hidden: bool,
        /// Also print password in cleartext under the SSID (less secure)
        #[arg(long, default_value_t = false)]
        show_password: bool,
        #[arg(long, value_enum, default_value_t = TextSide::Right)]
        text_side: TextSide,
        #[arg(long, default_value = "50x30")]
        label: String,
        #[arg(long, default_value_t = false)]
        border: bool,
        #[arg(short, long, default_value = "4", value_parser = parse_density)]
        density: Density,
        /// Save PNG (use a path outside the git repo for real credentials)
        #[arg(long)]
        save: Option<PathBuf>,
        /// Only generate PNG, do not print
        #[arg(long, default_value_t = false)]
        no_print: bool,
    },
    /// List system fonts this tool can use
    Fonts,
    /// Show print-task / hardware support matrix
    Tasks,
    /// Show / set saved default printer (config.json)
    Config {
        #[command(subcommand)]
        action: ConfigCmd,
    },
    /// Diagnose host + printer readiness (Bluetooth, scan, sensors)
    Doctor {
        /// BLE name / id, or serial path (default: saved config / THERMARK_ADDR; omit for host-only)
        #[arg(short, long)]
        addr: Option<String>,
        /// Connection type when connecting
        #[arg(short = 'c', long, value_enum)]
        conn: Option<ConnPref>,
        /// Printer model (default: config or b1)
        #[arg(short, long, value_enum)]
        model: Option<Model>,
        /// Print task to report on (default: model's default)
        #[arg(long, value_enum)]
        task: Option<PrintTask>,
        /// BLE scan seconds
        #[arg(short, long, default_value_t = 5)]
        seconds: u64,
        /// Use saved default printer even without -a (connect + sensors)
        #[arg(long, default_value_t = false)]
        use_config: bool,
        /// Allow substring BLE name matching when connecting (default: exact only)
        #[arg(long, default_value_t = false)]
        fuzzy: bool,
    },
    /// Encode a packet to hex (debug)
    Encode {
        /// Command byte (hex, e.g. 1a)
        cmd: String,
        /// Data bytes as hex (e.g. 01)
        #[arg(default_value = "")]
        data: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigCmd {
    /// Print path + current saved values
    Show {
        /// Emit raw JSON only (no labels)
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Print config file path only
    Path,
    /// Save default printer (merge into existing config.json)
    Set {
        /// BLE name / UUID or serial path (required)
        #[arg(short, long)]
        addr: String,
        /// Connection type
        #[arg(short = 'c', long, value_enum, default_value_t = ConnPref::Ble)]
        conn: ConnPref,
        /// Default model
        #[arg(short, long, value_enum)]
        model: Option<Model>,
        /// Default BLE scan seconds before connect
        #[arg(long)]
        scan_secs: Option<u64>,
    },
    /// Save the printable insets measured with `thermark calibrate`
    ///
    /// Values are millimetres from each edge. Omitted edges keep their current
    /// value. The feed (bottom) edge is usually the only non-zero one.
    SafeArea {
        /// Millimetre mark of the LAST ruler tick that printed, from
        /// `thermark calibrate`. Sets the bottom inset for you.
        #[arg(long)]
        last_tick: Option<f64>,
        /// Label size the reading came from (needed with --last-tick)
        #[arg(long, default_value = "50x30")]
        label: String,
        #[arg(long)]
        top: Option<f64>,
        #[arg(long)]
        bottom: Option<f64>,
        #[arg(long)]
        left: Option<f64>,
        #[arg(long)]
        right: Option<f64>,
        /// Forget the saved value and use the built-in default
        #[arg(long, default_value_t = false)]
        reset: bool,
    },
    /// Remove the config file
    Clear,
}
