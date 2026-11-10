use std::{
    collections::{BTreeMap, HashMap, hash_map::Entry},
    fs, panic,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, SystemTime},
};

use sha2::{Digest, Sha256};

use tauri::{
    UriSchemeResponder,
    http::{Request, Response},
};

const PREVIEW_SIZE: u32 = 256;
const MAX_BOX_SIDE: u32 = 512;
const JPEG_QUALITY: u8 = 90;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Shape {
    Whole,
    Box { width: u32, height: u32, fit: Fit },
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Fit {
    Cover,
    Contain,
}

const CACHE_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone)]
pub struct PreviewServer {
    shared: Arc<Shared>,
}

struct Shared {
    cache: Mutex<Cache>,
    waiting: Mutex<HashMap<Key, Waiting>>,
    lookups: Stack<Asked>,
    jobs: Stack<Key>,
    disk: OnceLock<Disk>,
}

struct Waiting {
    replies: Vec<Reply>,
    begun: bool,
}

type Reply = Box<dyn FnOnce(Option<Vec<u8>>) + Send>;

struct Asked {
    path: String,
    shape: Shape,
    reply: Reply,
}

const LOOKERS: usize = 2;

impl PreviewServer {
    pub fn start() -> Self {
        let shared = Arc::new(Shared {
            cache: Mutex::new(Cache::new(CACHE_BYTES)),
            waiting: Mutex::default(),
            lookups: Stack::default(),
            jobs: Stack::default(),
            disk: OnceLock::new(),
        });
        for _ in 0..LOOKERS {
            let shared = Arc::clone(&shared);
            thread::spawn(move || {
                crate::power::below_the_page();
                loop {
                    let Asked { path, shape, reply } = shared.lookups.pop();
                    let changed = modified(&path);
                    let key = (path, changed, shape);
                    let kept = shared.lock_cache().get(&key);
                    if let Some(kept) = kept {
                        reply(Some(kept));
                        continue;
                    }
                    if !shared.wait_for(key.clone(), reply) {
                        continue;
                    }
                    match shared.disk.get().and_then(|disk| disk.get(&key)) {
                        Some(kept) => {
                            shared.lock_cache().put(key.clone(), kept.clone());
                            shared.answer(&key, Some(kept));
                        }
                        None => shared.jobs.push(key),
                    }
                }
            });
        }
        let workers = thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 16);
        for _ in 0..workers {
            let shared = Arc::clone(&shared);
            thread::spawn(move || {
                crate::power::below_the_page();
                loop {
                    let key = shared.jobs.pop();
                    if !shared.begin(&key) {
                        continue;
                    }
                    let preview = panic::catch_unwind(|| make(&shared.cache, shared.disk.get(), &key))
                        .ok()
                        .flatten();
                    shared.answer(&key, preview);
                }
            });
        }
        Self { shared }
    }

    pub fn keep_on_disk_in(&self, dir: PathBuf) {
        let _ = self.shared.disk.set(Disk::open(dir, DISK_BYTES));
    }

    pub fn handle(&self, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
        let Some(path) = path_from_uri(request.uri().path()) else {
            return responder.respond(response(None));
        };
        let shape = shape_from_query(request.uri().query());
        self.ask(
            path,
            shape,
            Box::new(move |preview| responder.respond(response(preview))),
        );
    }

    fn ask(&self, path: String, shape: Shape, reply: Reply) {
        self.shared.lookups.push(Asked { path, shape, reply });
    }
}

impl Shared {
    fn lock_cache(&self) -> MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn lock_waiting(&self) -> MutexGuard<'_, HashMap<Key, Waiting>> {
        self.waiting.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn wait_for(&self, key: Key, reply: Reply) -> bool {
        match self.lock_waiting().entry(key) {
            Entry::Occupied(mut waiting) => {
                waiting.get_mut().replies.push(reply);
                !waiting.get().begun
            }
            Entry::Vacant(waiting) => {
                waiting.insert(Waiting {
                    replies: vec![reply],
                    begun: false,
                });
                true
            }
        }
    }

    fn begin(&self, key: &Key) -> bool {
        match self.lock_waiting().get_mut(key) {
            Some(waiting) if !waiting.begun => {
                waiting.begun = true;
                true
            }
            _ => false,
        }
    }

