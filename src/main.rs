//! CLI for pocket thermal label printers over BLE or USB serial.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::time::Duration;
use thermark::config::{Config, ConnPref};
use thermark::doctor::{self, DoctorConn};
use thermark::font;
use thermark::geometry::LabelMm;
use thermark::image_encode;
use thermark::label::{self, QrLabelOptions, TextSide};
use thermark::print_task::{PrintTask, hardware_matrix};
use thermark::printer::{PrintOptions, PrinterClient, PrinterSummary};
use thermark::protocol::Model;
use thermark::transport::{BleDeviceInfo, BleMatchMode, BleTransport, SerialTransport};
use thermark::types::{Density, Rotation, Threshold};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(
    name = "thermark",
    version,
    about = "Local thermal label printing over BLE/USB — QR, text, calibration (no vendor app)"
)]
struct Cli {
    /// Verbose logging (`RUST_LOG` still overrides when set)
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
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
        /// Force simple 1-byte PrintStart (plain-form; experimental)
        #[arg(long, default_value_t = false)]
        simple_start: bool,
        /// Print task: b1 (tested), b21v1, d110, simple (experimental)
        #[arg(long, value_enum)]
        task: Option<PrintTask>,
        /// Allow experimental print tasks (non-B1); required for b21v1/d110/simple
        #[arg(long, default_value_t = false)]
        allow_experimental: bool,
    },
    /// Print a full-bleed calibration pattern for a label size (find true print area)
    Calibrate {
        #[command(flatten)]
        conn: ConnArgs,
        /// Printer model (default: config or b1)
        #[arg(short, long, value_enum)]
        model: Option<Model>,
        /// Label size mm, e.g. 50x30
        #[arg(long, default_value = "50x30")]
        label: String,
        /// Density 1..=5 (default 4 = darker for full-bleed calibration)
        #[arg(short, long, default_value = "4", value_parser = parse_density)]
        density: Density,
        #[arg(long, default_value_t = false)]
        simple_start: bool,
        #[arg(long, value_enum)]
        task: Option<PrintTask>,
        /// Allow experimental print tasks (non-B1)
        #[arg(long, default_value_t = false)]
        allow_experimental: bool,
    },
    /// Design + print a square QR with side text (fills the label)
    Qr {
        #[command(flatten)]
        conn: ConnArgs,
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
        /// Path to a .ttf / .ttc font file
        #[arg(long)]
        font: Option<PathBuf>,
        /// Named system font: helvetica, times, arial, courier, …
        #[arg(long)]
        font_name: Option<String>,
        /// Text size in px (e.g. 11 = small). Default: auto-fit largest that fits.
        #[arg(long)]
        font_size: Option<f32>,
        /// Draw a 1px outer border (usually unnecessary)
        #[arg(long, default_value_t = false)]
        border: bool,
        /// Density 1..=5 (default 4 = darker for small QR/text)
        #[arg(short, long, default_value = "4", value_parser = parse_density)]
        density: Density,
        /// Also save PNG to this path
        #[arg(long)]
        save: Option<PathBuf>,
        #[arg(long, default_value_t = false)]
        simple_start: bool,
        #[arg(long, value_enum)]
        task: Option<PrintTask>,
        /// Allow experimental print tasks (non-B1)
        #[arg(long, default_value_t = false)]
        allow_experimental: bool,
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
enum ConfigCmd {
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
    /// Remove the config file
    Clear,
}

#[derive(Debug, Clone, clap::Args)]
struct ConnArgs {
    /// Connection type (default: config or ble)
    #[arg(short = 'c', long, value_enum)]
    conn: Option<ConnPref>,
    /// BLE advertising name or peripheral id (exact match), or serial path
    /// (default: config / THERMARK_ADDR). Use the full name from `thermark scan`.
    #[arg(short, long)]
    addr: Option<String>,
    /// BLE scan time before connect (seconds; default: config or 4)
    #[arg(long)]
    scan_secs: Option<u64>,
    /// Allow substring BLE name matching (default: exact name or id only)
    #[arg(long, default_value_t = false)]
    fuzzy: bool,
}

/// Resolved connection after applying config / env defaults.
struct ResolvedConn {
    conn: ConnPref,
    addr: String,
    scan_secs: u64,
    match_mode: BleMatchMode,
}

impl ConnArgs {
    fn resolve(&self, cfg: &Config) -> Result<ResolvedConn> {
        Ok(ResolvedConn {
            conn: cfg.resolve_connection(self.conn),
            addr: cfg.resolve_addr(self.addr.as_deref())?,
            scan_secs: cfg.resolve_scan_secs(self.scan_secs),
            match_mode: BleMatchMode::from_fuzzy(self.fuzzy),
        })
    }
}

fn parse_density(s: &str) -> std::result::Result<Density, String> {
    s.parse::<Density>().map_err(|e| e.to_string())
}

fn parse_rotation(s: &str) -> std::result::Result<Rotation, String> {
    s.parse::<Rotation>().map_err(|e| e.to_string())
}

fn parse_threshold(s: &str) -> std::result::Result<Threshold, String> {
    s.parse::<Threshold>().map_err(|e| e.to_string())
}

/// Open BLE or USB session with a resolved print task.
enum Session {
    Ble(PrinterClient<BleTransport>),
    Usb(PrinterClient<SerialTransport>),
}

impl Session {
    async fn connect(
        conn: &ResolvedConn,
        model: Model,
        simple_start: bool,
        task: Option<PrintTask>,
        allow_experimental: bool,
    ) -> Result<Self> {
        let task = resolve_task(model, simple_start, task, allow_experimental)?;
        match conn.conn {
            ConnPref::Ble => {
                let ble = BleTransport::connect_with(
                    &conn.addr,
                    Duration::from_secs(conn.scan_secs),
                    conn.match_mode,
                )
                .await
                .context("BLE connect")?;
                Ok(Self::Ble(
                    PrinterClient::new(ble, model).with_print_task(task),
                ))
            }
            ConnPref::Usb => {
                let ser = SerialTransport::open(&conn.addr)
                    .with_context(|| format!("open serial {}", conn.addr))?;
                Ok(Self::Usb(
                    PrinterClient::new(ser, model).with_print_task(task),
                ))
            }
        }
    }

    async fn fetch_summary(&mut self) -> Result<PrinterSummary> {
        match self {
            Self::Ble(c) => Ok(c.fetch_summary().await?),
            Self::Usb(c) => Ok(c.fetch_summary().await?),
        }
    }

    async fn print_image_file_opts(&mut self, path: &Path, opts: PrintOptions) -> Result<()> {
        match self {
            Self::Ble(c) => Ok(c.print_image_file_opts(path, opts).await?),
            Self::Usb(c) => Ok(c.print_image_file_opts(path, opts).await?),
        }
    }

    /// Prefer calling this after every successful path; [`BleTransport`]'s `Drop`
    /// still disconnects if this is skipped after an error.
    async fn finish(self) {
        if let Self::Ble(c) = self {
            c.into_transport().disconnect().await.ok();
        }
    }
}

/// Resolve print task once: `--task` wins, else `--simple-start`, else model default.
///
/// Non-B1 tasks require `--allow-experimental` so experimental sequences are not
/// used by accident when the model default maps to an untested path.
fn resolve_task(
    model: Model,
    simple_start: bool,
    task: Option<PrintTask>,
    allow_experimental: bool,
) -> Result<PrintTask> {
    let t = if let Some(task) = task {
        task
    } else if simple_start {
        PrintTask::Simple
    } else {
        PrintTask::for_model(model)
    };
    if !t.hardware_tested() {
        if !allow_experimental {
            bail!(
                "print task '{t}' is experimental (not hardware-tested in this project). \
                 Re-run with --allow-experimental if you accept the risk, \
                 or use --task b1 / --model b1. See: thermark tasks"
            );
        }
        eprintln!(
            "warning: print task '{t}' is experimental (not hardware-tested in this project)"
        );
    }
    Ok(t)
}

fn init_tracing(verbose: bool) {
    let default = if verbose { "debug" } else { "info" };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_writer(std::io::stderr)
        .init();
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);
    let cfg = Config::load().unwrap_or_default();

    match cli.command {
        Commands::Scan {
            seconds,
            save,
            name,
        } => cmd_scan(seconds, save, name.as_deref()).await?,
        Commands::Ports => cmd_ports()?,
        Commands::Info { conn, model } => {
            let model = cfg.resolve_model(model);
            let conn = conn.resolve(&cfg)?;
            // Info never runs a print task sequence; experimental gate does not apply.
            let mut session = Session::connect(&conn, model, false, None, true).await?;
            let result = session.fetch_summary().await;
            session.finish().await;
            print!("{}", result?);
        }
        Commands::Print {
            conn,
            image,
            model,
            density,
            rotate,
            threshold,
            fit,
            label,
            fill,
            no_fill,
            margin,
            dither,
            simple_start,
            task,
            allow_experimental,
        } => {
            if !image.exists() {
                bail!("image not found: {}", image.display());
            }
            let model = cfg.resolve_model(model);
            let label_mm = match label {
                Some(s) => Some(LabelMm::parse(&s)?),
                None => None,
            };
            // --no-fill wins over --fill; with a label, default is cover unless no_fill.
            let use_fill = if no_fill {
                false
            } else if label_mm.is_some() {
                fill
            } else {
                false
            };
            let opts = PrintOptions {
                density,
                rotate,
                threshold,
                fit,
                label: label_mm,
                fill: use_fill,
                margin_px: margin,
                dither,
            };
            let conn = conn.resolve(&cfg)?;
            let mut session =
                Session::connect(&conn, model, simple_start, task, allow_experimental).await?;
            let result = session.print_image_file_opts(&image, opts).await;
            session.finish().await;
            result?;
            println!("OK — sent print job");
        }
        Commands::Calibrate {
            conn,
            model,
            label,
            density,
            simple_start,
            task,
            allow_experimental,
        } => {
            let model = cfg.resolve_model(model);
            let label_mm = LabelMm::parse(&label)?;
            let lp = label_mm.to_pixels(model.max_width_px());
            info!(
                width_px = lp.width_px,
                height_px = lp.height_px,
                width_mm = label_mm.width_mm,
                height_mm = label_mm.height_mm,
                "calibration pattern"
            );
            let gray = image_encode::calibration_pattern(lp);
            let tmp = std::env::temp_dir().join("thermark_calibrate.png");
            gray.save(&tmp)?;
            let opts = PrintOptions {
                density,
                rotate: Rotation::Deg0,
                threshold: Threshold::DEFAULT,
                fit: false,
                label: Some(label_mm),
                fill: true,
                margin_px: 0,
                dither: false,
            };
            let conn = conn.resolve(&cfg)?;
            let mut session =
                Session::connect(&conn, model, simple_start, task, allow_experimental).await?;
            let result = session.print_image_file_opts(&tmp, opts).await;
            session.finish().await;
            result?;
            println!("OK — calibration printed ({label})");
        }
        Commands::Fonts => {
            let fonts = font::list_available_fonts();
            if fonts.is_empty() {
                println!("No candidate fonts found.");
            } else {
                println!("Usable fonts on this machine:");
                for p in fonts {
                    println!("  {}", p.display());
                }
                println!("\nUse with: thermark qr ... --font \"/path/to/font.ttf\"");
            }
        }
        Commands::Tasks => print_tasks_matrix(),
        Commands::Config { action } => cmd_config(action)?,
        Commands::Doctor {
            addr,
            conn,
            model,
            seconds,
            use_config,
            fuzzy,
        } => {
            // Host-only by default; -a or --use-config enables connect + sensors.
            let model = cfg.resolve_model(model);
            let addr = if let Some(a) = addr {
                Some(a)
            } else if use_config {
                Some(cfg.resolve_addr(None)?)
            } else {
                None
            };
            let kind = match cfg.resolve_connection(conn) {
                ConnPref::Ble => DoctorConn::Ble,
                ConnPref::Usb => DoctorConn::Usb,
            };
            let report = doctor::run_doctor(
                addr.as_deref(),
                model,
                seconds,
                kind,
                BleMatchMode::from_fuzzy(fuzzy),
            )
            .await
            .context("doctor")?;
            print!("{report}");
            if report.exit_code() != 0 {
                std::process::exit(report.exit_code());
            }
        }
        Commands::Qr {
            conn,
            model,
            url,
            text,
            text_side,
            label,
            font: font_path,
            font_name,
            font_size,
            border,
            density,
            save,
            simple_start,
            task,
            allow_experimental,
            no_print,
        } => {
            let model = cfg.resolve_model(model);
            let label_mm = LabelMm::parse(&label)?;
            let lp = label_mm.to_pixels(model.max_width_px());
            let text = text.replace("\\n", "\n");
            let gray = label::make_qr_label_opts(&QrLabelOptions {
                url: url.clone(),
                side_text: text,
                label: lp,
                text_side,
                border,
                font_path,
                font_name,
                font_size,
            })?;
            let qr_side = label::max_qr_side(lp);
            info!(
                width_px = lp.width_px,
                height_px = lp.height_px,
                width_mm = label_mm.width_mm,
                height_mm = label_mm.height_mm,
                qr_side,
                ?text_side,
                ?font_size,
                "qr label"
            );

            let png_path =
                save.unwrap_or_else(|| std::env::temp_dir().join("thermark_qr_label.png"));
            gray.save(&png_path)
                .with_context(|| format!("save {}", png_path.display()))?;
            println!("saved {}", png_path.display());

            if no_print {
                return Ok(());
            }

            let opts = PrintOptions {
                density,
                rotate: Rotation::Deg0,
                threshold: Threshold::DEFAULT,
                fit: false,
                label: Some(label_mm),
                fill: false,
                margin_px: 0,
                dither: false,
            };
            let conn = conn.resolve(&cfg)?;
            let mut session =
                Session::connect(&conn, model, simple_start, task, allow_experimental).await?;
            let result = session.print_image_file_opts(&png_path, opts).await;
            session.finish().await;
            result?;
            println!("OK — QR label printed");
        }
        Commands::Encode { cmd, data } => {
            let cmd = u8::from_str_radix(cmd.trim_start_matches("0x"), 16)
                .context("cmd must be hex, e.g. 1a")?;
            let data = if data.is_empty() {
                vec![]
            } else {
                hex::decode(data.trim_start_matches("0x")).context("data must be hex")?
            };
            let pkt = thermark::Packet::new(cmd, data);
            println!("{}", hex::encode(pkt.encode()));
        }
    }

    Ok(())
}

