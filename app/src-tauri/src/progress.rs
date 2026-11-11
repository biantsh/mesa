use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    ffi::{OsStr, OsString},
    ops::Bound,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::browse;

const FOLDERS_SHOWN: usize = 2;

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub name: String,
    pub queued: u64,
    pub counting: bool,
    pub found: u64,
    pub done: u64,
    pub total: u64,
    pub paused: bool,
    pub updating: bool,
    pub folders: Vec<String>,
    pub turbo: bool,
}

fn folders_below(root: &Path, folder: Option<&Path>) -> Vec<String> {
    let Some(below) = folder.and_then(|folder| folder.strip_prefix(root).ok()) else {
        return Vec::new();
    };
    below
        .components()
        .take(FOLDERS_SHOWN)
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect()
}

struct ActiveJob {
    ticket: u64,
    id: i64,
    root: PathBuf,
    started: bool,
    quiet: bool,
    count_taken: bool,
    found: u64,
    total: Option<u64>,
    done: u64,
    current: Option<PathBuf>,
    already: u64,
}

impl ActiveJob {
    fn new(ticket: u64, id: i64, root: PathBuf, quiet: bool) -> Self {
        Self {
            ticket,
            id,
            root,
            started: quiet,
            quiet,
            count_taken: quiet,
            found: 0,
            total: quiet.then_some(0),
            done: 0,
            current: None,
            already: 0,
        }
    }
}

#[derive(Default)]
pub struct ProgressState {
    next_ticket: u64,
    jobs: VecDeque<ActiveJob>,
    count_from: u64,
    untotalled: usize,
    finished: u64,
    released: bool,
    waiting: Roots,
    asked: Roots,
}

#[derive(Default)]
struct Roots(BTreeMap<PathBuf, usize>);

impl Roots {
    fn add(&mut self, path: &Path) {
        *self.0.entry(path.to_owned()).or_default() += 1;
    }

    fn remove(&mut self, path: &Path) {
        if let Some(count) = self.0.get_mut(path) {
            *count -= 1;
            if *count == 0 {
                self.0.remove(path);
            }
        }
    }

    fn contains(&self, path: &Path) -> bool {
        self.0.contains_key(path)
    }

    fn at_or_above(&self, path: &Path) -> bool {
        path.ancestors().any(|above| self.0.contains_key(above))
    }

    fn at_or_below(&self, path: &Path) -> bool {
        self.0
            .range::<Path, _>((Bound::Included(path), Bound::Unbounded))
            .next()
            .is_some_and(|(below, _)| below.starts_with(path))
    }

    fn of_children(&self, folder: &Path) -> (bool, HashSet<OsString>) {
        let above = folder.ancestors().any(|above| self.0.contains_key(above));
        (
            above,
            crate::index::children_with_something(crate::index::strictly_below(&self.0, folder), folder),
        )
    }

    fn children(&self, folder: &Path) -> HashSet<OsString> {
        crate::index::strictly_below(&self.0, folder)
            .filter(|path| path.parent() == Some(folder))
            .filter_map(|path| path.file_name().map(OsStr::to_owned))
            .collect()
    }

    fn under(&self, path: &Path) -> Vec<PathBuf> {
        self.0
            .range::<Path, _>((Bound::Included(path), Bound::Unbounded))
            .take_while(|(below, _)| below.starts_with(path))
            .flat_map(|(below, &count)| std::iter::repeat_n(below.clone(), count))
            .collect()
    }
}

impl ProgressState {
    #[cfg(test)]
    pub fn add_job(&mut self, id: i64, root: PathBuf) -> (u64, bool) {
        self.add_job_having_done(id, root, 0)
    }

    pub fn add_job_having_done(&mut self, id: i64, root: PathBuf, already: u64) -> (u64, bool) {
        let first = self.jobs.is_empty();
        if first {
            self.new_run();
        }
        self.next_ticket += 1;
        let mut job = ActiveJob::new(self.next_ticket, id, root, false);
        job.already = already;
        self.push_job(job);
        (self.next_ticket, first)
    }

    pub fn add_quiet(&mut self, root: PathBuf) -> u64 {
        if self.jobs.is_empty() {
            self.new_run();
        }
        self.released = true;
        self.next_ticket += 1;
        self.push_job(ActiveJob::new(self.next_ticket, 0, root, true));
        self.next_ticket
    }

    pub fn add_total(&mut self, ticket: u64, files: u64) {
        if !self.is_current(ticket) {
            return;
        }
        let total = self.jobs[0].total.unwrap_or(0) + files;
        self.set_total(0, total);
    }

