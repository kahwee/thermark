//! Reproducible microbenchmarks for the CPU-only image pipeline.
//!
//! Run with `cargo bench --bench image_pipeline`.
//! Owned-image cases include the fresh input clone needed to repeat a consuming
//! API call; the borrowed case isolates encoding when the renderer retains its
//! grayscale page. Measure RSS cases in separate processes, not from this suite.

use image::{DynamicImage, GrayImage, Luma, Rgb, RgbImage};
use std::hint::black_box;
use std::time::{Duration, Instant};
use thermark::geometry::{LabelPx, SafeArea};
use thermark::image_encode::{encode, encode_gray, fill_label, trim_white};

fn patterned(width: u32, height: u32) -> GrayImage {
    GrayImage::from_fn(width, height, |x, y| {
        let value = ((x.wrapping_mul(37) ^ y.wrapping_mul(73)) & 0xff) as u8;
        Luma([value])
    })
}

fn median_time<T>(iterations: usize, mut operation: impl FnMut() -> T) -> Duration {
    black_box(operation());
    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let started = Instant::now();
        black_box(operation());
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn report<T>(name: &str, iterations: usize, operation: impl FnMut() -> T) {
    let elapsed = median_time(iterations, operation);
    println!("{name:<30} {:>10.3} ms", elapsed.as_secs_f64() * 1_000.0);
}

fn main() {
    let page = patterned(384, 640);
    report("encode owned+clone hard", 100, || {
        black_box(encode(
            DynamicImage::ImageLuma8(page.clone()),
            384,
            127,
            false,
        ))
    });
    report("encode owned+clone dither", 40, || {
        black_box(encode(
            DynamicImage::ImageLuma8(page.clone()),
            384,
            127,
            true,
        ))
    });
    report("encode borrowed hard", 100, || {
        black_box(encode_gray(&page, 384, 127, false))
    });

    let tall = DynamicImage::ImageLuma8(patterned(100, 4_000));
    let label = LabelPx {
        width_px: 384,
        height_px: 240,
    };
    report("cover fill owned+clone", 20, || {
        black_box(fill_label(tall.clone(), label, SafeArea::B1, 0).unwrap())
    });

    let bordered = DynamicImage::ImageLuma8(GrayImage::from_fn(2_000, 1_500, |x, y| {
        if (100..1_900).contains(&x) && (100..1_400).contains(&y) {
            Luma([0])
        } else {
            Luma([255])
        }
    }));
    report("trim white owned+clone", 20, || {
        black_box(trim_white(bordered.clone(), 127))
    });

    let rgb_bordered = DynamicImage::ImageRgb8(RgbImage::from_fn(2_000, 1_500, |x, y| {
        if (100..1_900).contains(&x) && (100..1_400).contains(&y) {
            Rgb([
                ((x * 17 + y * 3) & 0xff) as u8,
                ((x * 5 + y * 29) & 0xff) as u8,
                ((x * 11 + y * 7) & 0xff) as u8,
            ])
        } else {
            Rgb([255, 255, 255])
        }
    }));
    report("trim RGB8 owned+clone", 20, || {
        black_box(trim_white(rgb_bordered.clone(), 127))
    });
}
