use super::*;

/// Wrap, size, and draw `text` inside `bx`. Shared by every label type.
///
/// `font_size` of `None` auto-fits the largest size that keeps words whole.
/// Returns an error without changing the image if the size is invalid or
/// any text ink would fall outside `bx`.
pub fn draw_text_block(
    img: &mut GrayImage,
    font: &LabelFont,
    text: &str,
    bx: Rect,
    align: TextAlign,
    font_size: Option<f32>,
) -> Result<()> {
    if bx.w == 0
        || bx.h == 0
        || bx
            .x
            .checked_add(bx.w)
            .is_none_or(|right| right > img.width())
        || bx
            .y
            .checked_add(bx.h)
            .is_none_or(|bottom| bottom > img.height())
    {
        return Err(Error::msg("text box must fit inside the label canvas"));
    }
    font.validate_text(text)?;
    let mut px = match font_size {
        Some(s) => crate::font::validate_font_size(s)?.clamp(6.0, 96.0),
        None => font.fit_size(text, bx.w, bx.h),
    };
    let (lines, positions) = loop {
        let lines = font.wrap(text, bx.w, px);
        let line_h = font.text_height(px) as f32 + 2.0;
        let top_align = font_size.is_some() && px <= 16.0;

        // Place by measured ink, not by font metrics — see `block_ink_bounds`.
        let mut baseline = match font.block_ink_bounds(&lines, px, line_h) {
            Some((ink_top, ink_bottom)) => {
                let ink_h = ink_bottom - ink_top;
                let offset = if top_align {
                    0.0
                } else {
                    ((bx.h as f32 - ink_h) / 2.0).max(0.0)
                };
                bx.y as f32 + offset - ink_top
            }
            // Nothing to measure (blank text): fall back to metrics.
            None => bx.y as f32 + font.ascent(px) as f32,
        };

        let mut positions = Vec::with_capacity(lines.len());
        let mut fits = true;
        for line in &lines {
            let tw = font.text_width(line, px);
            let free = bx.w.saturating_sub(tw) as f32;
            let tx = bx.x as f32
                + match align {
                    TextAlign::Left => 0.0,
                    TextAlign::Center => free / 2.0,
                    TextAlign::Right => free,
                };
            if !font.text_ink_fits(line, px, tx, baseline, bx) {
                fits = false;
                break;
            }
            positions.push((tx, baseline));
            baseline += line_h;
        }
        if fits {
            break (lines, positions);
        }
        if font_size.is_none() && px > crate::font::MIN_FONT_PX {
            px = (px - 1.0).max(crate::font::MIN_FONT_PX);
            continue;
        }
        return Err(Error::msg(
            "text does not fit inside the label text area; shorten the text, use a larger label, or reduce/omit --font-size",
        ));
    };
    for (line, (tx, baseline)) in lines.iter().zip(positions) {
        font.draw_text(img, tx, baseline, line, px);
    }
    Ok(())
}

/// Render a text-only sticker: no QR, text auto-fitted to fill the label.
pub fn make_text_label(opts: &TextLabelOptions) -> Result<GrayImage> {
    let font = load_font(opts.font_path.as_deref(), opts.font_name.as_deref())?;
    make_text_label_with_font(opts, &font)
}

/// Render with a reusable, already loaded font.
pub fn make_text_label_with_font(opts: &TextLabelOptions, font: &LabelFont) -> Result<GrayImage> {
    if opts.text.trim().is_empty() {
        return Err(Error::qr("text must not be empty"));
    }
    let (w, h) = (opts.label.width_px, opts.label.height_px);
    let area = content_box(opts.label, opts.safe)
        .filter(|a| a.w >= MIN_TEXT_BOX_PX && a.h >= MIN_TEXT_BOX_PX)
        .ok_or_else(|| {
            Error::qr(format!(
                "label {w}x{h}px leaves no printable room for text after insets"
            ))
        })?;

    let mut img = GrayImage::from_pixel(w, h, Luma([255]));
    draw_text_block(&mut img, font, &opts.text, area, opts.align, opts.font_size)?;
    if opts.border {
        draw_border(&mut img);
    }
    Ok(img)
}
