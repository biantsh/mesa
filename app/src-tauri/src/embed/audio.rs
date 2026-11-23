use std::{fs::File, path::Path};

use symphonia::core::{
    audio::SampleBuffer,
    codecs::{CODEC_TYPE_NULL, CODEC_TYPE_OPUS, DecoderOptions},
    errors::Error as DecodeError,
    formats::{FormatOptions, FormatReader},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};

use super::{
    prepare::{Prepared, Unit},
    resample::{MODEL_RATE, Resampler},
};

const RATE: usize = MODEL_RATE as usize;
const CLIP: usize = 30 * RATE;
const HOP: usize = 50 * RATE;
const WORTH_A_CLIP: usize = 2 * RATE;
const SHORTEST: usize = RATE / 2;
const MAX_CLIPS: usize = 32;

type Emit<'a> = &'a mut dyn FnMut(Unit) -> bool;

pub fn file(path: &Path, emit: Emit) -> Result<(), String> {
    let sent = std::cell::Cell::new(false);
    let mut counted = |unit: Unit| {
        sent.set(true);
        emit(unit)
    };
    let read = Sound::open(path).and_then(|sound| read_through(sound, &mut counted));
    match read {
        Err(why) if !sent.get() => {
            system_file(path, &mut counted).map_err(|system| if system.is_empty() { why } else { system })
        }
        read => read,
    }
}

fn read_through(mut sound: Sound, emit: Emit) -> Result<(), String> {
    if let Some(starts) = sound.seconds().and_then(spread_starts) {
        return sound.clips_at(&starts, emit);
    }
    let mut clips = Clips::new(emit);
    let mut resampled = Vec::new();
    while sound.next(&mut resampled)? {
        if !clips.push(&resampled) {
            return clips.finish();
        }
    }
    if !sound.decoded {
        return Err("no sound could be decoded from it".into());
    }
    sound.finish(&mut resampled);
    clips.push(&resampled);
    clips.finish()
}

fn system_file(path: &Path, emit: Emit) -> Result<(), String> {
    if let Some(starts) = system_length(path).and_then(spread_starts) {
        return system_clips_at(path, &starts, emit);
    }
    let mut clips = Clips::new(emit);
    system_sound(path, 0.0, None, &mut |sound| clips.push(sound))?;
    clips.finish()
}

fn system_clips_at(path: &Path, starts: &[f64], emit: Emit) -> Result<(), String> {
    let mut sent = 0;
    for &start in starts {
        let start = start.max(0.0);
        let mut clip = Vec::<i16>::with_capacity(CLIP);
        let read = system_sound(path, start, Some(CLIP as f64 / RATE as f64), &mut |sound| {
            clip.extend(sound.iter().map(|s| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16));
            clip.len() < CLIP
        });
        clip.truncate(CLIP);
        if read.is_err() || clip.len() < SHORTEST {
            continue;
        }
        sent += 1;
        if !emit(Unit {
            locator: (start * 1000.0).round() as u64,
            input: Prepared::Audio(wav(&clip)),
        }) {
            return Ok(());
        }
    }
    if sent == 0 {
        return Err("no sound could be decoded from it".into());
    }
    Ok(())
}

fn system_sound(
    path: &Path,
    from: f64,
    seconds: Option<f64>,
    each: &mut dyn FnMut(&[f32]) -> bool,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        super::avfoundation::sound(path, from, seconds, each)
    }
    #[cfg(windows)]
    {
        super::video::sound(path, from, seconds, each)
    }
    #[cfg(target_os = "linux")]
    {
        super::gstreamer::sound(path, from, seconds, each)
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = (path, from, seconds, each);
        Err(String::new())
    }
}

fn system_length(path: &Path) -> Option<f64> {
    #[cfg(target_os = "macos")]
    {
        super::avfoundation::sound_length(path)
    }
    #[cfg(windows)]
    {
        super::video::sound_length(path)
    }
    #[cfg(target_os = "linux")]
    {
        super::gstreamer::sound_length(path)
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        None
    }
}

pub fn clip_count(seconds: f64) -> usize {
    clips_in(seconds).min(MAX_CLIPS)
}

fn clips_in(seconds: f64) -> usize {
    let samples = (seconds * RATE as f64) as usize;
    if samples <= CLIP {
        return 1;
    }
    let whole = (samples - CLIP) / HOP + 1;
    let rest = samples.saturating_sub(whole * HOP);
    whole + usize::from(rest >= CLIP.saturating_sub(HOP) + WORTH_A_CLIP)
}

