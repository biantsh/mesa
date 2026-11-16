use std::{
    collections::HashMap,
    fmt::Write as _,
    io::{BufRead, BufReader, Read},
};

use quick_xml::{
    Reader, XmlVersion,
    events::{BytesStart, Event},
};

use super::{Props, Styles, attr, child, color, escape, is, parse, pt, relationships, zip_of};
use crate::document_preview::workbook_page::{Cut, Cuts, Room};

const MAX_CONTENT_BYTES: u64 = 512 * 1024 * 1024;
const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

pub(in super::super) fn xlsx(bytes: &[u8], room: Room) -> Option<(Vec<u8>, Cuts)> {
    let mut package = super::Package::open(bytes)?;
    let styles_xml = package.text("styles.xml");
    let head = head_of(&mut package)?;
    let content_head = parse(&head)?;
    let styles_document = styles_xml.as_deref().and_then(parse);
    let styles = Styles::of(styles_document.as_ref(), &content_head);

    let file = package.archive.by_name("content.xml").ok()?;
    let mut reader = Reader::from_reader(BufReader::new(file.take(MAX_CONTENT_BYTES)));
    let mut book = Book {
        styles: &styles,
        formats: Formats::new(&styles),
        sheets: Vec::new(),
        cells_left: room.cells,
        room,
    };
    book.read(&mut reader)?;
    let cuts = book
        .sheets
        .iter()
        .map(|sheet| (sheet.name.clone(), sheet.cut.clone()))
        .collect();
    Some((book.finish()?, cuts))
}

fn head_of(package: &mut super::Package) -> Option<String> {
    let file = package.archive.by_name("content.xml").ok()?;
    let mut reader = BufReader::new(file.take(super::MAX_XML_BYTES));
    let mut head = Vec::new();
    let mut body = false;
    loop {
        let read = reader.fill_buf().ok()?;
        if read.is_empty() {
            break;
        }
        let length = read.len();
        let from = head.len().saturating_sub(16);
        head.extend_from_slice(read);
        reader.consume(length);
        if let Some(at) = head[from..].windows(12).position(|window| window == b"<office:body") {
            head.truncate(from + at);
            body = true;
            break;
        }
    }
    let mut head = String::from_utf8(head).ok()?;
    if body {
        let root = head.find("<office:document-content")?;
        let name_end = root + head[root..].find(|c: char| c.is_whitespace() || c == '>')?;
        let name = head[root + 1..name_end].to_owned();
        let _ = write!(head, "</{name}>");
    }
    Some(head)
}

struct Sheet {
    name: String,
    master: Option<String>,
    columns: Vec<(Option<f64>, bool)>,
    rows: String,
    kept: usize,
    merges: Vec<String>,
    cut: Cut,
}

#[derive(Default)]
struct Formats {
    fonts: Vec<String>,
    fills: Vec<String>,
    borders: Vec<String>,
    cells: Vec<String>,
    by_style: HashMap<(String, bool), usize>,
}

