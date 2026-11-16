pub(super) mod sheets;
pub(super) mod slides;
pub(super) mod text;

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fmt::Write as _,
    io::{Cursor, Read, Write},
    rc::Rc,
};

use roxmltree::{Document, Node, ParsingOptions};

const MAX_XML_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PICTURE_BYTES: u64 = 32 * 1024 * 1024;

const NAMESPACES: [(&str, &str); 12] = [
    ("office", "urn:oasis:names:tc:opendocument:xmlns:office:1.0"),
    ("style", "urn:oasis:names:tc:opendocument:xmlns:style:1.0"),
    ("text", "urn:oasis:names:tc:opendocument:xmlns:text:1.0"),
    ("table", "urn:oasis:names:tc:opendocument:xmlns:table:1.0"),
    ("draw", "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"),
    ("fo", "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0"),
    ("svg", "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0"),
    ("presentation", "urn:oasis:names:tc:opendocument:xmlns:presentation:1.0"),
    ("xlink", "http://www.w3.org/1999/xlink"),
    ("number", "urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0"),
    (
        "loext",
        "urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0",
    ),
    ("xml", "http://www.w3.org/XML/1998/namespace"),
];

fn namespace(prefix: &str) -> Option<&'static str> {
    NAMESPACES
        .iter()
        .find(|(known, _)| *known == prefix)
        .map(|(_, uri)| *uri)
}

fn prefix_of(uri: &str) -> Option<&'static str> {
    NAMESPACES
        .iter()
        .find(|(_, known)| *known == uri)
        .map(|(prefix, _)| *prefix)
}

pub(super) fn is(node: Node, name: &str) -> bool {
    let (prefix, local) = name.split_once(':').unwrap_or(("", name));
    node.is_element() && node.tag_name().name() == local && node.tag_name().namespace() == namespace(prefix)
}

pub(super) fn attr<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    let (prefix, local) = name.split_once(':')?;
    node.attribute((namespace(prefix)?, local))
}

pub(super) fn elements<'a, 'input>(node: Node<'a, 'input>) -> impl Iterator<Item = Node<'a, 'input>> {
    node.children().filter(Node::is_element)
}

pub(super) fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    elements(node).find(|child| is(*child, name))
}

pub(super) struct Package<'a> {
    archive: zip::ZipArchive<Cursor<&'a [u8]>>,
}

impl<'a> Package<'a> {
    pub fn open(bytes: &'a [u8]) -> Option<Self> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
        if crate::embed::is_encrypted(&mut archive) {
            return None;
        }
        Some(Self { archive })
    }

    pub fn text(&mut self, name: &str) -> Option<String> {
        let mut text = String::new();
        self.archive
            .by_name(name)
            .ok()?
            .take(MAX_XML_BYTES)
            .read_to_string(&mut text)
            .ok()?;
        Some(text)
    }

    pub fn picture(&mut self, href: &str) -> Option<(Vec<u8>, Picture)> {
        let name = href.strip_prefix("./").unwrap_or(href);
        if name.contains("://") || name.starts_with('/') {
            return None;
        }
        let mut bytes = Vec::new();
        self.archive
            .by_name(name)
            .ok()?
            .take(MAX_PICTURE_BYTES)
            .read_to_end(&mut bytes)
            .ok()?;
        let format = Picture::of(&bytes)?;
        Some((bytes, format))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Picture {
    Png,
    Jpeg,
    Gif,
    Bmp,
    Tiff,
    Svg,
    Emf,
    Wmf,
}

impl Picture {
    pub(in crate::document_preview) fn of(bytes: &[u8]) -> Option<Self> {
        let starts = |magic: &[u8]| bytes.starts_with(magic);
        Some(if starts(b"\x89PNG") {
            Self::Png
        } else if starts(b"\xFF\xD8") {
            Self::Jpeg
        } else if starts(b"GIF8") {
            Self::Gif
        } else if starts(b"BM") {
            Self::Bmp
        } else if starts(b"II*\0") || starts(b"MM\0*") {
            Self::Tiff
        } else if starts(b"\x01\0\0\0") && bytes.get(40..44) == Some(b" EMF") {
            Self::Emf
        } else if starts(b"\xD7\xCD\xC6\x9A") || starts(b"\x01\0\x09\0") || starts(b"\x02\0\x09\0") {
            Self::Wmf
        } else {
            let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).to_ascii_lowercase();
            if head.contains("<svg") { Self::Svg } else { return None }
        })
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Gif => "gif",
            Self::Bmp => "bmp",
            Self::Tiff => "tiff",
            Self::Svg => "svg",
            Self::Emf => "emf",
            Self::Wmf => "wmf",
        }
    }

