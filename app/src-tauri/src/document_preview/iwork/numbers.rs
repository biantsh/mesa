use std::fmt::Write as _;

use crate::{
    document_preview::{
        opendocument::{escape, relationships, zip_of},
        workbook_page::{Cut, Cuts, Room},
    },
    iwork_read::{Book, shown},
};

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const TEXT: u32 = 0;
const NUMBER: u32 = 1;
const HEADER: u32 = 2;
const TITLE: u32 = 3;
const PROSE: u32 = 4;

pub(in super::super) fn xlsx(bytes: &[u8], room: Room) -> Option<(Vec<u8>, Cuts)> {
    let book = Book::from_bytes(bytes).ok()?;
    let tables = book.doc.tables();
    let mut sheets = Vec::new();
    let mut cuts = Vec::new();
    let mut cells_left = room.cells;
    let elements = book.doc.elements();
    for sheet in book.doc.sheets() {
        let mut on: Vec<(f32, f32, On)> = Vec::new();
        for id in &sheet.drawables {
            let Some(element) = elements.iter().find(|element| element.identifier.get() == *id) else {
                continue;
            };
            let at = (element.geometry.y, element.geometry.x);
            if let Some(table) = tables.iter().find(|table| table.identifier.get() == *id) {
                on.push((at.0, at.1, On::Table(table)));
            } else if let Some(storage) = element.text.filter(|_| element.kind == iwork::drawable::Kind::Shape) {
                on.push((at.0, at.1, On::Text(storage)));
            }
        }
        on.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let mut rows = String::new();
        let mut merges = Vec::new();
        let mut widths: Vec<f64> = Vec::new();
        let (mut row, mut kept) = (0usize, 0usize);
        let mut cut = Cut::default();
        for (n, (_, _, item)) in on.iter().enumerate() {
            if kept >= room.rows || cells_left == 0 {
                cut.rows.get_or_insert(row);
                break;
            }
            if n > 0 {
                row += 1;
            }
            let table = match item {
                On::Table(table) => *table,
                On::Text(storage) => {
                    for paragraph in book.paragraphs(*storage) {
                        let look = paragraph.runs.first().map(|run| book.look(paragraph.style, run.style));
                        let big = look
                            .as_ref()
                            .is_some_and(|look| look.size.unwrap_or(10.0) >= 14.0 || look.bold);
                        let capitals = look.as_ref().is_some_and(|look| look.caps == 1);
                        for line in paragraph.text().split(['\u{2028}', '\u{000B}']) {
                            let line = if capitals {
                                line.trim().to_uppercase()
                            } else {
                                line.trim().to_owned()
                            };
                            if line.is_empty() || kept >= room.rows {
                                continue;
                            }
                            row += 1;
                            kept += 1;
                            cells_left = cells_left.saturating_sub(1);
                            let _ = write!(
                                rows,
                                r#"<row r="{row}"><c r="A{row}" s="{}" t="inlineStr"><is><t xml:space="preserve">{}</t></is></c></row>"#,
                                if big { TITLE } else { PROSE },
                                escape(&line)
                            );
                        }
                    }
                    continue;
                }
            };
            row += 1;
            let _ = write!(
                rows,
                r#"<row r="{row}"><c r="A{row}" s="{TITLE}" t="inlineStr"><is><t xml:space="preserve">{}</t></is></c></row>"#,
                escape(&table.name)
            );
            kept += 1;
            let columns = table.columns.min(room.columns);
            if table.columns > room.columns {
                cut.columns = true;
            }
            for column in 0..columns {
                let width = table.column_width(column);
                if widths.len() <= column {
                    widths.push(width);
                } else {
                    widths[column] = widths[column].max(width);
                }
            }
            let first = row + 1;
            for at in 0..table.rows {
                if kept >= room.rows || cells_left == 0 {
                    cut.rows.get_or_insert(row);
                    break;
                }
                row += 1;
                kept += 1;
                let header_row = at < table.header_rows as usize;
                let _ = write!(rows, r#"<row r="{row}">"#);
                for column in 0..columns {
                    let reference = format!("{}{row}", column_name(column));
                    let cell = table.cell(at, column);
                    let text = cell.map_or_else(String::new, |cell| shown(&cell.value, &cell.format));
                    let number = cell.is_some_and(|cell| {
                        matches!(
                            cell.value,
                            iwork::CellValue::Number(_)
                                | iwork::CellValue::Currency(_)
                                | iwork::CellValue::Date(_)
                                | iwork::CellValue::Duration(_)
                        )
                    });
                    let style = if header_row || column < table.header_columns as usize {
                        HEADER
                    } else if number {
                        NUMBER
                    } else {
                        TEXT
                    };
                    if text.is_empty() {
                        let _ = write!(rows, r#"<c r="{reference}" s="{style}"/>"#);
                    } else {
                        cells_left = cells_left.saturating_sub(1);
                        let _ = write!(
                            rows,
                            r#"<c r="{reference}" s="{style}" t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#,
                            escape(&text)
                        );
                    }
                    if let Some(merge) = table.merge_at(at, column)
                        && (merge.rows > 1 || merge.columns > 1)
                    {
                        let last_column = (column + merge.columns - 1).min(columns.saturating_sub(1));
                        merges.push(format!(
                            "{reference}:{}{}",
                            column_name(last_column),
                            first + at + merge.rows - 1
                        ));
                    }
                }
                rows.push_str("</row>");
            }
        }
        sheets.push((sheet.name.clone(), rows, merges, widths));
        cuts.push((sheet.name.clone(), cut));
    }
    if sheets.is_empty() {
        return None;
    }
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
    let mut names: Vec<String> = Vec::new();
    for (n, (name, rows, merges, widths)) in sheets.iter().enumerate() {
        let number = n + 1;
        let mut fit: String = name
            .chars()
            .filter(|c| !matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
            .take(31)
            .collect();
        if fit.trim().is_empty() || names.iter().any(|known| known.eq_ignore_ascii_case(&fit)) {
            fit = format!("Sheet{number}");
        }
        names.push(fit.clone());
        let _ = write!(
            listed,
            r#"<sheet name="{}" sheetId="{number}" r:id="rId{number}"/>"#,
            escape(&fit)
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
        let mut xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="{MAIN}"><sheetPr><pageSetUpPr fitToPage="1"/></sheetPr>"#
        );
        if !widths.is_empty() {
            xml.push_str("<cols>");
            for (column, width) in widths.iter().enumerate() {
                let characters = ((width / 0.75 - 5.0) / 7.0).max(1.0);
                let _ = write!(
                    xml,
                    r#"<col min="{0}" max="{0}" width="{characters:.2}" customWidth="1"/>"#,
                    column + 1
                );
            }
            xml.push_str("</cols>");
        }
        let _ = write!(xml, "<sheetData>{rows}</sheetData>");
        if !merges.is_empty() {
            let _ = write!(xml, r#"<mergeCells count="{}">"#, merges.len());
            for merge in merges {
                let _ = write!(xml, r#"<mergeCell ref="{merge}"/>"#);
            }
            xml.push_str("</mergeCells>");
        }
        xml.push_str(r#"<pageMargins left="0.5" right="0.5" top="0.75" bottom="0.75" header="0.3" footer="0.3"/><pageSetup fitToWidth="1" fitToHeight="0"/></worksheet>"#);
        parts.push((format!("xl/worksheets/sheet{number}.xml"), xml.into_bytes()));
    }
    let rel = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    parts.push((
        "xl/workbook.xml".into(),
        format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="{MAIN}" xmlns:r="{rel}"><sheets>{listed}</sheets></workbook>"#).into_bytes(),
    ));
    parts.push((
        "xl/_rels/workbook.xml.rels".into(),
        relationships(&workbook_relationships),
    ));
    let grey = r#"<left style="thin"><color rgb="FFC8C8C8"/></left><right style="thin"><color rgb="FFC8C8C8"/></right><top style="thin"><color rgb="FFC8C8C8"/></top><bottom style="thin"><color rgb="FFC8C8C8"/></bottom><diagonal/>"#;
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><styleSheet xmlns="{MAIN}"><fonts count="3"><font><sz val="10"/><name val="Helvetica Neue"/></font><font><b/><sz val="10"/><name val="Helvetica Neue"/></font><font><b/><sz val="12"/><name val="Helvetica Neue"/></font></fonts><fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFEFEFEF"/><bgColor indexed="64"/></patternFill></fill></fills><borders count="2"><border><left/><right/><top/><bottom/><diagonal/></border><border>{grey}</border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="5"><xf numFmtId="0" fontId="0" fillId="0" borderId="1" xfId="0" applyBorder="1"/><xf numFmtId="0" fontId="0" fillId="0" borderId="1" xfId="0" applyBorder="1" applyAlignment="1"><alignment horizontal="right"/></xf><xf numFmtId="0" fontId="1" fillId="2" borderId="1" xfId="0" applyFont="1" applyFill="1" applyBorder="1"/><xf numFmtId="0" fontId="2" fillId="0" borderId="0" xfId="0" applyFont="1"/><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#
    );
    parts.push(("xl/styles.xml".into(), styles.into_bytes()));
    Some((zip_of(&parts, &overrides)?, cuts))
}

enum On<'t> {
    Table(&'t iwork::TableInfo),
    Text(u64),
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
