//! Text rendering for labels using system TrueType fonts.

use crate::errors::{Error, Result};
use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use image::{GrayImage, Luma};
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Smallest and largest text size [`LabelFont::fit_size`] will choose.
pub const MIN_FONT_PX: f32 = 10.0;
pub const MAX_FONT_PX: f32 = 72.0;

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
    if let Some(ref p) = prefer {
        if let Some(hit) = cands.iter().find(|path| {
            path.exists()
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.to_ascii_lowercase().contains(p.as_str()))
                    .unwrap_or(false)
        }) {
            return Some(hit.clone());
        }
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
pub struct LabelFont {
    data: Vec<u8>,
    path: PathBuf,
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
        Ok(Self {
            data,
            path: path.to_path_buf(),
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

    pub fn load_default() -> Result<Self> {
        Self::load_named("arial bold").or_else(|_| {
            let path = find_system_font(None).ok_or_else(|| {
                Error::font(
                    "no system font found — pass --font /path/to.ttf or --font-name helvetica",
                )
            })?;
            info!(path = %path.display(), "using font");
            Self::load(&path)
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn index(&self) -> u32 {
        self.index
    }

    fn font(&self) -> FontRef<'_> {
        FontRef::try_from_slice_and_index(&self.data, self.index).expect("font validated at load")
    }

    /// Pixel width of `text` at `px_height` (approx em size).
    pub fn text_width(&self, text: &str, px_height: f32) -> u32 {
        let font = self.font();
        let scale = PxScale::from(px_height);
        let sf = font.as_scaled(scale);
        let mut w = 0.0f32;
        for ch in text.chars() {
            let id = font.glyph_id(ch);
            w += sf.h_advance(id);
        }
        w.ceil().max(0.0) as u32
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
        let scale = PxScale::from(px_height);
        let sf = font.as_scaled(scale);
        let (w, h) = img.dimensions();

        let mut caret = x;
        for ch in text.chars() {
            let gid = font.glyph_id(ch);
            let glyph = gid.with_scale_and_position(scale, ab_glyph::point(caret, baseline_y));
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
            caret += sf.h_advance(gid);
        }
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
                        for ch in word.chars() {
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

    #[test]
    fn system_font_loads_on_macos() {
        let f = LabelFont::load_default();
        if cfg!(target_os = "macos") {
            let f = f.expect("macOS should have Arial");
            assert!(f.path().exists());
            assert!(f.text_width("ABC", 24.0) > 10);
        }
    }

    #[test]
    fn fit_size_never_returns_a_size_larger_than_the_smallest_tried() {
        let Ok(f) = LabelFont::load_default() else {
            return;
        };
        // A box too small for any size: the old fallback returned 12.0 — larger
        // than the 10px that had already failed — so text overflowed further.
        let px = f.fit_size("SOME LONGISH LABEL TEXT HERE", 20, 8);
        assert_eq!(px, MIN_FONT_PX);
    }

    #[test]
    fn fit_size_picks_the_largest_size_that_fits() {
        let Ok(f) = LabelFont::load_default() else {
            return;
        };
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
        let Ok(f) = LabelFont::load_default() else {
            return;
        };
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
        let Ok(f) = LabelFont::load_default() else {
            return;
        };
        // A single token far wider than the column: splitting is the only
        // option, so the whole-word preference must not deadlock.
        let px = f.fit_size("SUPERCALIFRAGILISTIC", 40, 200);
        assert!(px >= MIN_FONT_PX);
    }

    #[test]
    fn arial_abc_left_to_right() {
        let Ok(font) = LabelFont::load_default() else {
            return;
        };
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