    fn is_plain(self) -> bool {
        matches!(self, Self::Png | Self::Jpeg | Self::Gif | Self::Bmp)
    }

    pub fn size_of(self, bytes: &[u8]) -> Option<(f64, f64)> {
        if !self.is_plain() && self != Self::Tiff {
            return None;
        }
        let (width, height) = image::ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .ok()?
            .into_dimensions()
            .ok()?;
        Some((f64::from(width) * 0.75, f64::from(height) * 0.75))
    }
}

pub(super) fn frame_picture(package: &mut Package, frame: Node, allowed: &[Picture]) -> Option<(Vec<u8>, Picture)> {
    let mut found: Vec<(Vec<u8>, Picture)> = elements(frame)
        .filter(|child| is(*child, "draw:image"))
        .filter_map(|image| package.picture(attr(image, "xlink:href")?))
        .filter(|(_, format)| allowed.contains(format))
        .collect();
    let plain = found.iter().position(|(_, format)| format.is_plain()).unwrap_or(0);
    (!found.is_empty()).then(|| found.swap_remove(plain))
}

pub(super) fn parse(xml: &str) -> Option<Document<'_>> {
    Document::parse_with_options(
        xml,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: 50_000_000,
            ..ParsingOptions::default()
        },
    )
    .ok()
}

#[derive(Clone, Debug, Default)]
pub(super) struct Props(HashMap<String, String>);

impl Props {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    pub fn pt(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(pt)
    }

    fn apply(&mut self, style: Node) {
        for properties in elements(style) {
            let Some(kind) = properties.tag_name().name().strip_suffix("-properties") else {
                continue;
            };
            if properties.tag_name().namespace() != namespace("style") {
                continue;
            }
            for attribute in properties.attributes() {
                let Some(prefix) = attribute.namespace().and_then(prefix_of) else {
                    continue;
                };
                let key = format!("{kind} {prefix}:{}", attribute.name());
                let value = attribute.value();
                if let Some(percent) = value
                    .strip_suffix('%')
                    .and_then(|percent| percent.trim().parse::<f64>().ok())
                    && key.ends_with("font-size")
                {
                    let base = self.pt(&key).unwrap_or(12.0);
                    self.0.insert(key, format!("{}pt", base * percent / 100.0));
                } else {
                    self.0.insert(key, value.to_owned());
                }
            }
            if kind == "paragraph"
                && let Some(stops) = child(properties, "style:tab-stops")
            {
                let mut tabs = String::new();
                for stop in elements(stops).filter(|stop| is(*stop, "style:tab-stop")) {
                    if let Some(at) = attr(stop, "style:position").and_then(pt) {
                        let leader = match (attr(stop, "style:leader-text"), attr(stop, "style:leader-style")) {
                            (Some("."), _) | (_, Some("dotted")) => "dot",
                            (Some("-"), _) | (_, Some("dash")) => "hyphen",
                            (Some("_"), _) | (_, Some("solid")) => "underscore",
                            _ => "none",
                        };
                        let _ = write!(tabs, "{at}:{}:{leader};", attr(stop, "style:type").unwrap_or("left"));
                    }
                }
                self.0.insert("paragraph tabs".into(), tabs);
            }
        }
    }

