use std::{
    fs::File,
    io::{BufReader, Cursor, Read},
    path::Path,
    sync::Arc,
};

use image::{ImageFormat, imageops::FilterType};

mod epub;
mod iwork;
mod opendocument;

const MAX_THUMBNAIL_BYTES: u64 = 8 * 1024 * 1024;
const SAVED_THUMBNAILS: [&str; 5] = [
    "docProps/thumbnail.jpeg",
    "docProps/thumbnail.jpg",
    "docProps/thumbnail.png",
    "Thumbnails/thumbnail.png",
    "preview.jpg",
];
const MAX_IWORK_BYTES: u64 = 100 * 1024 * 1024;
const MAX_EPUB_BYTES: u64 = 100 * 1024 * 1024;

fn read_whole(path: &str, most: u64) -> Option<Vec<u8>> {
    if Path::new(path).is_dir() {
        return crate::iwork_read::package_bytes(Path::new(path), most);
    }
    let file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    if length > most {
        return None;
    }
    let mut data = Vec::with_capacity(length as usize);
    file.take(most).read_to_end(&mut data).ok()?;
    Some(data)
}

fn extension_of(path: &str) -> Option<String> {
    Some(Path::new(path).extension()?.to_str()?.to_ascii_lowercase())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    Pdf,
    Word,
    Excel,
    PowerPoint,
    OpenText,
    OpenSheet,
    OpenSlides,
    Pages,
    Numbers,
    Keynote,
    Epub,
}

fn family_of(path: &str) -> Option<Family> {
    Some(match extension_of(path)?.as_str() {
        "pdf" => Family::Pdf,
        "docx" | "docm" | "dotx" | "dotm" => Family::Word,
        "xlsx" | "xlsm" | "xltx" | "xltm" => Family::Excel,
        "pptx" | "pptm" | "ppsx" | "potx" | "potm" => Family::PowerPoint,
        "odt" | "ott" => Family::OpenText,
        "ods" | "ots" => Family::OpenSheet,
        "odp" | "otp" => Family::OpenSlides,
        "pages" => Family::Pages,
        "numbers" => Family::Numbers,
        "key" => Family::Keynote,
        "epub" => Family::Epub,
        _ => return None,
    })
}

pub fn render_before_system(path: &str, size: u32) -> Option<Vec<u8>> {
    match family_of(path)? {
        Family::Pdf => pdf::first_page(path, size),
        Family::Word | Family::OpenText => docx_page::first_page(path, size),
        Family::PowerPoint | Family::OpenSlides => slides::first(path, size),
        Family::Excel | Family::OpenSheet => workbook_page::first_page(path, size),
        Family::Pages | Family::Numbers | Family::Keynote => {
            saved_thumbnail(Path::new(path), size).or_else(|| render_page(path, size, 1))
        }
        Family::Epub => epub::cover(path, size).or_else(|| docx_page::first_page(path, size)),
    }
}

pub fn warm_up() {
    let _ = slides::convert(&slides::tiny(), None);
}

pub fn render_after_system(path: &str, size: u32) -> Option<Vec<u8>> {
    match family_of(path)? {
        #[cfg(windows)]
        Family::Pdf => windows_pdf::first_page(path, size),
        #[cfg(not(windows))]
        Family::Pdf => None,
        _ => saved_thumbnail(Path::new(path), size),
    }
}

fn is_made_whole(path: &str) -> bool {
    family_of(path).is_some_and(|family| family != Family::Pdf)
}

