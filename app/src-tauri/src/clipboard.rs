use std::path::PathBuf;

const HEADER_BYTES: usize = 20;

#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn drop_files(paths: &[PathBuf]) -> Vec<u8> {
    let mut bytes = vec![0u8; HEADER_BYTES];
    bytes[0..4].copy_from_slice(&(HEADER_BYTES as u32).to_le_bytes());
    bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
    for path in paths {
        for unit in path.to_string_lossy().encode_utf16() {
            bytes.extend(unit.to_le_bytes());
        }
        bytes.extend([0, 0]);
    }
    bytes.extend([0, 0]);
    bytes
}

#[cfg(windows)]
pub use windows_clipboard::copy_files;

#[cfg(target_os = "macos")]
pub use mac_clipboard::copy_files;

#[cfg(target_os = "linux")]
pub use linux_clipboard::copy_files;

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const URI_PATH: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b'/');

#[cfg(unix)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn file_uris(paths: &[PathBuf]) -> Vec<String> {
    use std::os::unix::ffi::OsStrExt;
    paths
        .iter()
        .filter(|path| path.is_absolute())
        .map(|path| {
            format!(
                "file://{}",
                percent_encoding::percent_encode(path.as_os_str().as_bytes(), URI_PATH)
            )
        })
        .collect()
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn uri_list(uris: &[String]) -> String {
    uris.iter().map(|uri| format!("{uri}\r\n")).collect()
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn gnome_copied_files(uris: &[String]) -> String {
    std::iter::once("copy")
        .chain(uris.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(target_os = "linux")]
mod linux_clipboard {
    use std::{path::PathBuf, sync::mpsc, time::Duration};

    use super::{file_uris, gnome_copied_files, uri_list};

    pub fn copy_files(window: &tauri::WebviewWindow, paths: &[PathBuf]) -> Result<(), String> {
        let uris = file_uris(paths);
        if uris.is_empty() {
            return Err("Those files can't be copied.".into());
        }
        let text = paths
            .iter()
            .map(|path| path.to_string_lossy())
            .collect::<Vec<_>>()
            .join("\n");
        let (sender, answer) = mpsc::channel();
        window
            .run_on_main_thread(move || {
                let _ = sender.send(put(&uris, text));
            })
            .map_err(|e| e.to_string())?;
        answer
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|_| Err("The clipboard didn't answer.".into()))
    }

    fn put(uris: &[String], text: String) -> Result<(), String> {
        use gtk::{TargetEntry, TargetFlags, gdk};

        let targets = [
            TargetEntry::new("x-special/gnome-copied-files", TargetFlags::empty(), 0),
            TargetEntry::new("text/uri-list", TargetFlags::empty(), 1),
            TargetEntry::new("UTF8_STRING", TargetFlags::empty(), 2),
            TargetEntry::new("text/plain;charset=utf-8", TargetFlags::empty(), 2),
            TargetEntry::new("text/plain", TargetFlags::empty(), 2),
        ];
        let payloads = [gnome_copied_files(uris), uri_list(uris), text];
        let clipboard = gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD);
        let taken = clipboard.set_with_data(&targets, move |_, selection, kind| {
            if let Some(payload) = payloads.get(kind as usize) {
                selection.set(&selection.target(), 8, payload.as_bytes());
            }
        });
        if taken {
            Ok(())
        } else {
            Err("The clipboard didn't take the files.".into())
        }
    }
}

#[cfg(target_os = "macos")]
mod mac_clipboard {
    use std::path::PathBuf;

    use objc2::{rc::Retained, runtime::ProtocolObject};
    use objc2_app_kit::{NSPasteboard, NSPasteboardWriting};
    use objc2_foundation::{NSArray, NSString, NSURL};

    pub fn copy_files(paths: &[PathBuf]) -> Result<(), String> {
        write_to(&NSPasteboard::generalPasteboard(), paths)
    }

    fn write_to(pasteboard: &NSPasteboard, paths: &[PathBuf]) -> Result<(), String> {
        let urls: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = paths
            .iter()
            .filter_map(|path| path.to_str())
            .map(|path| {
                let url = NSURL::fileURLWithPath(&NSString::from_str(path));
                ProtocolObject::from_retained(url)
            })
            .collect();
        if urls.is_empty() {
            return Err("Those files can't be copied.".into());
        }
        let urls = NSArray::from_retained_slice(&urls);
        pasteboard.clearContents();
        let written = pasteboard.writeObjects(&urls);
        if written {
            Ok(())
        } else {
            Err("The clipboard didn't take the files.".into())
        }
    }
}

#[cfg(windows)]
mod windows_clipboard {
    use std::{path::PathBuf, thread, time::Duration};

    use windows::{
        Win32::{
            Foundation::{GlobalFree, HANDLE, HWND},
            System::{
                DataExchange::{
                    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
                },
                Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
            },
        },
        core::w,
    };

    use super::drop_files;

    const CF_HDROP: u32 = 15;
    const DROPEFFECT_COPY: u32 = 1;

    struct Open;

    impl Open {
        fn new(owner: HWND) -> Result<Self, String> {
            let mut last = String::new();
            for _ in 0..10 {
                match unsafe { OpenClipboard(Some(owner)) } {
                    Ok(()) => return Ok(Self),
                    Err(e) => last = e.to_string(),
                }
                thread::sleep(Duration::from_millis(15));
            }
            Err(format!("The clipboard is in use by another program ({last})."))
        }
    }

    impl Drop for Open {
        fn drop(&mut self) {
            let _ = unsafe { CloseClipboard() };
        }
    }

    fn set(format: u32, bytes: &[u8]) -> Result<(), String> {
        // SAFETY: the memory is either handed to the clipboard, which then owns it, or freed here.
        unsafe {
            let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).map_err(|e| e.to_string())?;
            let target = GlobalLock(memory);
            if target.is_null() {
                let _ = GlobalFree(Some(memory));
                return Err("Out of memory.".into());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), target.cast::<u8>(), bytes.len());
            let _ = GlobalUnlock(memory);
            if let Err(e) = SetClipboardData(format, Some(HANDLE(memory.0))) {
                let _ = GlobalFree(Some(memory));
                return Err(e.to_string());
            }
        }
        Ok(())
    }

    pub fn copy_files(owner: HWND, paths: &[PathBuf]) -> Result<(), String> {
        let drop_effect = unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) };
        let _clipboard = Open::new(owner)?;
        unsafe { EmptyClipboard() }.map_err(|e| e.to_string())?;
        set(CF_HDROP, &drop_files(paths))?;
        set(drop_effect, &DROPEFFECT_COPY.to_le_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(bytes: &[u8]) -> Vec<u16> {
        bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect()
    }

    #[test]
    fn dropfiles_layout() {
        let bytes = drop_files(&[PathBuf::from("C:\\a\\one.jpg"), PathBuf::from("D:\\ö\\two.png")]);
        assert_eq!(
            u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize,
            HEADER_BYTES
        );
        assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 1);
        assert!(bytes[4..16].iter().all(|&b| b == 0));

        let names = units(&bytes[HEADER_BYTES..]);
        let text = String::from_utf16(&names).unwrap();
        assert_eq!(text, "C:\\a\\one.jpg\0D:\\ö\\two.png\0\0");
    }
}
