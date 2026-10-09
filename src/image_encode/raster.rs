//! Validated pages and wire row encoding.

use super::alpha::grayscale_on_white;
use super::monochrome::for_each_print_bit;
use crate::errors::{Error, Result};
use crate::packet::{MAX_DATA_LEN, Packet};
use crate::protocol;
use image::{DynamicImage, GenericImageView, GrayImage};

/// Six bytes precede bitmap pixels in a row packet, and the frame length is a
/// single byte. Physical profiles are much narrower, but the public encoder
/// still rejects caller-supplied limits that could create an unsendable row.
pub(super) const MAX_BITMAP_WIDTH_PX: u32 = ((MAX_DATA_LEN - 6) * 8) as u32;

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
/// Rotate beforehand with [`super::rotate`] if needed.
pub fn encode(img: DynamicImage, max_width: u32, threshold: u8, dither: bool) -> Result<Raster> {
    let (width, height) = img.dimensions();
    validate_encode_dimensions(width, height, max_width)?;

    // Consuming the dynamic image lets an existing Luma8 buffer pass through
    // without a second page-sized copy.
    let gray = grayscale_on_white(img);
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

pub(super) fn validate_encode_dimensions(width: u32, height: u32, max_width: u32) -> Result<()> {
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
pub(super) fn push_row_run(out: &mut Vec<Packet>, start: u16, repeats: u8, pixels: Option<&[u8]>) {
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
