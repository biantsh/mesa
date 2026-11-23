use std::{
    cell::Cell,
    fs::File,
    io::{BufReader, Read},
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
};

use quick_xml::{
    Reader,
    events::{BytesRef, Event},
};

use super::{
    picture,
    prepare::{Prepared, ReadText, Unit},
    scans,
};

pub(super) const PASSAGE_CHARS: usize = 1500;
const MIN_LETTERS: usize = 20;
pub(super) const MAX_PASSAGES: usize = 32;
pub(super) const WORDS_CHARS: usize = 2_000_000;
const MAX_PDF_PASSAGES: usize = 120;
const MAX_SCANNED_PAGES: usize = 25;
const TEXT_BYTES: u64 = (WORDS_CHARS * 4) as u64;
const MAX_DOCX_XML_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PDF_BYTES: u64 = 200 * 1024 * 1024;

pub(super) type Emit<'a> = &'a mut dyn FnMut(Unit) -> bool;

thread_local! {
    static EVERY_PASSAGE: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn every_passage<T>(read: impl FnOnce() -> T) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            EVERY_PASSAGE.with(|every| every.set(false));
        }
    }
    EVERY_PASSAGE.with(|every| every.set(true));
    let _reset = Reset;
    read()
}

fn kept(count: usize, most: usize) -> Vec<usize> {
    if count <= most || EVERY_PASSAGE.with(Cell::get) {
        return (0..count).collect();
    }
    (0..most).map(|n| (2 * n + 1) * count / (2 * most)).collect()
}

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

pub fn html(path: &Path, emit: Emit, words: &mut String) -> Result<(), String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(TEXT_BYTES).read_to_end(&mut bytes))
        .map_err(|e| format!("can't read the file: {e}"))?;
    send_passages(
        path,
        &html_text(&String::from_utf8_lossy(&bytes)),
        |n| n as u64,
        MAX_PASSAGES,
        emit,
        words,
    )
}

fn html_text(html: &str) -> String {
    const HIDDEN: &[&str] = &["script", "style", "template", "svg", "noscript", "head"];
    const BLOCKS: &[&str] = &[
        "p",
        "div",
        "br",
        "li",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "tr",
        "section",
        "article",
        "header",
        "footer",
        "ul",
        "ol",
        "table",
        "blockquote",
        "pre",
        "title",
        "dt",
        "dd",
        "figcaption",
        "main",
        "nav",
        "aside",
        "hr",
    ];
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len() / 2);
    let mut at = 0;
    while at < html.len() {
        let Some(open) = html[at..].find('<').map(|n| at + n) else {
            out.push_str(&html[at..]);
            break;
        };
        out.push_str(&html[at..open]);
        if lower[open..].starts_with("<!--") {
            at = lower[open..].find("-->").map_or(html.len(), |n| open + n + 3);
            continue;
        }
        let Some(close) = html[open..].find('>').map(|n| open + n) else {
            break;
        };
        let tag = lower[open + 1..close].trim_start_matches('/');
        let name: String = tag.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        at = close + 1;
        if HIDDEN.contains(&name.as_str()) && !lower[open + 1..].starts_with('/') {
            if name == "head"
                && let Some(title) = lower[at..].find("<title").map(|n| at + n)
                && let Some(end) = lower[title..].find("</title>").map(|n| title + n)
                && !lower[at..title].contains("</head>")
            {
                let text_from = html[title..].find('>').map_or(end, |n| title + n + 1);
                out.push_str(&html[text_from..end.max(text_from)]);
                out.push('\n');
            }
            at = lower[at..].find(&format!("</{name}")).map_or(html.len(), |n| at + n);
            at = html[at..].find('>').map_or(html.len(), |n| at + n + 1);
            continue;
        }
        if BLOCKS.contains(&name.as_str()) {
            out.push('\n');
        } else if (name == "td" || name == "th") && !lower[open + 1..].starts_with('/') {
            out.push('\t');
        }
    }
    decode_entities(&out)
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..semi];
        let named = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "hellip" => Some('…'),
            "rsquo" => Some('’'),
            "lsquo" => Some('‘'),
            "ldquo" => Some('“'),
            "rdquo" => Some('”'),
            "copy" => Some('©'),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|n| n.parse().ok()))
                .and_then(char::from_u32),
        };
        match named {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
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
            Event::GeneralRef(reference) if in_text => text.extend(entity(&reference)),
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Ok(text)
}

