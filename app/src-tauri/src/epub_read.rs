use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufReader, Cursor, Read, Seek},
    path::Path,
};

use roxmltree::{Document, Node, ParsingOptions};
use zip::ZipArchive;

const MAX_XML_BYTES: u64 = 32 * 1024 * 1024;

const INVISIBLE: [char; 4] = ['\u{2060}', '\u{200B}', '\u{FEFF}', '\u{00AD}'];

const FONT_OBFUSCATION: [&str; 2] = ["http://www.idpf.org/2008/embedding", "http://ns.adobe.com/pdf/enc#RC"];

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Text {
        kind: Kind,
        runs: Vec<Run>,
        label: Option<String>,
        depth: usize,
    },
    Picture(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Body,
    Heading(u8),
    Quote,
    Code,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
}

impl Block {
    pub fn text(&self) -> Option<String> {
        let Self::Text { runs, label, .. } = self else {
            return None;
        };
        let text: String = runs.iter().map(|run| run.text.as_str()).collect();
        Some(match label {
            Some(label) => format!("{label} {text}"),
            None => text,
        })
    }
}

pub struct Book<R: Read + Seek> {
    archive: ZipArchive<R>,
    pub chapters: Vec<String>,
    pub cover: Option<String>,
}

impl Book<BufReader<File>> {
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| format!("can't read the file: {e}"))?;
        Self::new(ZipArchive::new(BufReader::new(file)).map_err(|e| format!("not an EPUB book: {e}"))?)
    }
}

impl<'a> Book<Cursor<&'a [u8]>> {
    pub fn from_bytes(bytes: &'a [u8]) -> Result<Self, String> {
        Self::new(ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("not an EPUB book: {e}"))?)
    }
}

impl<R: Read + Seek> Book<R> {
    fn new(mut archive: ZipArchive<R>) -> Result<Self, String> {
        let container =
            text_of(&mut archive, "META-INF/container.xml").ok_or("not an EPUB book (it has no container)")?;
        let container = parse(&container).ok_or("not an EPUB book (its container can't be read)")?;
        let package_name = container
            .descendants()
            .find(|node| {
                node.has_tag_name("rootfile")
                    && node
                        .attribute("media-type")
                        .is_none_or(|kind| kind.contains("oebps-package"))
            })
            .and_then(|node| node.attribute("full-path"))
            .map(|name| resolve("", name))
            .ok_or("not an EPUB book (it doesn't say where its contents are)")?;
        let package = text_of(&mut archive, &package_name).ok_or("not an EPUB book (its contents' list is missing)")?;
        let package = parse(&package).ok_or("not an EPUB book (its contents' list can't be read)")?;
        let directory = package_name
            .rsplit_once('/')
            .map_or("", |(directory, _)| directory)
            .to_owned();

        let mut items: HashMap<&str, (String, &str, &str)> = HashMap::new();
        let mut in_order = Vec::new();
        for item in package.descendants().filter(|node| node.has_tag_name("item")) {
            let (Some(id), Some(href)) = (item.attribute("id"), item.attribute("href")) else {
                continue;
            };
            let name = resolve(&directory, href);
            items.insert(
                id,
                (
                    name,
                    item.attribute("media-type").unwrap_or(""),
                    item.attribute("properties").unwrap_or(""),
                ),
            );
            in_order.push(id);
        }
        let locked = locked(&mut archive);
        let mut chapters: Vec<String> = package
            .descendants()
            .filter(|node| node.has_tag_name("itemref"))
            .filter_map(|itemref| items.get(itemref.attribute("idref")?))
            .map(|(name, _, _)| name.clone())
            .collect();
        let had_chapters = !chapters.is_empty();
        chapters.retain(|name| !locked.contains(name));
        if had_chapters && chapters.is_empty() {
            return Err("the book is locked (it has DRM)".into());
        }
        if chapters.is_empty() {
            return Err("not an EPUB book (it has no chapters)".into());
        }

        let is_picture = |id: &&str| items.get(id).is_some_and(|(_, kind, _)| kind.starts_with("image/"));
        let cover = in_order
            .iter()
            .find(|id| {
                items[**id]
                    .2
                    .split_ascii_whitespace()
                    .any(|property| property == "cover-image")
            })
            .copied()
            .or_else(|| {
                package
                    .descendants()
                    .find(|node| node.has_tag_name("meta") && node.attribute("name") == Some("cover"))
                    .and_then(|meta| meta.attribute("content"))
                    .filter(is_picture)
            })
            .or_else(|| {
                in_order.iter().copied().filter(is_picture).find(|id| {
                    id.to_ascii_lowercase().contains("cover") || items[*id].0.to_ascii_lowercase().contains("cover")
                })
            })
            .map(|id| items[id].0.clone());
        Ok(Self {
            archive,
            chapters,
            cover,
        })
    }

