use std::path::Path;

use objc2_av_foundation::{AVAssetImageGenerator, AVURLAsset};
use objc2_core_foundation::CGSize;
use objc2_core_media::CMTime;
use objc2_foundation::{NSString, NSURL};

use super::{
    picture,
    prepare::{Prepared, Unit},
};
use crate::cgimage;

const FRAME_EVERY: f64 = 10.0;
const MAX_FRAMES: usize = 120;
const LARGEST_FRAME: f64 = 2048.0;
const TIMESCALE: i32 = 600;

pub fn frames(path: &Path, emit: &mut dyn FnMut(Unit) -> bool) -> Result<(), String> {
    objc2::rc::autoreleasepool(|_| read(path, &frame_times, emit))
}

pub fn frames_at(path: &Path, fractions: &[f64], emit: &mut dyn FnMut(Unit) -> bool) -> Result<(), String> {
    let times = |duration: f64| {
        if duration.is_finite() && duration > 0.0 {
            fractions.iter().map(|fraction| fraction * duration).collect()
        } else {
            vec![0.0]
        }
    };
    objc2::rc::autoreleasepool(|_| read(path, &times, emit))
}

#[allow(deprecated)]
fn read(path: &Path, times: &dyn Fn(f64) -> Vec<f64>, emit: &mut dyn FnMut(Unit) -> bool) -> Result<(), String> {
    let path = path.to_str().ok_or("the video's name can't be used")?;
    unsafe {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        let asset = AVURLAsset::URLAssetWithURL_options(&url, None);
        let duration = asset.duration();
        let duration = duration.seconds();
        let generator = AVAssetImageGenerator::assetImageGeneratorWithAsset(&asset);
        generator.setAppliesPreferredTrackTransform(true);
        generator.setMaximumSize(CGSize {
            width: LARGEST_FRAME,
            height: LARGEST_FRAME,
        });

        let mut sent = 0;
        let mut last_error = None;
        for seconds in times(duration) {
            let time = CMTime::with_seconds(seconds, TIMESCALE);
            let image = match generator.copyCGImageAtTime_actualTime_error(time, std::ptr::null_mut()) {
                Ok(image) => image,
                Err(e) => {
                    last_error = Some(e.localizedDescription().to_string());
                    continue;
                }
            };
            let Some((rgba, width, height)) = cgimage::pixels(&image) else {
                continue;
            };
            let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
            for pixel in rgba.as_chunks::<4>().0 {
                rgb.extend_from_slice(&pixel[..3]);
            }
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
            return Err(match last_error {
                Some(why) => format!("macOS can't read frames from this video: {why}"),
                None => "no frames could be read from it".into(),
            });
        }
        Ok(())
    }
}

#[allow(deprecated)]
pub fn sound(path: &Path, from: f64, seconds: Option<f64>, each: &mut dyn FnMut(&[f32]) -> bool) -> Result<(), String> {
    use objc2::{AnyThread, rc::Retained, runtime::AnyObject};
    use objc2_av_foundation::{AVAssetReader, AVAssetReaderTrackOutput, AVMediaTypeAudio};
    use objc2_core_media::CMTimeRange;
    use objc2_foundation::{NSDictionary, NSNumber};

    let path = path.to_str().ok_or("the file's name can't be used")?;
    objc2::rc::autoreleasepool(|_| unsafe {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        let asset = AVURLAsset::URLAssetWithURL_options(&url, None);
        if !asset.isReadable() {
            return Err("macOS can't open this kind of file".into());
        }
        let tracks = asset.tracksWithMediaType(AVMediaTypeAudio.ok_or("macOS has no type for sound")?);
        let track = tracks.firstObject().ok_or("there is no sound in it")?;
        let reader = AVAssetReader::assetReaderWithAsset_error(&asset)
            .map_err(|e| format!("macOS can't read its sound: {}", e.localizedDescription()))?;
        let keys = [
            "AVFormatIDKey",
            "AVSampleRateKey",
            "AVNumberOfChannelsKey",
            "AVLinearPCMBitDepthKey",
            "AVLinearPCMIsFloatKey",
            "AVLinearPCMIsNonInterleaved",
            "AVLinearPCMIsBigEndianKey",
        ]
        .map(NSString::from_str);
        let values: [Retained<NSNumber>; 7] = [
            NSNumber::new_u32(u32::from_be_bytes(*b"lpcm")),
            NSNumber::new_f64(f64::from(super::resample::MODEL_RATE)),
            NSNumber::new_u32(1),
            NSNumber::new_u32(32),
            NSNumber::new_bool(true),
            NSNumber::new_bool(false),
            NSNumber::new_bool(false),
        ];
        let key_refs: Vec<&NSString> = keys.iter().map(|key| &**key).collect();
        let value_refs: Vec<&AnyObject> = values.iter().map(|value| value.as_ref()).collect();
        let settings = NSDictionary::from_slices(&key_refs, &value_refs);
        let output = AVAssetReaderTrackOutput::initWithTrack_outputSettings(
            AVAssetReaderTrackOutput::alloc(),
            &track,
            Some(&settings),
        );
        output.setAlwaysCopiesSampleData(false);
        if !reader.canAddOutput(&output) {
            return Err("macOS can't decode its sound".into());
        }
        reader.addOutput(&output);
        if from > 0.0 || seconds.is_some() {
            let start = CMTime::with_seconds(from, TIMESCALE);
            let duration = match seconds {
                Some(seconds) => CMTime::with_seconds(seconds, TIMESCALE),
                None => CMTime::with_seconds((asset.duration().seconds() - from).max(0.0), TIMESCALE),
            };
            reader.setTimeRange(CMTimeRange { start, duration });
        }
        if !reader.startReading() {
            let why = reader
                .error()
                .map(|e| e.localizedDescription().to_string())
                .unwrap_or_default();
            return Err(format!("macOS can't decode its sound: {why}"));
        }
        let mut samples: Vec<f32> = Vec::new();
        while let Some(buffer) = output.copyNextSampleBuffer() {
            let Some(block) = buffer.data_buffer() else { continue };
            let length = block.data_length();
            if length < 4 {
                continue;
            }
            samples.clear();
            samples.resize(length / 4, 0.0);
            let room = std::ptr::NonNull::new(samples.as_mut_ptr().cast()).ok_or("no room for the sound")?;
            if block.copy_data_bytes(0, samples.len() * 4, room) != 0 {
                continue;
            }
            if !each(&samples) {
                reader.cancelReading();
                return Ok(());
            }
        }
        match reader.error() {
            Some(e) if samples.is_empty() => Err(format!("macOS can't decode its sound: {}", e.localizedDescription())),
            _ => Ok(()),
        }
    })
}

pub fn sound_length(path: &Path) -> Option<f64> {
    let path = path.to_str()?;
    objc2::rc::autoreleasepool(|_| {
        let seconds = unsafe {
            AVURLAsset::URLAssetWithURL_options(&NSURL::fileURLWithPath(&NSString::from_str(path)), None)
                .duration()
                .seconds()
        };
        (seconds.is_finite() && seconds > 0.0).then_some(seconds)
    })
}

fn frame_times(duration: f64) -> Vec<f64> {
    if duration.is_nan() || duration <= 0.0 {
        return vec![0.0];
    }
    let every = FRAME_EVERY.max(duration / MAX_FRAMES as f64);
    let times: Vec<f64> = (0..MAX_FRAMES)
        .map(|n| (n as f64 + 0.5) * every)
        .take_while(|&t| t < duration)
        .collect();
    if times.is_empty() { vec![duration / 2.0] } else { times }
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
}
