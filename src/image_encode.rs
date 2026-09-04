//! Rasterize images into NIIMBOT 1-bit row packets.

use crate::errors::{Error, Result};
use crate::geometry::{LabelPx, Rect, SafeArea};
use crate::packet::{MAX_DATA_LEN, Packet};
use crate::protocol;
use crate::types::Rotation;
use image::{DynamicImage, GenericImageView, GrayImage, Luma, Pixel, RgbaImage, imageops};

/// Six bytes precede bitmap pixels in a row packet, and the frame length is a
/// single byte. Physical profiles are much narrower, but the public encoder
/// still rejects caller-supplied limits that could create an unsendable row.
const MAX_BITMAP_WIDTH_PX: u32 = ((MAX_DATA_LEN - 6) * 8) as u32;

/// An encoded page: row packets plus the dimensions they were built from.
///
/// Bundling the three keeps them from drifting apart — the printer needs the
/// size in `SetPageSize` to agree with the rows it then receives, and passing
/// them as three loose arguments made disagreement easy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raster {
    width: u32,
    height: u32,
    rows: Vec<Packet>,
}

impl Raster {
    /// Construct an encoded page while enforcing page/row invariants.
    pub fn try_new(width: u32, height: u32, rows: Vec<Packet>) -> Result<Self> {
        let raster = Self {
            width,
            height,
            rows,
        };
        raster.validate()?;
        Ok(raster)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn rows(&self) -> &[Packet] {
        &self.rows
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub(crate) fn into_parts(self) -> (u32, u32, Vec<Packet>) {
        (self.width, self.height, self.rows)
    }

    #[cfg(test)]
    pub(crate) fn from_parts_unchecked(width: u32, height: u32, rows: Vec<Packet>) -> Self {
        Self {
            width,
            height,
            rows,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.width == 0 || self.height == 0 {
            return Err(Error::InvalidRaster("dimensions must be non-zero".into()));
        }
        if u16::try_from(self.width).is_err() || u16::try_from(self.height).is_err() {
            return Err(Error::ImageTooLarge {
                width: self.width,
                height: self.height,
            });
        }
        let mut logical_row = 0u32;
        for (packet_index, row) in self.rows.iter().enumerate() {
            if row.data.len() > MAX_DATA_LEN {
                return Err(Error::InvalidRaster(format!(
                    "row packet {packet_index} is too large for the one-byte frame length"
                )));
            }
            let expected_index = (logical_row as u16).to_be_bytes();
            if row.data.get(..2) != Some(expected_index.as_slice()) {
                return Err(Error::InvalidRaster(format!(
                    "row packet {packet_index} starts at the wrong logical row"
                )));
            }
            let repeats = match row.cmd {
                cmd if cmd == protocol::Cmd::PrintEmptyRow as u8 => {
                    if row.data.len() != 3 || row.data[2] == 0 {
                        return Err(Error::InvalidRaster(format!(
                            "row packet {packet_index} has an invalid empty-row payload"
                        )));
                    }
                    row.data[2]
                }
                cmd if cmd == protocol::Cmd::PrintBitmapRow as u8 => {
                    let pixel_bytes = (self.width as usize).div_ceil(8);
                    if row.data.len() != 6 + pixel_bytes || row.data[5] == 0 {
                        return Err(Error::InvalidRaster(format!(
                            "row packet {packet_index} has an invalid bitmap-row payload"
                        )));
                    }
                    row.data[5]
                }
                _ => {
                    return Err(Error::InvalidRaster(format!(
                        "row packet {packet_index} uses a non-row command"
                    )));
                }
            };
            logical_row = logical_row.checked_add(u32::from(repeats)).ok_or_else(|| {
                Error::InvalidRaster("row repeat count overflowed page height".into())
            })?;
            if logical_row > self.height {
                return Err(Error::InvalidRaster(format!(
                    "row packet {packet_index} repeats beyond page height {}",
                    self.height
                )));
            }
        }
        if logical_row != self.height {
            return Err(Error::InvalidRaster(format!(
                "height is {} but row packets cover {logical_row} logical rows",
                self.height
            )));
        }
        Ok(())
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
/// invert grayscale, then convert to 1-bit.
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
    let (width, height) = img.dimensions();
    validate_encode_dimensions(width, height, max_width)?;

    // Consuming the dynamic image lets an existing Luma8 buffer pass through
    // without a second page-sized copy.
    let gray = img.into_luma8();
    encode_gray_validated(&gray, width, height, threshold, dither)
}

/// Threshold a borrowed grayscale image and emit print row packets.
///
/// This is the zero-copy entry point for renderers that already produce a
/// [`GrayImage`], such as QR, text, and calibration labels.
pub fn encode_gray(
    gray: &GrayImage,
    max_width: u32,
    threshold: u8,
    dither: bool,
) -> Result<Raster> {
    let (width, height) = gray.dimensions();
    validate_encode_dimensions(width, height, max_width)?;
    encode_gray_validated(gray, width, height, threshold, dither)
}

fn validate_encode_dimensions(width: u32, height: u32, max_width: u32) -> Result<()> {
    if width > max_width {
        return Err(Error::ImageTooWide {
            width,
            max: max_width,
        });
    }
    if width == 0 || height == 0 {
        return Err(Error::InvalidRaster("dimensions must be non-zero".into()));
    }
    if width > MAX_BITMAP_WIDTH_PX
        || u16::try_from(width).is_err()
        || u16::try_from(height).is_err()
    {
        return Err(Error::ImageTooLarge { width, height });
    }
    Ok(())
}

fn encode_gray_validated(
    gray: &GrayImage,
    width: u32,
    height: u32,
    threshold: u8,
    dither: bool,
) -> Result<Raster> {
    let bytes_per_row = (width as usize).div_ceil(8);
    let mut packed = vec![0u8; bytes_per_row];
    let mut all_white = true;
    let mut runs = RowRunEncoder::new();

    for_each_print_bit(gray, threshold, dither, |x, y, burn| {
        if x == 0 {
            packed.fill(0);
            all_white = true;
        }
        if burn {
            all_white = false;
            packed[(x / 8) as usize] |= 1 << (7 - (x % 8));
        }
        if x + 1 == width {
            runs.push(y as u16, (!all_white).then_some(&packed));
        }
    });

    Raster::try_new(width, height, runs.finish())
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

/// Visit thresholded pixels in row-major order without materialising a second
/// image. Dithering keeps only the current and next error rows, so memory is
/// proportional to page width instead of width × height.
fn for_each_print_bit(
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
                let inverted = 255u8.saturating_sub(luma);
                visit(x as u32, y as u32, inverted > threshold);
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

fn push_row_run(out: &mut Vec<Packet>, start: u16, repeats: u8, pixels: Option<&[u8]>) {
    out.push(match pixels {
        Some(pixels) => protocol::print_bitmap_row(start, repeats, pixels),
        None => protocol::print_empty_row(start, repeats),
    });
}

/// Coalesce adjacent equal rows while reusing the caller's packing buffer.
/// Pixel bytes are copied only when a new run begins, not once per source row.
struct RowRunEncoder {
    packets: Vec<Packet>,
    run_start: u16,
    run_pixels: Option<Vec<u8>>,
    run_len: u8,
}

impl RowRunEncoder {
    fn new() -> Self {
        Self {
            packets: Vec::new(),
            run_start: 0,
            run_pixels: None,
            run_len: 0,
        }
    }

    fn push(&mut self, row_index: u16, pixels: Option<&[u8]>) {
        let matches_run = self.run_len > 0
            && match (&self.run_pixels, pixels) {
                (Some(current), Some(next)) => current.as_slice() == next,
                (None, None) => true,
                _ => false,
            };
        if matches_run && self.run_len < u8::MAX {
            self.run_len += 1;
            return;
        }
        self.flush();
        self.run_start = row_index;
        self.run_pixels = pixels.map(<[u8]>::to_vec);
        self.run_len = 1;
    }

    fn flush(&mut self) {
        if self.run_len > 0 {
            push_row_run(
                &mut self.packets,
                self.run_start,
                self.run_len,
                self.run_pixels.as_deref(),
            );
            self.run_len = 0;
        }
    }

    fn finish(mut self) -> Vec<Packet> {
        self.flush();
        self.packets
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
/// Returns the image unchanged when it is blank or already tight.
pub fn trim_white(img: DynamicImage, threshold: u8) -> DynamicImage {
    let (w, h) = img.dimensions();
    let Some(ink) = image_ink_bounds(&img, threshold) else {
        return img; // nothing but background
    };
    if ink.x == 0 && ink.y == 0 && ink.w == w && ink.h == h {
        return img; // already tight
    }
    // DynamicImage::crop_imm dispatches to the underlying buffer, preserving
    // its pixel format. Converting the entire input to RGBA first made a
    // grayscale crop four times larger than necessary.
    img.crop_imm(ink.x, ink.y, ink.w, ink.h)
}

/// Find ink directly in common 8-bit images without allocating a full
/// grayscale copy. High-precision formats keep the library's conversion order:
/// converting their channels to RGBA8 first can move luminance by one at a
/// threshold boundary.
fn image_ink_bounds(img: &DynamicImage, threshold: u8) -> Option<Rect> {
    if let Some(gray) = img.as_luma8() {
        return ink_bounds(gray, threshold);
    }
    let color = img.color();
    if color.bits_per_pixel() / u16::from(color.channel_count()) > 8 {
        return ink_bounds(&img.to_luma8(), threshold);
    }
    ink_bounds_from_luma(
        img.pixels().map(|(x, y, pixel)| (x, y, pixel.to_luma()[0])),
        threshold,
    )
}

fn ink_bounds_from_luma(
    pixels: impl Iterator<Item = (u32, u32, u8)>,
    threshold: u8,
) -> Option<Rect> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for (x, y, luma) in pixels {
        if luma <= threshold {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    (x0 != u32::MAX).then(|| Rect {
        x: x0,
        y: y0,
        w: x1 - x0 + 1,
        h: y1 - y0 + 1,
    })
}

/// Bounding box of ink — pixels at or below `threshold` — or `None` if blank.
///
/// One implementation for a question asked all over this crate and its tests:
/// where did anything actually get drawn? Answering it by hand each time is how
/// two call sites end up disagreeing about what counts as ink.
pub fn ink_bounds(gray: &GrayImage, threshold: u8) -> Option<Rect> {
    ink_bounds_from_luma(
        gray.enumerate_pixels()
            .map(|(x, y, pixel)| (x, y, pixel[0])),
        threshold,
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
    DynamicImage::ImageRgba8(canvas)
}

/// Scale `img` to **fit entirely** inside the configured content area.
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
pub fn calibration_pattern(
    label: LabelPx,
    safe: Option<SafeArea>,
    pixels_per_mm: f64,
) -> GrayImage {
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
    let ring_step = (0.5 * pixels_per_mm).round().max(1.0) as u32;
    for ring in 0..CALIBRATION_RINGS {
        let inset = ring * ring_step;
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
    let ruler_scale = pixels_per_mm / crate::geometry::PX_PER_MM;
    let major_len = (f64::from(CALIBRATION_RULER_MAJOR_PX) * ruler_scale).round() as u32;
    let minor_len = (f64::from(CALIBRATION_RULER_MINOR_PX) * ruler_scale).round() as u32;
    let height_mm = (f64::from(h) / pixels_per_mm).floor() as u32;
    for mm in 0..=height_mm {
        let y = (f64::from(mm) * pixels_per_mm).round() as u32;
        if y >= h {
            break;
        }
        let major = mm % 5 == 0;
        let len = if major { major_len } else { minor_len };
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
        assert_eq!(pkts.len(), 1);
        assert_eq!(pkts[0].data[5], 50);
    }

    #[test]
    fn encode_rejects_too_wide() {
        let img = DynamicImage::ImageLuma8(GrayImage::from_pixel(400, 10, Luma([0])));
        assert!(encode(img, 384, 127, false).is_err());
    }

    #[test]
    fn borrowed_and_owned_gray_encoding_match() {
        let gray = GrayImage::from_fn(17, 13, |x, y| Luma([((x * 37 + y * 71) & 0xff) as u8]));
        for dither in [false, true] {
            let borrowed = encode_gray(&gray, 384, 113, dither).unwrap();
            let owned = encode(DynamicImage::ImageLuma8(gray.clone()), 384, 113, dither).unwrap();
            assert_eq!(borrowed, owned);
        }
    }

    #[test]
    fn encode_rejects_an_over_tall_page_before_row_indices_wrap() {
        let gray = GrayImage::from_pixel(1, u32::from(u16::MAX) + 1, Luma([255]));
        assert!(matches!(
            encode_gray(&gray, 384, 127, false),
            Err(Error::ImageTooLarge {
                width: 1,
                height: 65_536
            })
        ));
    }

    #[test]
    fn encode_rejects_rows_too_wide_for_the_packet_length_byte() {
        let width = MAX_BITMAP_WIDTH_PX + 1;
        let gray = GrayImage::from_pixel(width, 1, Luma([0]));
        assert!(matches!(
            encode_gray(&gray, width, 127, false),
            Err(Error::ImageTooLarge { width: w, height: 1 }) if w == width
        ));
    }

    #[test]
    fn raster_constructor_rejects_malformed_row_payloads() {
        let missing_repeat = Packet::new(protocol::Cmd::PrintEmptyRow as u8, [0, 0]);
        assert!(matches!(
            Raster::try_new(8, 1, vec![missing_repeat]),
            Err(Error::InvalidRaster(_))
        ));

        let wrong_bitmap_width = protocol::print_bitmap_row(0, 1, &[0, 0]);
        assert!(matches!(
            Raster::try_new(8, 1, vec![wrong_bitmap_width]),
            Err(Error::InvalidRaster(_))
        ));

        let repeated = protocol::print_empty_row(0, 2);
        assert!(matches!(
            Raster::try_new(8, 1, vec![repeated]),
            Err(Error::InvalidRaster(_))
        ));

        let gap = protocol::print_empty_row(1, 1);
        assert!(matches!(
            Raster::try_new(8, 1, vec![gap]),
            Err(Error::InvalidRaster(_))
        ));

        let oversized = protocol::print_bitmap_row(0, 1, &[0; MAX_DATA_LEN - 5]);
        assert!(matches!(
            Raster::try_new(MAX_BITMAP_WIDTH_PX + 1, 1, vec![oversized]),
            Err(Error::InvalidRaster(_))
        ));
    }

    #[test]
    fn repeat_coalescing_splits_runs_at_255() {
        let blank = DynamicImage::ImageLuma8(GrayImage::from_pixel(8, 640, Luma([255])));
        let raster = encode(blank, 384, 127, false).unwrap();
        assert_eq!(raster.rows.len(), 3);
        let starts_and_repeats: Vec<_> = raster
            .rows
            .iter()
            .map(|packet| {
                (
                    u16::from_be_bytes([packet.data[0], packet.data[1]]),
                    packet.data[2],
                )
            })
            .collect();
        assert_eq!(starts_and_repeats, [(0, 255), (255, 255), (510, 130)]);
        raster.validate().unwrap();
    }

    #[test]
    fn only_consecutive_identical_rows_are_coalesced() {
        let mut image = GrayImage::from_pixel(8, 4, Luma([255]));
        for x in 0..8 {
            image.put_pixel(x, 1, Luma([0]));
            image.put_pixel(x, 3, Luma([0]));
        }
        let raster = encode(DynamicImage::ImageLuma8(image), 384, 127, false).unwrap();
        assert_eq!(raster.rows.len(), 4);
        assert!(raster.rows.iter().all(|packet| match packet.cmd {
            cmd if cmd == Cmd::PrintEmptyRow as u8 => packet.data[2] == 1,
            cmd if cmd == Cmd::PrintBitmapRow as u8 => packet.data[5] == 1,
            _ => false,
        }));
    }

    #[test]
    fn fused_encoder_matches_reference_packet_packing() {
        fn reference_packets(gray: &GrayImage, threshold: u8, dither: bool) -> Vec<Packet> {
            let bits = gray_to_print_bits(gray, threshold, dither);
            let (width, height) = bits.dimensions();
            let mut packets = Vec::new();
            let mut run_start = 0u16;
            let mut run_pixels: Option<Vec<u8>> = None;
            let mut run_len = 0u8;

            for y in 0..height {
                let mut row = vec![0u8; (width as usize).div_ceil(8)];
                for x in 0..width {
                    if bits.get_pixel(x, y)[0] > 127 {
                        row[(x / 8) as usize] |= 1 << (7 - (x % 8));
                    }
                }
                let pixels = row.iter().any(|&byte| byte != 0).then_some(row);
                if run_len > 0 && run_pixels == pixels && run_len < u8::MAX {
                    run_len += 1;
                    continue;
                }
                if run_len > 0 {
                    push_row_run(&mut packets, run_start, run_len, run_pixels.as_deref());
                }
                run_start = y as u16;
                run_pixels = pixels;
                run_len = 1;
            }
            if run_len > 0 {
                push_row_run(&mut packets, run_start, run_len, run_pixels.as_deref());
            }
            packets
        }

        for width in [1, 7, 8, 9, 31, 384] {
            let gray = GrayImage::from_fn(width, 17, |x, y| {
                Luma([((x * 37) ^ (y * 73) ^ (x * y)) as u8])
            });
            for threshold in [0, 127, 255] {
                for dither in [false, true] {
                    assert_eq!(
                        encode_gray(&gray, 384, threshold, dither).unwrap().rows,
                        reference_packets(&gray, threshold, dither),
                        "width={width}, threshold={threshold}, dither={dither}"
                    );
                }
            }
        }
    }

    #[test]
    fn fill_label_exact_size() {
        let src = DynamicImage::ImageLuma8(GrayImage::from_pixel(50, 50, Luma([0])));
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384, 8.0);
        let out = fill_label(src, lp, SafeArea::NONE, 0);
        assert_eq!(out.dimensions(), (lp.width_px, lp.height_px));
    }

    #[test]
    fn fill_label_centers_cover_crop_inside_safe_area_and_margin() {
        // Only the centred third is ink. Cover-fitting this wide source to the
        // tall content box must discard both white outer thirds before resize.
        let mut source = GrayImage::from_pixel(12, 4, Luma([255]));
        for y in 0..4 {
            for x in 4..8 {
                source.put_pixel(x, y, Luma([0]));
            }
        }
        let label = LabelPx {
            width_px: 10,
            height_px: 8,
        };
        let safe = SafeArea {
            top: 1,
            bottom: 1,
            left: 2,
            right: 2,
        };
        let output = fill_label(DynamicImage::ImageLuma8(source), label, safe, 1).to_luma8();

        assert_eq!(output.dimensions(), (10, 8));
        assert_eq!(
            ink_bounds(&output, 127),
            Some(Rect {
                x: 3,
                y: 2,
                w: 4,
                h: 4,
            })
        );
    }

    #[test]
    fn fill_label_handles_an_extreme_aspect_ratio() {
        // A resize-then-crop implementation expands this 1x100,000 source to
        // a 64x6,400,000 RGBA intermediate even though it uses only 32 rows.
        // Crop-first reduces the resize input to the two centre-aligned source
        // pixels (the extra one keeps an even-sized image exactly centred).
        let mut source = GrayImage::from_pixel(1, 100_000, Luma([255]));
        source.put_pixel(0, 49_999, Luma([0]));
        source.put_pixel(0, 50_000, Luma([0]));
        let label = LabelPx {
            width_px: 64,
            height_px: 32,
        };
        let output =
            fill_label(DynamicImage::ImageLuma8(source), label, SafeArea::NONE, 0).to_luma8();

        assert_eq!(output.dimensions(), (64, 32));
        assert!(output.pixels().all(|pixel| pixel[0] == 0));
    }

    #[test]
    fn fill_label_keeps_one_scale_factor_for_tiny_sources() {
        // The exact cover crop is 2x1.25 source pixels. Rounding that to one
        // row and stretching it directly made this entire label solid black.
        let mut source = GrayImage::from_pixel(2, 3, Luma([255]));
        for x in 0..2 {
            source.put_pixel(x, 1, Luma([0]));
        }
        let label = LabelPx {
            width_px: 384,
            height_px: 240,
        };
        let output =
            fill_label(DynamicImage::ImageLuma8(source), label, SafeArea::NONE, 0).to_luma8();

        let (darkest, lightest) = output.pixels().fold((u8::MAX, u8::MIN), |(lo, hi), pixel| {
            (lo.min(pixel[0]), hi.max(pixel[0]))
        });
        assert!(
            darkest < lightest,
            "cover resize flattened all source detail"
        );
        for x in 0..label.width_px {
            assert_eq!(
                output.get_pixel(x, 0),
                output.get_pixel(x, label.height_px - 1),
                "integer source crop shifted away from the image center"
            );
        }
    }

    #[test]
    fn contain_label_centers_with_white_margins() {
        // Tall image → letterbox left/right on a wide label.
        let src = DynamicImage::ImageLuma8(GrayImage::from_pixel(40, 80, Luma([0])));
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384, 8.0);
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
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384, 8.0);
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
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384, 8.0);
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
    fn trim_preserves_the_source_pixel_format() {
        let mut rgb = image::RgbImage::from_pixel(8, 6, image::Rgb([255, 255, 255]));
        for y in 2..4 {
            for x in 3..6 {
                rgb.put_pixel(x, y, image::Rgb([0, 0, 0]));
            }
        }

        let output = trim_white(DynamicImage::ImageRgb8(rgb), 127);
        assert_eq!(output.dimensions(), (3, 2));
        let DynamicImage::ImageRgb8(cropped) = output else {
            panic!("trimming converted an RGB source to another pixel format");
        };
        assert!(cropped.pixels().all(|pixel| pixel.0 == [0, 0, 0]));
    }

    #[test]
    fn trim_keeps_high_precision_luminance_at_threshold_boundaries() {
        let mut rgb = image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::from_pixel(
            3,
            1,
            image::Rgb([u16::MAX; 3]),
        );
        // Converting channels to RGBA8 before luminance gives 93; the image
        // crate's direct RGB16 -> Luma8 conversion gives 94.
        rgb.put_pixel(1, 0, image::Rgb([10_103, 24_170, 64_193]));

        let output = trim_white(DynamicImage::ImageRgb16(rgb), 93);
        assert_eq!(output.dimensions(), (3, 1));
        assert!(matches!(output, DynamicImage::ImageRgb16(_)));
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
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384, 8.0);
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
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384, 8.0);
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
    fn rolling_dither_matches_the_full_page_algorithm() {
        fn full_page_dither(gray: &GrayImage, threshold: u8) -> GrayImage {
            let (w, h) = gray.dimensions();
            let mut error = gray
                .pixels()
                .map(|pixel| f32::from(255u8.saturating_sub(pixel[0])))
                .collect::<Vec<_>>();
            let mut output = GrayImage::new(w, h);
            let threshold = f32::from(threshold);
            for y in 0..h {
                for x in 0..w {
                    let index = (y * w + x) as usize;
                    let old = error[index];
                    let new = if old > threshold { 255.0 } else { 0.0 };
                    let delta = old - new;
                    output.put_pixel(x, y, Luma([new as u8]));
                    if x + 1 < w {
                        error[index + 1] += delta * (7.0 / 16.0);
                    }
                    if y + 1 < h {
                        let next_row = index + w as usize;
                        if x > 0 {
                            error[next_row - 1] += delta * (3.0 / 16.0);
                        }
                        error[next_row] += delta * (5.0 / 16.0);
                        if x + 1 < w {
                            error[next_row + 1] += delta * (1.0 / 16.0);
                        }
                    }
                }
            }
            output
        }

        let gray = GrayImage::from_fn(31, 19, |x, y| Luma([((x * 29) ^ (y * 83) ^ (x * y)) as u8]));
        for threshold in [0, 63, 127, 191, 255] {
            assert_eq!(
                gray_to_print_bits(&gray, threshold, true),
                full_page_dither(&gray, threshold)
            );
        }
    }

    #[test]
    fn empty_gray_conversion_stays_empty() {
        assert!(gray_to_print_bits(&GrayImage::new(0, 0), 127, false).is_empty());
        assert!(gray_to_print_bits(&GrayImage::new(0, 0), 127, true).is_empty());
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
