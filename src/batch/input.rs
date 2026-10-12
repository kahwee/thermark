use super::*;
use clap::ValueEnum;
use serde::Deserialize;
use std::io::Read;
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Text,
    Qr,
}

/// Layout settings only: CSV owns content; CLI overrides template settings.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Template {
    pub(super) kind: Option<Kind>,
    pub(super) label: Option<String>,
    pub(super) font: Option<std::path::PathBuf>,
    pub(super) font_name: Option<String>,
    pub(super) font_size: Option<f32>,
    pub(super) align: Option<String>,
    pub(super) text_side: Option<String>,
    #[serde(default)]
    pub(super) border: bool,
    pub(super) density: Option<u8>,
}

pub struct Row {
    pub text: String,
    pub url: Option<String>,
}

pub struct Layout {
    pub(super) template: Template,
    pub(super) label: LabelMm,
    pub(super) align: TextAlign,
    pub(super) text_side: TextSide,
    pub(super) density: Density,
}

impl Layout {
    pub fn kind(&self) -> Option<Kind> {
        self.template.kind
    }
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

pub fn load_rows(path: &Path, kind: Option<Kind>) -> Result<Vec<Row>> {
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

pub fn load_layout(cfg: &Config, args: &BatchOptions) -> Result<Layout> {
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
    if args.font.path.is_some() || args.font.name.is_some() {
        template.font = args.font.path.clone();
        template.font_name = args.font.name.clone();
    }
    template.font_size = args.font.size.or(template.font_size);
    if let Some(size) = template.font_size {
        crate::font::validate_font_size(size)?;
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
