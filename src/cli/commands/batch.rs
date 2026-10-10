//! CSV batches reuse the existing label renderers and keep a durable print journal.

use anyhow::{Context, Result, bail, ensure};
use clap::ValueEnum;
use image::GrayImage;
use serde::Deserialize;
use serde_json::json;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use thermark::config::Config;
use thermark::geometry::LabelMm;
use thermark::label::{QrLabelOptions, TextAlign, TextLabelOptions, TextSide};
use thermark::profile::PrinterProfile;
use thermark::transport::Transport;
use thermark::{Density, Threshold};

use crate::cli::args::BatchCommand;
use crate::cli::session::{
    Session, combine_job_and_close, ensure_target_print_allowed, resolve_target,
};

const MAX_INPUT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ROWS: usize = 1000;
const MAX_RENDER_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Text,
    Qr,
}

/// Layout settings only: CSV owns content; CLI overrides template settings.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Template {
    kind: Option<Kind>,
    label: Option<String>,
    font: Option<std::path::PathBuf>,
    font_name: Option<String>,
    font_size: Option<f32>,
    align: Option<String>,
    text_side: Option<String>,
    #[serde(default)]
    border: bool,
    density: Option<u8>,
}

struct Row {
    text: String,
    url: Option<String>,
}

struct Layout {
    template: Template,
    label: LabelMm,
    align: TextAlign,
    text_side: TextSide,
    density: Density,
}

fn read_limited(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)
        .with_context(|| format!("open {}", path.display()))?
        .take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= MAX_INPUT_BYTES, "input exceeds 8 MiB");
    Ok(bytes)
}

fn load_rows(path: &Path, kind: Option<Kind>) -> Result<Vec<Row>> {
    let bytes = read_limited(path)?;
    let mut reader = csv::ReaderBuilder::new().from_reader(bytes.as_slice());
    let headers = reader.headers().context("read CSV headers")?.clone();
    ensure!(
        headers.iter().all(|h| matches!(h, "text" | "url")),
        "CSV headers must be text and optionally url"
    );
    ensure!(
        headers.iter().filter(|h| *h == "text").count() == 1,
        "CSV needs exactly one text column"
    );
    ensure!(
        headers.iter().filter(|h| *h == "url").count() <= 1,
        "duplicate url column"
    );
    let text_col = headers
        .iter()
        .position(|h| h == "text")
        .expect("validated text column");
    let url_col = headers.iter().position(|h| h == "url");
    let kind = kind.unwrap_or(if url_col.is_some() {
        Kind::Qr
    } else {
        Kind::Text
    });
    ensure!(
        !matches!(kind, Kind::Qr) || url_col.is_some(),
        "QR template needs a url column"
    );
    ensure!(
        !matches!(kind, Kind::Text) || url_col.is_none(),
        "text template cannot discard a url column; use a QR template"
    );
    let mut rows = Vec::new();
    for (index, record) in reader.records().enumerate() {
        let record = record.with_context(|| format!("CSV data row {}", index + 1))?;
        ensure!(
            index < MAX_ROWS,
            "batch exceeds {MAX_ROWS} labels; split the CSV"
        );
        let text = record[text_col].replace("\\n", "\n");
        ensure!(
            !text.trim().is_empty(),
            "CSV data row {} has empty text",
            index + 1
        );
        let url = url_col.map(|column| record[column].to_owned());
        ensure!(
            !url.as_ref().is_some_and(|value| value.trim().is_empty()),
            "CSV data row {} has empty url",
            index + 1
        );
        rows.push(Row { text, url });
    }
    ensure!(!rows.is_empty(), "CSV has no data rows");
    Ok(rows)
}

fn load_layout(cfg: &Config, args: &BatchCommand) -> Result<Layout> {
    let mut template: Template = if let Some(path) = &args.template {
        let mut template: Template =
            serde_json::from_slice(&read_limited(path)?).context("parse batch template")?;
        if let Some(font) = &mut template.font
            && font.is_relative()
        {
            *font = path.parent().unwrap_or_else(|| Path::new(".")).join(&*font);
        }
        template
    } else {
        Template::default()
    };
    // An explicit font selector replaces the template's selector as a pair.
    if args.font.font.is_some() || args.font.font_name.is_some() {
        template.font = args.font.font.clone();
        template.font_name = args.font.font_name.clone();
    }
    template.font_size = args.font.font_size.or(template.font_size);
    if let Some(size) = template.font_size {
        thermark::font::validate_font_size(size)?;
    }
    let align = TextAlign::from_str(template.align.as_deref().unwrap_or("center"), false)
        .map_err(anyhow::Error::msg)?;
    let text_side = TextSide::from_str(template.text_side.as_deref().unwrap_or("right"), false)
        .map_err(anyhow::Error::msg)?;
    let density = match args.density {
        Some(density) => density,
        None => Density::new(template.density.unwrap_or(4))?,
    };
    let label =
        LabelMm::parse(&cfg.resolve_label(args.label.as_deref().or(template.label.as_deref())))?;
    Ok(Layout {
        template,
        label,
        align,
        text_side,
        density,
    })
}

