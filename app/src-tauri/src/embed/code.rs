use std::{
    fs,
    path::{Path, PathBuf},
};

use super::{
    outline::Found,
    prepare::{Prepared, Unit},
};

pub const MAX_BYTES: u64 = 1024 * 1024;
const MAX_CHUNK: usize = 3000;
const SMALL: usize = 120;
const PASSAGE: usize = 1500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lang {
    pub name: &'static str,
}

const fn lang(name: &'static str) -> Lang {
    Lang { name }
}

const NAMED: &[(&str, Lang)] = &[
    ("Makefile", lang("Makefile")),
    ("makefile", lang("Makefile")),
    ("GNUmakefile", lang("Makefile")),
    ("Dockerfile", lang("Dockerfile")),
    ("Containerfile", lang("Dockerfile")),
    ("CMakeLists.txt", lang("CMake")),
    ("meson.build", lang("Meson")),
    ("justfile", lang("just")),
    ("Justfile", lang("just")),
    ("BUILD", lang("Starlark")),
    ("BUILD.bazel", lang("Starlark")),
    ("WORKSPACE", lang("Starlark")),
    ("Rakefile", lang("Ruby")),
    ("Gemfile", lang("Ruby")),
];

const LANGUAGES: &[(&[&str], Lang)] = &[
    (&["py", "pyw", "pyi"], lang("Python")),
    (&["js", "mjs", "cjs", "jsx"], lang("JavaScript")),
    (&["ts", "cts"], lang("TypeScript")),
    (&["tsx"], lang("TypeScript")),
    (&["rs"], lang("Rust")),
    (&["go"], lang("Go")),
    (&["java"], lang("Java")),
    (&["kt", "kts"], lang("Kotlin")),
    (&["c"], lang("C")),
    (
        &["h", "cc", "cpp", "cxx", "c++", "hpp", "hh", "hxx", "ipp", "ino"],
        lang("C++"),
    ),
    (&["cs", "csx"], lang("C#")),
    (&["swift"], lang("Swift")),
    (&["mm"], lang("Objective-C")),
    (&["m"], lang("Objective-C")),
    (&["rb", "rake", "gemspec"], lang("Ruby")),
    (&["php"], lang("PHP")),
    (&["sh", "bash", "zsh"], lang("Shell")),
    (&["fish"], lang("Fish")),
    (&["r"], lang("R")),
    (&["sql"], lang("SQL")),
    (&["ps1", "psm1"], lang("PowerShell")),
    (&["pl", "pm"], lang("Perl")),
    (&["lua"], lang("Lua")),
    (&["scala", "sc"], lang("Scala")),
    (&["dart"], lang("Dart")),
    (&["hs"], lang("Haskell")),
    (&["ex", "exs"], lang("Elixir")),
    (&["erl", "hrl"], lang("Erlang")),
    (&["jl"], lang("Julia")),
    (&["zig"], lang("Zig")),
    (&["ml", "mli"], lang("OCaml")),
    (&["fs", "fsi", "fsx"], lang("F#")),
    (&["groovy", "gradle"], lang("Groovy")),
    (&["clj", "cljs", "cljc"], lang("Clojure")),
    (&["lisp", "lsp"], lang("Common Lisp")),
    (&["scm", "ss"], lang("Scheme")),
    (&["el"], lang("Emacs Lisp")),
    (&["prolog"], lang("Prolog")),
    (&["elm"], lang("Elm")),
    (&["gleam"], lang("Gleam")),
    (&["res", "resi"], lang("ReScript")),
    (&["idr"], lang("Idris")),
    (&["adb", "ads"], lang("Ada")),
    (&["d"], lang("D")),
    (&["vb"], lang("Visual Basic")),
    (&["awk"], lang("AWK")),
    (&["vim"], lang("Vim script")),
    (&["sol"], lang("Solidity")),
    (&["glsl", "vert", "frag", "geom", "comp", "tesc", "tese"], lang("GLSL")),
    (&["hlsl"], lang("HLSL")),
    (&["asm", "s"], lang("Assembly")),
    (&["nasm"], lang("Assembly")),
    (&["nix"], lang("Nix")),
    (&["tf", "tfvars", "hcl"], lang("HCL")),
    (&["bzl", "star"], lang("Starlark")),
    (&["proto"], lang("Protocol Buffers")),
    (&["thrift"], lang("Thrift")),
    (&["capnp"], lang("Cap'n Proto")),
    (&["graphql", "gql"], lang("GraphQL")),
    (&["rego"], lang("Rego")),
    (&["bat", "cmd"], lang("Batch")),
    (&["vue"], lang("Vue")),
    (&["svelte"], lang("Svelte")),
    (&["cmake"], lang("CMake")),
    (&["mk"], lang("Makefile")),
    (&["ninja"], lang("Ninja")),
    (&["just"], lang("just")),
    (&["dockerfile"], lang("Dockerfile")),
];

