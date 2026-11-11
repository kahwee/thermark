//! Untrusted font, image and text input regression tests (no hardware).
use image::{DynamicImage, GrayImage, ImageFormat, RgbaImage};
use std::io::Cursor;
use thermark::font::{LabelFont, MAX_TEXT_BYTES, MIN_FONT_PX};
use thermark::image_encode::encode_path;
use thermark::printer::{PrintOptions, compose_for_label};

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(RgbaImage::new(width, height))
        .write_to(&mut bytes, ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = png(1, 1);
    bytes[16..20].copy_from_slice(&width.to_be_bytes());
    bytes[20..24].copy_from_slice(&height.to_be_bytes());
    let mut crc = !0u32;
    for byte in &bytes[12..29] {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    bytes[29..33].copy_from_slice(&(!crc).to_be_bytes());
    bytes
}

#[test]
fn both_image_entrypoints_reject_huge_headers_before_pixel_decode() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bomb.png");
    for (width, height) in [
        (16_385, 1),
        (1, 16_385),
        (16_384, 16_384),
        (u32::MAX, u32::MAX),
    ] {
        std::fs::write(&path, png_header(width, height)).unwrap();
        for error in [
            encode_path(&path, 384, 128, false).unwrap_err(),
            compose_for_label(&path, &PrintOptions::default(), 384, 8.0).unwrap_err(),
        ] {
            let message = error.to_string().to_lowercase();
            assert!(
                message.contains("limit"),
                "expected a resource limit for {width}x{height}, got {message}"
            );
        }
    }
}

#[test]
fn source_photos_can_exceed_printhead_and_dimension_boundary_is_inclusive() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.png");
    std::fs::write(&path, png(16_384, 1)).unwrap();
    let opts = PrintOptions {
        fit: true,
        trim: false,
        ..Default::default()
    };
    let img = compose_for_label(&path, &opts, 384, 8.0).unwrap();
    assert_eq!(img.width(), 384);
    std::fs::write(&path, png(128, 32)).unwrap();
    assert!(encode_path(&path, 384, 128, false).is_ok());
}

#[test]
fn oversized_media_files_are_rejected_before_parsing() {
    let file = tempfile::NamedTempFile::new().unwrap();
    file.as_file().set_len(64 * 1024 * 1024 + 1).unwrap();
    let font_error = LabelFont::load(file.path()).err().unwrap().to_string();
    assert!(font_error.contains("byte limit"));
    assert!(
        encode_path(file.path(), 384, 128, false)
            .unwrap_err()
            .to_string()
            .contains("byte limit")
    );
    assert!(
        compose_for_label(file.path(), &PrintOptions::default(), 384, 8.0)
            .unwrap_err()
            .to_string()
            .contains("byte limit")
    );
}

#[test]
fn text_limit_counts_bytes_and_bounds_public_layout_helpers() {
    let font = LabelFont::load_default().unwrap();
    assert!(font.validate_text(&"A".repeat(MAX_TEXT_BYTES)).is_ok());
    assert!(font.validate_text(&"é".repeat(MAX_TEXT_BYTES / 2)).is_ok());
    for text in [
        "A".repeat(MAX_TEXT_BYTES + 1),
        "é".repeat(MAX_TEXT_BYTES / 2 + 1),
        "\u{200d}".repeat(100_000),
    ] {
        assert!(
            font.validate_text(&text)
                .unwrap_err()
                .to_string()
                .contains("UTF-8 bytes")
        );
        assert_eq!(font.wrap(&text, 1, 24.0), vec![text.clone()]);
        assert!(!font.fits(&text, u32::MAX, u32::MAX, 24.0));
        assert_eq!(font.fit_size(&text, u32::MAX, u32::MAX), MIN_FONT_PX);
    }
}

#[test]
fn overlong_text_rejected_without_drawing() {
    let font = LabelFont::load_default().unwrap();
    let mut img = GrayImage::from_pixel(128, 64, image::Luma([255]));
    let before = img.clone();
    let result = thermark::label::draw_text_block(
        &mut img,
        &font,
        &"a".repeat(MAX_TEXT_BYTES + 1),
        thermark::geometry::Rect {
            x: 0,
            y: 0,
            w: 128,
            h: 64,
        },
        thermark::TextAlign::Left,
        None,
    );
    assert!(result.is_err());
    assert_eq!(img, before);
}

#[test]
fn ascii_hard_wrap_matches_prefix_measurement_and_preserves_text() {
    let font = LabelFont::load_default().unwrap();
    let text = "A_very-long.0123456789TOKEN/withPunctuation!";
    for size in [10.0, 24.0, 71.0] {
        for width in [0, 1, 15, 40, 80, 160] {
            let mut expected = Vec::new();
            let mut line = String::new();
            for ch in text.chars() {
                let trial = format!("{line}{ch}");
                if font.text_width(&trial, size) <= width {
                    line = trial;
                } else {
                    if !line.is_empty() {
                        expected.push(std::mem::take(&mut line));
                    }
                    line.push(ch);
                }
            }
            if !line.is_empty() {
                expected.push(line);
            }
            let actual = font.wrap(text, width, size);
            assert_eq!(actual, expected, "{width}px at {size}px");
            assert_eq!(actual.concat(), text);
        }
    }
}

