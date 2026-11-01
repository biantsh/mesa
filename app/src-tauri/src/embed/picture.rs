use std::{cell::RefCell, path::Path};

use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer, images::Image};
use image::{DynamicImage, ImageDecoder, ImageReader};

use super::IMAGE_TOKENS;

const PATCH: u32 = 16;
const POOLING: u32 = 3;
const SIDE: u32 = PATCH * POOLING;

pub fn from_file(path: &Path) -> Result<Vec<u8>, String> {
    let mut decoder = ImageReader::open(path)
        .and_then(|reader| reader.with_guessed_format())
        .map_err(|e| format!("can't open the picture: {e}"))?
        .into_decoder()
        .map_err(|e| format!("can't read the picture: {e}"))?;
    let orientation = decoder.orientation().ok();
    let mut picture = DynamicImage::from_decoder(decoder).map_err(|e| format!("can't decode the picture: {e}"))?;
    if let Some(orientation) = orientation {
        picture.apply_orientation(orientation);
    }
    let rgb = picture.into_rgb8();
    let (width, height) = rgb.dimensions();
    from_rgb(rgb.into_raw(), width, height)
}

pub fn from_rgb(rgb: Vec<u8>, width: u32, height: u32) -> Result<Vec<u8>, String> {
    let (target_width, target_height) = target_size(width, height, IMAGE_TOKENS)
        .ok_or_else(|| format!("the picture is too thin to read ({width} x {height})"))?;
    let pixels = if (target_width, target_height) == (width, height) {
        rgb
    } else {
        resize(rgb, (width, height), (target_width, target_height))?
    };
    Ok(bitmap(target_width, target_height, &pixels))
}

fn target_size(width: u32, height: u32, tokens: u32) -> Option<(u32, u32)> {
    if width == 0 || height == 0 {
        return None;
    }
    let max_patches = f64::from(tokens * POOLING * POOLING);
    let factor = (max_patches * f64::from(PATCH * PATCH) / (f64::from(height) * f64::from(width))).sqrt();
    let round_down = |side: u32| ((factor * f64::from(side)) / f64::from(SIDE)).floor() as u32 * SIDE;
    let (mut target_height, mut target_width) = (round_down(height), round_down(width));
    if target_height == 0 && target_width == 0 {
        return None;
    }
    let longest = tokens * SIDE;
    if target_height == 0 {
        target_height = SIDE;
        target_width = (width / height * SIDE).min(longest);
    } else if target_width == 0 {
        target_width = SIDE;
        target_height = (height / width * SIDE).min(longest);
    }
    Some((target_width, target_height))
}

thread_local! {
    static RESIZER: RefCell<Resizer> = RefCell::new(Resizer::new());
}

fn resize(rgb: Vec<u8>, from: (u32, u32), to: (u32, u32)) -> Result<Vec<u8>, String> {
    let source = Image::from_vec_u8(from.0, from.1, rgb, PixelType::U8x3).map_err(|e| e.to_string())?;
    let mut target = Image::new(to.0, to.1, PixelType::U8x3);
    let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::CatmullRom));
    RESIZER
        .with(|resizer| resizer.borrow_mut().resize(&source, &mut target, &options))
        .map_err(|e| e.to_string())?;
    Ok(target.into_vec())
}

fn bitmap(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    let row = width as usize * 3;
    let padded = row.next_multiple_of(4);
    let pixels = padded * height as usize;
    let mut file = Vec::with_capacity(54 + pixels);
    file.extend_from_slice(b"BM");
    file.extend_from_slice(&((54 + pixels) as u32).to_le_bytes());
    file.extend_from_slice(&[0; 4]);
    file.extend_from_slice(&54u32.to_le_bytes());
    file.extend_from_slice(&40u32.to_le_bytes());
    file.extend_from_slice(&width.to_le_bytes());
    file.extend_from_slice(&height.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&24u16.to_le_bytes());
    file.extend_from_slice(&[0; 4]);
    file.extend_from_slice(&(pixels as u32).to_le_bytes());
    file.extend_from_slice(&[0; 16]);
    for line in rgb.chunks_exact(row).rev() {
        for pixel in line.chunks_exact(3) {
            file.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
        }
        file.resize(file.len() + (padded - row), 0);
    }
    file
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mesa-picture-{tag}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn odd_width_bitmaps() {
        let rgb = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
        let file = bitmap(3, 2, &rgb);
        assert_eq!(file.len(), 54 + 12 * 2);
        let decoded = image::load_from_memory(&file).unwrap().to_rgb8();
        assert_eq!(decoded.into_raw(), rgb);
    }

    fn with_orientation(jpeg: &[u8], orientation: u16) -> Vec<u8> {
        let mut segment = vec![0xFF, 0xE1, 0x00, 34];
        segment.extend_from_slice(b"Exif\0\0");
        segment.extend_from_slice(&[b'M', b'M', 0, 42, 0, 0, 0, 8]);
        segment.extend_from_slice(&[0, 1, 0x01, 0x12, 0, 3, 0, 0, 0, 1]);
        segment.extend_from_slice(&orientation.to_be_bytes());
        segment.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        [&jpeg[..2], &segment, &jpeg[2..]].concat()
    }

    #[test]
    fn applies_exif_orientation() {
        let dir = temp_dir("upright");
        let mut jpeg = Vec::new();
        image::RgbImage::new(300, 100)
            .write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
            .unwrap();
        let size_of = |name: &str, bytes: &[u8]| {
            let path = dir.join(name);
            fs::write(&path, bytes).unwrap();
            image::load_from_memory(&from_file(&path).unwrap())
                .unwrap()
                .to_rgb8()
                .dimensions()
        };
        let (width, height) = size_of("plain.jpg", &jpeg);
        assert!(width > height);
        let (width, height) = size_of("turned.jpg", &with_orientation(&jpeg, 6));
        assert!(
            height > width,
            "a picture marked as turned a quarter is tall: {width} x {height}"
        );
    }
}
