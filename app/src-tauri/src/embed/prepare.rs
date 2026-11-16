use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
};

use super::{Item, audio, documents, office, picture, rtf};

pub type ReadText<'a> = Option<&'a mut dyn FnMut(u64, Vec<u8>, u32, u32)>;

pub enum Prepared {
    Image(Vec<u8>),
    Audio(Vec<u8>),
    Text(String),
}

pub struct Unit {
    pub locator: u64,
    pub input: Prepared,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Picture,
    SystemPicture,
    Text,
    Word,
    Excel,
    PowerPoint,
    Rtf,
    Pdf,
    Sound,
    Video,
}

fn kind(path: &Path) -> Option<Kind> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "jpg" | "jpeg" | "jfif" | "jpe" | "png" | "gif" | "webp" | "bmp" | "tif" | "tiff" => Kind::Picture,
        "heic" | "heif" => Kind::SystemPicture,
        "txt" | "md" | "markdown" => Kind::Text,
        "docx" | "docm" => Kind::Word,
        "xlsx" | "xlsm" => Kind::Excel,
        "pptx" | "pptm" | "ppsx" => Kind::PowerPoint,
        "rtf" => Kind::Rtf,
        "pdf" => Kind::Pdf,
        "mp3" | "wav" | "wave" | "flac" | "ogg" | "m4a" | "m4b" | "aac" | "aiff" | "aif" => Kind::Sound,
        "mp4" | "m4v" | "mov" | "mkv" | "avi" | "webm" => Kind::Video,
        _ => return None,
    })
}

pub fn is_supported(item: &Item) -> bool {
    match kind(&item.path) {
        None => false,
        Some(Kind::SystemPicture) => cfg!(any(windows, target_os = "macos", target_os = "linux")),
        Some(Kind::Video) => cfg!(any(windows, target_os = "macos", target_os = "linux")),
        Some(_) => true,
    }
}

/// A panic while reading one file fails only that file, not the whole run.
pub fn prepare(
    item: &Item,
    text: ReadText,
    emit: &mut dyn FnMut(Unit) -> bool,
    words: &mut String,
) -> Result<(), String> {
    catch_unwind(AssertUnwindSafe(|| read(item, text, emit, words))).unwrap_or_else(|panic| {
        let why = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied());
        Err(format!(
            "reading it went wrong inside Mesa ({})",
            why.unwrap_or("no details")
        ))
    })
}

pub fn passages(path: &Path, each: &mut dyn FnMut(u64, &str) -> bool) -> Result<(), String> {
    let mut words = String::new();
    let mut emit = |unit: Unit| match &unit.input {
        Prepared::Text(text) => each(unit.locator, documents::untitled(path, text)),
        _ => true,
    };
    catch_unwind(AssertUnwindSafe(|| {
        documents::every_passage(|| match kind(path) {
            Some(Kind::Text) => documents::plain(path, &mut emit, &mut words),
            Some(Kind::Word) => documents::docx(path, &mut emit, &mut words),
            Some(Kind::Excel) => office::xlsx(path, &mut emit, &mut words),
            Some(Kind::PowerPoint) => office::pptx_slides(path, &mut emit, &mut words),
            Some(Kind::Rtf) => rtf::rtf(path, &mut emit, &mut words),
            Some(Kind::Pdf) => documents::pdf(path, &mut emit, &mut words, None),
            _ => Err("it isn't a document".into()),
        })
    }))
    .unwrap_or_else(|_| Err("reading it went wrong inside Mesa".into()))
}

