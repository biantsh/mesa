use std::{
    fs::File,
    io::{BufReader, Read},
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
};

use quick_xml::{Reader, events::Event};

use super::prepare::{Prepared, Unit};

const PASSAGE_CHARS: usize = 1500;
const MIN_LETTERS: usize = 20;
const MAX_PASSAGES: usize = 32;
const MAX_PDF_PASSAGES: usize = 120;
const TEXT_BYTES: u64 = 256 * 1024;
const MAX_DOCX_XML_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PDF_BYTES: u64 = 200 * 1024 * 1024;

type Emit<'a> = &'a mut dyn FnMut(Unit) -> bool;

pub fn plain(path: &Path, emit: Emit, words: &mut String) -> Result<(), String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(TEXT_BYTES).read_to_end(&mut bytes))
        .map_err(|e| format!("can't read the file: {e}"))?;
    send_passages(
        path,
        &String::from_utf8_lossy(&bytes),
        |n| n as u64,
        MAX_PASSAGES,
        emit,
        words,
    )
}

pub fn docx(path: &Path, emit: Emit, words: &mut String) -> Result<(), String> {
    let file = File::open(path).map_err(|e| format!("can't read the file: {e}"))?;
    let mut archive = zip::ZipArchive::new(BufReader::new(file)).map_err(|e| format!("not a Word document: {e}"))?;
    let mut xml = Vec::new();
    archive
        .by_name("word/document.xml")
        .map_err(|e| format!("not a Word document: {e}"))?
        .take(MAX_DOCX_XML_BYTES)
        .read_to_end(&mut xml)
        .map_err(|e| format!("can't read the document: {e}"))?;
    send_passages(path, &docx_text(&xml)?, |n| n as u64, MAX_PASSAGES, emit, words)
}

fn docx_text(xml: &[u8]) -> Result<String, String> {
    let mut reader = Reader::from_reader(xml);
    let mut text = String::new();
    let mut in_text = false;
    let mut buffer = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buffer)
            .map_err(|e| format!("can't read the document: {e}"))?
        {
            Event::Start(tag) => match tag.local_name().as_ref() {
                "t" => in_text = true,
                "tab" => text.push('\t'),
                "br" | "cr" => text.push('\n'),
                _ => {}
            },
            Event::Empty(tag) => match tag.local_name().as_ref() {
                "tab" => text.push('\t'),
                "br" | "cr" => text.push('\n'),
                _ => {}
            },
            Event::End(tag) => match tag.local_name().as_ref() {
                "t" => in_text = false,
                "p" => text.push('\n'),
                _ => {}
            },
            Event::Text(content) if in_text => {
                text.push_str(&content.xml10_content());
            }
            Event::GeneralRef(reference) if in_text => {
                if let Ok(Some(c)) = reference.resolve_char_ref() {
                    text.push(c);
                } else {
                    match reference.as_ref() {
                        "amp" => text.push('&'),
                        "lt" => text.push('<'),
                        "gt" => text.push('>'),
                        "quot" => text.push('"'),
                        "apos" => text.push('\''),
                        _ => {}
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Ok(text)
}

pub fn pdf(path: &Path, emit: Emit, words: &mut String) -> Result<(), String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_PDF_BYTES).read_to_end(&mut bytes))
        .map_err(|e| format!("can't read the file: {e}"))?;
    let document = catch_unwind(AssertUnwindSafe(|| pdf_oxide::PdfDocument::from_bytes(bytes)))
        .map_err(|_| "can't read the PDF (its reader gave up on it)".to_owned())?
        .map_err(|e| format!("can't read the PDF: {e}"))?;
    let count = document.page_count().map_err(|e| format!("can't read the PDF: {e}"))?;

    let (mut sent, mut readable) = (0, 0);
    for page in 0..count {
        let Some(text) = catch_unwind(AssertUnwindSafe(|| document.extract_text(page)))
            .ok()
            .and_then(Result::ok)
        else {
            continue;
        };
        readable += 1;
        let text = normalise(&text);
        if !enough_text(&text) {
            continue;
        }
        for passage in passages(&text) {
            if sent == MAX_PDF_PASSAGES {
                return Ok(());
            }
            sent += 1;
            add_words(words, &passage);
            if !emit(Unit {
                locator: page as u64 + 1,
                input: Prepared::Text(titled(path, &passage)),
            }) {
                return Ok(());
            }
        }
    }
    match (sent, readable) {
        (0, 0) if count > 0 => Err("can't read the PDF (none of its pages could be read)".into()),
        (0, _) => Err("the PDF has no text to read (is it a scan?)".into()),
        _ => Ok(()),
    }
}

fn add_words(words: &mut String, passage: &str) {
    if !words.is_empty() {
        words.push('\n');
    }
    words.push_str(passage);
}

