use std::{
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use tauri::{AppHandle, Manager};

use crate::{index::Index, progress::Progress};

const SHOW_AFTER: Duration = Duration::from_secs(2);
const HIDE_AFTER: Duration = Duration::from_millis(1500);
const LOOK_EVERY: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Pause,
    Resume,
    Open,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gone {
    Dismissed,
    Used,
}

pub trait Surface {
    fn show(&mut self, progress: &Progress);

    fn update(&mut self, progress: &Progress) -> bool;

    fn hide(&mut self);

    fn gone(&mut self) -> Option<Gone>;
}

struct Nowhere;

impl Surface for Nowhere {
    fn show(&mut self, _: &Progress) {}

    fn update(&mut self, _: &Progress) -> bool {
        true
    }

    fn hide(&mut self) {}

    fn gone(&mut self) -> Option<Gone> {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Show,
    Update,
    Hide,
}

#[derive(Default)]
pub struct Controller {
    since: Option<Instant>,
    absent_since: Option<Instant>,
    shown: Option<Progress>,
    dismissed: bool,
}

impl Controller {
    pub fn observe(&mut self, now: Instant, progress: Option<&Progress>) -> Option<Step> {
        let Some(progress) = progress else {
            let absent = *self.absent_since.get_or_insert(now);
            if now.duration_since(absent) < HIDE_AFTER {
                return None;
            }
            self.since = None;
            self.absent_since = None;
            self.dismissed = false;
            return self.shown.take().map(|_| Step::Hide);
        };
        self.absent_since = None;
        let since = *self.since.get_or_insert(now);
        if self.dismissed {
            return None;
        }
        match &self.shown {
            None if now.duration_since(since) >= SHOW_AFTER => {
                self.shown = Some(progress.clone());
                Some(Step::Show)
            }
            None => None,
            Some(old) if old.paused != progress.paused => {
                self.shown = Some(progress.clone());
                Some(Step::Show)
            }
            Some(old) if old != progress => {
                self.shown = Some(progress.clone());
                Some(Step::Update)
            }
            Some(_) => None,
        }
    }

    pub fn gone(&mut self, why: Gone) {
        self.shown = None;
        self.dismissed = why == Gone::Dismissed;
    }
}

#[derive(Debug, PartialEq)]
pub struct Lines {
    pub title: String,
    pub fraction: Option<f64>,
    pub figures: String,
    pub status: &'static str,
}

pub fn lines(progress: &Progress) -> Lines {
    let Progress {
        name,
        queued,
        counting,
        found,
        done,
        total,
        paused,
        updating,
    } = progress;
    let title = if *queued > 0 {
        format!("{name} + {queued} more")
    } else {
        name.clone()
    };
    let determinate = !counting && *total > 0;
    let fraction = determinate.then(|| (*done as f64 / *total as f64).clamp(0.0, 1.0));
    let figures = if determinate {
        format!("{} of {}", grouped(*done), grouped(*total))
    } else if *counting {
        format!("Finding files\u{2026} {}", grouped(*found))
    } else {
        String::new()
    };
    let status = match (paused, counting, updating) {
        (true, _, _) => "Paused",
        (_, true, _) => "Looking for files",
        (_, _, true) => "Updating",
        _ => "Indexing",
    };
    Lines {
        title,
        fraction,
        figures,
        status,
    }
}

fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

pub fn start(app: AppHandle, data_dir: PathBuf) {
    let spawned = thread::Builder::new().name("progress-surface".into()).spawn(move || {
        let act: Arc<dyn Fn(Action) + Send + Sync> = {
            let app = app.clone();
            Arc::new(move |action| act(&app, action))
        };
        let mut surface = platform(&app, &data_dir, act);
        let mut controller = Controller::default();
        loop {
            thread::sleep(LOOK_EVERY);
            if let Some(why) = surface.gone() {
                controller.gone(why);
            }
            let progress = app.state::<Index>().progress();
            match controller.observe(Instant::now(), progress.as_ref()) {
                Some(Step::Show) => surface.show(progress.as_ref().expect("shown only with progress")),
                Some(Step::Update) => {
                    if !surface.update(progress.as_ref().expect("updated only with progress")) {
                        controller.gone(Gone::Dismissed);
                    }
                }
                Some(Step::Hide) => surface.hide(),
                None => {}
            }
        }
    });
    if let Err(e) = spawned {
        eprintln!("Mesa: can't show indexing progress: {e}");
    }
}

fn act(app: &AppHandle, action: Action) {
    match action {
        Action::Pause | Action::Resume => {
            if let Err(e) = app.state::<Index>().set_paused(action == Action::Pause) {
                eprintln!("Mesa: {e}");
            }
        }
        Action::Open => crate::show_window(app),
    }
}

#[cfg(windows)]
fn platform(app: &AppHandle, data_dir: &Path, act: Arc<dyn Fn(Action) + Send + Sync>) -> Box<dyn Surface> {
    match crate::toast::Toast::new(&app.config().identifier, data_dir, act) {
        Ok(toast) => Box::new(toast),
        Err(e) => {
            eprintln!("Mesa: can't show indexing progress in the notifications: {e}");
            Box::new(Nowhere)
        }
    }
}

#[cfg(not(windows))]
fn platform(_: &AppHandle, _: &Path, _: Arc<dyn Fn(Action) + Send + Sync>) -> Box<dyn Surface> {
    Box::new(Nowhere)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_separators() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }
}