    pub fn with(&self, other: &Props) -> Props {
        let mut both = self.clone();
        for (key, value) in &other.0 {
            both.0.insert(key.clone(), value.clone());
        }
        both
    }

    pub fn text_only(&self) -> Props {
        Props(
            self.0
                .iter()
                .filter(|(key, _)| key.starts_with("text "))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        )
    }
}

type WorkedOut = HashMap<(bool, String), HashMap<String, Rc<Props>>>;

pub(super) struct Styles<'a, 'input> {
    styles: HashMap<String, HashMap<String, Node<'a, 'input>>>,
    worked_out: RefCell<WorkedOut>,
    defaults: HashMap<String, Node<'a, 'input>>,
    pub lists: HashMap<String, Node<'a, 'input>>,
    fonts: HashMap<String, String>,
    pub layouts: HashMap<String, Node<'a, 'input>>,
    pub masters: Vec<Node<'a, 'input>>,
    pub fill_images: HashMap<String, Node<'a, 'input>>,
    pub gradients: HashMap<String, Node<'a, 'input>>,
    pub outline: Option<Node<'a, 'input>>,
    pub hidden_layers: HashSet<String>,
}

impl<'a, 'input> Styles<'a, 'input> {
    pub fn of(styles: Option<&'a Document<'input>>, content: &'a Document<'input>) -> Self {
        let mut found = Self {
            styles: HashMap::new(),
            worked_out: RefCell::new(HashMap::new()),
            defaults: HashMap::new(),
            lists: HashMap::new(),
            fonts: HashMap::new(),
            layouts: HashMap::new(),
            masters: Vec::new(),
            fill_images: HashMap::new(),
            gradients: HashMap::new(),
            outline: None,
            hidden_layers: HashSet::new(),
        };
        for document in styles.into_iter().chain([content]) {
            for node in document.root_element().descendants().filter(Node::is_element) {
                let name = attr(node, "style:name").or_else(|| attr(node, "draw:name"));
                if is(node, "style:style") {
                    if let (Some(name), Some(family)) = (name, attr(node, "style:family")) {
                        found
                            .styles
                            .entry(family.to_owned())
                            .or_default()
                            .insert(name.to_owned(), node);
                    }
                } else if is(node, "style:default-style") {
                    if let Some(family) = attr(node, "style:family") {
                        found.defaults.insert(family.to_owned(), node);
                    }
                } else if is(node, "text:list-style") {
                    if let Some(name) = name.filter(|_| {
                        node.parent()
                            .is_some_and(|parent| !is(parent, "style:graphic-properties"))
                    }) {
                        found.lists.insert(name.to_owned(), node);
                    }
                } else if is(node, "style:font-face") {
                    if let (Some(name), Some(family)) = (attr(node, "style:name"), attr(node, "svg:font-family")) {
                        found.fonts.insert(
                            name.to_owned(),
                            family.trim_matches(|c| c == '\'' || c == '"').to_owned(),
                        );
                    }
                } else if is(node, "style:page-layout") {
                    if let Some(name) = name {
                        found.layouts.insert(name.to_owned(), node);
                    }
                } else if is(node, "style:master-page") {
                    found.masters.push(node);
                } else if is(node, "draw:fill-image") {
                    if let Some(name) = name {
                        found.fill_images.insert(name.to_owned(), node);
                    }
                } else if is(node, "draw:gradient") {
                    if let Some(name) = name {
                        found.gradients.insert(name.to_owned(), node);
                    }
                } else if is(node, "text:outline-style") {
                    found.outline = Some(node);
                } else if is(node, "draw:layer")
                    && attr(node, "draw:display") == Some("none")
                    && let Some(name) = name
                {
                    found.hidden_layers.insert(name.to_owned());
                }
            }
        }
        found
    }

    pub fn style(&self, family: &str, name: &str) -> Option<Node<'a, 'input>> {
        self.styles.get(family)?.get(name).copied()
    }

