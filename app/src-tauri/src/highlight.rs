use std::{ops::Range, path::Path};

use serde::Serialize;

use crate::{
    embed::{
        code::Lang,
        outline::{Class, spans},
    },
    words,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Keyword,
    String,
    Comment,
    Number,
    Function,
    Type,
    Property,
    Variable,
}

const C: &str = "auto break case char const continue default do double else enum extern float for goto if inline int long register restrict return short signed sizeof static struct switch typedef union unsigned void volatile while bool _Bool";
const CPP: &str = "alignas alignof and asm catch class constexpr consteval constinit co_await co_return co_yield concept const_cast decltype delete dynamic_cast explicit export final friend mutable namespace new noexcept nullptr operator or override private protected public reinterpret_cast requires static_assert static_cast template throw try typeid typename using virtual wchar_t char8_t char16_t char32_t";
const JAVA: &str = "abstract assert boolean break byte case catch char class const continue default do double else enum extends final finally float for goto if implements import instanceof int interface long native new package private protected public return short static strictfp super switch synchronized throw throws transient try void volatile while var record sealed permits yield";
const JS: &str = "break case catch class const continue debugger default delete do else export extends finally for function if import in instanceof let new return super switch throw try typeof var void while with yield async await of static get set from as";
const PYTHON: &str = "and as assert async await break class continue def del elif else except finally for from global if import in is lambda nonlocal not or pass raise return try while with yield match case print";