    fn answer(&self, key: &Key, preview: Option<Vec<u8>>) {
        let waiting = self.lock_waiting().remove(key);
        for reply in waiting.into_iter().flat_map(|waiting| waiting.replies) {
            reply(preview.clone());
        }
    }
}

fn make(cache: &Mutex<Cache>, disk: Option<&Disk>, key: &Key) -> Option<Vec<u8>> {
    let lock = || cache.lock().unwrap_or_else(PoisonError::into_inner);
    let whole_key = (key.0.clone(), key.1, Shape::Whole);
    let kept = lock().get(&whole_key);
    let whole = match kept {
        Some(png) => png,
        None => {
            let png = render(&key.0, PREVIEW_SIZE)?;
            lock().put(whole_key, png.clone());
            png
        }
    };
    let Shape::Box { width, height, fit } = key.2 else {
        return Some(whole);
    };
    let made = boxed(&whole, width, height, fit)?;
    lock().put(key.clone(), made.clone());
    if let Some(disk) = disk {
        disk.put(key, &made);
    }
    Some(made)
}

const DISK_BYTES: u64 = 256 * 1024 * 1024;
const LEFT_OVER: Duration = Duration::from_secs(60);
const KEPT_FRESH: Duration = Duration::from_secs(24 * 60 * 60);

struct Disk {
    dir: PathBuf,
    limit: u64,
    counts: Arc<DiskCounts>,
}

struct DiskCounts {
    bytes: AtomicU64,
    tidying: AtomicBool,
}

impl Disk {
    fn open(dir: PathBuf, limit: u64) -> Self {
        let counts = Arc::new(DiskCounts {
            bytes: AtomicU64::new(0),
            tidying: AtomicBool::new(true),
        });
        let disk = Self { dir, limit, counts };
        disk.tidy_in_the_background(true);
        disk
    }

    fn tidy_in_the_background(&self, counting: bool) {
        let (dir, limit, counts) = (self.dir.clone(), self.limit, Arc::clone(&self.counts));
        let _ = thread::Builder::new().name("previews on disk".into()).spawn(move || {
            crate::power::give_way();
            let left = tidy(&dir, limit);
            if counting {
                counts.bytes.fetch_add(left, Ordering::SeqCst);
            } else {
                counts.bytes.store(left, Ordering::SeqCst);
            }
            counts.tidying.store(false, Ordering::SeqCst);
        });
    }

    fn file_of(&self, key: &Key) -> PathBuf {
        let mut hash = Sha256::new();
        hash.update(key.0.as_bytes());
        hash.update([0]);
        let changed = key
            .1
            .and_then(|at| at.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map_or(-1, |at| at.as_nanos() as i128);
        hash.update(changed.to_le_bytes());
        hash.update(format!("{:?}", key.2).as_bytes());
        let name: String = hash.finalize()[..16].iter().map(|byte| format!("{byte:02x}")).collect();
        self.dir.join(&name[..2]).join(name)
    }

    fn get(&self, key: &Key) -> Option<Vec<u8>> {
        let file = self.file_of(key);
        let bytes = fs::read(&file).ok()?;
        if !(bytes.starts_with(&[0xFF, 0xD8]) || bytes.starts_with(&[0x89, b'P', b'N', b'G'])) {
            return None;
        }
        let written = fs::metadata(&file).and_then(|meta| meta.modified()).ok();
        if written.is_none_or(|at| at.elapsed().is_ok_and(|age| age > KEPT_FRESH)) {
            let _ = fs::File::options()
                .append(true)
                .open(&file)
                .and_then(|kept| kept.set_modified(SystemTime::now()));
        }
        Some(bytes)
    }

    fn put(&self, key: &Key, bytes: &[u8]) {
        let file = self.file_of(key);
        let Some(folder) = file.parent() else { return };
        let written = fs::create_dir_all(folder).and_then(|()| {
            let part = file.with_extension(format!("part{}", std::process::id()));
            fs::write(&part, bytes)?;
            fs::rename(&part, &file)
        });
        if written.is_err() {
            return;
        }
        let total = self.counts.bytes.fetch_add(bytes.len() as u64, Ordering::SeqCst) + bytes.len() as u64;
        if total > self.limit && !self.counts.tidying.swap(true, Ordering::SeqCst) {
            self.tidy_in_the_background(false);
        }
    }
}

fn tidy(dir: &Path, limit: u64) -> u64 {
    let mut kept: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    for folder in fs::read_dir(dir).into_iter().flatten().flatten() {
        for file in fs::read_dir(folder.path()).into_iter().flatten().flatten() {
            let path = file.path();
            let Ok(meta) = file.metadata() else { continue };
            if path
                .extension()
                .is_some_and(|extension| extension.to_string_lossy().starts_with("part"))
            {
                if meta
                    .modified()
                    .ok()
                    .and_then(|at| at.elapsed().ok())
                    .is_some_and(|age| age > LEFT_OVER)
                {
                    let _ = fs::remove_file(&path);
                }
                continue;
            }
            kept.push((meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), meta.len(), path));
        }
    }
    let mut total: u64 = kept.iter().map(|(_, len, _)| len).sum();
    if total > limit {
        kept.sort();
        for (_, len, path) in kept {
            if total <= limit / 5 * 4 {
                break;
            }
            if fs::remove_file(&path).is_ok() {
                total -= len;
            }
        }
    }
    total
}

