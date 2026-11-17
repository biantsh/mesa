use std::ops::Range;

#[derive(Debug, Clone)]
pub(crate) struct Found {
    pub start: u32,
    pub end: u32,
    pub name: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Quote {
    String,
    Char,
    No,
}

#[derive(Clone, Copy)]
struct Syntax {
    line: &'static [&'static str],
    line_at_start: &'static [&'static str],
    block: &'static [(&'static str, &'static str)],
    multi: &'static [(&'static str, &'static str)],
    quote_lines: bool,
    quote: Quote,
    heredocs: bool,
    verbatim: bool,
    raw_strings: bool,
    preprocessor: bool,
    keywords: &'static [(&'static str, bool)],
    any_case: bool,
    c_signatures: bool,
    untyped_methods: bool,
    js_assign: bool,
    assign_function: bool,
    lisp: bool,
    equations: bool,
    targets: bool,
    sql: bool,
    merge_clauses: bool,
    words: Words,
}

const BASE: Syntax = Syntax {
    line: &[],
    line_at_start: &[],
    block: &[],
    multi: &[],
    quote_lines: false,
    quote: Quote::String,
    heredocs: false,
    verbatim: false,
    raw_strings: false,
    preprocessor: false,
    keywords: &[],
    any_case: false,
    c_signatures: false,
    untyped_methods: false,
    js_assign: false,
    assign_function: false,
    lisp: false,
    equations: false,
    targets: false,
    sql: false,
    merge_clauses: false,
    words: Words::Plain,
};

const C_COMMENTS: &[(&str, &str)] = &[("/*", "*/")];
const SLASHES: &[&str] = &["//"];
const HASH: &[&str] = &["#"];

