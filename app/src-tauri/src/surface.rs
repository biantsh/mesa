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
#[cfg_attr(not(windows), allow(dead_code))]
pub enum Action {
    Pause,
    Resume,
    Open,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
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

#[cfg_attr(any(target_os = "macos", target_os = "linux"), allow(dead_code))]
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
        refining,
        folders: _,
        turbo: _,
    } = progress;
    let title = if *refining {
        "index".into()
    } else if *queued > 0 {
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
        _ if *refining => "Refining",
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
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
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
        Action::Open => crate::main_window::show(app),
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

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn platform(app: &AppHandle, _: &Path, _: Arc<dyn Fn(Action) + Send + Sync>) -> Box<dyn Surface> {
    Box::new(menu_bar::MenuBar { app: app.clone() })
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn platform(_: &AppHandle, _: &Path, _: Arc<dyn Fn(Action) + Send + Sync>) -> Box<dyn Surface> {
    Box::new(Nowhere)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn menu_bar_text(progress: &Progress) -> (String, String, &'static str) {
    let said = lines(progress);
    let beside_icon = match (progress.paused, said.fraction) {
        (true, _) => "\u{23F8}".to_owned(),
        (_, Some(fraction)) => format!("{}%", (fraction * 100.0).floor() as u32),
        (_, None) => "\u{2026}".to_owned(),
    };
    let figures = if said.figures.is_empty() {
        String::new()
    } else {
        format!(" \u{b7} {}", said.figures)
    };
    let line = format!("{}: {}{figures}", said.status, said.title);
    (
        beside_icon,
        line,
        if progress.paused {
            "Resume Indexing"
        } else {
            "Pause Indexing"
        },
    )
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub mod menu_bar {
    use tauri::{AppHandle, Manager, Wry, menu::MenuItem};

    use super::{Gone, Surface, menu_bar_text};
    use crate::progress::Progress;

    pub const TRAY_ID: &str = "mesa";

    pub struct Items {
        pub status: MenuItem<Wry>,
        pub pause: MenuItem<Wry>,
    }

    pub struct MenuBar {
        pub app: AppHandle,
    }

    impl MenuBar {
        fn set(&self, title: Option<&str>, status: &str, pause: &str, can_pause: bool) {
            if let Some(tray) = self.app.tray_by_id(TRAY_ID) {
                let _ = tray.set_title(title);
            }
            if let Some(items) = self.app.try_state::<Items>() {
                let _ = items.status.set_text(status);
                let _ = items.pause.set_text(pause);
                let _ = items.pause.set_enabled(can_pause);
            }
        }
    }

    impl Surface for MenuBar {
        fn show(&mut self, progress: &Progress) {
            let (title, line, pause) = menu_bar_text(progress);
            self.set(Some(&title), &line, pause, true);
        }

        fn update(&mut self, progress: &Progress) -> bool {
            self.show(progress);
            true
        }

        fn hide(&mut self) {
            self.set(None, "Not indexing", "Pause Indexing", false);
        }

        fn gone(&mut self) -> Option<Gone> {
            None
        }
    }
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
