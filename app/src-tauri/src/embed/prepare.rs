use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
};

use super::{Item, audio, documents, picture};

pub type ReadText<'a> = Option<&'a mut dyn FnMut(Vec<u8>, u32, u32)>;

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
    Pdf,
    Sound,
    Video,
}

fn kind(path: &Path) -> Option<Kind> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" => Kind::Picture,
        "heic" | "heif" => Kind::SystemPicture,
        "txt" | "md" => Kind::Text,
        "docx" => Kind::Word,
        "pdf" => Kind::Pdf,
        "mp3" | "wav" | "flac" | "ogg" | "m4a" => Kind::Sound,
        "mp4" | "mov" | "mkv" | "avi" | "webm" => Kind::Video,
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
        Some(Kind::Pdf) => documents::pdf(path, emit, words),
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
        text(rgb, width, height);
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
