use std::{ops::Range, path::Path};

use serde::Serialize;

use crate::{embed::PageTexts, words};

#[derive(Serialize, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Details {
    pub snippets: Vec<Snippet>,
    pub closest: Option<Snippet>,
}

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
    }
}

pub fn of_document(path: &Path, query: &str, at: Option<u64>, scanned: &PageTexts) -> Result<Details, String> {
    let terms = words::terms(query);
    let paged = path.extension().is_some_and(|extension| {
        ["pdf", "pptx", "pptm", "ppsx"]
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
    Ok(Details { snippets: all, closest })
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
}
