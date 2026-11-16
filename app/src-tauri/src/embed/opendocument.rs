use std::{
    collections::HashSet,
    fs::File,
    io::{BufRead, BufReader, Read},
    path::Path,
};

use quick_xml::{
    Reader, XmlVersion,
    events::{BytesStart, Event},
};
use zip::ZipArchive;

use super::documents::{self, Emit, MAX_PASSAGES, WORDS_CHARS};

const MAX_CONTENT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_CELLS: usize = WORDS_CHARS / 4;
const MOST_REPEATS: usize = 64;

const LEFT_OUT: [&str; 18] = [
    "office:annotation",
    "text:note",
    "text:tracked-changes",
    "text:table-of-content-source",
    "text:alphabetical-index-source",
    "text:illustration-index-source",
    "text:table-index-source",
    "text:object-index-source",
    "text:user-index-source",
    "text:bibliography-source",
    "svg:title",
    "svg:desc",
    "office:automatic-styles",
    "office:font-face-decls",
    "office:scripts",
    "office:forms",
    "text:sequence-decls",
    "text:user-field-decls",
];

pub fn odt(path: &Path, emit: Emit, words: &mut String) -> Result<(), String> {
    let text = with_content(path, "an OpenDocument text", |reader| {
        let mut text = String::new();
        let mut reading = Reading::default();
        let mut buffer = Vec::new();
        loop {
            let event = reader
                .read_event_into(&mut buffer)
                .map_err(|e| format!("can't read the document: {e}"))?;
            if matches!(event, Event::Eof) || text.len() > WORDS_CHARS * 4 {
                break;
            }
            reading.read(&event, &mut text);
            buffer.clear();
        }
        Ok(text)
    })?;
    documents::send_passages(path, &text, |n| n as u64, MAX_PASSAGES, emit, words)
}

pub fn ods(path: &Path, emit: Emit, words: &mut String) -> Result<(), String> {
    let text = with_content(path, "an OpenDocument spreadsheet", |reader| Ok(sheets_text(reader)))?;
    documents::send_passages(path, &text, |n| n as u64, MAX_PASSAGES, emit, words)
}

pub fn odp(path: &Path, emit: Emit, words: &mut String) -> Result<(), String> {
    documents::send_sections(path, &slides_of(path)?, MAX_PASSAGES, emit, words)
}

pub fn odp_slides(path: &Path, emit: Emit, words: &mut String) -> Result<(), String> {
    documents::send_each_section(path, &slides_of(path)?, emit, words)
}

