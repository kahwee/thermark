use super::monochrome::burns_at_threshold;
use super::placement::{crop_to_bounds, luma_bounds_by};
use super::raster::MAX_BITMAP_WIDTH_PX;
use super::raster::push_row_run;
use super::*;
use crate::errors::Error;
use crate::geometry::LabelMm;
use crate::geometry::{LabelPx, Rect, SafeArea};
use crate::packet::MAX_DATA_LEN;
use crate::packet::Packet;
use crate::protocol;
use crate::protocol::Cmd;
use image::{DynamicImage, GenericImageView, GrayImage, Luma, RgbaImage, imageops};

#[test]
fn encode_respects_max_width() {
    let img = DynamicImage::ImageLuma8(GrayImage::from_pixel(100, 50, Luma([0])));
    let r = encode(img, 384, 127, false).unwrap();
    let (w, h, pkts) = (r.width(), r.height(), r.rows());
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
fn fit_width_preserves_grayscale_storage() {
    let image = DynamicImage::ImageLuma8(GrayImage::from_fn(800, 200, |x, y| {
        Luma([((x * 19 + y * 43) & 0xff) as u8])
    }));
    let expected = imageops::resize(&image, 400, 100, imageops::FilterType::Triangle);
    let resized = fit_width(image, 400);

    assert_eq!(resized.dimensions(), (400, 100));
    assert_eq!(resized.to_rgba8(), expected);
    assert!(
        matches!(resized, DynamicImage::ImageLuma8(_)),
        "grayscale input should not expand to four-byte RGBA pixels"
    );
}

#[test]
fn fit_width_preserves_legacy_high_precision_conversion() {
    let image = DynamicImage::ImageLuma16(image::ImageBuffer::from_fn(800, 2, |x, _| {
        image::Luma([((x * 83) & 0xffff) as u16])
    }));
    let expected = DynamicImage::ImageRgba8(imageops::resize(
        &image,
        400,
        1,
        imageops::FilterType::Triangle,
    ));

    assert_eq!(fit_width(image, 400), expected);
}

#[test]
fn fit_width_preserves_legacy_alpha_resampling() {
    let image = DynamicImage::ImageRgba8(image::RgbaImage::from_fn(800, 4, |x, y| {
        image::Rgba([
            ((x * 7) & 0xff) as u8,
            ((y * 61) & 0xff) as u8,
            ((x + y * 13) & 0xff) as u8,
            ((x * 29 + y) & 0xff) as u8,
        ])
    }));
    let expected = DynamicImage::ImageRgba8(imageops::resize(
        &image,
        400,
        2,
        imageops::FilterType::Triangle,
    ));

    assert_eq!(fit_width(image, 400), expected);
}

#[test]
fn transparent_black_is_white_for_encoding_and_trimming() {
    let transparent =
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(8, 2, image::Rgba([0, 0, 0, 0])));
    let gray = grayscale_on_white(transparent.clone());
    assert!(gray.pixels().all(|pixel| pixel[0] == 255));

    let raster = encode(transparent, 384, 127, false).unwrap();
    assert_eq!(raster.rows().len(), 1);
    assert_eq!(raster.rows()[0].cmd, Cmd::PrintEmptyRow as u8);
    assert_eq!(raster.rows()[0].data[2], 2);

    let mut rgba = RgbaImage::from_pixel(7, 5, image::Rgba([0, 0, 0, 0]));
    rgba.put_pixel(3, 2, image::Rgba([0, 0, 0, 255]));
    let trimmed = trim_white(DynamicImage::ImageRgba8(rgba), 127);
    assert_eq!(trimmed.dimensions(), (1, 1));
    assert_eq!(trimmed.get_pixel(0, 0), image::Rgba([0, 0, 0, 255]));
}