fn cmd_config(action: ConfigCmd) -> Result<()> {
    match action {
        ConfigCmd::Path => {
            println!("{}", Config::default_path()?.display());
        }
        ConfigCmd::Show { json } => {
            let path = Config::default_path()?;
            let cfg = Config::load()?;
            if json {
                println!("{}", cfg.to_json_pretty()?);
                return Ok(());
            }
            println!("path: {}", path.display());
            if cfg.is_empty() && !path.exists() {
                println!("(no config file yet)");
                println!("Save a printer: thermark scan --save");
                println!("  or:            thermark config set -a \"B1-YourPrinter\" -m b1");
                return Ok(());
            }
            println!("addr:        {}", cfg.addr.as_deref().unwrap_or("(unset)"));
            println!(
                "connection:  {}",
                cfg.connection
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "(default ble)".into())
            );
            println!(
                "model:       {}",
                cfg.model
                    .map(|m| m.to_string())
                    .unwrap_or_else(|| "(default b1)".into())
            );
            println!(
                "scan_secs:   {}",
                cfg.scan_secs
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "(default 4)".into())
            );
        }
        ConfigCmd::Set {
            addr,
            conn,
            model,
            scan_secs,
        } => {
            let mut cfg = Config::load().unwrap_or_default();
            cfg.apply_set(&addr, conn, model, scan_secs);
            let path = cfg.save()?;
            println!("saved default printer → {addr}");
            println!("  connection: {conn}");
            println!("  file:       {}", path.display());
            println!("Now you can run: thermark info   (no -a needed)");
            println!("JSON view:       thermark config show --json");
        }
        ConfigCmd::Clear => {
            let path = Config::default_path()?;
            if Config::clear()? {
                println!("removed {}", path.display());
            } else {
                println!("no config file at {}", path.display());
            }
        }
    }
    Ok(())
}

