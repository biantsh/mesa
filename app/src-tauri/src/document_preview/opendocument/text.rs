use std::{cell::RefCell, collections::HashMap, fmt::Write as _, rc::Rc};

use roxmltree::Node;

use super::{
    Package, Picture, Props, Styles, attr, child, color, elements, emu, escape, is, numeral, parse, pt, relationships,
    twips, zip_of,
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

#[derive(Clone, Copy)]
pub(in super::super) struct Limit {
    pub blocks: usize,
    pub chars: usize,
}

pub(in super::super) fn docx(bytes: &[u8], limit: Option<Limit>) -> Option<Vec<u8>> {
    let mut package = Package::open(bytes)?;
    let content_xml = package.text("content.xml")?;
    let styles_xml = package.text("styles.xml");
    let content = parse(&content_xml)?;
    let styles_document = styles_xml.as_deref().and_then(parse);
    let styles = Styles::of(styles_document.as_ref(), &content);
    let body = content
        .root_element()
        .descendants()
        .find(|node| is(*node, "office:text"))?;

    let mut writer = Writer::new(&styles, &mut package, limit);
    let first_master = first_master(&styles, body);
    writer.section = first_master;
    writer.blocks(body, None);
    writer.finish()
}

fn first_master(styles: &Styles, body: Node) -> Option<String> {
    let first = body
        .descendants()
        .find(|node| is(*node, "text:p") || is(*node, "text:h") || is(*node, "table:table"));
    let said = first.and_then(|first| {
        let (family, name) = if is(first, "table:table") {
            ("table", attr(first, "table:style-name"))
        } else {
            ("paragraph", attr(first, "text:style-name"))
        };
        master_named(styles, family, name?)
    });
    said.or_else(|| {
        styles
            .master(Some("Standard"))
            .and_then(|master| attr(master, "style:name"))
            .map(str::to_owned)
    })
}

fn master_named(styles: &Styles, family: &str, name: &str) -> Option<String> {
    let mut next = styles.style(family, name);
    for _ in 0..8 {
        let style = next?;
        if let Some(master) = attr(style, "style:master-page-name") {
            return (!master.is_empty()).then(|| master.to_owned());
        }
        next = attr(style, "style:parent-style-name").and_then(|parent| styles.style(family, parent));
    }
    None
}

#[derive(Clone)]
struct InList {
    label: Option<String>,
    left: f64,
    first: f64,
    after: After,
}

#[derive(Clone, Copy)]
enum After {
    Tab(f64),
    Space,
    Nothing,
}

#[derive(Default)]
struct Part {
    relationships: Vec<(String, &'static str, String)>,
}

type Written = (Rc<Props>, Rc<str>);
type Inherited = (Rc<Props>, Rc<Props>);

struct Writer<'s, 'a, 'input, 'p, 'b> {
    styles: &'s Styles<'a, 'input>,
    package: &'p mut Package<'b>,
    limit: Option<Limit>,
    body: String,
    part: Part,
    media: Vec<(String, Vec<u8>)>,
    notes: Vec<(u32, String)>,
    headers: Vec<(String, String, Part)>,
    section: Option<String>,
    blocks: usize,
    chars: usize,
    full: bool,
    headings: [u32; 10],
    lists: HashMap<String, [u32; 10]>,
    last_list: HashMap<String, String>,
    lists_made: usize,
    break_before: bool,
    in_header: bool,
    pictures: u32,
    texts: HashMap<usize, Inherited>,
    spans: HashMap<(usize, String), Rc<Props>>,
    rprs: RefCell<HashMap<usize, Written>>,
}

impl<'s, 'a, 'input, 'p, 'b> Writer<'s, 'a, 'input, 'p, 'b> {
    fn new(styles: &'s Styles<'a, 'input>, package: &'p mut Package<'b>, limit: Option<Limit>) -> Self {
        Self {
            styles,
            package,
            limit,
            body: String::new(),
            part: Part::default(),
            media: Vec::new(),
            notes: Vec::new(),
            headers: Vec::new(),
            section: None,
            blocks: 0,
            chars: 0,
            full: false,
            headings: [0; 10],
            lists: HashMap::new(),
            last_list: HashMap::new(),
            lists_made: 0,
            break_before: false,
            in_header: false,
            pictures: 0,
            texts: HashMap::new(),
            spans: HashMap::new(),
            rprs: RefCell::new(HashMap::new()),
        }
    }

    fn blocks(&mut self, node: Node<'_, 'input>, in_list: Option<&InList>) {
        for block in elements(node) {
            if self.full {
                return;
            }
            if is(block, "text:p") || is(block, "text:h") {
                self.paragraph(block, in_list.cloned());
                self.counted();
            } else if is(block, "text:list") {
                self.list(block, 0, None);
            } else if is(block, "table:table") {
                self.table(block);
                self.counted();
            } else if is(block, "text:section") || is(block, "text:index-body") || is(block, "text:numbered-paragraph")
            {
                self.blocks(block, in_list);
            } else if block.tag_name().name().ends_with("index")
                || is(block, "text:table-of-content")
                || is(block, "text:bibliography")
            {
                if let Some(index_body) = child(block, "text:index-body") {
                    self.blocks(index_body, in_list);
                }
            } else if is(block, "draw:frame") || is(block, "draw:a") {
                let mut runs = String::new();
                let mut after = Vec::new();
                self.drawing(block, &mut runs, &mut after);
                if !runs.is_empty() {
                    self.body.push_str("<w:p>");
                    self.body.push_str(&runs);
                    self.body.push_str("</w:p>");
                }
                for frame in after {
                    self.blocks(frame, None);
                }
            }
        }
    }

    fn counted(&mut self) {
        self.blocks += 1;
        if let Some(limit) = self.limit {
            self.full |= self.blocks >= limit.blocks || self.chars >= limit.chars;
        }
    }

    fn list(&mut self, list: Node<'_, 'input>, level: usize, inherited: Option<&str>) {
        let style = attr(list, "text:style-name").or(inherited).unwrap_or("").to_owned();
        let id = if level == 0 {
            let goes_on =
                attr(list, "text:continue-numbering") == Some("true") || attr(list, "text:continue-list").is_some();
            match self.last_list.get(&style).filter(|_| goes_on) {
                Some(id) => id.clone(),
                None => {
                    self.lists_made += 1;
                    let id = format!("{style}#{}", self.lists_made);
                    self.last_list.insert(style.clone(), id.clone());
                    id
                }
            }
        } else {
            self.last_list.get(&style).cloned().unwrap_or_default()
        };
        let level_style = self.level_style(&style, level);
        for item in elements(list) {
            if self.full {
                return;
            }
            let header = is(item, "text:list-header");
            if !(header || is(item, "text:list-item")) {
                continue;
            }
            let label = if header {
                None
            } else {
                Some(self.label(&id, level, level_style, attr(item, "text:start-value")))
            };
            let mut in_list = self.placed(level_style, label);
            for block in elements(item) {
                if self.full {
                    return;
                }
                if is(block, "text:list") {
                    self.list(block, level + 1, Some(&style));
                } else if is(block, "text:p") || is(block, "text:h") {
                    self.paragraph(block, Some(in_list.clone()));
                    self.counted();
                    in_list.label = None;
                } else {
                    self.blocks_of_one(block, &in_list);
                }
            }
        }
    }

    fn blocks_of_one(&mut self, block: Node<'_, 'input>, in_list: &InList) {
        if is(block, "table:table") {
            self.table(block);
            self.counted();
        } else if is(block, "text:section") || is(block, "text:numbered-paragraph") {
            self.blocks(block, Some(in_list));
        }
    }

    fn level_style(&self, style: &str, level: usize) -> Option<Node<'a, 'input>> {
        let list = self.styles.lists.get(style)?;
        (0..=level).rev().find_map(|level| {
            elements(*list)
                .find(|child| attr(*child, "text:level").and_then(|n| n.parse::<usize>().ok()) == Some(level + 1))
        })
    }

    fn label(&mut self, id: &str, level: usize, level_style: Option<Node>, start: Option<&str>) -> String {
        let counters = self.lists.entry(id.to_owned()).or_insert([0; 10]);
        let level = level.min(9);
        let Some(style) = level_style else { return "•".into() };
        let prefix = attr(style, "style:num-prefix").unwrap_or("");
        let suffix = attr(style, "style:num-suffix").unwrap_or("");
        if is(style, "text:list-level-style-number") {
            let first = attr(style, "text:start-value")
                .and_then(|n| n.parse().ok())
                .unwrap_or(1u32);
            counters[level] = match start.and_then(|n| n.parse().ok()) {
                Some(start) => start,
                None if counters[level] == 0 => first,
                None => counters[level] + 1,
            };
            for deeper in counters.iter_mut().skip(level + 1) {
                *deeper = 0;
            }
            let format = attr(style, "style:num-format").unwrap_or("1");
            let shown = attr(style, "text:display-levels")
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, level + 1);
            let numbers: Vec<String> = (level + 1 - shown..=level)
                .map(|at| numeral(counters[at].max(1), format))
                .collect();
            if format.is_empty() {
                format!("{prefix}{suffix}")
            } else {
                format!("{prefix}{}{suffix}", numbers.join("."))
            }
        } else if is(style, "text:list-level-style-bullet") {
            format!(
                "{prefix}{}{suffix}",
                super::bullet(attr(style, "text:bullet-char").unwrap_or("•"), style)
            )
        } else {
            "•".into()
        }
    }

    fn placed(&self, level_style: Option<Node>, label: Option<String>) -> InList {
        let properties = level_style.and_then(|style| child(style, "style:list-level-properties"));
        let aligned = properties.and_then(|properties| child(properties, "style:list-level-label-alignment"));
        let length = |node: Option<Node>, name: &str| node.and_then(|node| attr(node, name)).and_then(pt);
        match aligned {
            Some(aligned) => {
                let left = length(Some(aligned), "fo:margin-left").unwrap_or(0.0);
                let first = left + length(Some(aligned), "fo:text-indent").unwrap_or(0.0);
                let after = match attr(aligned, "text:label-followed-by") {
                    Some("space") => After::Space,
                    Some("nothing") => After::Nothing,
                    _ => After::Tab(
                        length(Some(aligned), "text:list-tab-stop-position")
                            .unwrap_or(left)
                            .max(first),
                    ),
                };
                InList {
                    label,
                    left,
                    first,
                    after,
                }
            }
            None => {
                let before = length(properties, "text:space-before").unwrap_or(0.0);
                let width = length(properties, "text:min-label-width").unwrap_or(0.0);
                InList {
                    label,
                    left: before + width,
                    first: before,
                    after: After::Tab(before + width),
                }
            }
        }
    }

    fn paragraph(&mut self, node: Node<'_, 'input>, in_list: Option<InList>) {
        let style = attr(node, "text:style-name");
        let props = self.styles.props("paragraph", style, true);
        let heading = is(node, "text:h").then(|| {
            attr(node, "text:outline-level")
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, 10)
        });

        let mut page_break = (std::mem::take(&mut self.break_before)
            || props.get("paragraph fo:break-before") == Some("page"))
            && self.blocks > 0;
        if !self.in_header
            && let Some(master) = style.and_then(|style| master_named(self.styles, "paragraph", style))
            && self.blocks > 0
        {
            if self.section.as_deref() != Some(master.as_str()) {
                let settings = self.section_settings();
                let _ = write!(self.body, "<w:p><w:pPr>{settings}</w:pPr></w:p>");
                self.section = Some(master);
            } else {
                page_break = true;
            }
        }
        self.break_before = props.get("paragraph fo:break-after") == Some("page");

        let mut in_list = in_list;
        if let Some(level) = heading
            && in_list.is_none()
            && attr(node, "text:is-list-header") != Some("true")
            && let Some(number) = self.heading_number(level)
        {
            let left = props.pt("paragraph fo:margin-left").unwrap_or(0.0);
            let first = left + props.pt("paragraph fo:text-indent").unwrap_or(0.0);
            in_list = Some(InList {
                label: Some(number),
                left,
                first,
                after: After::Space,
            });
        }

        let mut ppr = String::new();
        if props.get("paragraph fo:keep-with-next") == Some("always") || heading.is_some() {
            ppr.push_str("<w:keepNext/>");
        }
        if page_break {
            ppr.push_str("<w:pageBreakBefore/>");
        }
        ppr.push_str(&paragraph_borders(&props));
        if let Some(fill) = props.get("paragraph fo:background-color").and_then(color) {
            let _ = write!(ppr, r#"<w:shd w:val="clear" w:color="auto" w:fill="{fill}"/>"#);
        }
        let mut tabs: Vec<(f64, String, String)> = props
            .get("paragraph tabs")
            .unwrap_or("")
            .split(';')
            .filter_map(|stop| {
                let mut parts = stop.split(':');
                Some((
                    parts.next()?.parse().ok()?,
                    parts.next()?.to_owned(),
                    parts.next().unwrap_or("none").to_owned(),
                ))
            })
            .collect();
        if let Some(InList {
            after: After::Tab(at), ..
        }) = &in_list
            && in_list.as_ref().is_some_and(|in_list| in_list.label.is_some())
        {
            tabs.push((*at, "left".into(), "none".into()));
        }
        if !tabs.is_empty() {
            ppr.push_str("<w:tabs>");
            for (at, kind, leader) in &tabs {
                let kind = match kind.as_str() {
                    "center" => "center",
                    "right" => "right",
                    "char" => "decimal",
                    _ => "left",
                };
                let leader = if leader == "none" {
                    String::new()
                } else {
                    format!(r#" w:leader="{leader}""#)
                };
                let _ = write!(ppr, r#"<w:tab w:val="{kind}"{leader} w:pos="{}"/>"#, twips(*at));
            }
            ppr.push_str("</w:tabs>");
        }
        let line = match (
            props.get("paragraph fo:line-height"),
            props.pt("paragraph style:line-height-at-least"),
        ) {
            (Some(height), _) if height.ends_with('%') => height
                .trim_end_matches('%')
                .parse::<f64>()
                .ok()
                .map(|percent| format!(r#" w:line="{}" w:lineRule="auto""#, (240.0 * percent / 100.0).round())),
            (Some(height), _) => pt(height).map(|height| format!(r#" w:line="{}" w:lineRule="exact""#, twips(height))),
            (None, Some(least)) => Some(format!(r#" w:line="{}" w:lineRule="atLeast""#, twips(least))),
            _ => None,
        };
        let _ = write!(
            ppr,
            r#"<w:spacing w:before="{}" w:after="{}"{}/>"#,
            twips(props.pt("paragraph fo:margin-top").unwrap_or(0.0).max(0.0)),
            twips(props.pt("paragraph fo:margin-bottom").unwrap_or(0.0).max(0.0)),
            line.unwrap_or_default()
        );
        let (left, first) = match &in_list {
            Some(in_list) => (in_list.left, in_list.first),
            None => {
                let left = props.pt("paragraph fo:margin-left").unwrap_or(0.0);
                (left, left + props.pt("paragraph fo:text-indent").unwrap_or(0.0))
            }
        };
        let right = props.pt("paragraph fo:margin-right").unwrap_or(0.0);
        let indent = first - left;
        let first_line = if indent >= 0.0 {
            format!(r#" w:firstLine="{}""#, twips(indent))
        } else {
            format!(r#" w:hanging="{}""#, twips(-indent))
        };
        let _ = write!(
            ppr,
            r#"<w:ind w:left="{}" w:right="{}"{first_line}/>"#,
            twips(left),
            twips(right)
        );
        let justified = match props.get("paragraph fo:text-align") {
            Some("center") => Some("center"),
            Some("end" | "right") => Some("right"),
            Some("justify") => Some("both"),
            _ => None,
        };
        if let Some(justified) = justified {
            let _ = write!(ppr, r#"<w:jc w:val="{justified}"/>"#);
        }
        if let Some(level) = heading {
            let _ = write!(ppr, r#"<w:outlineLvl w:val="{}"/>"#, level - 1);
        }
        let text_props = self.text_of(&props);
        let _ = write!(ppr, "<w:rPr>{}</w:rPr>", self.rpr_of(&text_props));

        let mut runs = String::new();
        let mut after = Vec::new();
        if let Some(InList {
            label: Some(label),
            after: following,
            ..
        }) = &in_list
        {
            let mut label = label.clone();
            if let After::Space = following {
                label.push(' ');
            }
            self.run(&mut runs, &label, &text_props);
            if let After::Tab(_) = following {
                runs.push_str("<w:r><w:tab/></w:r>");
            }
        }
        let mut space = true;
        self.inline(node, &text_props, &mut runs, &mut after, &mut space);
        let _ = write!(self.body, "<w:p><w:pPr>{ppr}</w:pPr>{runs}</w:p>");
        for frame in after {
            self.blocks(frame, None);
        }
    }

    fn heading_number(&mut self, level: usize) -> Option<String> {
        let outline = self.styles.outline?;
        let style = elements(outline)
            .find(|style| attr(*style, "text:level").and_then(|n| n.parse::<usize>().ok()) == Some(level))?;
        self.headings[level - 1] += 1;
        for deeper in self.headings.iter_mut().skip(level) {
            *deeper = 0;
        }
        let format = attr(style, "style:num-format").unwrap_or("");
        if format.is_empty() {
            return None;
        }
        let shown = attr(style, "text:display-levels")
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(1)
            .clamp(1, level);
        let numbers: Vec<String> = (level - shown..level)
            .map(|at| numeral(self.headings[at].max(1), format))
            .collect();
        Some(format!(
            "{}{}{}",
            attr(style, "style:num-prefix").unwrap_or(""),
            numbers.join("."),
            attr(style, "style:num-suffix").unwrap_or("").trim_end()
        ))
    }

    fn inline<'n>(
        &mut self,
        node: Node<'n, 'input>,
        props: &Rc<Props>,
        runs: &mut String,
        after: &mut Vec<Node<'n, 'input>>,
        space: &mut bool,
    ) {
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
                self.chars += text.chars().count();
                self.run(runs, &text, props);
                continue;
            }
            if !item.is_element() {
                continue;
            }
            let name = item.tag_name().name();
            if is(item, "text:span") {
                let span = self.span_of(props, attr(item, "text:style-name"));
                self.inline(item, &span, runs, after, space);
            } else if is(item, "text:a") {
                let linked = self.span_of(props, attr(item, "text:style-name").or(Some("Internet_20_link")));
                let href = attr(item, "xlink:href").filter(|href| href.contains(':') && !href.starts_with('#'));
                match href {
                    Some(href) => {
                        let id = self.relate("hyperlink", href.to_owned());
                        let mut inside = String::new();
                        self.inline(item, &linked, &mut inside, after, space);
                        let _ = write!(runs, r#"<w:hyperlink r:id="{id}">{inside}</w:hyperlink>"#);
                    }
                    None => self.inline(item, &linked, runs, after, space),
                }
            } else if is(item, "text:s") {
                let count = attr(item, "text:c")
                    .and_then(|n| n.parse::<usize>().ok())
                    .unwrap_or(1)
                    .min(256);
                self.run(runs, &" ".repeat(count), props);
                *space = true;
            } else if is(item, "text:tab") {
                runs.push_str("<w:r><w:tab/></w:r>");
                *space = true;
            } else if is(item, "text:line-break") {
                runs.push_str("<w:r><w:br/></w:r>");
                *space = true;
            } else if is(item, "text:note") {
                self.note(item, props, runs);
            } else if is(item, "draw:frame") || is(item, "draw:a") {
                self.drawing(item, runs, after);
            } else if matches!(name, "custom-shape" | "rect" | "ellipse" | "polygon" | "path" | "g")
                && item.tag_name().namespace() == super::namespace("draw")
            {
                if item.descendants().any(|node| is(node, "text:p")) {
                    after.push(item);
                }
            } else if self.in_header && is(item, "text:page-number") {
                self.field(runs, "PAGE", item.text().unwrap_or("1"), props);
            } else if self.in_header && is(item, "text:page-count") {
                self.field(runs, "NUMPAGES", item.text().unwrap_or("1"), props);
            } else if is(item, "text:ruby") {
                if let Some(base) = child(item, "text:ruby-base") {
                    self.inline(base, props, runs, after, space);
                }
            } else if is_left_out(item) {
            } else {
                self.inline(item, props, runs, after, space);
            }
        }
    }

    fn run(&self, runs: &mut String, text: &str, props: &Rc<Props>) {
        if text.is_empty() {
            return;
        }
        let _ = write!(
            runs,
            r#"<w:r><w:rPr>{}</w:rPr><w:t xml:space="preserve">{}</w:t></w:r>"#,
            self.rpr_of(props),
            escape(text)
        );
    }

    fn field(&self, runs: &mut String, code: &str, shown: &str, props: &Rc<Props>) {
        let rpr = self.rpr_of(props);
        let _ = write!(
            runs,
            r#"<w:r><w:rPr>{rpr}</w:rPr><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:rPr>{rpr}</w:rPr><w:instrText xml:space="preserve"> {code} </w:instrText></w:r><w:r><w:rPr>{rpr}</w:rPr><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:rPr>{rpr}</w:rPr><w:t>{}</w:t></w:r><w:r><w:rPr>{rpr}</w:rPr><w:fldChar w:fldCharType="end"/></w:r>"#,
            escape(shown)
        );
    }

    fn text_of(&mut self, props: &Rc<Props>) -> Rc<Props> {
        let key = Rc::as_ptr(props) as usize;
        Rc::clone(
            &self
                .texts
                .entry(key)
                .or_insert_with(|| (Rc::clone(props), Rc::new(props.text_only())))
                .1,
        )
    }

    fn span_of(&mut self, props: &Rc<Props>, name: Option<&str>) -> Rc<Props> {
        let key = (Rc::as_ptr(props) as usize, name.unwrap_or("").to_owned());
        if let Some(known) = self.spans.get(&key) {
            return Rc::clone(known);
        }
        let span = Rc::new(props.with(&self.styles.props("text", name, false)));
        self.texts
            .entry(key.0)
            .or_insert_with(|| (Rc::clone(props), Rc::clone(props)));
        self.spans.insert(key, Rc::clone(&span));
        span
    }

    fn rpr_of(&self, props: &Rc<Props>) -> Rc<str> {
        let key = Rc::as_ptr(props) as usize;
        if let Some((_, known)) = self.rprs.borrow().get(&key) {
            return Rc::clone(known);
        }
        let rpr: Rc<str> = self.rpr(props).into();
        self.rprs.borrow_mut().insert(key, (Rc::clone(props), Rc::clone(&rpr)));
        rpr
    }

    fn rpr(&self, props: &Props) -> String {
        let mut rpr = String::new();
        let (latin, asian, complex) = self.styles.fonts_of(props);
        if latin.is_some() || asian.is_some() || complex.is_some() {
            rpr.push_str("<w:rFonts");
            if let Some(latin) = &latin {
                let _ = write!(rpr, r#" w:ascii="{0}" w:hAnsi="{0}""#, escape(latin));
            }
            if let Some(asian) = &asian {
                let _ = write!(rpr, r#" w:eastAsia="{}""#, escape(asian));
            }
            if let Some(complex) = complex.as_ref().or(latin.as_ref()) {
                let _ = write!(rpr, r#" w:cs="{}""#, escape(complex));
            }
            rpr.push_str("/>");
        }
        if is_bold(props.get("text fo:font-weight")) {
            rpr.push_str("<w:b/><w:bCs/>");
        }
        if matches!(props.get("text fo:font-style"), Some("italic" | "oblique")) {
            rpr.push_str("<w:i/><w:iCs/>");
        }
        if props.get("text fo:text-transform") == Some("uppercase") {
            rpr.push_str("<w:caps/>");
        }
        if props.get("text fo:font-variant") == Some("small-caps") {
            rpr.push_str("<w:smallCaps/>");
        }
        if props
            .get("text style:text-line-through-style")
            .is_some_and(|style| style != "none")
        {
            rpr.push_str("<w:strike/>");
        }
        if props.get("text text:display") == Some("none") {
            rpr.push_str("<w:vanish/>");
        }
        if props.get("text style:use-window-font-color") != Some("true")
            && let Some(hex) = props.get("text fo:color").and_then(color)
        {
            let _ = write!(rpr, r#"<w:color w:val="{hex}"/>"#);
        }
        if let Some(spacing) = props.pt("text fo:letter-spacing") {
            let _ = write!(rpr, r#"<w:spacing w:val="{}"/>"#, twips(spacing));
        }
        if let Some(size) = props.pt("text fo:font-size") {
            let half = (size * 2.0).round().max(2.0) as i64;
            let _ = write!(rpr, r#"<w:sz w:val="{half}"/><w:szCs w:val="{half}"/>"#);
        }
        if let Some(style) = props
            .get("text style:text-underline-style")
            .filter(|style| *style != "none")
        {
            let kind = match (style, props.get("text style:text-underline-type")) {
                (_, Some("double")) => "double",
                ("dotted", _) => "dotted",
                ("dash" | "long-dash", _) => "dash",
                ("wave", _) => "wave",
                _ => "single",
            };
            let _ = write!(rpr, r#"<w:u w:val="{kind}"/>"#);
        }
        if let Some(fill) = props.get("text fo:background-color").and_then(color) {
            let _ = write!(rpr, r#"<w:shd w:val="clear" w:color="auto" w:fill="{fill}"/>"#);
        }
        if let Some(position) = props.get("text style:text-position") {
            let first = position.split_whitespace().next().unwrap_or("");
            let raised = match first {
                "super" => Some(true),
                "sub" => Some(false),
                other => other
                    .trim_end_matches('%')
                    .parse::<f64>()
                    .ok()
                    .filter(|n| *n != 0.0)
                    .map(|n| n > 0.0),
            };
            if let Some(raised) = raised {
                let _ = write!(
                    rpr,
                    r#"<w:vertAlign w:val="{}"/>"#,
                    if raised { "superscript" } else { "subscript" }
                );
            }
        }
        rpr
    }

    fn note(&mut self, note: Node<'_, 'input>, props: &Rc<Props>, runs: &mut String) {
        let Some(body) = child(note, "text:note-body") else {
            return;
        };
        let id = self.notes.len() as u32 + 1;
        let text = std::mem::take(&mut self.body);
        let (blocks, full, limit) = (self.blocks, self.full, self.limit.take());
        self.blocks(body, None);
        let written = std::mem::replace(&mut self.body, text);
        (self.blocks, self.full, self.limit) = (blocks, full, limit);
        let reference = r#"<w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr><w:footnoteRef/></w:r><w:r><w:t xml:space="preserve"> </w:t></w:r>"#;
        let written = match written.find("</w:pPr>") {
            Some(at) => format!("{}{reference}{}", &written[..at + 8], &written[at + 8..]),
            None => format!("<w:p>{reference}</w:p>{written}"),
        };
        self.notes.push((id, written));
        let mut raised = (**props).clone();
        raised.0.insert("text style:text-position".into(), "super 58%".into());
        let _ = write!(
            runs,
            r#"<w:r><w:rPr>{}</w:rPr><w:footnoteReference w:id="{id}"/></w:r>"#,
            self.rpr(&raised)
        );
    }

    fn drawing<'n>(&mut self, frame: Node<'n, 'input>, runs: &mut String, after: &mut Vec<Node<'n, 'input>>) {
        if is(frame, "draw:a") {
            for inner in elements(frame).filter(|inner| is(*inner, "draw:frame")) {
                self.drawing(inner, runs, after);
            }
            return;
        }
        if let Some(text_box) = child(frame, "draw:text-box") {
            after.push(text_box);
            return;
        }
        let picture = if child(frame, "draw:image").is_some() {
            super::frame_picture(self.package, frame, &PICTURES)
        } else {
            None
        };
        let Some((bytes, format)) = picture else { return };
        let read = format.size_of(&bytes);
        let width = attr(frame, "svg:width").and_then(pt).or(read.map(|(width, _)| width));
        let height = attr(frame, "svg:height")
            .and_then(pt)
            .or(read.map(|(_, height)| height));
        let (Some(mut width), Some(mut height)) = (width, height) else {
            return;
        };
        let room = self.text_width();
        if width > room && width > 0.0 {
            height *= room / width;
            width = room;
        }
        self.pictures += 1;
        let number = self.pictures;
        let name = format!("media/image{number}.{}", format.extension());
        self.media.push((name.clone(), bytes));
        let id = self.relate("image", name);
        let (cx, cy) = (emu(width), emu(height));
        let _ = write!(
            runs,
            r#"<w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{number}" name="Picture {number}"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="{number}" name="Picture {number}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="{id}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#
        );
    }

    fn text_width(&self) -> f64 {
        let layout = self.page_layout();
        let get = |name| layout.and_then(|layout| attr(layout, name)).and_then(pt);
        (get("fo:page-width").unwrap_or(595.3)
            - get("fo:margin-left").unwrap_or(56.7)
            - get("fo:margin-right").unwrap_or(56.7))
        .max(72.0)
    }

    fn page_layout(&self) -> Option<Node<'a, 'input>> {
        let master = self.styles.master(self.section.as_deref())?;
        let layout = self.styles.layouts.get(attr(master, "style:page-layout-name")?)?;
        child(*layout, "style:page-layout-properties")
    }

    fn relate(&mut self, kind: &'static str, target: String) -> String {
        let id = format!("rId{}", self.part.relationships.len() + 1);
        self.part.relationships.push((id.clone(), kind, target));
        id
    }

    fn table(&mut self, table: Node<'_, 'input>) {
        let table_props = self.styles.props("table", attr(table, "table:style-name"), true);
        let mut columns = Vec::new();
        for column in table.descendants().filter(|node| is(*node, "table:table-column")) {
            if column.ancestors().skip(1).find(|node| is(*node, "table:table")) != Some(table) {
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
            columns.extend(std::iter::repeat_n(width, repeated));
        }
        let rows: Vec<(Node, bool)> = table
            .descendants()
            .filter(|node| {
                is(*node, "table:table-row")
                    && node.ancestors().skip(1).find(|node| is(*node, "table:table")) == Some(table)
            })
            .map(|row| {
                (
                    row,
                    row.parent().is_some_and(|parent| is(parent, "table:table-header-rows")),
                )
            })
            .collect();
        let count = rows
            .iter()
            .map(|(row, _)| {
                elements(*row)
                    .map(|cell| {
                        attr(cell, "table:number-columns-repeated")
                            .and_then(|n| n.parse::<usize>().ok())
                            .unwrap_or(1)
                            .min(64)
                    })
                    .sum::<usize>()
            })
            .max()
            .unwrap_or(0)
            .max(columns.len())
            .max(1);
        columns.resize(count, None);
        let room = self.text_width();
        let total = table_props.pt("table style:width").unwrap_or(room);
        let known: f64 = columns.iter().flatten().sum();
        let unknown = columns.iter().filter(|width| width.is_none()).count();
        let each = if unknown > 0 {
            ((total - known) / unknown as f64).max(18.0)
        } else {
            0.0
        };
        let mut widths: Vec<f64> = columns.iter().map(|width| width.unwrap_or(each)).collect();
        let sum: f64 = widths.iter().sum();
        if sum > room * 1.02 {
            widths.iter_mut().for_each(|width| *width *= room / sum);
        }

        let justified = match table_props.get("table table:align") {
            Some("center") => "center",
            Some("right") => "right",
            _ => "left",
        };
        let _ = write!(
            self.body,
            r#"<w:tbl><w:tblPr><w:tblW w:w="{}" w:type="dxa"/><w:jc w:val="{justified}"/><w:tblLayout w:type="fixed"/><w:tblCellMar><w:left w:w="0" w:type="dxa"/><w:right w:w="0" w:type="dxa"/></w:tblCellMar></w:tblPr><w:tblGrid>"#,
            twips(widths.iter().sum())
        );
        for width in &widths {
            let _ = write!(self.body, r#"<w:gridCol w:w="{}"/>"#, twips(*width));
        }
        self.body.push_str("</w:tblGrid>");
        if table_props.get("table fo:break-before") == Some("page") || std::mem::take(&mut self.break_before) {
            let at = self.body.rfind("<w:tbl>").unwrap_or(self.body.len());
            self.body
                .insert_str(at, "<w:p><w:pPr><w:pageBreakBefore/></w:pPr></w:p>");
        }

        let mut spanning: Vec<(usize, usize)> = vec![(0, 1); widths.len()];
        for (row, header) in rows {
            let repeated = attr(row, "table:number-rows-repeated")
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, 64);
            for _ in 0..repeated {
                if self.full {
                    break;
                }
                self.body.push_str("<w:tr>");
                if header {
                    self.body.push_str("<w:trPr><w:tblHeader/></w:trPr>");
                }
                let (mut column, mut written, mut beside) = (0, 0, 0);
                for cell in elements(row) {
                    let covered = is(cell, "table:covered-table-cell");
                    if !(covered || is(cell, "table:table-cell")) {
                        continue;
                    }
                    let repeats = attr(cell, "table:number-columns-repeated")
                        .and_then(|n| n.parse::<usize>().ok())
                        .unwrap_or(1)
                        .clamp(1, 64);
                    for _ in 0..repeats {
                        if column >= widths.len() {
                            break;
                        }
                        if covered {
                            let (rows_left, span) = spanning[column];
                            if column >= beside && rows_left > 0 {
                                spanning[column].0 -= 1;
                                let span = span.min(widths.len() - column);
                                let width: f64 = widths[column..column + span].iter().sum();
                                let _ = write!(
                                    self.body,
                                    r#"<w:tc><w:tcPr><w:tcW w:w="{}" w:type="dxa"/>{}<w:vMerge/></w:tcPr><w:p/></w:tc>"#,
                                    twips(width),
                                    if span > 1 {
                                        format!(r#"<w:gridSpan w:val="{span}"/>"#)
                                    } else {
                                        String::new()
                                    }
                                );
                                beside = column + span;
                                written = beside;
                            }
                            column += 1;
                            continue;
                        }
                        let span = attr(cell, "table:number-columns-spanned")
                            .and_then(|n| n.parse::<usize>().ok())
                            .unwrap_or(1)
                            .clamp(1, widths.len() - column);
                        let rows_spanned = attr(cell, "table:number-rows-spanned")
                            .and_then(|n| n.parse::<usize>().ok())
                            .unwrap_or(1)
                            .max(1);
                        spanning[column] = (rows_spanned - 1, span);
                        while written < column {
                            let _ = write!(
                                self.body,
                                r#"<w:tc><w:tcPr><w:tcW w:w="{}" w:type="dxa"/></w:tcPr><w:p/></w:tc>"#,
                                twips(widths[written])
                            );
                            written += 1;
                        }
                        let width: f64 = widths[column..column + span].iter().sum();
                        let cell_props = self.styles.props("table-cell", attr(cell, "table:style-name"), false);
                        let mut tcpr = format!(r#"<w:tcW w:w="{}" w:type="dxa"/>"#, twips(width));
                        if span > 1 {
                            let _ = write!(tcpr, r#"<w:gridSpan w:val="{span}"/>"#);
                        }
                        if rows_spanned > 1 {
                            tcpr.push_str(r#"<w:vMerge w:val="restart"/>"#);
                        }
                        tcpr.push_str(&cell_borders(&cell_props));
                        if let Some(fill) = cell_props.get("table-cell fo:background-color").and_then(color) {
                            let _ = write!(tcpr, r#"<w:shd w:val="clear" w:color="auto" w:fill="{fill}"/>"#);
                        }
                        let padding = |side: &str| {
                            cell_props
                                .pt(&format!("table-cell fo:padding-{side}"))
                                .or_else(|| cell_props.pt("table-cell fo:padding"))
                                .unwrap_or(2.85)
                        };
                        let _ = write!(
                            tcpr,
                            r#"<w:tcMar><w:top w:w="{}" w:type="dxa"/><w:left w:w="{}" w:type="dxa"/><w:bottom w:w="{}" w:type="dxa"/><w:right w:w="{}" w:type="dxa"/></w:tcMar>"#,
                            twips(padding("top")),
                            twips(padding("left")),
                            twips(padding("bottom")),
                            twips(padding("right"))
                        );
                        match cell_props.get("table-cell style:vertical-align") {
                            Some("middle") => tcpr.push_str(r#"<w:vAlign w:val="center"/>"#),
                            Some("bottom") => tcpr.push_str(r#"<w:vAlign w:val="bottom"/>"#),
                            _ => {}
                        }
                        let _ = write!(self.body, "<w:tc><w:tcPr>{tcpr}</w:tcPr>");
                        let before = self.body.len();
                        let (blocks, full, limit) = (self.blocks, self.full, self.limit.take());
                        self.blocks(cell, None);
                        (self.blocks, self.full, self.limit) = (blocks, full, limit);
                        if self.body.len() == before || self.body.ends_with("</w:tbl>") {
                            self.body.push_str("<w:p/>");
                        }
                        self.body.push_str("</w:tc>");
                        beside = column + span;
                        written = beside;
                        column += 1;
                    }
                }
                while written < widths.len() {
                    let _ = write!(
                        self.body,
                        r#"<w:tc><w:tcPr><w:tcW w:w="{}" w:type="dxa"/></w:tcPr><w:p/></w:tc>"#,
                        twips(widths[written])
                    );
                    written += 1;
                }
                self.body.push_str("</w:tr>");
            }
        }
        self.body.push_str("</w:tbl>");
        if let Some(limit) = self.limit {
            self.full |= self.chars >= limit.chars;
        }
    }

    fn section_settings(&mut self) -> String {
        let master = self.styles.master(self.section.as_deref());
        let layout = master.and_then(|master| {
            self.styles
                .layouts
                .get(attr(master, "style:page-layout-name")?)
                .copied()
        });
        let page = layout.and_then(|layout| child(layout, "style:page-layout-properties"));
        let get = |name| page.and_then(|page| attr(page, name)).and_then(pt);
        let (width, height) = (
            get("fo:page-width").unwrap_or(595.3),
            get("fo:page-height").unwrap_or(841.9),
        );
        let (mut top, mut bottom) = (
            get("fo:margin-top").unwrap_or(56.7),
            get("fo:margin-bottom").unwrap_or(56.7),
        );
        let (left, right) = (
            get("fo:margin-left").unwrap_or(56.7),
            get("fo:margin-right").unwrap_or(56.7),
        );
        let (header_at, footer_at) = (top, bottom);
        let mut references = String::new();
        for (kind, element) in [("header", "style:header"), ("footer", "style:footer")] {
            let Some(content) = master
                .and_then(|master| child(master, element))
                .filter(|content| attr(*content, "style:display") != Some("false"))
            else {
                continue;
            };
            let style = layout
                .and_then(|layout| child(layout, &format!("{element}-style")))
                .and_then(|style| child(style, "style:header-footer-properties"));
            let size = |name| style.and_then(|style| attr(style, name)).and_then(pt).unwrap_or(0.0);
            let lines = 14.0 * content.descendants().filter(|node| is(*node, "text:p")).count().max(1) as f64;
            if kind == "header" {
                top += size("fo:min-height").max(lines) + size("fo:margin-bottom");
            } else {
                bottom += size("fo:min-height").max(lines) + size("fo:margin-top");
            }
            let written = self.header(content, kind);
            let _ = write!(references, r#"<w:{kind}Reference w:type="default" r:id="{written}"/>"#);
        }
        let orientation = if width > height { r#" w:orient="landscape""# } else { "" };
        format!(
            r#"<w:sectPr>{references}<w:pgSz w:w="{}" w:h="{}"{orientation}/><w:pgMar w:top="{}" w:right="{}" w:bottom="{}" w:left="{}" w:header="{}" w:footer="{}" w:gutter="0"/></w:sectPr>"#,
            twips(width),
            twips(height),
            twips(top),
            twips(right),
            twips(bottom),
            twips(left),
            twips(header_at),
            twips(footer_at)
        )
    }

    fn header(&mut self, content: Node<'_, 'input>, kind: &str) -> String {
        let body = std::mem::take(&mut self.body);
        let part = std::mem::take(&mut self.part);
        let (blocks, full, limit, in_header, break_before) = (
            self.blocks,
            self.full,
            self.limit.take(),
            self.in_header,
            self.break_before,
        );
        self.in_header = true;
        self.break_before = false;
        self.blocks(content, None);
        let written = std::mem::replace(&mut self.body, body);
        let relationships = std::mem::replace(&mut self.part, part);
        (self.blocks, self.full, self.limit, self.in_header, self.break_before) =
            (blocks, full, limit, in_header, break_before);
        let name = format!("{kind}{}.xml", self.headers.len() + 1);
        let root = if kind == "header" { "hdr" } else { "ftr" };
        let written = if written.is_empty() {
            "<w:p/>".to_owned()
        } else {
            written
        };
        self.headers.push((
            name.clone(),
            format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:{root} {MAIN}>{written}</w:{root}>"#),
            relationships,
        ));
        self.relate(if kind == "header" { "header" } else { "footer" }, name)
    }

    fn finish(mut self) -> Option<Vec<u8>> {
        let settings = self.section_settings();
        let document = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {MAIN}><w:body>{}{settings}</w:body></w:document>"#,
            self.body
        );

        let defaults = self.styles.props("paragraph", None, true).text_only();
        let tab = self
            .styles
            .props("paragraph", None, true)
            .pt("paragraph style:tab-stop-distance")
            .filter(|distance| *distance > 0.0)
            .unwrap_or(35.45);
        let styles = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles {MAIN}><w:docDefaults><w:rPrDefault><w:rPr>{}</w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:before="0" w:after="0" w:line="240" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults></w:styles>"#,
            self.rpr(&defaults)
        );
        let settings = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:settings {MAIN}><w:defaultTabStop w:val="{}"/></w:settings>"#,
            twips(tab)
        );

        let mut parts: Vec<(String, Vec<u8>)> = vec![
            (
                "_rels/.rels".into(),
                relationships(&[("rId1".into(), "officeDocument", "word/document.xml".into())]),
            ),
            ("word/document.xml".into(), document.into_bytes()),
            ("word/styles.xml".into(), styles.into_bytes()),
            ("word/settings.xml".into(), settings.into_bytes()),
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
            (
                "word/settings.xml".to_owned(),
                "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml",
            ),
        ];
        self.relate("styles", "styles.xml".into());
        self.relate("settings", "settings.xml".into());
        if !self.notes.is_empty() {
            let mut notes = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:footnotes {MAIN}><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>"#
            );
            for (id, paragraphs) in &self.notes {
                let _ = write!(notes, r#"<w:footnote w:id="{id}">{paragraphs}</w:footnote>"#);
            }
            notes.push_str("</w:footnotes>");
            parts.push(("word/footnotes.xml".into(), notes.into_bytes()));
            overrides.push((
                "word/footnotes.xml".into(),
                "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml",
            ));
            self.relate("footnotes", "footnotes.xml".into());
        }
        for (name, xml, part) in std::mem::take(&mut self.headers) {
            let kind = if name.starts_with("header") { "header" } else { "footer" };
            overrides.push((
                format!("word/{name}"),
                if kind == "header" {
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"
                } else {
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"
                },
            ));
            if !part.relationships.is_empty() {
                parts.push((format!("word/_rels/{name}.rels"), relationships(&part.relationships)));
            }
            parts.push((format!("word/{name}"), xml.into_bytes()));
        }
        parts.push((
            "word/_rels/document.xml.rels".into(),
            relationships(&self.part.relationships),
        ));
        for (name, bytes) in std::mem::take(&mut self.media) {
            parts.push((format!("word/{name}"), bytes));
        }
        zip_of(&parts, &overrides)
    }
}

fn is_left_out(item: Node) -> bool {
    let name = item.tag_name().name();
    let namespace = item.tag_name().namespace();
    (namespace == super::namespace("office") && matches!(name, "annotation" | "annotation-end"))
        || (namespace == super::namespace("text")
            && matches!(
                name,
                "bookmark"
                    | "bookmark-start"
                    | "bookmark-end"
                    | "reference-mark"
                    | "reference-mark-start"
                    | "reference-mark-end"
                    | "change"
                    | "change-start"
                    | "change-end"
                    | "soft-page-break"
                    | "toc-mark"
                    | "alphabetical-index-mark"
                    | "user-index-mark"
            ))
}

pub(super) fn is_bold(weight: Option<&str>) -> bool {
    match weight {
        Some("bold" | "bolder") => true,
        Some(weight) => weight.parse::<u32>().is_ok_and(|weight| weight >= 600),
        None => false,
    }
}

fn word_border(value: &str) -> Option<String> {
    let (width, style, hex) = super::border(value)?;
    let style = match style {
        "double" => "double",
        "dotted" => "dotted",
        "dashed" => "dashed",
        _ => "single",
    };
    Some(format!(
        r#"w:val="{style}" w:sz="{}" w:space="0" w:color="{hex}""#,
        (width * 8.0).round().clamp(2.0, 96.0)
    ))
}

fn borders(props: &Props, kind: &str) -> Vec<(&'static str, String)> {
    ["top", "left", "bottom", "right"]
        .into_iter()
        .filter_map(|side| {
            let value = props
                .get(&format!("{kind} fo:border-{side}"))
                .or_else(|| props.get(&format!("{kind} fo:border")))?;
            Some((side, word_border(value)?))
        })
        .collect()
}

fn paragraph_borders(props: &Props) -> String {
    let found = borders(props, "paragraph");
    if found.is_empty() {
        return String::new();
    }
    let mut xml = String::from("<w:pBdr>");
    for (side, border) in found {
        let _ = write!(xml, "<w:{side} {border}/>");
    }
    xml.push_str("</w:pBdr>");
    xml
}

fn cell_borders(props: &Props) -> String {
    let found = borders(props, "table-cell");
    if found.is_empty() {
        return String::new();
    }
    let mut xml = String::from("<w:tcBorders>");
    for (side, border) in found {
        let _ = write!(xml, "<w:{side} {border}/>");
    }
    xml.push_str("</w:tcBorders>");
    xml
}
