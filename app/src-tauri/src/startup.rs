use tauri::AppHandle;
#[cfg(not(target_os = "linux"))]
use tauri_plugin_autostart::ManagerExt;

use crate::settings::{Settings, SettingsStore};

pub const BACKGROUND_ARG: &str = "--background";
pub const SEARCH_ARG: &str = "--search";

pub fn arguments() -> Vec<String> {
    std::env::args().collect()
}

pub fn starts_without_window() -> bool {
    let args = arguments();
    in_background(&args) || wants_search(&args)
}

pub fn wants_search(args: &[String]) -> bool {
    args.iter().any(|arg| arg == SEARCH_ARG)
}

pub fn in_background(args: &[String]) -> bool {
    args.iter().any(|arg| arg == BACKGROUND_ARG)
}

#[cfg(target_os = "macos")]
pub fn stay_out_of_relaunch_at_login() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    if let Some(mtm) = MainThreadMarker::new() {
        NSApplication::sharedApplication(mtm).disableRelaunchOnLogin();
    }
}

fn can_change() -> bool {
    if cfg!(debug_assertions) {
        return false;
    }
    !(cfg!(target_os = "macos") && std::env::current_exe().is_ok_and(|exe| not_installed(&exe)))
}

pub fn system_says(app: &AppHandle) -> Option<bool> {
    if !can_change() {
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        Some(macos::is_on())
    }
    #[cfg(target_os = "linux")]
    {
        Some(linux::is_on(&app.package_info().name))
    }
    #[cfg(windows)]
    {
        app.autolaunch().is_enabled().ok()
    }
}

#[derive(Debug, PartialEq, Eq)]
enum AtStart {
    Nothing,
    Follow(bool),
    Refresh,
    LeaveOff,
}

fn at_start_does(made: bool, kept: bool, system: Option<bool>) -> AtStart {
    match system {
        None => AtStart::Nothing,
        Some(_) if !made => AtStart::Follow(kept),
        Some(true) => AtStart::Refresh,
        Some(false) => AtStart::LeaveOff,
    }
}

pub fn at_start(app: &AppHandle, settings: &SettingsStore) {
    #[cfg(target_os = "macos")]
    if can_change() {
        macos::move_from_main_app();
    }
    let kept = settings.get();
    let does = at_start_does(kept.sign_in_entry_made, kept.start_at_sign_in, system_says(app));
    if does == AtStart::Nothing {
        return;
    }
    #[cfg(target_os = "macos")]
    let _ = app.autolaunch().disable();
    let done = match does {
        AtStart::Follow(true) => register(app, false),
        AtStart::Follow(false) => unregister(app),
        AtStart::Refresh if !cfg!(target_os = "macos") => register(app, false),
        _ => Ok(()),
    };
    if let Err(e) = done {
        eprintln!("Mesa: {e}");
        return;
    }
    let on = system_says(app).unwrap_or(kept.start_at_sign_in);
    if let Err(e) = settings.set(Settings {
        start_at_sign_in: on,
        sign_in_entry_made: true,
        ..settings.get()
    }) {
        eprintln!("Mesa: couldn't save the settings. {e}");
    }
}

pub fn current(app: &AppHandle, settings: &SettingsStore) -> Settings {
    let kept = settings.get();
    match system_says(app) {
        Some(on) if kept.sign_in_entry_made && on != kept.start_at_sign_in => {
            let now = Settings {
                start_at_sign_in: on,
                ..kept
            };
            let _ = settings.set(now.clone());
            now
        }
        _ => kept,
    }
}

pub fn set(app: &AppHandle, settings: &SettingsStore, on: bool) -> Result<(), String> {
    let changes = can_change();
    if changes {
        if on { register(app, true)? } else { unregister(app)? }
    }
    let kept = settings.get();
    let made = kept.sign_in_entry_made || changes;
    if let Err(e) = settings.set(Settings {
        start_at_sign_in: on,
        sign_in_entry_made: made,
        ..kept
    }) {
        if changes {
            let _ = if on { unregister(app) } else { register(app, true) };
        }
        return Err(format!("Couldn't save the setting. {e}"));
    }
    Ok(())
}

