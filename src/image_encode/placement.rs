//! Image cropping, rotation, and label composition.

use super::alpha::grayscale_on_white;
use super::monochrome::burns_at_threshold;
use crate::errors::{Error, Result};
use crate::geometry::{LabelPx, Rect, SafeArea};
use crate::types::Rotation;
use image::{DynamicImage, GenericImageView, GrayImage, RgbaImage, imageops};

/// Apply a [`Rotation`] to an image.
pub fn rotate(img: DynamicImage, rotation: Rotation) -> DynamicImage {
    match rotation {
        Rotation::Deg0 => img,
        Rotation::Deg90 => img.rotate90(),
        Rotation::Deg180 => img.rotate180(),
        Rotation::Deg270 => img.rotate270(),
    }
}

/// Crop uniform white space from the edges of an image.
///
/// Artwork usually carries its own margin. Placing it on a label without
/// trimming means that margin is *added* to the configured registration
/// inset, so the drawing ends up far smaller than the media allows — a
/// bulldozer with a 35 px built-in margin lost another 29 rows to it after
/// scaling, on top of the 40 reserved rows.
///
/// `threshold` is a direct luminance ceiling: pixels with luma less than or
/// equal to it count as ink. Returns the image unchanged when it is blank or
/// already tight.
pub fn trim_white(img: DynamicImage, threshold: u8) -> DynamicImage {
    let ink = image_luma_bounds(&img, |luma| luma <= threshold);
    crop_to_bounds(img, ink)
}

/// Trim to the pixels that the selected print conversion would burn.
///
/// Hard-threshold trimming shares the encoder's exact darkness predicate and
/// can crop all four edges. Dithering only removes leading and trailing white
/// rows: the full source width is retained because white columns can carry
/// Floyd–Steinberg error between rows and change later burn decisions.
pub(crate) fn trim_for_print(img: DynamicImage, threshold: u8, dither: bool) -> DynamicImage {
    let ink = if dither {
        image_luma_bounds(&img, |luma| luma < u8::MAX).map(|bounds| Rect {
            x: 0,
            y: bounds.y,
            w: img.width(),
            h: bounds.h,
        })
    } else {
        image_luma_bounds(&img, |luma| burns_at_threshold(luma, threshold))
    };
    crop_to_bounds(img, ink)
}

pub(super) fn crop_to_bounds(img: DynamicImage, bounds: Option<Rect>) -> DynamicImage {
    let (width, height) = img.dimensions();
    let Some(bounds) = bounds else {
        return img;
    };
    if bounds.x == 0 && bounds.y == 0 && bounds.w == width && bounds.h == height {
        return img;
    }
    // DynamicImage::crop_imm dispatches to the underlying buffer, preserving
    // its pixel format. Converting the entire input to RGBA first made a
    // grayscale crop four times larger than necessary.
    img.crop_imm(bounds.x, bounds.y, bounds.w, bounds.h)
}

/// Find matching luminance directly in native grayscale images. Other formats
/// go through the same color conversion and white alpha composite as encoding,
/// so trimming cannot disagree with the eventual print at a threshold edge.
fn image_luma_bounds(img: &DynamicImage, predicate: impl FnMut(u8) -> bool) -> Option<Rect> {
    if let Some(gray) = img.as_luma8() {
        return luma_bounds_by(
            gray.enumerate_pixels()
                .map(|(x, y, pixel)| (x, y, pixel[0])),
            predicate,
        );
    }

    // RGB8 is the common decoded-photo path. A full `to_luma8` conversion
    // temporarily retains one byte per source pixel solely to calculate four
    // coordinates. Convert bounded stripes instead, preserving the image
    // crate's exact color-space-aware conversion at threshold boundaries.
    if let Some(rgb) = img.as_rgb8() {
        return striped_rgb8_luma_bounds(rgb, predicate);
    }

    if !img.has_alpha() {
        let gray = img.to_luma8();
        return luma_bounds_by(
            gray.enumerate_pixels()
                .map(|(x, y, pixel)| (x, y, pixel[0])),
            predicate,
        );
    }

    // Alpha-bearing images need compositing before luma conversion, but trim
    // borrows its input so it can later return the original pixel format.
    striped_luma_bounds(img, predicate, grayscale_on_white)
}

