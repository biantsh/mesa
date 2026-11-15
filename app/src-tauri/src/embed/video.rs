use std::{path::Path, ptr, sync::OnceLock};

use windows::{
    Win32::{
        Media::MediaFoundation::{
            IMFAttributes, IMFMediaType, IMFSample, IMFSourceReader, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE,
            MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_MT_VIDEO_ROTATION, MF_PD_DURATION,
            MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
            MF_SOURCE_READER_MEDIASOURCE, MF_SOURCE_READERF_ENDOFSTREAM, MF_VERSION, MFCreateAttributes,
            MFCreateMediaType, MFCreateSourceReaderFromURL, MFMediaType_Video, MFSTARTUP_FULL, MFStartup,
            MFVideoFormat_RGB32,
        },
        System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
    },
    core::{GUID, HSTRING},
};

use super::{
    picture,
    prepare::{Prepared, Unit},
};

const FRAME_EVERY: f64 = 5.0;
const MAX_FRAMES: usize = 120;
const MOST_SKIPPED: usize = 600;

const HUNDRED_NANOSECONDS: f64 = 10_000_000.0;

pub fn frames(path: &Path, emit: &mut dyn FnMut(Unit) -> bool) -> Result<(), String> {
    unsafe {
        let com = CoInitializeEx(None, COINIT_MULTITHREADED);
        let result = read(path, emit);
        if com.is_ok() {
            CoUninitialize();
        }
        result
    }
}

fn startup() -> Result<(), String> {
    static STARTED: OnceLock<Result<(), String>> = OnceLock::new();
    STARTED
        .get_or_init(|| {
            unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.map_err(|e| format!("Media Foundation won't start: {e}"))
        })
        .clone()
}

unsafe fn read(path: &Path, emit: &mut dyn FnMut(Unit) -> bool) -> Result<(), String> {
    startup()?;
    unsafe {
        let mut attributes: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attributes, 1).map_err(|e| e.to_string())?;
        let attributes = attributes.ok_or("no attributes")?;
        attributes
            .SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)
            .map_err(|e| e.to_string())?;

        let reader = MFCreateSourceReaderFromURL(&HSTRING::from(path.as_os_str()), &attributes).map_err(|e| {
            format!("Windows can't open this video (it plays MP4, MOV and AVI of the usual kinds): {e}")
        })?;
        let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        reader
            .SetStreamSelection(stream, true)
            .map_err(|_| "there is no video in it".to_owned())?;

        let wanted = MFCreateMediaType().map_err(|e| e.to_string())?;
        wanted
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(|e| e.to_string())?;
        wanted
            .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)
            .map_err(|e| e.to_string())?;
        reader
            .SetCurrentMediaType(stream, None, &wanted)
            .map_err(|e| format!("Windows has no decoder for this video: {e}"))?;

        let rotation = reader
            .GetNativeMediaType(stream, 0)
            .ok()
            .map_or(0, |native| rotation_of(&native));
        let duration = duration_of(&reader);

        let mut sent = 0;
        for seconds in frame_times(duration) {
            let Some(frame) = frame_at(&reader, stream, seconds)? else {
                continue;
            };
            let (rgb, width, height) = upright(frame, rotation);
            let bitmap = picture::from_rgb(&rgb, width, height)?;
            sent += 1;
            let locator = (seconds * 1000.0).round() as u64;
            if !emit(Unit {
                locator,
                input: Prepared::Image(bitmap),
            }) {
                return Ok(());
            }
        }
        if sent == 0 {
            return Err("no frames could be read from it".into());
        }
        Ok(())
    }
}

fn frame_times(duration: f64) -> Vec<f64> {
    if duration <= 0.0 {
        return vec![0.0];
    }
    let every = FRAME_EVERY.max(duration / MAX_FRAMES as f64);
    let times: Vec<f64> = (0..MAX_FRAMES)
        .map(|n| (n as f64 + 0.5) * every)
        .take_while(|&t| t < duration)
        .collect();
    if times.is_empty() { vec![duration / 2.0] } else { times }
}

unsafe fn duration_of(reader: &IMFSourceReader) -> f64 {
    unsafe {
        reader
            .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
            .ok()
            .and_then(|value| u64::try_from(&value).ok())
            .map_or(0.0, |hundred_ns| hundred_ns as f64 / HUNDRED_NANOSECONDS)
    }
}

unsafe fn rotation_of(media_type: &IMFMediaType) -> u32 {
    unsafe {
        media_type
            .GetUINT32(&MF_MT_VIDEO_ROTATION)
            .map_or(0, |degrees| degrees % 360)
    }
}

struct Frame {
    bgra: Vec<u8>,
    width: usize,
    height: usize,
    stride: isize,
}

