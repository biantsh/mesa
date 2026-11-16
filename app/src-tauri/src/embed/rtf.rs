use std::{fs::File, io::Read, path::Path};

use encoding_rs::Encoding;

use super::documents::{self, Emit, MAX_PASSAGES, WORDS_CHARS};

const MAX_RTF_BYTES: u64 = 64 * 1024 * 1024;
const ENOUGH_BYTES: usize = WORDS_CHARS * 4;

const SKIPPED: &[&str] = &[
    "fonttbl",
    "colortbl",
    "stylesheet",
    "info",
    "pict",
    "object",
    "header",
    "headerl",
    "headerr",
    "headerf",
    "footer",
    "footerl",
    "footerr",
    "footerf",
    "fldinst",
    "themedata",
    "colorschememapping",
    "datastore",
    "latentstyles",
    "listtable",
    "listoverridetable",
    "rsidtbl",
    "generator",
    "xmlnstbl",
    "filetbl",
    "revtbl",
    "pgdsctbl",
    "nonshppict",
    "bkmkstart",
    "bkmkend",
];

pub fn rtf(path: &Path, emit: Emit, words: &mut String) -> Result<(), String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_RTF_BYTES).read_to_end(&mut bytes))
        .map_err(|e| format!("can't read the file: {e}"))?;
    documents::send_passages(path, &text_of(&bytes)?, |n| n as u64, MAX_PASSAGES, emit, words)
}

#[derive(Clone, Copy)]
struct Group {
    skipped: bool,
    fallback: usize,
}

fn text_of(bytes: &[u8]) -> Result<String, String> {
    if !bytes.trim_ascii_start().starts_with(b"{\\rtf") {
        return Err("not an RTF document".into());
    }
    let mut reader = Text {
        text: String::new(),
        pending: Vec::new(),
        encoding: encoding_rs::WINDOWS_1252,
        high_surrogate: None,
    };
    let mut groups = vec![Group {
        skipped: false,
        fallback: 1,
    }];
    let mut to_skip = 0;
    let mut at = 0;
    while at < bytes.len() && reader.text.len() < ENOUGH_BYTES {
        let group = *groups.last().unwrap_or(&Group {
            skipped: false,
            fallback: 1,
        });
        match bytes[at] {
            b'{' => {
                groups.push(group);
                to_skip = 0;
                at += 1;
            }
            b'}' => {
                if groups.len() > 1 {
                    groups.pop();
                }
                to_skip = 0;
                at += 1;
            }
            b'\r' | b'\n' => at += 1,
            b'\\' => {
                let Some(&next) = bytes.get(at + 1) else { break };
                if next.is_ascii_alphabetic() {
                    let (word, parameter, after) = control_word(bytes, at + 1);
                    at = after;
                    if word == "bin" {
                        at = at.saturating_add(parameter.unwrap_or(0).max(0) as usize);
                        continue;
                    }
                    if group.skipped {
                        continue;
                    }
                    if SKIPPED.contains(&word) {
                        set_skipped(&mut groups);
                        continue;
                    }
                    match word {
                        "uc" => {
                            if let Some(group) = groups.last_mut() {
                                group.fallback = parameter.unwrap_or(1).max(0) as usize;
                            }
                        }
                        "u" => {
                            reader.unicode(parameter.unwrap_or(0) as i16 as u16);
                            to_skip = group.fallback;
                        }
                        "ansicpg" => reader.encoding = code_page(parameter.unwrap_or(1252)),
                        "par" | "line" | "sect" | "page" | "row" => reader.push('\n'),
                        "tab" | "cell" => reader.push('\t'),
                        "emdash" => reader.push('—'),
                        "endash" => reader.push('–'),
                        "bullet" => reader.push('•'),
                        "lquote" => reader.push('‘'),
                        "rquote" => reader.push('’'),
                        "ldblquote" => reader.push('“'),
                        "rdblquote" => reader.push('”'),
                        "emspace" | "enspace" | "qmspace" => reader.push(' '),
                        _ => {}
                    }
                    continue;
                }
                at += 2;
                match next {
                    b'\'' => {
                        let byte = bytes
                            .get(at..at + 2)
                            .and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok());
                        at += 2;
                        if let Some(byte) = byte {
                            if to_skip > 0 {
                                to_skip -= 1;
                            } else if !group.skipped {
                                reader.byte(byte);
                            }
                        }
                    }
                    b'*' => set_skipped(&mut groups),
                    _ if group.skipped => {}
                    _ if to_skip > 0 => to_skip -= 1,
                    b'\\' | b'{' | b'}' => reader.byte(next),
                    b'~' => reader.push('\u{a0}'),
                    b'_' => reader.push('-'),
                    b'\r' | b'\n' => reader.push('\n'),
                    _ => {}
                }
            }
            byte => {
                at += 1;
                if to_skip > 0 {
                    to_skip -= 1;
                } else if !group.skipped {
                    reader.byte(byte);
                }
            }
        }
    }
    reader.flush();
    Ok(reader.text)
}

