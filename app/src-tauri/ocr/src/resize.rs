use std::cell::RefCell;

use fast_image_resize::{
    FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer,
    images::{Image, ImageRef},
};

use super::geometry::Picture;

thread_local! {
    static RESIZER: RefCell<Resizer> = RefCell::new(Resizer::new());
}

fn resize(
    bytes: &[u8],
    from: (usize, usize),
    to: (usize, usize),
    pixel: PixelType,
    algorithm: ResizeAlg,
) -> Result<Vec<u8>, String> {
    if from == to {
        return Ok(bytes.to_vec());
    }
    let source = ImageRef::new(from.0 as u32, from.1 as u32, bytes, pixel).map_err(|e| e.to_string())?;
    let mut target = Image::new(to.0 as u32, to.1 as u32, pixel);
    let options = ResizeOptions::new().resize_alg(algorithm);
    RESIZER
        .with(|resizer| resizer.borrow_mut().resize(&source, &mut target, &options))
        .map_err(|e| e.to_string())?;
    Ok(target.into_vec())
}

pub fn resize_rgb(picture: Picture, width: usize, height: usize) -> Result<Vec<u8>, String> {
    resize(
        picture.rgb,
        (picture.width, picture.height),
        (width, height),
        PixelType::U8x3,
        ResizeAlg::Interpolation(FilterType::Bilinear),
    )
}

pub fn resize_gray(gray: &[u8], from: (usize, usize), to: (usize, usize)) -> Result<Vec<u8>, String> {
    let algorithm = if to.1 < from.1 {
        ResizeAlg::Convolution(FilterType::Box)
    } else {
        ResizeAlg::Interpolation(FilterType::Bilinear)
    };
    resize(gray, from, to, PixelType::U8, algorithm)
}

pub fn gray(picture: Picture) -> Vec<u8> {
    picture
        .rgb
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| ((u32::from(p[0]) * 4899 + u32::from(p[1]) * 9617 + u32::from(p[2]) * 1868 + 8192) >> 14) as u8)
        .collect()
}