fn boxed(png: &[u8], width: u32, height: u32, fit: Fit) -> Option<Vec<u8>> {
    use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer, images::Image};
    use image::{ExtendedColorType, ImageEncoder, codecs::jpeg::JpegEncoder, codecs::png::PngEncoder};

    let picture = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    let (from_width, from_height) = picture.dimensions();
    if from_width == 0 || from_height == 0 {
        return None;
    }
    let (to_width, to_height) = match fit {
        Fit::Cover => (width, height),
        Fit::Contain => {
            let scale = (f64::from(width) / f64::from(from_width)).min(f64::from(height) / f64::from(from_height));
            (
                centred_side(f64::from(from_width) * scale, width),
                centred_side(f64::from(from_height) * scale, height),
            )
        }
    };
    let source = Image::from_vec_u8(from_width, from_height, picture.into_raw(), PixelType::U8x4).ok()?;
    let mut target = Image::new(to_width, to_height, PixelType::U8x4);
    let mut options = ResizeOptions::new()
        .resize_alg(ResizeAlg::Convolution(FilterType::CatmullRom))
        .use_alpha(true);
    if fit == Fit::Cover {
        options = options.fit_into_destination(Some((0.5, 0.5)));
    }
    Resizer::new().resize(&source, &mut target, &options).ok()?;
    let rgba = target.into_vec();

    let mut out = Vec::new();
    let opaque = rgba.chunks_exact(4).all(|pixel| pixel[3] == 255);
    if fit == Fit::Cover && opaque {
        let rgb: Vec<u8> = rgba
            .chunks_exact(4)
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect();
        JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
            .write_image(&rgb, to_width, to_height, ExtendedColorType::Rgb8)
            .ok()?;
    } else {
        PngEncoder::new_with_quality(
            &mut out,
            image::codecs::png::CompressionType::Fast,
            image::codecs::png::FilterType::Adaptive,
        )
        .write_image(&rgba, to_width, to_height, ExtendedColorType::Rgba8)
        .ok()?;
    }
    Some(out)
}

fn centred_side(exact: f64, room: u32) -> u32 {
    let mut side = exact.round().clamp(1.0, f64::from(room)) as u32;
    if !(room - side).is_multiple_of(2) {
        side = if (exact > f64::from(side) && side < room) || side == 1 {
            side + 1
        } else {
            side - 1
        };
    }
    side
}

fn modified(path: &str) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

struct Stack<T> {
    items: Mutex<Vec<T>>,
    ready: Condvar,
}

impl<T> Default for Stack<T> {
    fn default() -> Self {
        Self {
            items: Mutex::new(Vec::new()),
            ready: Condvar::new(),
        }
    }
}

impl<T> Stack<T> {
    fn push(&self, item: T) {
        self.items.lock().unwrap_or_else(PoisonError::into_inner).push(item);
        self.ready.notify_one();
    }

