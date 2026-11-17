use std::{ops::Range, path::Path};

use serde::Serialize;

use crate::{embed::PageTexts, words};

#[derive(Serialize, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Details {
    pub snippets: Vec<Snippet>,
    pub closest: Option<Snippet>,
    pub code: Option<CodeView>,
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CodeView {
    pub language: String,
    pub project: Option<String>,
    pub symbol: String,
    pub start: u32,
    pub end: u32,
    pub lines: Vec<Vec<crate::highlight::Part>>,
}

const MOST_CODE_LINES: usize = 60;

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Snippet {
    pub parts: Vec<Part>,
    pub page: Option<u64>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Part {
    pub text: String,
    pub hit: bool,
}

const MOST_SNIPPETS: usize = 3;
const BEFORE_CHARS: usize = 70;
const SNIPPET_CHARS: usize = 260;

pub fn of_picture(text: &str, query: &str) -> Details {
    let found = words::occurrences(text, &words::terms(query));
    Details {
        snippets: snippets(text, &found, None, MOST_SNIPPETS),
        closest: None,
        code: None,
    }
}

pub fn of_document(path: &Path, query: &str, at: Option<u64>, scanned: &PageTexts) -> Result<Details, String> {
    let terms = words::terms(query);
    let paged = path.extension().is_some_and(|extension| {
        ["pdf", "pptx", "pptm", "ppsx", "potx", "potm", "odp", "otp", "key"]
            .iter()
            .any(|paged| extension.eq_ignore_ascii_case(paged))
    });
    let page = |locator: u64| paged.then_some(locator);
    let (mut all, mut some, mut closest) = (Vec::new(), Vec::new(), None);
    let mut look = |locator: u64, passage: &str| {
        if closest.is_none() && at == Some(locator) {
            let (start, cut) = start_of(passage, SNIPPET_CHARS);
            let mut parts = vec![Part {
                text: collapsed(start),
                hit: false,
            }];
            if cut {
                parts.push(Part {
                    text: "…".into(),
                    hit: false,
                });
            }
            closest = Some(Snippet {
                parts,
                page: page(locator),
            });
        }
        if !terms.is_empty() && all.len() < MOST_SNIPPETS {
            let found = words::occurrences(passage, &terms);
            if (0..terms.len()).all(|term| found.iter().any(|(of, _)| *of == term)) {
                all.extend(snippets(passage, &found, page(locator), MOST_SNIPPETS - all.len()));
            } else if !found.is_empty() && some.len() < MOST_SNIPPETS {
                some.extend(snippets(passage, &found, page(locator), 1));
            }
        }
        (!terms.is_empty() && all.len() < MOST_SNIPPETS) || (at.is_some() && closest.is_none())
    };
    let mut wanted = true;
    let read = crate::embed::passages(path, &mut |locator, passage| {
        wanted = look(locator, passage);
        wanted
    });
    if let Err(why) = read
        && scanned.is_empty()
    {
        return Err(why);
    }
    for (page, text) in scanned {
        if !wanted {
            break;
        }
        wanted = look(*page, text);
    }
    drop(look);
    all.extend(some);
    all.truncate(MOST_SNIPPETS);
    if is_laid_out(path)
        && let Some(pages) = path
            .to_str()
            .and_then(crate::document_preview::page_texts)
            .map(|texts| Pages::new(&texts))
    {
        for snippet in all.iter_mut() {
            let from = snippet.parts.iter().position(|part| part.hit).unwrap_or(0);
            snippet.page = pages.page_of(&snippet.parts[from..]);
        }
        if let Some(closest) = closest.as_mut() {
            closest.page = pages.page_of(&closest.parts);
        }
    }
    Ok(Details {
        snippets: all,
        closest,
        code: None,
    })
}

pub fn of_code(path: &Path, query: &str, at: Option<u64>) -> Result<Details, String> {
    let text = crate::embed::code::text_of(path)?;
    let lang = crate::embed::code::language(path, &text).ok_or("it isn't code Mesa reads")?;
    let chunks = crate::embed::code::chunks(&text, lang);
    let terms = words::terms(query);
    let hits_in = |text: &str| {
        let found = words::occurrences(text, &terms);
        (0..terms.len())
            .filter(|term| found.iter().any(|(of, _)| of == term))
            .count()
    };
    let counts: Vec<usize> = chunks
        .iter()
        .map(|chunk| if terms.is_empty() { 0 } else { hits_in(&chunk.text) })
        .collect();
    let every = (!terms.is_empty())
        .then(|| counts.iter().position(|&n| n == terms.len()))
        .flatten();
    let closest = at.and_then(|at| chunks.iter().position(|chunk| u64::from(chunk.start) == at));
    let some = counts
        .iter()
        .enumerate()
        .filter(|(_, n)| **n > 0)
        .max_by_key(|(_, n)| **n)
        .map(|(i, _)| i);
    let Some(chosen) = every.or(closest).or(some).or((!chunks.is_empty()).then_some(0)) else {
        return Ok(Details::default());
    };
    let chunk = &chunks[chosen];
    let shown: Vec<Range<usize>> = crate::highlight::lines(&text)
        .into_iter()
        .skip(chunk.start as usize - 1)
        .take(((chunk.end - chunk.start + 1) as usize).min(MOST_CODE_LINES))
        .collect();
    let kinds = crate::highlight::kinds(&text, lang, shown.last().map_or(0, |line| line.end));
    let lines = shown
        .into_iter()
        .map(|line| crate::highlight::parts(&text, line, &kinds, &terms))
        .collect();
    let mut snippets = Vec::new();
    if !terms.is_empty() {
        for (n, line) in text.split('\n').enumerate() {
            let number = n as u32 + 1;
            if (chunk.start..=chunk.end).contains(&number) {
                continue;
            }
            let found = words::occurrences(line, &terms);
            if !found.is_empty() {
                let trimmed = line.trim_start();
                let shift = line.len() - trimmed.len();
                let marked: Vec<Range<usize>> = found.iter().map(|(_, r)| r.start - shift..r.end - shift).collect();
                snippets.push(Snippet {
                    parts: parts(trimmed, 0..trimmed.len(), &merged(marked)),
                    page: Some(u64::from(number)),
                });
                if snippets.len() == MOST_SNIPPETS {
                    break;
                }
            }
        }
    }
    let project = crate::embed::code::project_root(path)
        .and_then(|root| root.file_name().map(|n| n.to_string_lossy().into_owned()));
    let code = CodeView {
        language: lang.name.to_owned(),
        project,
        symbol: chunk.symbol.clone(),
        start: chunk.start,
        end: chunk.end,
        lines,
    };
    Ok(Details {
        snippets,
        closest: None,
        code: Some(code),
    })
}

fn merged(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.sort_by_key(|range| range.start);
    let mut out: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        match out.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => out.push(range),
        }
    }
    out
}

