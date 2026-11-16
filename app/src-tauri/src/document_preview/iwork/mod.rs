pub(super) mod keynote;
pub(super) mod numbers;
pub(super) mod pages;

use std::collections::HashMap;

use crate::iwork_read::Book;

use super::opendocument::Picture;

pub(super) fn data_names(book: &Book) -> HashMap<u64, String> {
    let files = book.doc.data_files();
    let stored: HashMap<&str, &str> = files
        .iter()
        .filter(|file| !file.stored_name.is_empty())
        .map(|file| (file.original_name.as_str(), file.stored_name.as_str()))
        .collect();
    files
        .iter()
        .filter_map(|file| {
            let stored_name = if file.stored_name.is_empty() {
                let (stem, extension) = file.original_name.rsplit_once('.').unwrap_or((&file.original_name, ""));
                *stored.get(format!("{stem}-small.{extension}").as_str())?
            } else {
                file.stored_name.as_str()
            };
            Some((file.identifier, format!("Data/{stored_name}")))
        })
        .collect()
}

pub(super) fn picture(
    book: &Book,
    names: &HashMap<u64, String>,
    data: u64,
    allowed: &[Picture],
) -> Option<(Vec<u8>, Picture)> {
    let bytes = book.doc.package().get(names.get(&data)?)?;
    let format = Picture::of(bytes)?;
    allowed.contains(&format).then(|| (bytes.to_vec(), format))
}

pub(super) fn hex([red, green, blue]: [u8; 3]) -> String {
    format!("{red:02X}{green:02X}{blue:02X}")
}

pub(super) fn bytes_of(color: &iwork::Color) -> [u8; 3] {
    let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
    [byte(color.red), byte(color.green), byte(color.blue)]
}
