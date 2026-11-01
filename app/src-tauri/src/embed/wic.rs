use std::path::Path;

use windows::{
    Win32::{
        Foundation::GENERIC_READ,
        Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_WICPixelFormat24bppRGB, IWICImagingFactory, WICBitmapDitherTypeNone,
            WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
        },
        System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize},
    },
    core::HSTRING,
};

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

unsafe fn read(path: &Path) -> Result<(Vec<u8>, u32, u32), String> {
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
        let decoder = factory
            .CreateDecoderFromFilename(&HSTRING::from(path.as_os_str()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)
            .map_err(|e| format!("Windows can't read this picture (HEIC needs the \"HEIF Image Extensions\" from the Microsoft Store): {e}"))?;
        let frame = decoder.GetFrame(0).map_err(|e| e.to_string())?;
        let converter = factory.CreateFormatConverter().map_err(|e| e.to_string())?;
        converter
            .Initialize(
                &frame,
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
        let mut rgb = vec![0u8; width as usize * height as usize * 3];
        converter
            .CopyPixels(std::ptr::null(), width * 3, &mut rgb)
            .map_err(|e| e.to_string())?;
        Ok((rgb, width, height))
    }
}