    pub fn has(&self, ticket: u64) -> bool {
        self.position_of(ticket).is_some()
    }

    #[cfg(test)]
    pub fn drop_job(&mut self, root: &Path) -> Option<i64> {
        self.drop_jobs(&[root.to_owned()]).pop().map(|(_, id)| id)
    }

    pub fn drop_jobs(&mut self, roots: &[PathBuf]) -> Vec<(PathBuf, i64)> {
        let mut wanted: HashMap<&Path, usize> = HashMap::new();
        for root in roots.iter().filter(|root| self.asked.contains(root)) {
            *wanted.entry(root.as_path()).or_default() += 1;
        }
        if wanted.is_empty() {
            return Vec::new();
        }
        let positions: Vec<usize> = (0..self.jobs.len())
            .filter(|&at| {
                let job = &self.jobs[at];
                !job.quiet
                    && wanted.get_mut(job.root.as_path()).is_some_and(|left| {
                        *left > 0 && {
                            *left -= 1;
                            true
                        }
                    })
            })
            .collect();
        let mut dropped: Vec<(PathBuf, i64)> = positions
            .into_iter()
            .rev()
            .map(|at| self.remove_job(at))
            .map(|job| (job.root, job.id))
            .collect();
        dropped.reverse();
        self.forget_if_over();
        dropped
    }

    fn push_job(&mut self, job: ActiveJob) {
        if !job.started {
            self.waiting.add(&job.root);
        }
        if !job.quiet {
            self.asked.add(&job.root);
        }
        if job.total.is_none() {
            self.untotalled += 1;
        }
        self.jobs.push_back(job);
    }

    fn remove_job(&mut self, position: usize) -> ActiveJob {
        let job = self.jobs.remove(position).expect("a job that is there");
        if job.total.is_none() {
            self.untotalled -= 1;
        }
        if !job.started {
            self.waiting.remove(&job.root);
        }
        if !job.quiet {
            self.asked.remove(&job.root);
        }
        job
    }

    pub fn roots_under(&self, path: &Path) -> Vec<PathBuf> {
        self.asked.under(path)
    }

    pub fn is_job_root(&self, root: &Path) -> bool {
        self.asked.contains(root)
    }

    pub fn waiting_children(&self, folder: &Path) -> (bool, HashSet<OsString>) {
        self.waiting.of_children(folder)
    }

    pub fn job_root_children(&self, folder: &Path) -> HashSet<OsString> {
        self.asked.children(folder)
    }

    pub fn job_roots(&self) -> Vec<PathBuf> {
        self.asked.0.keys().cloned().collect()
    }

    pub fn is_waiting(&self, path: &Path) -> bool {
        self.waiting.at_or_above(path) || self.waiting.at_or_below(path)
    }

    pub fn is_active(&self) -> bool {
        !self.jobs.is_empty()
    }

    #[cfg(test)]
    pub fn cancel_all(&mut self) -> Vec<(i64, PathBuf)> {
        self.new_run();
        self.waiting = Roots::default();
        self.asked = Roots::default();
        self.untotalled = 0;
        self.jobs
            .drain(..)
            .filter(|job| !job.quiet)
            .map(|job| (job.id, job.root))
            .collect()
    }

    #[cfg(test)]
    pub fn cancel_check(&mut self) -> bool {
        if !self.jobs.front().is_some_and(|job| job.quiet) {
            return false;
        }
        self.remove_job(0);
        self.forget_if_over();
        true
    }

    fn job_mut(&mut self, ticket: u64) -> Option<&mut ActiveJob> {
        let position = self.position_of(ticket)?;
        self.jobs.get_mut(position)
    }

    fn position_of(&self, ticket: u64) -> Option<usize> {
        self.jobs.binary_search_by_key(&ticket, |job| job.ticket).ok()
    }

    fn set_total(&mut self, position: usize, total: u64) {
        let job = &mut self.jobs[position];
        if job.total.is_none() {
            self.untotalled -= 1;
        }
        job.total = Some(total);
    }

    fn is_current(&self, ticket: u64) -> bool {
        self.jobs.front().is_some_and(|job| job.ticket == ticket)
    }

    fn new_run(&mut self) {
        self.finished = 0;
        self.released = false;
    }

    fn forget_if_over(&mut self) {
        if self.jobs.is_empty() {
            self.new_run();
        }
    }

    pub fn begin(&mut self, ticket: u64) -> bool {
        if !self.is_current(ticket) {
            return false;
        }
        if !self.jobs[0].started {
            self.jobs[0].started = true;
            let root = self.jobs[0].root.clone();
            self.waiting.remove(&root);
        }
        true
    }