fn read(item: &Item, text: ReadText, emit: &mut dyn FnMut(Unit) -> bool, words: &mut String) -> Result<(), String> {
    let path = &item.path;
    match kind(path) {
        Some(Kind::Picture) => one_picture(item, picture::decode(path)?, text, emit),
        #[cfg(windows)]
        Some(Kind::SystemPicture) => one_picture(item, super::wic::decode(path)?, text, emit),
        #[cfg(target_os = "macos")]
        Some(Kind::SystemPicture) => one_picture(item, super::imageio::decode(path)?, text, emit),
        #[cfg(target_os = "linux")]
        Some(Kind::SystemPicture) => one_picture(item, super::heif::decode(path)?, text, emit),
        Some(Kind::Text) => documents::plain(path, emit, words),
        Some(Kind::Word) => documents::docx(path, emit, words),
        Some(Kind::Excel) => office::xlsx(path, emit, words),
        Some(Kind::PowerPoint) => office::pptx(path, emit, words),
        Some(Kind::Rtf) => rtf::rtf(path, emit, words),
        Some(Kind::Pdf) => documents::pdf(
            path,
            emit,
            words,
            Some(documents::Scans {
                text,
                words_only: item.words_only,
            }),
        ),
        Some(Kind::Sound) => audio::file(path, emit),
        #[cfg(windows)]
        Some(Kind::Video) => super::video::frames(path, emit),
        #[cfg(target_os = "macos")]
        Some(Kind::Video) => super::avfoundation::frames(path, emit),
        #[cfg(target_os = "linux")]
        Some(Kind::Video) => super::gstreamer::frames(path, emit),
        #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
        Some(Kind::SystemPicture) => Err("this kind of file isn't read on this platform yet".into()),
        #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
        Some(Kind::Video) => Err("this kind of file isn't read on this platform yet".into()),
        None => Err("this kind of file isn't read".into()),
    }
}

pub const MOST_PIECES: usize = 8;
const ORDER: [usize; MOST_PIECES] = [0, 4, 2, 6, 1, 5, 3, 7];

fn order(slots: usize) -> Vec<usize> {
    ORDER.iter().copied().filter(|&slot| slot < slots).collect()
}

fn fraction(slot: usize, slots: usize) -> f64 {
    (slot as f64 + 0.5) / slots as f64
}

pub fn sample(item: &Item, from: usize, to: usize, emit: &mut dyn FnMut(Unit) -> bool) -> Result<usize, String> {
    catch_unwind(AssertUnwindSafe(|| sample_unguarded(item, from, to, emit))).unwrap_or_else(|panic| {
        let why = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied());
        Err(format!(
            "reading it went wrong inside Mesa ({})",
            why.unwrap_or("no details")
        ))
    })
}

fn sample_unguarded(item: &Item, from: usize, to: usize, emit: &mut dyn FnMut(Unit) -> bool) -> Result<usize, String> {
    let path = &item.path;
    let wanted = |slots: usize| {
        order(slots)
            .into_iter()
            .skip(from)
            .take(to.saturating_sub(from))
            .collect::<Vec<_>>()
    };
    match kind(path) {
        None => Err("this kind of file isn't read".into()),
        Some(Kind::Picture | Kind::SystemPicture) => {
            if from == 0 {
                read(item, None, emit, &mut String::new())?;
            }
            Ok(1)
        }
        Some(Kind::Sound) => match audio::length(path) {
            Some(seconds) => {
                let slots = audio::clip_count(seconds).min(MOST_PIECES);
                let starts: Vec<f64> = wanted(slots)
                    .into_iter()
                    .map(|slot| audio::clip_start(seconds, fraction(slot, slots)))
                    .collect();
                if !starts.is_empty() {
                    audio::clips_at(path, &starts, emit)?;
                }
                Ok(slots)
            }
            None => by_choosing(item, from, to, emit),
        },
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        Some(Kind::Video) => {
            let fractions: Vec<f64> = wanted(MOST_PIECES)
                .into_iter()
                .map(|slot| fraction(slot, MOST_PIECES))
                .collect();
            if !fractions.is_empty() {
                #[cfg(windows)]
                super::video::frames_at(path, &fractions, emit)?;
                #[cfg(target_os = "macos")]
                super::avfoundation::frames_at(path, &fractions, emit)?;
                #[cfg(target_os = "linux")]
                super::gstreamer::frames_at(path, &fractions, emit)?;
            }
            Ok(MOST_PIECES)
        }
        Some(Kind::Pdf) => documents::pdf_pages(
            path,
            &mut |pages| {
                let slots = pages.clamp(1, MOST_PIECES);
                wanted(slots)
                    .into_iter()
                    .map(|slot| {
                        (
                            (fraction(slot, slots) * pages as f64) as usize,
                            (slot + 1) * pages / slots,
                        )
                    })
                    .collect()
            },
            emit,
        )
        .map(|pages| pages.clamp(1, MOST_PIECES)),
        _ => by_choosing(item, from, to, emit),
    }
}