fn whole_of(path: &str) -> Option<Arc<whole::Made>> {
    let alone = |pdf: Option<Vec<u8>>| pdf.map(|pdf| (pdf, None));
    match family_of(path)? {
        Family::Pdf => None,
        Family::PowerPoint => whole::of(path, slides::MAX_BYTES, |data| alone(slides::convert(data, None))),
        Family::OpenSlides => whole::of(path, slides::MAX_BYTES, |data| {
            alone(slides::convert(&opendocument::slides::pptx(data, None)?, None))
        }),
        Family::Word => whole::of(path, docx_page::MAX_BYTES, |data| alone(docx_page::whole(data))),
        Family::OpenText => whole::of(path, docx_page::MAX_BYTES, |data| {
            alone(docx_page::whole(&opendocument::text::docx(data, None)?))
        }),
        Family::Excel | Family::OpenSheet => {
            whole::of(path, workbook_page::MAX_BYTES, |data| workbook_page::whole(path, data))
        }
        Family::Numbers => whole::of(path, MAX_IWORK_BYTES, |data| workbook_page::whole(path, data)),
        Family::Keynote => whole::of(path, MAX_IWORK_BYTES, |data| {
            alone(slides::convert(&iwork::keynote::pptx(data, None)?, None))
        }),
        Family::Epub => whole::of(path, MAX_EPUB_BYTES, |data| {
            alone(docx_page::whole(&epub::docx(data, None)?))
        }),
        Family::Pages => whole::of(path, MAX_IWORK_BYTES, |data| {
            alone(match iwork::pages::office(data, None)? {
                iwork::pages::Made::Word(docx) => docx_page::whole(&docx),
                iwork::pages::Made::Pages(pptx) => slides::convert(&pptx, None),
            })
        }),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Paging {
    pub pages: usize,
    pub note: Option<String>,
}

pub fn paging(path: &str) -> Option<Paging> {
    match family_of(path)? {
        Family::Pdf => Some(Paging {
            pages: pdf::page_count(path)?,
            note: None,
        }),
        _ => {
            let made = whole_of(path)?;
            Some(Paging {
                pages: pdf::count_bytes(Arc::clone(&made.pdf))?,
                note: made.note.clone(),
            })
        }
    }
}

pub fn render_page(path: &str, size: u32, page: u32) -> Option<Vec<u8>> {
    let index = page.checked_sub(1)? as usize;
    match family_of(path)? {
        Family::Pdf => pdf::page(path, size, index),
        _ => pdf::page_bytes(Arc::clone(&whole_of(path)?.pdf), size, index),
    }
}

pub fn draws_every_page(path: &str) -> bool {
    is_made_whole(path)
}

pub fn page_texts(path: &str) -> Option<Arc<Vec<String>>> {
    match family_of(path)? {
        Family::Word
        | Family::Excel
        | Family::OpenText
        | Family::OpenSheet
        | Family::Pages
        | Family::Numbers
        | Family::Keynote
        | Family::Epub => whole_of(path)?.texts(),
        _ => None,
    }
}

fn saved_thumbnail(path: &Path, size: u32) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    if path.is_dir() {
        File::open(path.join("preview.jpg"))
            .ok()?
            .take(MAX_THUMBNAIL_BYTES)
            .read_to_end(&mut bytes)
            .ok()?;
    } else {
        let mut archive = zip::ZipArchive::new(BufReader::new(File::open(path).ok()?)).ok()?;
        let index = (0..archive.len()).find(|&i| {
            archive.by_index_raw(i).is_ok_and(|file| {
                file.name()
                    .is_ok_and(|name| SAVED_THUMBNAILS.iter().any(|known| known.eq_ignore_ascii_case(&name)))
            })
        })?;
        archive
            .by_index(index)
            .ok()?
            .take(MAX_THUMBNAIL_BYTES)
            .read_to_end(&mut bytes)
            .ok()?;
    }
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

mod pdf {
    use std::{
        panic::{AssertUnwindSafe, catch_unwind},
        sync::{Condvar, Mutex, OnceLock, PoisonError},
        thread,
    };

    use hayro::{
        PixmapSettings, RenderCache, RenderSettings,
        hayro_interpret::InterpreterSettings,
        hayro_syntax::{Pdf, PdfData},
        render,
        vello_cpu::color::palette::css::WHITE,
    };

    const MAX_BYTES: u64 = 64 * 1024 * 1024;

    pub fn first_page(path: &str, size: u32) -> Option<Vec<u8>> {
        page(path, size, 0)
    }

    pub fn page_count(path: &str) -> Option<usize> {
        let data = super::read_whole(path, MAX_BYTES)?;
        catch_unwind(AssertUnwindSafe(|| Pdf::new(data).ok().map(|pdf| pdf.pages().len())))
            .ok()
            .flatten()
    }

    pub fn page(path: &str, size: u32, index: usize) -> Option<Vec<u8>> {
        let data = super::read_whole(path, MAX_BYTES)?;
        let _slot = slots().take();
        catch_unwind(AssertUnwindSafe(|| draw_page(data, size, index)))
            .ok()
            .flatten()
    }

    pub(super) fn draw_bytes(data: Vec<u8>, size: u32) -> Option<Vec<u8>> {
        draw_page(data, size, 0)
    }

    pub(super) fn page_bytes(data: impl Into<PdfData>, size: u32, index: usize) -> Option<Vec<u8>> {
        let _slot = slots().take();
        draw_held(data, size, index)
    }

    pub(super) fn draw_held(data: impl Into<PdfData>, size: u32, index: usize) -> Option<Vec<u8>> {
        catch_unwind(AssertUnwindSafe(|| draw_page(data, size, index)))
            .ok()
            .flatten()
    }

    pub(super) fn count_bytes(data: impl Into<PdfData>) -> Option<usize> {
        catch_unwind(AssertUnwindSafe(|| Pdf::new(data).ok().map(|pdf| pdf.pages().len())))
            .ok()
            .flatten()
    }

    fn draw_page(data: impl Into<PdfData>, size: u32, index: usize) -> Option<Vec<u8>> {
        let pdf = Pdf::new(data).ok()?;
        let page = pdf.pages().get(index)?;
        let (width, height) = page.render_dimensions();
        if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
            return None;
        }
        let scale = size as f32 / width.max(height);
        let settings = PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: WHITE,
        };
        let pixmap = render(
            page,
            &RenderCache::new(),
            &InterpreterSettings::default(),
            &RenderSettings::default(),
            &settings,
        );
        pixmap.into_png().ok()
    }

    pub(super) fn slots() -> &'static Slots {
        static SLOTS: OnceLock<Slots> = OnceLock::new();
        SLOTS.get_or_init(|| Slots::new((thread::available_parallelism().map_or(4, |n| n.get()) / 2).clamp(2, 4)))
    }

    pub(super) struct Slots {
        free: Mutex<usize>,
        freed: Condvar,
    }

    pub(super) struct Slot<'a>(&'a Slots);

    impl Slots {
        fn new(places: usize) -> Self {
            Self {
                free: Mutex::new(places),
                freed: Condvar::new(),
            }
        }

        pub(super) fn take(&self) -> Slot<'_> {
            let mut free = self.free.lock().unwrap_or_else(PoisonError::into_inner);
            while *free == 0 {
                free = self.freed.wait(free).unwrap_or_else(PoisonError::into_inner);
            }
            *free -= 1;
            Slot(self)
        }
    }

    impl Drop for Slot<'_> {
        fn drop(&mut self) {
            *self.0.free.lock().unwrap_or_else(PoisonError::into_inner) += 1;
            self.0.freed.notify_one();
        }
    }
}