fn striped_rgb8_luma_bounds(
    rgb: &image::RgbImage,
    mut predicate: impl FnMut(u8) -> bool,
) -> Option<Rect> {
    const STRIPE_ROWS: u32 = 64;
    let (width, height) = rgb.dimensions();
    let row_bytes = width as usize * 3;
    let color_space = rgb.color_space();
    let mut bounds = LumaBounds::new();
    let mut y = 0;
    while width != 0 && y < height {
        let rows = (height - y).min(STRIPE_ROWS);
        let start = y as usize * row_bytes;
        let end = (y + rows) as usize * row_bytes;
        let mut stripe = image::RgbImage::from_raw(width, rows, rgb.as_raw()[start..end].to_vec())
            .expect("a source-aligned RGB stripe has valid dimensions");
        stripe
            .set_color_space(color_space)
            .expect("the source RGB image already has a valid color space");
        let gray = DynamicImage::ImageRgb8(stripe).into_luma8();
        for (x, stripe_y, pixel) in gray.enumerate_pixels() {
            if predicate(pixel[0]) {
                bounds.include(x, y + stripe_y);
            }
        }
        y += rows;
    }
    bounds.finish()
}

/// Convert borrowed input in bounded-height stripes while accumulating bounds.
///
/// Cropping copies the source color-space metadata into each stripe, which is
/// important: calculating RGB luminance ourselves would disagree with
/// `DynamicImage::to_luma8` for non-sRGB inputs.
fn striped_luma_bounds(
    img: &DynamicImage,
    mut predicate: impl FnMut(u8) -> bool,
    mut into_gray: impl FnMut(DynamicImage) -> GrayImage,
) -> Option<Rect> {
    const STRIPE_ROWS: u32 = 64;
    let (width, height) = img.dimensions();
    let mut bounds = LumaBounds::new();
    let mut y = 0;
    while width != 0 && y < height {
        let rows = (height - y).min(STRIPE_ROWS);
        let gray = into_gray(img.crop_imm(0, y, width, rows));
        for (x, stripe_y, pixel) in gray.enumerate_pixels() {
            if predicate(pixel[0]) {
                bounds.include(x, y + stripe_y);
            }
        }
        y += rows;
    }
    bounds.finish()
}

pub(super) fn luma_bounds_by(
    pixels: impl Iterator<Item = (u32, u32, u8)>,
    mut predicate: impl FnMut(u8) -> bool,
) -> Option<Rect> {
    let mut bounds = LumaBounds::new();
    for (x, y, luma) in pixels {
        if predicate(luma) {
            bounds.include(x, y);
        }
    }
    bounds.finish()
}

struct LumaBounds {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

impl LumaBounds {
    fn new() -> Self {
        Self {
            x0: u32::MAX,
            y0: u32::MAX,
            x1: 0,
            y1: 0,
        }
    }

    fn include(&mut self, x: u32, y: u32) {
        self.x0 = self.x0.min(x);
        self.y0 = self.y0.min(y);
        self.x1 = self.x1.max(x);
        self.y1 = self.y1.max(y);
    }