    pub fn props(&self, family: &str, name: Option<&str>, defaults: bool) -> Rc<Props> {
        let key = name.unwrap_or("");
        if let Some(known) = self
            .worked_out
            .borrow()
            .get(&(defaults, family.to_owned()))
            .and_then(|styles| styles.get(key))
        {
            return Rc::clone(known);
        }
        let props = Rc::new(self.work_out(family, name, defaults));
        self.worked_out
            .borrow_mut()
            .entry((defaults, family.to_owned()))
            .or_default()
            .insert(key.to_owned(), Rc::clone(&props));
        props
    }

    fn work_out(&self, family: &str, name: Option<&str>, defaults: bool) -> Props {
        let mut chain = Vec::new();
        let mut next = name.and_then(|name| self.style(family, name));
        while let Some(style) = next {
            if chain.len() > 32 {
                break;
            }
            chain.push(style);
            next = attr(style, "style:parent-style-name").and_then(|parent| {
                self.style(family, parent).or_else(|| {
                    (family == "presentation")
                        .then(|| self.style("graphic", parent))
                        .flatten()
                })
            });
        }
        let mut props = Props::default();
        if defaults {
            let default = self.defaults.get(family).or_else(|| {
                (family == "presentation")
                    .then(|| self.defaults.get("graphic"))
                    .flatten()
            });
            if let Some(default) = default {
                props.apply(*default);
            }
        }
        for style in chain.iter().rev() {
            props.apply(*style);
        }
        props
    }

    pub fn fonts_of(&self, props: &Props) -> (Option<String>, Option<String>, Option<String>) {
        let family = |name: &str, direct: &str| {
            props
                .get(name)
                .and_then(|name| self.fonts.get(name).cloned())
                .or_else(|| {
                    props
                        .get(direct)
                        .map(|family| family.trim_matches(|c| c == '\'' || c == '"').to_owned())
                })
        };
        (
            family("text style:font-name", "text fo:font-family"),
            family("text style:font-name-asian", "text style:font-family-asian"),
            family("text style:font-name-complex", "text style:font-family-complex"),
        )
    }

    pub fn master(&self, name: Option<&str>) -> Option<Node<'a, 'input>> {
        name.and_then(|name| {
            self.masters
                .iter()
                .find(|master| attr(**master, "style:name") == Some(name))
        })
        .or(self.masters.first())
        .copied()
    }
}

pub(super) fn pt(value: &str) -> Option<f64> {
    let value = value.trim();
    let split =
        value.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E'))?;
    let (number, unit) = value.split_at(split);
    let number: f64 = number.parse().ok()?;
    let scale = match unit {
        "pt" => 1.0,
        "cm" => 72.0 / 2.54,
        "mm" => 72.0 / 25.4,
        "in" | "inch" => 72.0,
        "pc" => 12.0,
        "px" => 0.75,
        _ => return None,
    };
    let points = number * scale;
    points.is_finite().then_some(points)
}

pub(super) fn color(value: &str) -> Option<String> {
    let hex = value.trim().strip_prefix('#')?;
    let hex = match hex.len() {
        6 => hex.to_owned(),
        3 => hex.chars().flat_map(|c| [c, c]).collect(),
        _ => return None,
    };
    hex.chars()
        .all(|c| c.is_ascii_hexdigit())
        .then(|| hex.to_ascii_uppercase())
}

pub(super) fn border(value: &str) -> Option<(f64, &str, String)> {
    let mut width = None;
    let mut style = None;
    let mut found = None;
    for part in value.split_whitespace() {
        if let Some(points) = pt(part) {
            width = Some(points);
        } else if let Some(hex) = color(part) {
            found = Some(hex);
        } else {
            style = Some(part);
        }
    }
    let style = style.unwrap_or("solid");
    if style == "none" || style == "hidden" || width.is_some_and(|width| width <= 0.0) {
        return None;
    }
    Some((width.unwrap_or(0.75), style, found.unwrap_or_else(|| "000000".into())))
}

pub(super) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