fn enough_text(text: &str) -> bool {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .nth(MIN_LETTERS - 1)
        .is_some()
}

fn send_passages(
    path: &Path,
    text: &str,
    locator: impl Fn(usize) -> u64,
    most: usize,
    emit: Emit,
    words: &mut String,
) -> Result<(), String> {
    let text = normalise(text);
    if text.is_empty() {
        return Err("there is no text in it".into());
    }
    if !enough_text(&text) {
        return Err("there is too little text in it to say what it is about".into());
    }
    let passages = passages(&text);
    for (n, passage) in passages.into_iter().take(most).enumerate() {
        add_words(words, &passage);
        if !emit(Unit {
            locator: locator(n),
            input: Prepared::Text(titled(path, &passage)),
        }) {
            break;
        }
    }
    Ok(())
}

pub fn query_prompt(text: &str) -> String {
    format!("task: search result | query: {text}")
}

fn titled(path: &Path, passage: &str) -> String {
    let name = path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default();
    format!("title: {name} | text: {passage}")
}

pub fn untitled<'a>(path: &Path, prompt: &'a str) -> &'a str {
    prompt.strip_prefix(titled(path, "").as_str()).unwrap_or(prompt)
}

fn normalise(text: &str) -> String {
    text.replace('\r', "")
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect::<String>()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn passages(text: &str) -> Vec<String> {
    let mut passages = Vec::new();
    let mut current = String::new();
    let mut length = 0;
    for line in text.lines() {
        let mut rest = line;
        loop {
            let line_length = rest.chars().count();
            if length > 0 && length + 1 + line_length > PASSAGE_CHARS {
                passages.push(std::mem::take(&mut current));
                length = 0;
            }
            if line_length <= PASSAGE_CHARS {
                if length > 0 {
                    current.push('\n');
                    length += 1;
                }
                current.push_str(rest);
                length += line_length;
                break;
            }
            let cut = cut_point(rest);
            passages.push(rest[..cut].trim_end().to_owned());
            rest = rest[cut..].trim_start();
        }
    }
    if !current.is_empty() {
        passages.push(current);
    }
    passages
}

fn cut_point(line: &str) -> usize {
    let end = line.char_indices().nth(PASSAGE_CHARS).map_or(line.len(), |(at, _)| at);
    let from = line.char_indices().nth(PASSAGE_CHARS * 4 / 5).map_or(0, |(at, _)| at);
    line[from..end]
        .char_indices()
        .rfind(|(_, c)| c.is_whitespace())
        .map_or(end, |(at, c)| from + at + c.len_utf8())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mesa-documents-{tag}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn collect(reader: impl FnOnce(Emit) -> Result<(), String>) -> Result<Vec<(u64, String)>, String> {
        let mut units = Vec::new();
        reader(&mut |unit| {
            let Prepared::Text(prompt) = unit.input else {
                panic!("a document gives text")
            };
            units.push((unit.locator, prompt));
            true
        })?;
        Ok(units)
    }

    #[test]
    fn splits_long_text_at_line_breaks() {
        let dir = temp_dir("long");
        let path = dir.join("book.txt");
        let paragraph = "word ".repeat(100);
        fs::write(&path, vec![paragraph.trim(); 10].join("\n\n")).unwrap();
        let units = collect(|emit| plain(&path, emit, &mut String::new())).unwrap();
        assert_eq!(
            units.len(),
            4,
            "10 paragraphs of 500, three to a passage: {}",
            units.len()
        );
        assert_eq!(units.iter().map(|(n, _)| *n).collect::<Vec<_>>(), [0, 1, 2, 3]);
        assert!(
            units
                .iter()
                .all(|(_, prompt)| prompt.chars().count() <= PASSAGE_CHARS + 40)
        );
    }

    #[test]
    fn splits_after_multibyte_space() {
        let mut line = "a".repeat(PASSAGE_CHARS * 9 / 10);
        line.push('\u{a0}');
        line.push_str(&"b".repeat(PASSAGE_CHARS));
        let pieces = passages(&line);
        assert_eq!(pieces[0], "a".repeat(PASSAGE_CHARS * 9 / 10));
        for text in ["é".repeat(5000), "日本語 ".repeat(2000), "\u{a0}".repeat(4000)] {
            let pieces = passages(&text);
            assert!(pieces.iter().all(|p| p.chars().count() <= PASSAGE_CHARS));
        }
    }

    #[test]
    fn splits_on_char_boundaries() {
        let text = "日本語".repeat(1000);
        let pieces = passages(&text);
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0].chars().count(), PASSAGE_CHARS);
        assert_eq!(pieces.concat(), text);
    }
}
