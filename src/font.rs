//! Deterministic default fonts and script-aware text layout.

use crate::errors::{Error, Result};
use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use image::{GrayImage, Luma};
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Smallest and largest text size [`LabelFont::fit_size`] will choose.
pub const MIN_FONT_PX: f32 = 10.0;
pub const MAX_FONT_PX: f32 = 72.0;

/// Validate an explicit font size before layout. Rendering retains its
/// historical 6..=96 px clamp for finite, positive requests.
pub fn validate_font_size(size: f32) -> Result<f32> {
    if !size.is_finite() || size <= 0.0 {
        return Err(Error::msg("font size must be a finite, positive number"));
    }
    Ok(size)
}

/// Well-known font locations on macOS (and a few cross-platform fallbacks).
pub fn system_font_candidates() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut paths = vec![
        // Helvetica / Times (macOS)
        PathBuf::from("/System/Library/Fonts/Helvetica.ttc"),
        PathBuf::from("/System/Library/Fonts/HelveticaNeue.ttc"),
        PathBuf::from("/System/Library/Fonts/Times.ttc"),
        PathBuf::from("/System/Library/Fonts/NewYork.ttf"),
        PathBuf::from("/System/Library/Fonts/Supplemental/Times New Roman Bold.ttf"),
        PathBuf::from("/System/Library/Fonts/Supplemental/Times New Roman.ttf"),
        // Clean single-file TTFs
        PathBuf::from("/System/Library/Fonts/Supplemental/Arial Bold.ttf"),
        PathBuf::from("/System/Library/Fonts/Supplemental/Arial.ttf"),
        PathBuf::from("/System/Library/Fonts/Supplemental/Courier New Bold.ttf"),
        PathBuf::from("/System/Library/Fonts/Supplemental/Courier New.ttf"),
        PathBuf::from("/System/Library/Fonts/Supplemental/Arial Unicode.ttf"),
        PathBuf::from("/Library/Fonts/Arial.ttf"),
        PathBuf::from("/Library/Fonts/Arial Bold.ttf"),
        PathBuf::from("/Library/Fonts/SF-Pro.ttf"),
        PathBuf::from("/Library/Fonts/SF-Compact.ttf"),
        // Linux common
        PathBuf::from("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"),
        PathBuf::from("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
        PathBuf::from("/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf"),
        PathBuf::from("/usr/share/fonts/truetype/liberation/LiberationSerif-Bold.ttf"),
    ];
    if let Some(h) = home {
        paths.push(h.join("Library/Fonts/Arial.ttf"));
        paths.push(h.join("Library/Fonts/MesloLGS NF Regular.ttf"));
        paths.push(h.join("Library/Fonts/MesloLGS NF Bold.ttf"));
    }
    paths
}

fn bail_no_font(name: &str) -> Result<LabelFont> {
    Err(Error::font(format!(
        "could not load font '{name}'. Try: --font-name helvetica | times | arial \
         or --font /path/to.ttf  (run `thermark fonts` to list)"
    )))
}

/// Find the first readable candidate, optionally preferring a name substring (e.g. "Arial", "Bold").
pub fn find_system_font(prefer: Option<&str>) -> Option<PathBuf> {
    let prefer = prefer.map(|s| s.to_ascii_lowercase());
    let cands = system_font_candidates();
    // Let-chain (edition 2024): one condition instead of nested `if let`s.
    if let Some(p) = &prefer
        && let Some(hit) = cands.iter().find(|path| {
            path.exists()
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.to_ascii_lowercase().contains(p.as_str()))
        })
    {
        return Some(hit.clone());
    }
    cands.into_iter().find(|p| p.exists())
}

/// List existing candidate fonts (for CLI `fonts` command).
pub fn list_available_fonts() -> Vec<PathBuf> {
    system_font_candidates()
        .into_iter()
        .filter(|p| p.exists())
        .collect()
}

