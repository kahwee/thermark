//! CSV batches reuse the existing label renderers and keep a durable print journal.

use crate::config::Config;
use crate::geometry::LabelMm;
use crate::label::{QrLabelOptions, TextAlign, TextLabelOptions, TextSide};
use crate::profile::PrinterProfile;
use crate::transport::Transport;
use crate::{Density, Threshold};
use anyhow::{Context, Result, bail, ensure};
use image::GrayImage;
use serde_json::json;
use std::fs::{self, File};
use std::path::Path;

use crate::font::LabelFont;
use crate::prepared::{MAX_PAGE_BYTES, Manifest, Page, PreparedJob, digest};
use crate::session::{Session, combine_job_and_close};
mod journal;
pub use journal::{Event, JobReport, Journal, Outcome};

#[derive(Debug, Clone, Default)]
pub struct FontSelection {
    pub path: Option<std::path::PathBuf>,
    pub name: Option<String>,
    pub size: Option<f32>,
}
#[derive(Debug, Clone)]
pub struct BatchOptions {
    pub csv: std::path::PathBuf,
    pub template: Option<std::path::PathBuf>,
    pub preview_dir: std::path::PathBuf,
    pub start_at: u32,
    pub font: FontSelection,
    pub label: Option<String>,
    pub density: Option<Density>,
    pub task: Option<crate::PrintTask>,
}
impl Default for BatchOptions {
    fn default() -> Self {
        Self {
            csv: Default::default(),
            template: None,
            preview_dir: Default::default(),
            start_at: 1,
            font: Default::default(),
            label: None,
            density: None,
            task: None,
        }
    }
}

const MAX_INPUT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ROWS: usize = 1000;

mod input;
pub use input::{Kind, Layout, Row, load_layout, load_rows};

/// Prepare a complete batch on disk using one loaded font and one page in memory.
/// A failed preparation does not publish a partial output directory.
pub fn prepare(cfg: &Config, args: &BatchOptions, profile: &PrinterProfile) -> Result<PreparedJob> {
    let layout = load_layout(cfg, args)?;
    let rows = load_rows(&args.csv, layout.template.kind)?;
    prepare_rows(cfg, args, &rows, &layout, profile)
}
fn prepare_rows(
    cfg: &Config,
    args: &BatchOptions,
    rows: &[Row],
    layout: &Layout,
    profile: &PrinterProfile,
) -> Result<PreparedJob> {
    ensure!(
        !rows.is_empty()
            && rows.len() <= MAX_ROWS
            && args.start_at >= 1
            && args.start_at as usize <= rows.len(),
        "--start-at exceeds the {} data rows",
        rows.len()
    );
    ensure!(
        !args.preview_dir.exists(),
        "create fresh preview directory {} (use a new path for each run)",
        args.preview_dir.display()
    );
    let parent = args
        .preview_dir
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".thermark-prepare-")
        .tempdir_in(parent)?;
    let label = layout
        .label
        .to_pixels(profile.max_width_px, profile.pixels_per_mm());
    ensure!(
        label.width_px as u64 * label.height_px as u64 <= MAX_PAGE_BYTES as u64,
        "rendered page exceeds 64 MiB"
    );
    let safe = cfg.resolve_safe_area(profile.pixels_per_mm());
    let font = match (&layout.template.font, &layout.template.font_name) {
        (Some(path), _) => LabelFont::load(path)?,
        (_, Some(name)) => LabelFont::load_named(name)?,
        _ => LabelFont::load_default()?,
    };
    let mut pages = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        let gray = (|| -> Result<GrayImage> {
            let image = if let Some(url) = &row.url {
                crate::label::make_qr_label_with_font(
                    &QrLabelOptions {
                        url: url.clone(),
                        side_text: row.text.clone(),
                        label,
                        safe,
                        text_side: layout.text_side,
                        border: layout.template.border,
                        font_path: None,
                        font_name: None,
                        font_size: layout.template.font_size,
                    },
                    &font,
                )?
            } else {
                crate::label::make_text_label_with_font(
                    &TextLabelOptions {
                        text: row.text.clone(),
                        label,
                        safe,
                        align: layout.align,
                        border: layout.template.border,
                        font_path: None,
                        font_name: None,
                        font_size: layout.template.font_size,
                    },
                    &font,
                )?
            };
            Ok(crate::image_encode::render_print_preview(
                &image,
                profile.max_width_px,
                Threshold::DEFAULT.get(),
                false,
            )?)
        })()
        .with_context(|| format!("render CSV data row {}", index + 1))?;
        let row = index as u32 + 1;
        let path = staging.path().join(format!("label-{row:05}.png"));
        gray.save(&path)?;
        File::open(&path)?.sync_all()?;
        pages.push(Page {
            row,
            sha256: digest(&fs::read(&path)?),
            width_px: gray.width(),
            height_px: gray.height(),
        });
    }
    // Hash actual resolved content rather than paths or ambient environment.
    let input = rows
        .iter()
        .map(|r| json!({"text":r.text,"url":r.url}))
        .collect::<Vec<_>>();
    let layout_json = json!({"label":[label.width_px,label.height_px],"safe":[safe.top,safe.bottom,safe.left,safe.right],"font_size":layout.template.font_size,"align":format!("{:?}",layout.align),"text_side":format!("{:?}",layout.text_side),"border":layout.template.border});
    PreparedJob::seal(
        staging.path(),
        Manifest {
            schema_version: 2,
            job_id: String::new(),
            model: profile.model.to_string(),
            dpi: profile.dpi,
            density: layout.density.get(),
            print_task: args.task.or(profile.default_task).map(|t| t.to_string()),
            total: pages.len(),
            start_at: args.start_at,
            width_px: label.width_px,
            height_px: label.height_px,
            font_sha256: font.fingerprint(),
            font_face: font.index(),
            input_sha256: digest(&serde_json::to_vec(&input)?),
            layout_sha256: digest(&serde_json::to_vec(&layout_json)?),
            pages,
        },
    )?;
    File::open(staging.path().join("manifest.json"))?.sync_all()?;
    #[cfg(unix)]
    File::open(staging.path())?.sync_all()?;
    // rename_noreplace semantics via persist with an exclusive destination creation
    // are not portable for directories: reserve the name, then rename into it.
    fs::create_dir(&args.preview_dir).context("reserve fresh job directory")?;
    if let Err(error) = fs::rename(staging.path(), &args.preview_dir) {
        let _ = fs::remove_dir(&args.preview_dir);
        return Err(error.into());
    }
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    PreparedJob::open(&args.preview_dir)
}

