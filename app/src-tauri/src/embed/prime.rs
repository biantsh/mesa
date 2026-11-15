use super::{audio, documents, picture, prepare::Prepared, resample::MODEL_RATE};

const PICTURE_SIDE: u32 = 528;
const PASSAGE_CHARS: usize = 1500;
const CLIP_SECONDS: usize = 30;

pub fn batches() -> Vec<Vec<Prepared>> {
    vec![texts(), vec![picture()], vec![sound()]]
}

fn texts() -> Vec<Prepared> {
    let sentence = "The quarterly report lists the sales and the costs of every office, and the menu of the café. ";
    let passage: String = sentence.chars().cycle().take(PASSAGE_CHARS).collect();
    let query = Prepared::Text(documents::query_prompt("a photo of a dog"));
    let passages = (1..=8).map(|n| Prepared::Text(format!("title: file{n}.txt | text: {passage}")));
    std::iter::once(query).chain(passages).collect()
}

fn picture() -> Prepared {
    let side = PICTURE_SIDE as usize;
    let mut rgb = Vec::with_capacity(side * side * 3);
    for y in 0..side {
        for x in 0..side {
            rgb.extend_from_slice(&[x as u8, y as u8, (x + y) as u8]);
        }
    }
    Prepared::Image(picture::from_rgb(&rgb, PICTURE_SIDE, PICTURE_SIDE).expect("a square picture can be read"))
}

fn sound() -> Prepared {
    let rate = MODEL_RATE as usize;
    let samples: Vec<i16> = (0..CLIP_SECONDS * rate)
        .map(|n| (6000.0 * (n as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin()) as i16)
        .collect();
    Prepared::Audio(audio::wav(&samples))
}
