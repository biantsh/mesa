use std::{
    borrow::Cow,
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

const MAX_PART_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CELLS: usize = WORDS_CHARS / 4;
const COMPOUND_FILE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

pub fn xlsx(path: &Path, emit: Emit, words: &mut String) -> Result<(), String> {
    let mut package = Package::open(path, "an Excel workbook")?;
    let workbook = package.main_part("xl/workbook.xml");
    let relationships = package.relationships(&workbook);
    let sheets = sheets_of(&mut package, &workbook, &relationships)?;

    let mut read = Vec::new();
    let mut cells = 0;
    for (name, part) in sheets {
        if cells >= MAX_CELLS {
            break;
        }
        let (rows, taken) = rows_of(&mut package, &part, MAX_CELLS - cells);
        cells += taken;
        if !rows.is_empty() {
            read.push((name, rows));
        }
    }

    let last_shared = read
        .iter()
        .flat_map(|(_, rows)| rows.iter().flatten())
        .filter_map(|cell| match cell {
            Cell::Shared(index) => Some(*index),
            Cell::Value(_) => None,
        });
    let strings = match last_shared.max() {
        Some(last) => {
            let part = relationships
                .iter()
                .find(|relationship| relationship.kind.ends_with("/sharedStrings"))
                .map_or_else(
                    || "xl/sharedStrings.xml".to_owned(),
                    |relationship| relationship.target.clone(),
                );
            shared_strings(&mut package, &part, last)
        }
        None => Vec::new(),
    };

    let mut text = String::new();
    for (name, rows) in read {
        text.push_str(&name);
        text.push('\n');
        for row in rows {
            let values: Vec<&str> = row
                .iter()
                .filter_map(|cell| match cell {
                    Cell::Shared(index) => strings.get(*index).map(String::as_str),
                    Cell::Value(value) => Some(value.as_str()),
                })
                .filter(|value| !value.trim().is_empty())
                .collect();
            text.push_str(&values.join("\t"));
            text.push('\n');
        }
    }
    documents::send_passages(path, &text, |n| n as u64, MAX_PASSAGES, emit, words)
}

pub fn pptx(path: &Path, emit: Emit, words: &mut String) -> Result<(), String> {
    documents::send_sections(path, &slides_of(path)?, MAX_PASSAGES, emit, words)
}

pub fn pptx_slides(path: &Path, emit: Emit, words: &mut String) -> Result<(), String> {
    documents::send_each_section(path, &slides_of(path)?, emit, words)
}

fn slides_of(path: &Path) -> Result<Vec<(u64, String)>, String> {
    let mut package = Package::open(path, "a PowerPoint presentation")?;
    let presentation = package.main_part("ppt/presentation.xml");
    let slides = slide_ids(&mut package, &presentation)?;
    let relationships = package.relationships(&presentation);

    let mut sections = Vec::new();
    let mut length = 0;
    for (number, id) in slides.iter().enumerate() {
        if length >= WORDS_CHARS {
            break;
        }
        let Some(slide) = relationships.iter().find(|relationship| &relationship.id == id) else {
            continue;
        };
        let mut text = slide_text(&mut package, &slide.target);
        let notes = package
            .relationships(&slide.target)
            .into_iter()
            .find(|relationship| relationship.kind.ends_with("/notesSlide"));
        if let Some(notes) = notes {
            text.push('\n');
            text.push_str(&slide_text(&mut package, &notes.target));
        }
        length += text.chars().count();
        sections.push((number as u64 + 1, text));
    }
    Ok(sections)
}

struct Package {
    archive: ZipArchive<BufReader<File>>,
    what: &'static str,
}

struct Relationship {
    id: String,
    kind: String,
    target: String,
}

impl Package {
    fn open(path: &Path, what: &'static str) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| format!("can't read the file: {e}"))?;
        match ZipArchive::new(BufReader::new(file)) {
            Ok(archive) => Ok(Self { archive, what }),
            Err(_) if starts_with(path, &COMPOUND_FILE) => {
                Err("it is locked with a password, or is in the older format of before 2007 with a newer name".into())
            }
            Err(e) => Err(format!("not {what}: {e}")),
        }
    }

    fn part(&mut self, name: &str) -> Result<Reader<Box<dyn BufRead + '_>>, String> {
        let file = self
            .archive
            .by_name(name)
            .map_err(|e| format!("not {} (no {name} in it: {e})", self.what))?;
        Ok(Reader::from_reader(Box::new(BufReader::new(file.take(MAX_PART_BYTES)))))
    }

    fn main_part(&mut self, usual: &str) -> String {
        self.relationships("")
            .into_iter()
            .find(|relationship| relationship.kind.ends_with("/officeDocument"))
            .map_or_else(|| usual.to_owned(), |relationship| relationship.target)
    }

    fn relationships(&mut self, part: &str) -> Vec<Relationship> {
        let (folder, name) = part.rsplit_once('/').unwrap_or(("", part));
        let file = if folder.is_empty() {
            format!("_rels/{name}.rels")
        } else {
            format!("{folder}/_rels/{name}.rels")
        };
        let mut found = Vec::new();
        let Ok(mut reader) = self.part(&file) else { return found };
        let mut buffer = Vec::new();
        while let Ok(event) = reader.read_event_into(&mut buffer) {
            match event {
                Event::Start(tag) | Event::Empty(tag) if tag.local_name().as_ref() == "Relationship" => {
                    let external = attribute(&tag, "TargetMode").is_some_and(|mode| mode == "External");
                    if let (false, Some(id), Some(kind), Some(target)) = (
                        external,
                        attribute(&tag, "Id"),
                        attribute(&tag, "Type"),
                        attribute(&tag, "Target"),
                    ) {
                        found.push(Relationship {
                            id,
                            kind,
                            target: resolve(folder, &target),
                        });
                    }
                }
                Event::Eof => break,
                _ => {}
            }
            buffer.clear();
        }
        found
    }
}