const MATLAB: Lang = lang("MATLAB");
const HTML: Lang = lang("HTML");

pub fn is_html_name(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["html", "htm", "xhtml"].contains(&e.to_ascii_lowercase().as_str()))
}

pub fn is_code_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    is_code_name(name) || (is_html_name(name) && crate::code_scope::in_project(path))
}

fn by_name(name: &str) -> Option<Lang> {
    if let Some((_, lang)) = NAMED.iter().find(|(named, _)| *named == name) {
        return Some(*lang);
    }
    let extension = Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
    LANGUAGES
        .iter()
        .find(|(extensions, _)| extensions.contains(&extension.as_str()))
        .map(|(_, lang)| *lang)
}

pub fn language_named(name: &str) -> Option<&'static str> {
    LANGUAGES
        .iter()
        .map(|(_, lang)| lang.name)
        .chain(NAMED.iter().map(|(_, lang)| lang.name))
        .chain([MATLAB.name, HTML.name])
        .find(|known| known.eq_ignore_ascii_case(name))
}

pub fn names_of(name: &str) -> (Vec<&'static str>, Vec<&'static str>) {
    let mut extensions: Vec<&'static str> = LANGUAGES
        .iter()
        .filter(|(_, lang)| lang.name == name)
        .flat_map(|(extensions, _)| extensions.iter().copied())
        .collect();
    if name == MATLAB.name {
        extensions.push("m");
    }
    if name == HTML.name {
        extensions.extend(["html", "htm", "xhtml"]);
    }
    let whole = NAMED
        .iter()
        .filter(|(_, lang)| lang.name == name)
        .map(|(named, _)| *named)
        .collect();
    (extensions, whole)
}

pub fn is_code_name(name: &str) -> bool {
    by_name(name).is_some()
}

pub fn language(path: &Path, text: &str) -> Option<Lang> {
    let name = path.file_name()?.to_str()?;
    if is_html_name(name) {
        return Some(HTML);
    }
    let lang = by_name(name)?;
    if name.to_ascii_lowercase().ends_with(".m")
        && !["@interface", "@implementation", "#import", "@protocol"]
            .iter()
            .any(|w| text.contains(w))
    {
        return Some(MATLAB);
    }
    Some(lang)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub start: u32,
    pub end: u32,
    pub symbol: String,
    pub text: String,
}

fn refuse(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() as u64 > MAX_BYTES {
        return Some("it is too big to be code someone wrote");
    }
    if bytes.contains(&0) {
        return Some("it isn't text");
    }
    if is_minified(bytes) {
        return Some("it is minified");
    }
    None
}

fn is_minified(bytes: &[u8]) -> bool {
    let lines = bytes
        .split(|&b| b == b'\n')
        .filter(|line| !line.is_empty())
        .count()
        .max(1);
    let long = bytes
        .split(|&b| b == b'\n')
        .filter(|line| line.len() > 1000)
        .map(<[u8]>::len)
        .sum::<usize>();
    bytes.len() / lines > 300 || (bytes.len() > 4000 && long * 2 > bytes.len())
}