/// Loaded font ready to draw.
type GlyphLayout = (Vec<ab_glyph::Glyph>, f32);
type LayoutCache = std::collections::HashMap<(String, u32), GlyphLayout>;

pub struct LabelFont {
    data: std::sync::Arc<[u8]>,
    shaping_font: harfrust::Font,
    cache: std::sync::Mutex<LayoutCache>,
    path: PathBuf,
    embedded: bool,
    /// Face index inside a .ttc collection (0 for single-font files).
    index: u32,
}

impl LabelFont {
    pub fn load(path: &Path) -> Result<Self> {
        Self::load_index(path, 0)
    }

    /// Load a face from a `.ttf`/`.otf` or a face `index` inside a `.ttc` collection.
    pub fn load_index(path: &Path, index: u32) -> Result<Self> {
        let data = std::fs::read(path)
            .map_err(|e| Error::font(format!("read font {}: {e}", path.display())))?;
        FontRef::try_from_slice_and_index(&data, index).map_err(|e| {
            Error::font(format!(
                "parse font {} (index {index}): {e:?}",
                path.display()
            ))
        })?;
        let data: std::sync::Arc<[u8]> = data.into();
        let shaping_font = harfrust::Font::new(
            harfrust::font::Blob::Shared(std::sync::Arc::new(data.clone())),
            index,
        )
        .ok_or_else(|| Error::font("font cannot be shaped as OpenType"))?;
        Ok(Self {
            data,
            shaping_font,
            cache: Default::default(),
            path: path.to_path_buf(),
            embedded: false,
            index,
        })
    }

    /// Try common system names: "helvetica", "times", "arial", etc.
    pub fn load_named(name: &str) -> Result<Self> {
        let key = name.to_ascii_lowercase();
        let tries: &[(&str, u32)] = match key.as_str() {
            "helvetica" | "helvetica neue" | "helv" => &[
                ("/System/Library/Fonts/Helvetica.ttc", 0),
                ("/System/Library/Fonts/HelveticaNeue.ttc", 0),
            ],
            "helvetica bold" | "helvetica-bold" => &[
                // Bold face is often a later index in the collection
                ("/System/Library/Fonts/Helvetica.ttc", 1),
                ("/System/Library/Fonts/HelveticaNeue.ttc", 1),
                ("/System/Library/Fonts/Helvetica.ttc", 0),
            ],
            "times" | "times new roman" | "times-roman" | "tnr" => &[
                (
                    "/System/Library/Fonts/Supplemental/Times New Roman Bold.ttf",
                    0,
                ),
                ("/System/Library/Fonts/Supplemental/Times New Roman.ttf", 0),
                ("/System/Library/Fonts/Times.ttc", 0),
                ("/System/Library/Fonts/NewYork.ttf", 0),
            ],
            "times bold" | "times new roman bold" => &[
                (
                    "/System/Library/Fonts/Supplemental/Times New Roman Bold.ttf",
                    0,
                ),
                ("/System/Library/Fonts/Times.ttc", 1),
            ],
            "arial" | "arial bold" => &[
                ("/System/Library/Fonts/Supplemental/Arial Bold.ttf", 0),
                ("/System/Library/Fonts/Supplemental/Arial.ttf", 0),
            ],
            "courier" | "courier new" => &[
                ("/System/Library/Fonts/Supplemental/Courier New Bold.ttf", 0),
                ("/System/Library/Fonts/Supplemental/Courier New.ttf", 0),
            ],
            _ => &[],
        };

        for (path, idx) in tries {
            let p = Path::new(path);
            if p.exists() {
                match Self::load_index(p, *idx) {
                    Ok(f) => {
                        info!(path = %p.display(), face = idx, "using font");
                        return Ok(f);
                    }
                    Err(e) => {
                        debug!(path = %p.display(), face = idx, error = %e, "skip font");
                    }
                }
            }
        }

        // Fall back to path / substring search among candidates
        if let Some(path) = find_system_font(Some(&key)) {
            info!(path = %path.display(), "using font");
            return Self::load(&path);
        }

        bail_no_font(name)
    }

