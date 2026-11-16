use std::{fmt::Write as _, io::Cursor};

use image::{ImageFormat, imageops::FilterType};

use super::opendocument::{Picture, emu, escape, relationships, twips, zip_of};
use crate::epub_read::{Block, Book, Kind, Run};

const MAIN: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture""#;
const PAGE_WIDTH: f64 = 396.0;
const PAGE_HEIGHT: f64 = 612.0;
const MARGIN: f64 = 54.0;
const TEXT_WIDTH: f64 = PAGE_WIDTH - 2.0 * MARGIN;
const TEXT_HEIGHT: f64 = PAGE_HEIGHT - 2.0 * MARGIN;
const MAX_PICTURE_BYTES: u64 = 32 * 1024 * 1024;
const PICTURES: [Picture; 4] = [Picture::Png, Picture::Jpeg, Picture::Gif, Picture::Bmp];

pub(super) fn docx(bytes: &[u8], blocks: Option<usize>) -> Option<Vec<u8>> {
    let mut book = Book::from_bytes(bytes).ok()?;
    let mut writer = Writer::default();
    let mut written = 0;
    'book: for (n, chapter) in book.chapters.clone().iter().enumerate() {
        let mut new_page = n > 0;
        for block in book.blocks(chapter) {
            if blocks.is_some_and(|most| written >= most) {
                break 'book;
            }
            match block {
                Block::Text {
                    kind,
                    runs,
                    label,
                    depth,
                } => writer.paragraph(kind, &runs, label.as_deref(), depth, new_page),
                Block::Picture(name) => {
                    let Some(bytes) = book.file(&name, MAX_PICTURE_BYTES) else {
                        continue;
                    };
                    if !writer.picture(bytes, new_page) {
                        continue;
                    }
                }
            }
            new_page = false;
            written += 1;
        }
    }
    if written == 0 {
        return None;
    }
    let settings = format!(
        r#"<w:sectPr><w:pgSz w:w="{}" w:h="{}"/><w:pgMar w:top="{m}" w:right="{m}" w:bottom="{m}" w:left="{m}" w:header="0" w:footer="0" w:gutter="0"/></w:sectPr>"#,
        twips(PAGE_WIDTH),
        twips(PAGE_HEIGHT),
        m = twips(MARGIN)
    );
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {MAIN}><w:body>{}{settings}</w:body></w:document>"#,
        writer.body
    );
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles {MAIN}><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Georgia" w:hAnsi="Georgia" w:cs="Georgia"/><w:sz w:val="21"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:before="0" w:after="120" w:line="264" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults></w:styles>"#
    );
    writer.relate("styles", "styles.xml".into());
    let mut parts: Vec<(String, Vec<u8>)> = vec![
        (
            "_rels/.rels".into(),
            relationships(&[("rId1".into(), "officeDocument", "word/document.xml".into())]),
        ),
        ("word/document.xml".into(), document.into_bytes()),
        ("word/styles.xml".into(), styles.into_bytes()),
        (
            "word/_rels/document.xml.rels".into(),
            relationships(&writer.relationships),
        ),
    ];
    for (name, bytes) in writer.media {
        parts.push((format!("word/{name}"), bytes));
    }
    let overrides = [
        (
            "word/document.xml".to_owned(),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
        ),
        (
            "word/styles.xml".to_owned(),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml",
        ),
    ];
    zip_of(&parts, &overrides)
}

pub(super) fn cover(path: &str, size: u32) -> Option<Vec<u8>> {
    let mut book = Book::open(std::path::Path::new(path)).ok()?;
    let name = book.cover.clone()?;
    let bytes = book.file(&name, MAX_PICTURE_BYTES)?;
    let picture = image::load_from_memory(&bytes).ok()?;
    let picture = if picture.width() > size || picture.height() > size {
        picture.resize(size, size, FilterType::Triangle)
    } else {
        picture
    };
    let mut png = Cursor::new(Vec::new());
    picture.write_to(&mut png, ImageFormat::Png).ok()?;
    Some(png.into_inner())
}

#[derive(Default)]
struct Writer {
    body: String,
    relationships: Vec<(String, &'static str, String)>,
    media: Vec<(String, Vec<u8>)>,
    pictures: u32,
}

impl Writer {
    fn relate(&mut self, kind: &'static str, target: String) -> String {
        let id = format!("rId{}", self.relationships.len() + 1);
        self.relationships.push((id.clone(), kind, target));
        id
    }

