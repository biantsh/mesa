use std::{fs::File, path::Path};

use symphonia::core::{
    audio::SampleBuffer,
    codecs::{CODEC_TYPE_NULL, DecoderOptions},
    errors::Error as DecodeError,
    formats::FormatOptions,
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
const HOP: usize = 25 * RATE;
const WORTH_A_CLIP: usize = 2 * RATE;
const SHORTEST: usize = RATE / 2;
const MAX_CLIPS: usize = 360;

type Emit<'a> = &'a mut dyn FnMut(Unit) -> bool;

pub fn file(path: &Path, emit: Emit) -> Result<(), String> {
    let source = File::open(path).map_err(|e| format!("can't open the sound: {e}"))?;
    let stream = MediaSourceStream::new(Box::new(source), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }
    let mut format = symphonia::default::get_probe()
        .format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("not a sound the reader knows: {e}"))?
        .format;
    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("there is no sound in it")?;
    let (track_id, params) = (track.id, track.codec_params.clone());
    let mut decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|e| format!("can't decode this kind of sound: {e}"))?;

    let mut clips = Clips::new(emit);
    let mut resampler: Option<Resampler> = None;
    let mut samples: Option<SampleBuffer<f32>> = None;
    let (mut mono, mut resampled) = (Vec::new(), Vec::new());
    let mut decoded_anything = false;

    'packets: loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(DecodeError::IoError(_)) | Err(DecodeError::ResetRequired) => break,
            Err(e) => return Err(format!("can't read the sound: {e}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let audio = match decoder.decode(&packet) {
            Ok(audio) => audio,
            Err(DecodeError::DecodeError(_)) | Err(DecodeError::IoError(_)) => continue,
            Err(e) => return Err(format!("can't decode the sound: {e}")),
        };
        let spec = *audio.spec();
        let channels = spec.channels.count().max(1);
        let buffer = samples.get_or_insert_with(|| SampleBuffer::new(audio.capacity() as u64, spec));
        if buffer.capacity() < audio.capacity() * channels {
            *buffer = SampleBuffer::new(audio.capacity() as u64, spec);
        }
        buffer.copy_interleaved_ref(audio);
        decoded_anything = true;

        mono.clear();
        mono.extend(
            buffer
                .samples()
                .chunks_exact(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32),
        );
        resampled.clear();
        if spec.rate == MODEL_RATE {
            resampled.extend_from_slice(&mono);
        } else {
            resampler
                .get_or_insert_with(|| Resampler::new(spec.rate, MODEL_RATE))
                .push(&mono, &mut resampled);
        }
        if !clips.push(&resampled) {
            break 'packets;
        }
    }
    if !decoded_anything {
        return Err("no sound could be decoded from it".into());
    }
    if let Some(resampler) = resampler.as_mut() {
        resampled.clear();
        resampler.finish(&mut resampled);
        clips.push(&resampled);
    }
    clips.finish()
}

struct Clips<'a> {
    emit: Emit<'a>,
    buffer: Vec<i16>,
    start: usize,
    sent: usize,
    stopped: bool,
}

impl<'a> Clips<'a> {
    fn new(emit: Emit<'a>) -> Self {
        Self {
            emit,
            buffer: Vec::new(),
            start: 0,
            sent: 0,
            stopped: false,
        }
    }

    fn push(&mut self, sound: &[f32]) -> bool {
        self.buffer
            .extend(sound.iter().map(|s| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16));
        while !self.stopped && self.buffer.len() >= CLIP {
            self.send(CLIP);
            self.buffer.drain(..HOP);
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
        } else if self.buffer.len() >= (CLIP - HOP) + WORTH_A_CLIP {
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
    use super::*;

    #[test]
    fn wav_header() {
        let bytes = wav(&[1, -2, 3]);
        assert_eq!(bytes.len(), 44 + 6);
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), MODEL_RATE);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 6);
    }
}
