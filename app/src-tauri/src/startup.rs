use tauri::AppHandle;
#[cfg(not(target_os = "linux"))]
use tauri_plugin_autostart::ManagerExt;

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

pub fn register(app: &AppHandle) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Ok(());
    }
    if cfg!(target_os = "macos") && std::env::current_exe().is_ok_and(|exe| not_installed(&exe)) {
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        linux::register(&app.package_info().name).map_err(|e| format!("Couldn't make Mesa start when you sign in. {e}"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        app.autolaunch()
            .enable()
            .map_err(|e| format!("Couldn't make Mesa start when you sign in. {e}"))
    }
}

pub fn unregister(app: &AppHandle) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        linux::unregister(&app.package_info().name)
            .map_err(|e| format!("Couldn't stop Mesa starting when you sign in. {e}"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        app.autolaunch()
            .disable()
            .map_err(|e| format!("Couldn't stop Mesa starting when you sign in. {e}"))
    }
}

pub fn follow_setting(app: &AppHandle, on: bool) -> Result<(), String> {
    if on { register(app) } else { unregister(app) }
}

#[cfg(any(target_os = "linux", test))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod linux {
    use std::{fs, io};

    use super::BACKGROUND_ARG;

    pub fn register(name: &str) -> io::Result<()> {
        let program = crate::xdg::program().ok_or_else(|| io::Error::other("the program's path isn't known"))?;
        let program = program
            .to_str()
            .ok_or_else(|| io::Error::other("the program's path isn't text"))?;
        let entry = entry(name, &[program, BACKGROUND_ARG])
            .ok_or_else(|| io::Error::other("the program's path has a line break in it"))?;
        let folder = crate::xdg::base_dir("XDG_CONFIG_HOME", ".config")
            .ok_or_else(|| io::Error::other("there is no home folder"))?
            .join("autostart");
        let file = folder.join(format!("{name}.desktop"));
        let there = fs::read_to_string(&file).ok();
        let entry = match &there {
            Some(there) => keeping_it_off(entry, there),
            None => entry,
        };
        if there.is_some_and(|there| there == entry) {
            return Ok(());
        }
        fs::create_dir_all(&folder)?;
        let partial = folder.join(format!(".{name}.desktop.partial"));
        fs::write(&partial, entry)?;
        fs::rename(&partial, &file)
    }

    pub fn unregister(name: &str) -> io::Result<()> {
        let folder = crate::xdg::base_dir("XDG_CONFIG_HOME", ".config")
            .ok_or_else(|| io::Error::other("there is no home folder"))?
            .join("autostart");
        match fs::remove_file(folder.join(format!("{name}.desktop"))) {
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

    pub fn keeping_it_off(entry: String, there: &str) -> String {
        let says = |line: &str| there.lines().any(|written| written.trim() == line);
        let hidden = says("Hidden=true");
        if !hidden && !says("X-GNOME-Autostart-enabled=false") {
            return entry;
        }
        let mut entry = entry.replace("X-GNOME-Autostart-enabled=true", "X-GNOME-Autostart-enabled=false");
        if hidden {
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

fn not_installed(exe: &std::path::Path) -> bool {
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