#[test]
fn font_collections_still_load_nonzero_face_indices() {
    let ttf = include_bytes!("../assets/fonts/DejaVuSans.ttf");
    let mut collection = Vec::from(*b"ttcf");
    collection.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    collection.extend_from_slice(&2u32.to_be_bytes());
    collection.extend_from_slice(&20u32.to_be_bytes());
    collection.extend_from_slice(&(20 + ttf.len() as u32).to_be_bytes());
    for _ in 0..2 {
        let base = collection.len() as u32;
        let mut face = ttf.to_vec();
        let tables = u16::from_be_bytes([face[4], face[5]]) as usize;
        for table in 0..tables {
            let start = 12 + table * 16 + 8;
            let offset = u32::from_be_bytes(face[start..start + 4].try_into().unwrap());
            face[start..start + 4].copy_from_slice(&(offset + base).to_be_bytes());
        }
        collection.extend_from_slice(&face);
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("demo.ttc");
    std::fs::write(&path, collection).unwrap();
    let font = LabelFont::load_index(&path, 1).unwrap();
    assert_eq!(font.index(), 1);
    assert!(font.text_width("Collection", 24.0) > 0);
}

fn jpeg() -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(image::RgbImage::new(128, 32))
        .write_to(&mut bytes, ImageFormat::Jpeg)
        .unwrap();
    bytes.into_inner()
}

#[test]
fn jpeg_decode_and_oversized_sof_headers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg");
    let original = jpeg();
    std::fs::write(&path, &original).unwrap();
    assert!(encode_path(&path, 384, 128, false).is_ok());
    let opts = PrintOptions {
        trim: false,
        ..Default::default()
    };
    assert_eq!(
        compose_for_label(&path, &opts, 384, 8.0).unwrap().width(),
        128
    );
    // SOF0: marker, two-byte length, precision, height, width. Alter only
    // dimensions in a real encoder-produced JPEG, leaving the scan intact.
    let sof = original
        .windows(2)
        .position(|bytes| bytes == [0xff, 0xc0])
        .unwrap();
    for (width, height) in [
        (16_385u16, 1u16),
        (1, 16_385),
        (16_384, 16_384),
        (65_535, 65_535),
    ] {
        let mut bomb = original.clone();
        bomb[sof + 5..sof + 7].copy_from_slice(&height.to_be_bytes());
        bomb[sof + 7..sof + 9].copy_from_slice(&width.to_be_bytes());
        std::fs::write(&path, bomb).unwrap();
        for error in [
            encode_path(&path, 384, 128, false).unwrap_err(),
            compose_for_label(&path, &opts, 384, 8.0).unwrap_err(),
        ] {
            let message = error.to_string().to_lowercase();
            assert!(message.contains("limit"), "{width}x{height}: {message}");
        }
    }
}

#[test]
fn within_byte_limit_zero_advance_and_complex_tokens_are_bounded() {
    let font = LabelFont::load_default().unwrap();
    let simple = format!("{}{}", "\u{200b}".repeat(1200), "A".repeat(300));
    assert!(simple.len() <= MAX_TEXT_BYTES);
    let start = std::time::Instant::now();
    assert_eq!(font.try_wrap(&simple, 128, 24.0).unwrap().concat(), simple);
    let _ = font.try_fit_size(&simple, 128, 64);
    assert!(start.elapsed() < std::time::Duration::from_secs(5));

    // A combining mark forces complex shaping, and the long zero-width prefix
    // makes repeated prefix measurement expensive despite the small byte count.
    let complex = format!("{}a\u{0301}{}", "\u{200b}".repeat(1000), "Ж".repeat(100));
    assert!(complex.len() <= MAX_TEXT_BYTES);
    let start = std::time::Instant::now();
    let error = font.try_fit_size(&complex, 128, 64).unwrap_err();
    assert!(error.to_string().contains("shaping work limit"));
    // The infallible compatibility helper retains the complete input on error.
    assert_eq!(font.wrap(&complex, 128, 24.0), vec![complex.clone()]);
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
fn pathological_text_cli_finishes_without_printing() {
    let dir = tempfile::tempdir().unwrap();
    for text in [
        format!("{}{}", "\u{200b}".repeat(1200), "A".repeat(300)),
        format!("{}a\u{0301}{}", "\u{200b}".repeat(1000), "Ж".repeat(100)),
    ] {
        let mut cmd = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("thermark"));
        cmd.env("THERMARK_CONFIG", dir.path().join("config.toml"))
            .env_remove("THERMARK_ADDR")
            .args(["text", "--text", &text, "--label", "50x30", "--no-print"])
            .timeout(std::time::Duration::from_secs(5));
        let output = cmd.output().unwrap();
        assert!(
            output.status.code().is_some(),
            "CLI timed out or was killed"
        );
    }
}
