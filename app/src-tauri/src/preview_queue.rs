use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::{Condvar, Mutex, MutexGuard, PoisonError},
};

use crate::thumbnail::{Key, Shape};

const WARM_REACH: usize = 4_000;
const PARKED_MOST: usize = 2_000;

pub struct Folder {
    entries: Vec<Option<(String, Shape)>>,
    places: HashMap<String, usize>,
}

impl Folder {
    pub fn new(entries: Vec<Option<(String, Shape)>>) -> Self {
        let places = entries
            .iter()
            .enumerate()
            .filter_map(|(at, entry)| entry.as_ref().map(|(path, _)| (path.clone(), at)))
            .collect();
        Self { entries, places }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Job {
    Asked(Key),
    Warm(String, Shape),
}

#[derive(Default)]
struct State {
    other: Vec<Key>,
    shown: BTreeMap<usize, Vec<Key>>,
    parked: VecDeque<Key>,
    folder: Option<Folder>,
    generation: u64,
    early: Option<(u64, usize, usize, bool)>,
    seen: (usize, usize),
    warm: bool,
    looked: Vec<bool>,
    after: usize,
    before: usize,
}

impl State {
    fn distance(&self, at: usize) -> usize {
        let (low, high) = self.seen;
        if at < low {
            low - at
        } else if at >= high {
            at + 1 - high
        } else {
            0
        }
    }

    fn file(&mut self, key: Key, parking: bool) {
        if !matches!(key.2, Shape::Box { .. }) {
            return self.other.push(key);
        }
        match self.folder.as_ref().map(|folder| folder.places.get(&key.0).copied()) {
            Some(Some(at)) => self.shown.entry(at).or_default().push(key),
            Some(None) => self.parked.push_back(key),
            None if parking => self.parked.push_back(key),
            None => self.other.push(key),
        }
    }

    fn apply_window(&mut self, start: usize, end: usize, warm: bool) {
        let count = self.looked.len();
        let start = start.min(count);
        self.seen = (start, end.clamp(start, count));
        self.warm = warm;
        self.after = start;
        self.before = start;
    }

    fn nearest_asked(&self) -> Option<(usize, usize)> {
        let low = self.seen.0;
        let below = self.shown.range(low..).next().map(|(&at, _)| at);
        let above = self.shown.range(..low).next_back().map(|(&at, _)| at);
        [below, above]
            .into_iter()
            .flatten()
            .map(|at| (self.distance(at), at))
            .min()
            .map(|(distance, at)| (at, distance))
    }

    fn nearest_warm(&mut self) -> Option<(usize, usize)> {
        if !self.warm {
            return None;
        }
        let count = self.looked.len();
        while self.after < count && self.looked[self.after] {
            self.after += 1;
        }
        while self.before > 0 && self.looked[self.before - 1] {
            self.before -= 1;
        }
        let below = (self.after < count).then_some(self.after);
        let above = self.before.checked_sub(1);
        [below, above]
            .into_iter()
            .flatten()
            .map(|at| (self.distance(at), at))
            .filter(|&(distance, _)| distance <= WARM_REACH)
            .min()
            .map(|(distance, at)| (at, distance))
    }

    fn next(&mut self) -> Option<Job> {
        if let Some(key) = self.other.pop() {
            return Some(Job::Asked(key));
        }
        let asked = self.nearest_asked();
        let warm = self.nearest_warm();
        match (asked, warm) {
            (Some((_, asked_distance)), Some((at, warm_distance))) if warm_distance < asked_distance => {
                Some(self.take_warm(at))
            }
            (Some((at, _)), _) => {
                let keys = self.shown.get_mut(&at)?;
                let key = keys.pop()?;
                if keys.is_empty() {
                    self.shown.remove(&at);
                }
                Some(Job::Asked(key))
            }
            (None, Some((at, _))) => Some(self.take_warm(at)),
            (None, None) => None,
        }
    }

    fn take_warm(&mut self, at: usize) -> Job {
        self.looked[at] = true;
        let (path, shape) = self
            .folder
            .as_ref()
            .and_then(|folder| folder.entries[at].clone())
            .expect("only an entry with a preview is looked at");
        Job::Warm(path, shape)
    }
}

pub struct Queue {
    state: Mutex<State>,
    ready: Condvar,
}

impl Default for Queue {
    fn default() -> Self {
        Self {
            state: Mutex::default(),
            ready: Condvar::new(),
        }
    }
}

impl Queue {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn push(&self, key: Key) -> Vec<Key> {
        let mut state = self.lock();
        state.file(key, false);
        let over = state.parked.len().saturating_sub(PARKED_MOST);
        let given_back = state.parked.drain(..over).collect();
        drop(state);
        self.ready.notify_one();
        given_back
    }

    pub fn keep_parked(&self, keys: Vec<Key>) {
        self.lock().parked.extend(keys);
    }

    pub fn set_folder(&self, generation: u64, folder: Option<Folder>) {
        let mut state = self.lock();
        if generation < state.generation {
            return;
        }
        state.generation = generation;
        let mut asked: Vec<Key> = std::mem::take(&mut state.shown).into_values().flatten().collect();
        asked.extend(std::mem::take(&mut state.parked));
        state.looked = folder
            .as_ref()
            .map_or_else(Vec::new, |folder| folder.entries.iter().map(Option::is_none).collect());
        state.folder = folder;
        state.apply_window(0, 0, false);
        for key in asked {
            state.file(key, true);
        }
        if let Some((said, start, end, warm)) = state.early.take() {
            if said == generation {
                state.apply_window(start, end, warm);
            } else if said > generation {
                state.early = Some((said, start, end, warm));
            }
        }
        drop(state);
        self.ready.notify_all();
    }

    pub fn set_window(&self, generation: u64, start: usize, end: usize, warm: bool) {
        let mut state = self.lock();
        if generation < state.generation {
            return;
        }
        if generation > state.generation {
            state.early = Some((generation, start, end, warm));
            return;
        }
        state.apply_window(start, end, warm);
        drop(state);
        self.ready.notify_all();
    }

    pub fn pop(&self) -> Job {
        let mut state = self.lock();
        loop {
            if let Some(job) = state.next() {
                return job;
            }
            state = self.ready.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
    }

    #[cfg(test)]
    fn try_pop(&self) -> Option<Job> {
        self.lock().next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thumbnail::Fit;

    const TILE: Shape = Shape::Box {
        width: 152,
        height: 88,
        fit: Fit::Cover,
    };

    fn tile(path: &str) -> Key {
        (path.to_owned(), None, TILE)
    }

    fn folder(count: usize, without: &[usize]) -> Folder {
        Folder::new(
            (0..count)
                .map(|n| (!without.contains(&n)).then(|| (n.to_string(), TILE)))
                .collect(),
        )
    }

    fn drain(queue: &Queue) -> Vec<Job> {
        std::iter::from_fn(|| queue.try_pop()).collect()
    }

    fn asked(paths: &[&str]) -> Vec<Job> {
        paths.iter().map(|path| Job::Asked(tile(path))).collect()
    }

    #[test]
    fn visible_first_then_nearest() {
        let queue = Queue::default();
        queue.set_folder(1, Some(folder(100, &[])));
        queue.set_window(1, 40, 44, false);
        for n in [30, 41, 50, 39, 44, 43] {
            queue.push(tile(&n.to_string()));
        }
        assert_eq!(drain(&queue), asked(&["41", "43", "39", "44", "50", "30"]));
    }
}
