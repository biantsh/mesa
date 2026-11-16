use std::{cell::RefCell, collections::HashMap, path::Path, rc::Rc};

use iwork::{
    CellFormat, CellValue,
    pb::{Message, Value, decode_nested},
    style::get_path,
};

const STORAGE: u32 = 2001;
const CHARACTER_STYLE: u32 = 2021;
const PARAGRAPH_STYLE: u32 = 2022;
const LIST_STYLE: u32 = 2023;
const DRAWABLE_ATTACHMENT: u32 = 2003;

const PARAGRAPH_ENDS: [char; 5] = ['\n', '\r', '\u{000C}', '\u{0005}', '\u{0004}'];

pub struct Book {
    pub doc: iwork::Document,
    payloads: HashMap<u64, Vec<u8>>,
    decoded: RefCell<HashMap<u64, Option<Rc<Message>>>>,
    looks: RefCell<HashMap<(u64, u64), Rc<Look>>>,
    paragraph_looks: RefCell<HashMap<u64, Rc<ParagraphLook>>>,
}

fn why(error: iwork::Error) -> String {
    match error {
        iwork::Error::Encrypted { .. } => "it is locked with a password".into(),
        other => format!("it can't be read ({other})"),
    }
}

const MAX_PACKAGE_BYTES: u64 = 1024 * 1024 * 1024;

pub fn package(root: &Path, most: u64) -> Result<iwork::Package, String> {
    let (size, _) = crate::browse::package_state(root).ok_or("it can't be read")?;
    if size > most {
        return Err("it is too big to be read".into());
    }
    let mut package = iwork::Package::read_directory(root).map_err(why)?;
    if let Some(at) = package.entries.iter().position(|(name, _)| name == "Index.zip") {
        let (_, bytes) = package.entries.remove(at);
        for (name, data) in iwork::Package::from_bytes(&bytes).map_err(why)?.entries {
            let name = if name.starts_with("Index/") {
                name
            } else {
                format!("Index/{name}")
            };
            package.entries.push((name, data));
        }
    }
    Ok(package)
}

pub fn package_bytes(root: &Path, most: u64) -> Option<Vec<u8>> {
    use std::io::Write;
    let package = package(root, most).ok()?;
    let mut out = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    let stored = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .large_file(true);
    for (name, data) in &package.entries {
        zip.start_file(name.as_str(), stored).ok()?;
        zip.write_all(data).ok()?;
    }
    zip.finish().ok()?;
    Some(out.into_inner())
}

impl Book {
    pub fn open(path: &Path) -> Result<Self, String> {
        if path.is_dir() {
            return iwork::Document::from_package(package(path, MAX_PACKAGE_BYTES)?)
                .map(Self::of)
                .map_err(why);
        }
        iwork::Document::open(path).map(Self::of).map_err(why)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let package = iwork::Package::from_bytes(bytes).map_err(why)?;
        iwork::Document::from_package(package).map(Self::of).map_err(why)
    }

    fn of(doc: iwork::Document) -> Self {
        let payloads = doc
            .objects()
            .filter(|(_, object)| {
                matches!(
                    object.message_type(),
                    STORAGE | CHARACTER_STYLE | PARAGRAPH_STYLE | LIST_STYLE | DRAWABLE_ATTACHMENT
                )
            })
            .map(|(_, object)| (object.identifier, object.payload().to_vec()))
            .collect();
        Self {
            doc,
            payloads,
            decoded: RefCell::new(HashMap::new()),
            looks: RefCell::new(HashMap::new()),
            paragraph_looks: RefCell::new(HashMap::new()),
        }
    }

    fn message(&self, id: u64) -> Option<Rc<Message>> {
        if let Some(known) = self.decoded.borrow().get(&id) {
            return known.clone();
        }
        let message = self
            .payloads
            .get(&id)
            .and_then(|payload| Message::decode(payload).ok())
            .map(Rc::new);
        self.decoded.borrow_mut().insert(id, message.clone());
        message
    }