    pub fn blocks(&mut self, name: &str) -> Vec<Block> {
        let Some(xhtml) = text_of(&mut self.archive, name) else {
            return Vec::new();
        };
        let directory = name.rsplit_once('/').map_or("", |(directory, _)| directory);
        blocks_of(&xhtml, directory)
    }

    pub fn file(&mut self, name: &str, most: u64) -> Option<Vec<u8>> {
        let file = self.archive.by_name(name).ok()?;
        if file.size() > most {
            return None;
        }
        let mut bytes = Vec::with_capacity(file.size() as usize);
        file.take(most).read_to_end(&mut bytes).ok()?;
        Some(bytes)
    }
}

fn locked<R: Read + Seek>(archive: &mut ZipArchive<R>) -> HashSet<String> {
    let Some(xml) = text_of(archive, "META-INF/encryption.xml") else {
        return HashSet::new();
    };
    let Some(document) = parse(&xml) else {
        return HashSet::new();
    };
    document
        .descendants()
        .filter(|node| node.has_tag_name("EncryptedData"))
        .filter(|data| {
            let method = data
                .descendants()
                .find(|node| node.has_tag_name("EncryptionMethod"))
                .and_then(|method| method.attribute("Algorithm"));
            !method.is_some_and(|method| FONT_OBFUSCATION.contains(&method))
        })
        .filter_map(|data| {
            data.descendants()
                .find(|node| node.has_tag_name("CipherReference"))?
                .attribute("URI")
        })
        .map(|uri| resolve("", uri))
        .collect()
}

fn text_of<R: Read + Seek>(archive: &mut ZipArchive<R>, name: &str) -> Option<String> {
    let mut bytes = Vec::new();
    archive
        .by_name(name)
        .ok()?
        .take(MAX_XML_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    let text = String::from_utf8_lossy(&bytes);
    Some(text.strip_prefix('\u{FEFF}').unwrap_or(&text).to_owned())
}

fn parse(xml: &str) -> Option<Document<'_>> {
    Document::parse_with_options(
        xml,
        ParsingOptions {
            allow_dtd: true,
            nodes_limit: 50_000_000,
            ..ParsingOptions::default()
        },
    )
    .ok()
}

pub fn resolve(directory: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or_default();
    let href = percent_encoding::percent_decode_str(href).decode_utf8_lossy();
    let mut parts: Vec<&str> = if href.starts_with('/') {
        Vec::new()
    } else {
        directory.split('/').filter(|part| !part.is_empty()).collect()
    };
    for part in href.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.join("/")
}

pub fn blocks_of(xhtml: &str, directory: &str) -> Vec<Block> {
    let xml = numbered_entities(xhtml);
    let Some(document) = parse(&xml) else {
        return loose_blocks(xhtml);
    };
    let root = document.root_element();
    let body = root
        .descendants()
        .find(|node| node.tag_name().name().eq_ignore_ascii_case("body"))
        .unwrap_or(root);
    let mut walker = Walker {
        directory,
        ..Walker::default()
    };
    walker.children(body);
    walker.flush();
    walker.blocks
}