async fn cmd_scan(seconds: u64, save: bool, prefer_name: Option<&str>) -> Result<()> {
    info!(seconds, save, "scanning BLE");
    let devices = BleTransport::scan(Duration::from_secs(seconds)).await?;
    if devices.is_empty() {
        println!("No label-printer-like devices found.");
        println!("Tips: turn printer on, quit vendor apps, enable Bluetooth.");
        return Ok(());
    }
    println!("{:<40} {:<24} RSSI", "ID", "NAME");
    for d in &devices {
        let rssi = d.rssi.map(|r| r.to_string()).unwrap_or_else(|| "-".into());
        println!("{:<40} {:<24} {}", d.id, d.name, rssi);
    }

    if save {
        let pick = pick_scan_device(&devices, prefer_name)
            .ok_or_else(|| anyhow::anyhow!("no device matched for --save"))?;
        // Prefer BLE advertising name; fall back to peripheral id (macOS UUID).
        let addr = if pick.name.is_empty() || pick.name == "(no name)" {
            pick.id.clone()
        } else {
            pick.name.clone()
        };
        let mut cfg = Config::load().unwrap_or_default();
        let model = cfg.model;
        cfg.apply_set(&addr, ConnPref::Ble, model, None);
        let path = cfg.save()?;
        println!("\nsaved default printer → {addr}");
        println!("  file: {}", path.display());
        println!("Next: thermark doctor --use-config");
        println!("      thermark info");
        println!("      thermark calibrate --label 50x30");
    } else {
        println!("\nSave default:  thermark scan --save");
        println!("  or:           thermark config set -a \"<full advertising name>\"");
        println!("Then:          thermark info   (exact name; add --fuzzy only if needed)");
    }
    Ok(())
}

