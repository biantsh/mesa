use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    os::unix::ffi::OsStrExt,
    path::Path,
    ptr,
};

const LONGEST_SIDE: c_int = 8192;
const COLORSPACE_RGB: c_int = 1;
const CHROMA_INTERLEAVED_RGB: c_int = 10;
const CHANNEL_INTERLEAVED: c_int = 10;

#[repr(C)]
struct HeifError {
    code: c_int,
    subcode: c_int,
    message: *const c_char,
}

#[repr(C)]
struct Context {
    _opaque: [u8; 0],
}

#[repr(C)]
struct Handle {
    _opaque: [u8; 0],
}

#[repr(C)]
struct Image {
    _opaque: [u8; 0],
}

#[link(name = "heif")]
unsafe extern "C" {
    fn heif_context_alloc() -> *mut Context;
    fn heif_context_free(context: *mut Context);
    fn heif_context_read_from_file(context: *mut Context, filename: *const c_char, options: *const c_void)
    -> HeifError;
    fn heif_context_get_primary_image_handle(context: *mut Context, handle: *mut *mut Handle) -> HeifError;
    fn heif_image_handle_get_width(handle: *const Handle) -> c_int;
    fn heif_image_handle_get_height(handle: *const Handle) -> c_int;
    fn heif_image_handle_release(handle: *const Handle);
    fn heif_decode_image(
        handle: *const Handle,
        image: *mut *mut Image,
        colorspace: c_int,
        chroma: c_int,
        options: *const c_void,
    ) -> HeifError;
    fn heif_image_get_width(image: *const Image, channel: c_int) -> c_int;
    fn heif_image_get_height(image: *const Image, channel: c_int) -> c_int;
    fn heif_image_get_plane_readonly(image: *const Image, channel: c_int, stride: *mut c_int) -> *const u8;
    fn heif_image_release(image: *const Image);
}

struct Owned<T> {
    raw: *mut T,
    free: unsafe extern "C" fn(*mut T),
}

impl<T> Drop for Owned<T> {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { (self.free)(self.raw) };
        }
    }
}

unsafe extern "C" fn release_handle(handle: *mut Handle) {
    unsafe { heif_image_handle_release(handle) }
}

unsafe extern "C" fn release_image(image: *mut Image) {
    unsafe { heif_image_release(image) }
}

fn checked(error: HeifError, what: &str) -> Result<(), String> {
    if error.code == 0 {
        return Ok(());
    }
    let message = if error.message.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(error.message) }.to_string_lossy().into_owned()
    };
    Err(format!("{what} ({message})"))
}

pub fn decode(path: &Path) -> Result<(Vec<u8>, u32, u32), String> {
    let name = CString::new(path.as_os_str().as_bytes()).map_err(|_| "the picture's name can't be used")?;
    unsafe {
        let context = Owned {
            raw: heif_context_alloc(),
            free: heif_context_free,
        };
        if context.raw.is_null() {
            return Err("libheif couldn't be started".into());
        }
        checked(
            heif_context_read_from_file(context.raw, name.as_ptr(), ptr::null()),
            "libheif can't read this picture",
        )?;
        let mut handle = Owned {
            raw: ptr::null_mut(),
            free: release_handle,
        };
        checked(
            heif_context_get_primary_image_handle(context.raw, &mut handle.raw),
            "there is no picture in it",
        )?;
        let (width, height) = (
            heif_image_handle_get_width(handle.raw),
            heif_image_handle_get_height(handle.raw),
        );
        if width <= 0 || height <= 0 || width > LONGEST_SIDE * 4 || height > LONGEST_SIDE * 4 {
            return Err(format!("the picture is too big to read ({width} x {height})"));
        }
        let mut image = Owned {
            raw: ptr::null_mut(),
            free: release_image,
        };
        checked(
            heif_decode_image(
                handle.raw,
                &mut image.raw,
                COLORSPACE_RGB,
                CHROMA_INTERLEAVED_RGB,
                ptr::null(),
            ),
            "libheif can't decode this picture",
        )?;
        let (width, height) = (
            heif_image_get_width(image.raw, CHANNEL_INTERLEAVED),
            heif_image_get_height(image.raw, CHANNEL_INTERLEAVED),
        );
        let mut stride: c_int = 0;
        let plane = heif_image_get_plane_readonly(image.raw, CHANNEL_INTERLEAVED, &mut stride);
        if plane.is_null() || width <= 0 || height <= 0 || stride < width * 3 {
            return Err("libheif gave no picture".into());
        }
        let (width, height, stride) = (width as usize, height as usize, stride as usize);
        let mut rgb = Vec::with_capacity(width * height * 3);
        for row in 0..height {
            rgb.extend_from_slice(std::slice::from_raw_parts(plane.add(row * stride), width * 3));
        }
        let (rgb, width, height) = smaller(rgb, width as u32, height as u32);
        Ok((rgb, width, height))
    }
}

fn smaller(rgb: Vec<u8>, width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    let longest = LONGEST_SIDE as u32;
    if width <= longest && height <= longest {
        return (rgb, width, height);
    }
    let Some(picture) = image::RgbImage::from_raw(width, height, rgb) else {
        return (Vec::new(), 0, 0);
    };
    let picture = image::DynamicImage::ImageRgb8(picture)
        .resize(longest, longest, image::imageops::FilterType::Triangle)
        .into_rgb8();
    let (width, height) = picture.dimensions();
    (picture.into_raw(), width, height)
}
