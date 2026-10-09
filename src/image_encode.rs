//! Image composition and NIIMBOT 1-bit row encoding.
//!
//! Private stages share the same print conversion for previews and wire output.

mod alpha;
mod calibration;
mod monochrome;
mod placement;
mod raster;

pub use alpha::grayscale_on_white;
pub use calibration::{
    CALIBRATION_RING_STEP_PX, CALIBRATION_RINGS, CALIBRATION_RULER_MAJOR_PX,
    CALIBRATION_RULER_MINOR_PX, calibration_pattern,
};
pub use monochrome::{gray_to_print_bits, render_print_preview};
pub(crate) use placement::trim_for_print;
pub use placement::{contain_label, fill_label, fit_width, ink_bounds, rotate, trim_white};
pub use raster::{Raster, encode, encode_gray, encode_path};

#[cfg(test)]
mod tests;
