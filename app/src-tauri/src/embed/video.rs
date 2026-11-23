use std::{path::Path, ptr, sync::OnceLock};

use windows::{
    Win32::{
        Media::MediaFoundation::{
            IMFAttributes, IMFMediaType, IMFSample, IMFSourceReader, MF_MT_AUDIO_NUM_CHANNELS,
            MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
            MF_MT_VIDEO_ROTATION, MF_PD_DURATION, MF_SOURCE_READER_ALL_STREAMS,
            MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_AUDIO_STREAM,
            MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_SOURCE_READER_MEDIASOURCE,
            MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED, MF_SOURCE_READERF_ENDOFSTREAM, MF_VERSION, MFAudioFormat_Float,
            MFCreateAttributes, MFCreateMediaType, MFCreateSourceReaderFromURL, MFMediaType_Audio, MFMediaType_Video,
            MFSTARTUP_FULL, MFStartup, MFVideoFormat_RGB32,
        },
        System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
    },
    core::{GUID, HSTRING},
};

use super::{
    picture,
    prepare::{Prepared, Unit},
    resample::{MODEL_RATE, Resampler},
};

const FRAME_EVERY: f64 = 10.0;
const MAX_FRAMES: usize = 120;
const MOST_SKIPPED: usize = 600;

const HUNDRED_NANOSECONDS: f64 = 10_000_000.0;

pub fn frames(path: &Path, emit: &mut dyn FnMut(Unit) -> bool) -> Result<(), String> {
    with_com(|| read(path, &frame_times, emit))
}

pub fn frames_at(path: &Path, fractions: &[f64], emit: &mut dyn FnMut(Unit) -> bool) -> Result<(), String> {
    let times = |duration: f64| {
        if duration > 0.0 {
            fractions.iter().map(|fraction| fraction * duration).collect()
        } else {
            vec![0.0]
        }
    };
    with_com(|| read(path, &times, emit))
}

fn with_com(read: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    unsafe {
        let com = CoInitializeEx(None, COINIT_MULTITHREADED);
        let result = read();
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

fn read(path: &Path, times: &dyn Fn(f64) -> Vec<f64>, emit: &mut dyn FnMut(Unit) -> bool) -> Result<(), String> {
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
        for seconds in times(duration) {
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

pub fn sound(path: &Path, from: f64, seconds: Option<f64>, each: &mut dyn FnMut(&[f32]) -> bool) -> Result<(), String> {
    with_com(|| read_sound(path, from, seconds, each))
}

pub fn sound_length(path: &Path) -> Option<f64> {
    let mut seconds = None;
    let _ = with_com(|| {
        startup()?;
        let duration = unsafe {
            duration_of(
                &MFCreateSourceReaderFromURL(&HSTRING::from(path.as_os_str()), None::<&IMFAttributes>)
                    .map_err(|e| e.to_string())?,
            )
        };
        seconds = (duration > 0.0).then_some(duration);
        Ok(())
    });
    seconds
}

fn read_sound(
    path: &Path,
    from: f64,
    seconds: Option<f64>,
    each: &mut dyn FnMut(&[f32]) -> bool,
) -> Result<(), String> {
    startup()?;
    unsafe {
        let reader = MFCreateSourceReaderFromURL(&HSTRING::from(path.as_os_str()), None::<&IMFAttributes>)
            .map_err(|e| format!("Windows can't open it: {e}"))?;
        let stream = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
        let _ = reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false);
        reader
            .SetStreamSelection(stream, true)
            .map_err(|_| "there is no sound in it".to_owned())?;
        let wanted = MFCreateMediaType().map_err(|e| e.to_string())?;
        wanted
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
            .map_err(|e| e.to_string())?;
        wanted
            .SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_Float)
            .map_err(|e| e.to_string())?;
        reader
            .SetCurrentMediaType(stream, None, &wanted)
            .map_err(|e| format!("Windows has no decoder for its sound: {e}"))?;
        let shape = |reader: &IMFSourceReader| -> Result<(usize, u32), String> {
            let current = reader.GetCurrentMediaType(stream).map_err(|e| e.to_string())?;
            let channels = current
                .GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS)
                .map_err(|e| e.to_string())?
                .max(1) as usize;
            let rate = current
                .GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND)
                .map_err(|e| e.to_string())?;
            Ok((channels, rate))
        };
        let (mut channels, mut rate) = shape(&reader)?;
        let start = (from.max(0.0) * HUNDRED_NANOSECONDS) as i64;
        if start > 0 {
            let position = windows::Win32::System::Com::StructuredStorage::PROPVARIANT::from(start);
            reader
                .SetCurrentPosition(&GUID::zeroed(), &position)
                .map_err(|e| format!("can't move through its sound: {e}"))?;
        }
        let mut left = seconds.map(|seconds| (seconds * f64::from(MODEL_RATE)) as usize);
        let mut resampler: Option<Resampler> = None;
        let (mut mono, mut out) = (Vec::new(), Vec::new());
        loop {
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
                .map_err(|e| format!("can't read its sound: {e}"))?;
            if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
                (channels, rate) = shape(&reader)?;
                resampler = None;
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                break;
            }
            let Some(sample) = sample else { continue };
            let buffer = sample.ConvertToContiguousBuffer().map_err(|e| e.to_string())?;
            let (mut data, mut length) = (ptr::null_mut(), 0u32);
            buffer
                .Lock(&mut data, None, Some(&mut length))
                .map_err(|e| e.to_string())?;
            mono.clear();
            if !data.is_null() {
                let bytes = std::slice::from_raw_parts(data, length as usize);
                let early = if timestamp < start {
                    ((start - timestamp) as f64 / HUNDRED_NANOSECONDS * f64::from(rate)) as usize
                } else {
                    0
                };
                mono.extend(bytes.chunks_exact(4 * channels).skip(early).map(|frame| {
                    frame
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|four| f32::from_le_bytes([four[0], four[1], four[2], four[3]]))
                        .sum::<f32>()
                        / channels as f32
                }));
            }
            let _ = buffer.Unlock();
            out.clear();
            if rate == MODEL_RATE {
                out.extend_from_slice(&mono);
            } else {
                let resampler = match &mut resampler {
                    Some(resampler) => resampler,
                    none => none.insert(Resampler::new(rate, MODEL_RATE)?),
                };
                resampler.push(&mono, &mut out);
            }
            if let Some(left) = left.as_mut() {
                out.truncate(*left);
                *left -= out.len();
            }
            let more = out.is_empty() || each(&out);
            if !more || left == Some(0) {
                return Ok(());
            }
        }
        out.clear();
        if let Some(resampler) = resampler.as_mut() {
            resampler.finish(&mut out);
        }
        if let Some(left) = left {
            out.truncate(left);
        }
        if !out.is_empty() {
            each(&out);
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

    #[test]
    fn frames_every_ten_seconds() {
        assert_eq!(frame_times(35.0), [5.0, 15.0, 25.0]);
        assert_eq!(frame_times(22.0), [5.0, 15.0]);
        assert_eq!(frame_times(12.0), [5.0]);
        assert_eq!(frame_times(5.0), [2.5]);
    }

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
        rgb.as_chunks::<3>().0.iter().map(|p| [p[0], p[1], p[2]]).collect()
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