pub fn clip_start(seconds: f64, fraction: f64) -> f64 {
    let clip = CLIP as f64 / RATE as f64;
    (fraction * seconds - clip / 2.0).clamp(0.0, (seconds - clip).max(0.0))
}

fn spread_starts(seconds: f64) -> Option<Vec<f64>> {
    if !seconds.is_finite() || clips_in(seconds) <= MAX_CLIPS {
        return None;
    }
    Some(
        (0..MAX_CLIPS)
            .map(|n| clip_start(seconds, (n as f64 + 0.5) / MAX_CLIPS as f64))
            .collect(),
    )
}

pub fn length(path: &Path) -> Option<f64> {
    match Sound::open(path) {
        Ok(sound) => sound.seconds(),
        Err(_) => system_length(path),
    }
}

pub fn clips_at(path: &Path, starts: &[f64], emit: Emit) -> Result<(), String> {
    match Sound::open(path) {
        Ok(mut sound) => sound.clips_at(starts, emit),
        Err(why) => system_clips_at(path, starts, emit).map_err(|_| why),
    }
}

pub fn clips_around(path: &Path, places: impl FnOnce(usize) -> Vec<f64>, emit: Emit) -> bool {
    let mut sent = false;
    let mut counted = |unit: Unit| {
        sent = true;
        emit(unit)
    };
    let starts = |seconds: f64| -> Vec<f64> {
        places(clip_count(seconds))
            .into_iter()
            .map(|at| clip_start(seconds, at))
            .collect()
    };
    match Sound::open(path) {
        Ok(mut sound) => {
            if let Some(seconds) = sound.seconds() {
                let _ = sound.clips_at(&starts(seconds), &mut counted);
            }
        }
        Err(_) => {
            if let Some(seconds) = system_length(path) {
                let _ = system_clips_at(path, &starts(seconds), &mut counted);
            }
        }
    }
    sent
}

enum Decoder {
    Symphonia(Box<dyn symphonia::core::codecs::Decoder>),
    Opus(Box<Opus>),
}

struct Opus {
    decoder: rusty_opus::OpusDecoder,
    channels: usize,
    skip: usize,
    pcm: Vec<f32>,
}

const OPUS_MOST: usize = MODEL_RATE as usize * 120 / 1000;

impl Opus {
    fn new(head: Option<&[u8]>, channels: Option<usize>) -> Result<Self, String> {
        let head = head.filter(|head| head.starts_with(b"OpusHead"));
        let channels = head
            .and_then(|head| head.get(9))
            .map(|&count| usize::from(count))
            .or(channels)
            .unwrap_or(2);
        if head.and_then(|head| head.get(18)).is_some_and(|&family| family != 0) || !(1..=2).contains(&channels) {
            return Err(format!(
                "its Opus sound has {channels} channels in a layout that isn't read"
            ));
        }
        let skip = head
            .and_then(|head| Some(usize::from(u16::from_le_bytes([*head.get(10)?, *head.get(11)?]))))
            .unwrap_or(0)
            / 3;
        let decoder = rusty_opus::OpusDecoder::new(MODEL_RATE as i32, channels)
            .map_err(|e| format!("can't decode its Opus sound: {e}"))?;
        Ok(Self {
            decoder,
            channels,
            skip,
            pcm: vec![0.0; OPUS_MOST * channels],
        })
    }
}

struct Sound {
    format: Box<dyn FormatReader>,
    track_id: u32,
    decoder: Decoder,
    samples: Option<SampleBuffer<f32>>,
    resampler: Option<Resampler>,
    mono: Vec<f32>,
    decoded: bool,
}