#[test]
fn semi_transparent_black_uses_the_same_threshold_for_encode_and_trim() {
    let mut rgba = RgbaImage::from_pixel(8, 1, image::Rgba([0, 0, 0, 0]));
    rgba.put_pixel(1, 0, image::Rgba([0, 0, 0, 127]));
    rgba.put_pixel(2, 0, image::Rgba([0, 0, 0, 128]));
    let image = DynamicImage::ImageRgba8(rgba);

    // Over white, alpha 127 rounds to luma 128 (no burn), while alpha 128
    // is luma 127 (burn) at the default threshold.
    let raster = encode(image.clone(), 384, 127, false).unwrap();
    assert_eq!(raster.rows().len(), 1);
    assert_eq!(raster.rows()[0].cmd, Cmd::PrintBitmapRow as u8);
    assert_eq!(raster.rows()[0].data[6], 0b0010_0000);

    let trimmed = trim_white(image, 127);
    assert_eq!(trimmed.dimensions(), (1, 1));
    assert_eq!(trimmed.get_pixel(0, 0), image::Rgba([0, 0, 0, 128]));
}

#[test]
fn opaque_rgba_keeps_the_image_crate_luminance_at_the_burn_boundary() {
    let mut rgba = RgbaImage::from_pixel(3, 1, image::Rgba([255, 255, 255, 255]));
    rgba.put_pixel(1, 0, image::Rgba([0, 153, 251, 255]));
    let image = DynamicImage::ImageRgba8(rgba);

    let expected = image.clone().into_luma8();
    assert_eq!(expected.as_raw(), &[255, 128, 255]);
    assert_eq!(grayscale_on_white(image.clone()), expected);

    // Luma 128 does not burn at threshold 127, so print-aware trimming
    // must treat this as a blank image and leave its dimensions intact.
    assert_eq!(trim_for_print(image, 127, false).dimensions(), (3, 1));
}

#[test]
fn print_trim_uses_darkness_threshold_without_changing_public_trim_semantics() {
    let gray = GrayImage::from_raw(7, 1, vec![255, 204, 205, 0, 205, 204, 255]).unwrap();
    let image = DynamicImage::ImageLuma8(gray);

    // Public trim_white keeps its established direct-luma `<= threshold`
    // contract, which selects only the central black pixel here.
    let public = trim_white(image.clone(), 50).into_luma8();
    assert_eq!(public.dimensions(), (1, 1));
    assert_eq!(public.as_raw(), &[0]);

    // Print threshold 50 burns darkness > 50: luma 204 burns while the
    // exact luma-205 boundary does not. Both outer burned pixels survive.
    let print = trim_for_print(image, 50, false).into_luma8();
    assert_eq!(print.dimensions(), (5, 1));
    assert_eq!(print.as_raw(), &[204, 205, 0, 205, 204]);
    assert_eq!(
        render_print_preview(&print, 384, 50, false)
            .unwrap()
            .as_raw(),
        &[0, 255, 0, 255, 0]
    );
}

#[test]
fn print_trim_composites_alpha_before_applying_a_non_default_threshold() {
    let mut rgba = RgbaImage::from_pixel(7, 70, image::Rgba([0, 0, 0, 0]));
    rgba.put_pixel(1, 65, image::Rgba([0, 0, 0, 50]));
    rgba.put_pixel(2, 65, image::Rgba([0, 0, 0, 51]));
    rgba.put_pixel(4, 65, image::Rgba([0, 0, 0, 51]));
    rgba.put_pixel(5, 65, image::Rgba([0, 0, 0, 50]));

    // Alpha 50 composites to luma 205 (no burn), while alpha 51 becomes
    // luma 204 (burn). The y coordinate also exercises a second stripe.
    let trimmed = trim_for_print(DynamicImage::ImageRgba8(rgba), 50, false);
    assert_eq!(trimmed.dimensions(), (3, 1));
    assert_eq!(grayscale_on_white(trimmed).as_raw(), &[204, 255, 204]);
}