fn syntax(name: &str) -> Syntax {
    let c_like = Syntax {
        line: SLASHES,
        block: C_COMMENTS,
        quote: Quote::Char,
        c_signatures: true,
        ..BASE
    };
    match name {
        "Python" | "Starlark" => Syntax {
            line: HASH,
            multi: &[("\"\"\"", "\"\"\""), ("'''", "'''")],
            keywords: &[("def", false), ("class", true)],
            ..BASE
        },
        "JavaScript" | "TypeScript" | "Vue" | "Svelte" => Syntax {
            line: SLASHES,
            block: C_COMMENTS,
            multi: &[("`", "`")],
            keywords: &[
                ("function", false),
                ("class", true),
                ("interface", true),
                ("type", false),
                ("enum", false),
                ("namespace", true),
                ("module", true),
            ],
            untyped_methods: true,
            js_assign: true,
            ..BASE
        },
        "Rust" => Syntax {
            quote_lines: true,
            raw_strings: true,
            keywords: &[
                ("fn", false),
                ("struct", false),
                ("enum", false),
                ("union", false),
                ("trait", true),
                ("impl", true),
                ("mod", true),
                ("type", false),
                ("macro_rules", false),
            ],
            c_signatures: false,
            ..c_like
        },
        "Go" => Syntax {
            multi: &[("`", "`")],
            keywords: &[("func", false), ("type", false)],
            c_signatures: false,
            ..c_like
        },
        "Java" => Syntax {
            keywords: &[("class", true), ("interface", true), ("enum", false), ("record", false)],
            ..c_like
        },
        "Kotlin" => Syntax {
            multi: &[("\"\"\"", "\"\"\"")],
            keywords: &[
                ("fun", false),
                ("class", true),
                ("interface", true),
                ("object", true),
                ("typealias", false),
            ],
            c_signatures: false,
            ..c_like
        },
        "C" | "C++" | "GLSL" | "HLSL" => Syntax {
            preprocessor: true,
            keywords: &[
                ("class", true),
                ("struct", true),
                ("union", false),
                ("enum", false),
                ("namespace", true),
            ],
            ..c_like
        },
        "C#" => Syntax {
            preprocessor: true,
            verbatim: true,
            multi: &[("\"\"\"", "\"\"\"")],
            keywords: &[
                ("class", true),
                ("interface", true),
                ("struct", true),
                ("record", true),
                ("enum", false),
                ("namespace", true),
                ("delegate", false),
            ],
            ..c_like
        },
        "Swift" => Syntax {
            multi: &[("\"\"\"", "\"\"\"")],
            preprocessor: true,
            keywords: &[
                ("func", false),
                ("class", true),
                ("struct", true),
                ("enum", true),
                ("protocol", true),
                ("extension", true),
                ("actor", true),
                ("init", false),
                ("deinit", false),
                ("subscript", false),
                ("typealias", false),
            ],
            c_signatures: false,
            ..c_like
        },
        "Objective-C" => Syntax {
            preprocessor: true,
            keywords: &[
                ("@interface", true),
                ("@implementation", true),
                ("@protocol", true),
                ("struct", false),
                ("enum", false),
            ],
            ..c_like
        },
        "D" => Syntax {
            multi: &[("`", "`")],
            block: &[("/*", "*/"), ("/+", "+/")],
            quote_lines: true,
            keywords: &[
                ("class", true),
                ("struct", true),
                ("interface", true),
                ("union", false),
                ("enum", false),
                ("template", true),
            ],
            ..c_like
        },
        "Dart" => Syntax {
            multi: &[("\"\"\"", "\"\"\""), ("'''", "'''")],
            quote: Quote::String,
            keywords: &[
                ("class", true),
                ("mixin", true),
                ("extension", true),
                ("enum", false),
                ("typedef", false),
            ],
            untyped_methods: true,
            ..c_like
        },
        "Groovy" => Syntax {
            multi: &[("\"\"\"", "\"\"\""), ("'''", "'''")],
            quote: Quote::String,
            keywords: &[
                ("def", false),
                ("class", true),
                ("interface", true),
                ("enum", false),
                ("trait", true),
            ],
            ..c_like
        },
        "Scala" => Syntax {
            multi: &[("\"\"\"", "\"\"\"")],
            keywords: &[
                ("def", false),
                ("class", true),
                ("object", true),
                ("trait", true),
                ("enum", true),
                ("type", false),
                ("given", false),
                ("extension", true),
            ],
            c_signatures: false,
            ..c_like
        },
        "Solidity" => Syntax {
            keywords: &[
                ("contract", true),
                ("library", true),
                ("interface", true),
                ("function", false),
                ("modifier", false),
                ("event", false),
                ("struct", false),
                ("enum", false),
                ("constructor", false),
                ("fallback", false),
                ("receive", false),
            ],
            c_signatures: false,
            quote: Quote::String,
            ..c_like
        },
        "Zig" => Syntax {
            multi: &[],
            keywords: &[("fn", false), ("test", false)],
            c_signatures: false,
            ..c_like
        },
        "PHP" => Syntax {
            line: &["//", "#"],
            quote: Quote::String,
            quote_lines: true,
            heredocs: true,
            keywords: &[
                ("function", false),
                ("class", true),
                ("interface", true),
                ("trait", true),
                ("enum", true),
            ],
            c_signatures: false,
            ..c_like
        },
        "Ruby" => Syntax {
            line: HASH,
            block: &[("=begin", "=end")],
            quote_lines: true,
            heredocs: true,
            keywords: &[("def", false), ("class", true), ("module", true)],
            ..BASE
        },
        "Shell" => Syntax {
            line: HASH,
            quote_lines: true,
            heredocs: true,
            keywords: &[("function", false)],
            ..BASE
        },
        "Fish" => Syntax {
            line: HASH,
            quote_lines: true,
            keywords: &[("function", false)],
            ..BASE
        },
        "Perl" => Syntax {
            line: HASH,
            block: &[
                ("=pod", "=cut"),
                ("=head1", "=cut"),
                ("=head2", "=cut"),
                ("=begin", "=cut"),
            ],
            quote_lines: true,
            heredocs: true,
            keywords: &[("sub", false)],
            ..BASE
        },
        "R" => Syntax {
            line: HASH,
            quote_lines: true,
            assign_function: true,
            ..BASE
        },
        "Julia" => Syntax {
            line: HASH,
            block: &[("#=", "=#")],
            multi: &[("\"\"\"", "\"\"\"")],
            quote: Quote::Char,
            keywords: &[
                ("function", false),
                ("macro", false),
                ("struct", false),
                ("module", true),
                ("abstract", false),
                ("primitive", false),
            ],
            ..BASE
        },
        "Elixir" => Syntax {
            line: HASH,
            multi: &[("\"\"\"", "\"\"\""), ("'''", "'''")],
            keywords: &[
                ("defmodule", true),
                ("def", false),
                ("defp", false),
                ("defmacro", false),
                ("defmacrop", false),
                ("defprotocol", true),
                ("defimpl", true),
                ("defguard", false),
                ("defdelegate", false),
            ],
            merge_clauses: true,
            ..BASE
        },
        "Erlang" => Syntax {
            line: &["%"],
            quote: Quote::No,
            equations: true,
            merge_clauses: true,
            ..BASE
        },
        "Prolog" => Syntax {
            line: &["%"],
            block: C_COMMENTS,
            quote: Quote::String,
            equations: true,
            merge_clauses: true,
            ..BASE
        },
        "Haskell" | "Elm" | "Idris" => Syntax {
            words: Words::Primes,
            line: &["--"],
            block: &[("{-", "-}")],
            quote: Quote::No,
            keywords: &[
                ("data", false),
                ("newtype", false),
                ("type", false),
                ("class", true),
                ("instance", true),
                ("interface", true),
                ("implementation", true),
                ("record", false),
                ("port", false),
            ],
            equations: true,
            merge_clauses: true,
            ..BASE
        },
        "OCaml" => Syntax {
            words: Words::Primes,
            block: &[("(*", "*)")],
            quote: Quote::No,
            keywords: &[
                ("let", false),
                ("and", false),
                ("type", false),
                ("module", true),
                ("class", true),
                ("val", false),
                ("external", false),
            ],
            ..BASE
        },
        "F#" => Syntax {
            words: Words::Primes,
            line: SLASHES,
            block: &[("(*", "*)")],
            multi: &[("\"\"\"", "\"\"\"")],
            quote: Quote::No,
            keywords: &[
                ("let", false),
                ("and", false),
                ("type", true),
                ("module", true),
                ("member", false),
                ("abstract", false),
                ("new", false),
            ],
            ..BASE
        },
        "ReScript" => Syntax {
            words: Words::Primes,
            line: SLASHES,
            block: C_COMMENTS,
            multi: &[("`", "`")],
            quote: Quote::No,
            keywords: &[("let", false), ("type", false), ("module", true), ("external", false)],
            ..BASE
        },
        "Gleam" => Syntax {
            line: SLASHES,
            quote: Quote::No,
            keywords: &[("fn", false), ("type", false)],
            ..BASE
        },
        "Lua" => Syntax {
            line: &["--"],
            block: &[("--[[", "]]"), ("--[==[", "]==]")],
            multi: &[("[[", "]]")],
            keywords: &[("function", false)],
            assign_function: true,
            ..BASE
        },
        "Clojure" | "Common Lisp" | "Scheme" | "Emacs Lisp" => Syntax {
            line: &[";"],
            block: &[("#|", "|#")],
            quote_lines: true,
            quote: Quote::No,
            lisp: true,
            words: Words::Lisp,
            ..BASE
        },
        "Ada" => Syntax {
            line: &["--"],
            quote: Quote::Char,
            keywords: &[
                ("procedure", false),
                ("function", false),
                ("package", true),
                ("task", true),
                ("protected", true),
            ],
            any_case: true,
            ..BASE
        },
        "Visual Basic" => Syntax {
            line: &["'"],
            line_at_start: &["REM "],
            quote: Quote::No,
            keywords: &[
                ("sub", false),
                ("function", false),
                ("property", false),
                ("class", true),
                ("module", true),
                ("structure", true),
                ("interface", true),
                ("enum", false),
            ],
            any_case: true,
            ..BASE
        },
        "PowerShell" => Syntax {
            words: Words::Dashes,
            line: HASH,
            block: &[("<#", "#>")],
            multi: &[("@\"", "\"@"), ("@'", "'@")],
            keywords: &[
                ("function", false),
                ("filter", false),
                ("class", true),
                ("enum", false),
                ("workflow", false),
                ("configuration", false),
            ],
            any_case: true,
            ..BASE
        },
        "MATLAB" => Syntax {
            line: &["%"],
            block: &[("%{", "%}")],
            quote: Quote::No,
            keywords: &[("function", false), ("classdef", true)],
            ..BASE
        },
        "AWK" => Syntax {
            line: HASH,
            keywords: &[("function", false), ("func", false)],
            ..BASE
        },
        "Vim script" => Syntax {
            line_at_start: &["\""],
            quote: Quote::String,
            keywords: &[
                ("function", false),
                ("fu", false),
                ("fun", false),
                ("func", false),
                ("def", false),
            ],
            ..BASE
        },
        "SQL" => Syntax {
            line: &["--"],
            block: C_COMMENTS,
            multi: &[("$$", "$$")],
            sql: true,
            any_case: true,
            ..BASE
        },
        "Protocol Buffers" | "Thrift" | "Cap'n Proto" => Syntax {
            line: &["//", "#"],
            block: C_COMMENTS,
            keywords: &[
                ("message", false),
                ("service", true),
                ("enum", false),
                ("rpc", false),
                ("struct", true),
                ("union", false),
                ("exception", false),
                ("interface", true),
            ],
            ..BASE
        },
        "GraphQL" => Syntax {
            line: HASH,
            multi: &[("\"\"\"", "\"\"\"")],
            keywords: &[
                ("type", false),
                ("input", false),
                ("interface", false),
                ("enum", false),
                ("union", false),
                ("scalar", false),
                ("query", false),
                ("mutation", false),
                ("subscription", false),
                ("fragment", false),
                ("directive", false),
            ],
            ..BASE
        },
        "Makefile" | "just" => Syntax {
            line: HASH,
            quote: Quote::No,
            targets: true,
            ..BASE
        },
        "CMake" => Syntax {
            line: HASH,
            block: &[("#[[", "]]")],
            keywords: &[("function", false), ("macro", false)],
            any_case: true,
            ..BASE
        },
        "Rego" => Syntax {
            line: HASH,
            equations: true,
            ..BASE
        },
        "Batch" => Syntax {
            line_at_start: &["REM ", "rem ", "::"],
            quote: Quote::No,
            ..BASE
        },
        "Nix" | "HCL" | "Meson" | "Ninja" | "Dockerfile" => Syntax {
            line: &["#", "//"],
            block: C_COMMENTS,
            ..BASE
        },
        "Assembly" => Syntax {
            line: &[";", "#", "//"],
            quote: Quote::No,
            ..BASE
        },
        _ => BASE,
    }
}

