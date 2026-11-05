use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};

use image::{ImageFormat, Rgba, RgbaImage, imageops::FilterType};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use windows::{
    Data::Xml::Dom::XmlDocument,
    Foundation::TypedEventHandler,
    UI::Notifications::{
        NotificationData, NotificationUpdateResult, ToastActivatedEventArgs, ToastDismissalReason,
        ToastDismissedEventArgs, ToastNotification, ToastNotificationManager, ToastNotifier,
    },
    Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx},
    core::{HSTRING, IInspectable, Interface},
};

use crate::{
    progress::Progress,
    surface::{Action, Gone, Surface, lines},
};

const POWERSHELL: &str = "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";

const ICON: &[u8] = include_bytes!("../icons/icon.png");
const LOGO_FILE: &str = "notification-logo.png";
const LOGO_SIZE: u32 = 128;
const LOGO_MARGIN: f32 = 0.04;
const TAG: &str = "indexing";
const GROUP: &str = "mesa";

pub struct Toast {
    notifier: ToastNotifier,
    app_id: HSTRING,
    current: Option<ToastNotification>,
    data_dir: PathBuf,
    logo: Option<Option<String>>,
    sequence: u32,
    gone: Arc<AtomicU8>,
    act: Arc<dyn Fn(Action) + Send + Sync>,
}

impl Toast {
    pub fn new(
        identifier: &str,
        data_dir: &Path,
        act: Arc<dyn Fn(Action) + Send + Sync>,
    ) -> windows::core::Result<Self> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let app_id = HSTRING::from(if cfg!(debug_assertions) { POWERSHELL } else { identifier });
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&app_id)?;
        Ok(Self {
            notifier,
            app_id,
            current: None,
            data_dir: data_dir.to_owned(),
            logo: None,
            sequence: 0,
            gone: Arc::new(AtomicU8::new(0)),
            act,
        })
    }

    fn logo(&mut self) -> Option<String> {
        let data_dir = &self.data_dir;
        self.logo
            .get_or_insert_with(|| match save_logo(&data_dir.join(LOGO_FILE)) {
                Ok(path) => Some(file_address(path)),
                Err(e) => {
                    eprintln!("Mesa: can't make the logo for the indexing notification, which has none: {e}");
                    None
                }
            })
            .clone()
    }

    fn data(&mut self, progress: &Progress) -> windows::core::Result<NotificationData> {
        let said = lines(progress);
        let data = NotificationData::new()?;
        let values = data.Values()?;
        values.Insert(&HSTRING::from("progressTitle"), &HSTRING::from(said.title))?;
        let value = said
            .fraction
            .map_or_else(|| "indeterminate".to_owned(), |fraction| fraction.to_string());
        values.Insert(&HSTRING::from("progressValue"), &HSTRING::from(value))?;
        values.Insert(&HSTRING::from("progressValueString"), &HSTRING::from(said.figures))?;
        values.Insert(&HSTRING::from("progressStatus"), &HSTRING::from(said.status))?;
        self.sequence += 1;
        data.SetSequenceNumber(self.sequence)?;
        Ok(data)
    }

    fn post(&mut self, progress: &Progress) -> windows::core::Result<()> {
        let xml = XmlDocument::new()?;
        xml.LoadXml(&HSTRING::from(toast_xml(progress.paused, self.logo().as_deref())))?;
        let toast = ToastNotification::CreateToastNotification(&xml)?;
        toast.SetTag(&HSTRING::from(TAG))?;
        toast.SetGroup(&HSTRING::from(GROUP))?;
        toast.SetData(&self.data(progress)?)?;
        toast.SetSuppressPopup(true)?;
        toast.SetExpiresOnReboot(true)?;

        let (gone, act) = (self.gone.clone(), self.act.clone());
        toast.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(
            move |_, args| {
                let pressed = args
                    .as_ref()
                    .and_then(|args| args.cast::<ToastActivatedEventArgs>().ok())
                    .and_then(|args| args.Arguments().ok())
                    .map(|arguments| arguments.to_string());
                gone.fetch_max(2, Ordering::SeqCst);
                act(match pressed.as_deref() {
                    Some("pause") => Action::Pause,
                    Some("resume") => Action::Resume,
                    _ => Action::Open,
                });
                Ok(())
            },
        ))?;
        let gone = self.gone.clone();
        toast.Dismissed(&TypedEventHandler::<ToastNotification, ToastDismissedEventArgs>::new(
            move |_, args| {
                if args.as_ref().and_then(|args| args.Reason().ok()) == Some(ToastDismissalReason::UserCanceled) {
                    gone.fetch_max(1, Ordering::SeqCst);
                }
                Ok(())
            },
        ))?;

        self.notifier.Show(&toast)?;
        self.current = Some(toast);
        Ok(())
    }
}

impl Surface for Toast {
    fn show(&mut self, progress: &Progress) {
        self.gone.store(0, Ordering::SeqCst);
        if let Err(e) = self.post(progress) {
            eprintln!("Mesa: can't post the indexing notification: {e}");
        }
    }

