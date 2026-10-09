//! White-background alpha compositing.

use image::{DynamicImage, GrayImage};

/// Convert an image to grayscale after flattening transparency onto white.
///
/// Thermal media has a white background. Ignoring alpha makes a transparent
/// black PNG look solid black, so both encoding and trimming route alpha
/// pixels through the same compositing rule.
pub fn grayscale_on_white(img: DynamicImage) -> GrayImage {
    if !img.has_alpha() {
        return img.into_luma8();
    }

    flatten_alpha_on_white(img).into_luma8()
}

/// Composite alpha-bearing channels over white in their native precision.
///
/// Luminance conversion happens afterwards through `DynamicImage::into_luma8`.
/// Keeping that order preserves the image crate's color conversion and makes
/// an opaque RGBA pixel identical to the corresponding RGB pixel.
fn flatten_alpha_on_white(img: DynamicImage) -> DynamicImage {
    match img {
        DynamicImage::ImageLumaA8(mut pixels) => {
            for pixel in pixels.pixels_mut() {
                pixel[0] = composite_u8_on_white(pixel[0], pixel[1]);
                pixel[1] = u8::MAX;
            }
            DynamicImage::ImageLumaA8(pixels)
        }
        DynamicImage::ImageRgba8(mut pixels) => {
            for pixel in pixels.pixels_mut() {
                let alpha = pixel[3];
                for channel in &mut pixel.0[..3] {
                    *channel = composite_u8_on_white(*channel, alpha);
                }
                pixel[3] = u8::MAX;
            }
            DynamicImage::ImageRgba8(pixels)
        }
        DynamicImage::ImageLumaA16(mut pixels) => {
            for pixel in pixels.pixels_mut() {
                pixel[0] = composite_u16_on_white(pixel[0], pixel[1]);
                pixel[1] = u16::MAX;
            }
            DynamicImage::ImageLumaA16(pixels)
        }
        DynamicImage::ImageRgba16(mut pixels) => {
            for pixel in pixels.pixels_mut() {
                let alpha = pixel[3];
                for channel in &mut pixel.0[..3] {
                    *channel = composite_u16_on_white(*channel, alpha);
                }
                pixel[3] = u16::MAX;
            }
            DynamicImage::ImageRgba16(pixels)
        }
        DynamicImage::ImageRgba32F(mut pixels) => {
            for pixel in pixels.pixels_mut() {
                let alpha = pixel[3];
                for channel in &mut pixel.0[..3] {
                    *channel = *channel * alpha + (1.0 - alpha);
                }
                pixel[3] = 1.0;
            }
            DynamicImage::ImageRgba32F(pixels)
        }
        image => image,
    }
}

fn composite_u8_on_white(channel: u8, alpha: u8) -> u8 {
    let alpha = u16::from(alpha);
    ((u16::from(channel) * alpha + u16::from(u8::MAX) * (u16::from(u8::MAX) - alpha) + 127)
        / u16::from(u8::MAX)) as u8
}

fn composite_u16_on_white(channel: u16, alpha: u16) -> u16 {
    let alpha = u64::from(alpha);
    ((u64::from(channel) * alpha
        + u64::from(u16::MAX) * (u64::from(u16::MAX) - alpha)
        + u64::from(u16::MAX) / 2)
        / u64::from(u16::MAX)) as u16
}
