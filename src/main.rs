//! CLI for pocket thermal label printers over BLE or USB serial.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
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
use thermark::transport::{BleTransport, SerialTransport};
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
    },
    /// List USB serial ports
    Ports,
    /// Query printer info (serial, battery, versions)
    Info {
        #[command(flatten)]
        conn: ConnArgs,
        #[arg(short, long, value_enum, default_value_t = Model::B1)]
        model: Model,
    },
    /// Print an image (PNG/JPEG/…)
    Print {
        #[command(flatten)]
        conn: ConnArgs,
        /// Image path
        #[arg(short, long)]
        image: PathBuf,
        #[arg(short, long, value_enum, default_value_t = Model::B1)]
        model: Model,
        /// Print density 1..=5
        #[arg(short, long, default_value_t = 3)]
        density: u8,
        /// Rotate clockwise: 0, 90, 180, 270
        #[arg(short, long, default_value_t = 0)]
        rotate: u32,
        /// Black/white threshold after invert (0–255)
        #[arg(long, default_value_t = 127)]
        threshold: u8,
        /// Scale image down to fit printhead width only
        #[arg(long, default_value_t = false)]
        fit: bool,
        /// Physical label size in mm, e.g. 50x30 (width x height). Scales content to this canvas.
        #[arg(long)]
        label: Option<String>,
        /// With --label, scale image to cover the whole label (default true when --label set)
        #[arg(long, default_value_t = true)]
        fill: bool,
        /// Force simple 1-byte PrintStart (plain-form)
        #[arg(long, default_value_t = false)]
        simple_start: bool,
        /// Print task: b1 (tested), b21v1, d110, simple (experimental)
        #[arg(long, value_enum)]
        task: Option<PrintTask>,
    },
    /// Print a full-bleed calibration pattern for a label size (find true print area)
    Calibrate {
        #[command(flatten)]
        conn: ConnArgs,
        #[arg(short, long, value_enum, default_value_t = Model::B1)]
        model: Model,
        /// Label size mm, e.g. 50x30
        #[arg(long, default_value = "50x30")]
        label: String,
        #[arg(short, long, default_value_t = 4)]
        density: u8,
        #[arg(long, default_value_t = false)]
        simple_start: bool,
        #[arg(long, value_enum)]
        task: Option<PrintTask>,
    },
    /// Design + print a square QR with side text (fills the label)
    Qr {
        #[command(flatten)]
        conn: ConnArgs,
        #[arg(short, long, value_enum, default_value_t = Model::B1)]
        model: Model,
        /// URL or text encoded in the QR
        #[arg(long, default_value = "https://www.youtube.com")]
        url: String,
        /// Text drawn beside the QR (use \\n for new lines)
        #[arg(long, default_value = "ABC\nYOUTUBE\n123")]
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
        #[arg(short, long, default_value_t = 4)]
        density: u8,
        /// Also save PNG to this path
        #[arg(long)]
        save: Option<PathBuf>,
        #[arg(long, default_value_t = false)]
        simple_start: bool,
        #[arg(long, value_enum)]
        task: Option<PrintTask>,
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
        conn: Option<ConnKind>,
        #[arg(short, long, value_enum, default_value_t = Model::B1)]
        model: Model,
        /// BLE scan seconds
        #[arg(short, long, default_value_t = 5)]
        seconds: u64,
        /// Use saved default printer even without -a (connect + sensors)
        #[arg(long, default_value_t = false)]
        use_config: bool,
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
        #[arg(short = 'c', long, value_enum, default_value_t = ConnKind::Ble)]
        conn: ConnKind,
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
    conn: Option<ConnKind>,
    /// BLE name / peripheral id, or serial path (default: config / THERMARK_ADDR)
    #[arg(short, long)]
    addr: Option<String>,
    /// BLE scan time before connect (seconds; default: config or 4)
    #[arg(long)]
    scan_secs: Option<u64>,
}

/// Resolved connection after applying config / env defaults.
struct ResolvedConn {
    conn: ConnKind,
    addr: String,
    scan_secs: u64,
}

impl ConnArgs {
    fn resolve(&self, cfg: &Config) -> Result<ResolvedConn> {
        let addr = cfg.resolve_addr(self.addr.as_deref())?;
        let pref = cfg.resolve_connection(self.conn.map(ConnKind::as_pref_str));
        Ok(ResolvedConn {
            conn: ConnKind::from_pref(pref),
            addr,
            scan_secs: cfg.resolve_scan_secs(self.scan_secs),
        })
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "lower")]
enum ConnKind {
    Ble,
    Usb,
}