pub(crate) fn entity(reference: &BytesRef) -> Option<char> {
    if let Ok(Some(c)) = reference.resolve_char_ref() {
        return Some(c);
    }
    match reference.as_ref() {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => None,
    }
}

fn open_pdf(path: &Path) -> Result<(pdf_oxide::PdfDocument, usize), String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_PDF_BYTES).read_to_end(&mut bytes))
        .map_err(|e| format!("can't read the file: {e}"))?;
    let document = catch_unwind(AssertUnwindSafe(|| pdf_oxide::PdfDocument::from_bytes(bytes)))
        .map_err(|_| "can't read the PDF (its reader gave up on it)".to_owned())?
        .map_err(|e| format!("can't read the PDF: {e}"))?;
    let count = document.page_count().map_err(|e| format!("can't read the PDF: {e}"))?;
    Ok((document, count))
}

fn page_text(document: &pdf_oxide::PdfDocument, page: usize) -> Option<String> {
    catch_unwind(AssertUnwindSafe(|| document.extract_text(page)))
        .ok()
        .and_then(Result::ok)
        .map(|text| normalise(&text))
}

pub struct Scans<'a> {
    pub text: ReadText<'a>,
    pub words_only: bool,
}

pub fn pdf(path: &Path, emit: Emit, words: &mut String, mut scans: Option<Scans>) -> Result<(), String> {
    let (document, count) = open_pdf(path)?;

    let every = EVERY_PASSAGE.with(Cell::get);
    let (mut readable, mut read, mut streamed, mut found, mut without) = (0, 0, 0, Vec::new(), Vec::new());
    for page in 0..count {
        if read >= WORDS_CHARS {
            break;
        }
        let text = page_text(&document, page);
        if text.is_some() {
            readable += 1;
        }
        match text.filter(|text| enough_text(text)) {
            Some(text) => {
                for passage in passages(&text) {
                    read += passage.chars().count();
                    add_words(words, &passage);
                    if every {
                        if !emit(Unit {
                            locator: page as u64 + 1,
                            input: Prepared::Text(titled(path, &passage)),
                        }) {
                            return Ok(());
                        }
                        streamed += 1;
                        continue;
                    }
                    found.push((page, passage));
                }
            }
            None => without.push(page),
        }
    }
    let mut sent = 0;
    for n in kept(found.len(), MAX_PDF_PASSAGES) {
        let (page, passage) = &found[n];
        sent += 1;
        if !emit(Unit {
            locator: *page as u64 + 1,
            input: Prepared::Text(titled(path, passage)),
        }) {
            return Ok(());
        }
    }
    let mut scanned = 0;
    if let Some(scans) = scans.as_mut()
        && !without.is_empty()
    {
        let shares = without.len().min(MAX_SCANNED_PAGES);
        let mut drawing: Option<Option<scans::Pages>> = None;
        for share in 0..shares {
            let Some(pages) = drawing.get_or_insert_with(|| scans::Pages::open(path)).as_ref() else {
                break;
            };
            let (first, until) = (share * without.len() / shares, (share + 1) * without.len() / shares);
            let Some((page, (rgb, width, height))) = without[first..until]
                .iter()
                .find_map(|&page| pages.draw(page).map(|drawn| (page, drawn)))
            else {
                continue;
            };
            scanned += 1;
            let locator = page as u64 + 1;
            let wanted = scans.words_only
                || match picture::from_rgb(&rgb, width, height) {
                    Ok(bitmap) => emit(Unit {
                        locator,
                        input: Prepared::Image(bitmap),
                    }),
                    Err(_) => true,
                };
            if !wanted {
                return Ok(());
            }
            if let Some(text) = scans.text.as_mut() {
                text(locator, rgb, width, height);
            }
        }
    }
    match (streamed + sent + scanned, readable) {
        (0, 0) if count > 0 => Err("can't read the PDF (none of its pages could be read)".into()),
        (0, _) => Err("the PDF has no text to read, and no page with anything on it".into()),
        _ => Ok(()),
    }
}

