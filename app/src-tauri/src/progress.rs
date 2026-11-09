use std::path::{Path, PathBuf};

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
    jobs: Vec<ActiveJob>,
    finished: u64,
    released: bool,
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
        self.jobs.push(job);
        (self.next_ticket, first)
    }

    pub fn add_quiet(&mut self, root: PathBuf) -> u64 {
        if self.jobs.is_empty() {
            self.new_run();
        }
        self.released = true;
        self.next_ticket += 1;
        self.jobs.push(ActiveJob::new(self.next_ticket, 0, root, true));
        self.next_ticket
    }

    pub fn add_total(&mut self, ticket: u64, files: u64) {
        if !self.is_current(ticket) {
            return;
        }
        self.jobs[0].total = Some(self.jobs[0].total.unwrap_or(0) + files);
    }

    pub fn has(&self, ticket: u64) -> bool {
        self.jobs.iter().any(|job| job.ticket == ticket)
    }

    pub fn drop_job(&mut self, root: &Path) -> Option<i64> {
        let position = self.jobs.iter().position(|job| !job.quiet && job.root == root)?;
        let job = self.jobs.remove(position);
        self.forget_if_over();
        Some(job.id)
    }

    pub fn roots_under(&self, path: &Path) -> Vec<PathBuf> {
        self.jobs
            .iter()
            .filter(|job| !job.quiet && job.root.starts_with(path))
            .map(|job| job.root.clone())
            .collect()
    }

    pub fn is_job_root(&self, root: &Path) -> bool {
        self.jobs.iter().any(|job| !job.quiet && job.root == root)
    }

    pub fn is_waiting(&self, path: &Path) -> bool {
        self.jobs
            .iter()
            .any(|job| !job.started && (path.starts_with(&job.root) || job.root.starts_with(path)))
    }

    pub fn is_active(&self) -> bool {
        !self.jobs.is_empty()
    }

    #[cfg(test)]
    pub fn cancel_all(&mut self) -> Vec<(i64, PathBuf)> {
        self.new_run();
        self.jobs
            .drain(..)
            .filter(|job| !job.quiet)
            .map(|job| (job.id, job.root))
            .collect()
    }

    #[cfg(test)]
    pub fn cancel_check(&mut self) -> bool {
        if !self.jobs.first().is_some_and(|job| job.quiet) {
            return false;
        }
        self.jobs.remove(0);
        self.forget_if_over();
        true
    }

    fn job_mut(&mut self, ticket: u64) -> Option<&mut ActiveJob> {
        self.jobs.iter_mut().find(|job| job.ticket == ticket)
    }

    fn is_current(&self, ticket: u64) -> bool {
        self.jobs.first().is_some_and(|job| job.ticket == ticket)
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
        self.jobs[0].started = true;
        true
    }

    pub fn next_to_count(&mut self) -> Option<(u64, PathBuf)> {
        let job = self.jobs.iter_mut().find(|job| !job.count_taken)?;
        job.count_taken = true;
        Some((job.ticket, job.root.clone()))
    }

    pub fn ready(&self, ticket: u64) -> Option<bool> {
        let job = self.jobs.iter().find(|job| job.ticket == ticket)?;
        Some(job.total.is_some() && (self.released || self.jobs.iter().all(|job| job.total.is_some())))
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
        if let Some(job) = self.job_mut(ticket) {
            job.count_taken = true;
            job.found = total;
            job.total = Some(total);
        }
    }

    pub fn finish_counting(&mut self, ticket: u64) {
        if let Some(job) = self.job_mut(ticket) {
            job.total = Some(job.found);
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
        let Some(position) = self.jobs.iter().position(|job| job.ticket == ticket) else {
            return false;
        };
        let job = self.jobs.remove(position);
        self.finished += job.total.unwrap_or(0);
        self.forget_if_over();
        true
    }

    pub fn snapshot(&self, paused: bool) -> Option<Progress> {
        let current = self.jobs.first()?;
        let counted: u64 = self.jobs.iter().filter_map(|job| job.total).sum();
        let counting = current.total.is_none() || (!self.released && self.jobs.iter().any(|job| job.total.is_none()));
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