fn is_laid_out(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        [
            "docx", "docm", "dotx", "dotm", "xlsx", "xlsm", "xltx", "xltm", "odt", "ott", "ods", "ots", "pages",
            "numbers", "epub",
        ]
        .iter()
        .any(|laid_out| extension.eq_ignore_ascii_case(laid_out))
    })
}

struct Pages {
    text: String,
    starts: Vec<usize>,
}

const LOOKED_FOR: [(usize, usize); 2] = [(32, 8), (12, 16)];

impl Pages {
    fn new(texts: &[String]) -> Self {
        let mut pages = Pages {
            text: String::new(),
            starts: Vec::with_capacity(texts.len()),
        };
        for text in texts {
            pages.starts.push(pages.text.len());
            pages.text.extend(folded(text));
        }
        pages
    }

    fn page_of(&self, parts: &[Part]) -> Option<u64> {
        let piece: Vec<char> = parts
            .iter()
            .filter(|part| part.text != "…")
            .flat_map(|part| folded(&part.text))
            .collect();
        for (length, tries) in LOOKED_FOR {
            let length = length.min(piece.len());
            if length == 0 {
                return None;
            }
            for start in (0..tries)
                .map(|n| n * length)
                .filter(|&start| start + length <= piece.len())
            {
                let looked_for: String = piece[start..start + length].iter().collect();
                if let Some(at) = self.text.find(&looked_for) {
                    return Some(self.starts.partition_point(|&start| start <= at) as u64);
                }
            }
        }
        None
    }
}