fn keywords(name: &str) -> (Vec<&'static str>, bool) {
    let list: Vec<&str> = match name {
        "C" => vec![C],
        "C++" => vec![C, CPP],
        "Objective-C" => vec![
            C,
            "@interface @implementation @end @property @synthesize @protocol @class @selector @encode @try @catch @finally @autoreleasepool @optional @required id instancetype nonatomic atomic strong weak copy assign readonly readwrite nullable nonnull",
        ],
        "GLSL" | "HLSL" => vec![
            C,
            "uniform in out inout attribute varying layout precision highp mediump lowp discard cbuffer register vec2 vec3 vec4 ivec2 ivec3 ivec4 mat2 mat3 mat4 sampler2D samplerCube float2 float3 float4 float3x3 float4x4 half Texture2D SamplerState",
        ],
        "C#" => vec![
            "abstract as base bool break byte case catch char checked class const continue decimal default delegate do double else enum event explicit extern finally fixed float for foreach goto if implicit in int interface internal is lock long namespace new object operator out override params private protected public readonly ref return sbyte sealed short sizeof stackalloc static string struct switch throw try typeof uint ulong unchecked unsafe ushort using var virtual void volatile while async await yield record get set init when where nameof dynamic",
        ],
        "Java" => vec![JAVA],
        "Groovy" => vec![JAVA, "def as in trait"],
        "Kotlin" => vec![
            "as break class continue do else for fun if in interface is object package return super throw try typealias typeof val var when while by catch constructor finally get import init set where abstract annotation companion const data enum external final infix inline inner internal lateinit open operator out override private protected public reified sealed suspend tailrec vararg",
        ],
        "Swift" => vec![
            "associatedtype class deinit enum extension fileprivate func import init inout internal let open operator private protocol public rethrows static struct subscript typealias var break case continue default defer do else fallthrough for guard if in repeat return switch where while as catch is super throw throws try async await actor some any convenience dynamic final lazy mutating nonmutating optional override required weak unowned",
        ],
        "Go" => vec![
            "break case chan const continue default defer else fallthrough for func go goto if import interface map package range return select struct switch type var",
        ],
        "Rust" => vec![
            "as async await break const continue crate dyn else enum extern fn for if impl in let loop match mod move mut pub ref return static struct super trait type unsafe use where while",
        ],
        "JavaScript" | "Vue" | "Svelte" => vec![JS],
        "TypeScript" => vec![
            JS,
            "abstract any boolean declare enum implements interface keyof namespace never number object private protected public readonly string symbol type unknown is infer satisfies override module",
        ],
        "Python" | "Starlark" => vec![PYTHON],
        "Ruby" => vec![
            "alias and begin break case class def defined? do else elsif end ensure for if in module next not or redo rescue retry return super then undef unless until when while yield require require_relative include extend attr_reader attr_accessor attr_writer private protected public raise lambda proc",
        ],
        "PHP" => vec![
            "abstract and array as break callable case catch class clone const continue declare default do echo else elseif empty enddeclare endfor endforeach endif endswitch endwhile enum extends final finally fn for foreach function global goto if implements include include_once instanceof insteadof interface isset list match namespace new or print private protected public readonly require require_once return static switch throw trait try unset use var while xor yield",
        ],
        "Shell" => vec![
            "if then else elif fi case esac for while until do done in function select return break continue local export readonly declare unset shift exit source alias trap eval exec",
        ],
        "Fish" => vec!["function end if else switch case for in while begin and or not return set"],
        "Perl" => vec![
            "my our local sub package use require if elsif else unless while until for foreach do last next redo return and or not eq ne lt gt le ge print",
        ],
        "R" => vec!["if else repeat while function for in next break return library require"],
        "Julia" => vec![
            "abstract baremodule begin break catch const continue do else elseif end export finally for function global if import let local macro module mutable primitive quote return struct try type using while where in isa",
        ],
        "Lua" => {
            vec!["and break do else elseif end for function goto if in local not or repeat return then until while"]
        }
        "Scala" => vec![
            "abstract case catch class def do else enum export extends extension final finally for forSome given if implicit import lazy match new object override package private protected return sealed super then throw trait try type using val var while with yield",
        ],
        "Dart" => vec![
            "abstract as assert async await base break case catch class const continue covariant default deferred do dynamic else enum export extends extension external factory final finally for get hide if implements import in interface is late library mixin new on operator part required rethrow return sealed set show static super switch sync throw try typedef var void when while with yield",
        ],
        "Haskell" | "Idris" => vec![
            "case class data default deriving do else foreign if import in infix infixl infixr instance let module newtype of then type where qualified as hiding forall record interface implementation namespace total partial mutual",
        ],
        "Elm" => vec!["if then else case of let in type alias module exposing import as port"],
        "OCaml" => vec![
            "and as assert begin class constraint do done downto else end exception external for fun function functor if in include inherit initializer lazy let match method module mutable new nonrec object of open or private rec sig struct then to try type val virtual when while with",
        ],
        "F#" => vec![
            "abstract and as assert base begin class default delegate do done downcast downto elif else end exception extern finally for fun function global if in inherit inline interface internal lazy let match member module mutable namespace new not of open or override private public rec return sig static struct then to try type upcast use val void when while with yield async task",
        ],
        "ReScript" => vec![
            "and as assert constraint else exception external for if in include lazy let module mutable of open private rec switch try type when while",
        ],
        "Gleam" => vec!["as assert case const external fn if import let opaque pub todo type use panic"],
        "Elixir" => vec![
            "def defp defmodule defmacro defmacrop defprotocol defimpl defstruct defguard defdelegate defexception do end fn if else unless case cond with for receive try catch rescue after raise when and or not in import alias require use quote unquote",
        ],
        "Erlang" => vec![
            "after and andalso band begin bnot bor bsl bsr bxor case catch cond div end fun if let not of or orelse receive rem try when xor",
        ],
        "Clojure" => vec![
            "def defn defn- defmacro defmulti defmethod defprotocol defrecord deftype let letfn fn if if-let if-not do loop recur when when-let when-not cond condp case ns require import try catch finally throw quote var doseq dotimes for",
        ],
        "Common Lisp" | "Scheme" | "Emacs Lisp" => vec![
            "defun defmacro defvar defparameter defconst defcustom defgeneric defmethod defclass defstruct let let* flet labels lambda if cond when unless progn prog1 setq setf loop dolist dotimes define define-syntax begin set! and or not quote case",
        ],
        "Ada" => vec![
            "abort abs abstract accept access aliased all and array at begin body case constant declare delay delta digits do else elsif end entry exception exit for function generic goto if in interface is limited loop mod new not null of or others out overriding package pragma private procedure protected raise range record rem renames requeue return reverse select separate some subtype synchronized tagged task terminate then type until use when while with xor",
        ],
        "Visual Basic" => vec![
            "and as boolean byref byte byval call case catch class const dim do double each else elseif end enum exit finally nothing for friend function get handles if implements imports in inherits integer interface is let long loop me module mybase new next not of on option or overrides private property protected public raise readonly return select set shared short single static step string structure sub then throw to try typeof until using when while with",
        ],
        "PowerShell" => vec![
            "begin break catch class continue data define do dynamicparam else elseif end enum exit filter finally for foreach from function if in param process return switch throw trap try until using var while workflow",
        ],
        "MATLAB" => vec![
            "break case catch classdef continue else elseif end for function global if otherwise parfor persistent return spmd switch try while properties methods events",
        ],
        "SQL" => vec![
            "select from where insert into values update set delete create table view index drop alter add column primary key foreign references not null unique default check and or in is like between join left right inner outer full cross on as order by group having limit offset union all distinct case when then else end exists function procedure trigger returns return begin declare language replace if grant revoke with recursive asc desc integer int text varchar boolean timestamp serial",
        ],
        "Zig" => vec![
            "addrspace align allowzero and anyframe anytype asm async await break callconv catch comptime const continue defer else enum errdefer error export extern fn for if inline linksection noalias noinline nosuspend opaque or orelse packed pub resume return struct suspend switch test threadlocal try union unreachable usingnamespace var volatile while",
        ],
        "D" => vec![
            C,
            "abstract alias align asm assert body break cast catch class const debug delegate delete deprecated final finally foreach foreach_reverse function immutable import in inout interface invariant is lazy mixin module new nothrow out override package pragma private protected public pure real ref scope shared super synchronized template throw try typeid typeof ubyte uint ulong unittest ushort version wchar dchar with string",
        ],
        "Solidity" => vec![
            "contract library interface function modifier event struct enum mapping address bool string bytes uint int uint256 int256 bytes32 public private internal external view pure payable returns return if else for while do break continue emit require revert assert constructor memory storage calldata import pragma using is new delete",
        ],
        "Protocol Buffers" | "Thrift" | "Cap'n Proto" => vec![
            "syntax package import option message enum service rpc returns repeated optional required oneof map reserved extend struct union exception typedef namespace include const stream",
        ],
        "GraphQL" => vec![
            "type input interface enum union scalar schema query mutation subscription fragment on directive extend implements",
        ],
        "Makefile" => vec!["ifeq ifneq ifdef ifndef else endif include define endef export override"],
        "CMake" => vec![
            "if else elseif endif foreach endforeach while endwhile function endfunction macro endmacro set option return",
        ],
        "Dockerfile" => vec![
            "FROM RUN CMD LABEL EXPOSE ENV ADD COPY ENTRYPOINT VOLUME USER WORKDIR ARG ONBUILD STOPSIGNAL HEALTHCHECK SHELL AS",
        ],
        "Nix" => vec!["let in with rec inherit if then else assert import"],
        "HCL" => vec!["resource data variable output module provider locals terraform for in if"],
        "Meson" => vec!["if elif else endif foreach endforeach and or not"],
        "Vim script" => vec![
            "function endfunction let if else elseif endif for endfor while endwhile return call execute set map noremap nnoremap inoremap command augroup autocmd def enddef",
        ],
        "AWK" => vec![
            "BEGIN END function if else while for do break continue next exit return getline print printf in delete",
        ],
        "Batch" => vec!["echo set if else goto call exit for in do not exist defined errorlevel setlocal endlocal"],
        "Rego" => vec!["package import default not some every in if contains else with as"],
        "Ninja" => vec!["rule build pool default include subninja"],
        _ => vec![],
    };
    let any_case = matches!(
        name,
        "Ada" | "Visual Basic" | "PowerShell" | "SQL" | "CMake" | "Batch" | "Dockerfile"
    );
    (
        list.iter().flat_map(|words| words.split_whitespace()).collect(),
        any_case,
    )
}

