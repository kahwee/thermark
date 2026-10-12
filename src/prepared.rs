//! Disk-backed, content-addressed monochrome jobs. Hashes detect changes, not authorship.
use crate::{Density, profile::PrinterProfile};
use anyhow::{Context, Result, ensure};
use image::GrayImage;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const MAX_PAGE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_PAGES: usize = 1000;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

pub fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub(crate) fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)
        .with_context(|| format!("open {}", path.display()))?
        .take(max + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= max,
        "{} exceeds size limit",
        path.display()
    );
    Ok(bytes)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub row: u32,
    pub sha256: String,
    pub width_px: u32,
    pub height_px: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u8,
    pub job_id: String,
    pub model: String,
    pub dpi: u16,
    pub density: u8,
    pub print_task: Option<String>,
    pub total: usize,
    pub start_at: u32,
    pub width_px: u32,
    pub height_px: u32,
    pub font_sha256: String,
    pub font_face: u32,
    pub input_sha256: String,
    pub layout_sha256: String,
    pub pages: Vec<Page>,
}
impl Manifest {
    fn content_id(&self) -> Result<String> {
        let mut content = self.clone();
        content.job_id.clear();
        Ok(digest(&serde_json::to_vec(&content)?))
    }
}

/// Validated descriptor; pages are checked again when read for execution.
pub struct PreparedJob {
    directory: PathBuf,
    manifest: Manifest,
}
impl PreparedJob {
    pub fn open(directory: &Path) -> Result<Self> {
        let manifest: Manifest = serde_json::from_slice(&read_bounded(
            &directory.join("manifest.json"),
            1024 * 1024,
        )?)?;
        ensure!(
            manifest.schema_version == 2,
            "unsupported prepared-job schema; prepare a new job"
        );
        ensure!(
            !manifest.pages.is_empty()
                && manifest.pages.len() <= MAX_PAGES
                && manifest.total == manifest.pages.len(),
            "invalid page count"
        );
        ensure!(
            manifest.start_at >= 1 && manifest.start_at as usize <= manifest.total,
            "invalid start row"
        );
        Density::new(manifest.density)?;
        if let Some(task) = &manifest.print_task {
            ensure!(
                crate::PrintTask::parse(task).is_some(),
                "invalid prepared task"
            );
        }
        ensure!(
            manifest.job_id == manifest.content_id()?,
            "job manifest changed: content hash mismatch"
        );
        let model = <crate::protocol::Model as clap::ValueEnum>::from_str(&manifest.model, false)
            .map_err(anyhow::Error::msg)?;
        let profile = crate::profile_for_model(model);
        ensure!(
            manifest.dpi == profile.dpi,
            "job DPI differs from registered profile"
        );
        for (index, page) in manifest.pages.iter().enumerate() {
            ensure!(page.row as usize == index + 1, "invalid page order");
            ensure!(
                page.width_px > 0
                    && page.width_px <= profile.max_width_px
                    && page.height_px > 0
                    && page.height_px <= u16::MAX as u32,
                "invalid page dimensions"
            );
            ensure!(
                page.width_px == manifest.width_px && page.height_px == manifest.height_px,
                "page geometry differs from manifest"
            );
            ensure!(
                page.width_px as u64 * page.height_px as u64 <= MAX_PAGE_BYTES as u64,
                "page exceeds memory limit"
            );
        }
        let job = Self {
            directory: directory.to_owned(),
            manifest,
        };
        job.verify()?;
        Ok(job)
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn density(&self) -> Density {
        Density::new(self.manifest.density).expect("validated density")
    }
    pub fn verify_profile(&self, profile: &PrinterProfile) -> Result<()> {
        ensure!(
            self.manifest.model == profile.model.to_string() && self.manifest.dpi == profile.dpi,
            "prepared job targets {} at {} DPI, connected printer is {} at {} DPI; prepare and approve a new job",
            self.manifest.model,
            self.manifest.dpi,
            profile.model,
            profile.dpi
        );
        Ok(())
    }
    pub fn verify(&self) -> Result<()> {
        for row in 1..=self.manifest.total as u32 {
            self.page(row)?;
        }
        Ok(())
    }
    pub fn page(&self, row: u32) -> Result<GrayImage> {
        let page = self
            .manifest
            .pages
            .get(row.checked_sub(1).context("row starts at 1")? as usize)
            .context("row outside job")?;
        let bytes = read_bounded(
            &self.directory.join(format!("label-{row:05}.png")),
            MAX_FILE_BYTES,
        )?;
        ensure!(
            digest(&bytes) == page.sha256,
            "prepared page {row} changed: hash mismatch"
        );
        let reader =
            image::ImageReader::with_format(std::io::Cursor::new(&bytes), image::ImageFormat::Png);
        ensure!(
            reader.into_dimensions()? == (page.width_px, page.height_px),
            "page {row} dimensions changed"
        );
        let mut reader =
            image::ImageReader::with_format(std::io::Cursor::new(&bytes), image::ImageFormat::Png);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(page.width_px);
        limits.max_image_height = Some(page.height_px);
        limits.max_alloc = Some(MAX_PAGE_BYTES as u64);
        reader.limits(limits);
        let image = reader.decode()?.into_luma8();
        ensure!(
            image.as_raw().iter().all(|p| *p == 0 || *p == 255),
            "page {row} is not monochrome"
        );
        Ok(image)
    }
    pub(crate) fn seal(directory: &Path, mut manifest: Manifest) -> Result<Self> {
        manifest.job_id = manifest.content_id()?;
        fs::write(
            directory.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        Self::open(directory)
    }
}
