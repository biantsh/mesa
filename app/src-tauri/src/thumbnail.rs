use std::{
    collections::{BTreeMap, HashMap},
    fs, panic,
    sync::{Arc, Condvar, Mutex, PoisonError},
    thread,
    time::SystemTime,
};

use tauri::{
    UriSchemeResponder,
    http::{Request, Response},
};

const PREVIEW_SIZE: u32 = 256;
const CACHE_BYTES: usize = 128 * 1024 * 1024;
type Job = (Key, UriSchemeResponder);

pub struct PreviewServer {
    jobs: Arc<Stack<Job>>,
    cache: Arc<Mutex<Cache>>,
}

impl PreviewServer {
    pub fn start() -> Self {
        let jobs = Arc::new(Stack::<Job>::default());
        let cache = Arc::new(Mutex::new(Cache::new(CACHE_BYTES)));
        let workers = thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 8);
        for _ in 0..workers {
            let jobs = Arc::clone(&jobs);
            let cache = Arc::clone(&cache);
            thread::spawn(move || {
                loop {
                    let (key, responder) = jobs.pop();
                    let preview = panic::catch_unwind(|| render(&key.0, PREVIEW_SIZE)).ok().flatten();
                    if let Some(png) = &preview {
                        cache
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .put(key, png.clone());
                    }
                    responder.respond(response(preview));
                }
            });
        }
        Self { jobs, cache }
    }

    pub fn handle(&self, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
        let Some(path) = path_from_uri(request.uri().path()) else {
            return responder.respond(response(None));
        };
        let changed = modified(&path);
        let key = (path, changed);
        let kept = self.cache.lock().unwrap_or_else(PoisonError::into_inner).get(&key);
        match kept {
            Some(png) => responder.respond(response(Some(png))),
            None => self.jobs.push((key, responder)),
        }
    }
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

type Key = (String, Option<SystemTime>);

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

fn response(png: Option<Vec<u8>>) -> Response<Vec<u8>> {
    let builder = Response::builder().header("Access-Control-Allow-Origin", "*");
    let built = match png {
        Some(bytes) => builder
            .status(200)
            .header("Content-Type", "image/png")
            .header("Cache-Control", "private, max-age=300")
            .body(bytes),
        None => builder.status(404).body(Vec::new()),
    };
    built.expect("a response built from fixed headers is valid")
}

pub fn render(path: &str, size: u32) -> Option<Vec<u8>> {
    #[cfg(windows)]
    {
        shell::render(path, size)
    }
    #[cfg(target_os = "macos")]
    {
        quicklook::render(path, size)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (path, size);
        None
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
        (name.to_owned(), None)
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