pub fn text_of(path: &Path) -> Result<String, String> {
    let size = fs::metadata(path)
        .map_err(|e| format!("can't read the file: {e}"))?
        .len();
    if size > MAX_BYTES {
        return Err("it is too big to be code someone wrote".into());
    }
    let bytes = fs::read(path).map_err(|e| format!("can't read the file: {e}"))?;
    if let Some(why) = refuse(&bytes) {
        return Err(why.into());
    }
    Ok(String::from_utf8_lossy(&bytes).replace("\r\n", "\n"))
}

pub fn project_root(path: &Path) -> Option<PathBuf> {
    const MARKERS: &[&str] = &[
        ".git",
        "package.json",
        "Cargo.toml",
        "pyproject.toml",
        "setup.py",
        "go.mod",
        "pom.xml",
        "build.gradle",
        "build.gradle.kts",
        "Gemfile",
        "composer.json",
        "Package.swift",
        "mix.exs",
        "deno.json",
        "CMakeLists.txt",
        "Makefile",
    ];
    let mut best = None;
    for folder in path.ancestors().skip(1) {
        if folder.parent().is_none() {
            break;
        }
        if folder.join(".git").exists() {
            return Some(folder.to_owned());
        }
        if best.is_none() && MARKERS.iter().any(|marker| folder.join(marker).exists()) {
            best = Some(folder.to_owned());
        }
    }
    best
}

pub fn title(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match project_root(path) {
        Some(root) => {
            let project = root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let inside = path
                .strip_prefix(&root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or(name);
            format!("{project}/{inside}")
        }
        None => match path.parent().and_then(Path::file_name) {
            Some(folder) => format!("{}/{name}", folder.to_string_lossy()),
            None => name,
        },
    }
}

pub fn prompt(title: &str, chunk: &Chunk) -> String {
    if chunk.symbol.is_empty() {
        format!("title: {title} | text: {}", chunk.text)
    } else {
        format!("title: {title} · {} | text: {}", chunk.symbol, chunk.text)
    }
}

pub fn read(path: &Path, emit: &mut dyn FnMut(Unit) -> bool, words: &mut String) -> Result<(), String> {
    let text = text_of(path)?;
    let lang = language(path, &text).ok_or("it isn't code Mesa reads")?;
    let chunks = chunks(&text, lang);
    if chunks.is_empty() {
        return Err("there is too little code in it".into());
    }
    words.push_str(&text);
    let title = title(path);
    for chunk in &chunks {
        if !emit(Unit {
            locator: u64::from(chunk.start),
            input: Prepared::Text(prompt(&title, chunk)),
        }) {
            break;
        }
    }
    Ok(())
}

pub fn chunks_of(path: &Path) -> Result<Vec<Chunk>, String> {
    let text = text_of(path)?;
    let lang = language(path, &text).ok_or("it isn't code Mesa reads")?;
    Ok(chunks(&text, lang))
}

pub fn chunks(text: &str, lang: Lang) -> Vec<Chunk> {
    if text.chars().filter(|c| c.is_alphanumeric()).nth(19).is_none() {
        return Vec::new();
    }
    assemble(text, super::outline::symbols(text, lang.name))
}

fn assemble(text: &str, mut symbols: Vec<Found>) -> Vec<Chunk> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut covered = vec![false; lines.len() + 2];
    let mut out = Vec::new();
    symbols.sort_by_key(|s| (s.start, std::cmp::Reverse(s.end)));
    let mut last_end = 0;
    for symbol in symbols {
        if symbol.start <= last_end || symbol.start as usize > lines.len() {
            continue;
        }
        let body = lines_of(&lines, symbol.start, symbol.end);
        if body.trim().len() < SMALL {
            continue;
        }
        last_end = symbol.end;
        for line in symbol.start..=symbol.end.min(lines.len() as u32) {
            covered[line as usize] = true;
        }
        if body.len() <= MAX_CHUNK {
            out.push(Chunk {
                start: symbol.start,
                end: symbol.end,
                symbol: symbol.name,
                text: body,
            });
        } else {
            for (start, end, piece) in passages(&lines, symbol.start, symbol.end) {
                out.push(Chunk {
                    start,
                    end,
                    symbol: symbol.name.clone(),
                    text: piece,
                });
            }
        }
    }
    let mut run_start = None;
    for line in 1..=lines.len() as u32 + 1 {
        let free = (line as usize) <= lines.len() && !covered[line as usize];
        match (free, run_start) {
            (true, None) => run_start = Some(line),
            (false, Some(start)) => {
                for (s, e, piece) in passages(&lines, start, line - 1) {
                    if piece.chars().filter(|c| c.is_alphanumeric()).count() >= 20 {
                        out.push(Chunk {
                            start: s,
                            end: e,
                            symbol: String::new(),
                            text: piece,
                        });
                    }
                }
                run_start = None;
            }
            _ => {}
        }
    }
    out.sort_by_key(|chunk| chunk.start);
    out
}