pub fn pdf_pages(
    path: &Path,
    pages: &mut dyn FnMut(usize) -> Vec<(usize, usize)>,
    emit: Emit,
) -> Result<usize, String> {
    let (document, count) = open_pdf(path)?;
    let mut drawing: Option<Option<scans::Pages>> = None;
    let mut sent = 0;
    'asked: for (first, until) in pages(count) {
        for page in first..until.max(first + 1).min(count) {
            let text = page_text(&document, page);
            let input = match text
                .filter(|text| enough_text(text))
                .and_then(|text| passages(&text).into_iter().next())
            {
                Some(passage) => Prepared::Text(titled(path, &passage)),
                None => {
                    let Some(pages) = drawing.get_or_insert_with(|| scans::Pages::open(path)).as_ref() else {
                        continue;
                    };
                    let Some((rgb, width, height)) = pages.draw(page) else {
                        continue;
                    };
                    let Ok(bitmap) = picture::from_rgb(&rgb, width, height) else {
                        continue;
                    };
                    Prepared::Image(bitmap)
                }
            };
            sent += 1;
            if !emit(Unit {
                locator: page as u64 + 1,
                input,
            }) {
                return Ok(count);
            }
            continue 'asked;
        }
    }
    if sent == 0 && count > 0 {
        return Err("the PDF has no text to read, and no page with anything on it".into());
    }
    Ok(count)
}

pub fn page_passages(path: &Path, page: u64, text: &str) -> Vec<Unit> {
    let text = normalise(text);
    if !enough_text(&text) {
        return Vec::new();
    }
    passages(&text)
        .into_iter()
        .map(|passage| Unit {
            locator: page,
            input: Prepared::Text(titled(path, &passage)),
        })
        .collect()
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

pub(super) fn send_passages(
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
    let text: String = if text.len() > WORDS_CHARS {
        text.chars().take(WORDS_CHARS).collect()
    } else {
        text
    };
    let passages = passages(&text);
    for passage in &passages {
        add_words(words, passage);
    }
    for n in kept(passages.len(), most) {
        if !emit(Unit {
            locator: locator(n),
            input: Prepared::Text(titled(path, &passages[n])),
        }) {
            break;
        }
    }
    Ok(())
}

pub(super) fn send_sections(
    path: &Path,
    sections: &[(u64, String)],
    most: usize,
    emit: Emit,
    words: &mut String,
) -> Result<(), String> {
    let sections: Vec<(u64, String)> = sections
        .iter()
        .map(|(locator, text)| (*locator, normalise(text)))
        .filter(|(_, text)| !text.is_empty())
        .collect();
    if sections.is_empty() {
        return Err("there is no text in it".into());
    }
    if !enough_text(
        &sections
            .iter()
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    ) {
        return Err("there is too little text in it to say what it is about".into());
    }
    let lines = sections
        .iter()
        .flat_map(|(locator, text)| text.lines().map(move |line| (*locator, line)));
    let passages = located_passages(lines);
    for (_, passage) in &passages {
        add_words(words, passage);
    }
    for n in kept(passages.len(), most) {
        let (locator, passage) = &passages[n];
        if !emit(Unit {
            locator: *locator,
            input: Prepared::Text(titled(path, passage)),
        }) {
            break;
        }
    }
    Ok(())
}

pub(super) fn send_each_section(
    path: &Path,
    sections: &[(u64, String)],
    emit: Emit,
    words: &mut String,
) -> Result<(), String> {
    for (locator, text) in sections {
        for passage in passages(&normalise(text)) {
            add_words(words, &passage);
            if !emit(Unit {
                locator: *locator,
                input: Prepared::Text(titled(path, &passage)),
            }) {
                return Ok(());
            }
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
    located_passages(text.lines().map(|line| (0, line)))
        .into_iter()
        .map(|(_, passage)| passage)
        .collect()
}

fn located_passages<'a>(lines: impl IntoIterator<Item = (u64, &'a str)>) -> Vec<(u64, String)> {
    let mut passages = Vec::new();
    let mut current = String::new();
    let (mut length, mut at) = (0, 0);
    for (locator, line) in lines {
        let mut rest = line;
        loop {
            let line_length = rest.chars().count();
            if length > 0 && length + 1 + line_length > PASSAGE_CHARS {
                passages.push((at, std::mem::take(&mut current)));
                length = 0;
            }
            if line_length <= PASSAGE_CHARS {
                if length > 0 {
                    current.push('\n');
                    length += 1;
                } else {
                    at = locator;
                }
                current.push_str(rest);
                length += line_length;
                break;
            }
            let cut = cut_point(rest);
            passages.push((locator, rest[..cut].trim_end().to_owned()));
            rest = rest[cut..].trim_start();
        }
    }
    if !current.is_empty() {
        passages.push((at, current));
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
        fs::write(&path, [paragraph.trim(); 10].join("\n\n")).unwrap();
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