    fn pop(&self) -> T {
        let mut items = self.items.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(item) = items.pop() {
                return item;
            }
            items = self.ready.wait(items).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

type Key = (String, Option<SystemTime>, Shape);

struct Cache {
    limit: usize,
    bytes: usize,
    entries: HashMap<Key, (Vec<u8>, u64)>,
    order: BTreeMap<u64, Key>,
    clock: u64,
}

impl Cache {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            bytes: 0,
            entries: HashMap::new(),
            order: BTreeMap::new(),
            clock: 0,
        }
    }

    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn get(&mut self, key: &Key) -> Option<Vec<u8>> {
        let now = self.tick();
        let (png, asked) = self.entries.get_mut(key)?;
        self.order.remove(asked);
        *asked = now;
        self.order.insert(now, key.clone());
        Some(png.clone())
    }

    fn put(&mut self, key: Key, png: Vec<u8>) {
        if png.len() > self.limit {
            return;
        }
        let now = self.tick();
        self.bytes += png.len();
        if let Some((old, asked)) = self.entries.insert(key.clone(), (png, now)) {
            self.bytes -= old.len();
            self.order.remove(&asked);
        }
        self.order.insert(now, key);
        while self.bytes > self.limit {
            let Some((_, oldest)) = self.order.pop_first() else {
                break;
            };
            if let Some((png, _)) = self.entries.remove(&oldest) {
                self.bytes -= png.len();
            }
        }
    }
}

fn path_from_uri(uri_path: &str) -> Option<String> {
    let decoded = percent_encoding::percent_decode_str(uri_path.trim_start_matches('/'))
        .decode_utf8()
        .ok()?;
    (!decoded.is_empty()).then(|| decoded.into_owned())
}

fn shape_from_query(query: Option<&str>) -> Shape {
    let (mut width, mut height, mut fit) = (None, None, None);
    for pair in query.unwrap_or("").split('&') {
        match pair.split_once('=') {
            Some(("w", n)) => width = n.parse::<u32>().ok(),
            Some(("h", n)) => height = n.parse::<u32>().ok(),
            Some(("fit", "cover")) => fit = Some(Fit::Cover),
            Some(("fit", "contain")) => fit = Some(Fit::Contain),
            _ => {}
        }
    }
    let side = |n: Option<u32>| n.filter(|n| (1..=MAX_BOX_SIDE).contains(n));
    match (side(width), side(height), fit) {
        (Some(width), Some(height), Some(fit)) => Shape::Box { width, height, fit },
        _ => Shape::Whole,
    }
}

fn response(png: Option<Vec<u8>>) -> Response<Vec<u8>> {
    let builder = Response::builder().header("Access-Control-Allow-Origin", "*");
    let built = match png {
        Some(bytes) => builder
            .status(200)
            .header(
                "Content-Type",
                if bytes.starts_with(&[0xFF, 0xD8]) {
                    "image/jpeg"
                } else {
                    "image/png"
                },
            )
            .header("Cache-Control", "private, max-age=300")
            .body(bytes),
        None => builder.status(404).body(Vec::new()),
    };
    built.expect("a response built from fixed headers is valid")
}

pub fn render(path: &str, size: u32) -> Option<Vec<u8>> {
    crate::document_preview::render_before_system(path, size)
        .or_else(|| system_preview(path, size))
        .or_else(|| crate::document_preview::render_after_system(path, size))
}

fn system_preview(path: &str, size: u32) -> Option<Vec<u8>> {
    #[cfg(windows)]
    {
        shell::render(path, size)
    }
    #[cfg(target_os = "macos")]
    {
        imageio::render(path, size).or_else(|| quicklook::render(path, size))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (path, size);
        None
    }
}

#[cfg(target_os = "macos")]
mod imageio {
    use std::path::Path;

    use objc2_core_foundation::{CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType, CFURL};
    use objc2_core_graphics::CGImage;
    use objc2_image_io::{
        CGImageSource, kCGImagePropertyOrientation, kCGImagePropertyPixelHeight, kCGImagePropertyPixelWidth,
        kCGImageSourceCreateThumbnailFromImageAlways, kCGImageSourceCreateThumbnailFromImageIfAbsent,
        kCGImageSourceCreateThumbnailWithTransform, kCGImageSourceThumbnailMaxPixelSize,
    };

    use crate::cgimage;

