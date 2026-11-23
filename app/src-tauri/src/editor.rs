use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

#[derive(Clone, Copy)]
enum Style {
    Goto,
    Suffix,
    LineFlag,
}

struct Editor {
    program: PathBuf,
    style: Style,
}

const JETBRAINS: &[(&str, &[&str])] = &[
    ("IntelliJ IDEA", &["java", "kt", "kts", "scala", "groovy", "gradle"]),
    ("PyCharm", &["py", "pyw", "pyi"]),
    (
        "WebStorm",
        &["js", "mjs", "cjs", "jsx", "ts", "cts", "tsx", "vue", "svelte"],
    ),
    ("GoLand", &["go"]),
    ("RustRover", &["rs"]),
    ("CLion", &["c", "h", "cc", "cpp", "cxx", "hpp", "hh", "hxx"]),
    ("PhpStorm", &["php"]),
    ("RubyMine", &["rb", "rake", "gemspec"]),
    ("Rider", &["cs", "csx", "fs", "fsi", "fsx", "vb"]),
];

pub fn open_at(path: &Path, line: u32) -> Result<bool, String> {
    let Some(editor) = find(path) else { return Ok(false) };
    let target = path.to_string_lossy();
    let mut command = Command::new(&editor.program);
    match editor.style {
        Style::Goto => command.arg("-g").arg(format!("{target}:{line}")),
        Style::Suffix => command.arg(format!("{target}:{line}")),
        Style::LineFlag => command.arg("--line").arg(line.to_string()).arg(target.as_ref()),
    };
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let child = command
        .spawn()
        .map_err(|e| format!("Couldn't start {}: {e}", editor.program.display()))?;
    let_run(child);
    Ok(true)
}

#[cfg(unix)]
fn let_run(mut child: Child) {
    let _ = std::thread::Builder::new().name("editor-reaper".into()).spawn(move || {
        let _ = child.wait();
    });
}

#[cfg(not(unix))]
fn let_run(_: Child) {}

fn find(path: &Path) -> Option<Editor> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let editors = [
        candidates_vscode()
            .into_iter()
            .map(|program| Editor {
                program,
                style: Style::Goto,
            })
            .collect::<Vec<_>>(),
        candidates_cursor()
            .into_iter()
            .map(|program| Editor {
                program,
                style: Style::Goto,
            })
            .collect(),
        jetbrains(&extension)
            .into_iter()
            .map(|program| Editor {
                program,
                style: Style::LineFlag,
            })
            .collect(),
        candidates_zed()
            .into_iter()
            .map(|program| Editor {
                program,
                style: Style::Suffix,
            })
            .collect(),
        candidates_sublime()
            .into_iter()
            .map(|program| Editor {
                program,
                style: Style::Suffix,
            })
            .collect(),
    ];
    editors.into_iter().flatten().find(|editor| editor.program.is_file())
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn on_path(name: &str) -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).map(|dir| dir.join(name)).collect())
        .unwrap_or_default()
}

#[cfg(not(windows))]
fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn apps(name: &str) -> Vec<PathBuf> {
    vec![
        home().join("Applications").join(format!("{name}.app")),
        PathBuf::from("/Applications").join(format!("{name}.app")),
    ]
}

#[cfg(windows)]
fn local_programs() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("Programs")
}

#[cfg(windows)]
fn program_files() -> Vec<PathBuf> {
    ["ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect()
}

fn candidates_vscode() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    return apps("Visual Studio Code")
        .into_iter()
        .map(|app| app.join("Contents/Resources/app/bin/code"))
        .collect();
    #[cfg(windows)]
    return std::iter::once(local_programs().join("Microsoft VS Code/bin/code.cmd"))
        .chain(
            program_files()
                .into_iter()
                .map(|dir| dir.join("Microsoft VS Code/bin/code.cmd")),
        )
        .collect();
    #[cfg(target_os = "linux")]
    return on_path("code");
}

