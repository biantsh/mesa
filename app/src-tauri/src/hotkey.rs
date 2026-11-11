use std::{
    str::FromStr,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use serde::Serialize;
use tauri::{AppHandle, Manager, Wry, menu::MenuItem};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};

use crate::index::lock;

const DEFAULT: (Modifiers, Code) = (Modifiers::ALT, Code::Space);
const PAUSE_LIMIT: Duration = Duration::from_secs(60);

pub struct TrayItem(pub MenuItem<Wry>);

#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Parts {
    pub keys: Vec<String>,
    pub name: String,
}

#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub active: Option<Parts>,
    pub chosen_in: ChosenIn,
}

#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChosenIn {
    Mesa,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Desktop,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Unavailable {
        desktop: Option<String>,
    },
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum Error {
    Invalid(String),
    Failed(String),
}

impl From<Error> for String {
    fn from(error: Error) -> Self {
        match error {
            Error::Invalid(message) | Error::Failed(message) => message,
        }
    }
}

struct Inner {
    active: Option<Shortcut>,
    paused: Option<u64>,
    pauses: u64,
    chosen_in: ChosenIn,
    desktop_key: Option<Parts>,
    keys: Keys,
}

impl Inner {
    fn new(active: Option<Shortcut>, chosen_in: ChosenIn, keys: Keys) -> Self {
        Self {
            active,
            paused: None,
            pauses: 0,
            chosen_in,
            desktop_key: None,
            keys,
        }
    }

    fn is_mesas(&self) -> bool {
        self.chosen_in == ChosenIn::Mesa
    }

    fn shown(&self) -> Option<Parts> {
        if self.is_mesas() {
            self.active.as_ref().map(parts)
        } else {
            self.desktop_key.clone()
        }
    }
}

pub struct Hotkey {
    inner: Arc<Mutex<Inner>>,
}

impl Hotkey {
    pub fn new(app: &AppHandle, chosen: Option<&str>) -> Self {
        let keys = Keys::here();
        #[cfg(target_os = "linux")]
        if keys == Keys::Plugin && crate::wayland::is_wayland() {
            return Self::from_the_desktop(app, chosen);
        }
        let active = chosen
            .and_then(|text| match parse(text) {
                Ok(shortcut) => match keys.take(app, shortcut) {
                    Ok(()) => Some(shortcut),
                    Err(taken) => {
                        eprintln!("Mesa: {}", taken.sentence(&shortcut));
                        None
                    }
                },
                Err(e) => {
                    eprintln!(
                        "Mesa: the chosen search shortcut ({text}) can't be used: {}",
                        String::from(e)
                    );
                    None
                }
            })
            .or_else(|| take_default(app, keys));
        Self {
            inner: Arc::new(Mutex::new(Inner::new(active, ChosenIn::Mesa, keys))),
        }
    }

    #[cfg(target_os = "linux")]
    fn from_the_desktop(app: &AppHandle, chosen: Option<&str>) -> Self {
        let desktop = desktop_name(&crate::xdg::current_desktops());
        let inner = Arc::new(Mutex::new(Inner::new(
            None,
            ChosenIn::Unavailable { desktop },
            Keys::Plugin,
        )));
        let preferred = chosen
            .and_then(|text| parse(text).ok())
            .and_then(|shortcut| portal_trigger(&shortcut));
        let preferred = preferred
            .unwrap_or_else(|| portal_trigger(&default_shortcut()).expect("the default is a key the portal knows"));
        let (shared, app) = (inner.clone(), app.clone());
        let spawned = thread::Builder::new().name("hotkey-portal".into()).spawn(move || {
            let bound = |trigger: Option<String>| {
                let mut inner = lock(&shared);
                inner.chosen_in = ChosenIn::Desktop;
                inner.desktop_key = trigger.as_deref().map(desktop_parts);
                let name = inner.shown().map(|parts| parts.name);
                drop(inner);
                show_in_tray(&app, name.as_deref());
            };
            let pressed = || {
                let on_main = app.clone();
                let _ = app.run_on_main_thread(move || crate::widget::toggle(&on_main));
            };
            if let Err(e) = crate::wayland::bind_search(&preferred, bound, pressed) {
                eprintln!(
                    "Mesa: no search shortcut from the desktop ({e}); a shortcut of the user's own can run `{}`",
                    search_command()
                );
            }
        });
        if let Err(e) = spawned {
            eprintln!("Mesa: can't ask the desktop for a search shortcut: {e}");
        }
        Self { inner }
    }

