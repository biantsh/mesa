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

#[derive(Debug, PartialEq, Default)]
pub struct Ranked {
    pub files: Vec<(i64, f32)>,
    pub close: usize,
    pub reasons: HashMap<i64, Reason>,
    pub at: HashMap<i64, u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Reason {
    Words,
    Meaning,
    FewFiles,
}

impl Ranked {
    pub fn all_close(files: Vec<(i64, f32)>) -> Self {
        let close = files.len();
        Self {
            files,
            close,
            ..Self::default()
        }
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

pub fn rank(scored: &[(i64, i64, f32)], among: &[(i64, i64, f32)], by_words: &HashMap<i64, f64>) -> Ranked {
    let mut by_kind: HashMap<i64, Vec<f64>> = HashMap::new();
    for &(_, kind, score) in among {
        by_kind.entry(kind).or_default().push(f64::from(score));
    }
    let kinds: HashMap<i64, Usual> = by_kind
        .into_iter()
        .map(|(kind, scores)| (kind, Usual::of(scores)))
        .collect();
    let all = kinds
        .values()
        .any(|usual| usual.count < 2.0)
        .then(|| Usual::of(among.iter().map(|&(_, _, score)| f64::from(score)).collect()));
    let (spread, counted) = kinds
        .values()
        .filter(|usual| usual.count >= 2.0)
        .fold((0.0, 0.0), |(spread, counted), usual| {
            (spread + usual.variance * usual.count, counted + usual.count)
        });
    let shared = if counted > 0.0 { spread / counted } else { 0.0 };

    let unscored: Vec<i64> = {
        let listed: std::collections::HashSet<i64> = scored.iter().map(|&(file, _, _)| file).collect();
        by_words.keys().copied().filter(|file| !listed.contains(file)).collect()
    };
    let searched = scored.len() + unscored.len();
    let line = cutoff(searched);
    let by_words = |file: i64| by_words.get(&file).map_or(f64::NEG_INFINITY, |margin| line + margin);
    let few = searched < MIN_FILES;
    let mut reasons: HashMap<i64, Reason> = unscored.iter().map(|&file| (file, Reason::Words)).collect();
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
            let words = by_words(file);
            let reason = if words > f64::NEG_INFINITY && words >= z {
                Some(Reason::Words)
            } else if z >= line {
                Some(Reason::Meaning)
            } else {
                few.then_some(Reason::FewFiles)
            };
            if let Some(reason) = reason {
                reasons.insert(file, reason);
            }
            (file, score, z.max(words))
        })
        .chain(unscored.into_iter().map(|file| (file, 0.0, by_words(file))))
        .collect();
    measured.sort_unstable_by(|a, b| b.2.total_cmp(&a.2).then(a.0.cmp(&b.0)));

    let close = if few {
        searched
    } else {
        measured.iter().take_while(|(_, _, z)| *z >= line).count()
    };
    Ranked {
        files: measured.into_iter().map(|(file, score, _)| (file, score)).collect(),
        close,
        reasons,
        at: HashMap::new(),
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

    fn rank(scored: &[(i64, i64, f32)]) -> Ranked {
        super::rank(scored, scored, &HashMap::new())
    }

    fn close(ranked: &Ranked) -> Vec<i64> {
        let mut close: Vec<i64> = ranked.files[..ranked.close].iter().map(|(file, _)| *file).collect();
        close.sort_unstable();
        close
    }

    #[test]
    fn filter_keeps_close_matches() {
        let mut photos = spread(0, 200, PICTURE, 0.50, 0.70);
        let matches: Vec<_> = (0..7).map(|n| (1_000 + n, PICTURE, 0.74 + 0.01 * n as f32)).collect();
        photos.extend(&matches);
        let mut index = photos.clone();
        index.extend(spread(2_000, 60, PICTURE, 0.56, 0.60));

        let unfiltered = rank(&index);
        let wanted: Vec<i64> = matches.iter().map(|&(file, _, _)| file).collect();
        assert_eq!(close(&unfiltered), wanted);
        let filtered = super::rank(&photos, &index, &HashMap::new());
        assert_eq!(close(&filtered), wanted);
        assert!(ids(&filtered).iter().all(|&file| file < 2_000));
        assert_eq!(close(&super::rank(&photos, &photos, &HashMap::new())), vec![1_006]);
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
