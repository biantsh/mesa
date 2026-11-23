use std::path::Path;

use windows::{
    Win32::{
        Foundation::GENERIC_READ,
        Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_WICPixelFormat24bppRGB, IWICBitmapSource, IWICImagingFactory,
            WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom,
            WICDecodeMetadataCacheOnDemand,
        },
        System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize},
    },
    core::{HSTRING, Interface},
};

const LONGEST_SIDE: u32 = 8192;

pub fn decode(path: &Path) -> Result<(Vec<u8>, u32, u32), String> {
    unsafe {
        let com = CoInitializeEx(None, COINIT_MULTITHREADED);
        let result = read(path);
        if com.is_ok() {
            CoUninitialize();
        }
        result
    }
}

fn shrunk(width: u32, height: u32) -> (u32, u32) {
    let longest = width.max(height);
    if longest <= LONGEST_SIDE {
        return (width, height);
    }
    let side =
        |n: u32| ((u64::from(n) * u64::from(LONGEST_SIDE) + u64::from(longest) / 2) / u64::from(longest)).max(1) as u32;
    (side(width), side(height))
}

unsafe fn read(path: &Path) -> Result<(Vec<u8>, u32, u32), String> {
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
        let decoder = factory
            .CreateDecoderFromFilename(&HSTRING::from(path.as_os_str()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)
            .map_err(|e| format!("Windows can't read this picture (HEIC needs the \"HEIF Image Extensions\" from the Microsoft Store): {e}"))?;
        let frame = decoder.GetFrame(0).map_err(|e| e.to_string())?;
        let (mut width, mut height) = (0, 0);
        frame.GetSize(&mut width, &mut height).map_err(|e| e.to_string())?;
        if width == 0 || height == 0 {
            return Err("the picture is empty".into());
        }
        let (to_width, to_height) = shrunk(width, height);
        let source: IWICBitmapSource = if (to_width, to_height) == (width, height) {
            frame.cast().map_err(|e| e.to_string())?
        } else {
            let scaler = factory.CreateBitmapScaler().map_err(|e| e.to_string())?;
            scaler
                .Initialize(&frame, to_width, to_height, WICBitmapInterpolationModeFant)
                .map_err(|e| e.to_string())?;
            scaler.cast().map_err(|e| e.to_string())?
        };
        let converter = factory.CreateFormatConverter().map_err(|e| e.to_string())?;
        converter
            .Initialize(
                &source,
                &GUID_WICPixelFormat24bppRGB,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .map_err(|e| e.to_string())?;
        let (mut width, mut height) = (0, 0);
        converter.GetSize(&mut width, &mut height).map_err(|e| e.to_string())?;
        if width == 0 || height == 0 {
            return Err("the picture is empty".into());
        }
        if width.max(height) > LONGEST_SIDE {
            return Err(format!("the picture is too big to read ({width} x {height})"));
        }
        let mut rgb = vec![0u8; width as usize * height as usize * 3];
        converter
            .CopyPixels(std::ptr::null(), width * 3, &mut rgb)
            .map_err(|e| e.to_string())?;
        Ok((rgb, width, height))
    }
}

#[cfg(test)]
mod tests {
    use super::shrunk;

    #[test]
    fn shrinks_only_pictures_over_the_longest_side() {
        assert_eq!(shrunk(4000, 3000), (4000, 3000));
        assert_eq!(shrunk(8192, 100), (8192, 100));
        assert_eq!(shrunk(16384, 12288), (8192, 6144));
        assert_eq!(shrunk(65535, 65535), (8192, 8192));
        assert_eq!(shrunk(100_000, 1), (8192, 1));
    }
}
