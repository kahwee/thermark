use super::*;

/// Calibration pattern with printed millimetre numbers down the feed ruler.
///
/// The geometry comes from [`crate::image_encode::calibration_pattern`];
/// this adds the numerals, which need font access. Numbers make the photo
/// self-describing: read the last one that printed rather than counting ticks
/// from an edge that may itself be cut off.
pub fn make_calibration_label(
    label: LabelPx,
    safe: SafeArea,
    pixels_per_mm: f64,
    font_path: Option<&std::path::Path>,
) -> Result<GrayImage> {
    let mut img = crate::image_encode::calibration_pattern(label, Some(safe), pixels_per_mm);
    let Ok(font) = load_font(font_path, None) else {
        // Font loading failed; the geometric pattern remains usable.
        return Ok(img);
    };

    let scale = pixels_per_mm / crate::geometry::PX_PER_MM;
    let size = (13.0 * scale) as f32;
    // Clear of the ruler ticks, derived from their length rather than a second
    // constant that has to be kept in agreement by hand.
    let inset =
        ((crate::image_encode::CALIBRATION_RULER_MAJOR_PX + 4) as f64 * scale).round() as u32;

    let height_mm = (f64::from(label.height_px) / pixels_per_mm).floor() as u32;
    let mut number = core::fmt::NumBuffer::new();
    for mm in (0..=height_mm).step_by(5) {
        let y = (f64::from(mm) * pixels_per_mm).round() as u32;
        if y >= label.height_px {
            break;
        }
        let text = mm.format_into(&mut number);
        let w = font.text_width(text, size);
        if inset + w >= label.width_px - inset {
            break; // too narrow to letter without colliding
        }
        // Baseline sits just ABOVE its tick. Below the tick, the last numeral
        // falls past the printable band — and that is the reading that matters
        // most, so it is exactly the one you must not lose.
        let baseline = y.max(size as u32) as f32 - 1.0;
        font.draw_text(&mut img, inset as f32, baseline, text, size);
        font.draw_text(
            &mut img,
            (label.width_px - inset - w) as f32,
            baseline,
            text,
            size,
        );
    }
    Ok(img)
}

/// Millimetre range the boundary probe marks on `label`: the last
/// [`BOUNDARY_SPAN_MM`] millimetres, ending at the final drawable one.
///
/// Derived from the label rather than fixed. A hardcoded 17..29 drew three
/// bars on 40x20 media and probed the middle of a 50x80 label — nowhere near
/// the edge it exists to measure.
pub fn boundary_range(label: LabelPx, pixels_per_mm: f64) -> std::ops::RangeInclusive<u32> {
    let last = (f64::from(label.height_px) / pixels_per_mm)
        .floor()
        .max(1.0) as u32
        - 1;
    let first = last.saturating_sub(BOUNDARY_SPAN_MM - 1);
    first..=last
}

/// A staircase of numbered bars, one per millimetre, for finding exactly where
/// the printer stops.
///
/// The feed ruler on [`make_calibration_label`] puts its ticks 1 mm (8 px)
/// apart, which is too crowded to letter every one — so readings there are
/// "somewhere past 20", and estimating the rest from a photo is what produced
/// two contradictory measurements. Here each millimetre gets its own bar at its
/// own horizontal position, so the numbers never crowd: **the last bar you can
/// see is the answer**, with no counting and no scale estimation.
pub fn make_boundary_label(
    label: LabelPx,
    pixels_per_mm: f64,
    font_path: Option<&std::path::Path>,
) -> Result<GrayImage> {
    let (w, h) = (label.width_px, label.height_px);
    let mut img = GrayImage::from_pixel(w, h, Luma([255]));
    let font = load_font(font_path, None)?;

    let range = boundary_range(label, pixels_per_mm);
    let steps = range.end() - range.start() + 1;
    let slot = w / steps.max(1);
    let bar_w = slot.saturating_sub(4).max(1);
    let size = 12.0f32;

    for (i, mm) in range.enumerate() {
        let i = i as u32;
        let y = (f64::from(mm) * pixels_per_mm).round() as u32;
        let next_y = (f64::from(mm + 1) * pixels_per_mm).round() as u32;
        if next_y > h {
            break;
        }
        let x0 = i * slot;

        // Bar: 1 mm tall, so its own thickness cannot be mistaken for a
        // neighbour's.
        for yy in y..next_y.min(h) {
            for xx in x0..(x0 + bar_w).min(w) {
                img.put_pixel(xx, yy, Luma([0]));
            }
        }
        // Number sits above its bar. Horizontal separation is what keeps these
        // legible where a shared ruler column could not.
        let baseline = y as f32 - 2.0;
        if baseline > size {
            font.draw_text(&mut img, x0 as f32, baseline, &mm.to_string(), size);
        }
    }
    Ok(img)
}
