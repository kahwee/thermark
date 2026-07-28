//! Text rendering for labels: system TrueType fonts (preferred) + tiny bitmap fallback.

use crate::errors::{Error, Result};
use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use image::{GrayImage, Luma};
use tracing::{debug, info};
use std::path::{Path, PathBuf};

/// Glyph metrics for the built-in 5×7 fallback (used only if no TTF loads).
pub const GLYPH_W: u32 = 5;
pub const GLYPH_H: u32 = 7;
pub const GLYPH_ADVANCE: u32 = 6;

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
        let data = std::fs::read(path).map_err(|e| {
            Error::font(format!("read font {}: {e}", path.display()))
        })?;
        FontRef::try_from_slice_and_index(&data, index).map_err(|e| {
            Error::font(format!("parse font {} (index {index}): {e:?}", path.display()))
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
                (
                    "/System/Library/Fonts/Supplemental/Times New Roman.ttf",
                    0,
                ),
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
                (
                    "/System/Library/Fonts/Supplemental/Courier New Bold.ttf",
                    0,
                ),
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
        (sf.ascent() - sf.descent() + sf.line_gap())
            .ceil()
            .max(1.0) as u32
    }

    /// Draw black text (Luma 0) onto a white-ish label image. Origin = baseline-left.
    pub fn draw_text(&self, img: &mut GrayImage, x: f32, baseline_y: f32, text: &str, px_height: f32) {
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
    pub fn fit_size(&self, text: &str, max_w: u32, max_h: u32) -> f32 {
        for size in (10..=72).rev() {
            let ph = size as f32;
            let lines = self.wrap(text, max_w, ph);
            let line_h = self.text_height(ph) + 2;
            let need = lines.len() as u32 * line_h;
            let max_line = lines
                .iter()
                .map(|l| self.text_width(l, ph))
                .max()
                .unwrap_or(0);
            if need <= max_h && max_line <= max_w {
                return ph;
            }
        }
        12.0
    }
}

// ─── Bitmap fallback (kept for tests / no-font environments) ────────────────

pub fn glyph(ch: char) -> [u8; 7] {
    // bit4 (0x10) = LEFTMOST pixel (standard 5×7). Draw with col from 4 down to 0.
    match ch {
        ' ' => [0; 7],
        'A' | 'a' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' | 'b' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' | 'c' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'H' | 'h' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'E' | 'e' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'L' | 'l' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'O' | 'o' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'Y' | 'y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'U' | 'u' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'T' | 't' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        _ => [0x1F, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x1F],
    }
}

/// Draw bitmap text. bit4 = left (correct orientation).
pub fn draw_text_bitmap(img: &mut GrayImage, x: i32, y: i32, text: &str, scale: u32, black: bool) {
    let scale = scale.max(1);
    let color = if black { Luma([0u8]) } else { Luma([255u8]) };
    let (w, h) = img.dimensions();
    let mut cx = x;
    for ch in text.chars() {
        let g = glyph(ch);
        for (row, bits) in g.iter().enumerate() {
            // bit 4 = leftmost column of the 5-wide glyph
            for col in 0..5u32 {
                let bit = 4 - col; // col 0 → bit4, col 4 → bit0
                if bits & (1 << bit) != 0 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let px = cx + (col * scale) as i32 + dx as i32;
                            let py = y + (row as u32 * scale) as i32 + dy as i32;
                            if px >= 0 && py >= 0 && (px as u32) < w && (py as u32) < h {
                                img.put_pixel(px as u32, py as u32, color);
                            }
                        }
                    }
                }
            }
        }
        cx += (GLYPH_ADVANCE * scale) as i32;
    }
}

// Back-compat wrappers used by older code paths
pub fn draw_text(img: &mut GrayImage, x: i32, y: i32, text: &str, scale: u32, black: bool) {
    draw_text_bitmap(img, x, y, text, scale, black);
}

pub fn chars_fit(width_px: u32, scale: u32) -> u32 {
    let adv = GLYPH_ADVANCE * scale.max(1);
    width_px.checked_div(adv).unwrap_or(0)
}

pub fn text_width(text: &str, scale: u32) -> u32 {
    text.chars().count() as u32 * GLYPH_ADVANCE * scale.max(1)
}

pub fn text_height(scale: u32) -> u32 {
    GLYPH_H * scale.max(1)
}

pub fn wrap_text(text: &str, max_width_px: u32, scale: u32) -> Vec<String> {
    let max_chars = chars_fit(max_width_px, scale).max(1) as usize;
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
            if trial.chars().count() <= max_chars {
                current = trial;
            } else {
                if !current.is_empty() {
                    lines.push(current);
                }
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    lines
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
    fn abc_drawn_left_to_right() {
        // Bitmap path: ink should span left→right for "ABC"
        let mut img = GrayImage::from_pixel(80, 20, Luma([255]));
        draw_text_bitmap(&mut img, 2, 2, "ABC", 2, true);
        let mut min_x = 999u32;
        let mut max_x = 0u32;
        let mut count = 0u32;
        for (x, _y, p) in img.enumerate_pixels() {
            if p[0] < 128 {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                count += 1;
            }
        }
        assert!(count > 10, "expected ink for ABC");
        assert!(max_x > min_x + 20, "ABC should span left→right, got {min_x}..{max_x}");
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
        let left = img.enumerate_pixels().filter(|(x, _, p)| *x < mid1 && p[0] < 128).count();
        let right = img.enumerate_pixels().filter(|(x, _, p)| *x > mid2 && p[0] < 128).count();
        assert!(left > 5 && right > 5, "A and C regions should both have ink");
    }
}