impl Sound {
    fn open(path: &Path) -> Result<Self, String> {
        let source = File::open(path).map_err(|e| format!("can't open the sound: {e}"))?;
        let stream = MediaSourceStream::new(Box::new(source), Default::default());
        let mut hint = Hint::new();
        if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(extension);
        }
        let probe = symphonia::default::get_probe();
        let format = match probe.format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default()) {
            Ok(probed) => probed.format,
            Err(why) => {
                let (sound, codec) =
                    super::demux::sound_of(path).ok_or_else(|| format!("not a sound the reader knows: {why}"))?;
                let mut hint = Hint::new();
                hint.with_extension(codec.extension());
                let stream = MediaSourceStream::new(Box::new(sound), Default::default());
                probe
                    .format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default())
                    .map_err(|e| format!("can't read its sound: {e}"))?
                    .format
            }
        };
        let track = format
            .tracks()
            .iter()
            .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or("there is no sound in it")?;
        let track_id = track.id;
        let decoder = if track.codec_params.codec == CODEC_TYPE_OPUS {
            Decoder::Opus(Box::new(Opus::new(
                track.codec_params.extra_data.as_deref(),
                track.codec_params.channels.map(|channels| channels.count()),
            )?))
        } else {
            Decoder::Symphonia(
                symphonia::default::get_codecs()
                    .make(&track.codec_params, &DecoderOptions::default())
                    .map_err(|e| format!("can't decode this kind of sound: {e}"))?,
            )
        };
        Ok(Self {
            format,
            track_id,
            decoder,
            samples: None,
            resampler: None,
            mono: Vec::new(),
            decoded: false,
        })
    }

    fn seconds(&self) -> Option<f64> {
        let track = self.format.tracks().iter().find(|track| track.id == self.track_id)?;
        let time = track.codec_params.time_base?.calc_time(track.codec_params.n_frames?);
        Some(time.seconds as f64 + time.frac)
    }

    fn next(&mut self, out: &mut Vec<f32>) -> Result<bool, String> {
        out.clear();
        let packet = loop {
            match self.format.next_packet() {
                Ok(packet) if packet.track_id() == self.track_id => break packet,
                Ok(_) => continue,
                Err(DecodeError::IoError(_)) | Err(DecodeError::ResetRequired) => return Ok(false),
                Err(e) => return Err(format!("can't read the sound: {e}")),
            }
        };
        let decoder = match &mut self.decoder {
            Decoder::Symphonia(decoder) => decoder,
            Decoder::Opus(opus) => {
                let Ok(frames) = opus.decoder.decode(packet.buf(), OPUS_MOST, &mut opus.pcm) else {
                    return Ok(true);
                };
                self.decoded = true;
                let skipped = opus.skip.min(frames);
                opus.skip -= skipped;
                self.mono.clear();
                self.mono.extend(
                    opus.pcm[skipped * opus.channels..frames * opus.channels]
                        .chunks_exact(opus.channels)
                        .map(|frame| frame.iter().sum::<f32>() / opus.channels as f32),
                );
                out.extend_from_slice(&self.mono);
                return Ok(true);
            }
        };
        let audio = match decoder.decode(&packet) {
            Ok(audio) => audio,
            Err(DecodeError::DecodeError(_)) | Err(DecodeError::IoError(_)) => return Ok(true),
            Err(e) => return Err(format!("can't decode the sound: {e}")),
        };
        self.decoded = true;
        let spec = *audio.spec();
        let channels = spec.channels.count().max(1);
        let buffer = self
            .samples
            .get_or_insert_with(|| SampleBuffer::new(audio.capacity() as u64, spec));
        if buffer.capacity() < audio.capacity() * channels {
            *buffer = SampleBuffer::new(audio.capacity() as u64, spec);
        }
        buffer.copy_interleaved_ref(audio);
        self.mono.clear();
        self.mono.extend(
            buffer
                .samples()
                .chunks_exact(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32),
        );
        if spec.rate == MODEL_RATE {
            out.extend_from_slice(&self.mono);
        } else {
            let resampler = match &mut self.resampler {
                Some(resampler) => resampler,
                none => none.insert(Resampler::new(spec.rate, MODEL_RATE)?),
            };
            resampler.push(&self.mono, out);
        }
        Ok(true)
    }

    fn finish(&mut self, out: &mut Vec<f32>) {
        out.clear();
        if let Some(resampler) = self.resampler.as_mut() {
            resampler.finish(out);
        }
    }

    fn clips_at(&mut self, starts: &[f64], emit: Emit) -> Result<(), String> {
        use symphonia::core::{
            formats::{SeekMode, SeekTo},
            units::Time,
        };
        let mut sent = 0;
        let mut sound = Vec::new();
        for &start in starts {
            let start = start.max(0.0);
            let to = SeekTo::Time {
                time: Time::new(start as u64, start.fract()),
                track_id: Some(self.track_id),
            };
            if self.format.seek(SeekMode::Accurate, to).is_err() {
                continue;
            }
            match &mut self.decoder {
                Decoder::Symphonia(decoder) => decoder.reset(),
                Decoder::Opus(opus) => {
                    if let Ok(fresh) = rusty_opus::OpusDecoder::new(MODEL_RATE as i32, opus.channels) {
                        opus.decoder = fresh;
                    }
                }
            }
            self.resampler = None;
            let mut clip = Vec::<i16>::with_capacity(CLIP);
            while clip.len() < CLIP && self.next(&mut sound).unwrap_or(false) {
                clip.extend(sound.iter().map(|s| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16));
            }
            clip.truncate(CLIP);
            if clip.len() < SHORTEST {
                continue;
            }
            sent += 1;
            if !emit(Unit {
                locator: (start * 1000.0).round() as u64,
                input: Prepared::Audio(wav(&clip)),
            }) {
                return Ok(());
            }
        }
        if sent == 0 {
            return Err("no sound could be decoded from it".into());
        }
        Ok(())
    }
}

