use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
};

use super::{Item, audio, code, documents, epub, iwork, office, opendocument, picture, raw, rtf};

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
    Raw,
    Text,
    Word,
    Excel,
    PowerPoint,
    OpenText,
    OpenSheet,
    OpenSlides,
    Pages,
    Numbers,
    Keynote,
    Epub,
    Html,
    Rtf,
    Pdf,
    Sound,
    Video,
    Code,
}

fn kind(path: &Path, code: bool) -> Option<Kind> {
    if code {
        return Some(Kind::Code);
    }
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "jpg" | "jpeg" | "jfif" | "jpe" | "png" | "gif" | "webp" | "bmp" | "tif" | "tiff" => Kind::Picture,
        "heic" | "heif" | "avif" => Kind::SystemPicture,
        extension if raw::EXTENSIONS.contains(&extension) => Kind::Raw,
        "txt" | "md" | "markdown" => Kind::Text,
        "html" | "htm" | "xhtml" => Kind::Html,
        "docx" | "docm" | "dotx" | "dotm" => Kind::Word,
        "xlsx" | "xlsm" | "xltx" | "xltm" => Kind::Excel,
        "pptx" | "pptm" | "ppsx" | "potx" | "potm" => Kind::PowerPoint,
        "odt" | "ott" => Kind::OpenText,
        "ods" | "ots" => Kind::OpenSheet,
        "odp" | "otp" => Kind::OpenSlides,
        "pages" => Kind::Pages,
        "numbers" => Kind::Numbers,
        "key" => Kind::Keynote,
        "epub" => Kind::Epub,
        "rtf" => Kind::Rtf,
        "pdf" => Kind::Pdf,
        "mp3" | "wav" | "wave" | "flac" | "ogg" | "opus" | "m4a" | "m4b" | "aac" | "aiff" | "aif" | "caf" | "amr"
        | "wma" => Kind::Sound,
        "mp4" | "m4v" | "mov" | "mkv" | "avi" | "webm" | "3gp" | "3g2" | "mpg" | "mpeg" | "mts" | "m2ts" | "wmv" => {
            Kind::Video
        }
        _ => return None,
    })
}

pub fn is_supported(item: &Item) -> bool {
    match kind(&item.path, code::is_code_path(&item.path)) {
        None => false,
        Some(Kind::SystemPicture) => cfg!(any(windows, target_os = "macos", target_os = "linux")),
        Some(Kind::Video) => cfg!(any(windows, target_os = "macos", target_os = "linux")),
        Some(_) => true,
    }
}

pub fn overview_pieces(path: &Path) -> Option<usize> {
    match kind(path, code::is_code_path(path)) {
        Some(Kind::Video) => Some(super::overview::VIDEO_PIECES),
        Some(Kind::Sound) => Some(super::overview::SOUND_PIECES),
        _ => None,
    }
}

pub fn prepare(
    item: &Item,
    code: bool,
    text: ReadText,
    emit: &mut dyn FnMut(Unit) -> bool,
    words: &mut String,
) -> Result<(), String> {
    catch_unwind(AssertUnwindSafe(|| read(item, code, text, emit, words))).unwrap_or_else(|panic| {
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
        documents::every_passage(|| match kind(path, false) {
            Some(Kind::Text) => documents::plain(path, &mut emit, &mut words),
            Some(Kind::Word) => documents::docx(path, &mut emit, &mut words),
            Some(Kind::Excel) => office::xlsx(path, &mut emit, &mut words),
            Some(Kind::PowerPoint) => office::pptx_slides(path, &mut emit, &mut words),
            Some(Kind::OpenText) => opendocument::odt(path, &mut emit, &mut words),
            Some(Kind::OpenSheet) => opendocument::ods(path, &mut emit, &mut words),
            Some(Kind::OpenSlides) => opendocument::odp_slides(path, &mut emit, &mut words),
            Some(Kind::Pages) => iwork::pages(path, &mut emit, &mut words),
            Some(Kind::Numbers) => iwork::numbers(path, &mut emit, &mut words),
            Some(Kind::Keynote) => iwork::keynote_slides(path, &mut emit, &mut words),
            Some(Kind::Epub) => epub::epub(path, &mut emit, &mut words),
            Some(Kind::Html) => documents::html(path, &mut emit, &mut words),
            Some(Kind::Rtf) => rtf::rtf(path, &mut emit, &mut words),
            Some(Kind::Pdf) => documents::pdf(path, &mut emit, &mut words, None),
            _ => Err("it isn't a document".into()),
        })
    }))
    .unwrap_or_else(|_| Err("reading it went wrong inside Mesa".into()))
}