impl Formats {
    fn new(styles: &Styles) -> Self {
        let mut formats = Self::default();
        formats
            .fills
            .push(r#"<fill><patternFill patternType="none"/></fill>"#.into());
        formats
            .fills
            .push(r#"<fill><patternFill patternType="gray125"/></fill>"#.into());
        formats
            .borders
            .push("<border><left/><right/><top/><bottom/><diagonal/></border>".into());
        formats.of(styles, "Default", false);
        formats
    }

    fn index_of(list: &mut Vec<String>, xml: String) -> usize {
        match list.iter().position(|known| *known == xml) {
            Some(at) => at,
            None => {
                list.push(xml);
                list.len() - 1
            }
        }
    }

    fn of(&mut self, styles: &Styles, name: &str, number: bool) -> usize {
        if let Some(&known) = self.by_style.get(&(name.to_owned(), number)) {
            return known;
        }
        let props = styles.props("table-cell", Some(name), true);
        let font = self.font(styles, &props);
        let fill = match props.get("table-cell fo:background-color").and_then(color) {
            Some(hex) => Self::index_of(
                &mut self.fills,
                format!(
                    r#"<fill><patternFill patternType="solid"><fgColor rgb="FF{hex}"/><bgColor indexed="64"/></patternFill></fill>"#
                ),
            ),
            None => 0,
        };
        let border = self.border(&props);
        let horizontal = match props.get("paragraph fo:text-align") {
            _ if props.get("table-cell style:text-align-source") != Some("fix") && number => Some("right"),
            Some("center") => Some("center"),
            Some("end" | "right") => Some("right"),
            Some("justify") => Some("justify"),
            Some("start" | "left") => Some("left"),
            _ if number => Some("right"),
            _ => None,
        };
        let vertical = match props.get("table-cell style:vertical-align") {
            Some("top") => Some("top"),
            Some("middle") => Some("center"),
            _ => None,
        };
        let wrap = props.get("table-cell fo:wrap-option") == Some("wrap");
        let mut alignment = String::new();
        if horizontal.is_some() || vertical.is_some() || wrap {
            alignment.push_str("<alignment");
            if let Some(horizontal) = horizontal {
                let _ = write!(alignment, r#" horizontal="{horizontal}""#);
            }
            if let Some(vertical) = vertical {
                let _ = write!(alignment, r#" vertical="{vertical}""#);
            }
            if wrap {
                alignment.push_str(r#" wrapText="1""#);
            }
            alignment.push_str("/>");
        }
        let xml = format!(
            r#"<xf numFmtId="0" fontId="{font}" fillId="{fill}" borderId="{border}" xfId="0" applyFont="1" applyFill="1" applyBorder="1" applyAlignment="1">{alignment}</xf>"#
        );
        let index = Self::index_of(&mut self.cells, xml);
        self.by_style.insert((name.to_owned(), number), index);
        index
    }

    fn font(&mut self, styles: &Styles, props: &Props) -> usize {
        let mut xml = String::from("<font>");
        if super::text::is_bold(props.get("text fo:font-weight")) {
            xml.push_str("<b/>");
        }
        if matches!(props.get("text fo:font-style"), Some("italic" | "oblique")) {
            xml.push_str("<i/>");
        }
        if props
            .get("text style:text-line-through-style")
            .is_some_and(|style| style != "none")
        {
            xml.push_str("<strike/>");
        }
        if props
            .get("text style:text-underline-style")
            .is_some_and(|style| style != "none")
        {
            xml.push_str("<u/>");
        }
        let _ = write!(xml, r#"<sz val="{}"/>"#, props.pt("text fo:font-size").unwrap_or(10.0));
        if props.get("text style:use-window-font-color") != Some("true")
            && let Some(hex) = props.get("text fo:color").and_then(color)
        {
            let _ = write!(xml, r#"<color rgb="FF{hex}"/>"#);
        }
        let _ = write!(
            xml,
            r#"<name val="{}"/></font>"#,
            escape(&styles.fonts_of(props).0.unwrap_or_else(|| "Liberation Sans".into()))
        );
        Self::index_of(&mut self.fonts, xml)
    }

    fn border(&mut self, props: &Props) -> usize {
        let mut xml = String::from("<border>");
        let mut any = false;
        for side in ["left", "right", "top", "bottom"] {
            let value = props
                .get(&format!("table-cell fo:border-{side}"))
                .or_else(|| props.get("table-cell fo:border"));
            match value.and_then(super::border) {
                Some((width, style, hex)) => {
                    any = true;
                    let style = match style {
                        "double" => "double",
                        "dotted" => "dotted",
                        "dashed" => "dashed",
                        _ if width >= 2.0 => "thick",
                        _ if width >= 1.0 => "medium",
                        _ => "thin",
                    };
                    let _ = write!(xml, r#"<{side} style="{style}"><color rgb="FF{hex}"/></{side}>"#);
                }
                None => {
                    let _ = write!(xml, "<{side}/>");
                }
            }
        }
        xml.push_str("<diagonal/></border>");
        if any { Self::index_of(&mut self.borders, xml) } else { 0 }
    }
}

fn attribute(tag: &BytesStart, name: &str) -> Option<String> {
    tag.attributes()
        .with_checks(false)
        .flatten()
        .find(|attribute| attribute.key.as_ref() == name)
        .and_then(|attribute| {
            attribute
                .normalized_value(XmlVersion::Implicit1_0)
                .ok()
                .map(|value| value.into_owned())
        })
}

fn count(tag: &BytesStart, name: &str) -> usize {
    attribute(tag, name)
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(1)
        .max(1)
}

#[derive(Default)]
struct RowRead {
    repeats: usize,
    cells: Vec<(usize, CellRead)>,
    style: Option<String>,
    height: Option<f64>,
    hidden: bool,
}

#[derive(Default)]
struct CellRead {
    repeats: usize,
    columns: usize,
    rows: usize,
    style: Option<String>,
    number: bool,
    value: Option<String>,
    text: String,
    paragraphs: usize,
    space: bool,
    skipped: usize,
    covered: bool,
}

struct Book<'s, 'a, 'input> {
    styles: &'s Styles<'a, 'input>,
    formats: Formats,
    sheets: Vec<Sheet>,
    cells_left: usize,
    room: Room,
}

impl Book<'_, '_, '_> {
    fn read<R: BufRead>(&mut self, reader: &mut Reader<R>) -> Option<()> {
        let mut buffer = Vec::new();
        let mut sheet: Option<Sheet> = None;
        let mut depth_in_table = 0usize;
        let mut row_index = 0usize;
        let mut column_styles: Vec<Option<String>> = Vec::new();
        let mut row: Option<RowRead> = None;
        let mut column = 0usize;
        let mut cell: Option<CellRead> = None;
        let mut over = false;
        loop {
            let event = reader.read_event_into(&mut buffer).ok()?;
            match &event {
                Event::Eof => break,
                Event::Start(tag) if tag.name().as_ref() == "table:table" => {
                    depth_in_table += 1;
                    if depth_in_table == 1 {
                        let style = attribute(tag, "table:style-name");
                        let hidden = self
                            .styles
                            .props("table", style.as_deref(), false)
                            .get("table table:display")
                            == Some("false");
                        let master = style
                            .as_deref()
                            .and_then(|style| self.styles.style("table", style))
                            .and_then(|style| attr(style, "style:master-page-name"))
                            .map(str::to_owned);
                        sheet = (!hidden).then(|| Sheet {
                            name: attribute(tag, "table:name").unwrap_or_default(),
                            master,
                            columns: Vec::new(),
                            rows: String::new(),
                            kept: 0,
                            merges: Vec::new(),
                            cut: Cut::default(),
                        });
                        (row_index, column_styles, over) = (0, Vec::new(), self.cells_left == 0);
                    }
                }
                Event::End(tag) if tag.name().as_ref() == "table:table" => {
                    depth_in_table = depth_in_table.saturating_sub(1);
                    if depth_in_table == 0
                        && let Some(done) = sheet.take()
                    {
                        self.sheets.push(done);
                    }
                }
                _ if depth_in_table != 1 || sheet.is_none() => {}
                Event::Start(tag) | Event::Empty(tag) if tag.name().as_ref() == "table:table-column" => {
                    let Some(sheet) = sheet.as_mut() else { continue };
                    let repeats = count(tag, "table:number-columns-repeated");
                    let style = attribute(tag, "table:style-name");
                    let width = self
                        .styles
                        .props("table-column", style.as_deref(), false)
                        .pt("table-column style:column-width");
                    let hidden = attribute(tag, "table:visibility").is_some_and(|visibility| visibility != "visible");
                    let default = attribute(tag, "table:default-cell-style-name");
                    let room = self.room.columns.saturating_sub(sheet.columns.len());
                    for _ in 0..repeats.min(room) {
                        sheet.columns.push((width, hidden));
                        column_styles.push(default.clone());
                    }
                }
                Event::Start(tag) | Event::Empty(tag) if tag.name().as_ref() == "table:table-row" => {
                    let style = attribute(tag, "table:style-name");
                    let props = self.styles.props("table-row", style.as_deref(), false);
                    let height = (props.get("table-row style:use-optimal-row-height") == Some("false"))
                        .then(|| props.pt("table-row style:row-height"))
                        .flatten();
                    let hidden = attribute(tag, "table:visibility").is_some_and(|visibility| visibility != "visible");
                    row = Some(RowRead {
                        repeats: count(tag, "table:number-rows-repeated"),
                        cells: Vec::new(),
                        style: attribute(tag, "table:default-cell-style-name"),
                        height,
                        hidden,
                    });
                    column = 0;
                    if matches!(event, Event::Empty(_)) {
                        row_index += row.take().map_or(1, |row| row.repeats);
                    }
                }
                Event::End(tag) if tag.name().as_ref() == "table:table-row" => {
                    let Some(read) = &row else { continue };
                    let (repeats, has) = (read.repeats, read.cells.iter().any(|(_, cell)| !cell.text.is_empty()));
                    if has {
                        if over {
                            if let Some(sheet) = sheet.as_mut() {
                                sheet.cut.rows.get_or_insert(last_row(sheet));
                            }
                        } else if let (Some(sheet), Some(row)) = (sheet.as_mut(), row.as_ref()) {
                            over = self.write_row(sheet, row, row_index);
                        }
                    }
                    row_index += repeats;
                    row = None;
                }
                Event::Start(tag) | Event::Empty(tag)
                    if matches!(tag.name().as_ref(), "table:table-cell" | "table:covered-table-cell")
                        && row.is_some()
                        && cell.is_none() =>
                {
                    let value_type = attribute(tag, "office:value-type");
                    let number = matches!(
                        value_type.as_deref(),
                        Some("float" | "percentage" | "currency" | "date" | "time")
                    );
                    let value = match value_type.as_deref() {
                        Some("date") => attribute(tag, "office:date-value"),
                        Some("time") => attribute(tag, "office:time-value"),
                        Some("boolean") => {
                            attribute(tag, "office:boolean-value").map(|value| value.to_ascii_uppercase())
                        }
                        Some("string") => attribute(tag, "office:string-value"),
                        Some(_) => attribute(tag, "office:value"),
                        None => None,
                    };
                    let style = attribute(tag, "table:style-name")
                        .or_else(|| row.as_ref().and_then(|row| row.style.clone()))
                        .or_else(|| column_styles.get(column).cloned().flatten());
                    let read = CellRead {
                        repeats: count(tag, "table:number-columns-repeated"),
                        columns: count(tag, "table:number-columns-spanned"),
                        rows: count(tag, "table:number-rows-spanned"),
                        style,
                        number,
                        value,
                        covered: tag.name().as_ref() == "table:covered-table-cell",
                        space: true,
                        ..CellRead::default()
                    };
                    if matches!(event, Event::Empty(_)) {
                        self.end_cell(read, &mut row, &mut column, &mut sheet);
                    } else {
                        cell = Some(read);
                    }
                }
                Event::End(tag)
                    if matches!(tag.name().as_ref(), "table:table-cell" | "table:covered-table-cell")
                        && cell.as_ref().is_some_and(|cell| cell.skipped == 0) =>
                {
                    if let Some(read) = cell.take() {
                        self.end_cell(read, &mut row, &mut column, &mut sheet);
                    }
                }
                _ if cell.is_some() => {
                    if let Some(cell) = cell.as_mut() {
                        read_text(cell, &event);
                    }
                }
                _ => {}
            }
            buffer.clear();
        }
        Some(())
    }

    fn end_cell(
        &mut self,
        mut read: CellRead,
        row: &mut Option<RowRead>,
        column: &mut usize,
        sheet: &mut Option<Sheet>,
    ) {
        let at = *column;
        *column += read.repeats;
        if read.covered {
            return;
        }
        let text = read.text.trim_end_matches(char::is_whitespace).to_owned();
        let text = if text.is_empty() {
            read.value.take().unwrap_or_default()
        } else {
            text
        };
        if text.is_empty() && read.style.is_none() && read.columns == 1 && read.rows == 1 {
            return;
        }
        let (Some(RowRead { cells, .. }), Some(sheet)) = (row.as_mut(), sheet.as_mut()) else {
            return;
        };
        for repeat in 0..read.repeats {
            let place = at + repeat;
            if place >= self.room.columns {
                if !text.is_empty() {
                    sheet.cut.columns = true;
                }
                break;
            }
            cells.push((
                place,
                CellRead {
                    text: text.clone(),
                    style: read.style.clone(),
                    number: read.number,
                    columns: read.columns,
                    rows: read.rows,
                    ..CellRead::default()
                },
            ));
        }
    }

    fn write_row(&mut self, sheet: &mut Sheet, row: &RowRead, index: usize) -> bool {
        let RowRead {
            repeats,
            cells,
            height,
            hidden,
            ..
        } = row;
        let with_text = cells.iter().any(|(_, cell)| !cell.text.is_empty());
        for repeat in 0..*repeats {
            if sheet.kept >= self.room.rows || self.cells_left == 0 {
                if with_text {
                    sheet.cut.rows.get_or_insert(index + repeat);
                }
                return true;
            }
            let number = index + repeat + 1;
            let _ = write!(sheet.rows, r#"<row r="{number}""#);
            if let Some(height) = height {
                let _ = write!(sheet.rows, r#" ht="{height:.2}" customHeight="1""#);
            }
            if *hidden {
                sheet.rows.push_str(r#" hidden="1""#);
            }
            sheet.rows.push('>');
            for (place, cell) in cells {
                if self.cells_left == 0 {
                    if cells
                        .iter()
                        .any(|(later, cell)| later >= place && !cell.text.is_empty())
                    {
                        sheet.cut.rows.get_or_insert(number);
                    }
                    break;
                }
                let reference = format!("{}{number}", column_name(*place));
                let style = cell
                    .style
                    .as_deref()
                    .map_or(0, |style| self.formats.of(self.styles, style, cell.number));
                if cell.text.is_empty() {
                    let _ = write!(sheet.rows, r#"<c r="{reference}" s="{style}"/>"#);
                } else {
                    self.cells_left -= 1;
                    let _ = write!(
                        sheet.rows,
                        r#"<c r="{reference}" s="{style}" t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#,
                        escape(&cell.text)
                    );
                }
                if repeat == 0 && (cell.columns > 1 || cell.rows > 1) {
                    let last_column = (place + cell.columns - 1).min(self.room.columns - 1);
                    sheet.merges.push(format!(
                        "{reference}:{}{}",
                        column_name(last_column),
                        number + cell.rows - 1
                    ));
                }
            }
            sheet.rows.push_str("</row>");
            sheet.kept += 1;
        }
        sheet.kept >= self.room.rows || self.cells_left == 0
    }

    fn finish(mut self) -> Option<Vec<u8>> {
        let rel = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
        let mut parts: Vec<(String, Vec<u8>)> = vec![(
            "_rels/.rels".into(),
            relationships(&[("rId1".into(), "officeDocument", "xl/workbook.xml".into())]),
        )];
        let mut overrides = vec![
            (
                "xl/workbook.xml".to_owned(),
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",
            ),
            (
                "xl/styles.xml".to_owned(),
                "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml",
            ),
        ];
        let mut listed = String::new();
        let mut workbook_relationships = vec![("rIdStyles".to_owned(), "styles", "styles.xml".to_owned())];
        if self.sheets.is_empty() {
            self.sheets.push(Sheet {
                name: "Sheet1".into(),
                master: None,
                columns: Vec::new(),
                rows: String::new(),
                kept: 0,
                merges: Vec::new(),
                cut: Cut::default(),
            });
        }
        let mut names: Vec<String> = Vec::new();
        for (n, sheet) in self.sheets.iter().enumerate() {
            let number = n + 1;
            let mut name: String = sheet
                .name
                .chars()
                .filter(|c| !matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
                .take(31)
                .collect();
            if name.trim().is_empty() || names.iter().any(|known| known.eq_ignore_ascii_case(&name)) {
                name = format!("Sheet{number}");
            }
            names.push(name.clone());
            let _ = write!(
                listed,
                r#"<sheet name="{}" sheetId="{number}" r:id="rId{number}"/>"#,
                escape(&name)
            );
            workbook_relationships.push((
                format!("rId{number}"),
                "worksheet",
                format!("worksheets/sheet{number}.xml"),
            ));
            overrides.push((
                format!("xl/worksheets/sheet{number}.xml"),
                "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml",
            ));
            parts.push((
                format!("xl/worksheets/sheet{number}.xml"),
                self.sheet_xml(sheet).into_bytes(),
            ));
        }
        parts.push((
            "xl/workbook.xml".into(),
            format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="{MAIN}" xmlns:r="{rel}"><sheets>{listed}</sheets></workbook>"#).into_bytes(),
        ));
        parts.push((
            "xl/_rels/workbook.xml.rels".into(),
            relationships(&workbook_relationships),
        ));
        parts.push(("xl/styles.xml".into(), self.styles_xml().into_bytes()));
        zip_of(&parts, &overrides)
    }

    fn sheet_xml(&self, sheet: &Sheet) -> String {
        let mut xml = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="{MAIN}">"#);
        if !sheet.columns.is_empty() {
            xml.push_str("<cols>");
            for (n, (width, hidden)) in sheet.columns.iter().enumerate() {
                let characters = width.map_or(10.71, |width| ((width / 0.75 - 5.0) / 7.0).max(0.0));
                let _ = write!(
                    xml,
                    r#"<col min="{0}" max="{0}" width="{characters:.2}" customWidth="1"{1}/>"#,
                    n + 1,
                    if *hidden { r#" hidden="1""# } else { "" }
                );
            }
            xml.push_str("</cols>");
        }
        let _ = write!(xml, "<sheetData>{}</sheetData>", sheet.rows);
        if !sheet.merges.is_empty() {
            let _ = write!(xml, r#"<mergeCells count="{}">"#, sheet.merges.len());
            for merge in &sheet.merges {
                let _ = write!(xml, r#"<mergeCell ref="{merge}"/>"#);
            }
            xml.push_str("</mergeCells>");
        }
        let master = self.styles.master(sheet.master.as_deref().or(Some("Default")));
        let page = master
            .and_then(|master| {
                self.styles
                    .layouts
                    .get(attr(master, "style:page-layout-name")?)
                    .copied()
            })
            .and_then(|layout| child(layout, "style:page-layout-properties"));
        let get = |name| page.and_then(|page| attr(page, name)).and_then(pt);
        if page
            .and_then(|page| attr(page, "style:print"))
            .is_some_and(|print| print.split_whitespace().any(|what| what == "grid"))
        {
            xml.push_str(r#"<printOptions gridLines="1"/>"#);
        }
        let inches = |points: Option<f64>, default: f64| points.map_or(default, |points| points / 72.0);
        let header_height = |element: &str| {
            master
                .and_then(|master| child(master, element))
                .filter(|part| attr(*part, "style:display") != Some("false"))
                .map(|_| 0.3)
                .unwrap_or(0.0)
        };
        let _ = write!(
            xml,
            r#"<pageMargins left="{:.3}" right="{:.3}" top="{:.3}" bottom="{:.3}" header="{:.3}" footer="{:.3}"/>"#,
            inches(get("fo:margin-left"), 0.79),
            inches(get("fo:margin-right"), 0.79),
            inches(get("fo:margin-top"), 0.79) + header_height("style:header"),
            inches(get("fo:margin-bottom"), 0.79) + header_height("style:footer"),
            inches(get("fo:margin-top"), 0.79),
            inches(get("fo:margin-bottom"), 0.79),
        );
        let (width, height) = (
            get("fo:page-width").unwrap_or(595.3),
            get("fo:page-height").unwrap_or(841.9),
        );
        let (short, long) = (width.min(height), width.max(height));
        let paper = [
            (1, 612.0, 792.0),
            (5, 612.0, 1008.0),
            (8, 841.9, 1190.6),
            (9, 595.3, 841.9),
            (11, 419.5, 595.3),
        ]
        .into_iter()
        .find(|(_, w, h)| (short - w).abs() < 6.0 && (long - h).abs() < 6.0)
        .map_or(9, |(paper, ..)| paper);
        let orientation = if width > height { "landscape" } else { "portrait" };
        let scale = page
            .and_then(|page| attr(page, "style:scale-to"))
            .and_then(|scale| scale.trim_end_matches('%').parse::<f64>().ok())
            .map(|scale| format!(r#" scale="{}""#, scale.round().clamp(10.0, 400.0)))
            .unwrap_or_default();
        let _ = write!(
            xml,
            r#"<pageSetup paperSize="{paper}" orientation="{orientation}"{scale}/>"#
        );
        let header = master.and_then(|master| header_footer(master, "style:header"));
        let footer = master.and_then(|master| header_footer(master, "style:footer"));
        if header.is_some() || footer.is_some() {
            xml.push_str("<headerFooter>");
            if let Some(header) = header {
                let _ = write!(xml, "<oddHeader>{}</oddHeader>", escape(&header));
            }
            if let Some(footer) = footer {
                let _ = write!(xml, "<oddFooter>{}</oddFooter>", escape(&footer));
            }
            xml.push_str("</headerFooter>");
        }
        xml.push_str("</worksheet>");
        xml
    }

    fn styles_xml(&self) -> String {
        let mut xml = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><styleSheet xmlns="{MAIN}">"#);
        let _ = write!(
            xml,
            r#"<fonts count="{}">{}</fonts>"#,
            self.formats.fonts.len(),
            self.formats.fonts.concat()
        );
        let _ = write!(
            xml,
            r#"<fills count="{}">{}</fills>"#,
            self.formats.fills.len(),
            self.formats.fills.concat()
        );
        let _ = write!(
            xml,
            r#"<borders count="{}">{}</borders>"#,
            self.formats.borders.len(),
            self.formats.borders.concat()
        );
        xml.push_str(r#"<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>"#);
        let _ = write!(
            xml,
            r#"<cellXfs count="{}">{}</cellXfs>"#,
            self.formats.cells.len(),
            self.formats.cells.concat()
        );
        xml.push_str(
            r#"<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#,
        );
        xml
    }
}

fn last_row(sheet: &Sheet) -> usize {
    sheet
        .rows
        .rfind("<row r=\"")
        .and_then(|at| sheet.rows[at + 8..].split('"').next()?.parse().ok())
        .unwrap_or(0)
}

fn read_text(cell: &mut CellRead, event: &Event) {
    match event {
        Event::Start(tag) => {
            let name = tag.name();
            if cell.skipped > 0
                || matches!(
                    name.as_ref(),
                    "office:annotation" | "text:note" | "draw:frame" | "draw:custom-shape"
                )
            {
                cell.skipped += 1;
            } else if name.as_ref() == "text:p" {
                if cell.paragraphs > 0 {
                    cell.text.push('\n');
                }
                cell.paragraphs += 1;
                cell.space = true;
            }
        }
        Event::End(_) if cell.skipped > 0 => cell.skipped -= 1,
        Event::Empty(tag) if cell.skipped == 0 => match tag.name().as_ref() {
            "text:s" => {
                cell.text.extend(std::iter::repeat_n(' ', count(tag, "text:c").min(64)));
                cell.space = true;
            }
            "text:tab" => cell.text.push('\t'),
            "text:line-break" => cell.text.push('\n'),
            "text:p" => {
                if cell.paragraphs > 0 {
                    cell.text.push('\n');
                }
                cell.paragraphs += 1;
            }
            _ => {}
        },
        Event::Text(content) if cell.skipped == 0 => {
            for c in content.xml10_content().chars() {
                if !c.is_whitespace() {
                    cell.text.push(c);
                    cell.space = false;
                } else if !cell.space {
                    cell.text.push(' ');
                    cell.space = true;
                }
            }
        }
        Event::GeneralRef(reference) if cell.skipped == 0 => {
            cell.text.extend(crate::embed::entity(reference));
            cell.space = false;
        }
        _ => {}
    }
}

fn column_name(column: usize) -> String {
    let mut name = Vec::new();
    let mut n = column + 1;
    while n > 0 {
        n -= 1;
        name.push(b'A' + (n % 26) as u8);
        n /= 26;
    }
    name.reverse();
    String::from_utf8(name).unwrap_or_default()
}

fn header_footer(master: roxmltree::Node, element: &str) -> Option<String> {
    let part = child(master, element).filter(|part| attr(*part, "style:display") != Some("false"))?;
    let text_of = |node: roxmltree::Node| {
        let mut text = String::new();
        for paragraph in node.descendants().filter(|node| is(*node, "text:p")) {
            if !text.is_empty() {
                text.push('\n');
            }
            for piece in paragraph.descendants() {
                if piece.is_text()
                    && piece.parent().is_some_and(|parent| {
                        !is(parent, "text:sheet-name")
                            && !is(parent, "text:page-number")
                            && !is(parent, "text:page-count")
                            && !is(parent, "text:date")
                            && !is(parent, "text:time")
                            && !is(parent, "text:title")
                            && !is(parent, "text:file-name")
                    })
                {
                    text.push_str(&piece.text().unwrap_or("").replace('&', "&&"));
                } else if is(piece, "text:sheet-name") {
                    text.push_str("&A");
                } else if is(piece, "text:page-number") {
                    text.push_str("&P");
                } else if is(piece, "text:page-count") {
                    text.push_str("&N");
                } else if is(piece, "text:date") {
                    text.push_str("&D");
                } else if is(piece, "text:time") {
                    text.push_str("&T");
                } else if is(piece, "text:title") || is(piece, "text:file-name") {
                    text.push_str("&F");
                } else if is(piece, "text:s") {
                    text.push(' ');
                }
            }
        }
        text
    };
    let mut written = String::new();
    let regions = [
        ("style:region-left", "&L"),
        ("style:region-center", "&C"),
        ("style:region-right", "&R"),
    ];
    if regions.iter().any(|(region, _)| child(part, region).is_some()) {
        for (region, code) in regions {
            if let Some(region) = child(part, region) {
                let text = text_of(region);
                if !text.is_empty() {
                    written.push_str(code);
                    written.push_str(&text);
                }
            }
        }
    } else {
        let text = text_of(part);
        if !text.is_empty() {
            written = format!("&C{text}");
        }
    }
    (!written.is_empty()).then_some(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_names_past_z() {
        assert_eq!(column_name(0), "A");
        assert_eq!(column_name(25), "Z");
        assert_eq!(column_name(26), "AA");
        assert_eq!(column_name(701), "ZZ");
        assert_eq!(column_name(702), "AAA");
    }
}