const LITERALS: &[&str] = &[
    "true",
    "false",
    "null",
    "nil",
    "None",
    "True",
    "False",
    "undefined",
    "NaN",
    "Infinity",
    "YES",
    "NO",
    "nullptr",
    "NULL",
];
const BUILTIN_TYPES: &[&str] = &[
    "int",
    "uint",
    "float",
    "double",
    "bool",
    "boolean",
    "char",
    "byte",
    "short",
    "long",
    "void",
    "str",
    "string",
    "bytes",
    "dict",
    "tuple",
    "frozenset",
    "object",
    "number",
    "bigint",
    "symbol",
    "any",
    "unknown",
    "never",
    "decimal",
    "sbyte",
    "ushort",
    "ulong",
    "i8",
    "i16",
    "i32",
    "i64",
    "i128",
    "isize",
    "u8",
    "u16",
    "u32",
    "u64",
    "u128",
    "usize",
    "f32",
    "f64",
    "rune",
    "float32",
    "float64",
    "int8",
    "int16",
    "int32",
    "int64",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "uintptr",
    "complex64",
    "complex128",
    "size_t",
    "ssize_t",
    "wchar_t",
    "unsigned",
    "signed",
];
const SELVES: &[&str] = &["self", "this", "Self", "cls", "super", "me", "Me"];
const FUNCTION_WORDS: &[&str] = &[
    "def",
    "fn",
    "func",
    "function",
    "fun",
    "sub",
    "proc",
    "macro",
    "method",
    "defn",
    "defun",
    "defp",
    "procedure",
    "Sub",
    "Function",
];
const TYPE_WORDS: &[&str] = &[
    "class",
    "struct",
    "enum",
    "trait",
    "interface",
    "impl",
    "type",
    "protocol",
    "extension",
    "record",
    "object",
    "module",
    "namespace",
    "union",
    "data",
    "newtype",
    "defmodule",
    "typealias",
    "typedef",
    "extends",
    "implements",
    "new",
    "contract",
    "message",
    "service",
    "mixin",
    "actor",
];