/// Verify all pages before the first job, then recheck each page on consumption.
/// Confirmed rows are skipped on resume; uncertain rows require explicit review.
pub async fn execute<T: Transport>(
    session: &mut Session<T>,
    job: &PreparedJob,
    start_at: u32,
    resume: bool,
    acknowledge_uncertain: bool,
    mut progress: impl FnMut(u32),
) -> Result<()> {
    session.ensure_print_allowed()?;
    job.verify_profile(session.profile())?;
    ensure!(
        job.manifest().print_task.as_deref() == Some(session.print_task().to_string().as_str()),
        "connected print task differs from prepared job"
    );
    ensure!(
        start_at >= 1 && start_at as usize <= job.manifest().total,
        "start row outside job"
    );
    job.verify()?;
    let (mut journal, report) = Journal::create(job, resume, acknowledge_uncertain)?;
    let mut confirmed = 0usize;
    for row in start_at..=job.manifest().total as u32 {
        if resume && report.rows[row as usize - 1].outcome == Some(Outcome::Confirmed) {
            continue;
        }
        let image = job.page(row)?;
        journal.record(Outcome::Uncertain, Some(row), None)?;
        if let Err(error) = session
            .print_gray_with_progress(&image, job.density(), |stage| {
                journal
                    .record(Outcome::Stage, Some(row), Some(stage))
                    .map_err(|error| crate::Error::msg(format!("record print stage: {error:#}")))
            })
            .await
        {
            bail!(
                "batch stopped: {confirmed} labels confirmed in this run; row {row} uncertain; later rows not sent. Inspect the printed labels and journal.jsonl before resuming. Cause: {error:#}"
            );
        }
        journal.record(Outcome::Confirmed,Some(row),None).with_context(||format!("row {row} printed but confirmation could not be recorded; inspect output before resuming"))?;
        confirmed += 1;
        progress(row);
    }
    Ok(())
}

/// Prepare using detected geometry, execute, and close even after failures.
pub async fn prepare_and_print<T: Transport>(
    mut session: Session<T>,
    cfg: &Config,
    args: &BatchOptions,
) -> Result<()> {
    let result = async {
        session.ensure_print_allowed()?;
        let mut resolved = args.clone();
        resolved.task = Some(session.print_task());
        let job = prepare(cfg, &resolved, session.profile())?;
        execute(&mut session, &job, args.start_at, false, false, |_| {}).await
    }
    .await;
    let close = session.finish().await;
    combine_job_and_close(result, close)
}
#[cfg(test)]
async fn run_connected<T: Transport>(
    mut session: Session<T>,
    cfg: &Config,
    args: &BatchOptions,
    rows: &[Row],
    layout: &Layout,
) -> Result<()> {
    let result = async {
        session.ensure_print_allowed()?;
        let job = prepare_rows(cfg, args, rows, layout, session.profile())?;
        execute(&mut session, &job, args.start_at, false, false, |_| {}).await
    }
    .await;
    let close = session.finish().await;
    combine_job_and_close(result, close)
}
#[cfg(test)]
mod tests;
