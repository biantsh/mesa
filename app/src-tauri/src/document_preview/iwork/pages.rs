use std::{collections::HashMap, fmt::Write as _};

use iwork::{ElementInfo, drawable::Kind};

use super::{data_names, hex, picture};
use crate::{
    document_preview::opendocument::{Picture, emu, escape, relationships, twips, zip_of},
    iwork_read::{Book, Look, Paragraph},
};

const PICTURES: [Picture; 7] = [
    Picture::Png,
    Picture::Jpeg,
    Picture::Gif,
    Picture::Bmp,
    Picture::Svg,
    Picture::Emf,
    Picture::Wmf,
];
const MAIN: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture""#;

pub(in crate::document_preview) enum Made {
    Word(Vec<u8>),
    Pages(Vec<u8>),
}

pub(in crate::document_preview) fn office(bytes: &[u8], blocks: Option<usize>) -> Option<Made> {
    let book = Book::from_bytes(bytes).ok()?;
    let structure = book.doc.structure()?;
    if structure.mode == iwork::pages::Mode::PageLayout {
        return page_layout(&book, &structure, blocks.is_some()).map(Made::Pages);
    }
    docx(&book, &structure, blocks).map(Made::Word)
}

fn docx(book: &Book, structure: &iwork::pages::Structure, blocks: Option<usize>) -> Option<Vec<u8>> {
    let body = structure.body_storage.or_else(|| book.doc.body_storage())?;
    let elements = book.doc.elements();
    let by_id: HashMap<u64, &ElementInfo> = elements
        .iter()
        .map(|element| (element.identifier.get(), element))
        .collect();
    let tables: HashMap<u64, iwork::TableInfo> = book
        .doc
        .tables()
        .into_iter()
        .map(|table| (table.identifier.get(), table))
        .collect();
    let setup = &structure.setup;
    let text_width = (f64::from(setup.width) - f64::from(setup.left_margin) - f64::from(setup.right_margin)).max(72.0);
    let mut writer = Writer {
        book,
        names: data_names(book),
        by_id,
        tables,
        text_width,
        body: String::new(),
        relationships: Vec::new(),
        media: Vec::new(),
        numbers: HashMap::new(),
        pictures: 0,
    };

    let paragraphs = book.paragraphs(body);
    for (n, paragraph) in paragraphs.iter().enumerate() {
        if blocks.is_some_and(|most| n >= most) {
            break;
        }
        writer.paragraph(paragraph, n > 0);
    }

    let mut references = String::new();
    let mut headers = Vec::new();
    for footer in [false, true] {
        let shown = if footer {
            setup.footers_shown
        } else {
            setup.headers_shown
        };
        let zones = zones_of(structure, footer, None);
        if !shown || zones.iter().all(|zone| zone.text.trim().is_empty()) {
            continue;
        }
        let mut runs = String::new();
        for (n, zone) in zones.iter().enumerate() {
            if n > 0 {
                runs.push_str("<w:r><w:tab/></w:r>");
            }
            let look = book
                .paragraphs(zone.storage)
                .first()
                .and_then(|paragraph| paragraph.runs.first().map(|run| book.look(paragraph.style, run.style)));
            let rpr = look.as_deref().map(rpr).unwrap_or_default();
            for (at, piece) in zone.text.split('\u{FFFC}').enumerate() {
                if at > 0 {
                    let _ = write!(
                        runs,
                        r#"<w:r><w:rPr>{rpr}</w:rPr><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:rPr>{rpr}</w:rPr><w:instrText xml:space="preserve"> PAGE </w:instrText></w:r><w:r><w:rPr>{rpr}</w:rPr><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:rPr>{rpr}</w:rPr><w:t>1</w:t></w:r><w:r><w:rPr>{rpr}</w:rPr><w:fldChar w:fldCharType="end"/></w:r>"#
                    );
                }
                if !piece.is_empty() {
                    let _ = write!(
                        runs,
                        r#"<w:r><w:rPr>{rpr}</w:rPr><w:t xml:space="preserve">{}</w:t></w:r>"#,
                        escape(piece)
                    );
                }
            }
        }
        let (kind, root) = if footer { ("footer", "ftr") } else { ("header", "hdr") };
        let name = format!("{kind}1.xml");
        let tabs = format!(
            r#"<w:tabs><w:tab w:val="center" w:pos="{}"/><w:tab w:val="right" w:pos="{}"/></w:tabs>"#,
            twips(text_width / 2.0),
            twips(text_width)
        );
        headers.push((name.clone(), format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:{root} {MAIN}><w:p><w:pPr>{tabs}</w:pPr>{runs}</w:p></w:{root}>"#)));
        let id = writer.relate(kind, name);
        let _ = write!(references, r#"<w:{kind}Reference w:type="default" r:id="{id}"/>"#);
    }
    let orientation = if setup.width > setup.height {
        r#" w:orient="landscape""#
    } else {
        ""
    };
    let settings = format!(
        r#"<w:sectPr>{references}<w:pgSz w:w="{}" w:h="{}"{orientation}/><w:pgMar w:top="{}" w:right="{}" w:bottom="{}" w:left="{}" w:header="{}" w:footer="{}" w:gutter="0"/></w:sectPr>"#,
        twips(f64::from(setup.width)),
        twips(f64::from(setup.height)),
        twips(f64::from(setup.top_margin)),
        twips(f64::from(setup.right_margin)),
        twips(f64::from(setup.bottom_margin)),
        twips(f64::from(setup.left_margin)),
        twips(f64::from(setup.header_margin)),
        twips(f64::from(setup.footer_margin)),
    );
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {MAIN}><w:body>{}{settings}</w:body></w:document>"#,
        writer.body
    );
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles {MAIN}><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Helvetica Neue" w:hAnsi="Helvetica Neue" w:cs="Helvetica Neue"/><w:sz w:val="22"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:before="0" w:after="0" w:line="240" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults></w:styles>"#
    );
    writer.relate("styles", "styles.xml".into());
    let mut parts: Vec<(String, Vec<u8>)> = vec![
        (
            "_rels/.rels".into(),
            relationships(&[("rId1".into(), "officeDocument", "word/document.xml".into())]),
        ),
        ("word/document.xml".into(), document.into_bytes()),
        ("word/styles.xml".into(), styles.into_bytes()),
    ];
    let mut overrides = vec![
        (
            "word/document.xml".to_owned(),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
        ),
        (
            "word/styles.xml".to_owned(),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml",
        ),
    ];
    for (name, xml) in headers {
        overrides.push((
            format!("word/{name}"),
            if name.starts_with("header") {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"
            } else {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"
            },
        ));
        parts.push((format!("word/{name}"), xml.into_bytes()));
    }
    parts.push((
        "word/_rels/document.xml.rels".into(),
        relationships(&writer.relationships),
    ));
    for (name, bytes) in std::mem::take(&mut writer.media) {
        parts.push((format!("word/{name}"), bytes));
    }
    zip_of(&parts, &overrides)
}

struct Writer<'b> {
    book: &'b Book,
    names: HashMap<u64, String>,
    by_id: HashMap<u64, &'b ElementInfo>,
    tables: HashMap<u64, iwork::TableInfo>,
    text_width: f64,
    body: String,
    relationships: Vec<(String, &'static str, String)>,
    media: Vec<(String, Vec<u8>)>,
    numbers: HashMap<(u64, u64), u32>,
    pictures: u32,
}

impl Writer<'_> {
    fn relate(&mut self, kind: &'static str, target: String) -> String {
        let id = format!("rId{}", self.relationships.len() + 1);
        self.relationships.push((id.clone(), kind, target));
        id
    }

    fn paragraph(&mut self, paragraph: &Paragraph, after: bool) {
        let look = self.book.paragraph_look(paragraph.style);
        let mut ppr = String::new();
        if look.keep_with_next {
            ppr.push_str("<w:keepNext/>");
        }
        if after && (paragraph.break_before || look.page_break_before) {
            ppr.push_str("<w:pageBreakBefore/>");
        }
        if let Some(fill) = look.background {
            let _ = write!(ppr, r#"<w:shd w:val="clear" w:color="auto" w:fill="{}"/>"#, hex(fill));
        }
        let label = match self.book.list_label(paragraph.list, paragraph.level) {
            Some(bullet) if bullet.is_empty() => None,
            Some(bullet) => Some(bullet),
            None => {
                let list = paragraph.list.unwrap_or(0);
                self.numbers
                    .retain(|(other, level), _| *other != list || *level <= paragraph.level);
                let number = self.numbers.entry((list, paragraph.level)).or_insert(0);
                *number += 1;
                Some(format!("{number}."))
            }
        };
        let (left, first) = match &label {
            Some(_) => {
                let indent = self
                    .book
                    .list_indent(paragraph.list, paragraph.level)
                    .unwrap_or(paragraph.level as f64 * 18.0);
                let left = look.left.max(indent) + 18.0;
                (left, left - 18.0)
            }
            None => (look.left, look.first_line),
        };
        if label.is_some() {
            let _ = write!(ppr, r#"<w:tabs><w:tab w:val="left" w:pos="{}"/></w:tabs>"#, twips(left));
        }
        let line = look.line_spacing.map_or_else(String::new, |spacing| {
            format!(r#" w:line="{}" w:lineRule="auto""#, (240.0 * spacing).round())
        });
        let _ = write!(
            ppr,
            r#"<w:spacing w:before="{}" w:after="{}"{line}/>"#,
            twips(look.space_before.max(0.0)),
            twips(look.space_after.max(0.0))
        );
        let indent = first - left;
        let first_line = if indent >= 0.0 {
            format!(r#" w:firstLine="{}""#, twips(indent))
        } else {
            format!(r#" w:hanging="{}""#, twips(-indent))
        };
        let _ = write!(
            ppr,
            r#"<w:ind w:left="{}" w:right="{}"{first_line}/>"#,
            twips(left.max(0.0)),
            twips(look.right.max(0.0))
        );
        let justified = match look.alignment {
            1 => Some("right"),
            2 => Some("center"),
            3 => Some("both"),
            _ => None,
        };
        if let Some(justified) = justified {
            let _ = write!(ppr, r#"<w:jc w:val="{justified}"/>"#);
        }
        let mark = paragraph.runs.last().map_or_else(
            || self.book.look(paragraph.style, None),
            |run| self.book.look(paragraph.style, run.style),
        );
        let _ = write!(ppr, "<w:rPr>{}</w:rPr>", rpr(&mark));

        let mut runs = String::new();
        let mut after_it = Vec::new();
        if let Some(label) = label {
            let first_look = paragraph
                .runs
                .first()
                .map_or_else(|| mark.clone(), |run| self.book.look(paragraph.style, run.style));
            let _ = write!(
                runs,
                r#"<w:r><w:rPr>{}</w:rPr><w:t xml:space="preserve">{}</w:t></w:r><w:r><w:tab/></w:r>"#,
                rpr(&first_look),
                escape(&label)
            );
        }
        for run in &paragraph.runs {
            if let Some(attached) = run.attachment {
                self.attached(attached, &mut runs, &mut after_it);
                continue;
            }
            let look = self.book.look(paragraph.style, run.style);
            let rpr = rpr(&look);
            for (n, line) in run.text.split(['\u{2028}', '\u{000B}']).enumerate() {
                if n > 0 {
                    runs.push_str("<w:r><w:br/></w:r>");
                }
                for (m, piece) in line.split('\t').enumerate() {
                    if m > 0 {
                        runs.push_str("<w:r><w:tab/></w:r>");
                    }
                    if !piece.is_empty() {
                        let _ = write!(
                            runs,
                            r#"<w:r><w:rPr>{rpr}</w:rPr><w:t xml:space="preserve">{}</w:t></w:r>"#,
                            escape(piece)
                        );
                    }
                }
            }
        }
        let _ = write!(self.body, "<w:p><w:pPr>{ppr}</w:pPr>{runs}</w:p>");
        for block in after_it {
            self.body.push_str(&block);
        }
    }

    fn attached(&mut self, id: u64, runs: &mut String, after: &mut Vec<String>) {
        let Some(element) = self.by_id.get(&id).copied() else {
            return;
        };
        match element.kind {
            Kind::Image => {
                let Some((bytes, format)) = element
                    .media
                    .as_ref()
                    .and_then(|media| media.data)
                    .and_then(|data| picture(self.book, &self.names, data, &PICTURES))
                else {
                    return;
                };
                let (mut width, mut height) = (f64::from(element.geometry.width), f64::from(element.geometry.height));
                if width <= 0.0 || height <= 0.0 {
                    return;
                }
                if width > self.text_width {
                    height *= self.text_width / width;
                    width = self.text_width;
                }
                self.pictures += 1;
                let number = self.pictures;
                let name = format!("media/image{number}.{}", format.extension());
                self.media.push((name.clone(), bytes));
                let embed = self.relate("image", name);
                let (cx, cy) = (emu(width), emu(height));
                let _ = write!(
                    runs,
                    r#"<w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{number}" name="Picture {number}"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="{number}" name="Picture {number}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="{embed}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#
                );
            }
            Kind::Table => {
                if let Some(table) = self.tables.get(&id) {
                    after.push(self.table(table));
                }
            }
            Kind::Shape => {
                if let Some(storage) = element.text {
                    let body = std::mem::take(&mut self.body);
                    for paragraph in self.book.paragraphs(storage) {
                        self.paragraph(&paragraph, false);
                    }
                    after.push(std::mem::replace(&mut self.body, body));
                }
            }
            _ => {}
        }
    }

    fn table(&self, table: &iwork::TableInfo) -> String {
        let (rows, columns) = (table.rows.min(2_000), table.columns.min(50));
        if rows == 0 || columns == 0 {
            return String::new();
        }
        let mut widths: Vec<f64> = (0..columns).map(|column| table.column_width(column).max(9.0)).collect();
        let total: f64 = widths.iter().sum();
        if total > self.text_width {
            widths.iter_mut().for_each(|width| *width *= self.text_width / total);
        }
        let mut xml = format!(
            r#"<w:tbl><w:tblPr><w:tblW w:w="{}" w:type="dxa"/><w:tblLayout w:type="fixed"/><w:tblBorders><w:top w:val="single" w:sz="4" w:color="A0A0A0"/><w:left w:val="single" w:sz="4" w:color="A0A0A0"/><w:bottom w:val="single" w:sz="4" w:color="A0A0A0"/><w:right w:val="single" w:sz="4" w:color="A0A0A0"/><w:insideH w:val="single" w:sz="4" w:color="A0A0A0"/><w:insideV w:val="single" w:sz="4" w:color="A0A0A0"/></w:tblBorders><w:tblCellMar><w:left w:w="80" w:type="dxa"/><w:right w:w="80" w:type="dxa"/></w:tblCellMar></w:tblPr><w:tblGrid>"#,
            twips(widths.iter().sum())
        );
        for width in &widths {
            let _ = write!(xml, r#"<w:gridCol w:w="{}"/>"#, twips(*width));
        }
        xml.push_str("</w:tblGrid>");
        for row in 0..rows {
            xml.push_str("<w:tr>");
            if row < table.header_rows as usize {
                xml.push_str("<w:trPr><w:tblHeader/></w:trPr>");
            }
            for (column, width) in widths.iter().enumerate() {
                let shown = table
                    .cell(row, column)
                    .map_or_else(String::new, |cell| crate::iwork_read::shown(&cell.value, &cell.format));
                let bold = if row < table.header_rows as usize || column < table.header_columns as usize {
                    "<w:b/>"
                } else {
                    ""
                };
                let _ = write!(
                    xml,
                    r#"<w:tc><w:tcPr><w:tcW w:w="{}" w:type="dxa"/></w:tcPr><w:p><w:r><w:rPr>{bold}<w:sz w:val="20"/></w:rPr><w:t xml:space="preserve">{}</w:t></w:r></w:p></w:tc>"#,
                    twips(*width),
                    escape(&shown)
                );
            }
            xml.push_str("</w:tr>");
        }
        xml.push_str("</w:tbl><w:p/>");
        xml
    }
}

fn rpr(look: &Look) -> String {
    let mut rpr = String::new();
    if let Some(family) = &look.family {
        let _ = write!(
            rpr,
            r#"<w:rFonts w:ascii="{0}" w:hAnsi="{0}" w:eastAsia="{0}" w:cs="{0}"/>"#,
            escape(family)
        );
    }
    if look.bold {
        rpr.push_str("<w:b/><w:bCs/>");
    }
    if look.italic {
        rpr.push_str("<w:i/><w:iCs/>");
    }
    match look.caps {
        1 => rpr.push_str("<w:caps/>"),
        2 => rpr.push_str("<w:smallCaps/>"),
        _ => {}
    }
    if look.strike {
        rpr.push_str("<w:strike/>");
    }
    if let Some(color) = look.color {
        let _ = write!(rpr, r#"<w:color w:val="{}"/>"#, hex(color));
    }
    if let Some(size) = look.size {
        let half = (size * 2.0).round().max(2.0) as i64;
        let _ = write!(rpr, r#"<w:sz w:val="{half}"/><w:szCs w:val="{half}"/>"#);
    }
    if look.underline {
        rpr.push_str(r#"<w:u w:val="single"/>"#);
    }
    if let Some(fill) = look.background {
        let _ = write!(rpr, r#"<w:shd w:val="clear" w:color="auto" w:fill="{}"/>"#, hex(fill));
    }
    match look.raised {
        1 => rpr.push_str(r#"<w:vertAlign w:val="superscript"/>"#),
        -1 => rpr.push_str(r#"<w:vertAlign w:val="subscript"/>"#),
        _ => {}
    }
    rpr
}

fn zones_of(
    structure: &iwork::pages::Structure,
    footer: bool,
    template: Option<iwork::pages::TemplatePage>,
) -> Vec<&iwork::pages::HeaderFooter> {
    let of = |kind: iwork::pages::TemplatePage| -> Vec<&iwork::pages::HeaderFooter> {
        structure
            .header_footers
            .iter()
            .filter(|part| part.footer == footer && part.section == 0 && part.template == kind)
            .collect()
    };
    let said = |zones: &Vec<&iwork::pages::HeaderFooter>| {
        zones.iter().map(|zone| zone.text.trim().chars().count()).sum::<usize>()
    };
    match template {
        Some(kind) => of(kind),
        None => [
            iwork::pages::TemplatePage::Odd,
            iwork::pages::TemplatePage::First,
            iwork::pages::TemplatePage::Even,
        ]
        .into_iter()
        .map(of)
        .max_by_key(|zones| said(zones))
        .unwrap_or_default(),
    }
}

fn listed(book: &Book, id: u64, field: u32) -> Vec<u64> {
    let Some((_, object)) = book.doc.object(id) else {
        return Vec::new();
    };
    let Ok(archive) = iwork::pb::Message::decode(object.payload()) else {
        return Vec::new();
    };
    archive
        .all(field)
        .filter_map(|value| {
            if let iwork::pb::Value::Bytes(bytes) = value {
                iwork::pb::decode_nested(bytes)?.varint(1)
            } else {
                None
            }
        })
        .collect()
}

fn page_layout(book: &Book, structure: &iwork::pages::Structure, first: bool) -> Option<Vec<u8>> {
    let setup = &structure.setup;
    let (width, height) = (f64::from(setup.width), f64::from(setup.height));
    let mut pages: Vec<Vec<u64>> = Vec::new();
    for (_, object) in book.doc.objects().filter(|(_, object)| object.message_type() == 10010) {
        let Ok(archive) = iwork::pb::Message::decode(object.payload()) else {
            continue;
        };
        for group in archive.all(1).filter_map(|value| {
            if let iwork::pb::Value::Bytes(bytes) = value {
                iwork::pb::decode_nested(bytes)
            } else {
                None
            }
        }) {
            let page = group.varint(1).unwrap_or(0) as usize;
            if page > 2_000 {
                continue;
            }
            if pages.len() <= page {
                pages.resize(page + 1, Vec::new());
            }
            pages[page].extend(group.all(4).filter_map(|value| {
                let iwork::pb::Value::Bytes(bytes) = value else {
                    return None;
                };
                let entry = iwork::pb::decode_nested(bytes)?;
                entry
                    .bytes(1)
                    .and_then(iwork::pb::decode_nested)
                    .and_then(|reference| reference.varint(1))
                    .or_else(|| entry.varint(1))
            }));
        }
    }
    if pages.is_empty() {
        pages.push(Vec::new());
    }
    if first {
        pages.truncate(1);
    }
    let template_of = |kind: iwork::pages::TemplatePage| {
        structure
            .header_footers
            .iter()
            .find(|part| part.section == 0 && part.template == kind)
            .map(|part| part.section_template)
    };
    let first_different = structure
        .sections
        .first()
        .is_some_and(|section| section.first_page_different);
    for (n, page) in pages.iter_mut().enumerate() {
        let kind = if n == 0 && first_different {
            iwork::pages::TemplatePage::First
        } else {
            iwork::pages::TemplatePage::Odd
        };
        if let Some(template) = template_of(kind).or_else(|| template_of(iwork::pages::TemplatePage::Odd)) {
            let mut behind = listed(book, template, 3);
            behind.append(page);
            *page = behind;
        }
    }

    let elements = book.doc.elements();
    let by_id: HashMap<u64, &ElementInfo> = elements
        .iter()
        .map(|element| (element.identifier.get(), element))
        .collect();
    let mut texts: HashMap<u64, Vec<crate::iwork_read::Paragraph>> = HashMap::new();
    for thread in &structure.threads {
        let Some(storage) = thread.storage else { continue };
        let mut paragraphs = book.paragraphs(storage).into_iter().peekable();
        for (n, id) in thread.boxes.iter().enumerate() {
            let Some(element) = by_id.get(id) else { continue };
            let (room_wide, room_high) = (
                f64::from(element.geometry.width) - 8.0,
                f64::from(element.geometry.height) - 8.0,
            );
            let mut shown = Vec::new();
            let mut used = 0.0;
            let last = n + 1 == thread.boxes.len();
            while let Some(paragraph) = paragraphs.peek() {
                let tall = super::keynote::height_of(book, paragraph, room_wide, 1.0);
                if !last && !shown.is_empty() && used + tall > room_high {
                    break;
                }
                used += tall;
                shown.extend(paragraphs.next());
            }
            texts.insert(*id, shown);
        }
    }

    let (left, right) = (f64::from(setup.left_margin), f64::from(setup.right_margin));
    let third = (width - left - right) / 3.0;
    let over: Vec<String> = (0..pages.len())
        .map(|n| {
            let mut xml = String::new();
            for footer in [false, true] {
                let shown = if footer { setup.footers_shown } else { setup.headers_shown };
                if !shown {
                    continue;
                }
                let kind = if n == 0 && first_different { iwork::pages::TemplatePage::First } else { iwork::pages::TemplatePage::Odd };
                let zones = zones_of(structure, footer, Some(kind));
                let top = if footer { height - f64::from(setup.footer_margin) - 18.0 } else { f64::from(setup.header_margin) };
                for (at, zone) in zones.iter().enumerate() {
                    let text = zone.text.replace('\u{FFFC}', &(n + 1).to_string());
                    if text.trim().is_empty() {
                        continue;
                    }
                    let look = book.paragraphs(zone.storage).first().and_then(|paragraph| paragraph.runs.first().map(|run| book.look(paragraph.style, run.style)));
                    let look = look.unwrap_or_default();
                    let algn = ["l", "ctr", "r"][at.min(2)];
                    let id = 9000 + 10 * n + 3 * usize::from(footer) + at;
                    let _ = write!(
                        xml,
                        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Zone {id}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap="square" lIns="0" tIns="0" rIns="0" bIns="0" anchor="t"><a:noAutofit/></a:bodyPr><a:lstStyle/><a:p><a:pPr algn="{algn}"/><a:r><a:rPr{}>{}</a:rPr><a:t>{}</a:t></a:r></a:p></p:txBody></p:sp>"#,
                        emu(left + third * at as f64),
                        emu(top),
                        emu(third),
                        emu(18.0),
                        super::keynote::attributes(&look, 1.0),
                        super::keynote::children(&look),
                        escape(text.trim())
                    );
                }
            }
            xml
        })
        .collect();
    super::keynote::pages_as_slides(book, width, height, &pages, texts, &over)
}