mod whole {
    use std::{
        fs,
        panic::{AssertUnwindSafe, catch_unwind},
        sync::{Arc, Mutex, OnceLock, PoisonError},
        time::SystemTime,
    };

    const KEPT: usize = 4;
    type Key = (String, u64, Option<SystemTime>);

    pub(super) struct Made {
        pub pdf: Arc<Vec<u8>>,
        pub note: Option<String>,
        texts: OnceLock<Option<Arc<Vec<String>>>>,
    }

    impl Made {
        pub fn texts(&self) -> Option<Arc<Vec<String>>> {
            self.texts
                .get_or_init(|| {
                    let _slot = super::pdf::slots().take();
                    catch_unwind(AssertUnwindSafe(|| {
                        let document = pdf_oxide::PdfDocument::from_bytes(self.pdf.to_vec()).ok()?;
                        let count = document.page_count().ok()?;
                        Some(Arc::new(
                            (0..count)
                                .map(|page| document.extract_text(page).unwrap_or_default())
                                .collect(),
                        ))
                    }))
                    .ok()
                    .flatten()
                })
                .clone()
        }
    }

    static MADE: Mutex<Vec<(Key, Option<Arc<Made>>)>> = Mutex::new(Vec::new());
    static MAKING: Mutex<()> = Mutex::new(());

    fn key_of(path: &str) -> Option<Key> {
        let metadata = fs::metadata(path).ok()?;
        if metadata.is_dir() {
            let (size, newest) = crate::browse::package_state(std::path::Path::new(path))?;
            return Some((path.to_owned(), size, Some(newest)));
        }
        Some((path.to_owned(), metadata.len(), metadata.modified().ok()))
    }

    pub fn kept(path: &str) -> Option<Arc<Made>> {
        made(&key_of(path)?).flatten()
    }

    pub fn of(
        path: &str,
        most: u64,
        make: impl FnOnce(&[u8]) -> Option<(Vec<u8>, Option<String>)>,
    ) -> Option<Arc<Made>> {
        let key = key_of(path)?;
        if let Some(made) = made(&key) {
            return made;
        }
        let _making = MAKING.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(made) = made(&key) {
            return made;
        }
        let pdf = super::read_whole(path, most)
            .and_then(|data| {
                let _slot = super::pdf::slots().take();
                catch_unwind(AssertUnwindSafe(|| make(&data))).ok().flatten()
            })
            .map(|(pdf, note)| {
                Arc::new(Made {
                    pdf: Arc::new(pdf),
                    note,
                    texts: OnceLock::new(),
                })
            });
        let mut kept = MADE.lock().unwrap_or_else(PoisonError::into_inner);
        kept.retain(|(other, _)| other.0 != key.0);
        kept.insert(0, (key, pdf.clone()));
        kept.truncate(KEPT);
        pdf
    }

    fn made(key: &Key) -> Option<Option<Arc<Made>>> {
        let kept = MADE.lock().unwrap_or_else(PoisonError::into_inner);
        kept.iter().find(|(other, _)| other == key).map(|(_, pdf)| pdf.clone())
    }
}

mod slides {
    use std::{
        io::{Cursor, Write},
        panic::{AssertUnwindSafe, catch_unwind},
        sync::Arc,
    };

    use office2pdf::config::{ConvertOptions, Format, SlideRange};

    pub(super) const MAX_BYTES: u64 = 100 * 1024 * 1024;

    pub fn first(path: &str, size: u32) -> Option<Vec<u8>> {
        if let Some(made) = super::whole::kept(path) {
            return super::pdf::page_bytes(Arc::clone(&made.pdf), size, 0);
        }
        let data = super::read_whole(path, MAX_BYTES)?;
        let _slot = super::pdf::slots().take();
        let pdf = if super::family_of(path) == Some(super::Family::OpenSlides) {
            convert(
                &catch_unwind(AssertUnwindSafe(|| super::opendocument::slides::pptx(&data, Some(1))))
                    .ok()
                    .flatten()?,
                None,
            )?
        } else {
            convert(&data, Some(SlideRange::new(1, 1)))?
        };
        super::pdf::draw_held(pdf, size, 0)
    }

    pub(super) fn convert(data: &[u8], slides: Option<SlideRange>) -> Option<Vec<u8>> {
        let options = ConvertOptions {
            include_hidden_slides: true,
            slide_range: slides,
            ..ConvertOptions::default()
        };
        catch_unwind(AssertUnwindSafe(|| {
            office2pdf::convert_bytes(data, Format::Pptx, &options)
        }))
        .ok()?
        .ok()
        .map(|result| result.pdf)
    }

