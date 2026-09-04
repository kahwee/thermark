//! Command-line surface: the clap types and shared argument groups.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use thermark::config::{Config, ConnPref, MAX_SCAN_SECS};
use thermark::label::{TextAlign, TextSide};
use thermark::print_task::PrintTask;
use thermark::protocol::Model;
#[cfg(feature = "ble")]
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
    #[arg(long, value_parser = parse_scan_seconds)]
    pub scan_secs: Option<u64>,
    /// Allow substring BLE name matching (default: exact name or id only)
    #[arg(long, default_value_t = false)]
    pub fuzzy: bool,
}

/// Which on-wire print sequence to use. Flattened into every printing command.
#[derive(Debug, Clone, clap::Args)]
pub struct TaskArgs {
    /// Print task override: b1, d11v1, d110, or d110mv4
    #[arg(long, value_enum)]
    pub task: Option<PrintTask>,
    /// Allow a task not hardware-tested by thermark
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
#[cfg_attr(not(any(feature = "ble", feature = "serial")), allow(dead_code))]
pub struct ResolvedConn {
    pub conn: ConnPref,
    pub addr: String,
    #[cfg(feature = "ble")]
    pub scan_secs: u64,
    #[cfg(feature = "ble")]
    pub match_mode: BleMatchMode,
}

impl ConnArgs {
    pub fn resolve(&self, cfg: &Config) -> Result<ResolvedConn> {
        Ok(ResolvedConn {
            conn: cfg.resolve_connection(self.conn),
            addr: cfg.resolve_addr(self.addr.as_deref())?,
            #[cfg(feature = "ble")]
            scan_secs: cfg.resolve_scan_secs(self.scan_secs),
            #[cfg(feature = "ble")]
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

pub fn parse_scan_seconds(s: &str) -> std::result::Result<u64, String> {
    let seconds = s
        .parse::<u64>()
        .map_err(|_| "scan time must be an integer number of seconds".to_string())?;
    if !(1..=MAX_SCAN_SECS).contains(&seconds) {
        return Err(format!(
            "scan time must be between 1 and {MAX_SCAN_SECS} seconds"
        ));
    }
    Ok(seconds)
}

// ─── Commands ───────────────────────────────────────────────────────────────

#[derive(Debug, clap::Args)]
pub struct PrintCommand {
    #[command(flatten)]
    pub conn: ConnArgs,
    #[command(flatten)]
    pub task: TaskArgs,
    /// Image path
    #[arg(short, long)]
    pub image: PathBuf,
    /// Printer model (default: config or b1)
    #[arg(short, long, value_enum)]
    pub model: Option<Model>,
    /// Print density 1..=5 (default 3 = normal; use 4 for denser/darker)
    #[arg(short, long, default_value = "3", value_parser = parse_density)]
    pub density: Density,
    /// Rotate clockwise: 0, 90, 180, 270
    #[arg(short, long, default_value = "0", value_parser = parse_rotation)]
    pub rotate: Rotation,
    /// Black/white threshold after invert (0–255)
    #[arg(long, default_value = "127", value_parser = parse_threshold)]
    pub threshold: Threshold,
    /// Scale image down to fit printhead width only
    #[arg(long, default_value_t = false)]
    pub fit: bool,
    /// Physical label size in mm, e.g. 50x30 (width x height). Scales content to this canvas.
    #[arg(long)]
    pub label: Option<String>,
    /// Cover the label (may crop). Default on. Use --no-fill to fit the whole image centered.
    #[arg(long, default_value_t = true)]
    pub fill: bool,
    /// Fit the whole image on the label with white margins (no crop). Best for photos.
    #[arg(long, default_value_t = false)]
    pub no_fill: bool,
    /// White margin inset in pixels (each side). Avoids edge bleed; good with photos.
    #[arg(long, default_value_t = 0)]
    pub margin: u32,
    /// Floyd–Steinberg dither instead of hard B/W (recommended for photographs)
    #[arg(long, default_value_t = false)]
    pub dither: bool,
    /// Keep the image's own white border instead of cropping it.
    /// By default it is trimmed so the artwork fills the label.
    #[arg(long, default_value_t = false)]
    pub no_trim: bool,
    /// Ignore the configured registration inset and use the whole canvas.
    /// A charged B1 can address the full canvas; edge registration may vary.
    #[arg(long, default_value_t = false)]
    pub full_bleed: bool,
    /// Write the final monochrome print pixels to this PNG and do not print.
    /// Lets you check placement without a printer or a wasted label.
    #[arg(long)]
    pub preview: Option<PathBuf>,
}

#[derive(Debug, clap::Args)]
pub struct CalibrateCommand {
    #[command(flatten)]
    pub conn: ConnArgs,
    #[command(flatten)]
    pub task: TaskArgs,
    /// Printer model (default: config or b1)
    #[arg(short, long, value_enum)]
    pub model: Option<Model>,
    /// Label size mm, e.g. 50x30 (default: config, else 50x30)
    #[arg(long)]
    pub label: Option<String>,
    /// Density 1..=5 (default 4 = darker for full-bleed calibration)
    #[arg(short, long, default_value = "4", value_parser = parse_density)]
    pub density: Density,
    /// Print the boundary probe instead: one numbered bar per millimetre,
    /// so the last one you can see is exactly where the printer stops.
    #[arg(long, default_value_t = false)]
    pub boundary: bool,
}

#[derive(Debug, clap::Args)]
pub struct TextCommand {
    #[command(flatten)]
    pub conn: ConnArgs,
    #[command(flatten)]
    pub task: TaskArgs,
    #[command(flatten)]
    pub font: FontArgs,
    /// Printer model (default: config or b1)
    #[arg(short, long, value_enum)]
    pub model: Option<Model>,
    /// Text to print (use \\n for new lines)
    #[arg(long)]
    pub text: String,
    /// Horizontal alignment: left, center (default), right
    #[arg(long, value_enum, default_value_t = TextAlign::Center)]
    pub align: TextAlign,
    /// Label size mm, e.g. 50x30 (default: config, else 50x30)
    #[arg(long)]
    pub label: Option<String>,
    /// Draw a 1px outer border
    #[arg(long, default_value_t = false)]
    pub border: bool,
    /// Density 1..=5 (default 4 = darker for crisp text)
    #[arg(short, long, default_value = "4", value_parser = parse_density)]
    pub density: Density,
    /// Also save PNG to this path
    #[arg(long)]
    pub save: Option<PathBuf>,
    /// Only generate PNG, do not print
    #[arg(long, default_value_t = false)]
    pub no_print: bool,
}

#[derive(Debug, clap::Args)]
pub struct QrCommand {
    #[command(flatten)]
    pub conn: ConnArgs,
    #[command(flatten)]
    pub task: TaskArgs,
    #[command(flatten)]
    pub font: FontArgs,
    /// Printer model (default: config or b1)
    #[arg(short, long, value_enum)]
    pub model: Option<Model>,
    /// URL or text encoded in the QR
    #[arg(long, default_value = "https://example.com")]
    pub url: String,
    /// Text drawn beside the QR (use \\n for new lines)
    #[arg(long, default_value = "ABC\nHELLO")]
    pub text: String,
    /// Put text on left or right of the square QR
    #[arg(long, value_enum, default_value_t = TextSide::Right)]
    pub text_side: TextSide,
    /// Label size mm, e.g. 50x30 (default: config, else 50x30)
    #[arg(long)]
    pub label: Option<String>,
    /// Draw a 1px outer border (usually unnecessary)
    #[arg(long, default_value_t = false)]
    pub border: bool,
    /// Density 1..=5 (default 4 = darker for small QR/text)
    #[arg(short, long, default_value = "4", value_parser = parse_density)]
    pub density: Density,
    /// Also save PNG to this path
    #[arg(long)]
    pub save: Option<PathBuf>,
    /// Only generate PNG, do not print
    #[arg(long, default_value_t = false)]
    pub no_print: bool,
}

#[derive(Debug, clap::Args)]
pub struct WifiCommand {
    #[command(flatten)]
    pub conn: ConnArgs,
    #[command(flatten)]
    pub task: TaskArgs,
    #[command(flatten)]
    pub font: FontArgs,
    /// Printer model (default: config or b1)
    #[arg(short, long, value_enum)]
    pub model: Option<Model>,
    /// Network name (SSID) — shown on the sticker
    #[arg(long)]
    pub ssid: String,
    /// Wi‑Fi password (or set THERMARK_WIFI_PASSWORD — preferred, avoids shell history)
    #[arg(long, default_value = "")]
    pub password: String,
    /// Security: wpa (default), wep, nopass
    #[arg(long, value_enum, default_value_t = WifiSecurity::Wpa)]
    pub security: WifiSecurity,
    /// Hidden SSID
    #[arg(long, default_value_t = false)]
    pub hidden: bool,
    /// Also print password in cleartext under the SSID (less secure)
    #[arg(long, default_value_t = false)]
    pub show_password: bool,
    #[arg(long, value_enum, default_value_t = TextSide::Right)]
    pub text_side: TextSide,
    #[arg(long)]
    pub label: Option<String>,
    #[arg(long, default_value_t = false)]
    pub border: bool,
    #[arg(short, long, default_value = "4", value_parser = parse_density)]
    pub density: Density,
    /// Save PNG (use a path outside the git repo for real credentials)
    #[arg(long)]
    pub save: Option<PathBuf>,
    /// Only generate PNG, do not print
    #[arg(long, default_value_t = false)]
    pub no_print: bool,
}

#[derive(Debug, clap::Args)]
pub struct DoctorCommand {
    /// BLE name / id, or serial path (default: saved config / THERMARK_ADDR; omit for host-only)
    #[arg(short, long)]
    pub addr: Option<String>,
    /// Connection type when connecting
    #[arg(short = 'c', long, value_enum)]
    pub conn: Option<ConnPref>,
    /// Printer model (default: config or b1)
    #[arg(short, long, value_enum)]
    pub model: Option<Model>,
    /// Print task to report on (default: model's default)
    #[arg(long, value_enum)]
    pub task: Option<PrintTask>,
    /// BLE scan seconds
    #[arg(short, long, default_value_t = 5, value_parser = parse_scan_seconds)]
    pub seconds: u64,
    /// Use saved default printer even without -a (connect + sensors)
    #[arg(long, default_value_t = false)]
    pub use_config: bool,
    /// Allow substring BLE name matching when connecting (default: exact only)
    #[arg(long, default_value_t = false)]
    pub fuzzy: bool,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Scan for thermal label printers over Bluetooth LE
    Scan {
        /// How long to scan (seconds)
        #[arg(short, long, default_value_t = 5, value_parser = parse_scan_seconds)]
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
    },
    /// Identify the connected printer and resolve its capability profile
    Identify {
        #[command(flatten)]
        conn: ConnArgs,
        /// Emit stable machine-readable output for bug reports and captures
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Print an image (PNG/JPEG/…)
    Print(PrintCommand),
    /// Print a full-bleed calibration pattern for a label size (find true print area)
    Calibrate(CalibrateCommand),
    /// Print a text-only sticker (no QR) — auto-fitted to fill the label
    Text(TextCommand),
    /// Design + print a square QR with side text (fills the label)
    Qr(QrCommand),
    /// Guest Wi‑Fi sticker: scan-to-join QR + clear network name
    ///
    /// QR uses the standard WIFI: payload (phones join on scan). Side text shows
    /// the SSID large; password stays in the QR unless --show-password.
    /// Do not commit real credentials — print locally or --save outside the repo.
    Wifi(WifiCommand),
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
    Doctor(DoctorCommand),
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
        #[arg(long, value_parser = parse_scan_seconds)]
        scan_secs: Option<u64>,
        /// Default label size, e.g. 50x30 — saves repeating --label
        #[arg(short, long)]
        label: Option<String>,
    },
    /// Save content/registration insets measured with `thermark calibrate`
    ///
    /// Values are millimetres from each edge. Omitted edges keep their current
    /// value. These are placement margins, not assumed hardware limits.
    SafeArea {
        /// Millimetre mark of the LAST ruler tick that printed, from
        /// `thermark calibrate`. Sets the bottom inset for you.
        #[arg(long, conflicts_with = "bottom")]
        last_tick: Option<f64>,
        /// Label size the reading came from (needed with --last-tick)
        #[arg(long)]
        label: Option<String>,
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