    /// Embedded, redistributable DejaVu Sans; independent of host font installation.
    pub fn load_default() -> Result<Self> {
        let data: std::sync::Arc<[u8]> = include_bytes!("../assets/fonts/DejaVuSans.ttf")
            .as_slice()
            .into();
        Ok(Self {
            shaping_font: harfrust::Font::new(
                include_bytes!("../assets/fonts/DejaVuSans.ttf").as_slice(),
                0,
            )
            .expect("valid embedded font"),
            data,
            cache: Default::default(),
            path: "<embedded DejaVu Sans>".into(),
            embedded: true,
            index: 0,
        })
    }

    pub fn fingerprint(&self) -> String {
        crate::prepared::digest(&self.data)
    }

    /// Reject missing characters before any pixels are drawn.
    pub fn validate_text(&self, text: &str) -> Result<()> {
        let font = self.font();
        for ch in text.chars().filter(|c| !c.is_whitespace() && !matches!(*c,'\u{200c}'|'\u{200d}'|'\u{200e}'|'\u{200f}'|'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')) {
            if font.glyph_id(ch).0==0 { return Err(Error::font(format!("font has no glyph for U+{:04X}; choose --font with coverage for this text",ch as u32))); }
        }
        Ok(())
    }

    /// ASCII keeps historical placement; Unicode uses bidi runs and OpenType shaping.
    fn layout(&self, text: &str, px_height: f32) -> (Vec<ab_glyph::Glyph>, f32) {
        let key = (text.to_owned(), px_height.to_bits());
        if let Some(hit) = self.cache.lock().expect("font cache").get(&key) {
            return hit.clone();
        }
        let font = self.font();
        let scale = PxScale::from(px_height);
        let mut glyphs = Vec::new();
        let mut caret = 0.0f32;
        use unicode_script::UnicodeScript;
        let simple_latin=text.chars().all(|ch|matches!(ch.script(),unicode_script::Script::Latin|unicode_script::Script::Common) && !matches!(ch,'\u{200c}'..='\u{200f}'|'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}'));
        if simple_latin {
            let sf = font.as_scaled(scale);
            for ch in text.chars() {
                let id = font.glyph_id(ch);
                glyphs.push(id.with_scale_and_position(scale, ab_glyph::point(caret, 0.0)));
                caret += sf.h_advance(id);
            }
        } else {
            let face = harfrust::ShaperFont::new(&self.shaping_font);
            let factor = px_height / (font.ascent_unscaled() - font.descent_unscaled());
            let bidi = unicode_bidi::BidiInfo::new(text, None);
            for para in &bidi.paragraphs {
                let (levels, runs) = bidi.visual_runs(para, para.range.clone());
                for run in runs {
                    let rtl = levels[run.start].is_rtl();
                    let mut segments = Vec::new();
                    let mut begin = run.start;
                    let mut script = unicode_script::Script::Common;
                    for (offset, ch) in text[run.clone()].char_indices() {
                        let next = ch.script();
                        if !matches!(
                            next,
                            unicode_script::Script::Common
                                | unicode_script::Script::Inherited
                                | unicode_script::Script::Unknown
                        ) {
                            if script != unicode_script::Script::Common && script != next {
                                segments.push(begin..run.start + offset);
                                begin = run.start + offset;
                            }
                            script = next;
                        }
                    }
                    segments.push(begin..run.end);
                    if rtl {
                        segments.reverse();
                    }
                    for segment in segments {
                        let mut buffer = harfrust::Buffer::new();
                        buffer.push_str(&text[segment]);
                        buffer.guess_segment_properties();
                        buffer.set_direction(if rtl {
                            harfrust::Direction::RightToLeft
                        } else {
                            harfrust::Direction::LeftToRight
                        });
                        harfrust::shape(&face, &mut buffer, harfrust::ShapeOptions::default())
                            .expect("new buffer with explicit direction");
                        for (info, pos) in buffer.glyph_infos().iter().zip(buffer.glyph_positions())
                        {
                            glyphs.push(
                                ab_glyph::GlyphId(info.glyph_id as u16).with_scale_and_position(
                                    scale,
                                    ab_glyph::point(
                                        caret + pos.x_offset as f32 * factor,
                                        -pos.y_offset as f32 * factor,
                                    ),
                                ),
                            );
                            caret += pos.x_advance as f32 * factor;
                        }
                    }
                }
            }
        }
        let result = (glyphs, caret);
        let mut cache = self.cache.lock().expect("font cache");
        if result.0.len() > 4096 || text.len() > 16384 {
            return result;
        }
        if cache.len() >= 256
            || cache.values().map(|v| v.0.len()).sum::<usize>() + result.0.len() > 16384
        {
            cache.clear();
        }
        cache.insert(key, result.clone());
        result
    }
    fn positioned(&self, text: &str, px_height: f32, x: f32, y: f32) -> Vec<ab_glyph::Glyph> {
        self.layout(text, px_height)
            .0
            .into_iter()
            .map(|mut glyph| {
                glyph.position.x += x;
                glyph.position.y += y;
                glyph
            })
            .collect()
    }

