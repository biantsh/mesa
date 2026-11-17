use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use crate::{
    browse::{self, Category},
    store::{Filter, Store},
    words,
};

const COMMON: f64 = 0.02;
const MIN_SCORE: f64 = 4.0;
const MOST: usize = 10;
const MOST_PHRASES: usize = 400;

pub struct Found {
    pub file: i64,
    pub score: f64,
    pub phrases: Vec<Vec<String>>,
}

fn phrases_of(text: &str) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    for line in text.lines() {
        let line_words = words::words_of(line);
        for n in [3, 2, 1] {
            for window in line_words.windows(n) {
                let lettered = window
                    .iter()
                    .any(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 3);
                if !lettered || (n == 1 && window[0].chars().count() < 6) {
                    continue;
                }
                let phrase = window.to_vec();
                if !out.contains(&phrase) {
                    out.push(phrase);
                }
            }
        }
    }
    out.truncate(MOST_PHRASES);
    out
}

fn code_names_in(text: &str) -> Vec<String> {
    let mut names: Vec<String> = text
        .split(|c: char| {
            c.is_whitespace()
                || matches!(
                    c,
                    '"' | '\'' | '/' | '\\' | ':' | ',' | '(' | ')' | '[' | ']' | '<' | '>' | '`'
                )
        })
        .filter(|token| token.contains('.') && crate::embed::code::is_code_name(token))
        .map(str::to_owned)
        .collect();
    names.sort();
    names.dedup();
    names
}

pub fn code_by_clues(
    store: &Store,
    text: &str,
    examples: &[(PathBuf, Option<String>)],
    filter: Filter,
) -> Result<Vec<Found>, String> {
    if filter.category.is_some_and(|category| category != Category::Code) {
        return Ok(Vec::new());
    }
    let total = store.count_of(Category::Code).map_err(|e| e.to_string())?;
    if total == 0 {
        return Ok(Vec::new());
    }
    let mut scores: HashMap<i64, Found> = HashMap::new();
    let mut add = |file: i64, score: f64, phrase: Option<Vec<String>>| {
        let found = scores.entry(file).or_insert(Found {
            file,
            score: 0.0,
            phrases: Vec::new(),
        });
        found.score += score;
        found.phrases.extend(phrase);
    };
    let mut said = text.to_owned();
    let mut phrases: Vec<(Vec<String>, f64)> = Vec::new();
    for (example, read) in examples {
        let name = example
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let category = browse::category_of(&name);
        if category == Some(Category::Code) {
            continue;
        }
        if let Some(read) = read {
            said.push('\n');
            said.push_str(read);
            phrases.extend(phrases_of(read).into_iter().map(|phrase| (phrase, 1.0)));
        }
        let stem = Path::new(&name)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let stem_words = words::words_of(&stem);
        if stem_words
            .iter()
            .any(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 3)
        {
            phrases.push((stem_words, 2.0));
        }
    }
    for name in code_names_in(&said) {
        for file in store.code_named(&name, filter).map_err(|e| e.to_string())? {
            add(file, 50.0, None);
        }
    }
    let common = (total as f64 * COMMON).max(3.0);
    for (phrase, weight) in phrases {
        let found = store.code_with_phrase(&phrase, filter).map_err(|e| e.to_string())?;
        if found.is_empty() || found.len() as f64 > common {
            continue;
        }
        let score = (total as f64 / found.len() as f64).ln().max(0.5) * phrase.len() as f64 * weight;
        for file in found {
            add(file, score, Some(phrase.clone()));
        }
    }
    let mut found: Vec<Found> = scores.into_values().filter(|found| found.score >= MIN_SCORE).collect();
    found.sort_by(|a, b| b.score.total_cmp(&a.score));
    found.truncate(MOST);
    Ok(found)
}

pub fn best_line(path: &Path, phrases: &[Vec<String>]) -> Option<u64> {
    let chunks = crate::embed::code::chunks_of(path).ok()?;
    chunks
        .iter()
        .map(|chunk| {
            let chunk_words = words::words_of(&chunk.text);
            let held = phrases
                .iter()
                .filter(|phrase| chunk_words.windows(phrase.len()).any(|w| w == phrase.as_slice()))
                .count();
            (held, chunk.start)
        })
        .filter(|(held, _)| *held > 0)
        .max_by_key(|(held, start)| (*held, std::cmp::Reverse(*start)))
        .map(|(_, start)| u64::from(start))
}