    pub(super) fn tiny() -> Vec<u8> {
        const RELATIONSHIPS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
        let relationships = |id: &str, kind: &str, target: &str| {
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="{id}" Type="{RELATIONSHIPS}/{kind}" Target="{target}"/></Relationships>"#
            )
        };
        let namespaces = format!(
            r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="{RELATIONSHIPS}""#
        );
        let parts = [
            ("[Content_Types].xml", "<Types/>".to_owned()),
            (
                "_rels/.rels",
                relationships("rId1", "officeDocument", "ppt/presentation.xml"),
            ),
            (
                "ppt/presentation.xml",
                format!(
                    r#"<p:presentation {namespaces}><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst></p:presentation>"#
                ),
            ),
            (
                "ppt/_rels/presentation.xml.rels",
                relationships("rId1", "slide", "slides/slide1.xml"),
            ),
            (
                "ppt/slides/slide1.xml",
                format!(
                    r#"<p:sld {namespaces}><p:cSld><p:spTree><p:sp><p:txBody><a:bodyPr/><a:p><a:r><a:t>Mesa</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#
                ),
            ),
        ];
        let mut out = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut out);
            for (name, contents) in parts {
                let _ = zip
                    .start_file(name, zip::write::SimpleFileOptions::default())
                    .and_then(|()| Ok(zip.write_all(contents.as_bytes())?));
            }
            let _ = zip.finish();
        }
        out.into_inner()
    }
}

mod workbook_page {
    use std::{
        collections::HashMap,
        fmt::Write as _,
        io::{Cursor, Read, Write},
        panic::{AssertUnwindSafe, catch_unwind},
        path::Path,
        sync::Arc,
    };

    use office2pdf::config::{ConvertOptions, Format};
    use quick_xml::{
        Reader,
        events::{BytesStart, Event},
    };

    pub(super) const MAX_BYTES: u64 = 64 * 1024 * 1024;
    const MAX_SHEET_BYTES: u64 = 512 * 1024 * 1024;

    #[derive(Clone, Copy, Debug)]
    pub(super) struct Room {
        pub rows: usize,
        pub columns: usize,
        pub cells: usize,
    }

    pub(super) const FIRST_PAGE: Room = Room {
        rows: 100,
        columns: 40,
        cells: 4_000,
    };
    pub(super) const PAGED: Room = Room {
        rows: 2_000,
        columns: 50,
        cells: 10_000,
    };

    pub fn first_page(path: &str, size: u32) -> Option<Vec<u8>> {
        if let Some(made) = super::whole::kept(path) {
            return super::pdf::page_bytes(Arc::clone(&made.pdf), size, 0);
        }
        let bytes = super::read_whole(path, MAX_BYTES)?;
        let _slot = super::pdf::slots().take();
        let (pdf, _) = catch_unwind(AssertUnwindSafe(|| convert(path, &bytes, FIRST_PAGE)))
            .ok()
            .flatten()?;
        super::pdf::draw_held(pdf, size, 0)
    }

    pub(super) fn whole(path: &str, bytes: &[u8]) -> Option<(Vec<u8>, Option<String>)> {
        let (pdf, cuts) = convert(path, bytes, PAGED)?;
        Some((pdf, note(&cuts, PAGED)))
    }

    fn convert(path: &str, bytes: &[u8], room: Room) -> Option<(Vec<u8>, Cuts)> {
        let (cut, cuts) = match super::family_of(path) {
            Some(super::Family::OpenSheet) => super::opendocument::sheets::xlsx(bytes, room)?,
            Some(super::Family::Numbers) => super::iwork::numbers::xlsx(bytes, room)?,
            _ => {
                let sheets = crate::embed::shown_sheets(Path::new(path)).ok()?;
                cut_down(bytes, &sheets, room)?
            }
        };
        let pdf = office2pdf::convert_bytes(&cut, Format::Xlsx, &ConvertOptions::default())
            .ok()?
            .pdf;
        Some((pdf, cuts))
    }

    pub(super) type Cuts = Vec<(String, Cut)>;

    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub(super) struct Cut {
        pub rows: Option<usize>,
        pub columns: bool,
    }

    pub(super) fn note(cuts: &[(String, Cut)], room: Room) -> Option<String> {
        let cut = cuts.iter().filter(|(_, cut)| cut.rows.is_some() || cut.columns);
        let (partly, none): (Vec<_>, Vec<_>) = cut.partition(|(_, cut)| cut.rows != Some(0) || cut.columns);
        let sheets = |count: usize| {
            if count == 1 {
                "1 sheet".to_owned()
            } else {
                format!("{count} sheets")
            }
        };
        match (partly.as_slice(), none.len()) {
            ([], 0) => None,
            ([(name, cut)], rest) => {
                let what = match (cut.rows, cut.columns) {
                    (Some(rows), false) => format!("First {} rows", thousands(rows)),
                    (Some(rows), true) => format!("First {} rows and {} columns", thousands(rows), room.columns),
                    _ => format!("First {} columns", room.columns),
                };
                let of = if cuts.len() > 1 {
                    format!(" of \u{201c}{name}\u{201d}")
                } else {
                    String::new()
                };
                let after = match rest {
                    0 => String::new(),
                    1 => "; 1 more sheet not shown".into(),
                    more => format!("; {more} more sheets not shown"),
                };
                Some(format!("{what}{of} shown{after}"))
            }
            ([], rest) => Some(format!("{} not shown", sheets(rest))),
            (several, rest) => Some(format!("Only the start of {} shown", sheets(several.len() + rest))),
        }
    }