#[derive(Default)]
struct Walker<'d> {
    directory: &'d str,
    blocks: Vec<Block>,
    runs: Vec<Run>,
    label: Option<String>,
    heading: Option<u8>,
    depth: usize,
    lists: Vec<Option<u32>>,
    bold: u32,
    italic: u32,
    quote: u32,
    pre: u32,
}

impl Walker<'_> {
    fn children(&mut self, node: Node) {
        for child in node.children() {
            if child.is_text() {
                self.text(child.text().unwrap_or_default());
            } else if child.is_element() {
                self.element(child);
            }
        }
    }

    fn element(&mut self, node: Node) {
        let name = node.tag_name().name().to_ascii_lowercase();
        match name.as_str() {
            "script" | "style" | "head" | "title" | "noscript" | "template" => {}
            "br" => self.add("\n"),
            "img" => {
                if let Some(source) = node.attribute("src") {
                    self.picture(source);
                }
            }
            "image" => {
                let href = node
                    .attributes()
                    .find(|attribute| attribute.name() == "href")
                    .map(|attribute| attribute.value());
                if let Some(href) = href {
                    self.picture(href);
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.flush();
                let outer = self.heading.replace(name.as_bytes()[1] - b'0');
                self.children(node);
                self.flush();
                self.heading = outer;
            }
            "blockquote" => self.block(node, |walker| walker.quote += 1, |walker| walker.quote -= 1),
            "pre" => self.block(node, |walker| walker.pre += 1, |walker| walker.pre -= 1),
            "ul" | "ol" => {
                let start = if name == "ol" {
                    Some(
                        node.attribute("start")
                            .and_then(|start| start.parse().ok())
                            .unwrap_or(1),
                    )
                } else {
                    None
                };
                self.block(
                    node,
                    |walker| {
                        walker.depth += 1;
                        walker.lists.push(start);
                    },
                    |walker| {
                        walker.depth -= 1;
                        walker.lists.pop();
                    },
                );
            }
            "li" => {
                self.flush();
                self.label = Some(match self.lists.last_mut() {
                    Some(Some(number)) => {
                        let label = format!("{number}.");
                        *number += 1;
                        label
                    }
                    _ => "•".to_owned(),
                });
                self.children(node);
                self.flush();
            }
            "td" | "th" => {
                if self.runs.iter().any(|run| !run.text.trim().is_empty()) {
                    self.add("\t");
                }
                if name == "th" {
                    self.bold += 1;
                    self.children(node);
                    self.bold -= 1;
                } else {
                    self.children(node);
                }
            }
            "b" | "strong" => {
                self.bold += 1;
                self.children(node);
                self.bold -= 1;
            }
            "i" | "em" | "cite" | "var" | "dfn" => {
                self.italic += 1;
                self.children(node);
                self.italic -= 1;
            }
            "p" | "div" | "section" | "article" | "header" | "footer" | "aside" | "main" | "figure" | "figcaption"
            | "dl" | "dt" | "dd" | "table" | "thead" | "tbody" | "tfoot" | "tr" | "caption" | "center" | "address"
            | "hr" | "nav" | "body" | "hgroup" | "details" | "summary" => self.block(node, |_| {}, |_| {}),
            _ => self.children(node),
        }
    }

    fn block(&mut self, node: Node, enter: impl FnOnce(&mut Self), leave: impl FnOnce(&mut Self)) {
        self.flush();
        enter(self);
        self.children(node);
        self.flush();
        leave(self);
    }

    fn picture(&mut self, href: &str) {
        if href.starts_with("data:") || href.contains("://") {
            return;
        }
        self.flush();
        self.blocks.push(Block::Picture(resolve(self.directory, href)));
    }

    fn text(&mut self, text: &str) {
        if self.pre > 0 {
            self.add(text);
            return;
        }
        let mut collapsed = String::with_capacity(text.len());
        let mut space = self.runs.last().is_none_or(|run| run.text.ends_with([' ', '\n', '\t']));
        for c in text.chars() {
            if c.is_whitespace() && c != '\u{A0}' {
                if !space {
                    collapsed.push(' ');
                }
                space = true;
            } else {
                collapsed.push(c);
                space = false;
            }
        }
        self.add(&collapsed);
    }

    fn add(&mut self, text: &str) {
        let visible;
        let text = if text.contains(INVISIBLE) {
            visible = text.replace(INVISIBLE, "");
            visible.as_str()
        } else {
            text
        };
        if text.is_empty() {
            return;
        }
        let (bold, italic) = (self.bold > 0 || self.heading.is_some(), self.italic > 0);
        match self.runs.last_mut() {
            Some(run) if run.bold == bold && run.italic == italic => run.text.push_str(text),
            _ => self.runs.push(Run {
                text: text.to_owned(),
                bold,
                italic,
            }),
        }
    }

    fn flush(&mut self) {
        let mut runs = std::mem::take(&mut self.runs);
        let label = self.label.take();
        if self.pre == 0 {
            if let Some(last) = runs.last_mut() {
                let trimmed = last.text.trim_end_matches(' ').len();
                last.text.truncate(trimmed);
            }
            for run in &mut runs {
                if run.text.contains(" \n") || run.text.contains("\n ") {
                    run.text = run.text.replace(" \n", "\n").replace("\n ", "\n");
                }
            }
        }
        if runs.iter().all(|run| run.text.trim().is_empty()) {
            return;
        }
        let kind = match self.heading {
            Some(level) => Kind::Heading(level),
            None if self.pre > 0 => Kind::Code,
            None if self.quote > 0 => Kind::Quote,
            None => Kind::Body,
        };
        self.blocks.push(Block::Text {
            kind,
            runs,
            label,
            depth: self.depth,
        });
    }
}

