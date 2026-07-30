//! Rasterize images into NIIMBOT 1-bit row packets.

use crate::errors::{Error, Result};
use crate::geometry::{LabelPx, Rect, SafeArea};
use crate::packet::Packet;
use crate::protocol;
use crate::types::Rotation;
use image::{DynamicImage, GenericImageView, GrayImage, Luma, RgbaImage, imageops};

/// An encoded page: row packets plus the dimensions they were built from.
///
/// Bundling the three keeps them from drifting apart — the printer needs the
/// size in `SetPageSize` to agree with the rows it then receives, and passing
/// them as three loose arguments made disagreement easy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub rows: Vec<Packet>,
}

impl Raster {
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Apply a [`Rotation`] to an image.
pub fn rotate(img: DynamicImage, rotation: Rotation) -> DynamicImage {
    match rotation {
        Rotation::Deg0 => img,
        Rotation::Deg90 => img.rotate90(),
        Rotation::Deg180 => img.rotate180(),
        Rotation::Deg270 => img.rotate270(),
    }
}

/// Load, threshold to 1-bit, and emit print row packets.
///
/// Pixel convention: **1 = black (burn)**, **0 = white** after invert+threshold,
/// matching the simple print-task form (invert grayscale then convert to 1-bit).
pub fn encode_path(
    path: &std::path::Path,
    max_width: u32,
    threshold: u8,
    dither: bool,
) -> Result<Raster> {
    let img = image::open(path).map_err(Error::from)?;
    encode(img, max_width, threshold, dither)
}

/// Threshold an image to 1-bit and emit print row packets.
///
/// Rotate beforehand with [`rotate`] if needed.
pub fn encode(img: DynamicImage, max_width: u32, threshold: u8, dither: bool) -> Result<Raster> {
    let gray = img.to_luma8();
    let (width, height) = gray.dimensions();
    if width > max_width {
        return Err(Error::ImageTooWide {
            width,
            max: max_width,
        });
    }

    let bw = gray_to_print_bits(&gray, threshold, dither);
    Ok(Raster {
        width,
        height,
        rows: rows_to_packets(&bw),
    })
}

/// Convert a grayscale image to print bits (255 = burn / black).
///
/// Source dark pixels print. Hard threshold is fine for QR/text; **dither** is
/// better for photographs (avoids big blotchy black “bleed” regions).
pub fn gray_to_print_bits(gray: &GrayImage, threshold: u8, dither: bool) -> GrayImage {
    let (w, h) = gray.dimensions();
    if !dither {
        let mut bw = GrayImage::new(w, h);
        for (x, y, p) in gray.enumerate_pixels() {
            let inv = 255u8.saturating_sub(p[0]);
            let bit = if inv > threshold { 255 } else { 0 };
            bw.put_pixel(x, y, Luma([bit]));
        }
        return bw;
    }

    // Floyd–Steinberg on inverted luminance (dark source → high print energy).
    let mut err = vec![0.0f32; (w * h) as usize];
    for (i, p) in gray.pixels().enumerate() {
        err[i] = f32::from(255u8.saturating_sub(p[0]));
    }
    let mut bw = GrayImage::new(w, h);
    let thr = f32::from(threshold);
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let old = err[i];
            let new = if old > thr { 255.0 } else { 0.0 };
            let e = old - new;
            bw.put_pixel(x, y, Luma([new as u8]));
            // Standard FS coefficients
            if x + 1 < w {
                err[i + 1] += e * (7.0 / 16.0);
            }
            if y + 1 < h {
                let row = i + w as usize;
                if x > 0 {
                    err[row - 1] += e * (3.0 / 16.0);
                }
                err[row] += e * (5.0 / 16.0);
                if x + 1 < w {
                    err[row + 1] += e * (1.0 / 16.0);
                }
            }
        }
    }
    bw
}

fn rows_to_packets(bw: &GrayImage) -> Vec<Packet> {
    let (w, h) = bw.dimensions();
    let bytes_per_row = (w as usize).div_ceil(8);
    let mut out = Vec::with_capacity(h as usize);

    for y in 0..h {
        let mut row = vec![0u8; bytes_per_row];
        let mut all_white = true;
        for x in 0..w {
            // 255 = black to print
            if bw.get_pixel(x, y)[0] > 127 {
                all_white = false;
                let byte_i = (x / 8) as usize;
                let bit = 7 - (x % 8);
                row[byte_i] |= 1 << bit;
            }
        }

        if all_white {
            out.push(protocol::print_empty_row(y as u16, 1));
        } else {
            out.push(protocol::print_bitmap_row(y as u16, 1, &row));
        }
    }
    out
}