fn with_content<T>(
    path: &Path,
    what: &str,
    read: impl FnOnce(&mut Reader<Box<dyn BufRead + '_>>) -> Result<T, String>,
) -> Result<T, String> {
    let file = File::open(path).map_err(|e| format!("can't read the file: {e}"))?;
    let mut archive = ZipArchive::new(BufReader::new(file)).map_err(|e| format!("not {what}: {e}"))?;
    if is_encrypted(&mut archive) {
        return Err("it is locked with a password".into());
    }
    let content = archive
        .by_name("content.xml")
        .map_err(|e| format!("not {what} (no content.xml in it: {e})"))?;
    let mut reader: Reader<Box<dyn BufRead + '_>> =
        Reader::from_reader(Box::new(BufReader::new(content.take(MAX_CONTENT_BYTES))));
    read(&mut reader)
}

pub(crate) fn is_encrypted<R: Read + std::io::Seek>(archive: &mut ZipArchive<R>) -> bool {
    let mut manifest = String::new();
    archive
        .by_name("META-INF/manifest.xml")
        .is_ok_and(|file| file.take(4 * 1024 * 1024).read_to_string(&mut manifest).is_ok())
        && manifest.contains("encryption-data")
}

fn attribute(tag: &BytesStart, name: &str) -> Option<String> {
    tag.attributes()
        .with_checks(false)
        .flatten()
        .find(|attribute| attribute.key.as_ref() == name)
        .and_then(|attribute| {
            attribute
                .normalized_value(XmlVersion::Implicit1_0)
                .ok()
                .map(|value| value.into_owned())
        })
}

fn name_of(tag: &BytesStart) -> String {
    tag.name().as_ref().to_owned()
}

#[derive(Default)]
struct Reading {
    paragraphs: usize,
    skipped: usize,
}

impl Reading {
    fn read(&mut self, event: &Event, text: &mut String) {
        match event {
            Event::Start(tag) => {
                let name = name_of(tag);
                if self.skipped > 0 || LEFT_OUT.contains(&name.as_str()) {
                    self.skipped += 1;
                } else if name == "text:p" || name == "text:h" {
                    self.paragraphs += 1;
                } else if self.paragraphs > 0 {
                    blank(&name, tag, text);
                }
            }
            Event::Empty(tag) if self.skipped == 0 => {
                let name = name_of(tag);
                if name == "text:p" || name == "text:h" {
                    text.push('\n');
                } else if self.paragraphs > 0 {
                    blank(&name, tag, text);
                }
            }
            Event::End(tag) => {
                if self.skipped > 0 {
                    self.skipped -= 1;
                } else if matches!(tag.name().as_ref(), "text:p" | "text:h") {
                    self.paragraphs = self.paragraphs.saturating_sub(1);
                    text.push('\n');
                }
            }
            Event::Text(content) if self.skipped == 0 && self.paragraphs > 0 => {
                for c in content.xml10_content().chars() {
                    if !c.is_whitespace() {
                        text.push(c);
                    } else if !text.is_empty() && !text.ends_with(char::is_whitespace) {
                        text.push(' ');
                    }
                }
            }
            Event::GeneralRef(reference) if self.skipped == 0 && self.paragraphs > 0 => {
                text.extend(documents::entity(reference))
            }
            _ => {}
        }
    }
}

fn blank(name: &str, tag: &BytesStart, text: &mut String) {
    match name {
        "text:s" => {
            let count = attribute(tag, "text:c")
                .and_then(|count| count.parse::<usize>().ok())
                .unwrap_or(1);
            text.extend(std::iter::repeat_n(' ', count.min(64)));
        }
        "text:tab" => text.push('\t'),
        "text:line-break" => text.push('\n'),
        _ => {}
    }
}

fn repeats(tag: &BytesStart, name: &str) -> usize {
    attribute(tag, name)
        .and_then(|count| count.parse::<usize>().ok())
        .unwrap_or(1)
        .max(1)
}

fn sheets_text(reader: &mut Reader<Box<dyn BufRead + '_>>) -> String {
    let mut text = String::new();
    let mut hidden = HashSet::new();
    let mut style = None::<String>;
    let (mut shown, mut row, mut cell, mut cells) = (false, Vec::<String>::new(), String::new(), 0usize);
    let (mut row_repeats, mut cell_repeats) = (1, 1);
    let mut in_cell = false;
    let mut cell_reading = Reading::default();
    let mut buffer = Vec::new();
    let end_row = |row: &mut Vec<String>, repeats: usize, text: &mut String, cells: &mut usize| {
        if !row.is_empty() {
            for _ in 0..repeats.min(MOST_REPEATS) {
                text.push_str(&row.join("\t"));
                text.push('\n');
                *cells += row.len();
            }
        }
        row.clear();
    };
    while let Ok(event) = reader.read_event_into(&mut buffer) {
        if cells >= MAX_CELLS {
            break;
        }
        match &event {
            Event::Eof => break,
            Event::Start(tag) if tag.name().as_ref() == "style:style" => {
                style = (attribute(tag, "style:family").as_deref() == Some("table"))
                    .then(|| attribute(tag, "style:name"))
                    .flatten();
            }
            Event::Start(tag) | Event::Empty(tag) if tag.name().as_ref() == "style:table-properties" => {
                if let Some(style) = &style
                    && attribute(tag, "table:display").as_deref() == Some("false")
                {
                    hidden.insert(style.clone());
                }
            }
            Event::Start(tag) if tag.name().as_ref() == "table:table" && !in_cell => {
                shown = !attribute(tag, "table:style-name").is_some_and(|style| hidden.contains(&style));
                if shown {
                    text.push_str(&attribute(tag, "table:name").unwrap_or_default());
                    text.push('\n');
                }
            }
            Event::Start(tag) | Event::Empty(tag) if tag.name().as_ref() == "table:table-row" && shown && !in_cell => {
                row.clear();
                row_repeats = repeats(tag, "table:number-rows-repeated");
            }
            Event::End(tag) if tag.name().as_ref() == "table:table-row" && shown && !in_cell => {
                end_row(&mut row, row_repeats, &mut text, &mut cells);
            }
            Event::Start(tag)
                if matches!(tag.name().as_ref(), "table:table-cell" | "table:covered-table-cell")
                    && shown
                    && !in_cell =>
            {
                in_cell = true;
                cell.clear();
                cell_repeats = repeats(tag, "table:number-columns-repeated");
                cell_reading = Reading::default();
            }
            Event::End(tag)
                if matches!(tag.name().as_ref(), "table:table-cell" | "table:covered-table-cell")
                    && in_cell
                    && cell_reading.skipped == 0 =>
            {
                in_cell = false;
                let value = cell.split_whitespace().collect::<Vec<_>>().join(" ");
                if !value.is_empty() {
                    row.extend(std::iter::repeat_n(value, cell_repeats.min(MOST_REPEATS)));
                }
            }
            _ if in_cell => cell_reading.read(&event, &mut cell),
            _ => {}
        }
        buffer.clear();
    }
    end_row(&mut row, row_repeats, &mut text, &mut cells);
    text
}

fn slides_of(path: &Path) -> Result<Vec<(u64, String)>, String> {
    with_content(path, "an OpenDocument presentation", |reader| {
        let mut sections = Vec::new();
        let (mut slide, mut notes, mut length) = (String::new(), String::new(), 0);
        let (mut in_slide, mut in_notes) = (false, 0usize);
        let mut reading = Reading::default();
        let mut buffer = Vec::new();
        loop {
            let event = reader
                .read_event_into(&mut buffer)
                .map_err(|e| format!("can't read the presentation: {e}"))?;
            match &event {
                Event::Eof => break,
                Event::Start(tag) if tag.name().as_ref() == "draw:page" && !in_slide => {
                    in_slide = true;
                    slide.clear();
                    notes.clear();
                    reading = Reading::default();
                }
                Event::End(tag) if tag.name().as_ref() == "draw:page" && in_slide && reading.skipped == 0 => {
                    in_slide = false;
                    let mut text = std::mem::take(&mut slide);
                    if !notes.trim().is_empty() {
                        text.push('\n');
                        text.push_str(&notes);
                    }
                    length += text.chars().count();
                    sections.push((sections.len() as u64 + 1, text));
                    if length >= WORDS_CHARS {
                        break;
                    }
                }
                Event::Start(tag)
                    if tag.name().as_ref() == "presentation:notes" && in_slide && reading.skipped == 0 =>
                {
                    in_notes += 1
                }
                Event::End(tag)
                    if tag.name().as_ref() == "presentation:notes" && in_notes > 0 && reading.skipped == 0 =>
                {
                    in_notes -= 1
                }
                _ if in_slide => reading.read(&event, if in_notes > 0 { &mut notes } else { &mut slide }),
                _ => {}
            }
            buffer.clear();
        }
        Ok(sections)
    })
}