struct Line {
    indent: usize,
    code: String,
    inside: bool,
    comment: bool,
}

impl Line {
    fn blank(&self) -> bool {
        self.code.trim().is_empty()
    }
}

fn indent_of(line: &str) -> usize {
    let mut n = 0;
    for c in line.chars() {
        match c {
            ' ' => n += 1,
            '\t' => n += 4,
            _ => break,
        }
    }
    n
}

fn lex(text: &str, s: &Syntax) -> Vec<Line> {
    let spans = scan(text, s, text.len());
    let mut out = Vec::new();
    let mut next = 0;
    let mut start = 0;
    for raw in text.split('\n') {
        let end = start + raw.len();
        while next < spans.len() && spans[next].0.end <= start {
            next += 1;
        }
        let mut code = String::with_capacity(raw.len());
        let mut inside = false;
        let mut comment = false;
        let mut at = start;
        for (range, class) in spans[next..]
            .iter()
            .take_while(|(range, _)| range.start < end || range.start == start)
        {
            if range.start < start && range.end > start && matches!(class, Class::Comment | Class::String) {
                inside = true;
            }
            if *class == Class::Comment {
                let (from, to) = (range.start.max(start), range.end.min(end));
                if from < to {
                    comment = true;
                    code.push_str(&text[at..from.max(at)]);
                    at = to;
                }
            }
        }
        if at < end {
            code.push_str(&text[at..end]);
        }
        out.push(Line {
            indent: indent_of(raw),
            comment: comment && code.trim().is_empty(),
            code,
            inside,
        });
        start = end + 1;
    }
    out
}

fn find_close(text: &str, close: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(at) = text[from..].find(close) {
        let at = from + at;
        let slashes = text[..at].bytes().rev().take_while(|&b| b == b'\\').count();
        if slashes % 2 == 0 || close.len() > 1 {
            return Some(at);
        }
        from = at + close.len();
    }
    None
}