fn render_all(
    cfg: &Config,
    rows: &[Row],
    layout: &Layout,
    profile: &PrinterProfile,
) -> Result<Vec<GrayImage>> {
    let label = layout
        .label
        .to_pixels(profile.max_width_px, profile.pixels_per_mm());
    let total = (label.width_px as usize)
        .checked_mul(label.height_px as usize)
        .and_then(|bytes| bytes.checked_mul(rows.len()));
    ensure!(
        total.is_some_and(|bytes| bytes <= MAX_RENDER_BYTES),
        "rendered batch exceeds 64 MiB; split the CSV"
    );
    let safe = cfg.resolve_safe_area(profile.pixels_per_mm());
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            (|| -> Result<GrayImage> {
                let gray = if let Some(url) = &row.url {
                    thermark::make_qr_label_opts(&QrLabelOptions {
                        url: url.clone(),
                        side_text: row.text.clone(),
                        label,
                        safe,
                        text_side: layout.text_side,
                        border: layout.template.border,
                        font_path: layout.template.font.clone(),
                        font_name: layout.template.font_name.clone(),
                        font_size: layout.template.font_size,
                    })?
                } else {
                    thermark::make_text_label(&TextLabelOptions {
                        text: row.text.clone(),
                        label,
                        safe,
                        align: layout.align,
                        border: layout.template.border,
                        font_path: layout.template.font.clone(),
                        font_name: layout.template.font_name.clone(),
                        font_size: layout.template.font_size,
                    })?
                };
                // Save exactly the black/white pixels used by print_gray_image.
                Ok(thermark::image_encode::render_print_preview(
                    &gray,
                    profile.max_width_px,
                    Threshold::DEFAULT.get(),
                    false,
                )?)
            })()
            .with_context(|| format!("render CSV data row {}", index + 1))
        })
        .collect()
}

fn save_previews(
    dir: &Path,
    images: &[GrayImage],
    profile: &PrinterProfile,
    start_at: u32,
) -> Result<()> {
    if let Some(parent) = dir.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(dir).with_context(|| {
        format!(
            "create fresh preview directory {} (use a new path for each run)",
            dir.display()
        )
    })?;
    for (index, image) in images.iter().enumerate() {
        image.save(dir.join(format!("label-{:05}.png", index + 1)))?;
    }
    let manifest = json!({
        "schema_version": 1, "model": profile.model, "dpi": profile.dpi,
        "total": images.len(), "start_at": start_at,
        "width_px": images[0].width(), "height_px": images[0].height(),
        "note": "Print outcomes are recorded only in journal.jsonl; previews contain label content."
    });
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    println!(
        "Validated {} labels; previews saved in {}",
        images.len(),
        dir.display()
    );
    Ok(())
}

/// Append-only outcomes, synced before a job can act and after confirmation.
/// A trailing partial line after a crash cannot invalidate earlier entries.
struct Journal(File);

impl Journal {
    fn create(dir: &Path, total: usize, start_at: u32) -> Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join("journal.jsonl"))?;
        let mut journal = Self(file);
        journal.record(json!({"schema_version": 1, "status": "prepared", "total": total, "start_at": start_at}))?;
        Ok(journal)
    }

    fn record(&mut self, value: serde_json::Value) -> Result<()> {
        serde_json::to_writer(&mut self.0, &value)?;
        self.0.write_all(b"\n")?;
        self.0.sync_all().context("sync batch journal")
    }
}

async fn print_rows<T: Transport>(
    session: &mut Session<T>,
    images: &[GrayImage],
    density: Density,
    start_at: u32,
    journal: &mut Journal,
) -> Result<()> {
    for (index, image) in images.iter().enumerate().skip(start_at as usize - 1) {
        let row = index + 1;
        journal.record(json!({"row": row, "status": "uncertain"}))?;
        if let Err(error) = session.print_gray(image, density).await {
            bail!(
                "batch stopped: {} labels confirmed in this run; row {row} uncertain; later rows not sent. Inspect the printed labels and journal.jsonl before using --start-at with a fresh directory. Cause: {error:#}",
                row - start_at as usize
            );
        }
        journal.record(json!({"row": row, "status": "confirmed"}))
            .with_context(|| format!("row {row} printed but confirmation could not be recorded; inspect output before resuming"))?;
        println!("Confirmed label {row}/{}", images.len());
    }
    Ok(())
}