fn loose_blocks(html: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut paragraph = String::new();
    let mut rest = html;
    let mut flush = |paragraph: &mut String| {
        let text = paragraph.split_whitespace().collect::<Vec<_>>().join(" ");
        if !text.is_empty() {
            blocks.push(Block::Text {
                kind: Kind::Body,
                runs: vec![Run {
                    text: decode_entities(&text),
                    ..Run::default()
                }],
                label: None,
                depth: 0,
            });
        }
        paragraph.clear();
    };
    while let Some(open) = rest.find('<') {
        paragraph.push_str(&rest[..open]);
        rest = &rest[open..];
        let close = rest.find('>').map_or(rest.len(), |close| close + 1);
        let tag = rest[1..close]
            .trim_end_matches('>')
            .trim_start_matches('/')
            .to_ascii_lowercase();
        let name: String = tag.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        rest = &rest[close..];
        match name.as_str() {
            "script" | "style" | "head" => {
                let end = format!("</{name}");
                rest = rest.to_ascii_lowercase().find(&end).map_or("", |at| &rest[at..]);
            }
            "p" | "div" | "br" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "pre"
            | "section" | "hr" => flush(&mut paragraph),
            _ => {}
        }
    }
    paragraph.push_str(rest);
    flush(&mut paragraph);
    blocks
}

fn decode_entities(text: &str) -> String {
    let numbered = numbered_entities(text);
    let mut out = String::with_capacity(numbered.len());
    let mut rest = numbered.as_str();
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';').filter(|&end| end <= 12) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let c = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|decimal| decimal.parse().ok()))
                .and_then(char::from_u32),
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn numbered_entities(xhtml: &str) -> String {
    if !xhtml.contains('&') {
        return xhtml.to_owned();
    }
    let mut out = String::with_capacity(xhtml.len());
    let mut rest = xhtml;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let name_length = rest[1..].bytes().take_while(u8::is_ascii_alphanumeric).count();
        let name = &rest[1..1 + name_length];
        let ended = rest[1 + name_length..].starts_with(';');
        if (name_length == 0 && rest[1..].starts_with('#'))
            || (ended && ["amp", "lt", "gt", "quot", "apos"].contains(&name))
        {
            out.push('&');
            rest = &rest[1..];
            continue;
        }
        if !ended {
            out.push_str("&amp;");
            rest = &rest[1..];
            continue;
        }
        if let Some((_, code)) = ENTITIES.iter().find(|(known, _)| *known == name) {
            out.push_str(&format!("&#{code};"));
        }
        rest = &rest[name_length + 2..];
    }
    out.push_str(rest);
    out
}

