use super::{
    audio, documents, picture,
    prepare::{Prepared, Unit},
    resample::MODEL_RATE,
};

pub const VIDEO_PIECES: usize = 6;
pub const SOUND_PIECES: usize = 3;
const SHEET_FRAMES: usize = 4;
const SHEET_COLUMNS: usize = 2;
const CLIP_SAMPLES: usize = 30 * MODEL_RATE as usize;
const WAV_HEADER: usize = 44;
const TITLE_END: &str = " | text: ";
const SYMBOL_START: &str = " \u{b7} ";
const GAP: &str = "\n\u{2026}\n";

pub fn of_samples(units: Vec<Unit>) -> Option<Unit> {
    let (mut frames, mut clips) = (Vec::new(), Vec::new());
    for unit in units {
        match unit.input {
            Prepared::Image(bitmap) => frames.push((unit.locator, bitmap)),
            Prepared::Audio(wav) => clips.push((unit.locator, wav)),
            Prepared::Text(_) => {}
        }
    }
    frames.sort_by_key(|(locator, _)| *locator);
    clips.sort_by_key(|(locator, _)| *locator);
    let input = if !frames.is_empty() {
        let frames: Vec<Vec<u8>> = spread(frames, SHEET_FRAMES)
            .into_iter()
            .map(|(_, frame)| frame)
            .collect();
        Prepared::Image(contact_sheet(&frames).unwrap_or_else(|_| frames[0].clone()))
    } else if !clips.is_empty() {
        let clips: Vec<Vec<u8>> = clips.into_iter().map(|(_, clip)| clip).collect();
        Prepared::Audio(stitched_clip(&clips))
    } else {
        return None;
    };
    Some(Unit { locator: 0, input })
}

pub fn of_reading(units: Vec<Unit>) -> Option<Unit> {
    let mut texts: Vec<(u64, String)> = Vec::new();
    let mut first_other = None;
    for unit in units {
        match unit.input {
            Prepared::Text(text) => texts.push((unit.locator, text)),
            input => {
                first_other.get_or_insert(Unit {
                    locator: unit.locator,
                    input,
                });
            }
        }
    }
    if texts.is_empty() {
        return first_other;
    }
    texts.sort_by_key(|(locator, _)| *locator);
    let locator = texts[0].0;
    let texts: Vec<String> = texts.into_iter().map(|(_, text)| text).collect();
    Some(Unit {
        locator,
        input: Prepared::Text(stitched_text(&texts)),
    })
}

fn spread<T>(items: Vec<T>, most: usize) -> Vec<T> {
    let count = items.len();
    if count <= most {
        return items;
    }
    let wanted: Vec<usize> = (0..most)
        .map(|n| (n * (count - 1) + (most - 1) / 2) / (most - 1))
        .collect();
    items
        .into_iter()
        .enumerate()
        .filter_map(|(at, item)| wanted.contains(&at).then_some(item))
        .collect()
}

fn contact_sheet(frames: &[Vec<u8>]) -> Result<Vec<u8>, String> {
    if frames.len() == 1 {
        return Ok(frames[0].clone());
    }
    let unreadable = || "a frame couldn't be read back".to_owned();
    let (first, width, height) = picture::rgb_of(&frames[0]).ok_or_else(unreadable)?;
    let mut tiles = vec![first];
    for frame in &frames[1..] {
        let (rgb, w, h) = picture::rgb_of(frame).ok_or_else(unreadable)?;
        tiles.push(if (w, h) == (width, height) {
            rgb
        } else {
            picture::resize(&rgb, (w, h), (width, height))?
        });
    }
    let rows = tiles.len().div_ceil(SHEET_COLUMNS);
    let (tile_row, sheet_row) = (width as usize * 3, width as usize * 3 * SHEET_COLUMNS);
    let mut sheet = vec![0u8; sheet_row * height as usize * rows];
    for (at, tile) in tiles.iter().enumerate() {
        let (column, row) = (at % SHEET_COLUMNS, at / SHEET_COLUMNS);
        for (line, pixels) in tile.chunks_exact(tile_row).enumerate() {
            let start = (row * height as usize + line) * sheet_row + column * tile_row;
            sheet[start..start + tile_row].copy_from_slice(pixels);
        }
    }
    picture::from_rgb(&sheet, width * SHEET_COLUMNS as u32, height * rows as u32)
}

fn stitched_clip(clips: &[Vec<u8>]) -> Vec<u8> {
    let each = CLIP_SAMPLES / clips.len();
    let samples: Vec<i16> = clips
        .iter()
        .flat_map(|clip| {
            clip.get(WAV_HEADER..)
                .unwrap_or_default()
                .as_chunks::<2>()
                .0
                .iter()
                .take(each)
                .map(|bytes| i16::from_le_bytes(*bytes))
        })
        .collect();
    audio::wav(&samples)
}