    /// Filesystem path, or a display-only source name for the embedded font.
    /// Use [`Self::source_path`] before attempting filesystem operations.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Actual source path; embedded fonts have no filesystem dependency.
    pub fn source_path(&self) -> Option<&Path> {
        (!self.embedded).then_some(self.path.as_path())
    }

    pub fn index(&self) -> u32 {
        self.index
    }

    fn font(&self) -> FontRef<'_> {
        FontRef::try_from_slice_and_index(&self.data, self.index).expect("font validated at load")
    }

    /// Pixel width of `text` at `px_height` (approx em size).
    pub fn text_width(&self, text: &str, px_height: f32) -> u32 {
        self.layout(text, px_height).1.ceil().max(0.0) as u32
    }

    /// Distance from the top of a line to its baseline.
    ///
    /// This — not [`Self::text_height`] — is where the first baseline goes.
    /// `text_height` is the whole line box (ascent + descent + gap); using it
    /// as a baseline offset drops every line by a descender, so the last line
    /// overflows the bottom of its box.
    pub fn ascent(&self, px_height: f32) -> u32 {
        let font = self.font();
        let sf = font.as_scaled(PxScale::from(px_height));
        sf.ascent().ceil().max(1.0) as u32
    }

    pub fn text_height(&self, px_height: f32) -> u32 {
        let font = self.font();
        let scale = PxScale::from(px_height);
        let sf = font.as_scaled(scale);
        (sf.ascent() - sf.descent() + sf.line_gap()).ceil().max(1.0) as u32
    }

    /// Draw black text (Luma 0) onto a white-ish label image. Origin = baseline-left.
    pub fn draw_text(
        &self,
        img: &mut GrayImage,
        x: f32,
        baseline_y: f32,
        text: &str,
        px_height: f32,
    ) {
        let font = self.font();
        let (w, h) = img.dimensions();

        for glyph in self.positioned(text, px_height, x, baseline_y) {
            if let Some(outlined) = font.outline_glyph(glyph) {
                let bounds = outlined.px_bounds();
                outlined.draw(|gx, gy, cover| {
                    if cover < 0.3 {
                        return;
                    }
                    let px = bounds.min.x as i32 + gx as i32;
                    let py = bounds.min.y as i32 + gy as i32;
                    if px >= 0 && py >= 0 && (px as u32) < w && (py as u32) < h {
                        // coverage anti-alias → hard threshold for thermal
                        img.put_pixel(px as u32, py as u32, Luma([0]));
                    }
                });
            }
        }
    }

    /// Check the same positioned glyph bounds used by drawing, including
    /// overhangs that advance-width measurements do not account for.
    pub(crate) fn text_ink_fits(
        &self,
        text: &str,
        px_height: f32,
        x: f32,
        baseline_y: f32,
        bounds: crate::geometry::Rect,
    ) -> bool {
        let font = self.font();
        for glyph in self.positioned(text, px_height, x, baseline_y) {
            if let Some(outlined) = font.outline_glyph(glyph) {
                let ink = outlined.px_bounds();
                if ink.min.x < bounds.x as f32
                    || ink.min.y < bounds.y as f32
                    || ink.max.x > (u64::from(bounds.x) + u64::from(bounds.w)) as f32
                    || ink.max.y > (u64::from(bounds.y) + u64::from(bounds.h)) as f32
                {
                    return false;
                }
            }
        }
        true
    }

    /// Vertical extent of the *ink* in a wrapped block, relative to the first
    /// baseline, in pixels.
    ///
    /// Font metrics describe the line box, not the marks in it: `ascent`
    /// reserves room above cap height that most glyphs never use, so centring
    /// on metrics leaves more space above the text than below. Measuring the
    /// glyph outlines instead centres what the eye actually sees.
    ///
    /// Returns `(top, bottom)` — `top` is normally negative, since ink rises
    /// above the baseline. `None` when the text draws nothing (blank lines,
    /// whitespace, or glyphs the font lacks).
    pub fn block_ink_bounds(
        &self,
        lines: &[String],
        px_height: f32,
        line_h: f32,
    ) -> Option<(f32, f32)> {
        let font = self.font();
        let mut min_y = f32::INFINITY;
        let mut max_y = f32::NEG_INFINITY;

        for (i, line) in lines.iter().enumerate() {
            let baseline = i as f32 * line_h;
            for glyph in self.positioned(line, px_height, 0.0, baseline) {
                if let Some(outlined) = font.outline_glyph(glyph) {
                    let b = outlined.px_bounds();
                    min_y = min_y.min(b.min.y);
                    max_y = max_y.max(b.max.y);
                }
            }
        }
        (min_y.is_finite() && max_y.is_finite()).then_some((min_y, max_y))
    }

    /// Wrap text to fit max_width_px.
    pub fn wrap(&self, text: &str, max_width_px: u32, px_height: f32) -> Vec<String> {
        let mut lines = Vec::new();
        for paragraph in text.split('\n') {
            if paragraph.is_empty() {
                lines.push(String::new());
                continue;
            }
            let mut current = String::new();
            for word in paragraph.split_whitespace() {
                let trial = if current.is_empty() {
                    word.to_string()
                } else {
                    format!("{current} {word}")
                };
                if self.text_width(&trial, px_height) <= max_width_px {
                    current = trial;
                } else {
                    if !current.is_empty() {
                        lines.push(std::mem::take(&mut current));
                    }
                    if self.text_width(word, px_height) <= max_width_px {
                        current = word.to_string();
                    } else {
                        // hard-break very long tokens
                        let mut buf = String::new();
                        for ch in unicode_segmentation::UnicodeSegmentation::graphemes(word, true) {
                            let t = format!("{buf}{ch}");
                            if self.text_width(&t, px_height) <= max_width_px {
                                buf = t;
                            } else {
                                if !buf.is_empty() {
                                    lines.push(std::mem::take(&mut buf));
                                }
                                buf = ch.to_string();
                            }
                        }
                        current = buf;
                    }
                }
            }
            if !current.is_empty() {
                lines.push(current);
            }
        }
        lines
    }

    /// Pick the largest px height that fits all lines in the box.
    ///
    /// Prefers sizes at which no word has to be split mid-way: a hard break
    /// technically "fits", so choosing purely on fit renders `THERMARK` as
    /// `THER` / `MARK` at a large size instead of keeping it whole at a
    /// smaller one. Only when even [`MIN_FONT_PX`] cannot hold the longest
    /// word does splitting become acceptable.
    pub fn fit_size(&self, text: &str, max_w: u32, max_h: u32) -> f32 {
        let lo = MIN_FONT_PX as u32;
        let hi = MAX_FONT_PX as u32;
        for require_whole_words in [true, false] {
            for size in (lo..=hi).rev() {
                let ph = size as f32;
                if !self.fits(text, max_w, max_h, ph) {
                    continue;
                }
                if !require_whole_words || self.longest_word_width(text, ph) <= max_w {
                    return ph;
                }
            }
        }
        MIN_FONT_PX
    }

    /// Whether `text` wraps into `max_w` x `max_h` at this size.
    pub fn fits(&self, text: &str, max_w: u32, max_h: u32, px_height: f32) -> bool {
        let lines = self.wrap(text, max_w, px_height);
        let need = lines.len() as u32 * (self.text_height(px_height) + 2);
        let widest = lines
            .iter()
            .map(|l| self.text_width(l, px_height))
            .max()
            .unwrap_or(0);
        need <= max_h && widest <= max_w
    }

    /// Width of the widest whitespace-delimited word at this size.
    ///
    /// A word wider than the column is the only thing that forces
    /// [`Self::wrap`] to break mid-word.
    pub fn longest_word_width(&self, text: &str, px_height: f32) -> u32 {
        text.split_whitespace()
            .map(|w| self.text_width(w, px_height))
            .max()
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_font() -> LabelFont {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts/DejaVuSans.ttf");
        LabelFont::load(&path)
            .unwrap_or_else(|error| panic!("load vendored test font {}: {error}", path.display()))
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn system_font_loads_on_macos() {
        let f =
            LabelFont::load_named("arial bold").expect("macOS should have a supported system font");
        assert!(f.path().exists());
        assert!(f.text_width("ABC", 24.0) > 10);
    }

    #[test]
    fn fit_size_never_returns_a_size_larger_than_the_smallest_tried() {
        let f = test_font();
        // A box too small for any size: the old fallback returned 12.0 — larger
        // than the 10px that had already failed — so text overflowed further.
        let px = f.fit_size("SOME LONGISH LABEL TEXT HERE", 20, 8);
        assert_eq!(px, MIN_FONT_PX);
    }

    #[test]
    fn fit_size_picks_the_largest_size_that_fits() {
        let f = test_font();
        let (w, h) = (127, 228);
        let px = f.fit_size("Wi-Fi\nGuest\nScan to join", w, h);
        assert!(f.fits("Wi-Fi\nGuest\nScan to join", w, h, px));
        if px < MAX_FONT_PX {
            assert!(
                !f.fits("Wi-Fi\nGuest\nScan to join", w, h, px + 1.0),
                "{px} was not the largest fitting size"
            );
        }
    }

    #[test]
    fn fit_size_keeps_words_whole_instead_of_splitting_them() {
        let f = test_font();
        // The real case: a 127px text column beside a QR. Picking purely on
        // "does it fit" rendered THERMARK as THER / MARK, because the
        // hard-broken lines do fit. A smaller size keeps the word intact.
        let (w, h) = (127, 228);
        let text = "THERMARK\nv0.3.0\nQR test";
        let px = f.fit_size(text, w, h);
        assert!(
            f.longest_word_width(text, px) <= w,
            "{px}px still splits a word"
        );
        assert!(f.wrap(text, w, px).iter().any(|l| l == "THERMARK"));
    }

    #[test]
    fn fit_size_still_splits_when_a_word_can_never_fit() {
        let f = test_font();
        // A single token far wider than the column: splitting is the only
        // option, so the whole-word preference must not deadlock.
        let px = f.fit_size("SUPERCALIFRAGILISTIC", 40, 200);
        assert!(px >= MIN_FONT_PX);
    }

    #[test]
    fn font_abc_is_left_to_right() {
        let font = test_font();
        let mut img = GrayImage::from_pixel(200, 60, Luma([255]));
        // baseline ~ 40 for 32px font
        font.draw_text(&mut img, 5.0, 40.0, "ABC", 32.0);
        // Find leftmost and rightmost dark pixels
        let mut min_x = 999u32;
        let mut max_x = 0u32;
        for (x, _y, p) in img.enumerate_pixels() {
            if p[0] < 128 {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
            }
        }
        assert!(max_x > min_x + 20, "ABC should span significant width");

        // A is typically wider left stem; sample: left third should have ink (A), right third (C)
        let mid1 = min_x + (max_x - min_x) / 3;
        let mid2 = min_x + 2 * (max_x - min_x) / 3;
        let left = img
            .enumerate_pixels()
            .filter(|(x, _, p)| *x < mid1 && p[0] < 128)
            .count();
        let right = img
            .enumerate_pixels()
            .filter(|(x, _, p)| *x > mid2 && p[0] < 128)
            .count();
        assert!(
            left > 5 && right > 5,
            "A and C regions should both have ink"
        );
    }
}

#[cfg(test)]
mod shaping_tests {
    use super::*;
    #[test]
    fn default_is_embedded_and_has_stable_identity() {
        let font = LabelFont::load_default().unwrap();
        assert_eq!(
            font.fingerprint(),
            crate::prepared::digest(include_bytes!("../assets/fonts/DejaVuSans.ttf"))
        );
        assert_eq!(font.index(), 0);
        assert!(font.source_path().is_none());
    }
    #[test]
    fn arabic_lam_alef_is_shaped_as_a_ligature() {
        let font = LabelFont::load_default().unwrap();
        font.validate_text("لا").unwrap();
        let (glyphs, width) = font.layout("لا", 32.0);
        assert_eq!(glyphs.len(), 1);
        assert!(width > 0.0);
        assert_ne!(glyphs[0].id.0, 0);
    }
    #[test]
    fn mixed_bidi_and_combining_text_renders_within_validated_bounds() {
        let font = LabelFont::load_default().unwrap();
        for text in ["مرحبا ABC 123", "שלום 42", "café e\u{301}"] {
            let mut image = GrayImage::from_pixel(384, 240, Luma([255]));
            crate::label::draw_text_block(
                &mut image,
                &font,
                text,
                crate::geometry::Rect {
                    x: 8,
                    y: 8,
                    w: 368,
                    h: 224,
                },
                crate::TextAlign::Center,
                None,
            )
            .unwrap();
            assert!(image.as_raw().contains(&0));
        }
    }
    #[test]
    fn missing_glyph_error_preserves_the_canvas() {
        let font = LabelFont::load_default().unwrap();
        let mut image = GrayImage::from_pixel(100, 100, Luma([255]));
        let before = image.clone();
        let error = crate::label::draw_text_block(
            &mut image,
            &font,
            "\u{10ffff}",
            crate::geometry::Rect {
                x: 0,
                y: 0,
                w: 100,
                h: 100,
            },
            crate::TextAlign::Center,
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("no glyph"));
        assert_eq!(image, before);
    }
}

#[cfg(test)]
mod autofit_tests {
    #[test]
    fn trial_qr_side_text_fits_by_measured_ink() {
        let label = crate::geometry::LabelMm::parse("50x30")
            .unwrap()
            .to_pixels(384, 8.0);
        crate::label::make_qr_label_opts(&crate::label::QrLabelOptions {
            url: "https://example.com/trial/1".into(),
            side_text: "TRIAL 1".into(),
            label,
            safe: crate::geometry::SafeArea::B1,
            text_side: crate::label::TextSide::Right,
            border: false,
            font_path: None,
            font_name: None,
            font_size: None,
        })
        .unwrap();
    }
}
