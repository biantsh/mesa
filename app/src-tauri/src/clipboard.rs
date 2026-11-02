use std::path::PathBuf;

const HEADER_BYTES: usize = 20;

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
            .chunks_exact(2)
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