async fn run_connected<T: Transport>(
    mut session: Session<T>,
    cfg: &Config,
    args: &BatchCommand,
    rows: &[Row],
    layout: &Layout,
) -> Result<()> {
    let job = async {
        session.ensure_print_allowed()?;
        let images = render_all(cfg, rows, layout, session.profile())?;
        save_previews(&args.preview_dir, &images, session.profile(), args.start_at)?;
        let mut journal = Journal::create(&args.preview_dir, images.len(), args.start_at)?;
        print_rows(
            &mut session,
            &images,
            layout.density,
            args.start_at,
            &mut journal,
        )
        .await
    }
    .await;
    let close = session.finish().await;
    combine_job_and_close(job, close)
}

pub async fn run(cfg: &Config, args: BatchCommand) -> Result<()> {
    let layout = load_layout(cfg, &args)?;
    let rows = load_rows(&args.csv, layout.template.kind)?;
    ensure!(
        args.start_at as usize <= rows.len(),
        "--start-at exceeds the {} data rows",
        rows.len()
    );
    if !args.print {
        let profile = thermark::profile_for_model(cfg.resolve_model(args.model));
        let images = render_all(cfg, &rows, &layout, profile)?;
        save_previews(&args.preview_dir, &images, profile, args.start_at)?;
        println!(
            "Nothing printed. Inspect the PNGs, then repeat with --print and a fresh --preview-dir."
        );
        return Ok(());
    }
    let target = resolve_target(cfg, args.model, &args.task)?;
    ensure_target_print_allowed(target, cfg.resolve_connection(args.conn.conn))?;
    let conn = args.conn.resolve(cfg)?;
    let session = Session::connect(&conn, target).await?;
    run_connected(session, cfg, &args, &rows, &layout).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{Cli, Commands};
    use clap::Parser;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use thermark::{MockTransport, Packet, protocol::Cmd};

    #[derive(Default)]
    struct Observations {
        starts: usize,
        closes: usize,
        packets: Vec<Packet>,
    }

    struct Observed {
        inner: MockTransport,
        observations: Arc<Mutex<Observations>>,
        fail_on_start: Option<usize>,
        hang: bool,
        waiting: bool,
        dir: std::path::PathBuf,
    }

    impl Transport for Observed {
        async fn send_raw(&mut self, bytes: &[u8]) -> thermark::Result<()> {
            let packet = Packet::decode(bytes).unwrap();
            if packet.cmd == Cmd::PrintStart as u8 {
                let mut observations = self.observations.lock().unwrap();
                observations.starts += 1;
                // All pages exist and the uncertain marker is on disk before PrintStart.
                assert!(self.dir.join("label-00003.png").exists());
                let journal = fs::read_to_string(self.dir.join("journal.jsonl")).unwrap();
                assert!(journal.lines().last().unwrap().contains("uncertain"));
                if self.fail_on_start == Some(observations.starts) {
                    self.inner.fail_cmd(Cmd::PrintStart as u8, 2);
                }
                self.waiting = self.hang;
            }
            self.observations.lock().unwrap().packets.push(packet);
            self.inner.send_raw(bytes).await
        }

        async fn recv_raw(&mut self, wait: Duration) -> thermark::Result<Vec<u8>> {
            if self.waiting {
                std::future::pending().await
            } else {
                self.inner.recv_raw(wait).await
            }
        }

        async fn close(&mut self) -> thermark::Result<()> {
            self.observations.lock().unwrap().closes += 1;
            self.inner.close().await
        }
    }

    fn command(dir: &Path) -> BatchCommand {
        let cli = Cli::parse_from([
            "thermark",
            "batch",
            "--csv",
            "unused.csv",
            "--preview-dir",
            dir.to_str().unwrap(),
            "--print",
            "--font",
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fonts/DejaVuSans.ttf"),
        ]);
        let Commands::Batch(args) = cli.command else {
            unreachable!()
        };
        args
    }

    fn rows() -> Vec<Row> {
        ["ONE", "TWO", "THREE"]
            .into_iter()
            .map(|text| Row {
                text: text.into(),
                url: None,
            })
            .collect()
    }

    async fn session(
        dir: &Path,
        model_id: u16,
        fail_on_start: Option<usize>,
        hang: bool,
    ) -> (Session<Observed>, Arc<Mutex<Observations>>) {
        let observations = Arc::new(Mutex::new(Observations::default()));
        let mut inner = MockTransport::new();
        inner.set_model_id(model_id);
        let transport = Observed {
            inner,
            observations: Arc::clone(&observations),
            fail_on_start,
            hang,
            waiting: false,
            dir: dir.to_owned(),
        };
        (
            Session::with_test_transport(transport, true).await.unwrap(),
            observations,
        )
    }

    fn journal(dir: &Path) -> Vec<serde_json::Value> {
        fs::read_to_string(dir.join("journal.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[tokio::test(start_paused = true)]
    async fn batch_stops_on_second_job_and_closes_with_durable_outcomes() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("run");
        let args = command(&dir);
        let cfg = Config::default();
        let layout = load_layout(&cfg, &args).unwrap();
        let (session, observations) = session(&dir, 4096, Some(2), false).await;
        let error = run_connected(session, &cfg, &args, &rows(), &layout)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("1 labels confirmed"));
        assert!(error.to_string().contains("row 2 uncertain"));
        let events = observations.lock().unwrap();
        assert_eq!(events.starts, 2);
        assert_eq!(events.closes, 1);
        let outcomes = journal(&dir);
        assert_eq!(outcomes[1], json!({"row": 1, "status": "uncertain"}));
        assert_eq!(outcomes[2], json!({"row": 1, "status": "confirmed"}));
        assert_eq!(outcomes[3], json!({"row": 2, "status": "uncertain"}));
        assert_eq!(outcomes.len(), 4);
    }

    #[tokio::test(start_paused = true)]
    async fn batch_uses_detected_geometry_and_resumes_only_selected_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("run");
        let mut args = command(&dir);
        args.start_at = 2;
        let cfg = Config::default();
        let layout = load_layout(&cfg, &args).unwrap();
        let (session, observations) = session(&dir, 4097, None, false).await; // B1 Pro, 300 dpi
        run_connected(session, &cfg, &args, &rows(), &layout)
            .await
            .unwrap();
        assert_eq!(
            image::open(dir.join("label-00001.png"))
                .unwrap()
                .to_luma8()
                .dimensions(),
            (567, 354)
        );
        let events = observations.lock().unwrap();
        assert_eq!(events.starts, 2);
        assert_eq!(events.closes, 1);
        let pages: Vec<_> = events
            .packets
            .iter()
            .filter(|p| p.cmd == Cmd::SetPageSize as u8)
            .collect();
        assert_eq!(pages.len(), 2);
        for page in pages {
            assert_eq!(u16::from_be_bytes([page.data[0], page.data[1]]), 354);
            assert_eq!(u16::from_be_bytes([page.data[2], page.data[3]]), 567);
        }
        let outcomes = journal(&dir);
        assert_eq!(outcomes.len(), 5);
        assert_eq!(outcomes[1]["row"], 2);
        assert_eq!(outcomes[4], json!({"row": 3, "status": "confirmed"}));
    }

    #[tokio::test]
    async fn late_render_error_and_save_error_send_no_jobs_and_close() {
        for save_error in [false, true] {
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path().join("run");
            let args = command(&dir);
            let cfg = Config::default();
            let layout = load_layout(&cfg, &args).unwrap();
            let mut input = rows();
            if save_error {
                fs::create_dir(&dir).unwrap();
            } else {
                input[2].text = String::new();
            }
            let (session, observations) = session(&dir, 4096, None, false).await;
            let error = run_connected(session, &cfg, &args, &input, &layout)
                .await
                .unwrap_err();
            if !save_error {
                assert!(error.to_string().contains("data row 3"));
                assert!(!dir.exists());
            }
            let events = observations.lock().unwrap();
            assert_eq!(events.starts, 0);
            assert_eq!(events.closes, 1);
            assert!(!dir.join("journal.jsonl").exists());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn interrupted_print_leaves_uncertain_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("run");
        let args = command(&dir);
        let cfg = Config::default();
        let layout = load_layout(&cfg, &args).unwrap();
        let (session, observations) = session(&dir, 4096, None, true).await;
        // Dropping the command future models main's Ctrl-C branch. Native transport
        // disconnect-on-drop is tested separately; this verifies the batch receipt.
        assert!(
            tokio::time::timeout(
                Duration::from_millis(10),
                run_connected(session, &cfg, &args, &rows(), &layout)
            )
            .await
            .is_err()
        );
        assert_eq!(observations.lock().unwrap().starts, 1);
        assert_eq!(
            journal(&dir).last().unwrap(),
            &json!({"row": 1, "status": "uncertain"})
        );
    }

    #[tokio::test]
    async fn unrecognized_hardware_cannot_render_or_print_a_batch() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("run");
        let args = command(&dir);
        let cfg = Config::default();
        let layout = load_layout(&cfg, &args).unwrap();
        let (session, observations) = session(&dir, 65535, None, false).await;
        assert!(
            run_connected(session, &cfg, &args, &rows(), &layout)
                .await
                .unwrap_err()
                .to_string()
                .contains("unrecognized")
        );
        assert!(!dir.exists());
        assert_eq!(observations.lock().unwrap().closes, 1);
    }
}
