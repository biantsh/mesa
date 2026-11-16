use std::{
    fs::File,
    io::Read,
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
};

use hayro::{
    PixmapSettings, RenderCache, RenderSettings, hayro_interpret::InterpreterSettings, hayro_syntax::Pdf, render,
    vello_cpu::color::palette::css::WHITE,
};

const SIDE: u32 = 2000;
const MAX_BYTES: u64 = 200 * 1024 * 1024;
const INK: u8 = 200;
const INKED_PER_10_000: usize = 20;

pub struct Pages {
    pdf: Pdf,
}

impl Pages {
    pub fn open(path: &Path) -> Option<Self> {
        let mut bytes = Vec::new();
        File::open(path)
            .and_then(|file| file.take(MAX_BYTES).read_to_end(&mut bytes))
            .ok()?;
        let pdf = catch_unwind(AssertUnwindSafe(|| Pdf::new(bytes))).ok()?.ok()?;
        Some(Self { pdf })
    }

    pub fn draw(&self, index: usize) -> Option<(Vec<u8>, u32, u32)> {
        catch_unwind(AssertUnwindSafe(|| self.draw_page(index))).ok().flatten()
    }

    fn draw_page(&self, index: usize) -> Option<(Vec<u8>, u32, u32)> {
        let page = self.pdf.pages().get(index)?;
        let (width, height) = page.render_dimensions();
        if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
            return None;
        }
        let scale = SIDE as f32 / width.max(height);
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
        let (width, height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
        let rgb: Vec<u8> = pixmap
            .data_as_u8_slice()
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, _]| [r, g, b])
            .collect();
        (!blank(&rgb)).then_some((rgb, width, height))
    }
}

fn blank(rgb: &[u8]) -> bool {
    let pixels = rgb.len() / 3;
    let inked = rgb
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|pixel| pixel.iter().any(|&value| value < INK))
        .count();
    inked * 10_000 < pixels * INKED_PER_10_000
}
