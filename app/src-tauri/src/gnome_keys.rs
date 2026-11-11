//! GNOME before 48 has no shortcuts portal, so the search key is kept as a GNOME custom shortcut ("Mesa search")
//! that runs `mesa-app --search`.

use std::sync::atomic::{AtomicBool, Ordering};

use gtk::{gio, gio::prelude::*};

const MEDIA_KEYS: &str = "org.gnome.settings-daemon.plugins.media-keys";
const CUSTOM_KEY: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
const OURS: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/mesa-search/";
const NAME: &str = "Mesa search";

const GNOME_SCHEMAS: &[&str] = &[
    "org.gnome.desktop.wm.keybindings",
    "org.gnome.shell.keybindings",
    "org.gnome.mutter.keybindings",
    "org.gnome.mutter.wayland.keybindings",
    MEDIA_KEYS,
];

const WINDOW_MENU: (&str, &str) = ("org.gnome.desktop.wm.keybindings", "activate-window-menu");
const ALT_SPACE: &str = "<Alt>space";
static HAD_ALT_SPACE: AtomicBool = AtomicBool::new(false);

pub enum Refusal {
    TakenBy(String),
    Failed(String),
}

pub fn available() -> bool {
    let gnome = crate::xdg::current_desktops()
        .iter()
        .any(|name| name.eq_ignore_ascii_case("GNOME"));
    gnome && schema(MEDIA_KEYS).is_some() && schema(CUSTOM_KEY).is_some()
}

fn schema(id: &str) -> Option<gio::SettingsSchema> {
    gio::SettingsSchemaSource::default()?.lookup(id, true)
}

pub fn take(accelerator: &str, command: &str) -> Result<(), Refusal> {
    let failed = |e: gtk::glib::BoolError| Refusal::Failed(e.to_string());
    let ours = gio::Settings::with_path(CUSTOM_KEY, OURS);
    let had_alt_space = HAD_ALT_SPACE.load(Ordering::SeqCst) || same_key(&ours.string("binding"), ALT_SPACE);
    match taken_by(accelerator) {
        Some((schema_id, key, _))
            if (schema_id.as_str(), key.as_str()) == WINDOW_MENU && same_key(accelerator, ALT_SPACE) =>
        {
            let settings = gio::Settings::new(WINDOW_MENU.0);
            let left = strings(&settings, WINDOW_MENU.1)
                .into_iter()
                .filter(|key| !same_key(key, ALT_SPACE))
                .collect::<Vec<_>>();
            set_strings(&settings, WINDOW_MENU.1, &left).map_err(failed)?;
        }
        Some((_, _, what)) => return Err(Refusal::TakenBy(what)),
        None => {}
    }
    ours.set_string("name", NAME).map_err(failed)?;
    ours.set_string("command", command).map_err(failed)?;
    ours.set_string("binding", accelerator).map_err(failed)?;
    let list = gio::Settings::new(MEDIA_KEYS);
    let mut paths = strings(&list, "custom-keybindings");
    if !paths.iter().any(|path| path == OURS) {
        paths.push(OURS.to_owned());
        set_strings(&list, "custom-keybindings", &paths).map_err(failed)?;
    }
    let has_alt_space = same_key(accelerator, ALT_SPACE);
    HAD_ALT_SPACE.store(has_alt_space, Ordering::SeqCst);
    if had_alt_space && !has_alt_space {
        give_back_alt_space();
    }
    gio::Settings::sync();
    Ok(())
}

fn strings(settings: &gio::Settings, key: &str) -> Vec<String> {
    settings.strv(key).iter().map(|s| s.to_string()).collect()
}

fn set_strings(settings: &gio::Settings, key: &str, values: &[String]) -> Result<(), gtk::glib::BoolError> {
    settings.set_strv(key, values.iter().map(String::as_str).collect::<Vec<_>>().as_slice())
}

pub fn release() {
    let ours = gio::Settings::with_path(CUSTOM_KEY, OURS);
    let _ = ours.set_string("binding", "");
    gio::Settings::sync();
}

fn give_back_alt_space() {
    let settings = gio::Settings::new(WINDOW_MENU.0);
    if settings.strv(WINDOW_MENU.1).is_empty() && taken_by(ALT_SPACE).is_none() {
        let _ = settings.set_strv(WINDOW_MENU.1, [ALT_SPACE].as_slice());
    }
}

fn taken_by(accelerator: &str) -> Option<(String, String, String)> {
    for id in GNOME_SCHEMAS {
        let Some(schema) = schema(id) else { continue };
        let settings = gio::Settings::new(id);
        for key in schema.list_keys() {
            let value = settings.value(&key);
            let keys: Vec<String> = match value.type_().as_str() {
                "as" => value.get::<Vec<String>>().unwrap_or_default(),
                "s" => value.get::<String>().into_iter().collect(),
                _ => continue,
            };
            if keys.iter().any(|written| same_key(written, accelerator)) {
                let what = schema
                    .key(&key)
                    .summary()
                    .map_or_else(|| key.to_string(), |summary| summary.to_string());
                return Some((
                    id.to_string(),
                    key.to_string(),
                    format!("GNOME (for \u{201c}{what}\u{201d})"),
                ));
            }
        }
    }
    let list = gio::Settings::new(MEDIA_KEYS);
    for path in strings(&list, "custom-keybindings")
        .into_iter()
        .filter(|path| path != OURS)
    {
        let custom = gio::Settings::with_path(CUSTOM_KEY, &path);
        if same_key(&custom.string("binding"), accelerator) {
            let name = custom.string("name");
            return Some((
                CUSTOM_KEY.into(),
                path,
                format!("your custom shortcut \u{201c}{name}\u{201d}"),
            ));
        }
    }
    None
}

pub fn same_key(a: &str, b: &str) -> bool {
    match (normalized(a), normalized(b)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

fn normalized(accelerator: &str) -> Option<(Vec<&'static str>, String)> {
    let mut mods = Vec::new();
    let mut rest = accelerator.trim();
    while let Some(after) = rest.strip_prefix('<') {
        let (name, tail) = after.split_once('>')?;
        mods.push(match name.to_ascii_lowercase().as_str() {
            "primary" | "control" | "ctrl" | "ctl" => "ctrl",
            "alt" | "mod1" => "alt",
            "shift" | "shft" => "shift",
            "super" | "mod4" | "meta" | "hyper" => "super",
            _ => "other",
        });
        rest = tail;
    }
    if rest.is_empty() {
        return None;
    }
    mods.sort_unstable();
    mods.dedup();
    Some((mods, rest.to_lowercase()))
}
