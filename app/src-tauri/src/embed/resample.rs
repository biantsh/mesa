pub const MODEL_RATE: u32 = 16_000;
const ZEROS: f64 = 10.0;
const PASSBAND: f64 = 0.95;
const MOST_TAPS: u64 = 4_000_000;

pub struct Resampler {
    step_in: u64,
    step_out: u64,
    reach: i64,
    weights: Vec<Vec<f32>>,
    buffer: Vec<f32>,
    base: i64,
    next_output: u64,
    length: i64,
}

impl Resampler {
    pub fn new(rate_in: u32, rate_out: u32) -> Result<Self, String> {
        let unusable = || format!("its sample rate ({rate_in} Hz) can't be right");
        if rate_in == 0 || rate_out == 0 {
            return Err(unusable());
        }
        let divisor = gcd(u64::from(rate_in), u64::from(rate_out));
        let (step_in, step_out) = (u64::from(rate_in) / divisor, u64::from(rate_out) / divisor);
        let cutoff = (step_out as f64 / step_in as f64).min(1.0) * PASSBAND;
        let reach = (ZEROS / cutoff).ceil() as i64;
        if step_out.saturating_mul(2 * reach as u64) > MOST_TAPS {
            return Err(unusable());
        }

        let weights = (0..step_out)
            .map(|phase| {
                let fraction = phase as f64 / step_out as f64;
                let mut taps: Vec<f64> = (0..2 * reach)
                    .map(|k| {
                        let distance = fraction + (reach - 1 - k) as f64;
                        let sinc = if distance == 0.0 {
                            1.0
                        } else {
                            let x = std::f64::consts::PI * cutoff * distance;
                            x.sin() / x
                        };
                        let position = distance / reach as f64;
                        let hann = if position.abs() < 1.0 {
                            0.5 * (1.0 + (std::f64::consts::PI * position).cos())
                        } else {
                            0.0
                        };
                        cutoff * sinc * hann
                    })
                    .collect();
                let total: f64 = taps.iter().sum();
                taps.iter_mut().for_each(|w| *w /= total);
                taps.into_iter().map(|w| w as f32).collect()
            })
            .collect();

        Ok(Self {
            step_in,
            step_out,
            reach,
            weights,
            buffer: vec![0.0; reach as usize],
            base: -reach,
            next_output: 0,
            length: 0,
        })
    }

    pub fn push(&mut self, input: &[f32], output: &mut Vec<f32>) {
        self.buffer.extend_from_slice(input);
        self.length += input.len() as i64;
        self.produce(output, false);
    }

    pub fn finish(&mut self, output: &mut Vec<f32>) {
        self.buffer.extend(std::iter::repeat_n(0.0, self.reach as usize));
        self.produce(output, true);
    }

    fn produce(&mut self, output: &mut Vec<f32>, last: bool) {
        loop {
            let position = self.next_output * self.step_in;
            let index = (position / self.step_out) as i64;
            let end = index + self.reach - self.base;
            if (last && index >= self.length) || (!last && end >= self.buffer.len() as i64) {
                break;
            }
            let start = (index - self.reach + 1 - self.base) as usize;
            let taps = &self.weights[(position % self.step_out) as usize];
            let window = &self.buffer[start..start + taps.len()];
            output.push(window.iter().zip(taps).map(|(sample, weight)| sample * weight).sum());
            self.next_output += 1;
        }
        let next_index = (self.next_output * self.step_in / self.step_out) as i64;
        let used = next_index - self.reach + 1 - self.base;
        if used > 8192 {
            self.buffer.drain(..used as usize);
            self.base += used;
        }
    }
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, hertz: f64, seconds: f64) -> Vec<f32> {
        (0..(f64::from(rate) * seconds) as usize)
            .map(|n| (2.0 * std::f64::consts::PI * hertz * n as f64 / f64::from(rate)).sin() as f32)
            .collect()
    }

    fn resample_all(input: &[f32], rate_in: u32, chunk: usize) -> Vec<f32> {
        let mut resampler = Resampler::new(rate_in, MODEL_RATE).unwrap();
        let mut output = Vec::new();
        input.chunks(chunk).for_each(|part| resampler.push(part, &mut output));
        resampler.finish(&mut output);
        output
    }

    fn loudness(signal: &[f32], margin: usize) -> f32 {
        let middle = &signal[margin..signal.len() - margin];
        (middle.iter().map(|s| s * s).sum::<f32>() / middle.len() as f32).sqrt()
    }

    #[test]
    fn output_length() {
        for rate in [8000, 11025, 22050, 32000, 44100, 48000, 96000] {
            let output = resample_all(&tone(rate, 440.0, 2.0), rate, 4096);
            let expected = 2 * MODEL_RATE as usize;
            assert!(
                output.len().abs_diff(expected) <= 1,
                "{rate} Hz: {} samples, expected {expected}",
                output.len()
            );
        }
    }

    #[test]
    fn refuses_sample_rates_that_cant_be_right() {
        for rate in [0, 50_000_001, 4_294_967_295] {
            assert!(Resampler::new(rate, MODEL_RATE).is_err(), "{rate} Hz");
        }
        for rate in [
            8000, 11025, 22050, 44100, 44101, 48000, 96000, 192_000, 705_600, 768_000, 1_536_000,
        ] {
            assert!(Resampler::new(rate, MODEL_RATE).is_ok(), "{rate} Hz");
        }
    }

    #[test]
    fn filters_above_nyquist() {
        let output = resample_all(&tone(48000, 12_000.0, 1.0), 48000, 1000);
        assert!(loudness(&output, 500) < 0.01, "{}", loudness(&output, 500));
    }
}
