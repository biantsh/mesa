use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};

const ON_THE_LINE: f64 = 30.0;
const PER_FOLD: f64 = 0.75;
const FLOOR: f64 = -1.0;
const SCATTERED_IN_A_DOCUMENT: f64 = 0.5;

pub fn words_of(text: &str) -> Vec<String> {
    located(text).into_iter().map(|(word, _)| word).collect()
}

pub fn located(text: &str) -> Vec<(String, Range<usize>)> {
    let mut words = Vec::new();
    for word in text.split_whitespace() {
        let base = word.as_ptr() as usize - text.as_ptr() as usize;
        let digits = word.chars().any(|c| c.is_ascii_digit());
        let (mut current, mut at, mut numeric) = (String::new(), 0..0, false);
        let done = |current: &mut String, at: &Range<usize>, words: &mut Vec<(String, Range<usize>)>| {
            if !current.is_empty() {
                words.push((std::mem::take(current), at.clone()));
            }
        };
        for (offset, original) in word.char_indices() {
            let here = base + offset..base + offset + original.len_utf8();
            for c in original.to_lowercase() {
                let c = if digits && c == 'o' { '0' } else { c };
                if !c.is_alphanumeric() {
                    done(&mut current, &at, &mut words);
                } else if written_without_spaces(c) {
                    done(&mut current, &at, &mut words);
                    words.push((c.to_string(), here.clone()));
                } else {
                    if !current.is_empty() && c.is_numeric() != numeric {
                        done(&mut current, &at, &mut words);
                    }
                    numeric = c.is_numeric();
                    if current.is_empty() {
                        at = here.clone();
                    }
                    current.push(c);
                    at.end = here.end;
                }
            }
        }
        done(&mut current, &at, &mut words);
    }
    words
}

pub fn occurrences(text: &str, terms: &[String]) -> Vec<(usize, Range<usize>)> {
    let words = located(text);
    let mut found = Vec::new();
    for (n, term) in terms.iter().enumerate() {
        let parts: Vec<&str> = term.split(' ').collect();
        for run in words.windows(parts.len()) {
            if run.iter().zip(&parts).all(|((word, _), part)| word == part) {
                found.push((n, run[0].1.start..run[run.len() - 1].1.end));
            }
        }
    }
    found.sort_by_key(|(_, at)| (at.start, at.end));
    found
}

fn written_without_spaces(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF
        | 0x31F0..=0x31FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xF900..=0xFAFF
        | 0xFF66..=0xFF9F
        | 0x20000..=0x3FFFF
    )
}

pub fn fold(text: &str) -> String {
    let words = words_of(text);
    if words.is_empty() {
        String::new()
    } else {
        format!(" {} ", words.join(" "))
    }
}

pub fn terms(query: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for term in query.split_whitespace().map(|word| words_of(word).join(" ")) {
        if !term.is_empty() && !terms.contains(&term) {
            terms.push(term);
        }
    }
    terms
}

pub fn whole(term: &str) -> String {
    format!("\" {term} \"")
}

pub fn margins(exact: &[(i64, bool)], phrase: &[i64]) -> HashMap<i64, f64> {
    let margin = |files: usize| PER_FOLD * (ON_THE_LINE / files.max(1) as f64).ln();
    let in_order: HashSet<i64> = phrase.iter().copied().collect();
    let (by_phrase, here_and_there) = (margin(in_order.len()), margin(exact.len()));
    let mut margins: HashMap<i64, f64> = exact
        .iter()
        .map(|&(file, picture)| {
            let scattered = if picture {
                here_and_there
            } else {
                here_and_there.min(-SCATTERED_IN_A_DOCUMENT)
            };
            (
                file,
                if in_order.contains(&file) {
                    by_phrase.max(scattered)
                } else {
                    scattered
                },
            )
        })
        .collect();
    for &file in &in_order {
        margins.entry(file).or_insert(by_phrase);
    }
    margins.retain(|_, margin| *margin >= FLOOR);
    margins
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding() {
        assert_eq!(fold("Invoice INV - 2O24/11"), " invoice inv 2024 11 ");
        assert_eq!(terms("inv-2024-11"), ["inv 2024 11"]);
        assert_eq!(fold("ORDER 1O5"), " order 105 ");
        assert_eq!(fold("yticklabels=L3,"), " yticklabels l 3 ");
        assert_eq!(fold("ae4cl1c6"), " ae 4 cl 1 c 6 ");
        assert_eq!(fold("Größe: 42 €"), " größe 42 ");
        assert_eq!(fold("東京タワー 入口"), " 東 京 タ ワ ー 入 口 ");
        assert_eq!(fold("  ...  "), "");
        assert_eq!(terms("  Kyoto  kyoto, 2024 "), ["kyoto", "2024"]);
        assert_eq!(whole("inv 2024 11"), "\" inv 2024 11 \"");
    }
}