fn lines_of(lines: &[&str], start: u32, end: u32) -> String {
    lines[(start as usize - 1).min(lines.len())..(end as usize).min(lines.len())].join("\n")
}

fn passages(lines: &[&str], start: u32, end: u32) -> Vec<(u32, u32, String)> {
    let mut out = Vec::new();
    let mut piece: Vec<&str> = Vec::new();
    let mut piece_start = start;
    let mut size = 0;
    for n in start..=end.min(lines.len() as u32) {
        let line = lines[n as usize - 1];
        let line = if line.len() > PASSAGE {
            &line[..line.floor_char_boundary(PASSAGE)]
        } else {
            line
        };
        let at_break = line.trim().is_empty();
        if !piece.is_empty() && (size + line.len() + 1 > PASSAGE || (at_break && size > PASSAGE / 2)) {
            out.push((piece_start, n - 1, piece.join("\n")));
            piece.clear();
            size = 0;
        }
        if piece.is_empty() {
            if at_break {
                continue;
            }
            piece_start = n;
        }
        piece.push(line);
        size += line.len() + 1;
    }
    if !piece.is_empty() {
        let last = piece_start + piece.len() as u32 - 1;
        out.push((piece_start, last, piece.join("\n")));
    }
    out.into_iter()
        .map(|(s, e, t)| (s, e, t.trim_end().to_owned()))
        .filter(|(_, _, t)| !t.trim().is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbols(name: &str, text: &str) -> Vec<(u32, String)> {
        let lang = language(Path::new(name), text).unwrap();
        chunks(text, lang).into_iter().map(|c| (c.start, c.symbol)).collect()
    }

    const PY: &str = "import os\n\n# Tells a deleted folder from an unplugged drive.\ndef is_gone(path):\n    \"\"\"Whether it was deleted.\"\"\"\n    missing = not os.path.lexists(path)\n    return missing and any(os.path.lexists(p) for p in parents(path))\n\nclass Store:\n    def save(self, settings):\n        with open(self.path + '.new', 'w') as f:\n            f.write(settings)\n        os.replace(self.path + '.new', self.path)\n";

    #[test]
    fn python_chunks() {
        let found = symbols("a.py", PY);
        assert!(found.contains(&(3, "is_gone".into())), "{found:?}");
        assert!(found.contains(&(10, "Store::save".into())), "{found:?}");
    }

    #[test]
    fn rejects_minified_and_binary() {
        assert!(refuse(format!("var a={};", "x".repeat(5000)).as_bytes()).is_some());
        assert!(refuse(b"fn main() {}\0\0").is_some());
        assert!(refuse(b"fn main() {\n    println!(\"hello\");\n}\n").is_none());
    }
}