/// Choose a device for `--save`: optional name substring, else B1-like name, else strongest RSSI.
fn pick_scan_device<'a>(
    devices: &'a [BleDeviceInfo],
    prefer_name: Option<&str>,
) -> Option<&'a BleDeviceInfo> {
    if devices.is_empty() {
        return None;
    }
    if let Some(want) = prefer_name.map(|s| s.to_ascii_lowercase()) {
        if let Some(d) = devices.iter().find(|d| {
            d.name.to_ascii_lowercase().contains(&want) || d.id.to_ascii_lowercase().contains(&want)
        }) {
            return Some(d);
        }
    }
    // Prefer names that look like real pocket printers (not bare UUIDs).
    let mut scored: Vec<(i32, &BleDeviceInfo)> = devices
        .iter()
        .map(|d| {
            let name = d.name.to_ascii_lowercase();
            let mut score = i32::from(d.rssi.unwrap_or(-100));
            if name.starts_with("b1") || name.contains("b1-") {
                score += 1000;
            } else if name.starts_with("b21")
                || name.starts_with("d11")
                || name.contains("niim")
                || name.starts_with("jc-")
            {
                score += 500;
            } else if name == "(no name)" || name.is_empty() {
                score -= 200;
            }
            (score, d)
        })
        .collect();
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.first().map(|(_, d)| *d)
}

