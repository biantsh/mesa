use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
};

use super::{Item, audio, documents, picture};

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
        Some(Kind::SystemPicture) => cfg!(any(windows, target_os = "macos")),
        Some(Kind::Video) => cfg!(any(windows, target_os = "macos")),
        Some(_) => true,
    }
}

/// A panic while reading one file fails only that file, not the whole run.
pub fn prepare(item: &Item, emit: &mut dyn FnMut(Unit) -> bool) -> Result<(), String> {
    catch_unwind(AssertUnwindSafe(|| read(item, emit))).unwrap_or_else(|panic| {
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

fn read(item: &Item, emit: &mut dyn FnMut(Unit) -> bool) -> Result<(), String> {
    let path = &item.path;
    let one = |input: Prepared, emit: &mut dyn FnMut(Unit) -> bool| {
        emit(Unit { locator: 0, input });
        Ok(())
    };
    match kind(path) {
        Some(Kind::Picture) => one(Prepared::Image(picture::from_file(path)?), emit),
        #[cfg(windows)]
        Some(Kind::SystemPicture) => {
            let (rgb, width, height) = super::wic::decode(path)?;
            one(Prepared::Image(picture::from_rgb(rgb, width, height)?), emit)
        }
        #[cfg(target_os = "macos")]
        Some(Kind::SystemPicture) => {
            let (rgb, width, height) = super::imageio::decode(path)?;
            one(Prepared::Image(picture::from_rgb(rgb, width, height)?), emit)
        }
        Some(Kind::Text) => documents::plain(path, emit),
        Some(Kind::Word) => documents::docx(path, emit),
        Some(Kind::Pdf) => documents::pdf(path, emit),
        Some(Kind::Sound) => audio::file(path, emit),
        #[cfg(windows)]
        Some(Kind::Video) => super::video::frames(path, emit),
        #[cfg(target_os = "macos")]
        Some(Kind::Video) => super::avfoundation::frames(path, emit),
        #[cfg(not(any(windows, target_os = "macos")))]
        Some(Kind::SystemPicture) => Err("this kind of file isn't read on this platform yet".into()),
        #[cfg(not(any(windows, target_os = "macos")))]
        Some(Kind::Video) => Err("this kind of file isn't read on this platform yet".into()),
        None => Err("this kind of file isn't read".into()),
    }
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
        std::fs::write(&path, "some words").unwrap();
        let why = prepare(&Item { path }, &mut emit).unwrap_err();
        assert!(why.contains("boom"), "{why}");
    }
}