fn candidates_cursor() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    return apps("Cursor")
        .into_iter()
        .map(|app| app.join("Contents/Resources/app/bin/cursor"))
        .collect();
    #[cfg(windows)]
    return vec![local_programs().join("cursor/resources/app/bin/cursor.cmd")];
    #[cfg(target_os = "linux")]
    return on_path("cursor");
}

fn candidates_zed() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    return apps("Zed")
        .into_iter()
        .map(|app| app.join("Contents/MacOS/cli"))
        .collect();
    #[cfg(windows)]
    return vec![
        local_programs().join("Zed/bin/zed.exe"),
        local_programs().join("Zed/zed.exe"),
    ];
    #[cfg(target_os = "linux")]
    return [on_path("zed"), on_path("zeditor"), vec![home().join(".local/bin/zed")]].concat();
}

fn candidates_sublime() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    return apps("Sublime Text")
        .into_iter()
        .map(|app| app.join("Contents/SharedSupport/bin/subl"))
        .collect();
    #[cfg(windows)]
    return program_files()
        .into_iter()
        .map(|dir| dir.join("Sublime Text/subl.exe"))
        .collect();
    #[cfg(target_os = "linux")]
    return on_path("subl");
}

fn jetbrains(extension: &str) -> Vec<PathBuf> {
    let (mine, others): (Vec<_>, Vec<_>) = JETBRAINS
        .iter()
        .partition(|(_, extensions)| extensions.contains(&extension));
    mine.into_iter()
        .chain(others)
        .flat_map(|(name, _)| jetbrains_programs(name))
        .collect()
}

fn jetbrains_programs(name: &str) -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let binary = match name {
            "IntelliJ IDEA" => "idea",
            "PyCharm" => "pycharm",
            "WebStorm" => "webstorm",
            "GoLand" => "goland",
            "RustRover" => "rustrover",
            "CLion" => "clion",
            "PhpStorm" => "phpstorm",
            "RubyMine" => "rubymine",
            _ => "rider",
        };
        let mut out = Vec::new();
        for dir in [home().join("Applications"), PathBuf::from("/Applications")] {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let file = entry.file_name().to_string_lossy().into_owned();
                if file.starts_with(name) && file.ends_with(".app") {
                    out.push(entry.path().join("Contents/MacOS").join(binary));
                }
            }
        }
        out
    }
    #[cfg(windows)]
    {
        let mut out = Vec::new();
        for dir in program_files()
            .into_iter()
            .map(|dir| dir.join("JetBrains"))
            .chain(std::iter::once(local_programs()))
        {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                if entry.file_name().to_string_lossy().starts_with(name) {
                    for exe in std::fs::read_dir(entry.path().join("bin"))
                        .into_iter()
                        .flatten()
                        .flatten()
                    {
                        let file = exe.file_name().to_string_lossy().into_owned();
                        if file.ends_with("64.exe") {
                            out.push(exe.path());
                        }
                    }
                }
            }
        }
        out
    }
    #[cfg(target_os = "linux")]
    {
        let binary = name
            .to_ascii_lowercase()
            .replace("intellij idea", "idea")
            .replace(' ', "");
        [
            on_path(&binary),
            vec![home().join(".local/share/JetBrains/Toolbox/scripts").join(&binary)],
        ]
        .concat()
    }
}

pub fn open_as_text(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("/usr/bin/open");
        command.arg("-t").arg(path);
        command
    };
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new("notepad.exe");
        command.arg(path).creation_flags(0x0800_0000);
        command
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let editor = Command::new("xdg-mime")
            .args(["query", "default", "text/plain"])
            .output()
            .ok()
            .and_then(|out| {
                String::from_utf8(out.stdout)
                    .ok()
                    .map(|desktop| desktop.trim().trim_end_matches(".desktop").to_owned())
                    .filter(|d| !d.is_empty())
            });
        match editor {
            Some(desktop) => {
                let mut command = Command::new("gtk-launch");
                command.arg(desktop).arg(path);
                command
            }
            None => {
                let mut command = Command::new("xdg-open");
                command.arg(path);
                command
            }
        }
    };
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    command
        .spawn()
        .map(let_run)
        .map_err(|e| format!("Couldn't open it as text: {e}"))
}
