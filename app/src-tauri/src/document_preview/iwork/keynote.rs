use std::{collections::HashMap, fmt::Write as _};

use iwork::{ElementInfo, Fill, drawable::Kind};

use super::{bytes_of, data_names, hex, picture};
use crate::{
    document_preview::opendocument::{Picture, emu, escape, relationships, zip_of},
    iwork_read::{Book, Look, Paragraph},
};

const PICTURES: [Picture; 6] = [
    Picture::Png,
    Picture::Jpeg,
    Picture::Gif,
    Picture::Bmp,
    Picture::Tiff,
    Picture::Svg,
];
const NAMESPACES: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;
const INSET: f64 = 4.0;

pub(in super::super) fn pptx(bytes: &[u8], only: Option<usize>) -> Option<Vec<u8>> {
    let book = Book::from_bytes(bytes).ok()?;
    let show = book.doc.show()?;
    let mut slides = book.doc.slides();
    slides.sort_by_key(|slide| slide.index);
    let elements = book.doc.elements();
    let mut writer = Writer::new(&book, &elements);
    let mut xml = Vec::new();
    for (n, slide) in slides.iter().enumerate() {
        if only.is_none_or(|only| only == n + 1) {
            xml.push(writer.slide(slide));
        }
    }
    writer.finish(xml, f64::from(show.width), f64::from(show.height))
}

pub(super) fn pages_as_slides(
    book: &Book,
    width: f64,
    height: f64,
    pages: &[Vec<u64>],
    texts: HashMap<u64, Vec<Paragraph>>,
    over: &[String],
) -> Option<Vec<u8>> {
    let elements = book.doc.elements();
    let mut writer = Writer::new(book, &elements);
    writer.texts = texts;
    let mut xml = Vec::new();
    for (n, page) in pages.iter().enumerate() {
        let on: Vec<&ElementInfo> = page.iter().filter_map(|id| writer.by_id.get(id).copied()).collect();
        xml.push(writer.page(&on, over.get(n).map_or("", String::as_str)));
    }
    writer.finish(xml, width, height)
}

