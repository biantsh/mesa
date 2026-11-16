use std::fmt::Write as _;

use roxmltree::Node;

use super::{
    Package, Picture, Props, Styles, attr, child, color, elements, emu, escape, is, parse, pt, relationships, zip_of,
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
const PRESETS: [(&str, &str); 26] = [
    ("rectangle", "rect"),
    ("round-rectangle", "roundRect"),
    ("ellipse", "ellipse"),
    ("circle", "ellipse"),
    ("isosceles-triangle", "triangle"),
    ("right-triangle", "rtTriangle"),
    ("diamond", "diamond"),
    ("pentagon", "pentagon"),
    ("hexagon", "hexagon"),
    ("octagon", "octagon"),
    ("star4", "star4"),
    ("star5", "star5"),
    ("star6", "star6"),
    ("right-arrow", "rightArrow"),
    ("left-arrow", "leftArrow"),
    ("up-arrow", "upArrow"),
    ("down-arrow", "downArrow"),
    ("chevron", "chevron"),
    ("mso-spt100", "rect"),
    ("flowchart-process", "rect"),
    ("flowchart-alternate-process", "roundRect"),
    ("flowchart-decision", "diamond"),
    ("flowchart-connector", "ellipse"),
    ("round-rectangular-callout", "wedgeRoundRectCallout"),
    ("quad-bevel", "rect"),
    ("block-arc", "pie"),
];
const PLACEHOLDERS: [&str; 11] = [
    "title",
    "outline",
    "subtitle",
    "text",
    "notes",
    "date-time",
    "footer",
    "page-number",
    "header",
    "object",
    "graphic",
];

pub(in super::super) fn pptx(bytes: &[u8], only: Option<usize>) -> Option<Vec<u8>> {
    let mut package = Package::open(bytes)?;
    let content_xml = package.text("content.xml")?;
    let styles_xml = package.text("styles.xml");
    let content = parse(&content_xml)?;
    let styles_document = styles_xml.as_deref().and_then(parse);
    let styles = Styles::of(styles_document.as_ref(), &content);
    let body = content
        .root_element()
        .descendants()
        .find(|node| is(*node, "office:presentation"))?;
    let pages: Vec<Node> = elements(body).filter(|page| is(*page, "draw:page")).collect();

    let first_master = styles.master(pages.first().and_then(|page| attr(*page, "draw:master-page-name")));
    let layout = first_master.and_then(|master| styles.layouts.get(attr(master, "style:page-layout-name")?).copied());
    let page = layout.and_then(|layout| child(layout, "style:page-layout-properties"));
    let size = |name, default| page.and_then(|page| attr(page, name)).and_then(pt).unwrap_or(default);
    let (width, height) = (size("fo:page-width", 793.7), size("fo:page-height", 446.5));

    let mut writer = Writer {
        styles: &styles,
        package: &mut package,
        relationships: Vec::new(),
        media: Vec::new(),
        ids: 1,
        number: 0,
    };
    let mut parts: Vec<(String, Vec<u8>)> = vec![(
        "_rels/.rels".into(),
        relationships(&[("rId1".into(), "officeDocument", "ppt/presentation.xml".into())]),
    )];
    let mut overrides = vec![(
        "ppt/presentation.xml".to_owned(),
        "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml",
    )];
    let mut listed = String::new();
    let mut slide_relationships = Vec::new();
    for (n, page) in pages.iter().enumerate() {
        if only.is_some_and(|only| only != n + 1) {
            continue;
        }
        let number = slide_relationships.len() + 1;
        writer.number = n + 1;
        let xml = writer.slide(*page);
        parts.push((format!("ppt/slides/slide{number}.xml"), xml.into_bytes()));
        parts.push((
            format!("ppt/slides/_rels/slide{number}.xml.rels"),
            relationships(&std::mem::take(&mut writer.relationships)),
        ));
        overrides.push((
            format!("ppt/slides/slide{number}.xml"),
            "application/vnd.openxmlformats-officedocument.presentationml.slide+xml",
        ));
        slide_relationships.push((format!("rId{number}"), "slide", format!("slides/slide{number}.xml")));
        let _ = write!(listed, r#"<p:sldId id="{}" r:id="rId{number}"/>"#, 255 + number);
    }
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
    for (name, bytes) in std::mem::take(&mut writer.media) {
        parts.push((format!("ppt/{name}"), bytes));
    }
    zip_of(&parts, &overrides)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Place {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    turned: f64,
}

impl Place {
    fn of(shape: Node) -> Option<Self> {
        let length = |name| attr(shape, name).and_then(pt);
        let (width, height) = (length("svg:width")?, length("svg:height")?);
        let mut place = Place {
            x: length("svg:x").unwrap_or(0.0),
            y: length("svg:y").unwrap_or(0.0),
            width,
            height,
            turned: 0.0,
        };
        if let Some(transform) = attr(shape, "draw:transform") {
            let mut angle = 0.0;
            let mut moved = None;
            for (name, arguments) in transform.split(')').filter_map(|part| part.split_once('(')) {
                let numbers: Vec<f64> = arguments
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .filter_map(pt_or_number)
                    .collect();
                match name.trim() {
                    "rotate" => angle = numbers.first().copied().unwrap_or(0.0),
                    "translate" => {
                        moved = Some((
                            numbers.first().copied().unwrap_or(0.0),
                            numbers.get(1).copied().unwrap_or(0.0),
                        ))
                    }
                    _ => {}
                }
            }
            let (tx, ty) = moved.unwrap_or((place.x, place.y));
            let (half_w, half_h) = (width / 2.0, height / 2.0);
            let (sin, cos) = angle.sin_cos();
            let (cx, cy) = (half_w * cos + half_h * sin + tx, -half_w * sin + half_h * cos + ty);
            place = Place {
                x: cx - half_w,
                y: cy - half_h,
                width,
                height,
                turned: (-angle.to_degrees()).rem_euclid(360.0),
            };
        }
        Some(place)
    }

    fn xfrm(&self, element: &str, flips: &str) -> String {
        let turned = if self.turned.abs() > 0.01 {
            format!(r#" rot="{}""#, (self.turned * 60_000.0).round() as i64)
        } else {
            String::new()
        };
        format!(
            r#"<{element}{turned}{flips}><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></{element}>"#,
            emu(self.x),
            emu(self.y),
            emu(self.width.max(0.0)),
            emu(self.height.max(0.0))
        )
    }
}

fn pt_or_number(value: &str) -> Option<f64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    pt(value).or_else(|| value.parse().ok())
}

struct Writer<'s, 'a, 'input, 'p, 'b> {
    styles: &'s Styles<'a, 'input>,
    package: &'p mut Package<'b>,
    relationships: Vec<(String, &'static str, String)>,
    media: Vec<(String, Vec<u8>)>,
    ids: u32,
    number: usize,
}

impl<'a, 'input> Writer<'_, 'a, 'input, '_, '_> {
    fn slide(&mut self, page: Node<'_, 'input>) -> String {
        self.ids = 1;
        let master = self.styles.master(attr(page, "draw:master-page-name"));
        let page_props = self.styles.props("drawing-page", attr(page, "draw:style-name"), false);
        let master_props = master
            .map(|master| {
                self.styles
                    .props("drawing-page", attr(master, "draw:style-name"), false)
            })
            .unwrap_or_default();
        let has_fill = |props: &Props| props.get("drawing-page draw:fill").is_some_and(|fill| fill != "none");
        let background = if has_fill(&page_props) {
            page_props.clone()
        } else {
            master_props
        };
        let background = self
            .fill(&background, "drawing-page")
            .filter(|fill| !fill.contains("noFill"));

        let mut shapes = String::new();
        if let Some(master) = master
            && page_props.get("drawing-page presentation:background-objects-visible") != Some("false")
        {
            for shape in elements(master) {
                if attr(shape, "presentation:class").is_some_and(|class| PLACEHOLDERS.contains(&class))
                    || is(shape, "presentation:notes")
                {
                    continue;
                }
                self.shape(shape, &mut shapes);
            }
        }
        for shape in elements(page) {
            if is(shape, "presentation:notes") || attr(shape, "presentation:placeholder") == Some("true") {
                continue;
            }
            self.shape(shape, &mut shapes);
        }
        let background = background.map_or_else(String::new, |fill| {
            format!("<p:bg><p:bgPr>{fill}<a:effectLst/></p:bgPr></p:bg>")
        });
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sld {NAMESPACES}><p:cSld>{background}<p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{shapes}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#
        )
    }

    fn id(&mut self) -> u32 {
        self.ids += 1;
        self.ids
    }

    fn relate(&mut self, kind: &'static str, target: String) -> String {
        let id = format!("rId{}", self.relationships.len() + 1);
        self.relationships.push((id.clone(), kind, target));
        id
    }

    fn picture(&mut self, bytes: Vec<u8>, format: Picture) -> String {
        let name = format!("media/image{}.{}", self.media.len() + 1, format.extension());
        self.media.push((name.clone(), bytes));
        self.relate("image", format!("../{name}"))
    }

    fn shape(&mut self, shape: Node<'_, 'input>, out: &mut String) {
        if attr(shape, "draw:layer").is_some_and(|layer| self.styles.hidden_layers.contains(layer)) {
            return;
        }
        if is(shape, "draw:g") {
            for inner in elements(shape) {
                self.shape(inner, out);
            }
            return;
        }
        if is(shape, "draw:line") || is(shape, "draw:connector") {
            self.line(shape, out);
            return;
        }
        let Some(place) = Place::of(shape) else { return };
        let props = self.graphic_props(shape);
        if is(shape, "draw:frame") {
            if let Some(text_box) = child(shape, "draw:text-box") {
                self.text_shape(shape, text_box, place, &props, "rect", out);
            } else if let Some(table) = child(shape, "table:table") {
                self.table(table, place, out);
            } else if let Some((bytes, format)) = super::frame_picture(self.package, shape, &PICTURES) {
                let id = self.id();
                let embed = self.picture(bytes, format);
                let _ = write!(
                    out,
                    r#"<p:pic><p:nvPicPr><p:cNvPr id="{id}" name="Picture {id}"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="{embed}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr>{}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>"#,
                    place.xfrm("a:xfrm", "")
                );
            }
            return;
        }
        let geometry = if is(shape, "draw:custom-shape") {
            let kind = child(shape, "draw:enhanced-geometry")
                .and_then(|geometry| attr(geometry, "draw:type"))
                .unwrap_or("rectangle");
            match PRESETS.iter().find(|(known, _)| *known == kind) {
                Some((_, preset)) => preset.to_string(),
                None => "rect".into(),
            }
        } else if is(shape, "draw:rect") {
            if attr(shape, "draw:corner-radius")
                .and_then(pt)
                .is_some_and(|radius| radius > 0.0)
            {
                "roundRect".into()
            } else {
                "rect".into()
            }
        } else if is(shape, "draw:ellipse") || is(shape, "draw:circle") {
            "ellipse".into()
        } else if is(shape, "draw:polygon") || is(shape, "draw:polyline") {
            match polygon(shape, is(shape, "draw:polygon")) {
                Some(geometry) => geometry,
                None => return,
            }
        } else {
            return;
        };
        self.text_shape(shape, shape, place, &props, &geometry, out);
    }

    fn graphic_props(&self, shape: Node) -> Props {
        match attr(shape, "presentation:style-name") {
            Some(style) => (*self.styles.props("presentation", Some(style), true)).clone(),
            None => (*self.styles.props("graphic", attr(shape, "draw:style-name"), true)).clone(),
        }
    }

    fn text_shape(
        &mut self,
        shape: Node<'_, 'input>,
        text: Node<'_, 'input>,
        place: Place,
        props: &Props,
        geometry: &str,
        out: &mut String,
    ) {
        let id = self.id();
        let geometry = if geometry.starts_with('<') {
            geometry.to_owned()
        } else {
            format!(r#"<a:prstGeom prst="{geometry}"><a:avLst/></a:prstGeom>"#)
        };
        let fill = self.fill(props, "graphic").unwrap_or_else(|| "<a:noFill/>".into());
        let line = line_of(props);
        let flips = match child(shape, "draw:enhanced-geometry") {
            Some(geometry) => {
                let flip = |name| attr(geometry, name) == Some("true");
                format!(
                    "{}{}",
                    if flip("draw:mirror-horizontal") {
                        r#" flipH="1""#
                    } else {
                        ""
                    },
                    if flip("draw:mirror-vertical") {
                        r#" flipV="1""#
                    } else {
                        ""
                    }
                )
            }
            None => String::new(),
        };
        let paragraphs = self.paragraphs(shape, text, props);
        let inset = |side: &str, default: f64| {
            props
                .pt(&format!("graphic fo:padding-{side}"))
                .or_else(|| props.pt("graphic fo:padding"))
                .unwrap_or(default)
        };
        let anchor = match props.get("graphic draw:textarea-vertical-align") {
            Some("middle") => "ctr",
            Some("bottom") => "b",
            _ if is(shape, "draw:custom-shape") && props.get("graphic draw:textarea-vertical-align").is_none() => "ctr",
            _ => "t",
        };
        let wrap = if props.get("graphic fo:wrap-option") == Some("no-wrap") {
            "none"
        } else {
            "square"
        };
        let _ = write!(
            out,
            r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Shape {id}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{}{geometry}{fill}{line}</p:spPr><p:txBody><a:bodyPr wrap="{wrap}" lIns="{}" tIns="{}" rIns="{}" bIns="{}" anchor="{anchor}" rtlCol="0"><a:noAutofit/></a:bodyPr><a:lstStyle/>{paragraphs}</p:txBody></p:sp>"#,
            place.xfrm("a:xfrm", &flips),
            emu(inset("left", 7.1)),
            emu(inset("top", 3.6)),
            emu(inset("right", 7.1)),
            emu(inset("bottom", 3.6))
        );
    }

    fn fill(&mut self, props: &Props, kind: &str) -> Option<String> {
        let get = |name: &str| props.get(&format!("{kind} draw:{name}"));
        let alpha = props
            .get(&format!("{kind} draw:opacity"))
            .and_then(|opacity| opacity.trim_end_matches('%').parse::<f64>().ok())
            .filter(|opacity| *opacity < 100.0)
            .map(|opacity| format!(r#"<a:alpha val="{}"/>"#, (opacity * 1000.0).round()))
            .unwrap_or_default();
        match get("fill") {
            Some("solid") => {
                let hex = get("fill-color").and_then(color).unwrap_or_else(|| "729FCF".into());
                Some(format!(
                    r#"<a:solidFill><a:srgbClr val="{hex}">{alpha}</a:srgbClr></a:solidFill>"#
                ))
            }
            Some("gradient") => {
                let gradient = get("fill-gradient-name").and_then(|name| self.styles.gradients.get(name).copied());
                let start = gradient
                    .and_then(|gradient| attr(gradient, "draw:start-color"))
                    .and_then(color)
                    .unwrap_or_else(|| "FFFFFF".into());
                let end = gradient
                    .and_then(|gradient| attr(gradient, "draw:end-color"))
                    .and_then(color)
                    .unwrap_or_else(|| "000000".into());
                let angle = gradient
                    .and_then(|gradient| attr(gradient, "draw:angle"))
                    .map_or(0.0, |angle| {
                        if let Some(degrees) = angle.strip_suffix("deg") {
                            degrees.parse().unwrap_or(0.0)
                        } else if let Some(radians) = angle.strip_suffix("rad") {
                            radians.parse::<f64>().unwrap_or(0.0).to_degrees()
                        } else {
                            angle.parse::<f64>().unwrap_or(0.0) / 10.0
                        }
                    });
                let direction = (90.0 - angle).rem_euclid(360.0);
                Some(format!(
                    r#"<a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:srgbClr val="{start}"/></a:gs><a:gs pos="100000"><a:srgbClr val="{end}"/></a:gs></a:gsLst><a:lin ang="{}" scaled="0"/></a:gradFill>"#,
                    (direction * 60_000.0).round() as i64
                ))
            }
            Some("bitmap") => {
                let image = get("fill-image-name").and_then(|name| self.styles.fill_images.get(name).copied())?;
                let (bytes, format) = self.package.picture(attr(image, "xlink:href")?)?;
                if !PICTURES.contains(&format) {
                    return None;
                }
                let embed = self.picture(bytes, format);
                Some(format!(
                    r#"<a:blipFill dpi="0" rotWithShape="1"><a:blip r:embed="{embed}"/><a:srcRect/><a:stretch><a:fillRect/></a:stretch></a:blipFill>"#
                ))
            }
            Some("none") => Some("<a:noFill/>".into()),
            _ => None,
        }
    }

    fn line(&mut self, shape: Node, out: &mut String) {
        let length = |name| attr(shape, name).and_then(pt);
        let (Some(x1), Some(y1), Some(x2), Some(y2)) =
            (length("svg:x1"), length("svg:y1"), length("svg:x2"), length("svg:y2"))
        else {
            return;
        };
        let props = self.graphic_props(shape);
        let line = line_of(&props);
        if line.contains("noFill") {
            return;
        }
        let id = self.id();
        let flips = format!(
            "{}{}",
            if x2 < x1 { r#" flipH="1""# } else { "" },
            if y2 < y1 { r#" flipV="1""# } else { "" }
        );
        let place = Place {
            x: x1.min(x2),
            y: y1.min(y2),
            width: (x2 - x1).abs(),
            height: (y2 - y1).abs(),
            turned: 0.0,
        };
        let _ = write!(
            out,
            r#"<p:cxnSp><p:nvCxnSpPr><p:cNvPr id="{id}" name="Line {id}"/><p:cNvCxnSpPr/><p:nvPr/></p:nvCxnSpPr><p:spPr>{}<a:prstGeom prst="line"><a:avLst/></a:prstGeom>{line}</p:spPr></p:cxnSp>"#,
            place.xfrm("a:xfrm", &flips)
        );
    }

    fn table(&mut self, table: Node<'_, 'input>, place: Place, out: &mut String) {
        let mut widths = Vec::new();
        for column in elements(table).flat_map(|node| {
            if is(node, "table:table-columns") {
                elements(node).collect()
            } else {
                vec![node]
            }
        }) {
            if !is(column, "table:table-column") {
                continue;
            }
            let width = self
                .styles
                .props("table-column", attr(column, "table:style-name"), false)
                .pt("table-column style:column-width");
            let repeated = attr(column, "table:number-columns-repeated")
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, 64);
            widths.extend(std::iter::repeat_n(width, repeated));
        }
        let rows: Vec<Node> = table
            .descendants()
            .filter(|node| is(*node, "table:table-row"))
            .collect();
        if rows.is_empty() {
            return;
        }
        let count = widths
            .len()
            .max(rows.iter().map(|row| elements(*row).count()).max().unwrap_or(1))
            .max(1);
        widths.resize(count, None);
        let known: f64 = widths.iter().flatten().sum();
        let unknown = widths.iter().filter(|width| width.is_none()).count();
        let each = if unknown > 0 {
            ((place.width - known) / unknown as f64).max(9.0)
        } else {
            0.0
        };
        let widths: Vec<f64> = widths.iter().map(|width| width.unwrap_or(each)).collect();
        let row_height = place.height / rows.len() as f64;

        let id = self.id();
        let mut xml = String::from("<a:tbl><a:tblPr/><a:tblGrid>");
        for width in &widths {
            let _ = write!(xml, r#"<a:gridCol w="{}"/>"#, emu(*width));
        }
        xml.push_str("</a:tblGrid>");
        let edges: String = ["L", "R", "T", "B"]
            .iter()
            .map(|side| {
                format!(r#"<a:ln{side} w="9525"><a:solidFill><a:srgbClr val="808080"/></a:solidFill></a:ln{side}>"#)
            })
            .collect();
        for row in rows {
            let _ = write!(xml, r#"<a:tr h="{}">"#, emu(row_height));
            let mut written = 0;
            for cell in elements(row) {
                if written >= widths.len() {
                    break;
                }
                let covered = is(cell, "table:covered-table-cell");
                if !(covered || is(cell, "table:table-cell")) {
                    continue;
                }
                written += 1;
                if covered {
                    xml.push_str(
                        r#"<a:tc hMerge="1"><a:txBody><a:bodyPr/><a:lstStyle/><a:p/></a:txBody><a:tcPr/></a:tc>"#,
                    );
                    continue;
                }
                let cell_props = self.styles.props("table-cell", attr(cell, "table:style-name"), true);
                let span = attr(cell, "table:number-columns-spanned")
                    .and_then(|n| n.parse::<usize>().ok())
                    .filter(|span| *span > 1);
                let paragraphs = self.paragraphs(cell, cell, &cell_props);
                let fill = self
                    .fill(&cell_props, "graphic")
                    .filter(|fill| !fill.contains("noFill"))
                    .unwrap_or_default();
                let _ = write!(
                    xml,
                    r#"<a:tc{}><a:txBody><a:bodyPr/><a:lstStyle/>{paragraphs}</a:txBody><a:tcPr marL="91440" marR="91440" marT="45720" marB="45720">{edges}{fill}</a:tcPr></a:tc>"#,
                    span.map_or_else(String::new, |span| format!(r#" gridSpan="{span}""#)),
                );
            }
            while written < widths.len() {
                xml.push_str(r#"<a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p/></a:txBody><a:tcPr/></a:tc>"#);
                written += 1;
            }
            xml.push_str("</a:tr>");
        }
        xml.push_str("</a:tbl>");
        let _ = write!(
            out,
            r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="{id}" name="Table {id}"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>{}<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table">{xml}</a:graphicData></a:graphic></p:graphicFrame>"#,
            place.xfrm("p:xfrm", "")
        );
    }

    fn paragraphs(&mut self, shape: Node, container: Node<'_, 'input>, props: &Props) -> String {
        let mut out = String::new();
        let shape_list = self.embedded_list(shape);
        self.blocks(container, props, shape, shape_list, &mut out);
        if out.is_empty() {
            let _ = write!(out, "<a:p><a:endParaRPr{}/></a:p>", self.rpr_attributes(props));
        }
        out
    }

    fn embedded_list(&self, shape: Node) -> Option<Node<'a, 'input>> {
        let (family, mut name) = match attr(shape, "presentation:style-name") {
            Some(style) => ("presentation", Some(style)),
            None => ("graphic", attr(shape, "draw:style-name")),
        };
        for _ in 0..16 {
            let style = self
                .styles
                .style(family, name?)
                .or_else(|| self.styles.style("graphic", name?))?;
            if let Some(list) =
                child(style, "style:graphic-properties").and_then(|properties| child(properties, "text:list-style"))
            {
                return Some(list);
            }
            name = attr(style, "style:parent-style-name");
        }
        None
    }

    fn outline_level(&self, shape_props: &Props, shape: Node, level: usize) -> Props {
        let mut name = attr(shape, "presentation:style-name");
        for _ in 0..16 {
            let Some(style) = name.and_then(|name| self.styles.style("presentation", name)) else {
                break;
            };
            let own = attr(style, "style:name").unwrap_or("");
            if let Some(stem) = own.strip_suffix("outline1") {
                let leveled = format!("{stem}outline{level}");
                if self.styles.style("presentation", &leveled).is_some() {
                    return (*self.styles.props("presentation", Some(&leveled), true)).clone();
                }
                break;
            }
            name = attr(style, "style:parent-style-name");
        }
        shape_props.clone()
    }

    fn blocks(
        &mut self,
        node: Node<'_, 'input>,
        props: &Props,
        shape: Node,
        shape_list: Option<Node<'a, 'input>>,
        out: &mut String,
    ) {
        for block in elements(node) {
            if is(block, "text:p") || is(block, "text:h") {
                self.paragraph(block, props, None, 0, out);
            } else if is(block, "text:list") {
                self.list(block, props, shape, shape_list, None, 1, out);
            } else if is(block, "draw:text-box") || is(block, "text:section") {
                self.blocks(block, props, shape, shape_list, out);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn list(
        &mut self,
        list: Node<'_, 'input>,
        props: &Props,
        shape: Node,
        shape_list: Option<Node<'a, 'input>>,
        inherited: Option<Node<'a, 'input>>,
        level: usize,
        out: &mut String,
    ) {
        let own = attr(list, "text:style-name").and_then(|name| self.styles.lists.get(name).copied());
        let style = own.or(inherited).or(shape_list);
        let level_props = if attr(shape, "presentation:class") == Some("outline") {
            self.outline_level(props, shape, level)
        } else {
            props.clone()
        };
        for item in elements(list) {
            let header = is(item, "text:list-header");
            if !(header || is(item, "text:list-item")) {
                continue;
            }
            let mut first = !header;
            for inner in elements(item) {
                if is(inner, "text:list") {
                    self.list(inner, props, shape, shape_list, style, level + 1, out);
                } else if is(inner, "text:p") || is(inner, "text:h") {
                    self.paragraph(
                        inner,
                        &level_props,
                        style.map(|style| (style, level, first)),
                        level,
                        out,
                    );
                    first = false;
                }
            }
        }
    }

    fn paragraph(
        &mut self,
        node: Node<'_, 'input>,
        props: &Props,
        bullet: Option<(Node, usize, bool)>,
        level: usize,
        out: &mut String,
    ) {
        let props = props.with(&self.styles.props("paragraph", attr(node, "text:style-name"), false));
        let mut ppr = String::new();
        let algn = match props.get("paragraph fo:text-align") {
            Some("center") => Some("ctr"),
            Some("end" | "right") => Some("r"),
            Some("justify") => Some("just"),
            _ => None,
        };
        let mut left = props.pt("paragraph fo:margin-left").unwrap_or(0.0);
        let mut indent = props.pt("paragraph fo:text-indent").unwrap_or(0.0);
        let mut bullet_xml = String::from("<a:buNone/>");
        if let Some((list, at, first)) = bullet {
            let style = (1..=at).rev().find_map(|level| {
                elements(list)
                    .find(|child| attr(*child, "text:level").and_then(|n| n.parse::<usize>().ok()) == Some(level))
            });
            if let Some(style) = style {
                let properties = child(style, "style:list-level-properties");
                let aligned = properties.and_then(|properties| child(properties, "style:list-level-label-alignment"));
                let length = |node: Option<Node>, name: &str| node.and_then(|node| attr(node, name)).and_then(pt);
                match aligned {
                    Some(aligned) => {
                        left = length(Some(aligned), "fo:margin-left").unwrap_or(left);
                        indent = length(Some(aligned), "fo:text-indent").unwrap_or(indent);
                    }
                    None if properties.is_some() => {
                        let before = length(properties, "text:space-before").unwrap_or(0.0);
                        let width = length(properties, "text:min-label-width").unwrap_or(0.0);
                        left = left.max(before + width);
                        indent = -width;
                    }
                    None => {}
                }
                if first {
                    bullet_xml = bullet_of(style);
                } else {
                    bullet_xml = "<a:buNone/>".into();
                }
            }
        }
        let _ = write!(
            ppr,
            r#"<a:pPr marL="{}" indent="{}" lvl="{}""#,
            emu(left.max(0.0)),
            emu(indent),
            level.saturating_sub(1).min(8)
        );
        if let Some(algn) = algn {
            let _ = write!(ppr, r#" algn="{algn}""#);
        }
        ppr.push('>');
        if let Some(height) = props
            .get("paragraph fo:line-height")
            .and_then(|height| height.strip_suffix('%'))
            .and_then(|percent| percent.parse::<f64>().ok())
        {
            let _ = write!(
                ppr,
                r#"<a:lnSpc><a:spcPct val="{}"/></a:lnSpc>"#,
                (height * 1000.0).round()
            );
        }
        for (element, name) in [
            ("a:spcBef", "paragraph fo:margin-top"),
            ("a:spcAft", "paragraph fo:margin-bottom"),
        ] {
            if let Some(space) = props.pt(name) {
                let _ = write!(
                    ppr,
                    r#"<{element}><a:spcPts val="{}"/></{element}>"#,
                    (space.max(0.0) * 100.0).round()
                );
            }
        }
        ppr.push_str(&bullet_xml);
        ppr.push_str("</a:pPr>");

        let text_props = props.text_only();
        let mut runs = String::new();
        let mut space = true;
        self.runs(node, &text_props, &mut runs, &mut space);
        let _ = write!(
            out,
            "<a:p>{ppr}{runs}<a:endParaRPr{}/></a:p>",
            self.rpr_attributes(&text_props)
        );
    }

    fn runs(&mut self, node: Node, props: &Props, out: &mut String, space: &mut bool) {
        for item in node.children() {
            if item.is_text() {
                let mut text = String::new();
                for c in item.text().unwrap_or("").chars() {
                    if !c.is_whitespace() {
                        text.push(c);
                        *space = false;
                    } else if !*space {
                        text.push(' ');
                        *space = true;
                    }
                }
                self.run(out, &text, props);
            } else if is(item, "text:span") || is(item, "text:a") {
                let inner = props.with(&self.styles.props("text", attr(item, "text:style-name"), false));
                self.runs(item, &inner, out, space);
            } else if is(item, "text:s") {
                let count = attr(item, "text:c")
                    .and_then(|n| n.parse::<usize>().ok())
                    .unwrap_or(1)
                    .min(64);
                self.run(out, &" ".repeat(count), props);
                *space = true;
            } else if is(item, "text:tab") {
                self.run(out, "\t", props);
                *space = true;
            } else if is(item, "text:line-break") {
                let _ = write!(out, "<a:br><a:rPr{}/></a:br>", self.rpr_attributes(props));
                *space = true;
            } else if is(item, "text:page-number") {
                self.run(out, &self.number.to_string(), props);
                *space = false;
            } else if item.is_element() && !is(item, "office:annotation") && !is(item, "text:note") {
                self.runs(item, props, out, space);
            }
        }
    }

    fn run(&self, out: &mut String, text: &str, props: &Props) {
        if text.is_empty() {
            return;
        }
        let _ = write!(
            out,
            "<a:r><a:rPr{}>{}</a:rPr><a:t>{}</a:t></a:r>",
            self.rpr_attributes(props),
            self.rpr_children(props),
            escape(text)
        );
    }

    fn rpr_attributes(&self, props: &Props) -> String {
        let mut attributes = String::new();
        let size = props.pt("text fo:font-size").unwrap_or(18.0);
        let _ = write!(
            attributes,
            r#" lang="en-US" sz="{}""#,
            (size * 100.0).round().clamp(100.0, 400_000.0)
        );
        if super::text::is_bold(props.get("text fo:font-weight")) {
            attributes.push_str(r#" b="1""#);
        }
        if matches!(props.get("text fo:font-style"), Some("italic" | "oblique")) {
            attributes.push_str(r#" i="1""#);
        }
        if props
            .get("text style:text-underline-style")
            .is_some_and(|style| style != "none")
        {
            attributes.push_str(r#" u="sng""#);
        }
        if props
            .get("text style:text-line-through-style")
            .is_some_and(|style| style != "none")
        {
            attributes.push_str(r#" strike="sngStrike""#);
        }
        if let Some(position) = props.get("text style:text-position") {
            let first = position.split_whitespace().next().unwrap_or("");
            let shift = match first {
                "super" => Some(30.0),
                "sub" => Some(-25.0),
                other => other
                    .trim_end_matches('%')
                    .parse::<f64>()
                    .ok()
                    .filter(|shift| *shift != 0.0),
            };
            if let Some(shift) = shift {
                let _ = write!(attributes, r#" baseline="{}""#, (shift * 1000.0).round());
            }
        }
        if props.get("text fo:text-transform") == Some("uppercase") {
            attributes.push_str(r#" cap="all""#);
        } else if props.get("text fo:font-variant") == Some("small-caps") {
            attributes.push_str(r#" cap="small""#);
        }
        attributes
    }

    fn rpr_children(&self, props: &Props) -> String {
        let mut children = String::new();
        if props.get("text style:use-window-font-color") != Some("true")
            && let Some(hex) = props.get("text fo:color").and_then(color)
        {
            let _ = write!(children, r#"<a:solidFill><a:srgbClr val="{hex}"/></a:solidFill>"#);
        }
        let (latin, asian, complex) = self.styles.fonts_of(props);
        if let Some(latin) = latin {
            let _ = write!(children, r#"<a:latin typeface="{}"/>"#, escape(&latin));
        }
        if let Some(asian) = asian {
            let _ = write!(children, r#"<a:ea typeface="{}"/>"#, escape(&asian));
        }
        if let Some(complex) = complex {
            let _ = write!(children, r#"<a:cs typeface="{}"/>"#, escape(&complex));
        }
        children
    }
}

fn bullet_of(style: Node) -> String {
    let text = child(style, "style:text-properties");
    let mut xml = String::new();
    if let Some(hex) = text.and_then(|text| attr(text, "fo:color")).and_then(color) {
        let _ = write!(xml, r#"<a:buClr><a:srgbClr val="{hex}"/></a:buClr>"#);
    }
    if let Some(percent) = text
        .and_then(|text| attr(text, "fo:font-size"))
        .and_then(|size| size.strip_suffix('%'))
        .and_then(|percent| percent.parse::<f64>().ok())
    {
        let _ = write!(
            xml,
            r#"<a:buSzPct val="{}"/>"#,
            (percent * 1000.0).round().clamp(25_000.0, 400_000.0)
        );
    }
    if is(style, "text:list-level-style-number") {
        let format = attr(style, "style:num-format").unwrap_or("1");
        if format.is_empty() {
            return "<a:buNone/>".into();
        }
        let suffix = attr(style, "style:num-suffix").unwrap_or(".");
        let kind = match (format, suffix) {
            ("1", ")") => "arabicParenR",
            ("1", _) => "arabicPeriod",
            ("a", ")") => "alphaLcParenR",
            ("a", _) => "alphaLcPeriod",
            ("A", ")") => "alphaUcParenR",
            ("A", _) => "alphaUcPeriod",
            ("i", _) => "romanLcPeriod",
            ("I", _) => "romanUcPeriod",
            _ => "arabicPeriod",
        };
        let start = attr(style, "text:start-value")
            .map_or_else(String::new, |start| format!(r#" startAt="{}""#, escape(start)));
        let _ = write!(xml, r#"<a:buAutoNum type="{kind}"{start}/>"#);
    } else if is(style, "text:list-level-style-bullet") {
        let _ = write!(
            xml,
            r#"<a:buChar char="{}"/>"#,
            escape(&super::bullet(attr(style, "text:bullet-char").unwrap_or("•"), style))
        );
    } else {
        xml.push_str(r#"<a:buChar char="•"/>"#);
    }
    xml
}

fn line_of(props: &Props) -> String {
    match props.get("graphic draw:stroke") {
        Some("none") | None => "<a:ln><a:noFill/></a:ln>".into(),
        Some(stroke) => {
            let hex = props
                .get("graphic svg:stroke-color")
                .and_then(color)
                .unwrap_or_else(|| "000000".into());
            let width = props
                .pt("graphic svg:stroke-width")
                .filter(|width| *width > 0.0)
                .unwrap_or(0.75);
            let dash = if stroke == "dash" {
                r#"<a:prstDash val="dash"/>"#
            } else {
                ""
            };
            format!(
                r#"<a:ln w="{}"><a:solidFill><a:srgbClr val="{hex}"/></a:solidFill>{dash}</a:ln>"#,
                emu(width)
            )
        }
    }
}

fn polygon(shape: Node, closed: bool) -> Option<String> {
    let view: Vec<f64> = attr(shape, "svg:viewBox")?
        .split_whitespace()
        .filter_map(|n| n.parse().ok())
        .collect();
    let (width, height) = (*view.get(2)?, *view.get(3)?);
    let (left, top) = (view[0], view[1]);
    let points: Vec<(f64, f64)> = attr(shape, "draw:points")?
        .split_whitespace()
        .filter_map(|point| {
            let (x, y) = point.split_once(',')?;
            Some((x.parse::<f64>().ok()? - left, y.parse::<f64>().ok()? - top))
        })
        .collect();
    let (first, rest) = points.split_first()?;
    let mut path = format!(
        r#"<a:moveTo><a:pt x="{}" y="{}"/></a:moveTo>"#,
        first.0.round() as i64,
        first.1.round() as i64
    );
    for (x, y) in rest {
        let _ = write!(
            path,
            r#"<a:lnTo><a:pt x="{}" y="{}"/></a:lnTo>"#,
            x.round() as i64,
            y.round() as i64
        );
    }
    if closed {
        path.push_str("<a:close/>");
    }
    Some(format!(
        r#"<a:custGeom><a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/><a:rect l="0" t="0" r="r" b="b"/><a:pathLst><a:path w="{}" h="{}">{path}</a:path></a:pathLst></a:custGeom>"#,
        width.round() as i64,
        height.round() as i64
    ))
}
