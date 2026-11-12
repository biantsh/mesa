use std::collections::HashMap;

pub const CLOSE: f64 = 2.5;
const CUTOFFS: [(usize, f64); 5] = [
    (1_000, 3.0),
    (5_000, 3.5),
    (30_000, 4.0),
    (300_000, 4.5),
    (3_000_000, 5.0),
];
pub const MIN_FILES: usize = 30;
const PRIOR_FILES: f64 = 30.0;
const MIN_SPREAD: f64 = 0.01;
const MAD_TO_SPREAD: f64 = 1.4826;

#[derive(Debug, PartialEq)]
pub struct Ranked {
    pub files: Vec<(i64, f32)>,
    pub close: usize,
}

impl Ranked {
    pub fn all_close(files: Vec<(i64, f32)>) -> Self {
        let close = files.len();
        Self { files, close }
    }
}

fn median(values: &mut [f64]) -> f64 {
    let middle = values.len() / 2;
    let (_, &mut at, _) = values.select_nth_unstable_by(middle, f64::total_cmp);
    if values.len() % 2 == 1 {
        at
    } else {
        let below = values[..middle].iter().copied().fold(f64::NEG_INFINITY, f64::max);
        (at + below) / 2.0
    }
}

struct Usual {
    count: f64,
    middle: f64,
    variance: f64,
}

impl Usual {
    fn of(mut scores: Vec<f64>) -> Self {
        let middle = median(&mut scores);
        let mut distances: Vec<f64> = scores.iter().map(|score| (score - middle).abs()).collect();
        let spread = median(&mut distances) * MAD_TO_SPREAD;
        Self {
            count: scores.len() as f64,
            middle,
            variance: spread * spread,
        }
    }
}

pub fn cutoff(files: usize) -> f64 {
    CUTOFFS
        .iter()
        .rev()
        .find(|&&(from, _)| files >= from)
        .map_or(CLOSE, |&(_, cutoff)| cutoff)
}

pub fn rank(scored: &[(i64, i64, f32)]) -> Ranked {
    let mut by_kind: HashMap<i64, Vec<f64>> = HashMap::new();
    for &(_, kind, score) in scored {
        by_kind.entry(kind).or_default().push(f64::from(score));
    }
    let kinds: HashMap<i64, Usual> = by_kind
        .into_iter()
        .map(|(kind, scores)| (kind, Usual::of(scores)))
        .collect();
    let all = kinds
        .values()
        .any(|usual| usual.count < 2.0)
        .then(|| Usual::of(scored.iter().map(|&(_, _, score)| f64::from(score)).collect()));
    let (spread, counted) = kinds
        .values()
        .filter(|usual| usual.count >= 2.0)
        .fold((0.0, 0.0), |(spread, counted), usual| {
            (spread + usual.variance * usual.count, counted + usual.count)
        });
    let shared = if counted > 0.0 { spread / counted } else { 0.0 };

    let mut measured: Vec<(i64, f32, f64)> = scored
        .iter()
        .map(|&(file, kind, score)| {
            let own = &kinds[&kind];
            let middle = if own.count >= 2.0 {
                own.middle
            } else {
                all.as_ref().map_or(own.middle, |all| all.middle)
            };
            let variance = (own.variance * own.count + PRIOR_FILES * shared) / (own.count + PRIOR_FILES);
            let z = (f64::from(score) - middle) / variance.sqrt().max(MIN_SPREAD);
            (file, score, z)
        })
        .collect();
    measured.sort_unstable_by(|a, b| b.2.total_cmp(&a.2).then(a.0.cmp(&b.0)));

    let close = if measured.len() < MIN_FILES {
        measured.len()
    } else {
        let cutoff = cutoff(measured.len());
        measured.iter().take_while(|(_, _, z)| *z >= cutoff).count()
    };
    Ranked {
        files: measured.into_iter().map(|(file, score, _)| (file, score)).collect(),
        close,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PICTURE: i64 = 0;

    fn spread(first: i64, count: usize, kind: i64, low: f32, high: f32) -> Vec<(i64, i64, f32)> {
        (0..count)
            .map(|n| {
                (
                    first + n as i64,
                    kind,
                    low + (high - low) * n as f32 / (count - 1).max(1) as f32,
                )
            })
            .collect()
    }

    fn ids(ranked: &Ranked) -> Vec<i64> {
        ranked.files.iter().map(|(file, _)| *file).collect()
    }

    #[test]
    fn order_follows_scores() {
        let mut scored: Vec<_> = (0..120)
            .map(|n| (n, PICTURE, ((n * 37) % 101) as f32 / 400.0))
            .collect();
        scored.push((500, PICTURE, 0.9));
        let ranked = rank(&scored);
        assert!(ranked.files.windows(2).all(|pair| pair[0].1 >= pair[1].1));
    }

    #[test]
    fn nothing_close_without_outlier() {
        let ranked = rank(&spread(0, 400, PICTURE, 0.05, 0.15));
        assert_eq!((ranked.close, ranked.files.len()), (0, 400));
    }

    #[test]
    fn equal_scores() {
        let scored: Vec<_> = (0..50).rev().map(|n| (n, PICTURE, 0.2)).collect();
        let ranked = rank(&scored);
        assert_eq!(ranked.close, 0);
        assert_eq!(ids(&ranked), (0..50).collect::<Vec<_>>());
    }
}
