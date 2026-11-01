use std::{
    panic,
    sync::{Arc, Mutex, mpsc},
    thread,
};

use tauri::{
    UriSchemeResponder,
    http::{Request, Response},
};

const PREVIEW_SIZE: u32 = 256;

type Job = (String, UriSchemeResponder);

pub struct PreviewServer {
    jobs: mpsc::Sender<Job>,
}

impl PreviewServer {
    pub fn start() -> Self {
        let (jobs, queue) = mpsc::channel::<Job>();
        let queue = Arc::new(Mutex::new(queue));
        let workers = thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 8);
        for _ in 0..workers {
            let queue = Arc::clone(&queue);
            thread::spawn(move || {
                loop {
                    let job = queue.lock().unwrap_or_else(|e| e.into_inner()).recv();
                    let Ok((path, responder)) = job else { break };
                    let preview = panic::catch_unwind(|| render(&path, PREVIEW_SIZE)).ok().flatten();
                    responder.respond(response(preview));
                }
            });
        }
        Self { jobs }
    }

    pub fn handle(&self, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
        match path_from_uri(request.uri().path()) {
            Some(path) => {
                let _ = self.jobs.send((path, responder));
            }
            None => responder.respond(response(None)),
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
    #[cfg(not(windows))]
    {
        let _ = (path, size);
        None
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
}