    fn finish(self) -> Option<Rect> {
        (self.x0 != u32::MAX).then(|| Rect {
            x: self.x0,
            y: self.y0,
            w: self.x1 - self.x0 + 1,
            h: self.y1 - self.y0 + 1,
        })
    }
}

/// Bounding box of ink — pixels at or below `threshold` — or `None` if blank.
///
/// One implementation for a question asked all over this crate and its tests:
/// where did anything actually get drawn? Answering it by hand each time is how
/// two call sites end up disagreeing about what counts as ink.
pub fn ink_bounds(gray: &GrayImage, threshold: u8) -> Option<Rect> {
    luma_bounds_by(
        gray.enumerate_pixels()
            .map(|(x, y, pixel)| (x, y, pixel[0])),
        |luma| luma <= threshold,
    )
}

/// Resize preserving aspect to fit within max width (height free).
pub fn fit_width(img: DynamicImage, max_width: u32) -> DynamicImage {
    let (w, h) = img.dimensions();
    if w <= max_width {
        return img;
    }
    let new_h = ((h as f64) * (max_width as f64) / (w as f64)).round() as u32;
    match img {
        // Avoid expanding the overwhelmingly common monochrome input from one
        // byte to four bytes per pixel. Other formats retain the established
        // RGBA8 conversion: resizing high-precision channels in place can move
        // values across the later 8-bit print threshold.
        DynamicImage::ImageLuma8(gray) => DynamicImage::ImageLuma8(imageops::resize(
            &gray,
            max_width,
            new_h.max(1),
            imageops::FilterType::Triangle,
        )),
        image => DynamicImage::ImageRgba8(imageops::resize(
            &image,
            max_width,
            new_h.max(1),
            imageops::FilterType::Triangle,
        )),
    }
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
    fn for_label(label: LabelPx, safe: SafeArea, margin: u32) -> Result<Self> {
        let area = safe.content(label).ok_or_else(|| {
            Error::invalid_label(format!(
                "safe area (top {}, bottom {}, left {}, right {}) leaves no content on a {}x{}px label",
                safe.top,
                safe.bottom,
                safe.left,
                safe.right,
                label.width_px,
                label.height_px
            ))
        })?;
        Ok(Self::in_rect(label, area, margin))
    }

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

/// Smallest centred integer source rectangle that covers the destination.
///
/// The ideal fractional crop is rounded outward; the uniform resize below then
/// crops the sub-pixel remainder without stretching it. Cropping first bounds
/// the intermediate image, while resizing the whole source can create millions
/// of unused rows for a very tall, narrow input.
fn cover_crop(iw: u32, ih: u32, target_w: u32, target_h: u32) -> Rect {
    let center_aligned = |source: u32, crop: u32| {
        if crop < source && (source - crop) % 2 == 1 {
            crop + 1
        } else {
            crop
        }
    };
    let source_cross = u64::from(iw) * u64::from(target_h);
    let target_cross = u64::from(target_w) * u64::from(ih);

    if source_cross > target_cross {
        // Source is wider than the target: keep its full height and crop width.
        let numerator = u64::from(ih) * u64::from(target_w);
        let crop_w = center_aligned(
            iw,
            numerator
                .div_ceil(u64::from(target_h))
                .clamp(1, u64::from(iw)) as u32,
        );
        Rect {
            x: (iw - crop_w) / 2,
            y: 0,
            w: crop_w,
            h: ih,
        }
    } else if source_cross < target_cross {
        // Source is taller than the target: keep its full width and crop height.
        let numerator = u64::from(iw) * u64::from(target_h);
        let crop_h = center_aligned(
            ih,
            numerator
                .div_ceil(u64::from(target_w))
                .clamp(1, u64::from(ih)) as u32,
        );
        Rect {
            x: 0,
            y: (ih - crop_h) / 2,
            w: iw,
            h: crop_h,
        }
    } else {
        Rect {
            x: 0,
            y: 0,
            w: iw,
            h: ih,
        }
    }
}

/// Cover-fit `img` into the configured content area, cropping overflow.
///
/// Makes content as large as the media allows; `margin` keeps a white border
/// so heat is less likely to run to the edge. Pass [`SafeArea::NONE`] for full
/// bleed. Returns a full-size canvas with the image placed inside `safe`, or
/// an error when the insets leave no content area on the label.
pub fn fill_label(
    img: DynamicImage,
    label: LabelPx,
    safe: SafeArea,
    margin: u32,
) -> Result<DynamicImage> {
    let bx = ContentBox::for_label(label, safe, margin)?;
    let (iw, ih) = img.dimensions();
    let crop = cover_crop(iw, ih, bx.width, bx.height);
    let source = imageops::crop_imm(&img, crop.x, crop.y, crop.w, crop.h);
    // Keep one scale factor after the integer source crop. Resizing the crop
    // directly to the destination would stretch tiny or non-integral aspect
    // ratios; this bounded intermediate uses at most two source pixels beyond
    // the ideal fractional crop (one for rounding, one to retain its centre).
    let scale = f64::max(
        bx.width as f64 / crop.w as f64,
        bx.height as f64 / crop.h as f64,
    );
    let nw = ((crop.w as f64) * scale).round().max(1.0) as u32;
    let nh = ((crop.h as f64) * scale).round().max(1.0) as u32;
    let resized = imageops::resize(
        &*source,
        nw.max(bx.width),
        nh.max(bx.height),
        imageops::FilterType::CatmullRom,
    );
    let visible = imageops::crop_imm(
        &resized,
        resized.width().saturating_sub(bx.width) / 2,
        resized.height().saturating_sub(bx.height) / 2,
        bx.width,
        bx.height,
    );

    let mut canvas = bx.white_canvas();
    imageops::overlay(
        &mut canvas,
        &*visible,
        bx.origin_x as i64,
        bx.origin_y as i64,
    );
    Ok(DynamicImage::ImageRgba8(canvas))
}

/// Scale `img` to **fit entirely** inside the configured content area.
///
/// Prefer this for photographs so nothing is cropped. Pass [`SafeArea::NONE`]
/// to use the whole canvas. Returns an error when the insets leave no content
/// area on the label.
pub fn contain_label(
    img: DynamicImage,
    label: LabelPx,
    safe: SafeArea,
    margin: u32,
) -> Result<DynamicImage> {
    let bx = ContentBox::for_label(label, safe, margin)?;
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
    Ok(DynamicImage::ImageRgba8(canvas))
}