fn cmd_ports() -> Result<()> {
    let ports = SerialTransport::list_ports()?;
    if ports.is_empty() {
        println!("No serial ports found.");
    } else {
        for p in ports {
            println!("{p}");
        }
    }
    Ok(())
}

fn print_tasks_matrix() {
    println!("{:<16} {:<10} {:<14} NOTES", "MODEL", "TASK", "STATUS");
    for row in hardware_matrix() {
        println!(
            "{:<16} {:<10} {:<14} {}",
            row.model, row.task, row.status, row.notes
        );
    }
    println!();
    println!("Default: b1 (hardware-tested). Override: --task b1|b21v1|d110|simple");
    println!("Non-b1 tasks require: --allow-experimental");
}

#[cfg(test)]
mod pick_tests {
    use super::*;

    fn dev(name: &str, rssi: i16) -> BleDeviceInfo {
        BleDeviceInfo {
            id: format!("id-{name}"),
            name: name.into(),
            rssi: Some(rssi),
        }
    }

    #[test]
    fn prefers_b1_name_over_stronger_rssi() {
        let devices = vec![dev("random-watch", -40), dev("B1-ABC", -80)];
        let p = pick_scan_device(&devices, None).unwrap();
        assert_eq!(p.name, "B1-ABC");
    }

    #[test]
    fn name_filter_wins() {
        let devices = vec![dev("B1-One", -50), dev("B1-Two", -40)];
        let p = pick_scan_device(&devices, Some("two")).unwrap();
        assert_eq!(p.name, "B1-Two");
    }
}
