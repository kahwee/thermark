//! Rasterize images into NIIMBOT 1-bit row packets.

use crate::errors::{Error, Result};
use crate::geometry::LabelPx;
use crate::packet::Packet;
use crate::protocol;
use image::{DynamicImage, GenericImageView, GrayImage, Luma, RgbaImage, imageops};

/// Load, optionally rotate, threshold to 1-bit, and emit print row packets.
///
/// Pixel convention: **1 = black (burn)**, **0 = white** after invert+threshold,
/// matching the simple print-task form (invert grayscale then convert to 1-bit).
pub fn encode_image_path(
    path: &std::path::Path,
    max_width: u32,
    rotate_deg: u32,
    threshold: u8,
) -> Result<(u32, u32, Vec<Packet>)> {
    encode_image_path_opts(path, max_width, rotate_deg, threshold, false)
}

/// Like [`encode_image_path`] with optional Floyd–Steinberg dither (photos).
pub fn encode_image_path_opts(
    path: &std::path::Path,
    max_width: u32,
    rotate_deg: u32,
    threshold: u8,
    dither: bool,
) -> Result<(u32, u32, Vec<Packet>)> {
    let img = image::open(path).map_err(Error::from)?;
    encode_image_opts(img, max_width, rotate_deg, threshold, dither)
}

pub fn encode_image(
    img: DynamicImage,
    max_width: u32,
    rotate_deg: u32,
    threshold: u8,
) -> Result<(u32, u32, Vec<Packet>)> {
    encode_image_opts(img, max_width, rotate_deg, threshold, false)
}

