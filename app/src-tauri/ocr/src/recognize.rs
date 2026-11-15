use super::{
    geometry::Picture,
    net::{CORE_ML, Net},
    resize::resize_rgb,
};

const HEIGHT: usize = 48;
const WIDTHS: [usize; 3] = [320, 640, 1280];
const BATCHES: [usize; 2] = [8, 1];
const MOST_AT_ONCE: usize = 128;
const OVERLAP: usize = 160;

pub struct Recognizer {
    net: Net,
    characters: Vec<String>,
}

pub fn shapes() -> Vec<[usize; 4]> {
    let batches: &[usize] = if CORE_ML { &BATCHES } else { &[1] };
    WIDTHS
        .iter()
        .flat_map(|&width| batches.iter().map(move |&batch| [batch, 3, HEIGHT, width]))
        .collect()
}

impl Recognizer {
    pub fn net(&self) -> &Net {
        &self.net
    }

    pub fn new(net: Net) -> Result<Self, String> {
        let listed = net
            .metadata("character")
            .ok_or("a recognizer doesn't say what its characters are")?;
        let mut characters = vec![String::new()];
        characters.extend(listed.lines().map(str::to_owned));
        characters.push(" ".into());
        Ok(Self { net, characters })
    }

    pub fn read_all(&self, lines: &[Picture]) -> Result<Vec<(String, f32)>, String> {
        let mut windows_of: Vec<Vec<usize>> = Vec::with_capacity(lines.len());
        let mut jobs: Vec<Job> = Vec::new();
        for &line in lines {
            let width = ((HEIGHT as f64 * line.width as f64 / line.height.max(1) as f64).ceil() as usize).max(1);
            let scaled = resize_rgb(line, width, HEIGHT)?;
            let widest = WIDTHS[WIDTHS.len() - 1];
            let windows = if width <= widest { vec![0] } else { window_starts(width) };
            for (n, &start) in windows.iter().enumerate() {
                let shown = (width - start).min(widest);
                let canvas = WIDTHS.iter().copied().find(|&w| w >= shown).unwrap_or(widest);
                let mut input = vec![0f32; 3 * HEIGHT * canvas];
                for y in 0..HEIGHT {
                    for x in 0..shown {
                        let pixel = &scaled[(y * width + start + x) * 3..][..3];
                        for (channel, &value) in [pixel[2], pixel[1], pixel[0]].iter().enumerate() {
                            input[(channel * HEIGHT + y) * canvas + x] = f32::from(value) / 127.5 - 1.0;
                        }
                    }
                }
                let from = if n == 0 {
                    0.0
                } else {
                    (windows[n - 1] + widest - start) as f64 / 2.0
                };
                let to = match windows.get(n + 1) {
                    Some(&next) => (next - start) as f64 + (start + widest - next) as f64 / 2.0,
                    None => canvas as f64,
                };
                jobs.push(Job {
                    canvas,
                    input,
                    from,
                    to,
                    steps: Vec::new(),
                });
            }
            windows_of.push(windows);
        }

        for canvas in WIDTHS {
            let mine: Vec<usize> = (0..jobs.len()).filter(|&j| jobs[j].canvas == canvas).collect();
            let mut chunks: Vec<&[usize]> = Vec::new();
            let mut rest = &mine[..];
            while !rest.is_empty() {
                let size = match (self.net.on_gpu(), CORE_ML) {
                    (true, false) => rest.len().min(MOST_AT_ONCE),
                    (true, true) => BATCHES.iter().copied().find(|&b| b <= rest.len()).unwrap_or(1),
                    (false, _) => 1,
                };
                let (chunk, after) = rest.split_at(size);
                chunks.push(chunk);
                rest = after;
            }
            for chunk in chunks {
                let size = chunk.len();
                let mut input = Vec::with_capacity(size * 3 * HEIGHT * canvas);
                for &j in chunk {
                    input.extend_from_slice(&jobs[j].input);
                }
                let outputs = self.net.run(&[size, 3, HEIGHT, canvas], input)?;
                let [(shape, best), (_, likelihood)] = &outputs[..] else {
                    return Err(format!(
                        "a recognizer gave {} outputs, not its characters and their likelihoods",
                        outputs.len()
                    ));
                };
                let count = match shape[..] {
                    [n, count] if n == size && best.len() == n * count && likelihood.len() == best.len() => count,
                    _ => return Err(format!("a recognizer gave {shape:?} for {size} lines")),
                };
                let stride = canvas as f64 / count as f64;
                for (k, &j) in chunk.iter().enumerate() {
                    let job = &mut jobs[j];
                    for t in 0..count {
                        let centre = (t as f64 + 0.5) * stride;
                        if centre < job.from || centre >= job.to {
                            continue;
                        }
                        job.steps
                            .push((best[k * count + t] as usize, likelihood[k * count + t]));
                    }
                }
            }
        }

        let mut read = Vec::with_capacity(lines.len());
        let mut at = 0;
        for windows in &windows_of {
            let steps: Vec<(usize, f32)> = jobs[at..at + windows.len()]
                .iter()
                .flat_map(|job| job.steps.iter().copied())
                .collect();
            at += windows.len();
            read.push(decode(&steps, &self.characters));
        }
        Ok(read)
    }
}

struct Job {
    canvas: usize,
    input: Vec<f32>,
    from: f64,
    to: f64,
    steps: Vec<(usize, f32)>,
}

fn window_starts(width: usize) -> Vec<usize> {
    let window = WIDTHS[WIDTHS.len() - 1];
    let step = window - OVERLAP;
    let last = (width - window).div_ceil(8) * 8;
    let mut starts: Vec<usize> = (0..).map(|n| n * step).take_while(|&s| s < last).collect();
    starts.push(last);
    starts
}

fn decode(steps: &[(usize, f32)], characters: &[String]) -> (String, f32) {
    let (mut text, mut sum, mut kept) = (String::new(), 0f32, 0usize);
    let mut before = None;
    for &(index, likelihood) in steps {
        if Some(index) != before && index != 0 {
            text.push_str(characters.get(index).map_or("", String::as_str));
            sum += likelihood;
            kept += 1;
        }
        before = Some(index);
    }
    (text, if kept == 0 { 0.0 } else { sum / kept as f32 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctc_decode_merges_repeats() {
        let characters: Vec<String> = ["", "a", "b", " "].iter().map(|s| s.to_string()).collect();
        let steps = [(1, 0.9), (1, 0.7), (0, 0.99), (1, 0.8), (2, 0.6), (0, 0.5), (3, 1.0)];
        let (text, sure) = decode(&steps, &characters);
        assert_eq!(text, "aab ");
        assert!((sure - (0.9 + 0.8 + 0.6 + 1.0) / 4.0).abs() < 1e-6);
        assert_eq!(decode(&[(0, 0.9)], &characters), (String::new(), 0.0));
    }
}