fn read(
    item: &Item,
    code: bool,
    text: ReadText,
    emit: &mut dyn FnMut(Unit) -> bool,
    words: &mut String,
) -> Result<(), String> {
    let path = &item.path;
    match kind(path, code) {
        Some(Kind::Picture) => one_picture(item, picture::decode(path)?, text, emit),
        Some(Kind::Raw) => one_picture(item, raw::decode(path)?, text, emit),
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
        Some(Kind::OpenText) => opendocument::odt(path, emit, words),
        Some(Kind::OpenSheet) => opendocument::ods(path, emit, words),
        Some(Kind::OpenSlides) => opendocument::odp(path, emit, words),
        Some(Kind::Pages) => iwork::pages(path, emit, words),
        Some(Kind::Numbers) => iwork::numbers(path, emit, words),
        Some(Kind::Keynote) => iwork::keynote(path, emit, words),
        Some(Kind::Epub) => epub::epub(path, emit, words),
        Some(Kind::Html) => documents::html(path, emit, words),
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
        Some(Kind::Code) => code::read(path, emit, words),
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        Some(Kind::Video) => video(path, emit),
        #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
        Some(Kind::SystemPicture) => Err("this kind of file isn't read on this platform yet".into()),
        #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
        Some(Kind::Video) => Err("this kind of file isn't read on this platform yet".into()),
        None => Err("this kind of file isn't read".into()),
    }
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn video(path: &Path, emit: &mut dyn FnMut(Unit) -> bool) -> Result<(), String> {
    let stopped = std::cell::Cell::new(false);
    let mut handed = |unit: Unit| {
        let wanted = emit(unit);
        stopped.set(stopped.get() || !wanted);
        wanted
    };
    #[cfg(windows)]
    let frames = super::video::frames(path, &mut handed);
    #[cfg(target_os = "macos")]
    let frames = super::avfoundation::frames(path, &mut handed);
    #[cfg(target_os = "linux")]
    let frames = super::gstreamer::frames(path, &mut handed);
    if stopped.get() {
        return Ok(());
    }
    let sound = audio::file(path, &mut handed);
    match (frames, sound) {
        (Err(why), Err(_)) => Err(why),
        _ => Ok(()),
    }
}

pub const MOST_PIECES: usize = 8;
const ORDER: [usize; MOST_PIECES] = [0, 4, 2, 6, 1, 5, 3, 7];
const VIDEO_SOUND: [usize; 2] = [4, 1];

fn order(slots: usize) -> Vec<usize> {
    ORDER.iter().copied().filter(|&slot| slot < slots).collect()
}

fn fraction(slot: usize, slots: usize) -> f64 {
    (slot as f64 + 0.5) / slots as f64
}

pub fn sample(
    item: &Item,
    code: bool,
    from: usize,
    to: usize,
    emit: &mut dyn FnMut(Unit) -> bool,
) -> Result<usize, String> {
    catch_unwind(AssertUnwindSafe(|| sample_unguarded(item, code, from, to, emit))).unwrap_or_else(|panic| {
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

fn sample_unguarded(
    item: &Item,
    code: bool,
    from: usize,
    to: usize,
    emit: &mut dyn FnMut(Unit) -> bool,
) -> Result<usize, String> {
    let path = &item.path;
    let wanted = |slots: usize| {
        order(slots)
            .into_iter()
            .skip(from)
            .take(to.saturating_sub(from))
            .collect::<Vec<_>>()
    };
    match kind(path, code) {
        None => Err("this kind of file isn't read".into()),
        Some(Kind::Picture | Kind::SystemPicture | Kind::Raw) => {
            if from == 0 {
                read(item, code, None, emit, &mut String::new())?;
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
            None => by_choosing(item, code, from, to, emit),
        },
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        Some(Kind::Video) => {
            let wanted = wanted(MOST_PIECES);
            let stopped = std::cell::Cell::new(false);
            let mut sent = 0;
            let mut handed = |unit: Unit| {
                sent += 1;
                let more = emit(unit);
                stopped.set(stopped.get() || !more);
                more
            };
            let mut heard = Vec::new();
            if wanted.iter().any(|slot| VIDEO_SOUND.contains(slot)) {
                let read = audio::clips_around(
                    path,
                    |clips| {
                        heard = VIDEO_SOUND[..clips.min(VIDEO_SOUND.len())]
                            .iter()
                            .copied()
                            .filter(|slot| wanted.contains(slot))
                            .collect();
                        heard.iter().map(|&slot| fraction(slot, MOST_PIECES)).collect()
                    },
                    &mut handed,
                );
                if !read {
                    heard.clear();
                }
                if stopped.get() {
                    return Ok(MOST_PIECES);
                }
            }
            let frames: Vec<f64> = wanted
                .iter()
                .filter(|slot| !heard.contains(slot))
                .map(|&slot| fraction(slot, MOST_PIECES))
                .collect();
            let framed = if frames.is_empty() {
                Ok(())
            } else {
                #[cfg(windows)]
                let framed = super::video::frames_at(path, &frames, &mut handed);
                #[cfg(target_os = "macos")]
                let framed = super::avfoundation::frames_at(path, &frames, &mut handed);
                #[cfg(target_os = "linux")]
                let framed = super::gstreamer::frames_at(path, &frames, &mut handed);
                framed
            };
            match framed {
                Err(why) if sent == 0 => Err(why),
                _ => Ok(MOST_PIECES),
            }
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
        _ => by_choosing(item, code, from, to, emit),
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

fn by_choosing(
    item: &Item,
    code: bool,
    from: usize,
    to: usize,
    emit: &mut dyn FnMut(Unit) -> bool,
) -> Result<usize, String> {
    let mut units = Vec::new();
    let read_result = read(
        item,
        code,
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

    fn kind(path: &Path) -> Option<Kind> {
        super::kind(path, code::is_code_path(path))
    }

    #[test]
    fn kind_from_extension() {
        assert_eq!(kind(Path::new("a.JPG")), Some(Kind::Picture));
        assert_eq!(kind(Path::new("a.heic")), Some(Kind::SystemPicture));
        assert_eq!(kind(Path::new("a.AVIF")), Some(Kind::SystemPicture));
        assert_eq!(kind(Path::new("IMG_0001.CR3")), Some(Kind::Raw));
        assert_eq!(kind(Path::new("photo.dng")), Some(Kind::Raw));
        assert_eq!(kind(Path::new("notes.MD")), Some(Kind::Text));
        assert_eq!(kind(Path::new("report.docx")), Some(Kind::Word));
        assert_eq!(kind(Path::new("budget.XLSX")), Some(Kind::Excel));
        assert_eq!(kind(Path::new("talk.pptx")), Some(Kind::PowerPoint));
        assert_eq!(kind(Path::new("show.PPSX")), Some(Kind::PowerPoint));
        assert_eq!(kind(Path::new("deck.pptm")), Some(Kind::PowerPoint));
        assert_eq!(kind(Path::new("letter.docm")), Some(Kind::Word));
        assert_eq!(kind(Path::new("budget.xlsm")), Some(Kind::Excel));
        assert_eq!(kind(Path::new("letter.rtf")), Some(Kind::Rtf));
        assert_eq!(kind(Path::new("letter.ODT")), Some(Kind::OpenText));
        assert_eq!(kind(Path::new("budget.ods")), Some(Kind::OpenSheet));
        assert_eq!(kind(Path::new("talk.odp")), Some(Kind::OpenSlides));
        assert_eq!(kind(Path::new("Report.pages")), Some(Kind::Pages));
        assert_eq!(kind(Path::new("letter.dotx")), Some(Kind::Word));
        assert_eq!(kind(Path::new("budget.XLTX")), Some(Kind::Excel));
        assert_eq!(kind(Path::new("deck.potx")), Some(Kind::PowerPoint));
        assert_eq!(kind(Path::new("letter.ott")), Some(Kind::OpenText));
        assert_eq!(kind(Path::new("budget.numbers")), Some(Kind::Numbers));
        assert_eq!(kind(Path::new("talk.KEY")), Some(Kind::Keynote));
        assert_eq!(kind(Path::new("download.jfif")), Some(Kind::Picture));
        assert_eq!(kind(Path::new("scan.tiff")), Some(Kind::Picture));
        assert_eq!(kind(Path::new("book.m4b")), Some(Kind::Sound));
        assert_eq!(kind(Path::new("clip.m4v")), Some(Kind::Video));
        assert_eq!(kind(Path::new("paper.pdf")), Some(Kind::Pdf));
        assert_eq!(kind(Path::new("song.flac")), Some(Kind::Sound));
        assert_eq!(kind(Path::new("clip.mkv")), Some(Kind::Video));
        assert_eq!(kind(Path::new("holiday.MTS")), Some(Kind::Video));
        assert_eq!(kind(Path::new("old phone.3gp")), Some(Kind::Video));
        assert_eq!(kind(Path::new("voice memo.amr")), Some(Kind::Sound));
        assert_eq!(kind(Path::new("track 01.wma")), Some(Kind::Sound));
        assert_eq!(kind(Path::new("talk.opus")), Some(Kind::Sound));
        assert_eq!(kind(Path::new("index.ts")), Some(Kind::Code));
        assert_eq!(kind(Path::new("CMakeLists.txt")), Some(Kind::Code));
        assert_eq!(kind(Path::new("Moby Dick.EPUB")), Some(Kind::Epub));
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
                shallow: false,
            },
            false,
            None,
            &mut emit,
            &mut String::new(),
        )
        .unwrap_err();
        assert!(why.contains("boom"), "{why}");
    }
}
