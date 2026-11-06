use std::{str::FromStr, sync::Mutex, thread, time::Duration};

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
    pub chosen: Option<Parts>,
    pub active: Option<Parts>,
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
}

pub struct Hotkey {
    inner: Mutex<Inner>,
}

impl Hotkey {
    pub fn new(app: &AppHandle, chosen: Option<&str>) -> Self {
        let keys = app.global_shortcut();
        let active = chosen
            .and_then(|text| match parse(text) {
                Ok(shortcut) => match keys.register(shortcut) {
                    Ok(()) => Some(shortcut),
                    Err(e) => {
                        eprintln!(
                            "Mesa: {} is taken, so it can't be the search shortcut: {e}",
                            parts(&shortcut).name
                        );
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
            .or_else(|| take_default(app));
        Self {
            inner: Mutex::new(Inner {
                active,
                paused: None,
                pauses: 0,
            }),
        }
    }

    pub fn status(&self, chosen: Option<&str>) -> Status {
        Status {
            chosen: chosen
                .and_then(|text| parse(text).ok())
                .map(|shortcut| parts(&shortcut)),
            active: lock(&self.inner).active.as_ref().map(parts),
        }
    }

    pub fn name(&self) -> Option<String> {
        lock(&self.inner).active.as_ref().map(|shortcut| parts(shortcut).name)
    }

    pub fn change(&self, app: &AppHandle, new: Option<&str>) -> Result<Option<Shortcut>, Error> {
        let wanted = new.map(parse).transpose()?;
        let keys = app.global_shortcut();
        let mut inner = lock(&self.inner);
        let old = inner.active;
        inner.paused = None;
        if let Some(old) = old {
            let _ = keys.unregister(old);
        }
        let taken = match wanted {
            Some(shortcut) => keys.register(shortcut).map(|()| Some(shortcut)).map_err(|e| {
                eprintln!("Mesa: {} can't be the search shortcut: {e}", parts(&shortcut).name);
                let mut message = format!(
                    "{} is used by another program, so it can't be the search shortcut.",
                    parts(&shortcut).name
                );
                message.push_str(&still(old));
                message
            }),
            None => take_default(app).map(Some).ok_or_else(|| {
                format!(
                    "{} is used by another program, so it can't be the search shortcut.{}",
                    parts(&default_shortcut()).name,
                    still(old)
                )
            }),
        };
        match taken {
            Ok(active) => {
                inner.active = active;
                drop(inner);
                self.show_in_tray(app);
                Ok(old)
            }
            Err(message) => {
                if let Some(old) = old {
                    let _ = keys.register(old);
                }
                Err(Error::Failed(message))
            }
        }
    }

    pub fn restore(&self, app: &AppHandle, old: Option<Shortcut>) {
        let keys = app.global_shortcut();
        let mut inner = lock(&self.inner);
        if let Some(current) = inner.active {
            let _ = keys.unregister(current);
        }
        inner.active = old
            .filter(|shortcut| keys.register(*shortcut).is_ok())
            .or_else(|| take_default(app));
        drop(inner);
        self.show_in_tray(app);
    }

    pub fn pause(&self, app: &AppHandle) {
        let mut inner = lock(&self.inner);
        if inner.paused.is_some() {
            return;
        }
        if let Some(active) = inner.active {
            let _ = app.global_shortcut().unregister(active);
        }
        inner.pauses += 1;
        let pause = inner.pauses;
        inner.paused = Some(pause);
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
        let Some(active) = inner.active else { return };
        if app.global_shortcut().register(active).is_err() {
            inner.active = take_default(app);
            drop(inner);
            self.show_in_tray(app);
        }
    }

    fn show_in_tray(&self, app: &AppHandle) {
        if let Some(item) = app.try_state::<TrayItem>() {
            let _ = item.0.set_text(tray_text(self.name().as_deref()));
        }
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

fn take_default(app: &AppHandle) -> Option<Shortcut> {
    let shortcut = default_shortcut();
    match app.global_shortcut().register(shortcut) {
        Ok(()) => Some(shortcut),
        Err(e) => {
            eprintln!(
                "Mesa: {} is taken, so it can't be the search shortcut: {e}",
                parts(&shortcut).name
            );
            None
        }
    }
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
                "Ctrl, Option or Cmd"
            } else {
                "Ctrl, Alt or Win"
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

pub fn parts(shortcut: &Shortcut) -> Parts {
    let mac = cfg!(target_os = "macos");
    let mut keys = Vec::new();
    let mut name = Vec::new();
    for (modifier, symbol, word) in [
        (Modifiers::CONTROL, "⌃", "Ctrl"),
        (Modifiers::ALT, "⌥", if mac { "Option" } else { "Alt" }),
        (Modifiers::SHIFT, "⇧", "Shift"),
        (Modifiers::SUPER, "⌘", if mac { "Cmd" } else { "Win" }),
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