pub fn kinds(text: &str, lang: Lang, until: usize) -> Vec<Option<Kind>> {
    let mut out = vec![None; text.len()];
    let (keywords, any_case) = keywords(lang.name);
    let is_keyword = |word: &str| {
        if any_case {
            keywords.iter().any(|k| k.eq_ignore_ascii_case(word))
        } else {
            keywords.contains(&word)
        }
    };
    let markup = matches!(lang.name, "HTML" | "Vue" | "Svelte");
    let sigils = matches!(
        lang.name,
        "Shell" | "Fish" | "PHP" | "Perl" | "PowerShell" | "Makefile" | "Batch"
    );
    let capital_variables = matches!(lang.name, "Erlang" | "Prolog");
    let cases = !any_case && !matches!(lang.name, "Makefile" | "just" | "Assembly" | "Batch");
    let lisp = matches!(lang.name, "Clojure" | "Common Lisp" | "Scheme" | "Emacs Lisp");
    let equations = matches!(lang.name, "Haskell" | "Elm" | "Idris" | "PureScript");
    let shell = matches!(
        lang.name,
        "Shell" | "Fish" | "Batch" | "PowerShell" | "Makefile" | "Dockerfile" | "just"
    );
    let js = matches!(lang.name, "JavaScript" | "TypeScript" | "Vue" | "Svelte");
    let bytes = text.as_bytes();
    let next_mark = |mut at: usize| {
        while at < bytes.len() && (bytes[at] == b' ' || bytes[at] == b'\t') {
            at += 1;
        }
        bytes.get(at).copied()
    };
    let previous_mark = |at: usize| text[..at].trim_end_matches([' ', '\t']).bytes().last();
    let mut last_word: Option<&str> = None;
    let mut last_end = 0;
    for (range, class) in spans(text, lang.name, until) {
        let kind = match class {
            Class::Comment => Some(Kind::Comment),
            Class::String => Some(Kind::String),
            Class::Number => Some(Kind::Number),
            Class::Word => {
                let word = &text[range.clone()];
                let after = last_word.filter(|_| text[last_end..range.start].trim().is_empty());
                let next = next_mark(range.end);
                let before = previous_mark(range.start);
                let line_start = text[..range.start].rfind('\n').map_or(0, |n| n + 1);
                let at_column_0 = line_start == range.start;
                let command_start = {
                    let before_text = text[line_start..range.start].trim_end();
                    before_text.is_empty()
                        || before_text.ends_with(['|', '&', ';', '(', '{', '`'])
                        || ["then", "do", "else", "sudo", "exec"]
                            .iter()
                            .any(|w| before_text.ends_with(w))
                };
                let rest_of_line = text[range.end..].split('\n').next().unwrap_or("");
                let kind = if LITERALS.contains(&word) {
                    Some(Kind::Number)
                } else if SELVES.contains(&word) {
                    Some(Kind::Variable)
                } else if BUILTIN_TYPES.contains(&word) && next != Some(b'(') && !shell {
                    Some(Kind::Type)
                } else if is_keyword(word) {
                    Some(Kind::Keyword)
                } else if shell && command_start && next != Some(b'=') {
                    Some(Kind::Function)
                } else if shell && {
                    let dashes = text[..range.start].trim_end_matches('-');
                    dashes.len() < range.start && dashes.ends_with([' ', '\t'])
                } {
                    Some(Kind::Number)
                } else if equations && at_column_0 && word.starts_with(char::is_lowercase) {
                    Some(Kind::Function)
                } else if equations
                    && (next == Some(b'.') || before == Some(b'.'))
                    && word.starts_with(char::is_uppercase)
                {
                    Some(Kind::Type)
                } else if js && matches!(after, Some("const" | "let" | "var")) && next == Some(b'=') && {
                    let value = rest_of_line.trim_start().trim_start_matches('=').trim_start();
                    value.starts_with("function")
                        || value.starts_with("async")
                        || (value.starts_with('(') && rest_of_line.contains("=>"))
                        || value.split_whitespace().nth(1) == Some("=>")
                } {
                    Some(Kind::Function)
                } else if word.starts_with('$') {
                    sigils.then_some(Kind::Variable)
                } else if word.starts_with('@') {
                    Some(if matches!(lang.name, "Ruby" | "Elixir" | "CoffeeScript") {
                        Kind::Property
                    } else {
                        Kind::Function
                    })
                } else if markup && matches!(before, Some(b'<' | b'/')) {
                    Some(Kind::Property)
                } else if after.is_some_and(|w| FUNCTION_WORDS.contains(&w)) {
                    Some(Kind::Function)
                } else if after.is_some_and(|w| TYPE_WORDS.contains(&w)) {
                    Some(Kind::Type)
                } else if (lisp && before == Some(b'('))
                    || (!lisp && next == Some(b'('))
                    || (word.ends_with('!') && matches!(next, Some(b'(' | b'[' | b'{')))
                {
                    Some(Kind::Function)
                } else if !shell && (matches!(before, Some(b'.')) || text[..range.start].ends_with("->")) {
                    Some(Kind::Property)
                } else if capital_variables && word.starts_with(|c: char| c.is_uppercase() || c == '_') {
                    Some(Kind::Variable)
                } else if cases
                    && word.len() > 1
                    && word.chars().any(char::is_alphabetic)
                    && !word.chars().any(char::is_lowercase)
                    && word.chars().next().is_some_and(char::is_uppercase)
                {
                    Some(Kind::Number)
                } else if cases && word.starts_with(char::is_uppercase) {
                    Some(Kind::Type)
                } else {
                    None
                };
                last_word = Some(word);
                last_end = range.end;
                kind
            }
        };
        if kind.is_some() {
            out[range].fill(kind);
        }
    }
    out
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    pub text: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hit: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
}