fn stitched_text(texts: &[String]) -> String {
    if texts.len() == 1 {
        return texts[0].clone();
    }
    let picked = match texts.len() {
        2 => vec![&texts[0], &texts[1]],
        count => vec![&texts[0], &texts[count / 2], &texts[count - 1]],
    };
    let title = texts[0]
        .split_once(TITLE_END)
        .map(|(title, _)| title.split(SYMBOL_START).next().unwrap_or(title));
    let opening = documents::PASSAGE_CHARS / 2;
    let rest = (documents::PASSAGE_CHARS - opening) / (picked.len() - 1);
    let parts: Vec<String> = picked
        .iter()
        .enumerate()
        .map(|(at, text)| {
            let body = text.split_once(TITLE_END).map_or(text.as_str(), |(_, body)| body);
            let budget = if at == 0 { opening } else { rest };
            body.chars().take(budget).collect::<String>().trim().to_owned()
        })
        .collect();
    let joined = parts.join(GAP);
    match title {
        Some(title) => format!("{title}{TITLE_END}{joined}"),
        None => joined,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(shade: u8) -> Vec<u8> {
        picture::from_rgb(&vec![shade; 640 * 360 * 3], 640, 360).unwrap()
    }

    fn clip(value: i16, seconds: usize) -> Vec<u8> {
        audio::wav(&vec![value; seconds * MODEL_RATE as usize])
    }

    fn unit(locator: u64, input: Prepared) -> Unit {
        Unit { locator, input }
    }

    #[test]
    fn a_bitmap_reads_back_to_the_same_pixels() {
        let rgb: Vec<u8> = (0..528 * 528 * 3).map(|n| (n % 251) as u8).collect();
        let bitmap = picture::from_rgb(&rgb, 528, 528).unwrap();
        let (back, width, height) = picture::rgb_of(&bitmap).unwrap();
        assert_eq!((width, height), (528, 528));
        assert_eq!(back, rgb);
    }

    #[test]
    fn frames_become_one_sheet_and_sound_is_left_out() {
        let units = vec![
            unit(4000, Prepared::Image(frame(10))),
            unit(1000, Prepared::Image(frame(20))),
            unit(9000, Prepared::Audio(clip(5, 30))),
            unit(2000, Prepared::Image(frame(30))),
            unit(3000, Prepared::Image(frame(40))),
        ];
        let Some(Unit {
            locator: 0,
            input: Prepared::Image(sheet),
        }) = of_samples(units)
        else {
            panic!("expected one picture")
        };
        let (rgb, width, height) = picture::rgb_of(&sheet).unwrap();
        let at = |x: u32, y: u32| rgb[(y * width + x) as usize * 3];
        assert!(width > height);
        assert_eq!(
            [
                at(width / 4, height / 4),
                at(width * 3 / 4, height / 4),
                at(width / 4, height * 3 / 4),
                at(width * 3 / 4, height * 3 / 4)
            ],
            [20, 30, 40, 10]
        );
    }

    #[test]
    fn clips_are_stitched_into_one_clip_as_long_as_a_normal_one() {
        let units = vec![
            unit(60_000, Prepared::Audio(clip(3, 30))),
            unit(0, Prepared::Audio(clip(1, 30))),
            unit(30_000, Prepared::Audio(clip(2, 30))),
        ];
        let Some(Unit {
            input: Prepared::Audio(wav),
            ..
        }) = of_samples(units)
        else {
            panic!("expected one clip")
        };
        let samples: Vec<i16> = wav[WAV_HEADER..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes(*b))
            .collect();
        assert_eq!(samples.len(), CLIP_SAMPLES);
        let third = CLIP_SAMPLES / 3;
        assert_eq!([samples[0], samples[third], samples[2 * third]], [1, 2, 3]);
    }

    #[test]
    fn passages_are_stitched_from_the_start_middle_and_end_under_one_title() {
        let passage = |n: usize| {
            format!(
                "title: notes.md{SYMBOL_START}part {n} | text: passage {n} {}",
                "word ".repeat(400)
            )
        };
        let units: Vec<Unit> = (0..7).map(|n| unit(n as u64, Prepared::Text(passage(n)))).collect();
        let Some(Unit {
            locator: 0,
            input: Prepared::Text(text),
        }) = of_reading(units)
        else {
            panic!("expected one passage")
        };
        assert!(text.starts_with("title: notes.md | text: passage 0 "), "{text}");
        assert!(text.contains("passage 3 ") && text.contains("passage 6 ") && !text.contains("passage 1 "));
        assert!(
            text.chars().count()
                <= "title: notes.md | text: ".len() + documents::PASSAGE_CHARS + 2 * GAP.chars().count()
        );
    }

    #[test]
    fn a_document_without_text_keeps_its_first_page() {
        let units = vec![unit(1, Prepared::Image(frame(1))), unit(2, Prepared::Image(frame(2)))];
        let Some(Unit { locator: 1, .. }) = of_reading(units) else {
            panic!("expected the first page")
        };
    }
}