    pub fn storage_text(&self, id: u64) -> String {
        self.message(id)
            .map(|storage| {
                storage
                    .all(3)
                    .filter_map(|value| {
                        if let Value::Bytes(bytes) = value {
                            Some(String::from_utf8_lossy(bytes).into_owned())
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn table(storage: &Message, field: u32) -> Vec<(u64, Option<u64>)> {
        let Some(table) = storage.bytes(field).and_then(decode_nested) else {
            return Vec::new();
        };
        let anchoring = iwork::text::table(field).map_or(iwork::text::Anchoring::Run, |known| known.anchoring);
        iwork::text::entry_indices(&table, anchoring)
    }

    fn levels(storage: &Message) -> Vec<(u64, u64)> {
        let Some(table) = storage.bytes(6).and_then(decode_nested) else {
            return Vec::new();
        };
        table
            .all(1)
            .filter_map(|entry| {
                if let Value::Bytes(bytes) = entry {
                    decode_nested(bytes)
                } else {
                    None
                }
            })
            .filter_map(|entry| Some((entry.varint(1)?, entry.varint(2).unwrap_or(0))))
            .collect()
    }

    pub fn paragraphs(&self, id: u64) -> Vec<Paragraph> {
        let Some(storage) = self.message(id) else {
            return Vec::new();
        };
        let text = self.storage_text(id);
        let paragraph_styles = Self::table(&storage, 5);
        let list_styles = Self::table(&storage, 7);
        let character_styles = Self::table(&storage, 8);
        let levels = Self::levels(&storage);
        let attachments: HashMap<u64, u64> = Self::table(&storage, 9)
            .into_iter()
            .filter_map(|(at, attachment)| {
                Some((
                    at,
                    self.message(attachment?)?.bytes(1).and_then(decode_nested)?.varint(1)?,
                ))
            })
            .collect();
        let at_or_before = |entries: &[(u64, Option<u64>)], at: u64, keeps: bool| -> Option<u64> {
            let mut found = None;
            for (start, object) in entries {
                if *start > at {
                    break;
                }
                if object.is_some() || !keeps {
                    found = *object;
                }
            }
            found
        };

        let mut paragraphs = Vec::new();
        let mut current = Paragraph::default();
        let mut run = String::new();
        let mut run_style: Option<u64> = None;
        let (mut index, mut start) = (0u64, 0u64);
        let mut break_before = false;
        let mut started = false;
        for c in text.chars() {
            if !started {
                start = index;
                current = Paragraph {
                    style: at_or_before(&paragraph_styles, index, true),
                    list: at_or_before(&list_styles, index, true),
                    level: levels
                        .iter()
                        .rev()
                        .find(|(at, _)| *at <= index)
                        .map_or(0, |(_, level)| *level),
                    break_before: std::mem::take(&mut break_before),
                    ..Paragraph::default()
                };
                run_style = at_or_before(&character_styles, index, false);
                started = true;
            }
            let width = c.len_utf16() as u64;
            if PARAGRAPH_ENDS.contains(&c) {
                current.push(std::mem::take(&mut run), run_style, None);
                current.end = c;
                current.range = (start, index);
                paragraphs.push(std::mem::take(&mut current));
                break_before = c == '\u{000C}';
                started = false;
                index += width;
                continue;
            }
            let style_here = at_or_before(&character_styles, index, false);
            if style_here != run_style {
                current.push(std::mem::take(&mut run), run_style, None);
                run_style = style_here;
            }
            if c == '\u{FFFC}' {
                current.push(std::mem::take(&mut run), run_style, None);
                current.push(String::new(), run_style, attachments.get(&index).copied());
            } else {
                run.push(c);
            }
            index += width;
        }
        if started {
            current.push(run, run_style, None);
            current.range = (start, index);
            paragraphs.push(current);
        }
        paragraphs
    }

    fn property(&self, id: Option<u64>, path: &[u32]) -> Option<Value> {
        let mut next = id;
        for _ in 0..24 {
            let style = self.message(next?)?;
            if let Some(value) = get_path(&style, path) {
                return Some(value);
            }
            next = get_path(&style, &[1, 3, 1]).and_then(|value| {
                if let Value::Varint(parent) = value {
                    Some(parent)
                } else {
                    None
                }
            });
        }
        None
    }

    pub fn look(&self, paragraph: Option<u64>, character: Option<u64>) -> Rc<Look> {
        let key = (paragraph.unwrap_or(0), character.unwrap_or(0));
        if let Some(known) = self.looks.borrow().get(&key) {
            return Rc::clone(known);
        }
        let get = |path: &[u32]| {
            self.property(character, path)
                .or_else(|| self.property(paragraph, path))
        };
        let flag = |path: &[u32]| get(path).and_then(|value| number(&value)).map(|n| n as u64);
        let font = get(&[11, 5]).and_then(|value| text_of(&value));
        let look = Rc::new(Look {
            bold: flag(&[11, 1]) == Some(1)
                || font
                    .as_deref()
                    .is_some_and(|font| font.contains("Bold") || font.contains("Black") || font.contains("Heavy")),
            italic: flag(&[11, 2]) == Some(1)
                || font
                    .as_deref()
                    .is_some_and(|font| font.contains("Italic") || font.contains("Oblique")),
            size: get(&[11, 3])
                .and_then(|value| number(&value))
                .filter(|size| *size > 0.0),
            family: font.as_deref().map(family_of),
            color: get(&[11, 46, 1])
                .and_then(|value| color_of(&value))
                .or_else(|| get(&[11, 7]).and_then(|value| color_of(&value))),
            raised: match flag(&[11, 10]) {
                Some(1) => 1,
                Some(2) => -1,
                _ => 0,
            },
            underline: flag(&[11, 11]).is_some_and(|underline| underline > 0),
            strike: flag(&[11, 12]).is_some_and(|strike| strike > 0),
            caps: flag(&[11, 13]).unwrap_or(0) as u8,
            background: get(&[11, 26]).and_then(|value| color_of(&value)),
        });
        self.looks.borrow_mut().insert(key, Rc::clone(&look));
        look
    }

    pub fn paragraph_look(&self, style: Option<u64>) -> Rc<ParagraphLook> {
        let key = style.unwrap_or(0);
        if let Some(known) = self.paragraph_looks.borrow().get(&key) {
            return Rc::clone(known);
        }
        let get = |path: &[u32]| self.property(style, path).and_then(|value| number(&value));
        let look = Rc::new(ParagraphLook {
            alignment: get(&[12, 1]).map_or(0, |alignment| alignment as u8),
            first_line: get(&[12, 7]).unwrap_or(0.0),
            left: get(&[12, 11]).unwrap_or(0.0),
            right: get(&[12, 19]).unwrap_or(0.0),
            space_before: get(&[12, 21]).unwrap_or(0.0),
            space_after: get(&[12, 20]).unwrap_or(0.0),
            line_spacing: get(&[12, 13, 2]).filter(|spacing| *spacing > 0.1 && *spacing < 10.0),
            page_break_before: get(&[12, 14]).is_some_and(|flag| flag != 0.0),
            keep_with_next: get(&[12, 10]).is_some_and(|flag| flag != 0.0),
            background: self.property(style, &[12, 6]).and_then(|value| color_of(&value)),
        });
        self.paragraph_looks.borrow_mut().insert(key, Rc::clone(&look));
        look
    }

    pub fn list_label(&self, style: Option<u64>, level: u64) -> Option<String> {
        let Some(style) = style.and_then(|style| self.message(style)) else {
            return Some(String::new());
        };
        let at = level as usize;
        let kinds: Vec<u64> = match style.get(11) {
            Some(Value::Bytes(packed)) => varints(packed),
            _ => style
                .all(11)
                .filter_map(|value| {
                    if let Value::Varint(kind) = value {
                        Some(*kind)
                    } else {
                        None
                    }
                })
                .collect(),
        };
        let bullets: Vec<String> = style.all(16).filter_map(text_of).collect();
        match kinds.get(at).or(kinds.last()).copied().unwrap_or(0) {
            2 => Some(
                bullets
                    .get(at)
                    .or(bullets.last())
                    .cloned()
                    .unwrap_or_else(|| "•".into()),
            ),
            3 => None,
            _ => Some(String::new()),
        }
    }
}

impl Book {
    pub fn list_indent(&self, style: Option<u64>, level: u64) -> Option<f64> {
        let style = self.message(style?)?;
        let indents: Vec<f64> = match style.get(13) {
            Some(Value::Bytes(packed)) => packed
                .as_chunks::<4>()
                .0
                .iter()
                .map(|bytes| f64::from(f32::from_le_bytes(*bytes)))
                .collect(),
            _ => style.all(13).filter_map(number).collect(),
        };
        indents
            .get(level as usize)
            .or(indents.last())
            .copied()
            .filter(|indent| indent.is_finite() && *indent >= 0.0 && *indent < 2000.0)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Run {
    pub text: String,
    pub style: Option<u64>,
    pub attachment: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct Paragraph {
    pub runs: Vec<Run>,
    pub style: Option<u64>,
    pub list: Option<u64>,
    pub level: u64,
    pub break_before: bool,
    pub end: char,
    pub range: (u64, u64),
}

impl Paragraph {
    fn push(&mut self, text: String, style: Option<u64>, attachment: Option<u64>) {
        if !text.is_empty() || attachment.is_some() {
            self.runs.push(Run {
                text,
                style,
                attachment,
            });
        }
    }

    pub fn text(&self) -> String {
        self.runs.iter().map(|run| run.text.as_str()).collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Look {
    pub bold: bool,
    pub italic: bool,
    pub size: Option<f64>,
    pub family: Option<String>,
    pub color: Option<[u8; 3]>,
    pub raised: i8,
    pub underline: bool,
    pub strike: bool,
    pub caps: u8,
    pub background: Option<[u8; 3]>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParagraphLook {
    pub alignment: u8,
    pub first_line: f64,
    pub left: f64,
    pub right: f64,
    pub space_before: f64,
    pub space_after: f64,
    pub line_spacing: Option<f64>,
    pub page_break_before: bool,
    pub keep_with_next: bool,
    pub background: Option<[u8; 3]>,
}

fn number(value: &Value) -> Option<f64> {
    Some(match value {
        Value::Fixed32(bytes) => f64::from(f32::from_le_bytes(*bytes)),
        Value::Fixed64(bytes) => f64::from_le_bytes(*bytes),
        Value::Varint(n) => *n as f64,
        Value::Bytes(_) => return None,
    })
    .filter(|n| n.is_finite())
}

fn text_of(value: &Value) -> Option<String> {
    if let Value::Bytes(bytes) = value {
        std::str::from_utf8(bytes).ok().map(str::to_owned)
    } else {
        None
    }
}

fn color_of(value: &Value) -> Option<[u8; 3]> {
    let Value::Bytes(bytes) = value else { return None };
    let color = decode_nested(bytes)?;
    let channel = |field| {
        color
            .get(field)
            .and_then(number)
            .map(|n| (n.clamp(0.0, 1.0) * 255.0).round() as u8)
    };
    Some([channel(3)?, channel(4)?, channel(5)?])
}

fn varints(bytes: &[u8]) -> Vec<u64> {
    let mut found = Vec::new();
    let (mut value, mut shift) = (0u64, 0u32);
    for byte in bytes {
        value |= u64::from(byte & 0x7F) << shift.min(63);
        shift += 7;
        if byte & 0x80 == 0 {
            found.push(value);
            (value, shift) = (0, 0);
        }
    }
    found
}

pub fn family_of(postscript: &str) -> String {
    let base = postscript.split('-').next().unwrap_or(postscript);
    let base = base
        .strip_suffix("PSMT")
        .or_else(|| base.strip_suffix("MT"))
        .or_else(|| base.strip_suffix("PS"))
        .unwrap_or(base);
    let mut family = String::new();
    let mut last: Option<char> = None;
    for c in base.chars() {
        if c.is_uppercase() && last.is_some_and(char::is_lowercase) {
            family.push(' ');
        }
        family.push(c);
        last = Some(c);
    }
    if family.is_empty() {
        postscript.to_owned()
    } else {
        family
    }
}

pub fn shown(value: &CellValue, format: &CellFormat) -> String {
    match value {
        CellValue::Empty | CellValue::Span => String::new(),
        CellValue::Text(text) | CellValue::RichText(text) => text.trim_end_matches(['\n', '\r']).to_owned(),
        CellValue::Bool(true) => "TRUE".into(),
        CellValue::Bool(false) => "FALSE".into(),
        CellValue::Number(number) | CellValue::Currency(number) => {
            let is_currency = matches!(value, CellValue::Currency(_)) || *format == CellFormat::Currency;
            let shown = number.to_string();
            let Ok(amount) = shown.parse::<f64>() else { return shown };
            if *format == CellFormat::Percentage {
                format!("{}%", trimmed(amount * 100.0, 2))
            } else if is_currency {
                grouped(amount, 2)
            } else {
                shown
            }
        }
        CellValue::Date(seconds) => {
            let stamp = iwork::table::format_date(*seconds);
            match stamp.strip_suffix("T00:00:00Z") {
                Some(day) => day.to_owned(),
                None => stamp.replace('T', " ").trim_end_matches('Z').to_owned(),
            }
        }
        CellValue::Duration(seconds) => duration(*seconds),
        CellValue::Error => "#ERROR".into(),
        CellValue::Unknown(_) => String::new(),
    }
}

fn trimmed(value: f64, places: usize) -> String {
    let shown = format!("{value:.places$}");
    if shown.contains('.') {
        shown.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        shown
    }
}

fn grouped(value: f64, places: usize) -> String {
    let shown = format!("{:.places$}", value.abs());
    let (whole, part) = shown.split_once('.').unwrap_or((&shown, ""));
    let mut digits = String::new();
    for (at, digit) in whole.chars().enumerate() {
        if at > 0 && (whole.len() - at).is_multiple_of(3) {
            digits.push(',');
        }
        digits.push(digit);
    }
    let sign = if value < 0.0 { "-" } else { "" };
    if part.is_empty() {
        format!("{sign}{digits}")
    } else {
        format!("{sign}{digits}.{part}")
    }
}

fn duration(seconds: f64) -> String {
    let total = seconds.round() as i64;
    let (hours, minutes, rest) = (total / 3600, (total % 3600) / 60, total % 60);
    let mut parts = Vec::new();
    if hours != 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes != 0 {
        parts.push(format!("{minutes}m"));
    }
    if rest != 0 || parts.is_empty() {
        parts.push(format!("{rest}s"));
    }
    parts.join(" ")
}