unsafe fn frame_at(reader: &IMFSourceReader, stream: u32, seconds: f64) -> Result<Option<Frame>, String> {
    unsafe {
        let position =
            windows::Win32::System::Com::StructuredStorage::PROPVARIANT::from((seconds * HUNDRED_NANOSECONDS) as i64);
        reader
            .SetCurrentPosition(&GUID::zeroed(), &position)
            .map_err(|e| format!("can't move through the video: {e}"))?;

        let target = (seconds * HUNDRED_NANOSECONDS) as i64;
        for _ in 0..MOST_SKIPPED {
            let (mut flags, mut timestamp) = (0u32, 0i64);
            let mut sample: Option<IMFSample> = None;
            reader
                .ReadSample(
                    stream,
                    0,
                    None,
                    Some(&mut flags),
                    Some(&mut timestamp),
                    Some(&mut sample),
                )
                .map_err(|e| format!("can't read the video: {e}"))?;
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return Ok(None);
            }
            let Some(sample) = sample else { continue };
            if timestamp < target {
                continue;
            }
            return copy_frame(reader, stream, &sample).map(Some);
        }
        Ok(None)
    }
}

unsafe fn copy_frame(reader: &IMFSourceReader, stream: u32, sample: &IMFSample) -> Result<Frame, String> {
    unsafe {
        let current = reader.GetCurrentMediaType(stream).map_err(|e| e.to_string())?;
        let size = current.GetUINT64(&MF_MT_FRAME_SIZE).map_err(|e| e.to_string())?;
        let (width, height) = ((size >> 32) as usize, (size & 0xFFFF_FFFF) as usize);
        let stride = current
            .GetUINT32(&MF_MT_DEFAULT_STRIDE)
            .map_or((width * 4) as isize, |s| s as i32 as isize);

        let buffer = sample.ConvertToContiguousBuffer().map_err(|e| e.to_string())?;
        let (mut data, mut length) = (ptr::null_mut(), 0u32);
        buffer
            .Lock(&mut data, None, Some(&mut length))
            .map_err(|e| e.to_string())?;
        let needed = stride.unsigned_abs() * height;
        let bgra = if data.is_null() || (length as usize) < needed {
            None
        } else {
            Some(std::slice::from_raw_parts(data, needed).to_vec())
        };
        let _ = buffer.Unlock();
        let bgra = bgra.ok_or("the frame is smaller than it says")?;
        Ok(Frame {
            bgra,
            width,
            height,
            stride,
        })
    }
}

fn upright(frame: Frame, rotation: u32) -> (Vec<u8>, u32, u32) {
    let Frame {
        bgra,
        width,
        height,
        stride,
    } = frame;
    let row_bytes = stride.unsigned_abs();
    let pixel = |x: usize, y: usize| {
        let row = if stride < 0 { height - 1 - y } else { y };
        let at = row * row_bytes + x * 4;
        [bgra[at + 2], bgra[at + 1], bgra[at]]
    };
    let (out_width, out_height) = if rotation == 90 || rotation == 270 {
        (height, width)
    } else {
        (width, height)
    };
    let mut rgb = Vec::with_capacity(out_width * out_height * 3);
    for y in 0..out_height {
        for x in 0..out_width {
            let (source_x, source_y) = match rotation {
                90 => (y, height - 1 - x),
                180 => (width - 1 - x, height - 1 - y),
                270 => (width - 1 - y, x),
                _ => (x, y),
            };
            rgb.extend_from_slice(&pixel(source_x, source_y));
        }
    }
    (rgb, out_width as u32, out_height as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(stride: isize) -> Frame {
        let rows = [[[255, 0, 0, 0], [0, 255, 0, 0]], [[0, 0, 255, 0], [255, 255, 255, 0]]];
        let mut bgra = Vec::new();
        let order: Vec<usize> = if stride < 0 { vec![1, 0] } else { vec![0, 1] };
        for row in order {
            rows[row].iter().for_each(|px| bgra.extend_from_slice(px));
        }
        Frame {
            bgra,
            width: 2,
            height: 2,
            stride,
        }
    }

    const BLUE: [u8; 3] = [0, 0, 255];
    const GREEN: [u8; 3] = [0, 255, 0];
    const RED: [u8; 3] = [255, 0, 0];
    const WHITE: [u8; 3] = [255, 255, 255];

    fn pixels(rgb: &[u8]) -> Vec<[u8; 3]> {
        rgb.chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect()
    }

    #[test]
    fn rotation_is_clockwise() {
        let (rgb, width, height) = upright(frame(8), 90);
        assert_eq!((width, height), (2, 2));
        assert_eq!(pixels(&rgb), [RED, BLUE, WHITE, GREEN]);
        assert_eq!(pixels(&upright(frame(8), 180).0), [WHITE, RED, GREEN, BLUE]);
        assert_eq!(pixels(&upright(frame(8), 270).0), [GREEN, WHITE, BLUE, RED]);
    }
}
