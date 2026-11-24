use std::ffi::c_void;

use objc2_core_foundation::CGRect;
use objc2_core_graphics::{CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo};

pub fn pixels(image: &CGImage) -> Option<(Vec<u8>, u32, u32)> {
    let (width, height) = (CGImage::width(Some(image)), CGImage::height(Some(image)));
    if width == 0 || height == 0 || width > u32::MAX as usize || height > u32::MAX as usize {
        return None;
    }
    let mut rgba = vec![0u8; width.checked_mul(height)?.checked_mul(4)?];
    let space = CGColorSpace::new_device_rgb()?;
    // SAFETY: the buffer is width * height * 4 bytes and outlives the context.
    let context = unsafe {
        CGBitmapContextCreate(
            rgba.as_mut_ptr().cast::<c_void>(),
            width,
            height,
            8,
            width * 4,
            Some(&space),
            CGImageAlphaInfo::PremultipliedLast.0,
        )
    }?;
    CGContext::draw_image(
        Some(&context),
        CGRect {
            origin: objc2_core_foundation::CGPoint { x: 0.0, y: 0.0 },
            size: objc2_core_foundation::CGSize {
                width: width as f64,
                height: height as f64,
            },
        },
        Some(image),
    );
    drop(context);
    unpremultiply(&mut rgba);
    Some((rgba, width as u32, height as u32))
}

fn unpremultiply(rgba: &mut [u8]) {
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let alpha = u32::from(pixel[3]);
        if alpha != 0 && alpha != 255 {
            for colour in &mut pixel[..3] {
                *colour = ((u32::from(*colour) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
}