    pub fn render(path: &str, size: u32) -> Option<Vec<u8>> {
        let url = CFURL::from_file_path(Path::new(path))?;
        let image = unsafe {
            let source = CGImageSource::with_url(&url, None)?;
            if source.count() == 0 {
                return None;
            }
            let picture = upright_size(&source);
            let preview = thumbnail(&source, kCGImageSourceCreateThumbnailFromImageIfAbsent, size)?;
            let got = (CGImage::width(Some(&preview)), CGImage::height(Some(&preview)));
            if stands_for(got, picture, size as usize) {
                preview
            } else {
                thumbnail(&source, kCGImageSourceCreateThumbnailFromImageAlways, size)?
            }
        };
        let (rgba, width, height) = cgimage::pixels(&image)?;
        super::quicklook::encode_png(width, height, &rgba)
    }

    unsafe fn thumbnail(source: &CGImageSource, from: &CFString, size: u32) -> Option<CFRetained<CGImage>> {
        let yes = CFBoolean::new(true);
        let side = CFNumber::new_i64(i64::from(size));
        let options: CFRetained<CFDictionary<CFString, CFType>> = CFDictionary::from_slices(
            &[from, unsafe { kCGImageSourceCreateThumbnailWithTransform }, unsafe {
                kCGImageSourceThumbnailMaxPixelSize
            }],
            &[yes.as_ref(), yes.as_ref(), side.as_ref()],
        );
        unsafe { source.thumbnail_at_index(0, Some(options.as_opaque())) }
    }

    unsafe fn upright_size(source: &CGImageSource) -> Option<(usize, usize)> {
        let properties = unsafe { source.properties_at_index(0, None) }?;
        let properties: CFRetained<CFDictionary<CFString, CFType>> = unsafe { CFRetained::cast_unchecked(properties) };
        let number = |key: &CFString| properties.get(key)?.downcast::<CFNumber>().ok()?.as_i64();
        let (width, height) = unsafe {
            (
                number(kCGImagePropertyPixelWidth)?,
                number(kCGImagePropertyPixelHeight)?,
            )
        };
        let (width, height) = (usize::try_from(width).ok()?, usize::try_from(height).ok()?);
        let turned = unsafe { number(kCGImagePropertyOrientation) }.is_some_and(|o| (5..=8).contains(&o));
        Some(if turned { (height, width) } else { (width, height) })
    }

    pub(super) fn stands_for(got: (usize, usize), picture: Option<(usize, usize)>, size: usize) -> bool {
        let longest = got.0.max(got.1);
        let Some((width, height)) = picture.filter(|&(w, h)| w > 0 && h > 0) else {
            return longest >= size;
        };
        if longest < size.min(width.max(height)) {
            return false;
        }
        let shaped = got.0 as f64 * height as f64 / width as f64;
        (shaped - got.1 as f64).abs() <= 1.0
    }
}

#[cfg(target_os = "macos")]
mod quicklook {
    use std::{sync::mpsc, time::Duration};

    use block2::RcBlock;
    use objc2::{AnyThread, rc::autoreleasepool};
    use objc2_core_foundation::CGSize;
    use objc2_foundation::{NSError, NSString, NSURL};
    use objc2_quick_look_thumbnailing::{
        QLThumbnailGenerationRequest, QLThumbnailGenerationRequestRepresentationTypes, QLThumbnailGenerator,
        QLThumbnailRepresentation,
    };

    use crate::cgimage;

    const GIVE_UP: Duration = Duration::from_secs(20);

    pub fn render(path: &str, size: u32) -> Option<Vec<u8>> {
        let (sent, received) = mpsc::channel();
        autoreleasepool(|_| unsafe {
            let url = NSURL::fileURLWithPath(&NSString::from_str(path));
            let side = f64::from(size);
            let request = QLThumbnailGenerationRequest::initWithFileAtURL_size_scale_representationTypes(
                QLThumbnailGenerationRequest::alloc(),
                &url,
                CGSize {
                    width: side,
                    height: side,
                },
                1.0,
                QLThumbnailGenerationRequestRepresentationTypes::Thumbnail,
            );
            let handler = RcBlock::new(
                move |representation: *mut QLThumbnailRepresentation, _error: *mut NSError| {
                    let picture = representation
                        .as_ref()
                        .and_then(|representation| cgimage::pixels(&representation.CGImage()));
                    let _ = sent.send(picture);
                },
            );
            QLThumbnailGenerator::sharedGenerator()
                .generateBestRepresentationForRequest_completionHandler(&request, &handler);
        });
        let (rgba, width, height) = received.recv_timeout(GIVE_UP).ok().flatten()?;
        encode_png(width, height, &rgba)
    }

