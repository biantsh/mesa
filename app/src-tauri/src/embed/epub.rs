use std::path::Path;

use super::documents::{self, Emit, MAX_PASSAGES, WORDS_CHARS};
use crate::epub_read::Book;

pub fn epub(path: &Path, emit: Emit, words: &mut String) -> Result<(), String> {
    let mut book = Book::open(path)?;
    let mut text = String::new();
    for chapter in book.chapters.clone() {
        if text.len() >= WORDS_CHARS * 4 {
            break;
        }
        for block in book.blocks(&chapter) {
            if let Some(line) = block.text() {
                text.push_str(&line.replace(['\n', '\t'], " "));
                text.push('\n');
            }
        }
    }
    documents::send_passages(path, &text, |n| n as u64, MAX_PASSAGES, emit, words)
}