fn folded(text: &str) -> impl Iterator<Item = char> + '_ {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
}

fn snippets(text: &str, found: &[(usize, Range<usize>)], page: Option<u64>, most: usize) -> Vec<Snippet> {
    let mut made = Vec::new();
    let mut next = 0;
    while next < found.len() && made.len() < most {
        let first = &found[next].1;
        let start = back_from(text, first.start, BEFORE_CHARS);
        let end = on_from(text, start, SNIPPET_CHARS).max(first.end);
        let mut marked: Vec<Range<usize>> = Vec::new();
        while next < found.len() && found[next].1.end <= end {
            let range = found[next].1.clone();
            match marked.last_mut() {
                Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
                _ => marked.push(range),
            }
            next += 1;
        }
        made.push(Snippet {
            parts: parts(text, start..end, &marked),
            page,
        });
    }
    made
}

fn parts(text: &str, within: Range<usize>, marked: &[Range<usize>]) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut push = |piece: &str, hit: bool| {
        if !piece.is_empty() {
            parts.push(Part {
                text: collapsed(piece),
                hit,
            });
        }
    };
    if within.start > 0 {
        push("…", false);
    }
    let mut at = within.start;
    for range in marked {
        push(&text[at..range.start], false);
        push(&text[range.clone()], true);
        at = range.end;
    }
    push(&text[at..within.end], false);
    if within.end < text.len() {
        push("…", false);
    }
    parts
}

fn back_from(text: &str, at: usize, chars: usize) -> usize {
    let Some((start, _)) = text[..at].char_indices().rev().nth(chars.saturating_sub(1)) else {
        return 0;
    };
    text[start..at]
        .find(char::is_whitespace)
        .map_or(start, |space| start + space + 1)
}

fn on_from(text: &str, start: usize, chars: usize) -> usize {
    let Some((end, _)) = text[start..].char_indices().nth(chars) else {
        return text.len();
    };
    let end = start + end;
    match text[start..end].rfind(char::is_whitespace) {
        Some(space) if space > (end - start) / 2 => start + space,
        _ => end,
    }
}

fn start_of(text: &str, chars: usize) -> (&str, bool) {
    let end = on_from(text, 0, chars);
    (&text[..end], end < text.len())
}

fn collapsed(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(snippet: &Snippet) -> String {
        snippet
            .parts
            .iter()
            .map(|part| {
                if part.hit {
                    format!("[{}]", part.text)
                } else {
                    part.text.clone()
                }
            })
            .collect()
    }

    #[test]
    fn snippet_cut_at_word_edges() {
        let text = format!("{} the needle is here {}", "hay ".repeat(100), "straw ".repeat(100));
        let found = words::occurrences(&text, &words::terms("needle"));
        let snippet = &snippets(&text, &found, Some(4), 3)[0];
        let shown = shown(snippet);
        assert!(shown.starts_with("…hay ") && shown.ends_with("straw…"), "{shown}");
        assert!(shown.contains("the [needle] is here"));
        assert!(shown.chars().count() < SNIPPET_CHARS + 10);
        assert_eq!(snippet.page, Some(4));
    }

    #[test]
    fn finds_text_ignoring_spacing_and_case() {
        let customers = "Customers\t1,204.50\tTirana, and the rest of the cities in the north";
        let pages = Pages::new(&[
            "Whales of the\nNorth Atlantic".to_owned(),
            String::new(),
            customers.to_owned(),
        ]);
        let piece = |text: &str| {
            vec![
                Part {
                    text: "…".into(),
                    hit: false,
                },
                Part {
                    text: text.into(),
                    hit: false,
                },
            ]
        };
        assert_eq!(pages.page_of(&piece("whales of the north atlantic")), Some(1));
        assert_eq!(pages.page_of(&piece("Customers 1204.5 Tirana")), Some(3));
        assert_eq!(
            pages.page_of(&piece(
                "Sheet1\nCustomers\t1204.5\tTirana, and the rest of the cities in the north"
            )),
            Some(3)
        );
        assert_eq!(pages.page_of(&piece("nowhere at all")), None);
        assert_eq!(pages.page_of(&piece("…")), None);
    }
}
