use std::{
    borrow::Cow,
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
    counted: bool,
    to_do: u64,
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
            counted: quiet,
            to_do: 0,
            done: 0,
            current: None,
            already: 0,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Tally {
    found: u64,
    done: u64,
}

impl Tally {
    fn shown_done(self) -> u64 {
        self.done.min(self.found)
    }
}

pub struct Dropped {
    pub root: PathBuf,
    pub id: i64,
    pub started: bool,
}

#[derive(Default)]
pub struct ProgressState {
    next_ticket: u64,
    jobs: VecDeque<ActiveJob>,
    count_from: u64,
    uncounted: usize,
    released: bool,
    unstarted: usize,
    asked: Roots,
    tallies: BTreeMap<PathBuf, Tally>,
    found: u64,
    done: u64,
    already: u64,
    checks_to_do: u64,
    checks_done: u64,
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

    pub fn upcoming(&self, most: usize) -> Vec<(u64, i64, PathBuf)> {
        self.jobs
            .iter()
            .take_while(|job| !job.started && !job.quiet)
            .take(most)
            .map(|job| (job.ticket, job.id, job.root.clone()))
            .collect()
    }

    pub fn has_waiting(&self) -> bool {
        self.unstarted > 0
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
        if !self.is_current(ticket) || !self.jobs[0].quiet {
            return;
        }
        self.jobs[0].to_do += files;
        self.checks_to_do += files;
    }

    pub fn has(&self, ticket: u64) -> bool {
        self.position_of(ticket).is_some()
    }

    #[cfg(test)]
    pub fn drop_job(&mut self, root: &Path) -> Option<i64> {
        self.drop_jobs(&[root.to_owned()]).pop().map(|dropped| dropped.id)
    }

    pub fn drop_jobs(&mut self, roots: &[PathBuf]) -> Vec<Dropped> {
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
        let mut dropped: Vec<Dropped> = positions
            .into_iter()
            .rev()
            .map(|at| self.remove_job(at))
            .map(|job| Dropped {
                root: job.root,
                id: job.id,
                started: job.started,
            })
            .collect();
        dropped.reverse();
        for Dropped { root, .. } in &dropped {
            let gone: Vec<PathBuf> = self
                .tallies_under(root)
                .filter(|folder| !self.asked.at_or_above(folder))
                .cloned()
                .collect();
            gone.iter().for_each(|folder| self.forget(folder));
        }
        self.forget_if_over();
        dropped
    }

    fn push_job(&mut self, job: ActiveJob) {
        if !job.started {
            self.unstarted += 1;
        }
        if !job.quiet {
            self.asked.add(&job.root);
        }
        if !job.counted {
            self.uncounted += 1;
        }
        self.already += job.already;
        self.jobs.push_back(job);
    }

    fn remove_job(&mut self, position: usize) -> ActiveJob {
        let job = self.jobs.remove(position).expect("a job that is there");
        if !job.counted {
            self.uncounted -= 1;
        }
        if !job.started {
            self.unstarted -= 1;
        }
        if !job.quiet {
            self.asked.remove(&job.root);
        }
        self.already -= job.already;
        self.checks_to_do -= job.to_do;
        self.checks_done -= job.done;
        job
    }

    pub fn roots_under(&self, path: &Path) -> Vec<PathBuf> {
        self.asked.under(path)
    }

    pub fn is_job_root(&self, root: &Path) -> bool {
        self.asked.contains(root)
    }

    pub fn job_root_children(&self, folder: &Path) -> HashSet<OsString> {
        self.asked.children(folder)
    }

    pub fn job_roots(&self) -> Vec<PathBuf> {
        self.asked.0.keys().cloned().collect()
    }

    pub fn is_active(&self) -> bool {
        !self.jobs.is_empty()
    }

    pub fn has_asked_for(&self) -> bool {
        self.jobs.iter().any(|job| !job.quiet)
    }

    fn position_of(&self, ticket: u64) -> Option<usize> {
        self.jobs.binary_search_by_key(&ticket, |job| job.ticket).ok()
    }

    fn is_current(&self, ticket: u64) -> bool {
        self.jobs.front().is_some_and(|job| job.ticket == ticket)
    }

    fn new_run(&mut self) {
        self.released = false;
        self.tallies = BTreeMap::new();
        self.found = 0;
        self.done = 0;
        self.already = 0;
        self.checks_to_do = 0;
        self.checks_done = 0;
    }

    fn forget_if_over(&mut self) {
        if self.jobs.is_empty() {
            self.new_run();
        }
    }

    fn tallies_under<'a>(&'a self, path: &'a Path) -> impl Iterator<Item = &'a PathBuf> + 'a {
        self.tallies
            .range::<Path, _>((Bound::Included(path), Bound::Unbounded))
            .map(|(folder, _)| folder)
            .take_while(move |folder| folder.starts_with(path))
    }

    fn change(&mut self, folder: &Path, change: impl FnOnce(&mut Tally)) {
        if !self.tallies.contains_key(folder) {
            self.tallies.insert(folder.to_owned(), Tally::default());
        }
        let tally = self.tallies.get_mut(folder).expect("just made");
        let before = *tally;
        change(tally);
        self.found = self.found - before.found + tally.found;
        self.done = self.done - before.shown_done() + tally.shown_done();
    }

    fn forget(&mut self, folder: &Path) {
        if let Some(tally) = self.tallies.remove(folder) {
            self.found -= tally.found;
            self.done -= tally.shown_done();
        }
    }

    #[cfg(test)]
    pub fn begin(&mut self, ticket: u64) -> bool {
        if !self.is_current(ticket) {
            return false;
        }
        if !self.jobs[0].started {
            self.jobs[0].started = true;
            self.unstarted -= 1;
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
                self.unstarted -= 1;
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
        Some(job.counted && (self.released || self.uncounted == 0))
    }

    pub fn release(&mut self) {
        self.released = true;
    }

    #[cfg(test)]
    pub fn found(&mut self, ticket: u64, files: u64) {
        if let Some(position) = self.position_of(ticket) {
            let root = self.jobs[position].root.clone();
            let before = self.tallies.get(&root).map_or(0, |tally| tally.found);
            self.found_in(ticket, &root, before + files);
        }
    }

    pub fn found_in(&mut self, ticket: u64, folder: &Path, files: u64) {
        if self
            .position_of(ticket)
            .is_some_and(|position| !self.jobs[position].quiet)
        {
            self.change(folder, |tally| tally.found = files);
        }
    }

    pub fn counted_already(&mut self, ticket: u64, folders: &[(PathBuf, u64)]) {
        if !self.has(ticket) {
            return;
        }
        for (folder, files) in folders {
            self.found_in(ticket, folder, *files);
        }
        let position = self.position_of(ticket).expect("there");
        self.jobs[position].count_taken = true;
        self.finish_counting(ticket);
    }

    pub fn finish_counting(&mut self, ticket: u64) {
        if let Some(position) = self.position_of(ticket)
            && !self.jobs[position].counted
        {
            self.jobs[position].counted = true;
            self.uncounted -= 1;
        }
    }

    pub fn excluded(&mut self, path: &Path) {
        let gone: Vec<PathBuf> = self.tallies_under(path).cloned().collect();
        gone.iter().for_each(|folder| self.forget(folder));
    }

    #[cfg(test)]
    pub fn processed(&mut self, ticket: u64, files: u64) {
        if let Some(position) = self.position_of(ticket) {
            let root = self.jobs[position].root.clone();
            self.processed_in(ticket, files, &root);
        }
    }

    pub fn processed_in(&mut self, ticket: u64, files: u64, folder: &Path) {
        let Some(position) = self.position_of(ticket) else {
            return;
        };
        let job = &mut self.jobs[position];
        if job.current.as_deref() != Some(folder) {
            job.current = Some(folder.to_owned());
        }
        if job.quiet {
            job.done += files;
            self.checks_done += files;
            return;
        }
        let tallied = if folder.starts_with(&job.root) {
            Cow::Borrowed(folder)
        } else {
            Cow::Owned(job.root.clone())
        };
        self.change(&tallied, |tally| tally.done += files);
    }

    pub fn finish_job(&mut self, ticket: u64) -> bool {
        let Some(position) = self.position_of(ticket) else {
            return false;
        };
        let job = self.remove_job(position);
        if job.quiet {
            self.checks_to_do += job.to_do;
            self.checks_done += job.to_do;
        } else {
            let whole: Vec<PathBuf> = self
                .tallies_under(&job.root)
                .filter(|folder| {
                    !folder
                        .ancestors()
                        .take_while(|above| above.starts_with(&job.root))
                        .any(|above| self.asked.contains(above))
                })
                .cloned()
                .collect();
            whole
                .iter()
                .for_each(|folder| self.change(folder, |tally| tally.done = tally.done.max(tally.found)));
        }
        self.forget_if_over();
        true
    }

    pub fn snapshot(&self, paused: bool) -> Option<Progress> {
        let current = self.jobs.front()?;
        let counting = !current.counted || (!self.released && self.uncounted > 0);
        let total = self.found + self.checks_to_do;
        let done = (self.done.max(self.already) + self.checks_done.min(self.checks_to_do)).min(total);
        Some(Progress {
            name: browse::display_name(&current.root),
            queued: self.jobs.len() as u64 - 1,
            counting,
            found: self.found,
            done,
            total: if counting { 0 } else { total },
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
    fn stop_all_ignores_late_updates() {
        let (mut state, old) = state_with(&["a", "b"]);
        state.begin(old[0]);
        state.found(old[0], 10);
        state.finish_counting(old[0]);
        let roots = state.job_roots();
        state.drop_jobs(&roots);
        assert_eq!(state.snapshot(false), None);

        let new = state.add_job(0, root("c")).0;
        state.begin(new);
        state.found(old[0], 99);
        state.processed(old[0], 99);
        state.finish_counting(old[0]);
        state.finish_job(old[0]);
        let snapshot = state.snapshot(false).unwrap();
        assert_eq!(
            (snapshot.name.as_str(), snapshot.found, snapshot.done, snapshot.total),
            ("c", 0, 0, 0)
        );
        assert!(snapshot.counting);
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
