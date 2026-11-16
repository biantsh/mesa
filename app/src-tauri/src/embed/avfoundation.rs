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
            for pixel in rgba.chunks_exact(4) {
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

fn frame_times(duration: f64) -> Vec<f64> {
    if !(duration > 0.0) {
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
