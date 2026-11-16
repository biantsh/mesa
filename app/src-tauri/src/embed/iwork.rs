use std::{collections::HashSet, path::Path};

use super::documents::{self, Emit, MAX_PASSAGES, WORDS_CHARS};
use crate::iwork_read::{Book, shown};

const MAX_CELLS: usize = WORDS_CHARS / 4;

fn lines_of(text: &str) -> String {
    text.chars()
        .filter_map(|c| match c {
            '\r' | '\u{000C}' | '\u{0005}' | '\u{0004}' | '\u{2028}' | '\u{2029}' | '\u{000B}' => Some('\n'),
            '\u{FFFC}' | '\u{000E}' => None,
            c => Some(c),
        })
        .collect()
}

fn table_text(table: &iwork::TableInfo, room: usize) -> (String, usize) {
    let mut text = String::new();
    let mut cells = 0;
    for row in 0..table.rows {
        if cells >= room {
            break;
        }
        let values: Vec<String> = (0..table.columns)
            .filter_map(|column| table.cell(row, column).map(|cell| shown(&cell.value, &cell.format)))
            .filter(|value| !value.trim().is_empty())
            .map(|value| value.replace(['\n', '\r', '\t'], " "))
            .collect();
        cells += values.len();
        if !values.is_empty() {
            text.push_str(&values.join("\t"));
            text.push('\n');
        }
    }
    (text, cells)
}

pub fn pages(path: &Path, emit: Emit, words: &mut String) -> Result<(), String> {
    let book = Book::open(path)?;
    let body = book.doc.body_storage();
    let mut text = body.map(|body| lines_of(&book.storage_text(body))).unwrap_or_default();
    for storage in book.doc.text_storages() {
        if Some(storage.identifier) != body && text.len() < WORDS_CHARS * 4 {
            text.push('\n');
            text.push_str(&lines_of(&storage.text));
        }
    }
    for table in book.doc.tables() {
        text.push('\n');
        text.push_str(&table_text(&table, MAX_CELLS).0);
    }
    documents::send_passages(path, &text, |n| n as u64, MAX_PASSAGES, emit, words)
}

pub fn numbers(path: &Path, emit: Emit, words: &mut String) -> Result<(), String> {
    let book = Book::open(path)?;
    let tables = book.doc.tables();
    let elements = book.doc.elements();
    let mut text = String::new();
    let mut cells = 0;
    for sheet in book.doc.sheets() {
        if cells >= MAX_CELLS {
            break;
        }
        text.push_str(&sheet.name);
        text.push('\n');
        for id in &sheet.drawables {
            if let Some(table) = tables.iter().find(|table| table.identifier.get() == *id) {
                let (rows, read) = table_text(table, MAX_CELLS.saturating_sub(cells));
                cells += read;
                text.push_str(&table.name);
                text.push('\n');
                text.push_str(&rows);
            } else if let Some(storage) = elements
                .iter()
                .find(|element| element.identifier.get() == *id)
                .and_then(|element| element.text)
            {
                text.push_str(&lines_of(&book.storage_text(storage)));
                text.push('\n');
            }
        }
    }
    documents::send_passages(path, &text, |n| n as u64, MAX_PASSAGES, emit, words)
}

pub fn keynote(path: &Path, emit: Emit, words: &mut String) -> Result<(), String> {
    documents::send_sections(path, &slides_of(path)?, MAX_PASSAGES, emit, words)
}

pub fn keynote_slides(path: &Path, emit: Emit, words: &mut String) -> Result<(), String> {
    documents::send_each_section(path, &slides_of(path)?, emit, words)
}

fn slides_of(path: &Path) -> Result<Vec<(u64, String)>, String> {
    let book = Book::open(path)?;
    let mut slides = book.doc.slides();
    slides.sort_by_key(|slide| slide.index);
    let elements = book.doc.elements();
    let tables = book.doc.tables();
    let mut sections = Vec::new();
    let mut length = 0;
    for (n, slide) in slides.iter().enumerate() {
        if length >= WORDS_CHARS {
            break;
        }
        let mut text = String::new();
        let mut seen = HashSet::new();
        for element in elements
            .iter()
            .filter(|element| element.parent == Some(slide.identifier))
        {
            if let Some(storage) = element.text.filter(|storage| seen.insert(*storage)) {
                text.push_str(&lines_of(&book.storage_text(storage)));
                text.push('\n');
            }
            if let Some(table) = tables
                .iter()
                .find(|table| table.identifier.get() == element.identifier.get())
            {
                text.push_str(&table_text(table, MAX_CELLS).0);
            }
        }
        if let Some(note) = slide.note {
            let notes = Some(book.storage_text(note))
                .filter(|notes| !notes.is_empty())
                .or_else(|| note_text(&book, note))
                .unwrap_or_default();
            if !notes.trim().is_empty() {
                text.push('\n');
                text.push_str(&lines_of(&notes));
            }
        }
        length += text.chars().count();
        sections.push((n as u64 + 1, text));
    }
    Ok(sections)
}

fn note_text(book: &Book, note: u64) -> Option<String> {
    let (_, object) = book.doc.object(note)?;
    let archive = iwork::pb::Message::decode(object.payload()).ok()?;
    let storage = archive.bytes(1).and_then(iwork::pb::decode_nested)?.varint(1)?;
    Some(book.storage_text(storage))
}
