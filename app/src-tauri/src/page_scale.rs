#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(target_os = "linux")]
static FACTOR: AtomicU64 = AtomicU64::new(0x3FF0_0000_0000_0000);

pub fn factor() -> f64 {
    #[cfg(target_os = "linux")]
    {
        f64::from_bits(FACTOR.load(Ordering::Relaxed))
    }
    #[cfg(not(target_os = "linux"))]
    {
        1.0
    }
}

#[cfg(target_os = "linux")]
pub fn watch() {
    use gtk::prelude::*;

    let Some(settings) = gtk::Settings::default() else {
        return;
    };
    let read =
        |settings: &gtk::Settings| FACTOR.store(from_xft_dpi(settings.gtk_xft_dpi()).to_bits(), Ordering::Relaxed);
    read(&settings);
    settings.connect_gtk_xft_dpi_notify(read);
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn from_xft_dpi(xft_dpi: i32) -> f64 {
    if xft_dpi <= 0 {
        return 1.0;
    }
    let factor = f64::from(xft_dpi) / 1024.0 / 96.0;
    if (0.5..=4.0).contains(&factor) { factor } else { 1.0 }
}