    pub fn status(&self) -> Status {
        let inner = lock(&self.inner);
        Status {
            active: inner.shown(),
            chosen_in: inner.chosen_in.clone(),
        }
    }

    pub fn name(&self) -> Option<String> {
        lock(&self.inner).shown().map(|parts| parts.name)
    }

    pub fn change(&self, app: &AppHandle, new: Option<&str>) -> Result<Option<Shortcut>, Error> {
        let wanted = new.map(parse).transpose()?;
        let mut inner = lock(&self.inner);
        if !inner.is_mesas() {
            return Err(Error::Failed(
                "The search shortcut is chosen in your desktop's keyboard settings.".into(),
            ));
        }
        let keys = inner.keys;
        let old = inner.active;
        inner.paused = None;
        crate::system_menu::set_recording(false);
        if let Some(old) = old {
            keys.release(app, old);
        }
        let shortcut = wanted.unwrap_or_else(default_shortcut);
        let taken = keys
            .take(app, shortcut)
            .map(|()| Some(shortcut))
            .map_err(|taken| format!("{}{}", taken.sentence(&shortcut), still(old)));
        match taken {
            Ok(active) => {
                inner.active = active;
                drop(inner);
                self.show_in_tray(app);
                Ok(old)
            }
            Err(message) => {
                if let Some(old) = old {
                    let _ = keys.take(app, old);
                }
                Err(Error::Failed(message))
            }
        }
    }

    pub fn restore(&self, app: &AppHandle, old: Option<Shortcut>) {
        let mut inner = lock(&self.inner);
        if !inner.is_mesas() {
            return;
        }
        let keys = inner.keys;
        if let Some(current) = inner.active {
            keys.release(app, current);
        }
        inner.active = old
            .filter(|shortcut| keys.take(app, *shortcut).is_ok())
            .or_else(|| take_default(app, keys));
        drop(inner);
        self.show_in_tray(app);
    }

    pub fn pause(&self, app: &AppHandle) {
        let mut inner = lock(&self.inner);
        if inner.paused.is_some() || !inner.is_mesas() {
            return;
        }
        if let Some(active) = inner.active {
            inner.keys.release(app, active);
        }
        inner.pauses += 1;
        let pause = inner.pauses;
        inner.paused = Some(pause);
        crate::system_menu::set_recording(true);
        drop(inner);
        let app = app.clone();
        let _ = thread::Builder::new().name("hotkey-pause-limit".into()).spawn(move || {
            thread::sleep(PAUSE_LIMIT);
            if let Some(hotkey) = app.try_state::<Hotkey>() {
                hotkey.resume_pause(&app, Some(pause));
            }
        });
    }

    pub fn resume(&self, app: &AppHandle) {
        self.resume_pause(app, None);
    }

    fn resume_pause(&self, app: &AppHandle, only: Option<u64>) {
        let mut inner = lock(&self.inner);
        if inner.paused.is_none() || only.is_some_and(|pause| inner.paused != Some(pause)) {
            return;
        }
        inner.paused = None;
        crate::system_menu::set_recording(false);
        let Some(active) = inner.active else { return };
        let keys = inner.keys;
        if keys.take(app, active).is_err() {
            inner.active = take_default(app, keys);
            drop(inner);
            self.show_in_tray(app);
        }
    }

    fn show_in_tray(&self, app: &AppHandle) {
        show_in_tray(app, self.name().as_deref());
    }
}

fn show_in_tray(app: &AppHandle, name: Option<&str>) {
    if let Some(item) = app.try_state::<TrayItem>() {
        let _ = item.0.set_text(tray_text(name));
    }
}

