use std::{
    fs::File,
    io::{BufReader, Cursor, Read},
    path::Path,
};

use image::{ImageFormat, imageops::FilterType};

const MAX_THUMBNAIL_BYTES: u64 = 8 * 1024 * 1024;
const DOCX_THUMBNAILS: [&str; 3] = [
    "docProps/thumbnail.jpeg",
    "docProps/thumbnail.jpg",
    "docProps/thumbnail.png",
];

fn read_whole(path: &str, most: u64) -> Option<Vec<u8>> {
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

pub fn render_before_system(path: &str, size: u32) -> Option<Vec<u8>> {
    match extension_of(path)?.as_str() {
        "pdf" => pdf::first_page(path, size),
        _ => None,
    }
}

pub fn render_after_system(path: &str, size: u32) -> Option<Vec<u8>> {
    match extension_of(path)?.as_str() {
        "docx" => docx_thumbnail(Path::new(path), size).or_else(|| docx_page::first_page(path, size)),
        #[cfg(windows)]
        "pdf" => windows_pdf::first_page(path, size),
        _ => None,
    }
}

pub fn page_count(path: &str) -> Option<usize> {
    match extension_of(path)?.as_str() {
        "pdf" => pdf::page_count(path),
        _ => None,
    }
}

pub fn render_page(path: &str, size: u32, page: u32) -> Option<Vec<u8>> {
    match extension_of(path)?.as_str() {
        "pdf" => pdf::page(path, size, page.checked_sub(1)? as usize),
        _ => None,
    }
}

fn docx_thumbnail(path: &Path, size: u32) -> Option<Vec<u8>> {
    let mut archive = zip::ZipArchive::new(BufReader::new(File::open(path).ok()?)).ok()?;
    let index = (0..archive.len()).find(|&i| {
        archive.by_index_raw(i).is_ok_and(|file| {
            file.name()
                .is_ok_and(|name| DOCX_THUMBNAILS.iter().any(|known| known.eq_ignore_ascii_case(&name)))
        })
    })?;
    let mut bytes = Vec::new();
    archive
        .by_index(index)
        .ok()?
        .take(MAX_THUMBNAIL_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
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
        PixmapSettings, RenderCache, RenderSettings, hayro_interpret::InterpreterSettings, hayro_syntax::Pdf, render,
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

    fn draw_page(data: Vec<u8>, size: u32, index: usize) -> Option<Vec<u8>> {
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

mod docx_page {
    use std::{
        io::{Cursor, Read},
        panic::{AssertUnwindSafe, catch_unwind},
    };

    use docxide_pdf::{ConvertOptions, convert_docx_bytes_to_pdf_bytes_with};
    use quick_xml::{Reader, events::Event};

    const MAX_BYTES: u64 = 64 * 1024 * 1024;
    const MAX_XML_BYTES: u64 = 32 * 1024 * 1024;
    const MAX_BLOCKS: usize = 120;
    const MAX_TEXT: usize = 8_000;

    pub fn first_page(path: &str, size: u32) -> Option<Vec<u8>> {
        let bytes = super::read_whole(path, MAX_BYTES)?;
        let _slot = super::pdf::slots().take();
        catch_unwind(AssertUnwindSafe(|| draw(&bytes, size))).ok().flatten()
    }

    fn draw(bytes: &[u8], size: u32) -> Option<Vec<u8>> {
        let cut = cut_down(bytes);
        let source = cut.as_deref().unwrap_or(bytes);
        let pdf = convert_docx_bytes_to_pdf_bytes_with(source, &ConvertOptions::new()).ok()?;
        super::pdf::draw_bytes(pdf, size)
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