    pub fn begin_together(&mut self, tickets: &[u64]) -> Vec<u64> {
        let mut begun = Vec::new();
        for &ticket in tickets {
            let Some(position) = self.position_of(ticket) else {
                continue;
            };
            if position != begun.len() {
                break;
            }
            let job = &mut self.jobs[position];
            if !job.started {
                job.started = true;
                let root = job.root.clone();
                self.waiting.remove(&root);
            }
            begun.push(ticket);
        }
        begun
    }

    pub fn next_to_count(&mut self) -> Option<(u64, PathBuf)> {
        let from = self.jobs.partition_point(|job| job.ticket < self.count_from);
        let Some(position) = (from..self.jobs.len()).find(|&at| !self.jobs[at].count_taken) else {
            self.count_from = self.next_ticket + 1;
            return None;
        };
        let job = &mut self.jobs[position];
        job.count_taken = true;
        self.count_from = job.ticket + 1;
        Some((job.ticket, job.root.clone()))
    }

    pub fn ready(&self, ticket: u64) -> Option<bool> {
        let job = &self.jobs[self.position_of(ticket)?];
        Some(job.total.is_some() && (self.released || self.untotalled == 0))
    }

    pub fn release(&mut self) {
        self.released = true;
    }

    pub fn found(&mut self, ticket: u64, files: u64) {
        if let Some(job) = self.job_mut(ticket) {
            job.found += files;
        }
    }

    pub fn counted_already(&mut self, ticket: u64, total: u64) {
        if let Some(position) = self.position_of(ticket) {
            let job = &mut self.jobs[position];
            job.count_taken = true;
            job.found = total;
            self.set_total(position, total);
        }
    }

    pub fn finish_counting(&mut self, ticket: u64) {
        if let Some(position) = self.position_of(ticket) {
            let found = self.jobs[position].found;
            self.set_total(position, found);
        }
    }

    pub fn processed(&mut self, ticket: u64, files: u64) {
        if let Some(job) = self.job_mut(ticket) {
            job.done += files;
        }
    }

    pub fn processed_in(&mut self, ticket: u64, files: u64, folder: &Path) {
        if let Some(job) = self.job_mut(ticket) {
            job.done += files;
            if job.current.as_deref() != Some(folder) {
                job.current = Some(folder.to_owned());
            }
        }
    }

    pub fn finish_job(&mut self, ticket: u64) -> bool {
        let Some(position) = self.position_of(ticket) else {
            return false;
        };
        let job = self.remove_job(position);
        self.finished += job.total.unwrap_or(0);
        self.forget_if_over();
        true
    }

    pub fn snapshot(&self, paused: bool) -> Option<Progress> {
        let current = self.jobs.front()?;
        let counted: u64 = self.jobs.iter().filter_map(|job| job.total).sum();
        let counting = current.total.is_none() || (!self.released && self.untotalled > 0);
        let done: u64 = self
            .jobs
            .iter()
            .map(|job| job.done.max(job.already).min(job.total.unwrap_or(0)))
            .sum();
        Some(Progress {
            name: browse::display_name(&current.root),
            queued: self.jobs.len() as u64 - 1,
            counting,
            found: self.finished + self.jobs.iter().map(|job| job.found).sum::<u64>(),
            done: self.finished + done,
            total: if counting { 0 } else { self.finished + counted },
            paused,
            updating: current.quiet,
            folders: folders_below(&current.root, current.current.as_deref()),
            turbo: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        PathBuf::from("folder").join(name)
    }

    fn state_with(names: &[&str]) -> (ProgressState, Vec<u64>) {
        let mut state = ProgressState::default();
        let tickets = names.iter().map(|name| state.add_job(0, root(name)).0).collect();
        (state, tickets)
    }

    #[test]
    fn dropped_job_leaves_totals() {
        let (mut state, jobs) = state_with(&["a", "b"]);
        state.begin(jobs[0]);
        for job in &jobs {
            state.found(*job, 50);
            state.finish_counting(*job);
        }
        state.release();
        state.processed(jobs[0], 20);
        assert_eq!(state.snapshot(false).unwrap().total, 100);

        assert!(state.drop_job(&root("b")).is_some());
        let snapshot = state.snapshot(false).unwrap();
        assert_eq!((snapshot.done, snapshot.total, snapshot.queued), (20, 50, 0));
    }

    #[test]
    fn done_never_exceeds_total() {
        let (mut state, jobs) = state_with(&["a"]);
        state.begin(jobs[0]);
        state.found(jobs[0], 3);
        state.finish_counting(jobs[0]);
        state.processed(jobs[0], 4);
        assert_eq!(state.snapshot(false).unwrap().done, 3);
    }
}