fn starts_with(path: &Path, bytes: &[u8]) -> bool {
    let mut start = vec![0; bytes.len()];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut start))
        .is_ok()
        && start == bytes
}

fn resolve(folder: &str, target: &str) -> String {
    let target = target.replace('\\', "/");
    let (mut parts, rest): (Vec<&str>, &str) = match target.strip_prefix('/') {
        Some(rest) => (Vec::new(), rest),
        None => (folder.split('/').filter(|part| !part.is_empty()).collect(), &target),
    };
    for piece in rest.split('/') {
        match piece {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            piece => parts.push(piece),
        }
    }
    parts.join("/")
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
                .map(Cow::into_owned)
        })
}

fn relationship_id(tag: &BytesStart) -> Option<String> {
    tag.attributes()
        .with_checks(false)
        .flatten()
        .find(|attribute| attribute.key.prefix().is_some() && attribute.key.local_name().as_ref() == "id")
        .and_then(|attribute| {
            attribute
                .normalized_value(XmlVersion::Implicit1_0)
                .ok()
                .map(Cow::into_owned)
        })
}

enum Cell {
    Shared(usize),
    Value(String),
}

fn sheets_of(
    package: &mut Package,
    workbook: &str,
    relationships: &[Relationship],
) -> Result<Vec<(String, String)>, String> {
    let mut reader = package.part(workbook)?;
    let mut sheets = Vec::new();
    let mut buffer = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buffer)
            .map_err(|e| format!("can't read the workbook: {e}"))?
        {
            Event::Start(tag) | Event::Empty(tag) if tag.local_name().as_ref() == "sheet" => {
                let hidden = attribute(&tag, "state").is_some_and(|state| state != "visible");
                let sheet = relationship_id(&tag)
                    .and_then(|id| relationships.iter().find(|relationship| relationship.id == id));
                if let (false, Some(sheet)) = (hidden, sheet.filter(|sheet| sheet.kind.ends_with("/worksheet"))) {
                    sheets.push((attribute(&tag, "name").unwrap_or_default(), sheet.target.clone()));
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Ok(sheets)
}

fn rows_of(package: &mut Package, part: &str, room: usize) -> (Vec<Vec<Cell>>, usize) {
    let (mut rows, mut row, mut taken) = (Vec::new(), Vec::new(), 0);
    let Ok(mut reader) = package.part(part) else {
        return (rows, taken);
    };
    let (mut kind, mut value, mut inline) = (String::new(), String::new(), String::new());
    let (mut in_value, mut in_text, mut phonetic) = (false, false, 0usize);
    let mut buffer = Vec::new();
    while let Ok(event) = reader.read_event_into(&mut buffer) {
        match event {
            Event::Start(tag) => match tag.local_name().as_ref() {
                "c" => {
                    kind = attribute(&tag, "t").unwrap_or_default();
                    value.clear();
                    inline.clear();
                }
                "v" => in_value = true,
                "t" => in_text = true,
                "rPh" => phonetic += 1,
                _ => {}
            },
            Event::End(tag) => match tag.local_name().as_ref() {
                "v" => in_value = false,
                "t" => in_text = false,
                "rPh" => phonetic = phonetic.saturating_sub(1),
                "c" => {
                    if let Some(cell) = cell(&kind, &value, &inline) {
                        row.push(cell);
                        taken += 1;
                    }
                }
                "row" => {
                    if !row.is_empty() {
                        rows.push(std::mem::take(&mut row));
                    }
                    if taken >= room {
                        break;
                    }
                }
                _ => {}
            },
            Event::Text(content) if in_value => value.push_str(&content.xml10_content()),
            Event::Text(content) if in_text && phonetic == 0 => inline.push_str(&content.xml10_content()),
            Event::GeneralRef(reference) if in_value => value.extend(documents::entity(&reference)),
            Event::GeneralRef(reference) if in_text && phonetic == 0 => inline.extend(documents::entity(&reference)),
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !row.is_empty() {
        rows.push(row);
    }
    (rows, taken)
}

fn cell(kind: &str, value: &str, inline: &str) -> Option<Cell> {
    let cell = match kind {
        "s" => Cell::Shared(value.trim().parse().ok()?),
        "inlineStr" => Cell::Value(inline.to_owned()),
        "b" | "e" => return None,
        _ => Cell::Value(value.to_owned()),
    };
    match &cell {
        Cell::Value(value) if value.trim().is_empty() => None,
        _ => Some(cell),
    }
}

fn shared_strings(package: &mut Package, part: &str, last: usize) -> Vec<String> {
    let mut strings = Vec::new();
    let Ok(mut reader) = package.part(part) else {
        return strings;
    };
    let (mut current, mut in_text, mut phonetic) = (String::new(), false, 0usize);
    let mut buffer = Vec::new();
    while let Ok(event) = reader.read_event_into(&mut buffer) {
        match event {
            Event::Start(tag) => match tag.local_name().as_ref() {
                "si" => current.clear(),
                "t" => in_text = true,
                "rPh" => phonetic += 1,
                _ => {}
            },
            Event::Empty(tag) if tag.local_name().as_ref() == "si" => strings.push(String::new()),
            Event::End(tag) => match tag.local_name().as_ref() {
                "t" => in_text = false,
                "rPh" => phonetic = phonetic.saturating_sub(1),
                "si" => strings.push(std::mem::take(&mut current)),
                _ => {}
            },
            Event::Text(content) if in_text && phonetic == 0 => current.push_str(&content.xml10_content()),
            Event::GeneralRef(reference) if in_text && phonetic == 0 => current.extend(documents::entity(&reference)),
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
        if strings.len() > last {
            break;
        }
    }
    strings
}

fn slide_ids(package: &mut Package, presentation: &str) -> Result<Vec<String>, String> {
    let mut reader = package.part(presentation)?;
    let mut ids = Vec::new();
    let mut buffer = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buffer)
            .map_err(|e| format!("can't read the presentation: {e}"))?
        {
            Event::Start(tag) | Event::Empty(tag) if tag.local_name().as_ref() == "sldId" => {
                ids.extend(relationship_id(&tag))
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Ok(ids)
}

fn slide_text(package: &mut Package, part: &str) -> String {
    let mut text = String::new();
    let Ok(mut reader) = package.part(part) else {
        return text;
    };
    let (mut in_text, mut field) = (false, 0usize);
    let mut buffer = Vec::new();
    while let Ok(event) = reader.read_event_into(&mut buffer) {
        match event {
            Event::Start(tag) => match tag.local_name().as_ref() {
                "t" => in_text = true,
                "fld" => field += 1,
                _ => {}
            },
            Event::Empty(tag) if tag.local_name().as_ref() == "br" => text.push('\n'),
            Event::End(tag) => match tag.local_name().as_ref() {
                "t" => in_text = false,
                "fld" => field = field.saturating_sub(1),
                "p" => text.push('\n'),
                _ => {}
            },
            Event::Text(content) if in_text && field == 0 => text.push_str(&content.xml10_content()),
            Event::GeneralRef(reference) if in_text && field == 0 => text.extend(documents::entity(&reference)),
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    text
}