impl ConnKind {
    fn as_pref_str(self) -> &'static str {
        match self {
            Self::Ble => "ble",
            Self::Usb => "usb",
        }
    }

    fn from_pref(p: ConnPref) -> Self {
        match p {
            ConnPref::Ble => Self::Ble,
            ConnPref::Usb => Self::Usb,
        }
    }

    fn to_pref(self) -> ConnPref {
        match self {
            Self::Ble => ConnPref::Ble,
            Self::Usb => ConnPref::Usb,
        }
    }
}

impl std::fmt::Display for ConnKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_pref_str())
    }
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
    ) -> Result<Self> {
        let task = resolve_task(model, simple_start, task);
        match conn.conn {
            ConnKind::Ble => {
                let ble = BleTransport::connect(&conn.addr, Duration::from_secs(conn.scan_secs))
                    .await
                    .context("BLE connect")?;
                Ok(Self::Ble(
                    PrinterClient::new(ble, model).with_print_task(task),
                ))
            }
            ConnKind::Usb => {
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
fn resolve_task(model: Model, simple_start: bool, task: Option<PrintTask>) -> PrintTask {
    let t = if let Some(task) = task {
        task
    } else if simple_start {
        PrintTask::Simple
    } else {
        PrintTask::for_model(model)
    };
    if !t.hardware_tested() {
        eprintln!(
            "warning: print task '{t}' is experimental (not hardware-tested in this project)"
        );
    }
    t
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
        Commands::Scan { seconds } => cmd_scan(seconds).await?,
        Commands::Ports => cmd_ports()?,
        Commands::Info { conn, model } => {
            let conn = conn.resolve(&cfg)?;
            let mut session = Session::connect(&conn, model, false, None).await?;
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
            simple_start,
            task,
        } => {
            if !(1..=5).contains(&density) {
                bail!("density must be 1..=5");
            }
            if !image.exists() {
                bail!("image not found: {}", image.display());
            }
            let label_mm = match label {
                Some(s) => Some(LabelMm::parse(&s)?),
                None => None,
            };
            let opts = PrintOptions {
                density,
                rotate,
                threshold,
                fit,
                label: label_mm,
                fill: fill || label_mm.is_some(),
            };
            let conn = conn.resolve(&cfg)?;
            let mut session = Session::connect(&conn, model, simple_start, task).await?;
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
        } => {
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
                rotate: 0,
                threshold: 127,
                fit: false,
                label: Some(label_mm),
                fill: true,
            };
            let conn = conn.resolve(&cfg)?;
            let mut session = Session::connect(&conn, model, simple_start, task).await?;
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
        } => {
            // Host-only by default; -a or --use-config enables connect + sensors.
            let addr = if let Some(a) = addr {
                Some(a)
            } else if use_config {
                Some(cfg.resolve_addr(None)?)
            } else {
                None
            };
            let kind = match conn
                .map(ConnKind::to_pref)
                .unwrap_or_else(|| cfg.resolve_connection(None))
            {
                ConnPref::Ble => DoctorConn::Ble,
                ConnPref::Usb => DoctorConn::Usb,
            };
            let report = doctor::run_doctor(addr.as_deref(), model, seconds, kind)
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
            no_print,
        } => {
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
                rotate: 0,
                threshold: 127,
                fit: false,
                label: Some(label_mm),
                fill: false,
            };
            let conn = conn.resolve(&cfg)?;
            let mut session = Session::connect(&conn, model, simple_start, task).await?;
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
                println!("Save a printer: thermark config set -a \"B1-YourPrinter\"");
                return Ok(());
            }
            println!("addr:        {}", cfg.addr.as_deref().unwrap_or("(unset)"));
            println!(
                "connection:  {}",
                cfg.connection.as_deref().unwrap_or("(default ble)")
            );
            println!(
                "model:       {}",
                cfg.model.as_deref().unwrap_or("(default b1)")
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
            cfg.apply_set(
                &addr,
                conn.to_pref(),
                model.map(|m| m.to_string()),
                scan_secs,
            );
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

async fn cmd_scan(seconds: u64) -> Result<()> {
    info!(seconds, "scanning BLE");
    let devices = BleTransport::scan(Duration::from_secs(seconds)).await?;
    if devices.is_empty() {
        println!("No label-printer-like devices found.");
        println!("Tips: turn printer on, quit vendor apps, enable Bluetooth.");
        return Ok(());
    }
    println!("{:<40} {:<24} RSSI", "ID", "NAME");
    for d in devices {
        let rssi = d.rssi.map(|r| r.to_string()).unwrap_or_else(|| "-".into());
        println!("{:<40} {:<24} {}", d.id, d.name, rssi);
    }
    println!("\nSave default:  thermark config set -a \"<name or id>\"");
    println!("Then connect:  thermark info");
    Ok(())
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
    println!("Override with: thermark print|qr|calibrate --task b1|b21v1|d110|simple");
}