struct Clips<'a> {
    emit: Emit<'a>,
    buffer: Vec<i16>,
    skip: usize,
    start: usize,
    sent: usize,
    stopped: bool,
}

impl<'a> Clips<'a> {
    fn new(emit: Emit<'a>) -> Self {
        Self {
            emit,
            buffer: Vec::new(),
            skip: 0,
            start: 0,
            sent: 0,
            stopped: false,
        }
    }

    fn push(&mut self, sound: &[f32]) -> bool {
        let skipped = self.skip.min(sound.len());
        self.skip -= skipped;
        self.buffer.extend(
            sound[skipped..]
                .iter()
                .map(|s| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16),
        );
        while !self.stopped && self.buffer.len() >= CLIP {
            self.send(CLIP);
            if HOP < self.buffer.len() {
                self.buffer.drain(..HOP);
            } else {
                self.skip = HOP - self.buffer.len();
                self.buffer.clear();
            }
            self.start += HOP;
        }
        !self.stopped
    }

    fn finish(mut self) -> Result<(), String> {
        if self.stopped {
            return Ok(());
        }
        if self.sent == 0 {
            if self.buffer.len() < SHORTEST {
                return Err("it is too short to hear".into());
            }
            self.send(self.buffer.len());
        } else if self.buffer.len() >= CLIP.saturating_sub(HOP) + WORTH_A_CLIP {
            self.send(self.buffer.len());
        }
        Ok(())
    }

    fn send(&mut self, length: usize) {
        if self.sent == MAX_CLIPS {
            self.stopped = true;
            return;
        }
        self.sent += 1;
        let locator = (self.start as u64 * 1000) / u64::from(MODEL_RATE);
        let unit = Unit {
            locator,
            input: Prepared::Audio(wav(&self.buffer[..length])),
        };
        if !(self.emit)(unit) {
            self.stopped = true;
        }
    }
}

pub(super) fn wav(samples: &[i16]) -> Vec<u8> {
    let data = samples.len() * 2;
    let mut file = Vec::with_capacity(44 + data);
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&((36 + data) as u32).to_le_bytes());
    file.extend_from_slice(b"WAVEfmt ");
    file.extend_from_slice(&16u32.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&MODEL_RATE.to_le_bytes());
    file.extend_from_slice(&(MODEL_RATE * 2).to_le_bytes());
    file.extend_from_slice(&2u16.to_le_bytes());
    file.extend_from_slice(&16u16.to_le_bytes());
    file.extend_from_slice(b"data");
    file.extend_from_slice(&(data as u32).to_le_bytes());
    samples.iter().for_each(|s| file.extend_from_slice(&s.to_le_bytes()));
    file
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;
    use crate::embed::fixtures::stereo_wav;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mesa-audio-{tag}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn clips_of(path: &Path) -> Result<Vec<(u64, Vec<i16>)>, String> {
        let mut clips = Vec::new();
        file(path, &mut |unit| {
            let Prepared::Audio(bytes) = unit.input else {
                panic!("sound gives sound")
            };
            assert_eq!(&bytes[..4], b"RIFF");
            let samples = bytes[44..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .collect();
            clips.push((unit.locator, samples));
            true
        })?;
        Ok(clips)
    }

    #[test]
    fn long_audio_split_into_clips() {
        let dir = temp_dir("long");
        let path = dir.join("long.wav");
        fs::write(&path, stereo_wav(16000, 130.0)).unwrap();
        let clips = clips_of(&path).unwrap();
        let starts: Vec<u64> = clips.iter().map(|(start, _)| *start).collect();
        assert_eq!(starts, [0, 50_000, 100_000]);
        assert!(clips.iter().all(|(_, sound)| sound.len() == CLIP));
        assert_eq!(clip_count(130.0), 3);
    }

    #[test]
    fn wav_header() {
        let bytes = wav(&[1, -2, 3]);
        assert_eq!(bytes.len(), 44 + 6);
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), MODEL_RATE);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 6);
    }
}