const ENTITIES: &[(&str, u32)] = &[
    ("nbsp", 160),
    ("iexcl", 161),
    ("cent", 162),
    ("pound", 163),
    ("curren", 164),
    ("yen", 165),
    ("brvbar", 166),
    ("sect", 167),
    ("uml", 168),
    ("copy", 169),
    ("ordf", 170),
    ("laquo", 171),
    ("not", 172),
    ("shy", 173),
    ("reg", 174),
    ("macr", 175),
    ("deg", 176),
    ("plusmn", 177),
    ("sup2", 178),
    ("sup3", 179),
    ("acute", 180),
    ("micro", 181),
    ("para", 182),
    ("middot", 183),
    ("cedil", 184),
    ("sup1", 185),
    ("ordm", 186),
    ("raquo", 187),
    ("frac14", 188),
    ("frac12", 189),
    ("frac34", 190),
    ("iquest", 191),
    ("Agrave", 192),
    ("Aacute", 193),
    ("Acirc", 194),
    ("Atilde", 195),
    ("Auml", 196),
    ("Aring", 197),
    ("AElig", 198),
    ("Ccedil", 199),
    ("Egrave", 200),
    ("Eacute", 201),
    ("Ecirc", 202),
    ("Euml", 203),
    ("Igrave", 204),
    ("Iacute", 205),
    ("Icirc", 206),
    ("Iuml", 207),
    ("ETH", 208),
    ("Ntilde", 209),
    ("Ograve", 210),
    ("Oacute", 211),
    ("Ocirc", 212),
    ("Otilde", 213),
    ("Ouml", 214),
    ("times", 215),
    ("Oslash", 216),
    ("Ugrave", 217),
    ("Uacute", 218),
    ("Ucirc", 219),
    ("Uuml", 220),
    ("Yacute", 221),
    ("THORN", 222),
    ("szlig", 223),
    ("agrave", 224),
    ("aacute", 225),
    ("acirc", 226),
    ("atilde", 227),
    ("auml", 228),
    ("aring", 229),
    ("aelig", 230),
    ("ccedil", 231),
    ("egrave", 232),
    ("eacute", 233),
    ("ecirc", 234),
    ("euml", 235),
    ("igrave", 236),
    ("iacute", 237),
    ("icirc", 238),
    ("iuml", 239),
    ("eth", 240),
    ("ntilde", 241),
    ("ograve", 242),
    ("oacute", 243),
    ("ocirc", 244),
    ("otilde", 245),
    ("ouml", 246),
    ("divide", 247),
    ("oslash", 248),
    ("ugrave", 249),
    ("uacute", 250),
    ("ucirc", 251),
    ("uuml", 252),
    ("yacute", 253),
    ("thorn", 254),
    ("yuml", 255),
    ("OElig", 338),
    ("oelig", 339),
    ("Scaron", 352),
    ("scaron", 353),
    ("Yuml", 376),
    ("fnof", 402),
    ("circ", 710),
    ("tilde", 732),
    ("Alpha", 913),
    ("Beta", 914),
    ("Gamma", 915),
    ("Delta", 916),
    ("Theta", 920),
    ("Lambda", 923),
    ("Pi", 928),
    ("Sigma", 931),
    ("Phi", 934),
    ("Psi", 936),
    ("Omega", 937),
    ("alpha", 945),
    ("beta", 946),
    ("gamma", 947),
    ("delta", 948),
    ("epsilon", 949),
    ("theta", 952),
    ("lambda", 955),
    ("mu", 956),
    ("pi", 960),
    ("sigma", 963),
    ("phi", 966),
    ("omega", 969),
    ("ensp", 8194),
    ("emsp", 8195),
    ("thinsp", 8201),
    ("zwnj", 8204),
    ("zwj", 8205),
    ("lrm", 8206),
    ("rlm", 8207),
    ("ndash", 8211),
    ("mdash", 8212),
    ("lsquo", 8216),
    ("rsquo", 8217),
    ("sbquo", 8218),
    ("ldquo", 8220),
    ("rdquo", 8221),
    ("bdquo", 8222),
    ("dagger", 8224),
    ("Dagger", 8225),
    ("bull", 8226),
    ("hellip", 8230),
    ("permil", 8240),
    ("prime", 8242),
    ("Prime", 8243),
    ("lsaquo", 8249),
    ("rsaquo", 8250),
    ("oline", 8254),
    ("frasl", 8260),
    ("euro", 8364),
    ("trade", 8482),
    ("larr", 8592),
    ("uarr", 8593),
    ("rarr", 8594),
    ("darr", 8595),
    ("harr", 8596),
    ("rArr", 8658),
    ("hArr", 8660),
    ("minus", 8722),
    ("infin", 8734),
    ("ne", 8800),
    ("le", 8804),
    ("ge", 8805),
    ("asymp", 8776),
    ("loz", 9674),
    ("spades", 9824),
    ("clubs", 9827),
    ("hearts", 9829),
    ("diams", 9830),
];

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;

    use super::*;

    pub(crate) fn epub(chapters: &[(&str, &str)], extra: &[(&str, &[u8])], manifest_extra: &str) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut out);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("mimetype", options.compression_method(zip::CompressionMethod::Stored))
            .unwrap();
        zip.write_all(b"application/epub+zip").unwrap();
        zip.start_file("META-INF/container.xml", options).unwrap();
        zip.write_all(br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        let mut manifest = String::new();
        let mut spine = String::new();
        for (n, (name, _)) in chapters.iter().enumerate() {
            manifest.push_str(&format!(
                r#"<item id="c{n}" href="{name}" media-type="application/xhtml+xml"/>"#
            ));
            spine.push_str(&format!(r#"<itemref idref="c{n}"/>"#));
        }
        zip.start_file("OEBPS/content.opf", options).unwrap();
        zip.write_all(format!(r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>A Book</dc:title></metadata><manifest>{manifest}{manifest_extra}</manifest><spine>{spine}</spine></package>"#).as_bytes()).unwrap();
        for (name, xhtml) in chapters {
            zip.start_file(format!("OEBPS/{name}"), options).unwrap();
            zip.write_all(xhtml.as_bytes()).unwrap();
        }
        for (name, bytes) in extra {
            zip.start_file(*name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        out.into_inner()
    }

    pub(crate) fn chapter(body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?><!DOCTYPE html><html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title><style>p {{ color: red }}</style></head><body>{body}</body></html>"#
        )
    }

    fn texts(blocks: &[Block]) -> Vec<String> {
        blocks.iter().filter_map(Block::text).collect()
    }

    #[test]
    fn chapters_in_spine_order() {
        let bytes = epub(
            &[
                ("one.xhtml", &chapter("<p>first</p>")),
                ("Text/two.xhtml", &chapter("<p>second</p>")),
            ],
            &[("OEBPS/Images/front.jpg", b"\xFF\xD8")],
            r#"<item id="pic" href="Images/front.jpg" media-type="image/jpeg" properties="cover-image"/>"#,
        );
        let mut book = Book::from_bytes(&bytes).unwrap();
        assert_eq!(book.chapters, ["OEBPS/one.xhtml", "OEBPS/Text/two.xhtml"]);
        assert_eq!(book.cover.as_deref(), Some("OEBPS/Images/front.jpg"));
        assert_eq!(texts(&book.blocks("OEBPS/Text/two.xhtml")), ["second"]);
    }
}