fn set_skipped(groups: &mut [Group]) {
    if let Some(group) = groups.last_mut() {
        group.skipped = true;
    }
}

fn control_word(bytes: &[u8], at: usize) -> (&str, Option<i32>, usize) {
    let mut end = at;
    while end < bytes.len() && bytes[end].is_ascii_alphabetic() && end - at < 32 {
        end += 1;
    }
    let word = std::str::from_utf8(&bytes[at..end]).unwrap_or("");
    let mut number_end = end;
    if bytes.get(number_end) == Some(&b'-') {
        number_end += 1;
    }
    while number_end < bytes.len() && bytes[number_end].is_ascii_digit() && number_end - end < 10 {
        number_end += 1;
    }
    let parameter = std::str::from_utf8(&bytes[end..number_end])
        .ok()
        .and_then(|number| number.parse().ok());
    let mut after = if parameter.is_some() { number_end } else { end };
    if bytes.get(after) == Some(&b' ') {
        after += 1;
    }
    (word, parameter, after)
}

struct Text {
    text: String,
    pending: Vec<u8>,
    encoding: &'static Encoding,
    high_surrogate: Option<u16>,
}

impl Text {
    fn byte(&mut self, byte: u8) {
        self.high_surrogate = None;
        self.pending.push(byte);
    }

    fn push(&mut self, c: char) {
        self.flush();
        self.high_surrogate = None;
        self.text.push(c);
    }

    fn unicode(&mut self, unit: u16) {
        self.flush();
        match (self.high_surrogate.take(), unit) {
            (_, 0xD800..=0xDBFF) => self.high_surrogate = Some(unit),
            (Some(high), 0xDC00..=0xDFFF) => self.text.extend(char::decode_utf16([high, unit]).flatten()),
            (_, 0xDC00..=0xDFFF) => {}
            (_, unit) => self.text.extend(char::from_u32(u32::from(unit))),
        }
    }

    fn flush(&mut self) {
        if !self.pending.is_empty() {
            let (text, _) = self.encoding.decode_without_bom_handling(&self.pending);
            self.text.push_str(&text);
            self.pending.clear();
        }
    }
}

fn code_page(number: i32) -> &'static Encoding {
    match number {
        874 => encoding_rs::WINDOWS_874,
        932 => encoding_rs::SHIFT_JIS,
        936 => encoding_rs::GBK,
        949 => encoding_rs::EUC_KR,
        950 => encoding_rs::BIG5,
        1250 => encoding_rs::WINDOWS_1250,
        1251 => encoding_rs::WINDOWS_1251,
        1253 => encoding_rs::WINDOWS_1253,
        1254 => encoding_rs::WINDOWS_1254,
        1255 => encoding_rs::WINDOWS_1255,
        1256 => encoding_rs::WINDOWS_1256,
        1257 => encoding_rs::WINDOWS_1257,
        1258 => encoding_rs::WINDOWS_1258,
        10000 => encoding_rs::MACINTOSH,
        _ => encoding_rs::WINDOWS_1252,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(rtf: &str) -> String {
        text_of(rtf.as_bytes()).unwrap()
    }

    #[test]
    fn reads_textedit_rtf() {
        let rtf = r"{\rtf1\ansi\ansicpg1252\cocoartf2761
\cocoatextscaling0\cocoaplatform0{\fonttbl\f0\fswiss\fcharset0 Helvetica;}
{\colortbl;\red255\green255\blue255;}
{\*\expandedcolortbl;;}
\paperw11900\paperh16840\margl1440\margr1440\vieww11520\viewh8400\viewkind0
\pard\tx720\tx1440\pardirnatural\partightenfactor0

\f0\fs24 \cf0 Shopping list\
Caf\'e9 beans \'96 two bags\
\uc0\u8364 12 and \u55357 \u56842  done}";
        assert_eq!(text(rtf), "Shopping list\nCafé beans – two bags\n€12 and 😊 done");
    }

    #[test]
    fn double_byte_code_pages() {
        assert_eq!(text(r"{\rtf1\ansi\ansicpg932 \'93\'fa\'96\'7b}"), "日本");
        assert_eq!(text(r"{\rtf1\ansi\ansicpg1251 \'cf\'f0\'e8\'e2\'e5\'f2}"), "Привет");
    }
}