fn register(app: &AppHandle, turn_on: bool) -> Result<(), String> {
    let failed = |e: String| format!("Couldn't make Mesa start when you sign in. {e}");
    #[cfg(target_os = "macos")]
    {
        let _ = (app, failed);
        macos::register(turn_on)
    }
    #[cfg(target_os = "linux")]
    {
        linux::register(&app.package_info().name, turn_on).map_err(|e| failed(e.to_string()))
    }
    #[cfg(windows)]
    {
        let _ = turn_on;
        app.autolaunch().enable().map_err(|e| failed(e.to_string()))
    }
}

fn unregister(app: &AppHandle) -> Result<(), String> {
    let failed = |e: String| format!("Couldn't stop Mesa starting when you sign in. {e}");
    #[cfg(target_os = "macos")]
    {
        let _ = (app, failed);
        macos::unregister()
    }
    #[cfg(target_os = "linux")]
    {
        linux::unregister(&app.package_info().name).map_err(|e| failed(e.to_string()))
    }
    #[cfg(windows)]
    {
        app.autolaunch().disable().map_err(|e| failed(e.to_string()))
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use smappservice_rs::{AppService, ServiceStatus, ServiceType};

    const AGENT_PLIST: &str = "com.mesa.desktop.agent.plist";

    fn service() -> AppService {
        AppService::new(ServiceType::Agent { plist_name: AGENT_PLIST })
    }

    pub fn move_from_main_app() {
        let old = AppService::new(ServiceType::MainApp);
        if old.status() == ServiceStatus::Enabled && old.unregister().is_ok() && !is_on() {
            let _ = register(false);
        }
    }

    pub fn is_on() -> bool {
        service().status() == ServiceStatus::Enabled
    }

    pub fn register(open_settings: bool) -> Result<(), String> {
        let service = service();
        let result = service.register();
        match service.status() {
            ServiceStatus::Enabled => Ok(()),
            ServiceStatus::RequiresApproval => {
                if open_settings {
                    AppService::open_system_settings_login_items();
                }
                Err("Turn Mesa on in System Settings, under Login Items > Allow in the Background, to have it start when you sign in.".into())
            }
            _ => Err(format!(
                "Couldn't make Mesa start when you sign in. {}",
                result.err().map_or_else(
                    || "macOS didn't add it to the Login Items.".to_owned(),
                    |e| e.to_string()
                )
            )),
        }
    }

    pub fn unregister() -> Result<(), String> {
        let service = service();
        let result = service.unregister();
        match service.status() {
            ServiceStatus::Enabled => Err(format!(
                "Couldn't stop Mesa starting when you sign in. {}",
                result
                    .err()
                    .map_or_else(|| "macOS kept it in the Login Items.".to_owned(), |e| e.to_string())
            )),
            _ => Ok(()),
        }
    }
}

#[cfg(any(target_os = "linux", test))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod linux {
    use std::{fs, io};

    use super::BACKGROUND_ARG;

    fn file(name: &str) -> io::Result<std::path::PathBuf> {
        let folder = crate::xdg::base_dir("XDG_CONFIG_HOME", ".config")
            .ok_or_else(|| io::Error::other("there is no home folder"))?
            .join("autostart");
        Ok(folder.join(format!("{name}.desktop")))
    }

    pub fn register(name: &str, turn_on: bool) -> io::Result<()> {
        let program = crate::xdg::program().ok_or_else(|| io::Error::other("the program's path isn't known"))?;
        let program = program
            .to_str()
            .ok_or_else(|| io::Error::other("the program's path isn't text"))?;
        let entry = entry(name, &[program, BACKGROUND_ARG])
            .ok_or_else(|| io::Error::other("the program's path has a line break in it"))?;
        let file = file(name)?;
        let there = fs::read_to_string(&file).ok();
        let entry = match &there {
            Some(there) if !turn_on => keeping_it_off(entry, there),
            _ => entry,
        };
        if there.is_some_and(|there| there == entry) {
            return Ok(());
        }
        let folder = file.parent().expect("the entry is in a folder");
        fs::create_dir_all(folder)?;
        let partial = folder.join(format!(".{name}.desktop.partial"));
        fs::write(&partial, entry)?;
        fs::rename(&partial, &file)
    }

    pub fn is_on(name: &str) -> bool {
        file(name)
            .and_then(fs::read_to_string)
            .is_ok_and(|there| !turned_off(&there))
    }

    pub fn unregister(name: &str) -> io::Result<()> {
        match fs::remove_file(file(name)?) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    pub fn entry(name: &str, command: &[&str]) -> Option<String> {
        if name.contains(['\n', '\r']) || command.iter().any(|part| part.contains(['\n', '\r'])) {
            return None;
        }
        let exec = command
            .iter()
            .map(|part| exec_argument(part))
            .collect::<Vec<_>>()
            .join(" ");
        Some(format!(
            "[Desktop Entry]\nType=Application\nName={name}\nComment=Starts {name} at sign-in, to keep indexing and watching for changes\n\
             Exec={}\nIcon=mesa-app\nTerminal=false\nStartupNotify=false\nX-GNOME-Autostart-enabled=true\n",
            string_value(&exec),
        ))
    }

    pub fn turned_off(there: &str) -> bool {
        let says = |line: &str| there.lines().any(|written| written.trim() == line);
        says("Hidden=true") || says("X-GNOME-Autostart-enabled=false")
    }

    pub fn keeping_it_off(entry: String, there: &str) -> String {
        if !turned_off(there) {
            return entry;
        }
        let mut entry = entry.replace("X-GNOME-Autostart-enabled=true", "X-GNOME-Autostart-enabled=false");
        if there.lines().any(|written| written.trim() == "Hidden=true") {
            entry.push_str("Hidden=true\n");
        }
        entry
    }

    fn exec_argument(argument: &str) -> String {
        const RESERVED: &[char] = &[
            ' ', '\t', '"', '\'', '\\', '>', '<', '~', '|', '&', ';', '$', '*', '?', '#', '(', ')', '`',
        ];
        let argument = argument.replace('%', "%%");
        if !argument.contains(RESERVED) {
            return argument;
        }
        let mut quoted = String::from('"');
        for c in argument.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                quoted.push('\\');
            }
            quoted.push(c);
        }
        quoted.push('"');
        quoted
    }

    fn string_value(value: &str) -> String {
        value.replace('\\', "\\\\")
    }
}