pub(super) fn numeral(number: u32, format: &str) -> String {
    match format {
        "a" | "A" => {
            let mut letters = Vec::new();
            let mut n = number.max(1);
            while n > 0 {
                n -= 1;
                letters.push((b'a' + (n % 26) as u8) as char);
                n /= 26;
            }
            let label: String = letters.into_iter().rev().collect();
            if format == "A" {
                label.to_ascii_uppercase()
            } else {
                label
            }
        }
        "i" | "I" => {
            let mut n = number.clamp(1, 3999);
            let mut label = String::new();
            for (value, letters) in [
                (1000, "m"),
                (900, "cm"),
                (500, "d"),
                (400, "cd"),
                (100, "c"),
                (90, "xc"),
                (50, "l"),
                (40, "xl"),
                (10, "x"),
                (9, "ix"),
                (5, "v"),
                (4, "iv"),
                (1, "i"),
            ] {
                while n >= value {
                    label.push_str(letters);
                    n -= value;
                }
            }
            if format == "I" {
                label.to_ascii_uppercase()
            } else {
                label
            }
        }
        "" => String::new(),
        _ => number.to_string(),
    }
}

pub(super) fn bullet(bullet: &str, style: Node) -> String {
    let font = child(style, "style:text-properties")
        .and_then(|text| attr(text, "fo:font-family").or_else(|| attr(text, "style:font-name")))
        .unwrap_or("")
        .to_ascii_lowercase();
    let Some(c) = bullet.chars().next() else {
        return "•".into();
    };
    if font.contains("wingdings") {
        let shape = match c {
            'l' => '●',
            'n' => '■',
            'q' => '❑',
            'u' | 'w' => '◆',
            'v' => '❖',
            '§' => '▪',
            'Ø' => '➢',
            'ü' => '✔',
            'à' | 'è' => '➔',
            _ => '•',
        };
        return shape.to_string();
    }
    if font.contains("symbol") && !font.contains("opensymbol") && !font.contains("starsymbol") {
        return (if c == '-' { "–" } else { "•" }).into();
    }
    if ('\u{E000}'..='\u{F8FF}').contains(&c) {
        "•".into()
    } else {
        c.to_string()
    }
}

pub(super) fn zip_of(parts: &[(String, Vec<u8>)], overrides: &[(String, &str)]) -> Option<Vec<u8>> {
    let mut types = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
    );
    types.push_str(r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>"#);
    for (extension, kind) in [
        ("png", "image/png"),
        ("jpeg", "image/jpeg"),
        ("gif", "image/gif"),
        ("bmp", "image/bmp"),
        ("tiff", "image/tiff"),
        ("svg", "image/svg+xml"),
        ("emf", "image/x-emf"),
        ("wmf", "image/x-wmf"),
    ] {
        let _ = write!(types, r#"<Default Extension="{extension}" ContentType="{kind}"/>"#);
    }
    for (part, kind) in overrides {
        let _ = write!(types, r#"<Override PartName="/{part}" ContentType="{kind}"/>"#);
    }
    types.push_str("</Types>");

    let mut out = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(&mut out);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    writer.start_file("[Content_Types].xml", stored).ok()?;
    writer.write_all(types.as_bytes()).ok()?;
    for (name, bytes) in parts {
        writer.start_file(name.as_str(), stored).ok()?;
        writer.write_all(bytes).ok()?;
    }
    writer.finish().ok()?;
    Some(out.into_inner())
}

pub(super) fn relationships(relationships: &[(String, &str, String)]) -> Vec<u8> {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    for (id, kind, target) in relationships {
        let external = if *kind == "hyperlink" {
            r#" TargetMode="External""#
        } else {
            ""
        };
        let _ = write!(
            xml,
            r#"<Relationship Id="{id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}" Target="{}"{external}/>"#,
            escape(target)
        );
    }
    xml.push_str("</Relationships>");
    xml.into_bytes()
}

pub(super) fn twips(points: f64) -> i64 {
    (points * 20.0).round() as i64
}

pub(super) fn emu(points: f64) -> i64 {
    (points * 12_700.0).round() as i64
}
