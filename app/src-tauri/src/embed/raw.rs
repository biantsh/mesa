use std::{fs::File, io::Read, path::Path};

use image::{DynamicImage, ImageDecoder, codecs::jpeg::JpegDecoder, metadata::Orientation};

const MAX_BYTES: u64 = 400 * 1024 * 1024;
const SMALLEST: u32 = 320;
pub const EXTENSIONS: [&str; 13] = [
    "dng", "cr2", "cr3", "nef", "nrw", "arw", "srf", "sr2", "orf", "rw2", "pef", "raf", "srw",
];

pub fn decode(path: &Path) -> Result<(Vec<u8>, u32, u32), String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_BYTES).read_to_end(&mut bytes))
        .map_err(|e| format!("can't read the file: {e}"))?;
    if let Some(picture) = preview(&bytes) {
        return Ok(picture);
    }
    #[cfg(target_os = "macos")]
    return super::imageio::decode(path);
    #[cfg(windows)]
    return super::wic::decode(path);
    #[cfg(not(any(target_os = "macos", windows)))]
    Err("there is no picture in it to read, and this system can't develop a RAW file".into())
}

pub fn preview(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let mut found = jpegs(bytes);
    found.retain(|jpeg| jpeg.width.max(jpeg.height) >= SMALLEST);
    found.sort_by_key(|jpeg| std::cmp::Reverse(u64::from(jpeg.width) * u64::from(jpeg.height)));
    let raw_orientation = orientation_of(bytes);
    for jpeg in found {
        let Ok(mut decoder) = JpegDecoder::new(std::io::Cursor::new(&bytes[jpeg.start..jpeg.end])) else {
            continue;
        };
        let own = decoder
            .orientation()
            .ok()
            .filter(|orientation| *orientation != Orientation::NoTransforms);
        let Ok(mut picture) = DynamicImage::from_decoder(decoder) else {
            continue;
        };
        if let Some(orientation) = own.or(raw_orientation) {
            picture.apply_orientation(orientation);
        }
        let rgb = picture.into_rgb8();
        let (width, height) = rgb.dimensions();
        return Some((rgb.into_raw(), width, height));
    }
    None
}

#[derive(Debug, PartialEq, Eq)]
struct Jpeg {
    start: usize,
    end: usize,
    width: u32,
    height: u32,
}

fn jpegs(bytes: &[u8]) -> Vec<Jpeg> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(offset) = bytes[at..].windows(3).position(|window| window == [0xFF, 0xD8, 0xFF]) {
        let start = at + offset;
        match jpeg_at(bytes, start) {
            Some(jpeg) => {
                at = jpeg.end;
                found.push(jpeg);
            }
            None => at = start + 2,
        }
    }
    found
}

fn jpeg_at(bytes: &[u8], start: usize) -> Option<Jpeg> {
    let be16 =
        |at: usize| -> Option<usize> { Some(usize::from(*bytes.get(at)?) << 8 | usize::from(*bytes.get(at + 1)?)) };
    let mut at = start + 2;
    let mut size = None;
    for _ in 0..10_000 {
        while bytes.get(at) == Some(&0xFF) && bytes.get(at + 1) == Some(&0xFF) {
            at += 1;
        }
        if *bytes.get(at)? != 0xFF {
            return None;
        }
        let marker = *bytes.get(at + 1)?;
        match marker {
            0xD9 => {
                let (width, height) = size?;
                return Some(Jpeg {
                    start,
                    end: at + 2,
                    width,
                    height,
                });
            }
            0xD0..=0xD7 | 0x01 => at += 2,
            0x00 => return None,
            _ => {
                let length = be16(at + 2)?;
                if length < 2 {
                    return None;
                }
                match marker {
                    0xC0 | 0xC1 | 0xC2 | 0xC9 | 0xCA => {
                        let (height, width) = (be16(at + 5)?, be16(at + 7)?);
                        if width == 0 || height == 0 {
                            return None;
                        }
                        size = Some((width as u32, height as u32));
                    }
                    0xC3 | 0xC5..=0xC7 | 0xCB | 0xCD..=0xCF => return None,
                    _ => {}
                }
                at += 2 + length;
                if marker == 0xDA {
                    loop {
                        let next = at + bytes.get(at..)?.iter().position(|&byte| byte == 0xFF)?;
                        let after = *bytes.get(next + 1)?;
                        if after == 0x00 || (0xD0..=0xD7).contains(&after) {
                            at = next + 2;
                        } else {
                            at = next;
                            break;
                        }
                    }
                }
            }
        }
    }
    None
}

fn orientation_of(bytes: &[u8]) -> Option<Orientation> {
    let tiff = if bytes.starts_with(b"II") || bytes.starts_with(b"MM") {
        bytes
    } else {
        let at = bytes.windows(4).take(1 << 20).position(|window| window == b"CMT1")?;
        bytes.get(at + 4..)?
    };
    let little = tiff.starts_with(b"II");
    let u16_at = |at: usize| -> Option<u16> {
        let pair = [*tiff.get(at)?, *tiff.get(at + 1)?];
        Some(if little {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        })
    };
    let u32_at = |at: usize| -> Option<u32> {
        let four = [
            *tiff.get(at)?,
            *tiff.get(at + 1)?,
            *tiff.get(at + 2)?,
            *tiff.get(at + 3)?,
        ];
        Some(if little {
            u32::from_le_bytes(four)
        } else {
            u32::from_be_bytes(four)
        })
    };
    let first = u32_at(4)? as usize;
    let count = usize::from(u16_at(first)?);
    (0..count.min(512)).find_map(|n| {
        let entry = first + 2 + 12 * n;
        (u16_at(entry)? == 0x0112)
            .then(|| u16_at(entry + 8))
            .flatten()
            .and_then(|value| Orientation::from_exif(value as u8))
    })
}