    fn paragraph(&mut self, kind: Kind, runs: &[Run], label: Option<&str>, depth: usize, new_page: bool) {
        let mut ppr = String::new();
        if new_page {
            ppr.push_str("<w:pageBreakBefore/>");
        }
        let (size, font, italic) = match kind {
            Kind::Heading(1) => (36, None, false),
            Kind::Heading(2) => (30, None, false),
            Kind::Heading(3) => (25, None, false),
            Kind::Heading(_) => (22, None, false),
            Kind::Quote => (21, None, true),
            Kind::Code => (17, Some("Courier New"), false),
            Kind::Body => (21, None, false),
        };
        match kind {
            Kind::Heading(level) => {
                let before = if level <= 2 { 360 } else { 240 };
                let _ = write!(ppr, r#"<w:keepNext/><w:spacing w:before="{before}" w:after="160"/>"#);
            }
            Kind::Code => ppr.push_str(r#"<w:spacing w:after="120" w:line="240" w:lineRule="auto"/>"#),
            _ => {}
        }
        let quote = if kind == Kind::Quote { 24.0 } else { 0.0 };
        match label {
            Some(_) => {
                let left = 18.0 * depth as f64 + 18.0;
                let _ = write!(
                    ppr,
                    r#"<w:tabs><w:tab w:val="left" w:pos="{}"/></w:tabs><w:ind w:left="{}" w:right="{}" w:hanging="{}"/>"#,
                    twips(left),
                    twips(left + quote),
                    twips(quote),
                    twips(18.0)
                );
            }
            None if quote > 0.0 => {
                let _ = write!(ppr, r#"<w:ind w:left="{q}" w:right="{q}"/>"#, q = twips(quote));
            }
            None => {}
        }
        let look = |bold: bool, slanted: bool| {
            let mut rpr = String::new();
            if let Some(font) = font {
                let _ = write!(rpr, r#"<w:rFonts w:ascii="{font}" w:hAnsi="{font}" w:cs="{font}"/>"#);
            }
            if bold {
                rpr.push_str("<w:b/>");
            }
            if slanted || italic {
                rpr.push_str("<w:i/>");
            }
            let _ = write!(rpr, r#"<w:sz w:val="{size}"/>"#);
            rpr
        };
        let mut text = String::new();
        if let Some(label) = label {
            let _ = write!(
                text,
                r#"<w:r><w:rPr>{}</w:rPr><w:t xml:space="preserve">{}</w:t></w:r><w:r><w:tab/></w:r>"#,
                look(false, false),
                escape(label)
            );
        }
        for run in runs {
            let rpr = look(run.bold, run.italic);
            for (n, line) in run.text.split('\n').enumerate() {
                if n > 0 {
                    text.push_str("<w:r><w:br/></w:r>");
                }
                for (m, piece) in line.split('\t').enumerate() {
                    if m > 0 {
                        text.push_str("<w:r><w:tab/></w:r>");
                    }
                    if !piece.is_empty() {
                        let _ = write!(
                            text,
                            r#"<w:r><w:rPr>{rpr}</w:rPr><w:t xml:space="preserve">{}</w:t></w:r>"#,
                            escape(piece)
                        );
                    }
                }
            }
        }
        let _ = write!(
            self.body,
            "<w:p><w:pPr>{ppr}<w:rPr>{}</w:rPr></w:pPr>{text}</w:p>",
            look(false, false)
        );
    }

    fn picture(&mut self, bytes: Vec<u8>, new_page: bool) -> bool {
        let Some(format) = Picture::of(&bytes).filter(|format| PICTURES.contains(format)) else {
            return false;
        };
        let Some((mut width, mut height)) = format
            .size_of(&bytes)
            .filter(|(width, height)| *width > 0.0 && *height > 0.0)
        else {
            return false;
        };
        let scale = (TEXT_WIDTH / width).min((TEXT_HEIGHT - 24.0) / height).min(1.0);
        width *= scale;
        height *= scale;
        self.pictures += 1;
        let number = self.pictures;
        let name = format!("media/image{number}.{}", format.extension());
        self.media.push((name.clone(), bytes));
        let embed = self.relate("image", name);
        let (cx, cy) = (emu(width), emu(height));
        let page = if new_page { "<w:pageBreakBefore/>" } else { "" };
        let _ = write!(
            self.body,
            r#"<w:p><w:pPr>{page}<w:jc w:val="center"/></w:pPr><w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{number}" name="Picture {number}"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="{number}" name="Picture {number}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="{embed}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#
        );
        true
    }
}