/// Crop uniform white space from the edges of an image.
///
/// Artwork usually carries its own margin. Placing it on a label without
/// trimming means that margin is *added* to the label's own unprintable
/// inset, so the drawing ends up far smaller than the media allows — a
/// bulldozer with a 35 px built-in margin lost another 29 rows to it after
/// scaling, on top of the 40 reserved rows.
///
/// Returns the image unchanged when it is blank or already tight.
pub fn trim_white(img: DynamicImage, threshold: u8) -> DynamicImage {
    let gray = img.to_luma8();
    let (w, h) = gray.dimensions();
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for (x, y, p) in gray.enumerate_pixels() {
        if p[0] <= threshold {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if x0 == u32::MAX {
        return img; // nothing but background
    }
    if x0 == 0 && y0 == 0 && x1 == w - 1 && y1 == h - 1 {
        return img; // already tight
    }
    DynamicImage::ImageRgba8(
        imageops::crop_imm(&img.to_rgba8(), x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image(),
    )
}

/// Resize preserving aspect to fit within max width (height free).
pub fn fit_width(img: DynamicImage, max_width: u32) -> DynamicImage {
    let (w, h) = img.dimensions();
    if w <= max_width {
        return img;
    }
    let new_h = ((h as f64) * (max_width as f64) / (w as f64)).round() as u32;
    DynamicImage::ImageRgba8(imageops::resize(
        &img,
        max_width,
        new_h.max(1),
        imageops::FilterType::Triangle,
    ))
}

/// The drawable area of a label once the margin is inset.
///
/// The requested margin is capped at a quarter of each axis so a large
/// `--margin` cannot collapse the content box to nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ContentBox {
    canvas_w: u32,
    canvas_h: u32,
    /// Top-left of the content box on the full canvas.
    origin_x: u32,
    origin_y: u32,
    margin: u32,
    width: u32,
    height: u32,
}

impl ContentBox {
    /// Content box for `area` within a full `label` canvas.
    fn in_rect(label: LabelPx, area: Rect, margin: u32) -> Self {
        let canvas_w = label.width_px.max(1);
        let canvas_h = label.height_px.max(1);
        let aw = area.w.max(1);
        let ah = area.h.max(1);
        let margin = margin.min(aw / 4).min(ah / 4);
        Self {
            canvas_w,
            canvas_h,
            origin_x: area.x + margin,
            origin_y: area.y + margin,
            margin,
            width: aw.saturating_sub(margin * 2).max(1),
            height: ah.saturating_sub(margin * 2).max(1),
        }
    }

    fn white_canvas(&self) -> RgbaImage {
        RgbaImage::from_pixel(
            self.canvas_w,
            self.canvas_h,
            image::Rgba([255, 255, 255, 255]),
        )
    }
}

/// Scale `img` by `scale`, rounding up to at least 1px on each axis.
fn scaled_dimensions(img: &DynamicImage, scale: f64) -> (u32, u32) {
    let (iw, ih) = img.dimensions();
    (
        ((iw as f64) * scale).round().max(1.0) as u32,
        ((ih as f64) * scale).round().max(1.0) as u32,
    )
}

/// Cover-fit `img` into the printable area of `label`, cropping overflow.
///
/// Makes content as large as the media allows; `margin` keeps a white border
/// so heat is less likely to run to the edge. Pass [`SafeArea::NONE`] for full
/// bleed. Returns a full-size canvas with the image placed inside `safe`.
pub fn fill_label(img: DynamicImage, label: LabelPx, safe: SafeArea, margin: u32) -> DynamicImage {
    let area = safe.content(label).unwrap_or(Rect {
        x: 0,
        y: 0,
        w: label.width_px,
        h: label.height_px,
    });
    let bx = ContentBox::in_rect(label, area, margin);
    let (iw, ih) = img.dimensions();
    // Scale so both dimensions cover the content box (may overflow one axis).
    let scale = f64::max(bx.width as f64 / iw as f64, bx.height as f64 / ih as f64);
    let (nw, nh) = scaled_dimensions(&img, scale);

    let resized = imageops::resize(&img, nw, nh, imageops::FilterType::CatmullRom);
    let cropped = imageops::crop_imm(
        &resized,
        nw.saturating_sub(bx.width) / 2,
        nh.saturating_sub(bx.height) / 2,
        bx.width,
        bx.height,
    )
    .to_image();

    let mut canvas = bx.white_canvas();
    imageops::overlay(
        &mut canvas,
        &cropped,
        bx.origin_x as i64,
        bx.origin_y as i64,
    );
    DynamicImage::ImageRgba8(canvas)
}

/// Scale `img` to **fit entirely** inside the printable area, centred there.
///
/// Prefer this for photographs so nothing is cropped. Pass [`SafeArea::NONE`]
/// to use the whole canvas.
pub fn contain_label(
    img: DynamicImage,
    label: LabelPx,
    safe: SafeArea,
    margin: u32,
) -> DynamicImage {
    let area = safe.content(label).unwrap_or(Rect {
        x: 0,
        y: 0,
        w: label.width_px,
        h: label.height_px,
    });
    let bx = ContentBox::in_rect(label, area, margin);
    let (iw, ih) = img.dimensions();
    let scale = f64::min(bx.width as f64 / iw as f64, bx.height as f64 / ih as f64);
    let (nw, nh) = scaled_dimensions(&img, scale);

    let resized = imageops::resize(&img, nw, nh, imageops::FilterType::CatmullRom);
    let mut canvas = bx.white_canvas();
    // Centre within the content box, not the raw canvas — centring on the
    // canvas pushes content into the band the printer cannot reach.
    imageops::overlay(
        &mut canvas,
        &resized,
        (bx.origin_x + bx.width.saturating_sub(nw) / 2) as i64,
        (bx.origin_y + bx.height.saturating_sub(nh) / 2) as i64,
    );
    DynamicImage::ImageRgba8(canvas)
}

/// Spacing between calibration rings, in px (0.5 mm at 8 px/mm).
pub const CALIBRATION_RING_STEP_PX: u32 = 4;
/// How many rings the calibration pattern draws.
pub const CALIBRATION_RINGS: u32 = 6;
/// Length of a major (5 mm) feed-ruler tick, in px. Numerals are placed clear
/// of this — see [`crate::label::make_calibration_label`].
pub const CALIBRATION_RULER_MAJOR_PX: u32 = 26;
/// Length of a minor (1 mm) feed-ruler tick, in px.
pub const CALIBRATION_RULER_MINOR_PX: u32 = 12;

/// Calibration pattern: concentric rings at known insets, plus diagonals and a
/// centre cross.
///
/// Ring *k* (counting inward from 0) sits `k * CALIBRATION_RING_STEP_PX` from
/// the edge. Print it, count how many rings came out **complete on all four
/// sides**, and the first complete ring's inset is the safe margin for that
/// media. A single border only tells you *that* something clipped; the rings
/// tell you *how much*.
/// Additionally outlines `safe` as a thick rectangle.
///
/// The thick box is the pass/fail test: if it prints complete on all four
/// sides, the configured [`SafeArea`] is inside the real printable region and
/// labels will not clip. The thin rings around it measure how much headroom
/// (or shortfall) there is.
pub fn calibration_pattern(label: LabelPx, safe: Option<SafeArea>) -> GrayImage {
    let w = label.width_px;
    let h = label.height_px;
    let mut img = GrayImage::from_pixel(w, h, Luma([255]));
    if w == 0 || h == 0 {
        return img;
    }

    // Diagonals + centre cross: reveal skew and vertical centring.
    for y in 0..h {
        for x in 0..w {
            let expect_down = (y as i64 * (w as i64 - 1)) / (h as i64 - 1).max(1);
            let expect_up = ((h as i64 - 1 - y as i64) * (w as i64 - 1)) / (h as i64 - 1).max(1);
            let on_diag = (x as i64 - expect_down).abs() <= 1 || (x as i64 - expect_up).abs() <= 1;
            let on_cross =
                (x as i64 - w as i64 / 2).abs() <= 1 || (y as i64 - h as i64 / 2).abs() <= 1;
            if on_diag || on_cross {
                img.put_pixel(x, y, Luma([0]));
            }
        }
    }

    // Concentric rings, 1px each so a clipped ring is unambiguous.
    for ring in 0..CALIBRATION_RINGS {
        let inset = ring * CALIBRATION_RING_STEP_PX;
        if inset * 2 + 1 >= w.min(h) {
            break;
        }
        let (x0, y0) = (inset, inset);
        let (x1, y1) = (w - 1 - inset, h - 1 - inset);
        for x in x0..=x1 {
            img.put_pixel(x, y0, Luma([0]));
            img.put_pixel(x, y1, Luma([0]));
        }
        for y in y0..=y1 {
            img.put_pixel(x0, y, Luma([0]));
            img.put_pixel(x1, y, Luma([0]));
        }
    }

    // Feed ruler down both sides: a minor tick every 1 mm, a long major tick
    // every 5 mm. Read off where the print stops to get the exact loss at the
    // feed edge — the rings only resolve 0.5 mm near the very edge.
    let px_per_mm = crate::geometry::PX_PER_MM as u32;
    for mm in 0..=(h / px_per_mm) {
        let y = mm * px_per_mm;
        if y >= h {
            break;
        }
        let major = mm % 5 == 0;
        let len = if major {
            CALIBRATION_RULER_MAJOR_PX
        } else {
            CALIBRATION_RULER_MINOR_PX
        };
        let thick = if major { 3 } else { 1 };
        for t in 0..thick {
            let yy = (y + t).min(h - 1);
            for x in 0..len.min(w) {
                img.put_pixel(x, yy, Luma([0]));
                img.put_pixel(w - 1 - x, yy, Luma([0]));
            }
        }
    }

    // The safe-area box, drawn thick so it is unmistakable next to the rings.
    if let Some(area) = safe.and_then(|s| s.content(label)) {
        let t = 3i64;
        let (x0, y0) = (area.x as i64, area.y as i64);
        let (x1, y1) = (x0 + area.w as i64 - 1, y0 + area.h as i64 - 1);
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                let inside = x >= x0 && x <= x1 && y >= y0 && y <= y1;
                let near_edge = (x - x0).abs() < t
                    || (x - x1).abs() < t
                    || (y - y0).abs() < t
                    || (y - y1).abs() < t;
                if inside && near_edge {
                    img.put_pixel(x as u32, y as u32, Luma([0]));
                }
            }
        }
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::LabelMm;
    use crate::protocol::Cmd;

    #[test]
    fn encode_respects_max_width() {
        let img = DynamicImage::ImageLuma8(GrayImage::from_pixel(100, 50, Luma([0])));
        let r = encode(img, 384, 127, false).unwrap();
        let (w, h, pkts) = (r.width, r.height, r.rows);
        assert_eq!((w, h), (100, 50));
        assert_eq!(pkts.len(), 50);
    }

    #[test]
    fn encode_rejects_too_wide() {
        let img = DynamicImage::ImageLuma8(GrayImage::from_pixel(400, 10, Luma([0])));
        assert!(encode(img, 384, 127, false).is_err());
    }

    #[test]
    fn fill_label_exact_size() {
        let src = DynamicImage::ImageLuma8(GrayImage::from_pixel(50, 50, Luma([0])));
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384);
        let out = fill_label(src, lp, SafeArea::NONE, 0);
        assert_eq!(out.dimensions(), (lp.width_px, lp.height_px));
    }

    #[test]
    fn contain_label_centers_with_white_margins() {
        // Tall image → letterbox left/right on a wide label.
        let src = DynamicImage::ImageLuma8(GrayImage::from_pixel(40, 80, Luma([0])));
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384);
        let out = contain_label(src, lp, SafeArea::NONE, 0).to_luma8();
        assert_eq!(out.dimensions(), (lp.width_px, lp.height_px));
        // Corners of canvas should stay white (letterbox / padding).
        assert_eq!(out.get_pixel(0, 0)[0], 255);
        assert_eq!(out.get_pixel(lp.width_px - 1, 0)[0], 255);
        // Center should have content (black source → still black in gray canvas).
        let cx = lp.width_px / 2;
        let cy = lp.height_px / 2;
        assert_eq!(out.get_pixel(cx, cy)[0], 0);
    }

    #[test]
    fn contain_label_respects_margin() {
        let src = DynamicImage::ImageLuma8(GrayImage::from_pixel(200, 200, Luma([0])));
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384);
        let margin = 16u32;
        let out = contain_label(src, lp, SafeArea::NONE, margin).to_luma8();
        // Outer margin ring must be white.
        for x in 0..lp.width_px {
            assert_eq!(out.get_pixel(x, 0)[0], 255);
            assert_eq!(out.get_pixel(x, margin - 1)[0], 255);
        }
    }

    #[test]
    fn raw_images_are_kept_out_of_the_unprintable_band() {
        // The bug this pins: `thermark print` scaled images across the whole
        // canvas, so the bottom rows landed in the band the printer never
        // reaches and were silently lost.
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384);
        let safe = SafeArea::B1;
        let src = DynamicImage::ImageLuma8(GrayImage::from_pixel(100, 100, Luma([0])));

        for placed in [
            fill_label(src.clone(), lp, safe, 0),
            contain_label(src.clone(), lp, safe, 0),
        ] {
            let g = placed.to_luma8();
            assert_eq!(g.dimensions(), (lp.width_px, lp.height_px));
            for y in (lp.height_px - safe.bottom)..lp.height_px {
                for x in 0..lp.width_px {
                    assert_eq!(g.get_pixel(x, y)[0], 255, "ink at ({x},{y}) is unprintable");
                }
            }
            for y in 0..safe.top {
                for x in 0..lp.width_px {
                    assert_eq!(g.get_pixel(x, y)[0], 255, "ink at ({x},{y}) is unprintable");
                }
            }
        }
    }

    #[test]
    fn trim_removes_the_artwork_s_own_margin() {
        // 100x100 canvas with a 20x20 mark at (40,40): 40px of margin all round.
        let mut g = GrayImage::from_pixel(100, 100, Luma([255]));
        for y in 40..60 {
            for x in 40..60 {
                g.put_pixel(x, y, Luma([0]));
            }
        }
        let out = trim_white(DynamicImage::ImageLuma8(g), 127);
        assert_eq!(out.dimensions(), (20, 20));
    }

    #[test]
    fn trim_leaves_blank_and_already_tight_images_alone() {
        let blank = DynamicImage::ImageLuma8(GrayImage::from_pixel(40, 20, Luma([255])));
        assert_eq!(trim_white(blank, 127).dimensions(), (40, 20));
        let full = DynamicImage::ImageLuma8(GrayImage::from_pixel(40, 20, Luma([0])));
        assert_eq!(trim_white(full, 127).dimensions(), (40, 20));
    }

    #[test]
    fn trimmed_art_fills_the_printable_band() {
        // The bug this pins: the artwork's own margin was *added* to the
        // label's inset, so the drawing came out far smaller than the media.
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384);
        let safe = SafeArea::B1;
        let mut g = GrayImage::from_pixel(384, 240, Luma([255]));
        for y in 60..180 {
            for x in 90..300 {
                g.put_pixel(x, y, Luma([0]));
            }
        }
        let art = trim_white(DynamicImage::ImageLuma8(g), 127);
        let placed = contain_label(art, lp, safe, 0).to_luma8();

        let usable = lp.height_px - safe.bottom;
        let mut lowest = 0;
        for (_, y, p) in placed.enumerate_pixels() {
            if p[0] < 128 {
                lowest = lowest.max(y);
            }
        }
        assert!(lowest < usable, "ink at {lowest} is unprintable");
        assert!(
            lowest + 8 >= usable,
            "only reached row {lowest} of a {usable}-row band — not filling it"
        );
    }

    #[test]
    fn safe_area_none_still_fills_the_whole_canvas() {
        // Calibration depends on this: it must reach the true edges.
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384);
        let src = DynamicImage::ImageLuma8(GrayImage::from_pixel(100, 100, Luma([0])));
        let g = fill_label(src, lp, SafeArea::NONE, 0).to_luma8();
        assert_eq!(g.get_pixel(0, 0)[0], 0);
        assert_eq!(g.get_pixel(lp.width_px - 1, lp.height_px - 1)[0], 0);
    }

    #[test]
    fn dither_produces_mixed_dots_on_gray() {
        let g = GrayImage::from_pixel(32, 32, Luma([128]));
        let hard = gray_to_print_bits(&g, 127, false);
        let dit = gray_to_print_bits(&g, 127, true);
        let hard_black = hard.pixels().filter(|p| p[0] > 127).count();
        let dit_black = dit.pixels().filter(|p| p[0] > 127).count();
        // Mid-gray hard threshold: all black (inv 127 is not > 127 → all white actually)
        // inv(128)=127, 127 > 127 is false → all white for hard.
        assert_eq!(hard_black, 0);
        // Dither should scatter some black dots for mid-gray.
        assert!(dit_black > 50, "dither black count {dit_black}");
        assert!(dit_black < 32 * 32 - 50, "dither not solid");
    }

    #[test]
    fn dark_source_pixels_become_bitmap_rows() {
        // Dark source (0) inverts to 255 > threshold, so it burns → PrintBitmapRow.
        let dark = DynamicImage::ImageLuma8(GrayImage::from_pixel(16, 2, Luma([0])));
        let rows = encode(dark, 384, 127, false).unwrap().rows;
        assert!(rows.iter().all(|p| p.cmd == Cmd::PrintBitmapRow as u8));
    }

    #[test]
    fn white_source_pixels_become_empty_rows() {
        // The complement: white source burns nothing, so rows are sent as empty.
        let light = DynamicImage::ImageLuma8(GrayImage::from_pixel(16, 2, Luma([255])));
        let rows = encode(light, 384, 127, false).unwrap().rows;
        assert!(rows.iter().all(|p| p.cmd == Cmd::PrintEmptyRow as u8));
    }
}