pub fn encode_image_opts(
    img: DynamicImage,
    max_width: u32,
    rotate_deg: u32,
    threshold: u8,
    dither: bool,
) -> Result<(u32, u32, Vec<Packet>)> {
    let img = match rotate_deg % 360 {
        0 => img,
        90 => img.rotate90(),
        180 => img.rotate180(),
        270 => img.rotate270(),
        other => return Err(Error::InvalidRotation(other)),
    };

    let gray = img.to_luma8();
    let (w, h) = gray.dimensions();
    if w > max_width {
        return Err(Error::ImageTooWide {
            width: w,
            max: max_width,
        });
    }

    let bw = gray_to_print_bits(&gray, threshold, dither);
    let packets = rows_to_packets(&bw)?;
    Ok((w, h, packets))
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

fn rows_to_packets(bw: &GrayImage) -> Result<Vec<Packet>> {
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
    Ok(out)
}

/// Create a simple solid-text-like test pattern (black rectangle + border).
#[allow(dead_code)]
pub fn test_pattern(width: u32, height: u32) -> GrayImage {
    let mut img = GrayImage::from_pixel(width, height, Luma([0]));
    // Border
    for x in 0..width {
        img.put_pixel(x, 0, Luma([255]));
        img.put_pixel(x, height - 1, Luma([255]));
    }
    for y in 0..height {
        img.put_pixel(0, y, Luma([255]));
        img.put_pixel(width - 1, y, Luma([255]));
    }
    // Diagonal
    let steps = width.min(height);
    for i in 0..steps {
        img.put_pixel(i, i, Luma([255]));
    }
    // Filled block in center
    let x0 = width / 4;
    let y0 = height / 4;
    let x1 = width * 3 / 4;
    let y1 = height * 3 / 4;
    for y in y0..y1 {
        for x in x0..x1 {
            img.put_pixel(x, y, Luma([255]));
        }
    }
    img
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

/// Scale image to fill the label canvas (cover), then center-crop to exact size.
///
/// This makes content as large as possible on the physical label. Optional
/// `margin` keeps a white border so heat/ink is less likely to run to the edge.
pub fn fill_label(img: DynamicImage, label: LabelPx) -> DynamicImage {
    fill_label_with_margin(img, label, 0)
}

/// Like [`fill_label`] with a white inset margin (pixels on each side).
pub fn fill_label_with_margin(img: DynamicImage, label: LabelPx, margin: u32) -> DynamicImage {
    let tw = label.width_px.max(1);
    let th = label.height_px.max(1);
    let m = margin.min(tw / 4).min(th / 4);
    let cw = tw.saturating_sub(m * 2).max(1);
    let ch = th.saturating_sub(m * 2).max(1);

    let (iw, ih) = img.dimensions();
    // Scale so both dimensions cover the content box (may overflow one axis).
    let scale = f64::max(cw as f64 / iw as f64, ch as f64 / ih as f64);
    let nw = ((iw as f64) * scale).round().max(1.0) as u32;
    let nh = ((ih as f64) * scale).round().max(1.0) as u32;

    let resized = imageops::resize(&img, nw, nh, imageops::FilterType::CatmullRom);
    let x0 = nw.saturating_sub(cw) / 2;
    let y0 = nh.saturating_sub(ch) / 2;
    let cropped = imageops::crop_imm(&resized, x0, y0, cw, ch).to_image();

    let mut canvas = RgbaImage::from_pixel(tw, th, image::Rgba([255, 255, 255, 255]));
    imageops::overlay(&mut canvas, &cropped, m as i64, m as i64);
    DynamicImage::ImageRgba8(canvas)
}

/// Scale image to **fit entirely** inside the label (contain), centered on white.
///
/// Prefer this for photographs so nothing is cropped and content sits in the
/// middle of the label with clean margins.
pub fn contain_label(img: DynamicImage, label: LabelPx, margin: u32) -> DynamicImage {
    let tw = label.width_px.max(1);
    let th = label.height_px.max(1);
    let m = margin.min(tw / 4).min(th / 4);
    let cw = tw.saturating_sub(m * 2).max(1);
    let ch = th.saturating_sub(m * 2).max(1);

    let (iw, ih) = img.dimensions();
    let scale = f64::min(cw as f64 / iw as f64, ch as f64 / ih as f64);
    let nw = ((iw as f64) * scale).round().max(1.0) as u32;
    let nh = ((ih as f64) * scale).round().max(1.0) as u32;

    let resized = imageops::resize(&img, nw, nh, imageops::FilterType::CatmullRom);
    let mut canvas = RgbaImage::from_pixel(tw, th, image::Rgba([255, 255, 255, 255]));
    // Center within the full canvas (margin is already baked into max size).
    let x0 = tw.saturating_sub(nw) / 2;
    let y0 = th.saturating_sub(nh) / 2;
    imageops::overlay(&mut canvas, &resized, x0 as i64, y0 as i64);
    DynamicImage::ImageRgba8(canvas)
}

/// Pad (or center) an image onto a full label canvas with white background.
/// Does **not** upscale — use [`fill_label`] / [`contain_label`] to scale.
pub fn pad_to_label(img: DynamicImage, label: LabelPx) -> DynamicImage {
    let (iw, ih) = img.dimensions();
    let tw = label.width_px;
    let th = label.height_px;
    let mut canvas = RgbaImage::from_pixel(tw, th, image::Rgba([255, 255, 255, 255]));
    let x0 = tw.saturating_sub(iw) / 2;
    let y0 = th.saturating_sub(ih) / 2;
    imageops::overlay(&mut canvas, &img.to_rgba8(), x0 as i64, y0 as i64);
    DynamicImage::ImageRgba8(canvas)
}

/// Full-bleed calibration pattern: thick border + diagonals + center cross.
/// Fills the entire label so you can see true print area.
pub fn calibration_pattern(label: LabelPx) -> GrayImage {
    let w = label.width_px;
    let h = label.height_px;
    let mut img = GrayImage::from_pixel(w, h, Luma([255]));
    let t = 6u32; // border thickness px

    for y in 0..h {
        for x in 0..w {
            let on_border = x < t || y < t || x >= w.saturating_sub(t) || y >= h.saturating_sub(t);
            let on_diag1 = {
                let expect = (y as i64 * (w as i64 - 1)) / (h as i64 - 1).max(1);
                (x as i64 - expect).abs() <= 2
            };
            let on_diag2 = {
                let expect = ((h as i64 - 1 - y as i64) * (w as i64 - 1)) / (h as i64 - 1).max(1);
                (x as i64 - expect).abs() <= 2
            };
            let on_cross =
                (x as i64 - w as i64 / 2).abs() <= 2 || (y as i64 - h as i64 / 2).abs() <= 2;
            if on_border || on_diag1 || on_diag2 || on_cross {
                img.put_pixel(x, y, Luma([0]));
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
        let (w, h, pkts) = encode_image(img, 384, 0, 127).unwrap();
        assert_eq!((w, h), (100, 50));
        assert_eq!(pkts.len(), 50);
    }

    #[test]
    fn encode_rejects_too_wide() {
        let img = DynamicImage::ImageLuma8(GrayImage::from_pixel(400, 10, Luma([0])));
        assert!(encode_image(img, 384, 0, 127).is_err());
    }

    #[test]
    fn fill_label_exact_size() {
        let src = DynamicImage::ImageLuma8(GrayImage::from_pixel(50, 50, Luma([0])));
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384);
        let out = fill_label(src, lp);
        assert_eq!(out.dimensions(), (lp.width_px, lp.height_px));
    }

    #[test]
    fn contain_label_centers_with_white_margins() {
        // Tall image → letterbox left/right on a wide label.
        let src = DynamicImage::ImageLuma8(GrayImage::from_pixel(40, 80, Luma([0])));
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384);
        let out = contain_label(src, lp, 0).to_luma8();
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
        let out = contain_label(src, lp, margin).to_luma8();
        // Outer margin ring must be white.
        for x in 0..lp.width_px {
            assert_eq!(out.get_pixel(x, 0)[0], 255);
            assert_eq!(out.get_pixel(x, margin - 1)[0], 255);
        }
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

    /// One-shot: `cargo test --lib generate_photo_print_preview -- --ignored --nocapture`
    /// Writes `/tmp/thermark_photo_preview.png` (contain + margin 16 + dither, display-inverted).
    #[test]
    #[ignore = "writes /tmp/thermark_photo_preview.png"]
    fn generate_photo_print_preview() {
        let path = std::path::Path::new("fixtures/photo_unsplash_mountains.jpg");
        let img = image::open(path).expect("open photo fixture");
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384);
        let placed = contain_label(img, lp, 16);
        let bw = gray_to_print_bits(&placed.to_luma8(), 127, true);
        // Print bits: 255 = burn/black → invert for on-screen paper-like preview.
        let mut preview = GrayImage::new(bw.width(), bw.height());
        for (x, y, p) in bw.enumerate_pixels() {
            let v = if p[0] > 127 { 0 } else { 255 };
            preview.put_pixel(x, y, Luma([v]));
        }
        let out = "/tmp/thermark_photo_preview.png";
        preview.save(out).expect("save preview");
        eprintln!(
            "wrote {out} ({}x{}) contain+margin16+dither",
            preview.width(),
            preview.height()
        );
    }

    #[test]
    fn black_row_uses_bitmap_packet() {
        // full black row → PrintBitmapRow (0x85), not empty
        let mut g = GrayImage::from_pixel(16, 1, Luma([0]));
        for x in 0..16 {
            g.put_pixel(x, 0, Luma([255])); // our convention before invert path:
        }
        // encode_image inverts: white source → not printed. Use dark source.
        let img = DynamicImage::ImageLuma8(GrayImage::from_pixel(16, 2, Luma([0])));
        let (_w, _h, pkts) = encode_image(img, 384, 0, 127).unwrap();
        // after invert, black (0) becomes white (no print)… wait:
        // inv = 255-0 = 255 > threshold → bit 255 = print black. Good.
        assert!(pkts.iter().any(|p| p.cmd == Cmd::PrintBitmapRow as u8));
    }
}