pub fn parts(text: &str, line: Range<usize>, kinds: &[Option<Kind>], terms: &[String]) -> Vec<Part> {
    let source = &text[line.clone()];
    let hits: Vec<Range<usize>> = if terms.is_empty() {
        Vec::new()
    } else {
        words::occurrences(source, terms).into_iter().map(|(_, r)| r).collect()
    };
    let is_hit = |at: usize| hits.iter().any(|range| range.contains(&at));
    let mut out: Vec<Part> = Vec::new();
    for (at, c) in source.char_indices() {
        let kind = kinds.get(line.start + at).copied().flatten();
        let hit = is_hit(at);
        match out.last_mut() {
            Some(last) if last.kind == kind && last.hit == hit => last.text.push(c),
            _ => out.push(Part {
                text: c.to_string(),
                hit,
                kind,
            }),
        }
    }
    if out.is_empty() {
        out.push(Part {
            text: String::new(),
            hit: false,
            kind: None,
        });
    }
    out
}

pub fn lines(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    for (at, _) in text.match_indices('\n') {
        out.push(start..at);
        start = at + 1;
    }
    out.push(start..text.len());
    out
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CodeFile {
    pub language: String,
    pub lines: Vec<Vec<Part>>,
    pub total: usize,
}

const MOST_LINES: usize = 20_000;

pub fn code_file(path: &Path, query: &str) -> Result<CodeFile, String> {
    let text = crate::embed::code::text_of(path)?;
    let lang = crate::embed::code::language(path, &text).ok_or("it isn't code Mesa reads")?;
    let all = lines(&text);
    let total = all.len();
    let kinds = kinds(&text, lang, all.get(MOST_LINES).map_or(text.len(), |line| line.start));
    let terms = words::terms(query);
    let lines = all
        .into_iter()
        .take(MOST_LINES)
        .map(|line| parts(&text, line, &kinds, &terms))
        .collect();
    Ok(CodeFile {
        language: lang.name.to_owned(),
        lines,
        total,
    })
}

const PEEK_LINES: usize = 16;

pub fn peek(path: &Path, line: Option<u64>) -> Result<Vec<Vec<Part>>, String> {
    let text = crate::embed::code::text_of(path)?;
    let lang = crate::embed::code::language(path, &text).ok_or("it isn't code Mesa reads")?;
    let all = lines(&text);
    let first = match line {
        Some(line) => (line as usize).saturating_sub(1).min(all.len().saturating_sub(1)),
        None => all
            .iter()
            .position(|range| !text[range.clone()].trim().is_empty())
            .unwrap_or(0),
    };
    let shown: Vec<Range<usize>> = all.into_iter().skip(first).take(PEEK_LINES).collect();
    let indent = shown
        .iter()
        .map(|range| &text[range.clone()])
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    let kinds = kinds(&text, lang, shown.last().map_or(0, |line| line.end));
    Ok(shown
        .into_iter()
        .map(|range| {
            let from = (range.start + indent).min(range.end);
            let from = if text.is_char_boundary(from) { from } else { range.start };
            parts(&text, from..range.end, &kinds, &[])
        })
        .collect())
}