pub fn chosen(count: usize, from: usize, to: usize) -> (Vec<usize>, usize) {
    let slots = count.min(MOST_PIECES);
    let picked = order(slots)
        .into_iter()
        .skip(from)
        .take(to.saturating_sub(from))
        .map(|slot| (fraction(slot, slots) * count as f64) as usize);
    (picked.collect(), slots)
}

fn by_choosing(item: &Item, from: usize, to: usize, emit: &mut dyn FnMut(Unit) -> bool) -> Result<usize, String> {
    let mut units = Vec::new();
    let read_result = read(
        item,
        None,
        &mut |unit| {
            units.push(unit);
            true
        },
        &mut String::new(),
    );
    if units.is_empty() {
        return read_result.map(|()| 0);
    }
    let (picked, slots) = chosen(units.len(), from, to);
    let mut units: Vec<Option<Unit>> = units.into_iter().map(Some).collect();
    for index in picked {
        if let Some(unit) = units[index].take()
            && !emit(unit)
        {
            break;
        }
    }
    Ok(slots)
}

fn one_picture(
    item: &Item,
    (rgb, width, height): (Vec<u8>, u32, u32),
    text: ReadText,
    emit: &mut dyn FnMut(Unit) -> bool,
) -> Result<(), String> {
    let wanted = item.words_only
        || emit(Unit {
            locator: 0,
            input: Prepared::Image(picture::from_rgb(&rgb, width, height)?),
        });
    if wanted && let Some(text) = text {
        text(0, rgb, width, height);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_from_extension() {
        assert_eq!(kind(Path::new("a.JPG")), Some(Kind::Picture));
        assert_eq!(kind(Path::new("a.heic")), Some(Kind::SystemPicture));
        assert_eq!(kind(Path::new("notes.MD")), Some(Kind::Text));
        assert_eq!(kind(Path::new("report.docx")), Some(Kind::Word));
        assert_eq!(kind(Path::new("budget.XLSX")), Some(Kind::Excel));
        assert_eq!(kind(Path::new("talk.pptx")), Some(Kind::PowerPoint));
        assert_eq!(kind(Path::new("show.PPSX")), Some(Kind::PowerPoint));
        assert_eq!(kind(Path::new("deck.pptm")), Some(Kind::PowerPoint));
        assert_eq!(kind(Path::new("letter.docm")), Some(Kind::Word));
        assert_eq!(kind(Path::new("budget.xlsm")), Some(Kind::Excel));
        assert_eq!(kind(Path::new("letter.rtf")), Some(Kind::Rtf));
        assert_eq!(kind(Path::new("download.jfif")), Some(Kind::Picture));
        assert_eq!(kind(Path::new("scan.tiff")), Some(Kind::Picture));
        assert_eq!(kind(Path::new("book.m4b")), Some(Kind::Sound));
        assert_eq!(kind(Path::new("clip.m4v")), Some(Kind::Video));
        assert_eq!(kind(Path::new("paper.pdf")), Some(Kind::Pdf));
        assert_eq!(kind(Path::new("song.flac")), Some(Kind::Sound));
        assert_eq!(kind(Path::new("clip.mkv")), Some(Kind::Video));
        assert_eq!(kind(Path::new("setup.exe")), None);
        assert_eq!(kind(Path::new("README")), None);
    }

    #[test]
    fn panic_fails_only_that_file() {
        let mut emit = |_: Unit| -> bool { panic!("boom") };
        let dir = std::env::temp_dir().join("mesa-prepare-panic");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.txt");
        std::fs::write(&path, "some words, enough of them for the model").unwrap();
        let why = prepare(
            &Item {
                path,
                words_only: false,
            },
            None,
            &mut emit,
            &mut String::new(),
        )
        .unwrap_err();
        assert!(why.contains("boom"), "{why}");
    }
}