#[cfg(target_os = "linux")]
fn search_command() -> String {
    let exe = crate::xdg::program().map_or_else(|| "mesa-app".to_owned(), |exe| exe.to_string_lossy().into_owned());
    command_line(&exe)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn command_line(exe: &str) -> String {
    let plain = exe.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c));
    let exe = if plain {
        exe.to_owned()
    } else {
        format!("'{}'", exe.replace('\'', "'\\''"))
    };
    format!("{exe} {}", crate::startup::SEARCH_ARG)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn desktop_name(names: &[String]) -> Option<String> {
    const KNOWN: &[(&str, &str)] = &[
        ("GNOME", "GNOME"),
        ("KDE", "KDE Plasma"),
        ("sway", "Sway"),
        ("Hyprland", "Hyprland"),
        ("COSMIC", "COSMIC"),
        ("X-Cinnamon", "Cinnamon"),
        ("Budgie", "Budgie"),
        ("XFCE", "Xfce"),
        ("LXQt", "LXQt"),
        ("MATE", "MATE"),
        ("niri", "niri"),
        ("river", "river"),
        ("Wayfire", "Wayfire"),
        ("labwc", "labwc"),
    ];
    names
        .iter()
        .find_map(|name| {
            KNOWN
                .iter()
                .find(|(id, _)| id.eq_ignore_ascii_case(name))
                .map(|(_, known)| (*known).to_owned())
        })
        .or_else(|| names.last().map(|name| (*name).to_owned()))
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn portal_trigger(shortcut: &Shortcut) -> Option<String> {
    let mut words: Vec<String> = modifiers_named(shortcut, ["CTRL", "ALT", "SHIFT", "LOGO"])
        .map(str::to_owned)
        .collect();
    words.push(keysym(shortcut.key)?);
    Some(words.join("+"))
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn modifiers_named<'a>(shortcut: &Shortcut, names: [&'a str; 4]) -> impl Iterator<Item = &'a str> {
    let held = shortcut.mods;
    [Modifiers::CONTROL, Modifiers::ALT, Modifiers::SHIFT, Modifiers::SUPER]
        .into_iter()
        .zip(names)
        .filter(move |(modifier, _)| held.contains(*modifier))
        .map(|(_, name)| name)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn desktop_parts(trigger: &str) -> Parts {
    Parts {
        keys: trigger
            .split('+')
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(str::to_owned)
            .collect(),
        name: trigger.to_owned(),
    }
}

pub fn tray_text(name: Option<&str>) -> String {
    name.map_or_else(|| "Search".to_owned(), |name| format!("Search ({name})"))
}

fn still(old: Option<Shortcut>) -> String {
    old.map_or_else(String::new, |old| {
        format!(" The shortcut is still {}.", parts(&old).name)
    })
}

fn default_shortcut() -> Shortcut {
    Shortcut::new(Some(DEFAULT.0), DEFAULT.1)
}

fn take_default(app: &AppHandle, keys: Keys) -> Option<Shortcut> {
    let shortcut = default_shortcut();
    match keys.take(app, shortcut) {
        Ok(()) => Some(shortcut),
        Err(taken) => {
            eprintln!("Mesa: {}", taken.sentence(&shortcut));
            None
        }
    }
}

enum Taken {
    By(String),
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Cannot(String),
}