    fn update(&mut self, progress: &Progress) -> bool {
        let result = self.data(progress).and_then(|data| {
            self.notifier
                .UpdateWithTagAndGroup(&data, &HSTRING::from(TAG), &HSTRING::from(GROUP))
        });
        match result {
            Ok(result) => result != NotificationUpdateResult::NotificationNotFound,
            Err(e) => {
                eprintln!("Mesa: can't update the indexing notification: {e}");
                true
            }
        }
    }

    fn hide(&mut self) {
        self.current = None;
        let removed = ToastNotificationManager::History().and_then(|history| {
            history.RemoveGroupedTagWithId(&HSTRING::from(TAG), &HSTRING::from(GROUP), &self.app_id)
        });
        if let Err(e) = removed {
            eprintln!("Mesa: can't take away the indexing notification: {e}");
        }
        self.gone.store(0, Ordering::SeqCst);
    }

    fn gone(&mut self) -> Option<Gone> {
        match self.gone.swap(0, Ordering::SeqCst) {
            1 => Some(Gone::Dismissed),
            2 => Some(Gone::Used),
            _ => None,
        }
    }
}

fn save_logo(path: &Path) -> Result<&Path, String> {
    let png = logo_image(ICON)?;
    if fs::read(path).ok().as_deref() != Some(png.as_slice()) {
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        }
        fs::write(path, &png).map_err(|e| format!("can't write {}: {e}", path.display()))?;
    }
    Ok(path)
}

fn logo_image(icon: &[u8]) -> Result<Vec<u8>, String> {
    let icon = image::load_from_memory_with_format(icon, ImageFormat::Png)
        .map_err(|e| e.to_string())?
        .to_rgba8();

    let (mut left, mut top, mut right, mut bottom) = (icon.width(), icon.height(), 0, 0);
    for (x, y, Rgba([_, _, _, alpha])) in icon.enumerate_pixels() {
        if *alpha > 0 {
            (left, top, right, bottom) = (left.min(x), top.min(y), right.max(x + 1), bottom.max(y + 1));
        }
    }
    if right <= left || bottom <= top {
        return Err("there is nothing in the icon".into());
    }

    let (width, height) = (right - left, bottom - top);
    let side = (width.max(height) as f32 * (1.0 + 2.0 * LOGO_MARGIN)).ceil() as u32;
    let mut square = RgbaImage::new(side, side);
    image::imageops::overlay(
        &mut square,
        &image::imageops::crop_imm(&icon, left, top, width, height).to_image(),
        i64::from((side - width) / 2),
        i64::from((side - height) / 2),
    );
    let small = shrink(&square, LOGO_SIZE);

    let mut png = Vec::new();
    small
        .write_to(&mut io::Cursor::new(&mut png), ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(png)
}

fn shrink(picture: &RgbaImage, size: u32) -> RgbaImage {
    let weighed = RgbaImage::from_fn(picture.width(), picture.height(), |x, y| {
        let Rgba([red, green, blue, alpha]) = *picture.get_pixel(x, y);
        let weigh = |colour: u8| (f32::from(colour) * f32::from(alpha) / 255.0).round() as u8;
        Rgba([weigh(red), weigh(green), weigh(blue), alpha])
    });
    let mut small = image::imageops::resize(&weighed, size, size, FilterType::Triangle);
    for Rgba([red, green, blue, alpha]) in small.pixels_mut() {
        if *alpha > 0 {
            let unweigh = |colour: u8| (f32::from(colour) * 255.0 / f32::from(*alpha)).round().min(255.0) as u8;
            (*red, *green, *blue) = (unweigh(*red), unweigh(*green), unweigh(*blue));
        }
    }
    small
}

fn file_address(path: &Path) -> String {
    const KEPT: &AsciiSet = &NON_ALPHANUMERIC
        .remove(b'/')
        .remove(b':')
        .remove(b'.')
        .remove(b'-')
        .remove(b'_');
    let path = path.to_string_lossy().replace('\\', "/");
    format!("file:///{}", utf8_percent_encode(path.trim_start_matches("//?/"), KEPT))
}

fn toast_xml(paused: bool, logo: Option<&str>) -> String {
    let (label, arguments) = if paused {
        ("Resume", "resume")
    } else {
        ("Pause", "pause")
    };
    let logo = logo
        .map(|address| format!(r#"<image placement="appLogoOverride" src="{address}"/>"#))
        .unwrap_or_default();
    format!(
        r#"<toast launch="open"><visual><binding template="ToastGeneric">{logo}<text>Indexing your files</text><progress title="{{progressTitle}}" value="{{progressValue}}" valueStringOverride="{{progressValueString}}" status="{{progressStatus}}"/></binding></visual><actions><action content="{label}" arguments="{arguments}" activationType="background"/><action content="Open Mesa" arguments="open" activationType="foreground"/></actions></toast>"#
    )
}
