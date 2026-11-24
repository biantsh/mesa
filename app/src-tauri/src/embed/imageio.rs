use std::path::Path;

use objc2_core_foundation::{CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType, CFURL};
use objc2_image_io::{
    CGImageSource, kCGImageSourceCreateThumbnailFromImageAlways, kCGImageSourceCreateThumbnailWithTransform,
    kCGImageSourceThumbnailMaxPixelSize,
};

use crate::cgimage;

const LONGEST_SIDE: i64 = 8192;

pub fn decode(path: &Path) -> Result<(Vec<u8>, u32, u32), String> {
    let url = CFURL::from_file_path(path).ok_or("the picture's name can't be used")?;
    let image = unsafe {
        let source = CGImageSource::with_url(&url, None).ok_or("macOS can't read this picture")?;
        if source.count() == 0 {
            return Err("macOS can't read this picture (there is nothing in it)".into());
        }
        let yes = CFBoolean::new(true);
        let longest = CFNumber::new_i64(LONGEST_SIDE);
        let options: CFRetained<CFDictionary<CFString, CFType>> = CFDictionary::from_slices(
            &[
                kCGImageSourceCreateThumbnailFromImageAlways,
                kCGImageSourceCreateThumbnailWithTransform,
                kCGImageSourceThumbnailMaxPixelSize,
            ],
            &[yes.as_ref(), yes.as_ref(), longest.as_ref()],
        );
        source
            .thumbnail_at_index(0, Some(options.as_opaque()))
            .ok_or("macOS can't decode this picture")?
    };
    let (rgba, width, height) = cgimage::pixels(&image).ok_or("the picture is empty")?;
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    for pixel in rgba.as_chunks::<4>().0 {
        rgb.extend_from_slice(&pixel[..3]);
    }
    Ok((rgb, width, height))
}
