use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::browse;

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub name: String,
    pub queued: u64,
    pub counting: bool,
    pub found: u64,
    pub done: u64,
    pub total: u64,
    pub paused: bool,
}

struct ActiveJob {
    ticket: u64,
    id: i64,
    root: PathBuf,
    started: bool,
}

#[derive(Default)]
pub struct ProgressState {
    next_ticket: u64,
    jobs: Vec<ActiveJob>,
    counting: bool,
    found: u64,
    total: u64,
    done: u64,
}

impl ProgressState {
    pub fn add_job(&mut self, id: i64, root: PathBuf) -> (u64, bool) {
        let first = self.jobs.is_empty();
        self.next_ticket += 1;
        self.jobs.push(ActiveJob {
            ticket: self.next_ticket,
            id,
            root,
            started: false,
        });
        (self.next_ticket, first)
    }

    pub fn drop_job(&mut self, root: &Path) -> Option<i64> {
        let position = self.jobs.iter().position(|job| job.root == root)?;
        if position == 0 {
            self.reset_numbers();
        }
        Some(self.jobs.remove(position).id)
    }

    pub fn is_job_root(&self, root: &Path) -> bool {
        self.jobs.iter().any(|job| job.root == root)
    }

    pub fn is_waiting(&self, path: &Path) -> bool {
        self.jobs
            .iter()
            .any(|job| !job.started && (path.starts_with(&job.root) || job.root.starts_with(path)))
    }

    pub fn is_active(&self) -> bool {
        !self.jobs.is_empty()
    }

    pub fn cancel_all(&mut self) -> Vec<(i64, PathBuf)> {
        self.reset_numbers();
        self.jobs.drain(..).map(|job| (job.id, job.root)).collect()
    }

    fn is_current(&self, ticket: u64) -> bool {
        self.jobs.first().is_some_and(|job| job.ticket == ticket)
    }

    fn reset_numbers(&mut self) {
        self.counting = false;
        self.found = 0;
        self.total = 0;
        self.done = 0;
    }

    pub fn begin(&mut self, ticket: u64) -> bool {
        if !self.is_current(ticket) {
            return false;
        }
        self.reset_numbers();
        self.counting = true;
        self.jobs[0].started = true;
        true
    }

    pub fn found(&mut self, ticket: u64, files: u64) {
        if self.is_current(ticket) {
            self.found += files;
        }
    }

    pub fn finish_counting(&mut self, ticket: u64) {
        if self.is_current(ticket) {
            self.counting = false;
            self.total = self.found;
        }
    }

    pub fn processed(&mut self, ticket: u64, files: u64) {
        if self.is_current(ticket) {
            self.done += files;
        }
    }

    pub fn finish_job(&mut self, ticket: u64) -> bool {
        if self.is_current(ticket) {
            self.reset_numbers();
        }
        let before = self.jobs.len();
        self.jobs.retain(|job| job.ticket != ticket);
        self.jobs.len() < before
    }

    pub fn snapshot(&self, paused: bool) -> Option<Progress> {
        let current = self.jobs.first()?;
        Some(Progress {
            name: browse::display_name(&current.root),
            queued: self.jobs.len() as u64 - 1,
            counting: self.counting,
            found: self.found,
            done: self.done.min(self.total),
            total: self.total,
            paused,
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
    fn done_never_exceeds_total() {
        let (mut state, jobs) = state_with(&["a"]);
        state.begin(jobs[0]);
        state.found(jobs[0], 3);
        state.finish_counting(jobs[0]);
        state.processed(jobs[0], 4);
        assert_eq!(state.snapshot(false).unwrap().done, 3);
    }
}