type Slide = (String, Vec<(String, &'static str, String)>);

struct Writer<'b> {
    book: &'b Book,
    names: HashMap<u64, String>,
    by_parent: HashMap<u64, Vec<&'b ElementInfo>>,
    by_id: HashMap<u64, &'b ElementInfo>,
    tables: HashMap<u64, iwork::TableInfo>,
    media: Vec<(String, Vec<u8>)>,
    relationships: Vec<(String, &'static str, String)>,
    ids: u32,
    texts: HashMap<u64, Vec<Paragraph>>,
}

impl<'b> Writer<'b> {
    fn new(book: &'b Book, elements: &'b [ElementInfo]) -> Self {
        let mut by_parent: HashMap<u64, Vec<&ElementInfo>> = HashMap::new();
        for element in elements {
            if let Some(parent) = element.parent {
                by_parent.entry(parent).or_default().push(element);
            }
        }
        let tables = book
            .doc
            .tables()
            .into_iter()
            .map(|table| (table.identifier.get(), table))
            .collect();
        let by_id = elements
            .iter()
            .map(|element| (element.identifier.get(), element))
            .collect();
        Writer {
            book,
            names: data_names(book),
            by_parent,
            by_id,
            tables,
            media: Vec::new(),
            relationships: Vec::new(),
            ids: 1,
            texts: HashMap::new(),
        }
    }

    fn finish(mut self, slides: Vec<Slide>, width: f64, height: f64) -> Option<Vec<u8>> {
        let mut parts: Vec<(String, Vec<u8>)> = vec![(
            "_rels/.rels".into(),
            relationships(&[("rId1".into(), "officeDocument", "ppt/presentation.xml".into())]),
        )];
        let mut overrides = vec![(
            "ppt/presentation.xml".to_owned(),
            "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml",
        )];
        let (mut listed, mut slide_relationships) = (String::new(), Vec::new());
        for (n, (xml, own)) in slides.into_iter().enumerate() {
            let number = n + 1;
            parts.push((format!("ppt/slides/slide{number}.xml"), xml.into_bytes()));
            parts.push((format!("ppt/slides/_rels/slide{number}.xml.rels"), relationships(&own)));
            overrides.push((
                format!("ppt/slides/slide{number}.xml"),
                "application/vnd.openxmlformats-officedocument.presentationml.slide+xml",
            ));
            slide_relationships.push((format!("rId{number}"), "slide", format!("slides/slide{number}.xml")));
            let _ = write!(listed, r#"<p:sldId id="{}" r:id="rId{number}"/>"#, 255 + number);
        }
        let (width, height) = (width.max(1.0), height.max(1.0));
        parts.push((
            "ppt/presentation.xml".into(),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:presentation {NAMESPACES}><p:sldIdLst>{listed}</p:sldIdLst><p:sldSz cx="{}" cy="{}"/><p:notesSz cx="{}" cy="{}"/></p:presentation>"#,
                emu(width),
                emu(height),
                emu(height),
                emu(width)
            )
            .into_bytes(),
        ));
        parts.push((
            "ppt/_rels/presentation.xml.rels".into(),
            relationships(&slide_relationships),
        ));
        for (name, bytes) in std::mem::take(&mut self.media) {
            parts.push((format!("ppt/{name}"), bytes));
        }
        zip_of(&parts, &overrides)
    }

    fn page(&mut self, on: &[&ElementInfo], over: &str) -> Slide {
        self.ids = 1;
        let mut shapes = String::new();
        for element in on {
            self.element(element, &mut shapes);
        }
        shapes.push_str(over);
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sld {NAMESPACES}><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{shapes}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#
        );
        (xml, std::mem::take(&mut self.relationships))
    }
    fn slide(&mut self, slide: &iwork::SlideInfo) -> Slide {
        self.ids = 1;
        let mut shapes = String::new();
        if let Some(layout) = slide.layout {
            for element in self.ordered(layout) {
                if element.kind != Kind::Placeholder {
                    self.element(element, &mut shapes);
                }
            }
        }
        for element in self.ordered(slide.identifier) {
            self.element(element, &mut shapes);
        }
        let background = self
            .style_of(slide.identifier)
            .and_then(|style| self.book.doc.object_style(style))
            .and_then(|style| style.fill)
            .and_then(|fill| self.fill(&fill))
            .map_or_else(String::new, |fill| {
                format!("<p:bg><p:bgPr>{fill}<a:effectLst/></p:bgPr></p:bg>")
            });
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sld {NAMESPACES}><p:cSld>{background}<p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{shapes}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#
        );
        (xml, std::mem::take(&mut self.relationships))
    }

    fn archive(&self, id: u64) -> Option<iwork::pb::Message> {
        let (_, object) = self.book.doc.object(id)?;
        iwork::pb::Message::decode(object.payload()).ok()
    }

    fn style_of(&self, slide: u64) -> Option<u64> {
        self.archive(slide)?
            .bytes(1)
            .and_then(iwork::pb::decode_nested)?
            .varint(1)
    }

    fn ordered(&self, container: u64) -> Vec<&'b ElementInfo> {
        let on: Vec<&'b ElementInfo> = self.by_parent.get(&container).cloned().unwrap_or_default();
        let order: Vec<u64> = self
            .archive(container)
            .map(|archive| {
                archive
                    .all(42)
                    .filter_map(|value| {
                        if let iwork::pb::Value::Bytes(bytes) = value {
                            iwork::pb::decode_nested(bytes)?.varint(1)
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut ordered: Vec<&'b ElementInfo> = order
            .iter()
            .filter_map(|id| on.iter().find(|element| element.identifier.get() == *id).copied())
            .collect();
        for element in on {
            if !ordered
                .iter()
                .any(|known| known.identifier.get() == element.identifier.get())
            {
                ordered.push(element);
            }
        }
        ordered
    }

    fn id(&mut self) -> u32 {
        self.ids += 1;
        self.ids
    }

    fn relate(&mut self, bytes: Vec<u8>, format: Picture) -> String {
        let name = format!("media/image{}.{}", self.media.len() + 1, format.extension());
        self.media.push((name.clone(), bytes));
        let id = format!("rId{}", self.relationships.len() + 1);
        self.relationships.push((id.clone(), "image", format!("../{name}")));
        id
    }

    fn fill(&mut self, fill: &Fill) -> Option<String> {
        Some(match fill {
            Fill::None => "<a:noFill/>".into(),
            Fill::Color(color) => format!(
                r#"<a:solidFill><a:srgbClr val="{}"/></a:solidFill>"#,
                hex(bytes_of(color))
            ),
            Fill::Gradient(gradient) => {
                let mut stops = String::new();
                for (color, at) in &gradient.stops {
                    let _ = write!(
                        stops,
                        r#"<a:gs pos="{}"><a:srgbClr val="{}"/></a:gs>"#,
                        (at.clamp(0.0, 1.0) * 100_000.0).round(),
                        hex(bytes_of(color))
                    );
                }
                let angle = (-f64::from(gradient.angle)).rem_euclid(360.0);
                format!(
                    r#"<a:gradFill rotWithShape="1"><a:gsLst>{stops}</a:gsLst><a:lin ang="{}" scaled="0"/></a:gradFill>"#,
                    (angle * 60_000.0).round() as i64
                )
            }
            Fill::Image(image) => {
                let (bytes, format) = match &image.source {
                    iwork::drawable::ImageSource::Stored(data) => picture(self.book, &self.names, *data, &PICTURES)?,
                    iwork::drawable::ImageSource::Bytes { bytes, .. } => {
                        let format = Picture::of(bytes)?;
                        (bytes.clone(), format)
                    }
                };
                let embed = self.relate(bytes, format);
                format!(
                    r#"<a:blipFill dpi="0" rotWithShape="1"><a:blip r:embed="{embed}"/><a:srcRect/><a:stretch><a:fillRect/></a:stretch></a:blipFill>"#
                )
            }
        })
    }

    fn element(&mut self, element: &ElementInfo, out: &mut String) {
        self.element_at(element, (0.0, 0.0), out);
    }

    fn element_at(&mut self, element: &ElementInfo, offset: (f64, f64), out: &mut String) {
        let geometry = &element.geometry;
        let place = Place {
            x: f64::from(geometry.x) + offset.0,
            y: f64::from(geometry.y) + offset.1,
            width: f64::from(geometry.width),
            height: f64::from(geometry.height),
            angle: f64::from(geometry.angle),
        };
        match element.kind {
            Kind::Group => {
                for inner in self
                    .by_parent
                    .get(&element.identifier.get())
                    .cloned()
                    .unwrap_or_default()
                {
                    self.element_at(inner, (place.x, place.y), out);
                }
            }
            Kind::Image => self.image(element, place, out),
            Kind::Table => self.table(element, place, out),
            Kind::Shape | Kind::Placeholder => {
                let paragraphs = match self.texts.get(&element.identifier.get()) {
                    Some(shown) => shown.clone(),
                    None => element
                        .text
                        .map(|storage| self.book.paragraphs(storage))
                        .unwrap_or_default(),
                };
                let has_text = paragraphs.iter().any(|paragraph| !paragraph.text().trim().is_empty());
                if element.kind == Kind::Placeholder && !has_text {
                    return;
                }
                if place.width <= 0.0 && place.height <= 0.0 {
                    return;
                }
                let style = element.style.and_then(|style| self.book.doc.object_style(style));
                let fill = style
                    .as_ref()
                    .and_then(|style| style.fill.as_ref())
                    .and_then(|fill| self.fill(fill))
                    .unwrap_or_else(|| "<a:noFill/>".into());
                let line = style.as_ref().and_then(|style| style.stroke.as_ref()).map_or_else(
                    || "<a:ln><a:noFill/></a:ln>".to_owned(),
                    |stroke| match &stroke.color {
                        Some(color)
                            if stroke.width > 0.0 && stroke.pattern != iwork::drawable::StrokePattern::Empty =>
                        {
                            format!(
                                r#"<a:ln w="{}"><a:solidFill><a:srgbClr val="{}"/></a:solidFill></a:ln>"#,
                                emu(f64::from(stroke.width)),
                                hex(bytes_of(color))
                            )
                        }
                        _ => "<a:ln><a:noFill/></a:ln>".into(),
                    },
                );
                if !has_text && fill.contains("noFill") && line.contains("noFill") {
                    return;
                }
                let id = self.id();
                let layout = self.text_layout(element.style);
                let [left, top, right, bottom] = layout.insets;
                let scale = if layout.shrinks {
                    self.fitting(&paragraphs, place.width - left - right, place.height - top - bottom)
                } else {
                    1.0
                };
                let text = self.paragraphs(&paragraphs, scale);
                let anchor = match layout.vertical {
                    1 => "ctr",
                    2 => "b",
                    _ => "t",
                };
                let _ = write!(
                    out,
                    r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Shape {id}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>{fill}{line}</p:spPr><p:txBody><a:bodyPr wrap="square" lIns="{}" tIns="{}" rIns="{}" bIns="{}" anchor="{anchor}" rtlCol="0"><a:noAutofit/></a:bodyPr><a:lstStyle/>{text}</p:txBody></p:sp>"#,
                    place.xfrm(),
                    emu(left),
                    emu(top),
                    emu(right),
                    emu(bottom),
                );
            }
            _ => {}
        }
    }

    fn image(&mut self, element: &ElementInfo, mut place: Place, out: &mut String) {
        let Some(data) = element.media.as_ref().and_then(|media| media.data) else {
            return;
        };
        let Some((bytes, format)) = picture(self.book, &self.names, data, &PICTURES) else {
            return;
        };
        let mut crop = String::from("<a:srcRect/>");
        if let Some(mask) = element.mask().and_then(|mask| self.by_id.get(&mask).copied()) {
            let window = &mask.geometry;
            let (width, height) = (place.width.max(1.0), place.height.max(1.0));
            let (left, top) = (f64::from(window.x) / width, f64::from(window.y) / height);
            let (right, bottom) = (
                1.0 - left - f64::from(window.width) / width,
                1.0 - top - f64::from(window.height) / height,
            );
            let share = |fraction: f64| (fraction.clamp(-1.0, 1.0) * 100_000.0).round() as i64;
            crop = format!(
                r#"<a:srcRect l="{}" t="{}" r="{}" b="{}"/>"#,
                share(left),
                share(top),
                share(right),
                share(bottom)
            );
            place = Place {
                x: place.x + f64::from(window.x),
                y: place.y + f64::from(window.y),
                width: f64::from(window.width),
                height: f64::from(window.height),
                ..place
            };
        }
        let id = self.id();
        let embed = self.relate(bytes, format);
        let _ = write!(
            out,
            r#"<p:pic><p:nvPicPr><p:cNvPr id="{id}" name="Picture {id}"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="{embed}"/>{crop}<a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr>{}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>"#,
            place.xfrm()
        );
    }

    fn table(&mut self, element: &ElementInfo, place: Place, out: &mut String) {
        let id = self.id();
        let Some(table) = self.tables.get(&element.identifier.get()) else {
            return;
        };
        let (rows, columns) = (table.rows.min(200), table.columns.min(50));
        if rows == 0 || columns == 0 {
            return;
        }
        let mut xml = String::from("<a:tbl><a:tblPr/><a:tblGrid>");
        for column in 0..columns {
            let _ = write!(xml, r#"<a:gridCol w="{}"/>"#, emu(table.column_width(column).max(9.0)));
        }
        xml.push_str("</a:tblGrid>");
        let edges: String = ["L", "R", "T", "B"]
            .iter()
            .map(|side| {
                format!(r#"<a:ln{side} w="9525"><a:solidFill><a:srgbClr val="A0A0A0"/></a:solidFill></a:ln{side}>"#)
            })
            .collect();
        for row in 0..rows {
            let _ = write!(xml, r#"<a:tr h="{}">"#, emu(table.row_height(row).max(9.0)));
            for column in 0..columns {
                let shown = table
                    .cell(row, column)
                    .map_or_else(String::new, |cell| crate::iwork_read::shown(&cell.value, &cell.format));
                let bold = row < table.header_rows as usize || column < table.header_columns as usize;
                let _ = write!(
                    xml,
                    r#"<a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US" sz="1000"{}/><a:t>{}</a:t></a:r></a:p></a:txBody><a:tcPr marL="45720" marR="45720" marT="22860" marB="22860">{edges}</a:tcPr></a:tc>"#,
                    if bold { r#" b="1""# } else { "" },
                    escape(&shown)
                );
            }
            xml.push_str("</a:tr>");
        }
        xml.push_str("</a:tbl>");
        let _ = write!(
            out,
            r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="{id}" name="Table {id}"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>{}<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table">{xml}</a:graphicData></a:graphic></p:graphicFrame>"#,
            place.xfrm().replace("a:xfrm", "p:xfrm")
        );
    }

    fn text_layout(&self, style: Option<u64>) -> ResolvedLayout {
        let mut layout = TextLayout {
            insets: [INSET; 4],
            vertical: None,
            shrinks: None,
            insets_said: false,
        };
        let mut next = style;
        for _ in 0..12 {
            let Some(archive) = next.and_then(|id| self.archive(id)) else {
                break;
            };
            if let Some(properties) = archive.bytes(11).and_then(iwork::pb::decode_nested) {
                if layout.vertical.is_none() {
                    layout.vertical = properties.varint(2);
                }
                if layout.shrinks.is_none() {
                    layout.shrinks = properties.varint(1).map(|shrinks| shrinks != 0);
                }
                if !layout.insets_said
                    && let Some(padding) = properties.bytes(6).and_then(iwork::pb::decode_nested)
                {
                    for (side, field) in [(0, 1), (1, 2), (2, 3), (3, 4)] {
                        if let Some(iwork::pb::Value::Fixed32(bytes)) = padding.get(field) {
                            layout.insets[side] = f64::from(f32::from_le_bytes(*bytes)).clamp(0.0, 200.0);
                        }
                    }
                    layout.insets_said = true;
                }
            }
            next = iwork::style::get_path(&archive, &[1, 1, 3, 1])
                .or_else(|| iwork::style::get_path(&archive, &[1, 3, 1]))
                .and_then(|value| {
                    if let iwork::pb::Value::Varint(parent) = value {
                        Some(parent)
                    } else {
                        None
                    }
                });
        }
        TextLayout {
            vertical: Some(layout.vertical.unwrap_or(0)),
            shrinks: Some(layout.shrinks.unwrap_or(false)),
            ..layout
        }
        .into()
    }

    fn fitting(&self, paragraphs: &[Paragraph], width: f64, height: f64) -> f64 {
        fitting(self.book, paragraphs, width, height)
    }
}

pub(super) fn fitting(book: &Book, paragraphs: &[Paragraph], width: f64, height: f64) -> f64 {
    if width <= 0.0 || height <= 0.0 {
        return 1.0;
    }
    let mut scale = 1.0;
    while scale > 0.3
        && paragraphs
            .iter()
            .map(|paragraph| height_of(book, paragraph, width, scale))
            .sum::<f64>()
            > height
    {
        scale -= 0.05;
    }
    scale
}

pub(super) fn height_of(book: &Book, paragraph: &Paragraph, width: f64, scale: f64) -> f64 {
    let look = book.paragraph_look(paragraph.style);
    let first = paragraph.runs.first().map(|run| book.look(paragraph.style, run.style));
    let size = first.as_ref().and_then(|look| look.size).unwrap_or(18.0) * scale;
    let text = paragraph.text();
    let wide_as = match first.as_ref() {
        Some(look) if look.caps == 1 || (!text.is_empty() && text.chars().all(|c| !c.is_lowercase())) => 0.66,
        Some(look) if look.bold => 0.56,
        _ => 0.5,
    };
    let lines = (text.chars().count() as f64 * size * wide_as / width.max(1.0))
        .ceil()
        .max(1.0);
    lines * size * 1.2 * look.line_spacing.unwrap_or(1.0) + (look.space_before + look.space_after) * scale
}

impl Writer<'_> {
    fn paragraphs(&self, paragraphs: &[Paragraph], scale: f64) -> String {
        let mut out = String::new();
        let mut numbers: HashMap<(u64, u64), u32> = HashMap::new();
        for paragraph in paragraphs {
            let look = self.book.paragraph_look(paragraph.style);
            let algn = match look.alignment {
                1 => Some("r"),
                2 => Some("ctr"),
                3 => Some("just"),
                _ => None,
            };
            let label = self.book.list_label(paragraph.list, paragraph.level);
            let listed = label.as_deref() != Some("");
            let indent_step = self
                .book
                .list_indent(paragraph.list, paragraph.level)
                .unwrap_or(paragraph.level as f64 * 36.0);
            let (left, first) = if listed {
                (look.left.max(indent_step) + 20.0, -20.0)
            } else {
                (look.left, look.first_line - look.left)
            };
            let mut ppr = format!(r#"<a:pPr marL="{}" indent="{}""#, emu(left.max(0.0)), emu(first));
            if let Some(algn) = algn {
                let _ = write!(ppr, r#" algn="{algn}""#);
            }
            ppr.push('>');
            if let Some(spacing) = look.line_spacing {
                let _ = write!(
                    ppr,
                    r#"<a:lnSpc><a:spcPct val="{}"/></a:lnSpc>"#,
                    (spacing * 100_000.0).round()
                );
            }
            let _ = write!(
                ppr,
                r#"<a:spcBef><a:spcPts val="{}"/></a:spcBef><a:spcAft><a:spcPts val="{}"/></a:spcAft>"#,
                (look.space_before.max(0.0) * 100.0).round(),
                (look.space_after.max(0.0) * 100.0).round()
            );
            match &label {
                Some(bullet) if !bullet.is_empty() => {
                    let _ = write!(ppr, r#"<a:buChar char="{}"/>"#, escape(bullet));
                }
                None => {
                    let number = numbers
                        .entry((paragraph.list.unwrap_or(0), paragraph.level))
                        .or_insert(0);
                    *number += 1;
                    let _ = write!(ppr, r#"<a:buAutoNum type="arabicPeriod" startAt="{number}"/>"#);
                }
                _ => ppr.push_str("<a:buNone/>"),
            }
            ppr.push_str("</a:pPr>");
            let mut runs = String::new();
            let mut last = None;
            for run in &paragraph.runs {
                let look = self.book.look(paragraph.style, run.style);
                if !run.text.is_empty() {
                    let _ = write!(
                        runs,
                        "<a:r><a:rPr{}>{}</a:rPr><a:t>{}</a:t></a:r>",
                        attributes(&look, scale),
                        children(&look),
                        escape(&run.text.replace('\u{2028}', "\n"))
                    );
                }
                last = Some(look);
            }
            let end = last.unwrap_or_else(|| self.book.look(paragraph.style, None));
            let _ = write!(out, "<a:p>{ppr}{runs}<a:endParaRPr{}/></a:p>", attributes(&end, scale));
        }
        if out.is_empty() {
            out.push_str("<a:p/>");
        }
        out
    }
}

pub(super) fn attributes(look: &Look, scale: f64) -> String {
    let mut attributes = format!(
        r#" lang="en-US" sz="{}""#,
        (look.size.unwrap_or(18.0) * scale * 100.0)
            .round()
            .clamp(100.0, 400_000.0)
    );
    if look.bold {
        attributes.push_str(r#" b="1""#);
    }
    if look.italic {
        attributes.push_str(r#" i="1""#);
    }
    if look.underline {
        attributes.push_str(r#" u="sng""#);
    }
    if look.strike {
        attributes.push_str(r#" strike="sngStrike""#);
    }
    match look.raised {
        1 => attributes.push_str(r#" baseline="30000""#),
        -1 => attributes.push_str(r#" baseline="-25000""#),
        _ => {}
    }
    match look.caps {
        1 => attributes.push_str(r#" cap="all""#),
        2 => attributes.push_str(r#" cap="small""#),
        _ => {}
    }
    attributes
}

pub(super) fn children(look: &Look) -> String {
    let mut children = String::new();
    if let Some(color) = look.color {
        let _ = write!(
            children,
            r#"<a:solidFill><a:srgbClr val="{}"/></a:solidFill>"#,
            hex(color)
        );
    }
    if let Some(family) = &look.family {
        let _ = write!(
            children,
            r#"<a:latin typeface="{0}"/><a:ea typeface="{0}"/><a:cs typeface="{0}"/>"#,
            escape(family)
        );
    }
    children
}

struct TextLayout {
    insets: [f64; 4],
    vertical: Option<u64>,
    shrinks: Option<bool>,
    insets_said: bool,
}

impl From<TextLayout> for ResolvedLayout {
    fn from(layout: TextLayout) -> Self {
        ResolvedLayout {
            insets: layout.insets,
            vertical: layout.vertical.unwrap_or(0),
            shrinks: layout.shrinks.unwrap_or(false),
        }
    }
}

struct ResolvedLayout {
    insets: [f64; 4],
    vertical: u64,
    shrinks: bool,
}

#[derive(Clone, Copy)]
struct Place {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    angle: f64,
}

impl Place {
    fn xfrm(&self) -> String {
        let turned = (-self.angle).rem_euclid(360.0);
        let rot = if turned.abs() > 0.01 && (360.0 - turned).abs() > 0.01 {
            format!(r#" rot="{}""#, (turned * 60_000.0).round() as i64)
        } else {
            String::new()
        };
        format!(
            r#"<a:xfrm{rot}><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></a:xfrm>"#,
            emu(self.x),
            emu(self.y),
            emu(self.width.max(0.0)),
            emu(self.height.max(0.0))
        )
    }
}
