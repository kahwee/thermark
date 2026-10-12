use super::*;

/// Build a label: **square** QR + readable side text using a system TTF font.
pub fn make_qr_label_opts(opts: &QrLabelOptions) -> Result<GrayImage> {
    let font = load_font(opts.font_path.as_deref(), opts.font_name.as_deref())?;
    make_qr_label_with_font(opts, &font)
}

/// Render with a reusable, already loaded font.
pub fn make_qr_label_with_font(opts: &QrLabelOptions, font: &LabelFont) -> Result<GrayImage> {
    if opts.url.is_empty() {
        return Err(Error::qr("url must not be empty"));
    }
    let w = opts.label.width_px;
    let h = opts.label.height_px;
    let layout = qr_layout(opts.label, opts.safe).ok_or_else(|| {
        Error::qr(format!(
            "label {w}x{h}px is too small for a QR beside text \
             (need at least {QR_SIDE_MIN}px of QR after margins). \
             Use a wider label, or a model with a wider printhead."
        ))
    })?;

    let mut img = GrayImage::from_pixel(w, h, Luma([255]));

    let QrLayout {
        area,
        gap,
        text_col_w,
        qr_side,
    } = layout;

    let (qr_x, text_x) = match opts.text_side {
        TextSide::Right => (area.x, area.x + qr_side + gap),
        TextSide::Left => (area.x + text_col_w + gap, area.x),
    };
    // Centre within the printable band, not the raw canvas — otherwise the QR
    // drifts into the clipped bottom edge.
    let qr_y = area.y + (area.h.saturating_sub(qr_side)) / 2;

    // --- Square QR ---
    let qr_img = render_qr_square(&opts.url, qr_side)?;
    overlay_gray(&mut img, &qr_img, qr_x, qr_y);

    // --- Text ---
    draw_text_block(
        &mut img,
        font,
        &opts.side_text,
        Rect {
            x: text_x,
            y: area.y,
            w: text_col_w.saturating_sub(4),
            h: area.h,
        },
        TextAlign::Center,
        opts.font_size,
    )?;

    if opts.border {
        draw_border(&mut img);
    }

    Ok(img)
}

pub fn render_qr_square(url: &str, side: u32) -> Result<GrayImage> {
    let code = QrCode::new(url.as_bytes()).map_err(|e| Error::qr(format!("QR encode: {e}")))?;
    let colors = code.to_colors();
    let modules = code.width();
    let quiet = 2usize;
    let total = modules + quiet * 2;
    let mpx = side as usize / total;

    if mpx < QR_MIN_MODULE_PX {
        return Err(Error::qr(format!(
            "content needs {modules} QR modules, which is {mpx}px per module in {side}px — \
             too fine to scan once printed (need {QR_MIN_MODULE_PX}px). \
             Shorten the content, or use a larger label."
        )));
    }
    if mpx < QR_COMFORTABLE_MODULE_PX {
        tracing::warn!(
            module_px = mpx,
            modules,
            "QR modules are small; it should scan but shorten the content if it does not"
        );
    }
    let drawn = (total * mpx) as u32;
    let ox = (side.saturating_sub(drawn)) / 2;
    let oy = ox;

    let mut img = GrayImage::from_pixel(side, side, Luma([255]));
    for y in 0..modules {
        for x in 0..modules {
            if colors[y * modules + x] != qrcode::Color::Dark {
                continue;
            }
            let px = ox + ((x + quiet) * mpx) as u32;
            let py = oy + ((y + quiet) * mpx) as u32;
            for dy in 0..mpx as u32 {
                for dx in 0..mpx as u32 {
                    if px + dx < side && py + dy < side {
                        img.put_pixel(px + dx, py + dy, Luma([0]));
                    }
                }
            }
        }
    }
    Ok(img)
}