pub fn not_installed(exe: &std::path::Path) -> bool {
    exe.starts_with("/Volumes") || exe.components().any(|part| part.as_os_str() == "AppTranslocation")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_entry_quotes_path() {
        let plain = linux::entry("Mesa", &["/usr/bin/mesa-app", BACKGROUND_ARG]).unwrap();
        assert!(
            plain.starts_with("[Desktop Entry]\nType=Application\nName=Mesa\n"),
            "{plain}"
        );
        assert!(plain.contains("\nExec=/usr/bin/mesa-app --background\n"), "{plain}");
        assert!(plain.contains("\nX-GNOME-Autostart-enabled=true\n"));

        let exec = |program: &str| {
            let entry = linux::entry("Mesa", &[program, BACKGROUND_ARG]).unwrap();
            entry
                .lines()
                .find_map(|line| line.strip_prefix("Exec="))
                .unwrap()
                .to_owned()
        };
        assert_eq!(
            exec("/home/me/My Apps/Mesa.AppImage"),
            "\"/home/me/My Apps/Mesa.AppImage\" --background"
        );
        assert_eq!(
            exec("/home/me/$HOME \"x\"/Mesa"),
            "\"/home/me/\\\\$HOME \\\\\"x\\\\\"/Mesa\" --background"
        );
        assert_eq!(
            exec("/home/me/100%/Mesa.AppImage"),
            "/home/me/100%%/Mesa.AppImage --background"
        );
        assert_eq!(exec("/home/me/a\\b"), "\"/home/me/a\\\\\\\\b\" --background");
        assert_eq!(linux::entry("Mesa", &["/home/me/two\nlines", BACKGROUND_ARG]), None);
    }
}