    pub(super) fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
        writer.finish().ok()?;
        Some(out)
    }
}

#[cfg(windows)]
mod shell {
    use std::{ffi::c_void, mem::size_of};

    use windows::{
        Win32::{
            Foundation::SIZE,
            Graphics::Gdi::{
                BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits,
                GetObjectW, HBITMAP, HGDIOBJ, ReleaseDC,
            },
            System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
            UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_THUMBNAILONLY},
        },
        core::HSTRING,
    };

    pub fn render(path: &str, size: u32) -> Option<Vec<u8>> {
        unsafe {
            let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let png = fetch(path, size);
            if com.is_ok() {
                CoUninitialize();
            }
            png
        }
    }

    unsafe fn fetch(path: &str, size: u32) -> Option<Vec<u8>> {
        unsafe {
            let factory: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(path), None).ok()?;
            let side = size as i32;
            let bitmap = factory
                .GetImage(SIZE { cx: side, cy: side }, SIIGBF_THUMBNAILONLY)
                .ok()?;
            let pixels = read_pixels(bitmap);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let (width, height, rgba) = pixels?;
            encode_png(width, height, &rgba)
        }
    }

    unsafe fn read_pixels(bitmap: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
        unsafe {
            let mut bm = BITMAP::default();
            let got = GetObjectW(
                HGDIOBJ(bitmap.0),
                size_of::<BITMAP>() as i32,
                Some(&mut bm as *mut BITMAP as *mut c_void),
            );
            if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
                return None;
            }
            let (width, height) = (bm.bmWidth, bm.bmHeight);

            let mut info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut pixels = vec![0u8; width as usize * height as usize * 4];
            let hdc = GetDC(None);
            let rows = GetDIBits(
                hdc,
                bitmap,
                0,
                height as u32,
                Some(pixels.as_mut_ptr() as *mut c_void),
                &mut info,
                DIB_RGB_COLORS,
            );
            ReleaseDC(None, hdc);
            if rows == 0 {
                return None;
            }

            bgra_to_rgba(&mut pixels);
            Some((width as u32, height as u32, pixels))
        }
    }

    fn bgra_to_rgba(pixels: &mut [u8]) {
        let has_alpha = pixels.chunks_exact(4).any(|px| px[3] != 0);
        for px in pixels.chunks_exact_mut(4) {
            let (b, g, r, a) = (px[0], px[1], px[2], px[3]);
            let unpremultiply = |c: u8| ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8;
            let rgba = match (has_alpha, a) {
                (false, _) => [r, g, b, 255],
                (true, 0 | 255) => [r, g, b, a],
                (true, _) => [unpremultiply(r), unpremultiply(g), unpremultiply(b), a],
            };
            px.copy_from_slice(&rgba);
        }
    }

    fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
        writer.finish().ok()?;
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_url_path() {
        assert_eq!(
            path_from_uri("/C%3A%5CUsers%5Cme%5Choliday%20photo%20%231.jpg").as_deref(),
            Some("C:\\Users\\me\\holiday photo #1.jpg")
        );
        assert_eq!(
            path_from_uri("/%2Fhome%2Fme%2Fa.png").as_deref(),
            Some("/home/me/a.png")
        );
    }

    fn key(name: &str) -> Key {
        (name.to_owned(), None, Shape::Whole)
    }

    #[test]
    fn cache_evicts_least_recently_used() {
        let mut cache = Cache::new(10);
        cache.put(key("a"), vec![0; 4]);
        cache.put(key("b"), vec![0; 4]);
        assert!(cache.get(&key("a")).is_some());
        cache.put(key("c"), vec![0; 4]);
        assert!(cache.get(&key("b")).is_none());
        assert!(cache.get(&key("a")).is_some());
        assert!(cache.get(&key("c")).is_some());
        assert_eq!(cache.bytes, 8);
    }
}