#[test]
fn dither_trim_keeps_full_width_and_preserves_retained_burn_bits() {
    // This exact pair of rows is a counterexample to horizontal dither
    // trimming: error carried by the white columns changes whether a
    // retained content pixel burns after cropping to columns 2..4.
    let content = GrayImage::from_raw(
        6,
        2,
        vec![255, 255, 32, 32, 255, 255, 255, 255, 192, 128, 255, 255],
    )
    .unwrap();
    let expected = render_print_preview(&content, 384, 128, true).unwrap();
    let horizontally_cropped = imageops::crop_imm(&content, 2, 0, 2, 2).to_image();
    let cropped_bits = render_print_preview(&horizontally_cropped, 384, 128, true).unwrap();
    let expected_content_bits = imageops::crop_imm(&expected, 2, 0, 2, 2).to_image();
    assert_ne!(cropped_bits, expected_content_bits);

    // Leading white rows start with no error and trailing rows cannot
    // influence earlier pixels, so those vertical edges remain safe.
    let mut padded = GrayImage::from_pixel(6, 4, Luma([255]));
    imageops::replace(&mut padded, &content, 0, 1);
    let padded_bits = render_print_preview(&padded, 384, 128, true).unwrap();
    let expected_retained_bits = imageops::crop_imm(&padded_bits, 0, 1, 6, 2).to_image();

    let trimmed = trim_for_print(DynamicImage::ImageLuma8(padded), 128, true).into_luma8();
    assert_eq!(trimmed, content);
    assert_eq!(trimmed.dimensions(), (6, 2));
    assert_eq!(
        render_print_preview(&trimmed, 384, 128, true).unwrap(),
        expected_retained_bits
    );
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
    assert_eq!(raster.rows().len(), 3);
    let starts_and_repeats: Vec<_> = raster
        .rows()
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
    assert_eq!(raster.rows().len(), 4);
    assert!(raster.rows().iter().all(|packet| match packet.cmd {
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
                    encode_gray(&gray, 384, threshold, dither).unwrap().rows(),
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
    let out = fill_label(src, lp, SafeArea::NONE, 0).unwrap();
    assert_eq!(out.dimensions(), (lp.width_px, lp.height_px));
}

#[test]
fn label_placement_rejects_invalid_label_and_safe_area_combinations() {
    let src = DynamicImage::ImageLuma8(GrayImage::from_pixel(1, 1, Luma([0])));
    let invalid = [
        (
            LabelPx {
                width_px: 10,
                height_px: 8,
            },
            SafeArea {
                top: 0,
                bottom: 0,
                left: 5,
                right: 5,
            },
        ),
        (
            LabelPx {
                width_px: 0,
                height_px: 8,
            },
            SafeArea::NONE,
        ),
    ];

    for (label, safe) in invalid {
        for result in [
            fill_label(src.clone(), label, safe, 0),
            contain_label(src.clone(), label, safe, 0),
        ] {
            assert!(
                matches!(result, Err(Error::InvalidLabel(ref message)) if message.contains("leaves no content")),
                "unexpected placement result: {result:?}"
            );
        }
    }
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
    let output = fill_label(DynamicImage::ImageLuma8(source), label, safe, 1)
        .unwrap()
        .to_luma8();

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
    let output = fill_label(DynamicImage::ImageLuma8(source), label, SafeArea::NONE, 0)
        .unwrap()
        .to_luma8();

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
    let output = fill_label(DynamicImage::ImageLuma8(source), label, SafeArea::NONE, 0)
        .unwrap()
        .to_luma8();

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
    let out = contain_label(src, lp, SafeArea::NONE, 0)
        .unwrap()
        .to_luma8();
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
    let out = contain_label(src, lp, SafeArea::NONE, margin)
        .unwrap()
        .to_luma8();
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
        fill_label(src.clone(), lp, safe, 0).unwrap(),
        contain_label(src, lp, safe, 0).unwrap(),
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

fn trim_from_reference_gray(
    source: &DynamicImage,
    gray: &GrayImage,
    predicate: impl FnMut(u8) -> bool,
) -> DynamicImage {
    let bounds = luma_bounds_by(
        gray.enumerate_pixels()
            .map(|(x, y, pixel)| (x, y, pixel[0])),
        predicate,
    );
    crop_to_bounds(source.clone(), bounds)
}

#[test]
fn striped_rgb8_trim_matches_full_conversion_at_every_threshold() {
    let mut rgb = image::RgbImage::from_fn(23, 139, |x, y| {
        if !(2..21).contains(&x) || !(3..136).contains(&y) {
            image::Rgb([255, 255, 255])
        } else {
            image::Rgb([
                ((x * 17 + y * 3) & 0xff) as u8,
                ((x * 5 + y * 29) & 0xff) as u8,
                ((x * 11 + y * 7) & 0xff) as u8,
            ])
        }
    });
    rgb.set_color_space(image::metadata::Cicp::DISPLAY_P3)
        .unwrap();
    let image = DynamicImage::ImageRgb8(rgb);
    let full_page_gray = image.to_luma8();

    // Make this sensitive to accidentally losing the source color space
    // while splitting it at the 64-row stripe boundary.
    let mut srgb = image.as_rgb8().unwrap().clone();
    srgb.set_color_space(image::metadata::Cicp::SRGB).unwrap();
    assert_ne!(full_page_gray, DynamicImage::ImageRgb8(srgb).to_luma8());

    for threshold in 0..=u8::MAX {
        let expected = trim_from_reference_gray(&image, &full_page_gray, |luma| luma <= threshold);
        assert_eq!(
            trim_white(image.clone(), threshold),
            expected,
            "public trim differs at threshold {threshold}"
        );

        let expected = trim_from_reference_gray(&image, &full_page_gray, |luma| {
            burns_at_threshold(luma, threshold)
        });
        assert_eq!(
            trim_for_print(image.clone(), threshold, false),
            expected,
            "print trim differs at threshold {threshold}"
        );
    }
}

#[test]
fn high_precision_and_alpha_trim_still_match_full_conversion() {
    let rgb16 = image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::from_fn(17, 73, |x, y| {
        image::Rgb([
            ((x * 4_099 + y * 307) & 0xffff) as u16,
            ((x * 977 + y * 8_191) & 0xffff) as u16,
            ((x * 65_521 + y * 31) & 0xffff) as u16,
        ])
    });
    let high_precision = DynamicImage::ImageRgb16(rgb16);
    let high_precision_gray = high_precision.to_luma8();

    let rgba = RgbaImage::from_fn(19, 137, |x, y| {
        image::Rgba([
            ((x * 47 + y * 13) & 0xff) as u8,
            ((x * 7 + y * 53) & 0xff) as u8,
            ((x * 31 + y * 19) & 0xff) as u8,
            ((x * 61 + y * 23) & 0xff) as u8,
        ])
    });
    let alpha = DynamicImage::ImageRgba8(rgba);
    let alpha_gray = grayscale_on_white(alpha.clone());

    for threshold in 0..=u8::MAX {
        assert_eq!(
            trim_white(high_precision.clone(), threshold),
            trim_from_reference_gray(&high_precision, &high_precision_gray, |luma| {
                luma <= threshold
            }),
            "RGB16 trim differs at threshold {threshold}"
        );
        assert_eq!(
            trim_white(alpha.clone(), threshold),
            trim_from_reference_gray(&alpha, &alpha_gray, |luma| luma <= threshold),
            "RGBA8 trim differs at threshold {threshold}"
        );
    }
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
    let placed = contain_label(art, lp, safe, 0).unwrap().to_luma8();

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
    let g = fill_label(src, lp, SafeArea::NONE, 0).unwrap().to_luma8();
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
    let raster = encode(dark, 384, 127, false).unwrap();
    let rows = raster.rows();
    assert!(rows.iter().all(|p| p.cmd == Cmd::PrintBitmapRow as u8));
}

#[test]
fn white_source_pixels_become_empty_rows() {
    // The complement: white source burns nothing, so rows are sent as empty.
    let light = DynamicImage::ImageLuma8(GrayImage::from_pixel(16, 2, Luma([255])));
    let raster = encode(light, 384, 127, false).unwrap();
    let rows = raster.rows();
    assert!(rows.iter().all(|p| p.cmd == Cmd::PrintEmptyRow as u8));
}
