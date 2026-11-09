use std::{
    mem,
    path::{Path, PathBuf},
    sync::{TryLockError, Weak},
    thread,
    time::Duration,
};

use crate::{
    index::{Shared, lock},
    power, store,
    store::Reclaimable,
    vectors::RECORD_BYTES,
};

const FIRST_LOOK: Duration = Duration::from_secs(20);
const LOOK_AGAIN: Duration = Duration::from_secs(5);
const REST: Duration = Duration::from_millis(15);
const VECTORS_PER_STEP: usize = 256;
const PAGES_PER_STEP: u32 = 64;
const WORTH_BYTES: u64 = 1 << 20;
const WORTH_SMALL_BYTES: u64 = 64 << 10;

fn worth_it(free: u64, total: u64) -> bool {
    free >= WORTH_BYTES || (free >= WORTH_SMALL_BYTES && free.saturating_mul(4) >= total)
}

impl Reclaimable {
    fn vectors_worth_it(&self) -> bool {
        worth_it(self.free_slots * RECORD_BYTES as u64, self.slots * RECORD_BYTES as u64)
    }

    fn pages_worth_it(&self) -> bool {
        worth_it(self.free_pages * self.page_size, self.pages * self.page_size)
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Step {
    Nothing,
    More,
}

#[derive(Default)]
pub struct Janitor {
    needs_checkpoint: bool,
}

impl Janitor {
    pub fn step(&mut self, shared: &Shared, dir: &Path) -> Step {
        if shared.is_busy() {
            return Step::Nothing;
        }
        let mut store = match shared.store.try_lock() {
            Ok(store) => store,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return Step::Nothing,
        };
        let Ok(found) = store.reclaimable() else {
            return Step::Nothing;
        };

        if found.vectors_worth_it() {
            match store.begin_compaction(VECTORS_PER_STEP) {
                Ok(Some(copied)) => {
                    self.needs_checkpoint = true;
                    drop(store);
                    let _ = copied.flush();
                    let _ = lock(&shared.store).finish_compaction(copied);
                    return Step::More;
                }
                Ok(None) if store.trim_vectors().is_ok_and(|cut| cut > 0) => {
                    self.needs_checkpoint = true;
                    return Step::More;
                }
                Ok(None) => {}
                Err(_) => return Step::Nothing,
            }
        }
        if found.pages_worth_it() {
            return if store.give_back_pages(PAGES_PER_STEP).is_ok() {
                self.needs_checkpoint = true;
                Step::More
            } else {
                Step::Nothing
            };
        }
        drop(store);
        if mem::take(&mut self.needs_checkpoint) {
            store::checkpoint(dir);
        }
        Step::Nothing
    }
}

pub fn spawn(shared: Weak<Shared>, dir: PathBuf) -> std::io::Result<()> {
    thread::Builder::new().name("reclaim".into()).spawn(move || {
        power::give_way();
        thread::sleep(FIRST_LOOK);
        let mut janitor = Janitor::default();
        loop {
            let next = {
                let Some(shared) = shared.upgrade() else { break };
                janitor.step(&shared, &dir)
            };
            thread::sleep(if next == Step::More { REST } else { LOOK_AGAIN });
        }
    })?;
    Ok(())
}