fn heredoc_word(after: &str) -> Option<String> {
    let after = after.trim_start_matches(['-', '~', '<']).trim_start();
    let after = after.trim_start_matches(['\'', '"']);
    let word: String = after.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    (!word.is_empty() && word.chars().next().is_some_and(|c| c.is_ascii_uppercase() || c == '_')).then_some(word)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Class {
    Comment,
    String,
    Number,
    Word,
}

pub(crate) fn spans(text: &str, language: &str, until: usize) -> Vec<(Range<usize>, Class)> {
    scan(text, &syntax(language), until)
}

fn scan(text: &str, s: &Syntax, until: usize) -> Vec<(Range<usize>, Class)> {
    let words = s.words;
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line_has_code = false;
    let mut heredoc: Option<String> = None;
    let until = until.min(bytes.len());
    let is_word = |c: char| {
        c.is_alphanumeric()
            || c == '_'
            || (words == Words::Lisp && matches!(c, '-' | '?' | '!' | '*' | '>' | '<' | '=' | '/' | '+'))
            || (words == Words::Dashes && c == '-')
            || (words == Words::Primes && c == '\'')
    };
    let close_at = |from: usize, close: &str, lines: bool| -> usize {
        let rest = &text[from..];
        let limit = if lines {
            rest.len()
        } else {
            rest.find('\n').unwrap_or(rest.len())
        };
        match find_close(&rest[..limit], close) {
            Some(at) => from + at + close.len(),
            None => from + limit,
        }
    };
    while i < until {
        if !text.is_char_boundary(i) {
            i += 1;
            continue;
        }
        let rest = &text[i..];
        let c = rest.chars().next().unwrap_or(' ');
        if c == '\n' {
            i += 1;
            line_has_code = false;
            if let Some(word) = heredoc.take() {
                let mut end = i;
                for line in text[i..].split_inclusive('\n') {
                    end += line.len();
                    if line.trim().trim_end_matches(';') == word {
                        break;
                    }
                }
                out.push((i - 1..end.min(text.len()), Class::String));
                i = end.min(text.len());
            }
            continue;
        }
        if c == ' ' || c == '\t' || c == '\r' {
            i += 1;
            continue;
        }
        let at_line_start = !line_has_code;
        line_has_code = true;
        if s.line.iter().any(|m| rest.starts_with(m))
            || (at_line_start && s.line_at_start.iter().any(|m| rest.starts_with(m)))
        {
            let end = i + rest.find('\n').unwrap_or(rest.len());
            out.push((i..end, Class::Comment));
            i = end;
            continue;
        }
        let column_0 = i == 0 || bytes[i - 1] == b'\n';
        if let Some((open, close)) = s
            .block
            .iter()
            .find(|(open, _)| rest.starts_with(open) && (!open.starts_with('=') || column_0))
        {
            let end = text[i + open.len()..]
                .find(close)
                .map_or(text.len(), |at| i + open.len() + at + close.len());
            out.push((i..end, Class::Comment));
            i = end;
            continue;
        }
        if let Some((open, close)) = s.multi.iter().find(|(open, _)| rest.starts_with(open)) {
            let end = close_at(i + open.len(), close, true);
            out.push((i..end, Class::String));
            i = end;
            continue;
        }
        if s.verbatim && (rest.starts_with("@\"") || rest.starts_with("$@\"") || rest.starts_with("@$\"")) {
            let mut k = i + if rest.starts_with("@\"") { 2 } else { 3 };
            while k < bytes.len() {
                if bytes[k] == b'"' {
                    if bytes.get(k + 1) == Some(&b'"') {
                        k += 2;
                        continue;
                    }
                    k += 1;
                    break;
                }
                k += 1;
            }
            out.push((i..k.min(text.len()), Class::String));
            i = k.min(text.len());
            continue;
        }
        let after_word = i > 0
            && text[..i]
                .chars()
                .next_back()
                .is_some_and(|p| p.is_alphanumeric() || p == '_');
        if s.raw_strings
            && !after_word
            && (rest.starts_with("r\"")
                || rest.starts_with("r#\"")
                || rest.starts_with("r##\"")
                || rest.starts_with("br\""))
        {
            let at = rest.find('r').unwrap_or(0) + 1;
            let hashes = rest[at..].bytes().take_while(|&b| b == b'#').count();
            let close = format!("\"{}", "#".repeat(hashes));
            let open = i + at + hashes + 1;
            let end = text[open..].find(&close).map_or(text.len(), |e| open + e + close.len());
            out.push((i..end, Class::String));
            i = end;
            continue;
        }
        if s.heredocs
            && rest.starts_with("<<")
            && let Some(word) = heredoc_word(&rest[2..])
        {
            heredoc = Some(word);
        }
        if c == '"' {
            let end = close_at(i + 1, "\"", s.quote_lines);
            out.push((i..end, Class::String));
            i = end;
            continue;
        }
        if c == '\'' && s.quote != Quote::No {
            let end = close_at(i + 1, "'", false);
            let closed = text[i + 1..end].ends_with('\'');
            if s.quote == Quote::String || (closed && end - i <= 8) {
                out.push((i..end, Class::String));
                i = end;
                continue;
            }
        }
        if c == '`' && s.multi.is_empty() && words != Words::Lisp {
            let end = close_at(i + 1, "`", false);
            out.push((i..end, Class::String));
            i = end;
            continue;
        }
        if c.is_ascii_digit() && !after_word {
            let mut k = i;
            while k < bytes.len()
                && (bytes[k].is_ascii_alphanumeric()
                    || bytes[k] == b'_'
                    || (bytes[k] == b'.' && bytes.get(k + 1).is_some_and(u8::is_ascii_digit)))
            {
                k += 1;
            }
            out.push((i..k, Class::Number));
            i = k;
            continue;
        }
        if is_word(c)
            || ((c == '@' || c == '$') && rest[1..].chars().next().is_some_and(|d| d.is_alphabetic() || d == '_'))
        {
            let mut k = i + c.len_utf8();
            for d in text[k..].chars() {
                if !is_word(d) {
                    break;
                }
                k += d.len_utf8();
            }
            if k < bytes.len() && matches!(bytes[k], b'?' | b'!') && !bytes.get(k + 1).is_some_and(|b| *b == b'=') {
                k += 1;
            }
            out.push((i..k, Class::Word));
            i = k;
            continue;
        }
        i += c.len_utf8();
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tok<'a> {
    Word(&'a str),
    Mark(char),
    Text(&'a str),
}

#[derive(Clone, Copy, PartialEq)]
enum Words {
    Plain,
    Lisp,
    Primes,
    Dashes,
}

fn tokens(code: &str, words: Words) -> Vec<Tok<'_>> {
    let lisp = words == Words::Lisp;
    let mut out = Vec::new();
    let mut chars = code.char_indices().peekable();
    let is_word = |c: char| {
        c.is_alphanumeric()
            || c == '_'
            || c == '$'
            || (lisp && matches!(c, '-' | '?' | '!' | '*' | '>' | '<' | '=' | '/' | '+' | '.'))
            || (words == Words::Primes && c == '\'')
            || (words == Words::Dashes && c == '-')
    };
    while let Some((i, c)) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        if is_word(c) {
            let mut end = i + c.len_utf8();
            while let Some(&(j, d)) = chars.peek() {
                if !is_word(d) {
                    break;
                }
                end = j + d.len_utf8();
                chars.next();
            }
            out.push(Tok::Word(&code[i..end]));
        } else if c == '"' || c == '`' || (c == '\'' && !lisp && words != Words::Primes) {
            let rest = &code[i + 1..];
            match rest.find(c) {
                Some(at) if c == '"' || at <= 6 => {
                    out.push(Tok::Text(&rest[..at]));
                    while let Some(&(j, _)) = chars.peek() {
                        if j > i + at + 1 {
                            break;
                        }
                        chars.next();
                    }
                }
                _ => out.push(Tok::Mark(c)),
            }
        } else {
            out.push(Tok::Mark(c));
        }
    }
    out
}

fn after_balanced(t: &[Tok], at: usize, open: char, close: char) -> usize {
    let mut depth = 0;
    for (i, tok) in t.iter().enumerate().skip(at) {
        match tok {
            Tok::Mark(c) if *c == open => depth += 1,
            Tok::Mark(c) if *c == close => {
                depth -= 1;
                if depth <= 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    t.len()
}

const MODIFIERS: &[&str] = &[
    "pub",
    "public",
    "private",
    "protected",
    "internal",
    "static",
    "final",
    "abstract",
    "sealed",
    "open",
    "override",
    "virtual",
    "async",
    "export",
    "default",
    "unsafe",
    "extern",
    "inline",
    "data",
    "inner",
    "lazy",
    "partial",
    "readonly",
    "declare",
    "suspend",
    "operator",
    "infix",
    "tailrec",
    "fileprivate",
    "mutating",
    "nonmutating",
    "convenience",
    "required",
    "implicit",
    "case",
    "local",
    "global",
    "const",
    "value",
    "annotation",
    "companion",
    "actual",
    "expect",
    "external",
    "transient",
    "synchronized",
    "native",
    "strictfp",
    "constexpr",
    "consteval",
    "friend",
    "explicit",
    "volatile",
    "mutable",
    "dynamic",
    "nonisolated",
    "indirect",
    "prefix",
    "postfix",
    "accessor",
    "noinline",
    "crossinline",
    "reified",
    "lateinit",
    "shared",
    "immutable",
    "nothrow",
    "pure",
    "auto",
    "ref",
    "rec",
    "nonrec",
    "packed",
    "mixin",
    "enum",
    "friend",
    "overrides",
    "overridable",
    "mustoverride",
    "notoverridable",
    "overloads",
    "shadows",
    "iterator",
    "notinheritable",
    "mustinherit",
    "unique",
    "transparent",
    "opaque",
    "erased",
    "thread_local",
    "register",
    "__global__",
    "__device__",
    "__host__",
    "__forceinline__",
    "WINAPI",
    "struct",
];

const CONTROL: &[&str] = &[
    "if",
    "for",
    "while",
    "switch",
    "return",
    "catch",
    "else",
    "do",
    "sizeof",
    "new",
    "throw",
    "case",
    "using",
    "delete",
    "foreach",
    "lock",
    "await",
    "yield",
    "assert",
    "elif",
    "when",
    "with",
    "match",
    "try",
    "unless",
    "until",
    "guard",
    "defer",
    "go",
    "select",
    "fixed",
    "checked",
    "unchecked",
    "import",
    "package",
    "typedef",
    "goto",
    "in",
    "is",
    "as",
    "and",
    "or",
    "not",
    "print",
    "printf",
    "echo",
    "throws",
    "static_assert",
    "decltype",
    "typeof",
    "alignas",
    "alignof",
    "requires",
    "co_await",
    "co_return",
    "co_yield",
    "super",
    "this",
    "self",
    "Self",
];

impl Syntax {
    fn keyword(&self, word: &str) -> Option<bool> {
        self.keywords
            .iter()
            .find(|(k, _)| {
                if self.any_case {
                    k.eq_ignore_ascii_case(word)
                } else {
                    *k == word
                }
            })
            .map(|&(_, holds)| holds)
    }
}

fn definition(s: &Syntax, line: &Line, t: &[Tok], after: &[Line], inside: Option<&str>) -> Option<(String, bool)> {
    if t.is_empty() {
        return None;
    }
    if s.lisp {
        return lisp_definition(t);
    }
    if s.sql {
        return sql_definition(t);
    }
    if s.targets {
        return target(line, t);
    }
    if s.equations
        && line.indent == 0
        && let Some(found) = equation(s, t)
    {
        return Some(found);
    }
    let mut i = 0;
    loop {
        match t.get(i) {
            Some(Tok::Word(w))
                if (MODIFIERS.contains(w) || (s.any_case && MODIFIERS.iter().any(|m| m.eq_ignore_ascii_case(w))))
                    && s.keyword(w).is_none() =>
            {
                i += 1;
                if matches!(t.get(i), Some(Tok::Mark('('))) && (*w == "pub" || *w == "extern") {
                    i = after_balanced(t, i, '(', ')');
                }
                if matches!(t.get(i), Some(Tok::Text(_))) && *w == "extern" {
                    i += 1;
                }
            }
            Some(Tok::Word("template")) if matches!(t.get(i + 1), Some(Tok::Mark('<'))) => {
                i = after_balanced(t, i + 1, '<', '>')
            }
            Some(Tok::Mark('@')) => match t.get(i + 1) {
                Some(Tok::Word(w)) if s.keyword(&format!("@{w}")).is_some() => break,
                Some(Tok::Word(_)) => {
                    i += 2;
                    if matches!(t.get(i), Some(Tok::Mark('('))) {
                        i = after_balanced(t, i, '(', ')');
                    }
                }
                _ => break,
            },
            Some(Tok::Mark('[')) if i == 0 && s.c_signatures => i = after_balanced(t, i, '[', ']'),
            _ => break,
        }
    }
    if i >= t.len() {
        return None;
    }
    if let Some(found) = keyword_definition(s, line, t, i) {
        return Some(found);
    }
    if s.js_assign
        && let Some(found) = js_function(t, i)
    {
        return Some(found);
    }
    if s.assign_function
        && let Some(name) = assigned_function(t)
    {
        return Some((name, false));
    }
    if (s.c_signatures || (s.untyped_methods && inside.is_some()))
        && let Some(name) = signature(s, line, t, i, after, inside)
    {
        return Some((name, false));
    }
    if s.c_signatures
        && let Some(name) = objc_method(t)
    {
        return Some((name, false));
    }
    None
}

fn keyword_definition(s: &Syntax, line: &Line, t: &[Tok], mut i: usize) -> Option<(String, bool)> {
    let (word, at_mark) = match t.get(i) {
        Some(Tok::Mark('@')) => match t.get(i + 1) {
            Some(Tok::Word(w)) => (format!("@{w}"), true),
            _ => return None,
        },
        Some(Tok::Word(w)) => (w.to_string(), false),
        _ => return None,
    };
    let mut holds = s.keyword(&word)?;
    i += if at_mark { 2 } else { 1 };
    while let Some(Tok::Word(w)) = t.get(i) {
        match s.keyword(w) {
            Some(second) if !matches!(*w, "type" | "and") || word != "let" => {
                holds = second;
                i += 1;
            }
            _ if matches!(
                *w,
                "rec"
                    | "inline"
                    | "private"
                    | "mutable"
                    | "body"
                    | "static"
                    | "class"
                    | "internal"
                    | "public"
                    | "override"
                    | "default"
            ) && word != "class" =>
            {
                i += 1
            }
            _ => break,
        }
    }
    let code = line.code.trim_end();
    if code.ends_with(';') && !code.contains('{') && !code.contains('(') && word != "type" {
        return None;
    }
    let lower = word.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "init" | "deinit" | "subscript" | "constructor" | "fallback" | "receive" | "new"
    ) {
        return Some((lower, false));
    }
    match lower.as_str() {
        "func" if matches!(t.get(i), Some(Tok::Mark('('))) => i = after_balanced(t, i, '(', ')'),
        "impl" => {
            if matches!(t.get(i), Some(Tok::Mark('<'))) {
                i = after_balanced(t, i, '<', '>');
            }
            let rest = &t[i..];
            let at = rest
                .iter()
                .position(|tok| matches!(tok, Tok::Word("for")))
                .map_or(0, |at| at + 1);
            return rest[at..].iter().find_map(|tok| match tok {
                Tok::Word(w) if !matches!(*w, "dyn" | "mut") => Some((w.to_string(), true)),
                _ => None,
            });
        }
        "macro_rules" if matches!(t.get(i), Some(Tok::Mark('!'))) => i += 1,
        _ => {}
    }
    loop {
        match t.get(i) {
            Some(Tok::Mark('*')) => i += 1,
            Some(Tok::Mark('<')) => i = after_balanced(t, i, '<', '>'),
            Some(Tok::Word("self")) if matches!(t.get(i + 1), Some(Tok::Mark('.'))) => i += 2,
            _ => break,
        }
    }
    let mut name = match t.get(i) {
        Some(Tok::Word(w)) => w.to_string(),
        Some(Tok::Text(w)) if !w.is_empty() && !matches!(lower.as_str(), "module" | "package") => format!("`{w}`"),
        Some(Tok::Mark('[')) if lower == "function" => {
            let at = after_balanced(t, i, '[', ']');
            return named_before_paren(&t[at..]).map(|n| (n, false));
        }
        _ => return None,
    };
    let mut j = i + 1;
    while (matches!(t.get(j), Some(Tok::Mark('.'))) || (s.assign_function && matches!(t.get(j), Some(Tok::Mark(':')))))
        && matches!(t.get(j + 1), Some(Tok::Word(_)))
    {
        if let Some(Tok::Word(w)) = t.get(j + 1) {
            if lower.starts_with("def") && holds {
                name = format!("{name}.{w}");
            } else {
                name = w.to_string();
            }
        }
        j += 2;
    }
    if lower == "function" && matches!(t.get(j), Some(Tok::Mark('='))) {
        return named_before_paren(&t[j..]).map(|n| (n, false));
    }
    Some((name, holds))
}

fn named_before_paren(t: &[Tok]) -> Option<String> {
    let at = t.iter().position(|tok| matches!(tok, Tok::Mark('(')))?;
    match t.get(at.checked_sub(1)?) {
        Some(Tok::Word(w)) => Some(w.to_string()),
        _ => None,
    }
}

fn signature(s: &Syntax, line: &Line, t: &[Tok], from: usize, after: &[Line], inside: Option<&str>) -> Option<String> {
    if line.code.trim_start().starts_with('#') {
        return None;
    }
    if let Some(Tok::Word(w)) = t.first()
        && CONTROL.contains(w)
    {
        return None;
    }
    let paren = t.iter().position(|tok| matches!(tok, Tok::Mark('(')))?;
    if paren < from + 1 {
        return None;
    }
    let mut start = paren - 1;
    let Tok::Word(last) = t[start] else { return None };
    if CONTROL.contains(&last) || last == "operator" {
        return None;
    }
    let mut name = last.to_string();
    let mut qualified = false;
    loop {
        let tilde = start >= 1 && matches!(t[start - 1], Tok::Mark('~'));
        let at = if tilde { start - 1 } else { start };
        if at >= from + 3
            && matches!(t[at - 1], Tok::Mark(':'))
            && matches!(t[at - 2], Tok::Mark(':'))
            && let Tok::Word(outer) = t[at - 3]
        {
            if !qualified && tilde {
                name = format!("~{name}");
            }
            name = format!("{outer}::{name}");
            qualified = true;
            start = at - 3;
        } else {
            if tilde && !qualified {
                name = format!("~{name}");
                start = at;
            }
            break;
        }
    }
    let prefix = &t[from..start];
    let typed = prefix.iter().all(|tok| match tok {
        Tok::Word(w) => !CONTROL.contains(w),
        Tok::Mark(c) => matches!(c, '*' | '&' | '<' | '>' | ',' | ':' | '[' | ']' | '.' | '?' | '!'),
        Tok::Text(_) => false,
    });
    if !typed {
        return None;
    }
    let words_before = prefix.iter().filter(|tok| matches!(tok, Tok::Word(_))).count();
    let constructor = inside.is_some_and(|c| c.rsplit("::").next() == Some(name.as_str()));
    if words_before == 0 && !qualified && !constructor && !(s.untyped_methods && inside.is_some()) {
        return None;
    }
    let mut text = String::new();
    for piece in std::iter::once(line).chain(after.iter().take(12)) {
        if !piece.inside {
            text.push_str(&piece.code);
        }
        text.push('\n');
    }
    let open = text.find('(')?;
    let mut depth = 0i32;
    let mut closed = None;
    for (k, c) in text[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    closed = Some(open + k + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    let rest = &text[closed?..];
    let body = rest.find('{');
    let end = rest.find(';');
    let arrow = rest.find("=>");
    let first = |a: Option<usize>, b: Option<usize>| match (a, b) {
        (Some(a), Some(b)) => a < b,
        (Some(_), None) => true,
        _ => false,
    };
    (first(body, end) || (first(arrow, end) && first(arrow, body))).then_some(name)
}

fn objc_method(t: &[Tok]) -> Option<String> {
    if !matches!(t.first(), Some(Tok::Mark('-')) | Some(Tok::Mark('+'))) || !matches!(t.get(1), Some(Tok::Mark('('))) {
        return None;
    }
    let at = after_balanced(t, 1, '(', ')');
    match t.get(at) {
        Some(Tok::Word(w)) => Some(w.to_string()),
        _ => None,
    }
}

fn js_function(t: &[Tok], from: usize) -> Option<(String, bool)> {
    let declared = |i: usize| matches!(t.get(i), Some(Tok::Word("const" | "let" | "var")));
    let i = if declared(from) {
        from + 1
    } else if from > 0 && declared(from - 1) {
        from
    } else {
        return None;
    };
    let Some(Tok::Word(name)) = t.get(i) else { return None };
    let equals = t[i..].iter().position(|tok| matches!(tok, Tok::Mark('=')))? + i;
    if matches!(t.get(equals + 1), Some(Tok::Mark('=') | Tok::Mark('>'))) {
        return None;
    }
    let mut k = equals + 1;
    if matches!(t.get(k), Some(Tok::Word("async"))) {
        k += 1;
    }
    if matches!(t.get(k), Some(Tok::Word("class"))) {
        return Some((name.to_string(), true));
    }
    let function = match t.get(k) {
        Some(Tok::Word("function")) => true,
        Some(Tok::Mark('(')) | Some(Tok::Mark('<')) => {
            t[k..].windows(2).any(|w| matches!(w, [Tok::Mark('='), Tok::Mark('>')]))
        }
        Some(Tok::Word(_)) => matches!(t.get(k + 1..k + 3), Some([Tok::Mark('='), Tok::Mark('>')])),
        _ => false,
    };
    function.then(|| (name.to_string(), false))
}

fn assigned_function(t: &[Tok]) -> Option<String> {
    let mut i = 0;
    if matches!(t.first(), Some(Tok::Word("local"))) {
        i = 1;
    }
    let mut name = match t.get(i) {
        Some(Tok::Word(w)) => w.to_string(),
        _ => return None,
    };
    i += 1;
    while matches!(t.get(i), Some(Tok::Mark('.') | Tok::Mark(':') | Tok::Mark('$'))) {
        if let Some(Tok::Word(w)) = t.get(i + 1) {
            name = w.to_string();
        }
        i += 2;
    }
    match t.get(i..i + 2) {
        Some([Tok::Mark('<'), Tok::Mark('-')]) => i += 2,
        Some([Tok::Mark('='), next]) if !matches!(next, Tok::Mark('=')) => i += 1,
        _ => return None,
    }
    matches!(t.get(i), Some(Tok::Word("function"))).then_some(name)
}

fn lisp_definition(t: &[Tok]) -> Option<(String, bool)> {
    const DEFINES: &[&str] = &[
        "defun",
        "defn",
        "defn-",
        "defmacro",
        "define",
        "defmethod",
        "defclass",
        "defstruct",
        "defvar",
        "defparameter",
        "defgeneric",
        "defrecord",
        "defprotocol",
        "deftype",
        "define-syntax",
        "defmulti",
        "defsubst",
        "cl-defun",
        "define-minor-mode",
        "define-derived-mode",
        "defcustom",
        "defconst",
        "defalias",
    ];
    if !matches!(t.first(), Some(Tok::Mark('('))) {
        return None;
    }
    let Some(Tok::Word(head)) = t.get(1) else { return None };
    if !DEFINES.contains(head) {
        return None;
    }
    let mut i = 2;
    while matches!(t.get(i), Some(Tok::Mark('^') | Tok::Mark(':') | Tok::Mark('('))) {
        i += 1;
        if matches!(t.get(i - 1), Some(Tok::Mark('^') | Tok::Mark(':')))
            && matches!(t.get(i), Some(Tok::Word(_)))
            && matches!(t.get(i + 1), Some(Tok::Word(_)))
        {
            i += 1;
        }
    }
    match t.get(i) {
        Some(Tok::Word(w)) => Some((w.to_string(), false)),
        _ => None,
    }
}

fn sql_definition(t: &[Tok]) -> Option<(String, bool)> {
    const KINDS: &[&str] = &[
        "function",
        "procedure",
        "trigger",
        "view",
        "table",
        "index",
        "type",
        "schema",
        "sequence",
        "domain",
    ];
    let word = |i: usize| match t.get(i) {
        Some(Tok::Word(w)) => Some(w.to_ascii_lowercase()),
        _ => None,
    };
    if word(0).as_deref() != Some("create") {
        return None;
    }
    let mut i = 1;
    while let Some(w) = word(i) {
        i += 1;
        if KINDS.contains(&w.as_str()) {
            while matches!(word(i).as_deref(), Some("if" | "not" | "exists")) {
                i += 1;
            }
            let mut name = match t.get(i) {
                Some(Tok::Word(w)) => w.to_string(),
                Some(Tok::Text(w)) => w.to_string(),
                _ => return None,
            };
            while matches!(t.get(i + 1), Some(Tok::Mark('.'))) {
                if let Some(Tok::Word(w) | Tok::Text(w)) = t.get(i + 2) {
                    name = w.to_string();
                }
                i += 2;
            }
            return Some((name, false));
        }
    }
    None
}

fn target(line: &Line, t: &[Tok]) -> Option<(String, bool)> {
    if line.indent > 0
        || line.code.contains(":=")
        || line.code.contains("::=")
        || line.code.trim_start().starts_with('.')
    {
        return None;
    }
    let colon = t.iter().position(|tok| matches!(tok, Tok::Mark(':')))?;
    if matches!(t.get(colon + 1), Some(Tok::Mark('='))) || t[..colon].iter().any(|tok| matches!(tok, Tok::Mark('='))) {
        return None;
    }
    match t.first() {
        Some(Tok::Word(w))
            if !matches!(
                *w,
                "export" | "include" | "ifeq" | "ifneq" | "ifdef" | "else" | "endif" | "set" | "import" | "alias"
            ) =>
        {
            Some((w.to_string(), false))
        }
        _ => None,
    }
}

fn equation(s: &Syntax, t: &[Tok]) -> Option<(String, bool)> {
    const NOT: &[&str] = &[
        "import", "module", "where", "let", "in", "if", "then", "else", "case", "of", "do", "infixl", "infixr",
        "infix", "deriving", "foreign", "package", "default", "true", "false", "not", "some", "every", "with",
    ];
    let Some(Tok::Word(name)) = t.first() else { return None };
    if NOT.contains(name)
        || s.keyword(name).is_some()
        || !name.chars().next().is_some_and(|c| c.is_lowercase() || c == '_')
    {
        return None;
    }
    if s.keyword("data").is_some() {
        return Some((name.to_string(), false));
    }
    let marks: String = t
        .iter()
        .filter_map(|tok| if let Tok::Mark(c) = tok { Some(*c) } else { None })
        .collect();
    let defines = marks.contains("->")
        || marks.contains(":-")
        || marks.contains("::")
        || (marks.contains('=') && !marks.contains("=="))
        || marks.contains('|')
        || marks.ends_with('{');
    defines.then(|| (name.to_string(), false))
}

fn closes(line: &Line) -> bool {
    let code = line.code.trim_start();
    let first: String = code
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '@')
        .collect();
    code.starts_with(['}', ')', ']'])
        || matches!(
            first.to_ascii_lowercase().as_str(),
            "end"
                | "fi"
                | "done"
                | "esac"
                | "@end"
                | "endfunction"
                | "endfunc"
                | "enddef"
                | "endif"
                | "endmodule"
                | "endclass"
                | "endmacro"
                | "until"
        )
        || code.starts_with("end;")
}

fn end_of(lines: &[Line], at: usize, s: &Syntax) -> usize {
    let level = lines[at].indent;
    let mut last = at;
    for (k, line) in lines.iter().enumerate().skip(at + 1) {
        if line.inside || line.blank() {
            continue;
        }
        if s.preprocessor && line.code.trim_start().starts_with('#') {
            continue;
        }
        if line.indent > level {
            last = k;
            continue;
        }
        if line.indent == level && (closes(line) || line.code.trim_start().starts_with('{')) {
            last = k;
            continue;
        }
        if s.c_signatures && line.indent < level && is_label(line) {
            continue;
        }
        break;
    }
    last
}

fn is_label(line: &Line) -> bool {
    let code = line.code.trim();
    code.ends_with(':')
        && code[..code.len() - 1].chars().all(|c| c.is_alphanumeric() || c == '_')
        && !code.starts_with("default")
}

fn start_of(lines: &[Line], at: usize) -> usize {
    let mut start = at;
    while start > 0 {
        let above = &lines[start - 1];
        let code = above.code.trim();
        let attribute = code.starts_with('@')
            || code.starts_with("#[")
            || (code.starts_with("#!") && code.ends_with(']'))
            || (code.starts_with('[') && code.ends_with(']'));
        if above.comment || attribute {
            start -= 1;
        } else {
            break;
        }
    }
    start
}

fn find(lines: &[Line], s: &Syntax, from: usize, to: usize, outer: Option<&str>, depth: usize, out: &mut Vec<Found>) {
    let mut k = from;
    while k < to {
        let line = &lines[k];
        if line.inside || line.blank() {
            k += 1;
            continue;
        }
        let t = tokens(&line.code, s.words);
        let Some((name, holds)) = definition(s, line, &t, &lines[k + 1..], outer) else {
            k += 1;
            continue;
        };
        let end = end_of(lines, k, s).min(to.saturating_sub(1)).max(k);
        let full = match outer {
            Some(outer) => format!("{outer}::{name}"),
            None => name,
        };
        let start = start_of(lines, k);
        if holds && depth < 8 && end > k {
            let before = out.len();
            find(lines, s, k + 1, end + 1, Some(&full), depth + 1, out);
            if out.len() > before {
                k = end + 1;
                continue;
            }
        }
        out.push(Found {
            start: start as u32 + 1,
            end: end as u32 + 1,
            name: full,
        });
        k = end + 1;
    }
}

pub(crate) fn symbols(text: &str, language: &str) -> Vec<Found> {
    let s = syntax(language);
    let lines = lex(text, &s);
    let mut out = Vec::new();
    find(&lines, &s, 0, lines.len(), None, 0, &mut out);
    if s.merge_clauses {
        let mut merged: Vec<Found> = Vec::new();
        for found in out {
            match merged.last_mut() {
                Some(last) if last.name == found.name && found.start <= last.end + 3 => last.end = found.end,
                _ => merged.push(found),
            }
        }
        out = merged;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(language: &str, text: &str) -> Vec<(u32, u32, String)> {
        symbols(text, language)
            .into_iter()
            .map(|f| (f.start, f.end, f.name))
            .collect()
    }

    #[test]
    fn python_functions_with_decorators() {
        let py = "import os\n\n# Tells a deleted folder from an unplugged drive.\ndef is_gone(path):\n    \"\"\"Whether it was deleted.\n\nOr not.\"\"\"\n    return os.path.lexists(path)\n\nclass Store:\n    @property\n    def save(self, settings):\n        with open(self.path) as f:\n            f.write(settings)\n\nx = 1\n";
        let found = names("Python", py);
        assert_eq!(found, [(3, 8, "is_gone".into()), (11, 14, "Store::save".into())]);
    }

    #[test]
    fn rust_methods_and_braces_in_strings() {
        let rs = "/// Settings.\npub struct Store {\n    path: String,\n}\n\nimpl<T> Keep for Store<T> {\n    /// Keeps them.\n    pub fn set(&self, s: &str) -> Result<()> {\n        let x = \"}\";\n        fs::write(&self.path, s)\n    }\n}\n";
        assert_eq!(
            names("Rust", rs),
            [(1, 4, "Store".into()), (7, 11, "Store::set".into())]
        );
    }
}
