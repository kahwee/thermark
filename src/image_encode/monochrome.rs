//! Shared threshold and dither traversal for encoding and preview.

use super::raster::validate_encode_dimensions;
use crate::errors::Result;
use image::{GrayImage, Luma};

/// Whether the hard-threshold encoder burns a source luminance value.
#[inline]
pub(super) fn burns_at_threshold(luma: u8, threshold: u8) -> bool {
    255u8.saturating_sub(luma) > threshold
}

/// Convert a grayscale image to print bits (255 = burn / black).
///
/// Source dark pixels print. Hard threshold is fine for QR/text; **dither** is
/// better for photographs (avoids big blotchy black “bleed” regions).
pub fn gray_to_print_bits(gray: &GrayImage, threshold: u8, dither: bool) -> GrayImage {
    let (w, h) = gray.dimensions();
    let mut bw = GrayImage::new(w, h);
    for_each_print_bit(gray, threshold, dither, |x, y, burn| {
        if burn {
            bw.get_pixel_mut(x, y)[0] = 255;
        }
    });
    bw
}

/// Render validated print bits as a viewable black-on-white image.
///
/// This uses the same dimension checks and threshold/dither traversal as
/// [`super::encode_gray`], while mapping a burn bit to black instead of the encoder's
/// internal `255 = burn` mask convention.
pub fn render_print_preview(
    gray: &GrayImage,
    max_width: u32,
    threshold: u8,
    dither: bool,
) -> Result<GrayImage> {
    let (width, height) = gray.dimensions();
    validate_encode_dimensions(width, height, max_width)?;

    let mut preview = GrayImage::from_pixel(width, height, Luma([255]));
    for_each_print_bit(gray, threshold, dither, |x, y, burn| {
        if burn {
            preview.get_pixel_mut(x, y)[0] = 0;
        }
    });
    Ok(preview)
}

/// Visit thresholded pixels in row-major order without materialising a second
/// image. Dithering keeps only the current and next error rows, so memory is
/// proportional to page width instead of width × height.
pub(super) fn for_each_print_bit(
    gray: &GrayImage,
    threshold: u8,
    dither: bool,
    mut visit: impl FnMut(u32, u32, bool),
) {
    let (width, height) = gray.dimensions();
    let width_usize = width as usize;

    if width == 0 || height == 0 {
        return;
    }

    if !dither {
        for (y, row) in gray.as_raw().chunks_exact(width_usize).enumerate() {
            for (x, &luma) in row.iter().enumerate() {
                visit(x as u32, y as u32, burns_at_threshold(luma, threshold));
            }
        }
        return;
    }

    let source = gray.as_raw();
    let mut current = source[..width_usize]
        .iter()
        .map(|&luma| f32::from(255u8.saturating_sub(luma)))
        .collect::<Vec<_>>();
    let mut next = vec![0.0f32; width_usize];
    if height > 1 {
        initialize_error_row(&mut next, &source[width_usize..width_usize * 2]);
    }

    let threshold = f32::from(threshold);
    for y in 0..height as usize {
        for x in 0..width_usize {
            let old = current[x];
            let burn = old > threshold;
            let new = if burn { 255.0 } else { 0.0 };
            let error = old - new;
            visit(x as u32, y as u32, burn);

            // Standard Floyd–Steinberg coefficients. Keep the same update
            // order as the former full-page buffer for pixel-identical output.
            if x + 1 < width_usize {
                current[x + 1] += error * (7.0 / 16.0);
            }
            if y + 1 < height as usize {
                if x > 0 {
                    next[x - 1] += error * (3.0 / 16.0);
                }
                next[x] += error * (5.0 / 16.0);
                if x + 1 < width_usize {
                    next[x + 1] += error * (1.0 / 16.0);
                }
            }
        }

        std::mem::swap(&mut current, &mut next);
        if y + 2 < height as usize {
            let start = (y + 2) * width_usize;
            initialize_error_row(&mut next, &source[start..start + width_usize]);
        }
    }
}

fn initialize_error_row(error_row: &mut [f32], source_row: &[u8]) {
    for (error, &luma) in error_row.iter_mut().zip(source_row) {
        *error = f32::from(255u8.saturating_sub(luma));
    }
}
