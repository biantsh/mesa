use std::{path::Path, sync::OnceLock};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video::{self as gst_video, prelude::*};

use super::{
    picture,
    prepare::{Prepared, Unit},
};

const FRAME_EVERY: f64 = 5.0;
const MAX_FRAMES: usize = 120;
const STEP_TIMEOUT: gst::ClockTime = gst::ClockTime::from_seconds(15);

const SINK: &str = "videoflip video-direction=auto ! videoconvert ! videoscale ! video/x-raw,format=RGB,pixel-aspect-ratio=1/1 ! \
                    appsink name=frames sync=false max-buffers=1";

fn started() -> Result<(), String> {
    static STARTED: OnceLock<Result<(), String>> = OnceLock::new();
    STARTED
        .get_or_init(|| gst::init().map_err(|e| format!("GStreamer can't be started ({e})")))
        .clone()
}

pub fn frames(path: &Path, emit: &mut dyn FnMut(Unit) -> bool) -> Result<(), String> {
    let video = Video::open(path)?;
    let mut sent = 0;
    let mut last_error = None;
    for seconds in frame_times(video.duration) {
        let (rgb, width, height) = match video.frame_at(seconds) {
            Ok(frame) => frame,
            Err(why) => {
                last_error = Some(why);
                continue;
            }
        };
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
        return Err(last_error.unwrap_or_else(|| "no frames could be read from it".into()));
    }
    Ok(())
}

pub fn preview_frame(path: &Path) -> Option<(Vec<u8>, u32, u32)> {
    let video = Video::open(path).ok()?;
    video.frame_at(frame_times(video.duration)[0]).ok()
}

struct Video {
    pipeline: gst::Element,
    sink: gst_app::AppSink,
    duration: f64,
}

impl Drop for Video {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

impl Video {
    fn open(path: &Path) -> Result<Self, String> {
        started()?;
        let uri = gst::glib::filename_to_uri(path, None).map_err(|_| "the video's name can't be used")?;
        let pipeline = gst::ElementFactory::make("playbin")
            .property("uri", uri.as_str())
            .build()
            .map_err(|_| missing("playbin"))?;
        pipeline.set_property_from_str("flags", "video");
        let sink_bin = gst::parse::bin_from_description(SINK, true)
            .map_err(|e| format!("GStreamer can't make what reads frames ({e})"))?;
        let sink = sink_bin
            .by_name("frames")
            .and_then(|sink| sink.downcast::<gst_app::AppSink>().ok())
            .ok_or("GStreamer can't make what reads frames")?;
        pipeline.set_property("video-sink", &sink_bin);
        let mut video = Self {
            pipeline,
            sink,
            duration: f64::NAN,
        };
        video
            .pipeline
            .set_state(gst::State::Paused)
            .map_err(|_| video.reason("GStreamer can't open this video"))?;
        video.settle("GStreamer can't open this video")?;
        video.duration = video
            .pipeline
            .query_duration::<gst::ClockTime>()
            .map_or(f64::NAN, |length| length.nseconds() as f64 / 1e9);
        Ok(video)
    }

    fn settle(&self, what: &str) -> Result<(), String> {
        let bus = self.pipeline.bus().ok_or_else(|| what.to_owned())?;
        match bus.timed_pop_filtered(STEP_TIMEOUT, &[gst::MessageType::AsyncDone, gst::MessageType::Error]) {
            Some(message) => match message.view() {
                gst::MessageView::AsyncDone(_) => Ok(()),
                gst::MessageView::Error(error) => Err(explain(what, &error.error())),
                _ => Err(what.to_owned()),
            },
            None => Err(format!("{what} (it took too long)")),
        }
    }

    fn reason(&self, what: &str) -> String {
        self.pipeline
            .bus()
            .and_then(|bus| bus.pop_filtered(&[gst::MessageType::Error]))
            .and_then(|message| match message.view() {
                gst::MessageView::Error(error) => Some(explain(what, &error.error())),
                _ => None,
            })
            .unwrap_or_else(|| what.to_owned())
    }

    fn frame_at(&self, seconds: f64) -> Result<(Vec<u8>, u32, u32), String> {
        let at = gst::ClockTime::from_nseconds((seconds.max(0.0) * 1e9) as u64);
        self.pipeline
            .seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE, at)
            .map_err(|_| format!("GStreamer can't go to {seconds:.1} s in this video"))?;
        self.settle("GStreamer can't read a frame of this video")?;
        let sample = self
            .sink
            .try_pull_preroll(STEP_TIMEOUT)
            .ok_or("GStreamer gave no frame")?;
        let (buffer, caps) = (
            sample.buffer().ok_or("GStreamer gave no frame")?,
            sample.caps().ok_or("GStreamer gave no frame")?,
        );
        let info = gst_video::VideoInfo::from_caps(caps).map_err(|_| "GStreamer gave a frame of no known shape")?;
        let frame = gst_video::VideoFrameRef::from_buffer_ref_readable(buffer, &info)
            .map_err(|_| "GStreamer's frame can't be read")?;
        let (width, height) = (info.width() as usize, info.height() as usize);
        let stride = frame.plane_stride()[0] as usize;
        let data = frame.plane_data(0).map_err(|_| "GStreamer's frame can't be read")?;
        if width == 0 || height == 0 || stride < width * 3 || data.len() < stride * (height - 1) + width * 3 {
            return Err("GStreamer gave a frame of no known shape".into());
        }
        let mut rgb = Vec::with_capacity(width * height * 3);
        for row in 0..height {
            rgb.extend_from_slice(&data[row * stride..row * stride + width * 3]);
        }
        Ok((rgb, width as u32, height as u32))
    }
}

fn explain(what: &str, error: &gst::glib::Error) -> String {
    if error.matches(gst::CoreError::MissingPlugin) || error.matches(gst::StreamError::CodecNotFound) {
        return format!("{what}: no decoder for its format is installed (most are in the gstreamer1.0-libav package)");
    }
    format!("{what} ({})", error.message())
}

fn missing(element: &str) -> String {
    format!("GStreamer's {element} isn't installed (it is in the gstreamer1.0-plugins-base package)")
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
