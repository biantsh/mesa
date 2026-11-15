use crate::relevance::Ranked;

const FIRST: usize = 30;

pub fn fold(text: &str) -> String {
    let mut folded = String::with_capacity(text.len());
    for word in text.split_whitespace() {
        let digits = word.chars().any(|c| c.is_ascii_digit());
        for c in word.chars().flat_map(char::to_lowercase) {
            if c.is_alphanumeric() {
                folded.push(if digits && c == 'o' { '0' } else { c });
            }
        }
    }
    folded
}

pub fn short(folded: &str) -> String {
    let characters: Vec<char> = folded.chars().collect();
    let singles = characters.iter().map(|&c| [c, '\0']);
    let pairs = characters.windows(2).map(|pair| [pair[0], pair[1]]);
    let mut seen = std::collections::HashSet::new();
    let mut pieces = String::new();
    for piece in singles.chain(pairs).filter(|piece| seen.insert(*piece)) {
        if !pieces.is_empty() {
            pieces.push(' ');
        }
        pieces.extend(piece.into_iter().filter(|&c| c != '\0'));
    }
    pieces
}

pub fn terms(query: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for term in query.split_whitespace().map(fold) {
        if !term.is_empty() && !terms.contains(&term) {
            terms.push(term);
        }
    }
    terms
}

pub fn merge(ranked: Ranked, exact: &[i64]) -> Ranked {
    if exact.is_empty() {
        return ranked;
    }
    let found: std::collections::HashSet<i64> = exact.iter().copied().collect();
    let (mut close, mut rest) = (Vec::new(), Vec::new());
    let mut by_words = Vec::with_capacity(exact.len());
    for (n, &(file, score)) in ranked.files.iter().enumerate() {
        if found.contains(&file) {
            by_words.push((file, score));
        } else if n < ranked.close {
            close.push((file, score));
        } else {
            rest.push((file, score));
        }
    }
    let listed: std::collections::HashSet<i64> = by_words.iter().map(|&(file, _)| file).collect();
    let mut unlisted: Vec<i64> = exact.iter().copied().filter(|file| !listed.contains(file)).collect();
    unlisted.sort_unstable();
    by_words.extend(unlisted.into_iter().map(|file| (file, 0.0)));

    let close_count = close.len() + by_words.len();
    let files = if by_words.len() <= FIRST {
        by_words.into_iter().chain(close).chain(rest).collect()
    } else {
        close.into_iter().chain(by_words).chain(rest).collect()
    };
    Ranked {
        files,
        close: close_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding() {
        assert_eq!(fold("Invoice INV - 2024/11"), "invoiceinv202411");
        assert_eq!(fold("INV-2O24"), fold("inv 2024"));
        assert_eq!(fold("ORDER 1O5"), "order105");
        assert_eq!(fold("Größe: 42 €"), "größe42");
        assert_eq!(fold("東京 タワー"), "東京タワー");
        assert_eq!(terms("  Kyoto  kyoto, 2024 "), ["kyoto", "2024"]);
    }
}