impl Taken {
    fn sentence(&self, shortcut: &Shortcut) -> String {
        let name = parts(shortcut).name;
        match self {
            Self::By(by) => format!("{name} is used by {by}, so it can't be the search shortcut."),
            Self::Cannot(why) => format!("{name} can't be the search shortcut: {why}."),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Keys {
    Plugin,
    #[cfg(target_os = "linux")]
    Gnome,
}

impl Keys {
    fn here() -> Self {
        #[cfg(target_os = "linux")]
        if crate::gnome_keys::available() {
            return Self::Gnome;
        }
        Self::Plugin
    }

    fn take(self, app: &AppHandle, shortcut: Shortcut) -> Result<(), Taken> {
        match self {
            Self::Plugin => app.global_shortcut().register(shortcut).map_err(|e| {
                eprintln!("Mesa: {} can't be the search shortcut: {e}", parts(&shortcut).name);
                Taken::By("another program".into())
            }),
            #[cfg(target_os = "linux")]
            Self::Gnome => {
                use crate::gnome_keys::Refusal;
                let accelerator = gnome_accelerator(&shortcut)
                    .ok_or_else(|| Taken::Cannot("GNOME has no name for that key".into()))?;
                crate::gnome_keys::take(&accelerator, &search_command()).map_err(|refusal| match refusal {
                    Refusal::TakenBy(by) => Taken::By(by),
                    Refusal::Failed(why) => {
                        eprintln!("Mesa: GNOME's settings couldn't be changed: {why}");
                        Taken::Cannot("GNOME's settings couldn't be changed".into())
                    }
                })
            }
        }
    }

    fn release(self, app: &AppHandle, shortcut: Shortcut) {
        match self {
            Self::Plugin => {
                let _ = app.global_shortcut().unregister(shortcut);
            }
            #[cfg(target_os = "linux")]
            Self::Gnome => {
                let _ = shortcut;
                crate::gnome_keys::release();
            }
        }
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn gnome_accelerator(shortcut: &Shortcut) -> Option<String> {
    let mut written: String = modifiers_named(shortcut, ["<Control>", "<Alt>", "<Shift>", "<Super>"]).collect();
    written.push_str(&keysym(shortcut.key)?);
    Some(written)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn keysym(code: Code) -> Option<String> {
    let text = code.to_string();
    if let Some(letter) = text.strip_prefix("Key") {
        return Some(letter.to_ascii_lowercase());
    }
    if let Some(digit) = text.strip_prefix("Digit") {
        return Some(digit.to_owned());
    }
    if text.starts_with('F') && text[1..].parse::<u8>().is_ok() {
        return Some(text);
    }
    Some(
        match code {
            Code::Space => "space",
            Code::Enter => "Return",
            Code::Tab => "Tab",
            Code::Slash => "slash",
            Code::Period => "period",
            Code::Comma => "comma",
            Code::Minus => "minus",
            Code::Equal => "equal",
            Code::Semicolon => "semicolon",
            Code::Quote => "apostrophe",
            Code::Backquote => "grave",
            Code::Backslash => "backslash",
            Code::BracketLeft => "bracketleft",
            Code::BracketRight => "bracketright",
            Code::ArrowUp => "Up",
            Code::ArrowDown => "Down",
            Code::ArrowLeft => "Left",
            Code::ArrowRight => "Right",
            Code::Home => "Home",
            Code::End => "End",
            Code::PageUp => "Page_Up",
            Code::PageDown => "Page_Down",
            Code::Insert => "Insert",
            Code::Delete => "Delete",
            _ => return None,
        }
        .to_owned(),
    )
}

pub fn parse(text: &str) -> Result<Shortcut, Error> {
    let shortcut =
        Shortcut::from_str(text).map_err(|_| Error::Invalid("That key can't be used for a shortcut.".into()))?;
    validate(&shortcut).map_err(Error::Invalid)?;
    Ok(shortcut)
}

pub fn canonical(shortcut: &Shortcut) -> String {
    shortcut.into_string()
}

fn validate(shortcut: &Shortcut) -> Result<(), String> {
    let name = parts(shortcut).name;
    let mods = shortcut.mods & (Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT | Modifiers::SUPER);
    let command = if cfg!(target_os = "macos") {
        Modifiers::SUPER
    } else {
        Modifiers::CONTROL
    };
    if !mods.intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER) {
        let key = key_name(shortcut.key);
        return Err(format!(
            "{key} on its own would stop you typing it anywhere. Hold {} with it.",
            if cfg!(target_os = "macos") {
                "Ctrl, Option or Cmd".to_owned()
            } else {
                format!("Ctrl, Alt or {SUPER_WORD}")
            }
        ));
    }
    let taken_by = |what: &str| Err(format!("{name} {what} Try another combination."));
    if cfg!(target_os = "macos") {
        if mods == Modifiers::SUPER && shortcut.key == Code::Space {
            return taken_by("is Spotlight's.");
        }
        if mods == Modifiers::SUPER && shortcut.key == Code::Tab {
            return taken_by("switches between apps.");
        }
        if mods == Modifiers::SUPER && matches!(shortcut.key, Code::KeyQ | Code::KeyW | Code::KeyH) {
            return taken_by("quits, closes or hides apps.");
        }
    } else {
        if mods == Modifiers::ALT && shortcut.key == Code::F4 {
            return taken_by("closes windows.");
        }
        if mods == Modifiers::CONTROL.union(Modifiers::ALT) && shortcut.key == Code::Delete {
            return taken_by("belongs to Windows.");
        }
    }
    if mods == command
        && matches!(
            shortcut.key,
            Code::KeyC | Code::KeyV | Code::KeyX | Code::KeyZ | Code::KeyA
        )
    {
        return taken_by("is copy, paste and undo in every program.");
    }
    Ok(())
}

const SUPER_WORD: &str = if cfg!(target_os = "macos") {
    "Cmd"
} else if cfg!(windows) {
    "Win"
} else {
    "Super"
};

pub fn parts(shortcut: &Shortcut) -> Parts {
    let mac = cfg!(target_os = "macos");
    let mut keys = Vec::new();
    let mut name = Vec::new();
    for (modifier, symbol, word) in [
        (Modifiers::CONTROL, "⌃", "Ctrl"),
        (Modifiers::ALT, "⌥", if mac { "Option" } else { "Alt" }),
        (Modifiers::SHIFT, "⇧", "Shift"),
        (Modifiers::SUPER, "⌘", SUPER_WORD),
    ] {
        if shortcut.mods.contains(modifier) {
            keys.push(if mac { symbol } else { word }.to_owned());
            name.push(word.to_owned());
        }
    }
    let key = key_name(shortcut.key);
    keys.push(key.clone());
    name.push(key);
    Parts {
        keys,
        name: name.join("+"),
    }
}

fn key_name(code: Code) -> String {
    let named = match code {
        Code::ArrowUp => "↑",
        Code::ArrowDown => "↓",
        Code::ArrowLeft => "←",
        Code::ArrowRight => "→",
        Code::PageUp => "PgUp",
        Code::PageDown => "PgDn",
        Code::Backquote => "`",
        Code::Minus => "-",
        Code::Equal => "=",
        Code::BracketLeft => "[",
        Code::BracketRight => "]",
        Code::Backslash => "\\",
        Code::Semicolon => ";",
        Code::Quote => "'",
        Code::Comma => ",",
        Code::Period => ".",
        Code::Slash => "/",
        _ => "",
    };
    if !named.is_empty() {
        return named.to_owned();
    }
    let text = code.to_string();
    text.strip_prefix("Key")
        .or_else(|| text.strip_prefix("Digit"))
        .unwrap_or(&text)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shortcut(text: &str) -> Shortcut {
        text.parse().unwrap()
    }

    #[test]
    fn rejects_key_without_modifier() {
        for text in ["KeyK", "shift+KeyK", "Space", "shift+Digit1"] {
            let Err(Error::Invalid(message)) = parse(text) else {
                panic!("{text} was taken")
            };
            assert!(message.contains("on its own"), "{message}");
        }
    }

    #[test]
    fn key_round_trips() {
        for text in [
            "alt+Space",
            "control+alt+Space",
            "shift+control+KeyK",
            "super+alt+ArrowUp",
            "control+F12",
            "alt+Digit7",
        ] {
            let shortcut = shortcut(text);
            assert_eq!(parse(&canonical(&shortcut)).unwrap(), shortcut, "{text}");
        }
        assert_eq!(
            canonical(&shortcut("Ctrl+Alt+KeyK")),
            canonical(&shortcut("alt+control+KeyK"))
        );
    }
}