    fn thousands(n: usize) -> String {
        let digits = n.to_string();
        let mut written = String::new();
        for (at, digit) in digits.chars().enumerate() {
            if at > 0 && (digits.len() - at).is_multiple_of(3) {
                written.push(',');
            }
            written.push(digit);
        }
        written
    }

    pub(super) fn cut_down(bytes: &[u8], sheets: &[(String, String)], room: Room) -> Option<(Vec<u8>, Cuts)> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
        let mut read = |name: &str| -> Option<Vec<u8>> {
            let mut xml = Vec::new();
            archive
                .by_name(name)
                .ok()?
                .take(MAX_SHEET_BYTES)
                .read_to_end(&mut xml)
                .ok()?;
            Some(xml)
        };
        let mut cut = HashMap::new();
        let mut cuts = Vec::new();
        let mut cells = room.cells;
        for (name, sheet) in sheets {
            let Some(xml) = read(sheet) else { continue };
            let (xml, kept, left_out) = cut_sheet(&xml, Room { cells, ..room })?;
            cells -= kept;
            cut.insert(sheet.as_str(), xml);
            cuts.push((name.clone(), left_out));
        }
        let mut out = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut out);
        for i in 0..archive.len() {
            let name: String = archive.by_index_raw(i).ok()?.name().ok()?.to_string();
            if name.starts_with("xl/tables/") && name.ends_with(".xml") {
                let mut xml = Vec::new();
                archive
                    .by_index(i)
                    .ok()?
                    .take(MAX_SHEET_BYTES)
                    .read_to_end(&mut xml)
                    .ok()?;
                writer
                    .start_file(&*name, zip::write::SimpleFileOptions::default())
                    .ok()?;
                writer.write_all(&cut_table(&xml, room).unwrap_or(xml)).ok()?;
                continue;
            }
            if !(name.starts_with("xl/worksheets/") && name.ends_with(".xml")) {
                writer.raw_copy_file(archive.by_index_raw(i).ok()?).ok()?;
                continue;
            }
            let xml = match cut.remove(name.as_str()) {
                Some(xml) => xml,
                None => {
                    let mut xml = Vec::new();
                    archive
                        .by_index(i)
                        .ok()?
                        .take(MAX_SHEET_BYTES)
                        .read_to_end(&mut xml)
                        .ok()?;
                    cut_sheet(&xml, Room { cells: 0, ..room })?.0
                }
            };
            writer
                .start_file(&*name, zip::write::SimpleFileOptions::default())
                .ok()?;
            writer.write_all(&xml).ok()?;
        }
        writer.finish().ok()?;
        Some((out.into_inner(), cuts))
    }

    pub(super) fn cut_sheet(xml: &[u8], room: Room) -> Option<(Vec<u8>, usize, Cut)> {
        let xml = xml.strip_prefix(b"\xEF\xBB\xBF".as_slice()).unwrap_or(xml);
        let mut out = Vec::new();
        let (mut copied, mut offset) = (0, 0);
        let reader_of = |xml| {
            let mut reader = Reader::from_reader(xml);
            reader.config_mut().allow_unmatched_ends = true;
            reader
        };
        let mut reader = reader_of(xml);
        let (mut rows, mut cells, mut column) = (0, 0, 0);
        let (mut row, mut left_out) = (0, Cut::default());
        loop {
            let before = offset + reader.buffer_position() as usize;
            match reader.read_event().ok()? {
                Event::Start(tag) | Event::Empty(tag) if tag.local_name().as_ref() == "row" => {
                    if rows == room.rows || cells >= room.cells {
                        left_out.rows.get_or_insert(row);
                        out.extend_from_slice(&xml[copied..before]);
                        copied = before + end_tag(&xml[before..], b"sheetData", true)?;
                        break;
                    }
                    rows += 1;
                    column = 0;
                    row = tag
                        .try_get_attribute("r")
                        .ok()
                        .flatten()
                        .and_then(|r| r.value.parse().ok())
                        .unwrap_or(row + 1);
                }
                Event::Start(tag) | Event::Empty(tag) if tag.local_name().as_ref() == "c" => {
                    column = column_of(&tag).unwrap_or(column + 1);
                    if column > room.columns {
                        left_out.columns = true;
                    } else if cells >= room.cells {
                        left_out.rows.get_or_insert(row);
                    }
                    if column > room.columns || cells >= room.cells {
                        out.extend_from_slice(&xml[copied..before]);
                        copied = before + end_tag(&xml[before..], b"row", false)?;
                        offset = copied;
                        reader = reader_of(&xml[copied..]);
                        continue;
                    }
                    cells += 1;
                }
                Event::Eof => break,
                _ => {}
            }
        }
        out.extend_from_slice(&xml[copied..]);
        Some((out, cells, left_out))
    }

    pub(super) fn cut_table(xml: &[u8], room: Room) -> Option<Vec<u8>> {
        let text = std::str::from_utf8(xml).ok()?;
        let start = text.find(" ref=\"")? + 6;
        let end = start + text[start..].find('"')?;
        let range = &text[start..end];
        let (first, last) = range.split_once(':')?;
        let place = |cell: &str| -> Option<(usize, usize)> {
            let letters: String = cell.chars().take_while(char::is_ascii_alphabetic).collect();
            let row: usize = cell[letters.len()..].parse().ok()?;
            let column = letters.bytes().try_fold(0usize, |column, letter| {
                column
                    .checked_mul(26)?
                    .checked_add(usize::from(letter.to_ascii_uppercase() - b'A') + 1)
            })?;
            Some((column, row))
        };
        let ((first_column, first_row), (last_column, last_row)) = (place(first)?, place(last)?);
        let kept_last_column = last_column
            .min(first_column + room.columns.saturating_sub(1))
            .max(first_column);
        let kept_last_row = last_row.min(first_row + room.rows).max(first_row);
        if (kept_last_column, kept_last_row) == (last_column, last_row) {
            return None;
        }
        let name = |mut column: usize| {
            let mut letters = Vec::new();
            while column > 0 {
                column -= 1;
                letters.push(b'A' + (column % 26) as u8);
                column /= 26;
            }
            letters.reverse();
            String::from_utf8(letters).unwrap_or_default()
        };
        let kept = format!("{first}:{}{kept_last_row}", name(kept_last_column));
        let text = text.replace(&format!("ref=\"{range}\""), &format!("ref=\"{kept}\""));
        let columns = kept_last_column - first_column + 1;
        let Some(open) = text.find("<tableColumns") else {
            return Some(text.into_bytes());
        };
        let close = text[open..].find("</tableColumns>").map(|at| open + at)?;
        let inside_start = open + text[open..].find('>')? + 1;
        let mut out = String::with_capacity(text.len());
        out.push_str(&text[..open]);
        let _ = write!(out, r#"<tableColumns count="{columns}">"#);
        let (mut at, mut taken) = (inside_start, 0);
        while taken < columns {
            let Some(found) = text[at..close].find("<tableColumn") else {
                break;
            };
            let begin = at + found;
            let head_end = begin + text[begin..].find('>')?;
            let end = if text[..=head_end].ends_with("/>") {
                head_end + 1
            } else {
                begin + text[begin..].find("</tableColumn>")? + "</tableColumn>".len()
            };
            out.push_str(&text[begin..end]);
            at = end;
            taken += 1;
        }
        out.push_str(&text[close..]);
        Some(out.into_bytes())
    }

    fn column_of(tag: &BytesStart) -> Option<usize> {
        let reference = tag.try_get_attribute("r").ok()??.value;
        let mut column: usize = 0;
        for letter in reference.bytes().take_while(|byte| byte.is_ascii_alphabetic()) {
            column = column
                .checked_mul(26)?
                .checked_add(usize::from(letter.to_ascii_uppercase() - b'A') + 1)?;
        }
        (column > 0).then_some(column)
    }

    fn end_tag(xml: &[u8], name: &[u8], last: bool) -> Option<usize> {
        let name = [name, b">"].concat();
        let is_end = |at: usize| {
            let open = xml[..at].iter().rposition(|&byte| byte == b'<');
            open.filter(|&open| {
                let between = &xml[open + 1..at];
                between.first() == Some(&b'/')
                    && (between.len() == 1 || between.ends_with(b":"))
                    && between[1..]
                        .iter()
                        .all(|&byte| byte.is_ascii_alphanumeric() || byte == b':' || byte == b'_' || byte == b'-')
            })
        };
        let mut found = xml
            .windows(name.len())
            .enumerate()
            .filter(|(_, window)| *window == name.as_slice())
            .map(|(at, _)| at);
        if last {
            let mut until = xml.len();
            while let Some(at) = xml[..until]
                .windows(name.len())
                .rposition(|window| window == name.as_slice())
            {
                if let Some(open) = is_end(at) {
                    return Some(open);
                }
                until = at;
            }
            None
        } else {
            found.find_map(is_end)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const ROOM: Room = Room {
            rows: 100,
            columns: 3,
            cells: 1_000,
        };

        fn sheet(rows: usize, columns: usize) -> String {
            let rows: String = (1..=rows)
                .map(|n| {
                    let cells: String = (0..columns)
                        .map(|c| format!(r#"<c r="{}{n}"><v>{n}</v></c>"#, (b'A' + c as u8) as char))
                        .collect();
                    format!(r#"<row r="{n}">{cells}</row>"#)
                })
                .collect();
            format!(
                r#"<?xml version="1.0"?><worksheet xmlns="x"><cols><col min="1" max="1" width="9"/></cols><sheetData>{rows}</sheetData><mergeCells count="1"><mergeCell ref="A1:B1"/></mergeCells></worksheet>"#
            )
        }

        fn cut_of(xml: &str, room: Room) -> (String, usize, Cut) {
            let (cut, kept, left_out) = cut_sheet(xml.as_bytes(), room).unwrap();
            (String::from_utf8(cut).unwrap(), kept, left_out)
        }

        #[test]
        fn sheet_truncated_to_first_rows() {
            let (cut, kept, left_out) = cut_of(&sheet(150, 1), ROOM);
            assert_eq!((cut.matches("<row ").count(), kept), (100, 100));
            assert_eq!(
                left_out,
                Cut {
                    rows: Some(100),
                    columns: false
                }
            );
            assert!(cut.contains(r#"<row r="100">"#) && !cut.contains(r#"<row r="101">"#));
            assert!(
                cut.ends_with(
                    r#"</row></sheetData><mergeCells count="1"><mergeCell ref="A1:B1"/></mergeCells></worksheet>"#
                ),
                "{}",
                &cut[cut.len() - 120..]
            );
            assert!(cut.contains("<cols>"));
            assert_eq!(
                cut_sheet(sheet(100, 1).as_bytes(), ROOM).unwrap(),
                (sheet(100, 1).into_bytes(), 100, Cut::default())
            );
            let prefixed = sheet(101, 1)
                .replace("sheetData>", "x:sheetData>")
                .replace("<row ", "<x:row ")
                .replace("</row>", "</x:row>");
            assert!(cut_of(&prefixed, ROOM).0.contains("</x:row></x:sheetData>"));
        }
    }
}

mod docx_page {
    use std::{
        io::{Cursor, Read},
        panic::{AssertUnwindSafe, catch_unwind},
        sync::Arc,
    };

    use docxide_pdf::{ConvertOptions, convert_docx_bytes_to_pdf_bytes_with};
    use quick_xml::{Reader, events::Event};

    pub(super) const MAX_BYTES: u64 = 64 * 1024 * 1024;
    const MAX_XML_BYTES: u64 = 32 * 1024 * 1024;
    const MAX_BLOCKS: usize = 120;
    const MAX_TEXT: usize = 8_000;

    pub fn first_page(path: &str, size: u32) -> Option<Vec<u8>> {
        if let Some(made) = super::whole::kept(path) {
            return super::pdf::page_bytes(Arc::clone(&made.pdf), size, 0);
        }
        let bytes = super::read_whole(path, MAX_BYTES)?;
        let _slot = super::pdf::slots().take();
        catch_unwind(AssertUnwindSafe(|| match super::family_of(path) {
            Some(super::Family::OpenText) => draw(
                &super::opendocument::text::docx(
                    &bytes,
                    Some(super::opendocument::text::Limit {
                        blocks: MAX_BLOCKS,
                        chars: MAX_TEXT,
                    }),
                )?,
                size,
            ),
            Some(super::Family::Epub) => draw(&super::epub::docx(&bytes, Some(MAX_BLOCKS))?, size),
            _ => draw(&bytes, size),
        }))
        .ok()
        .flatten()
    }

    fn draw(bytes: &[u8], size: u32) -> Option<Vec<u8>> {
        let cut = cut_down(bytes);
        let source = cut.as_deref().unwrap_or(bytes);
        let pdf = convert_docx_bytes_to_pdf_bytes_with(source, &ConvertOptions::new()).ok()?;
        super::pdf::draw_bytes(pdf, size)
    }

    pub(super) fn whole(bytes: &[u8]) -> Option<Vec<u8>> {
        convert_docx_bytes_to_pdf_bytes_with(bytes, &ConvertOptions::new()).ok()
    }

    fn cut_down(bytes: &[u8]) -> Option<Vec<u8>> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
        let mut xml = String::new();
        archive
            .by_name("word/document.xml")
            .ok()?
            .take(MAX_XML_BYTES)
            .read_to_string(&mut xml)
            .ok()?;
        let cut = first_blocks(&xml)?;
        replace_document(&mut archive, &cut)
    }

    fn replace_document(archive: &mut zip::ZipArchive<Cursor<&[u8]>>, xml: &str) -> Option<Vec<u8>> {
        use std::io::Write;
        let mut out = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut out);
        for i in 0..archive.len() {
            let file = archive.by_index_raw(i).ok()?;
            if file.name().ok()? == "word/document.xml" {
                writer
                    .start_file("word/document.xml", zip::write::SimpleFileOptions::default())
                    .ok()?;
                writer.write_all(xml.as_bytes()).ok()?;
            } else {
                writer.raw_copy_file(file).ok()?;
            }
        }
        writer.finish().ok()?;
        Some(out.into_inner())
    }

    fn first_blocks(xml: &str) -> Option<String> {
        let xml = xml.strip_prefix('\u{feff}').unwrap_or(xml);
        let mut reader = Reader::from_str(xml);
        let (mut root, mut body) = (None::<String>, None::<String>);
        let (mut depth, mut body_depth) = (0usize, 0usize);
        let mut in_text = false;
        let mut kept = Kept::default();
        let mut settings = None::<(usize, usize)>;
        let mut reading = None::<(usize, usize)>;
        loop {
            let from = reader.buffer_position() as usize;
            let event = reader.read_event().ok()?;
            let to = reader.buffer_position() as usize;
            match event {
                Event::Start(tag) => {
                    depth += 1;
                    let local = tag.local_name();
                    if root.is_none() {
                        root = Some(tag.name().as_ref().to_owned());
                    } else if body.is_none() && depth == 2 && local.as_ref() == "body" {
                        body = Some(tag.name().as_ref().to_owned());
                        body_depth = depth;
                    }
                    if local.as_ref() == "sectPr" && settings.is_none() && reading.is_none() {
                        reading = Some((depth, from));
                    }
                    in_text |= local.as_ref() == "t";
                }
                Event::Empty(tag) => {
                    let is_settings = tag.local_name().as_ref() == "sectPr";
                    if is_settings && settings.is_none() && reading.is_none() {
                        settings = Some((from, to));
                    }
                    if body.is_some() && depth == body_depth && !is_settings {
                        kept.block_ended(to);
                    }
                }
                Event::End(tag) => {
                    let local = tag.local_name();
                    if local.as_ref() == "t" {
                        in_text = false;
                    }
                    if let Some((began_at_depth, start)) = reading
                        && depth == began_at_depth
                    {
                        settings = Some((start, to));
                        reading = None;
                    }
                    if body.is_some() && depth == body_depth + 1 && local.as_ref() != "sectPr" {
                        kept.block_ended(to);
                    }
                    depth = depth.saturating_sub(1);
                }
                Event::Text(content) if in_text && body.is_some() => kept.text += content.len(),
                Event::Eof => break,
                _ => {}
            }
            if kept.cut.is_some() && settings.is_some() {
                break;
            }
        }
        let (root, body, cut) = (root?, body?, kept.cut?);
        let settings = settings.map_or("", |(from, to)| &xml[from..to]);
        Some(format!("{}{settings}</{body}></{root}>", &xml[..cut]))
    }

    #[derive(Default)]
    struct Kept {
        blocks: usize,
        text: usize,
        cut: Option<usize>,
    }

    impl Kept {
        fn block_ended(&mut self, end: usize) {
            if self.cut.is_some() {
                return;
            }
            self.blocks += 1;
            if self.blocks >= MAX_BLOCKS || self.text >= MAX_TEXT {
                self.cut = Some(end);
            }
        }
    }

    #[cfg(test)]
    mod tests {

        use super::*;

        const NAMESPACE: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
        const PORTRAIT: &str = r#"<w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr>"#;

        fn paragraph(text: &str) -> String {
            format!(r#"<w:p><w:r><w:t xml:space="preserve">{text}</w:t></w:r></w:p>"#)
        }

        fn document(blocks: &str, settings: &str) -> String {
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {NAMESPACE}><w:body>{blocks}{settings}</w:body></w:document>"#
            )
        }

        fn paragraphs(count: usize) -> String {
            (0..count)
                .map(|n| paragraph(&format!("Paragraph number {n}.")))
                .collect()
        }

        fn is_well_formed(xml: &str) -> bool {
            let mut reader = Reader::from_str(xml);
            loop {
                match reader.read_event() {
                    Ok(Event::Eof) => return true,
                    Ok(_) => {}
                    Err(_) => return false,
                }
            }
        }

        #[test]
        fn long_docx_truncated_but_valid() {
            let xml = document(&paragraphs(500), PORTRAIT);
            let cut = first_blocks(&xml).unwrap();
            assert!(cut.len() < xml.len() / 3);
            assert!(cut.contains("Paragraph number 0.") && cut.contains("Paragraph number 100."));
            assert!(!cut.contains("Paragraph number 200.") && !cut.contains("Paragraph number 499."));
            assert!(cut.ends_with(&format!("{PORTRAIT}</w:body></w:document>")));
            assert!(is_well_formed(&cut));
        }

        #[test]
        fn truncation_handles_bom() {
            let xml = format!("\u{feff}{}", document(&paragraphs(500), PORTRAIT));
            let cut = first_blocks(&xml).unwrap();
            assert!(cut.ends_with(&format!("{PORTRAIT}</w:body></w:document>")));
            assert!(is_well_formed(&cut));
        }
    }
}

#[cfg(windows)]
pub(crate) fn fitted(width: f32, height: f32, size: u32) -> Option<(u32, u32)> {
    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
        return None;
    }
    let scale = size as f32 / width.max(height);
    let pixels = |side: f32| ((side * scale).round() as u32).clamp(1, size);
    Some((pixels(width), pixels(height)))
}

#[cfg(windows)]
mod windows_pdf {
    use windows::{
        Data::Pdf::{PdfDocument, PdfPageRenderOptions},
        Storage::{
            StorageFile,
            Streams::{DataReader, InMemoryRandomAccessStream},
        },
        UI::Color,
        Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
        core::HSTRING,
    };

    pub fn first_page(path: &str, size: u32) -> Option<Vec<u8>> {
        unsafe {
            let started = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let png = draw(path, size);
            if started {
                CoUninitialize();
            }
            png
        }
    }

    fn draw(path: &str, size: u32) -> Option<Vec<u8>> {
        let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.replace('/', "\\")))
            .ok()?
            .join()
            .ok()?;
        let document = PdfDocument::LoadFromFileAsync(&file).ok()?.join().ok()?;
        let page = document.GetPage(0).ok()?;
        let page_size = page.Size().ok()?;
        let (width, height) = super::fitted(page_size.Width, page_size.Height, size)?;

        let options = PdfPageRenderOptions::new().ok()?;
        options.SetDestinationWidth(width).ok()?;
        options.SetDestinationHeight(height).ok()?;
        options
            .SetBackgroundColor(Color {
                A: 255,
                R: 255,
                G: 255,
                B: 255,
            })
            .ok()?;

        let stream = InMemoryRandomAccessStream::new().ok()?;
        page.RenderWithOptionsToStreamAsync(&stream, &options)
            .ok()?
            .join()
            .ok()?;
        let length = u32::try_from(stream.Size().ok()?).ok().filter(|&length| length > 0)?;
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0).ok()?).ok()?;
        reader.LoadAsync(length).ok()?.join().ok()?;
        let mut png = vec![0u8; length as usize];
        reader.ReadBytes(&mut png).ok()?;
        Some(png)
    }
}
