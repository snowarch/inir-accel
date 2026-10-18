//! `cv2.imread(path, IMREAD_COLOR | IMREAD_GRAYSCALE)` as opencv-contrib-python 5.0.0 reads the formats wallpapers
//! come in: JPEG through libjpeg-turbo (islow, fancy upsampling, Y alone for grey), PNG through the wheel's own
//! libpng with OpenCV's transforms (its RGB->grey honours gAMA/sRGB, so it is called, not rewritten), WebP as libwebp
//! decodes it plus `cvtColor(BGR2GRAY)`, then the EXIF orientation from JPEG APP1 and PNG eXIf.
//!
//! `Read::None` is imread's None (missing or unreadable file, a PNG libpng refuses); `Read::Delegate` is everything the
//! port does not claim to read the same way (other formats, CMYK, APNG, animated WebP, odd chunk sizes).

use super::Mat;
use std::ffi::{c_char, c_int, c_void};
use std::sync::OnceLock;

pub enum Read {
    Ok(Mat),
    None,
    Delegate,
}

/// OpenCV's `validateInputImageSize` limits; past them imread raises instead of returning.
const MAX_SIDE: usize = 1 << 20;
const MAX_PIXELS: usize = 1 << 30;

pub fn imread(path: &str, gray: bool, site_packages: &str) -> Read {
    let Ok(bytes) = std::fs::read(path) else { return Read::None };
    let (mat, orientation) = if bytes.starts_with(b"\xFF\xD8\xFF") {
        match jpeg(&bytes, gray) {
            Some(m) => (m, jpeg_orientation(&bytes)),
            None => return Read::Delegate,
        }
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        match png(&bytes, gray, site_packages) {
            Png::Ok(m, o) => (m, o),
            Png::Failed => return Read::None,
            Png::Delegate => return Read::Delegate,
        }
    } else if bytes.len() >= 30 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        match webp(&bytes, gray) {
            Some(m) => (m, None),
            None => return Read::Delegate,
        }
    } else {
        // Empty, a directory read as nothing, or a signature some other OpenCV decoder may claim.
        return if bytes.is_empty() { Read::None } else { Read::Delegate };
    };
    Read::Ok(orient(mat, orientation))
}

fn sane(w: usize, h: usize) -> bool {
    w > 0 && h > 0 && w <= MAX_SIDE && h <= MAX_SIDE && w * h <= MAX_PIXELS
}

// ---------------------------------------------------------------- JPEG

fn jpeg(bytes: &[u8], gray: bool) -> Option<Mat> {
    const TJPF_BGR: c_int = 1;
    const TJPF_GRAY: c_int = 6;
    let cn = if gray { 1 } else { 3 };
    let (w, h, data) = crate::material::pixels::turbo_decode(bytes, if gray { TJPF_GRAY } else { TJPF_BGR }, cn)?;
    sane(w, h).then_some(Mat { w, h, cn, data })
}

/// The orientation tag of the first APP1 "Exif" segment that carries one, read up to the scan as libjpeg saves them.
fn jpeg_orientation(bytes: &[u8]) -> Option<u16> {
    let mut exif = Exif::default();
    let mut pos = 2;
    while pos + 4 <= bytes.len() {
        if bytes[pos] != 0xFF {
            break;
        }
        let marker = bytes[pos + 1];
        if marker == 0xFF {
            pos += 1;
            continue;
        }
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        if (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            pos += 2;
            continue;
        }
        let len = u16::from_be_bytes([bytes[pos + 2], bytes[pos + 3]]) as usize;
        if len < 2 {
            break;
        }
        let body = &bytes[(pos + 4).min(bytes.len())..(pos + 2 + len).min(bytes.len())];
        if marker == 0xE1 && body.len() > 6 && body.starts_with(b"Exif\0\0") {
            exif.parse(&body[6..]);
        }
        pos += 2 + len;
    }
    exif.orientation
}

/// OpenCV's ExifReader (exif.cpp): IFD0 only, entries go into a map that keeps the first of each tag, and a field read
/// out of bounds stops the parse but keeps what was already in.
#[derive(Default)]
struct Exif {
    orientation: Option<u16>,
}

impl Exif {
    fn parse(&mut self, d: &[u8]) {
        if d.is_empty() {
            return;
        }
        let intel = !(d.len() > 1 && d[0] != d[1]) && d[0] == b'I';
        let u16_at = |o: usize| -> Option<u32> {
            if o + 1 >= d.len() {
                return None;
            }
            Some(if intel { d[o] as u32 + ((d[o + 1] as u32) << 8) } else { ((d[o] as u32) << 8) + d[o + 1] as u32 })
        };
        let u32_at = |o: usize| -> Option<u32> {
            if o + 3 >= d.len() {
                return None;
            }
            let b = [d[o] as u32, d[o + 1] as u32, d[o + 2] as u32, d[o + 3] as u32];
            Some(if intel { b[0] + (b[1] << 8) + (b[2] << 16) + (b[3] << 24) } else { (b[0] << 24) + (b[1] << 16) + (b[2] << 8) + b[3] })
        };
        let rationals = |o: usize, n: usize| -> Option<()> {
            let at = u32_at(o + 8)? as usize;
            for i in 0..n {
                u32_at(at + 8 * i)?;
                u32_at(at + 8 * i + 4)?;
            }
            Some(())
        };
        let string = |o: usize| -> Option<()> {
            let size = u32_at(o + 4)? as usize;
            let at = if size > 4 { u32_at(o + 8)? as usize } else { 8 };
            (at <= d.len() && at + size <= d.len()).then_some(())
        };
        let mut walk = || -> Option<()> {
            if u16_at(2)? != 0x2A {
                return Some(());
            }
            let mut offset = u32_at(4)? as usize;
            let count = u16_at(offset)?;
            offset += 2;
            let mut orientation = self.orientation;
            let result = (|| {
                for _ in 0..count {
                    match u16_at(offset)? {
                        0x010E | 0x010F | 0x0110 | 0x0131 | 0x0132 | 0x8298 => string(offset)?,
                        0x0112 => {
                            let v = u16_at(offset + 8)? as u16;
                            orientation.get_or_insert(v);
                        }
                        0x011A | 0x011B => rationals(offset, 1)?,
                        0x013E => rationals(offset, 2)?,
                        0x013F | 0x0214 => rationals(offset, 6)?,
                        0x0211 => rationals(offset, 3)?,
                        0x0128 | 0x0213 => {
                            u16_at(offset + 8)?;
                        }
                        _ => {}
                    }
                    offset += 12;
                }
                Some(())
            })();
            self.orientation = orientation;
            result
        };
        let _ = walk();
    }
}

/// ExifTransform: flips and transposes for orientations 2..8, nothing for anything else.
fn orient(m: Mat, orientation: Option<u16>) -> Mat {
    match orientation {
        Some(2) => flip(&m, true, false),
        Some(3) => flip(&m, true, true),
        Some(4) => flip(&m, false, true),
        Some(5) => transpose(&m),
        Some(6) => flip(&transpose(&m), true, false),
        Some(7) => flip(&transpose(&m), true, true),
        Some(8) => flip(&transpose(&m), false, true),
        _ => m,
    }
}

fn flip(m: &Mat, horizontal: bool, vertical: bool) -> Mat {
    let mut data = vec![0u8; m.data.len()];
    for y in 0..m.h {
        let sy = if vertical { m.h - 1 - y } else { y };
        for x in 0..m.w {
            let sx = if horizontal { m.w - 1 - x } else { x };
            let (d, s) = ((y * m.w + x) * m.cn, (sy * m.w + sx) * m.cn);
            data[d..d + m.cn].copy_from_slice(&m.data[s..s + m.cn]);
        }
    }
    Mat { w: m.w, h: m.h, cn: m.cn, data }
}

fn transpose(m: &Mat) -> Mat {
    let mut data = vec![0u8; m.data.len()];
    for y in 0..m.h {
        for x in 0..m.w {
            let (d, s) = ((x * m.h + y) * m.cn, (y * m.w + x) * m.cn);
            data[d..d + m.cn].copy_from_slice(&m.data[s..s + m.cn]);
        }
    }
    Mat { w: m.h, h: m.w, cn: m.cn, data }
}

// ---------------------------------------------------------------- WebP

fn webp(bytes: &[u8], gray: bool) -> Option<Mat> {
    // VP8X with the animation flag goes through WebPAnimDecoder.
    if &bytes[12..16] == b"VP8X" && bytes[20] & 0x02 != 0 {
        return None;
    }
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::WebP).ok()?;
    let (w, h) = (img.width() as usize, img.height() as usize);
    if !sane(w, h) {
        return None;
    }
    let (src, scn) = match img {
        image::DynamicImage::ImageRgb8(b) => (b.into_raw(), 3),
        image::DynamicImage::ImageRgba8(b) => (b.into_raw(), 4),
        _ => return None,
    };
    let data = if gray {
        src.chunks_exact(scn).map(|p| gray_of(p[2], p[1], p[0])).collect()
    } else {
        src.chunks_exact(scn).flat_map(|p| [p[2], p[1], p[0]]).collect()
    };
    Some(Mat { w, h, cn: if gray { 1 } else { 3 }, data })
}

/// `cvtColor(COLOR_BGR2GRAY)` on 8 bits in OpenCV 5 (RGB2Gray<uchar>): 15 fractional bits (BY15, GY15, RY15), rounded.
pub fn gray_of(b: u8, g: u8, r: u8) -> u8 {
    ((b as u32 * 3735 + g as u32 * 19235 + r as u32 * 9798 + (1 << 14)) >> 15) as u8
}

// ---------------------------------------------------------------- PNG through the wheel's libpng

enum Png {
    Ok(Mat, Option<u16>),
    Failed,
    Delegate,
}

type PngPtr = *mut c_void;
type ErrFn = Option<unsafe extern "C-unwind" fn(PngPtr, *const c_char)>;
type ReadFn = unsafe extern "C-unwind" fn(PngPtr, *mut u8, usize);
type LongjmpFn = unsafe extern "C-unwind" fn(*mut c_void, c_int);

struct LibPng {
    create_read: unsafe extern "C-unwind" fn(*const c_char, *mut c_void, ErrFn, ErrFn) -> PngPtr,
    create_info: unsafe extern "C-unwind" fn(PngPtr) -> PngPtr,
    destroy_read: unsafe extern "C-unwind" fn(*mut PngPtr, *mut PngPtr, *mut PngPtr),
    set_longjmp_fn: unsafe extern "C-unwind" fn(PngPtr, LongjmpFn, usize) -> *mut c_void,
    set_read_fn: unsafe extern "C-unwind" fn(PngPtr, *mut c_void, ReadFn),
    get_io_ptr: unsafe extern "C-unwind" fn(PngPtr) -> *mut c_void,
    error: unsafe extern "C-unwind" fn(PngPtr, *const c_char),
    read_info: unsafe extern "C-unwind" fn(PngPtr, PngPtr),
    get_ihdr: unsafe extern "C-unwind" fn(PngPtr, PngPtr, *mut u32, *mut u32, *mut c_int, *mut c_int, *mut c_int, *mut c_int, *mut c_int) -> u32,
    set_strip_16: unsafe extern "C-unwind" fn(PngPtr),
    set_swap: unsafe extern "C-unwind" fn(PngPtr),
    set_strip_alpha: unsafe extern "C-unwind" fn(PngPtr),
    set_palette_to_rgb: unsafe extern "C-unwind" fn(PngPtr),
    set_expand_gray_1_2_4_to_8: unsafe extern "C-unwind" fn(PngPtr),
    set_bgr: unsafe extern "C-unwind" fn(PngPtr),
    set_gray_to_rgb: unsafe extern "C-unwind" fn(PngPtr),
    set_rgb_to_gray: unsafe extern "C-unwind" fn(PngPtr, c_int, f64, f64),
    set_interlace_handling: unsafe extern "C-unwind" fn(PngPtr) -> c_int,
    read_update_info: unsafe extern "C-unwind" fn(PngPtr, PngPtr),
    get_rowbytes: unsafe extern "C-unwind" fn(PngPtr, PngPtr) -> usize,
    read_image: unsafe extern "C-unwind" fn(PngPtr, *mut *mut u8),
    read_end: unsafe extern "C-unwind" fn(PngPtr, PngPtr),
    get_valid: unsafe extern "C-unwind" fn(PngPtr, PngPtr, u32) -> u32,
    get_exif_1: unsafe extern "C-unwind" fn(PngPtr, PngPtr, *mut u32, *mut *mut u8) -> u32,
}

/// The libpng cv2 links (`opencv_contrib_python.libs/libpng16-*.so.16.58.0`): the same build, so the same warnings
/// and the same grey.
fn libpng(site_packages: &str) -> Option<&'static LibPng> {
    static LIB: OnceLock<Option<LibPng>> = OnceLock::new();
    LIB.get_or_init(|| unsafe {
        let dir = std::path::Path::new(site_packages).join("opencv_contrib_python.libs");
        let file = std::fs::read_dir(&dir).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("libpng16-") && n.ends_with(".so.16.58.0"))
        })?;
        let cpath = std::ffi::CString::new(file.to_str()?).ok()?;
        let handle = libc::dlopen(cpath.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if handle.is_null() {
            return None;
        }
        macro_rules! sym {
            ($name:literal) => {{
                let p = libc::dlsym(handle, concat!($name, "\0").as_ptr() as *const c_char);
                if p.is_null() {
                    return None;
                }
                std::mem::transmute::<*mut c_void, _>(p)
            }};
        }
        Some(LibPng {
            create_read: sym!("png_create_read_struct"),
            create_info: sym!("png_create_info_struct"),
            destroy_read: sym!("png_destroy_read_struct"),
            set_longjmp_fn: sym!("png_set_longjmp_fn"),
            set_read_fn: sym!("png_set_read_fn"),
            get_io_ptr: sym!("png_get_io_ptr"),
            error: sym!("png_error"),
            read_info: sym!("png_read_info"),
            get_ihdr: sym!("png_get_IHDR"),
            set_strip_16: sym!("png_set_strip_16"),
            set_swap: sym!("png_set_swap"),
            set_strip_alpha: sym!("png_set_strip_alpha"),
            set_palette_to_rgb: sym!("png_set_palette_to_rgb"),
            set_expand_gray_1_2_4_to_8: sym!("png_set_expand_gray_1_2_4_to_8"),
            set_bgr: sym!("png_set_bgr"),
            set_gray_to_rgb: sym!("png_set_gray_to_rgb"),
            set_rgb_to_gray: sym!("png_set_rgb_to_gray"),
            set_interlace_handling: sym!("png_set_interlace_handling"),
            read_update_info: sym!("png_read_update_info"),
            get_rowbytes: sym!("png_get_rowbytes"),
            read_image: sym!("png_read_image"),
            read_end: sym!("png_read_end"),
            get_valid: sym!("png_get_valid"),
            get_exif_1: sym!("png_get_eXIf_1"),
        })
    })
    .as_ref()
}

/// What OpenCV's PngDecoder::readHeader checks with its own chunk reader before libpng sees the file.
fn png_prescan(bytes: &[u8]) -> Result<(), Png> {
    let chunk = |pos: usize| -> Option<(usize, &[u8])> {
        let head = bytes.get(pos..pos + 8)?;
        let size = u32::from_be_bytes(head[0..4].try_into().ok()?) as usize + 12;
        bytes.get(pos..pos + size)?;
        Some((size, &head[4..8]))
    };
    let Some((size, id)) = chunk(8) else { return Err(Png::Failed) };
    if id != b"IHDR" || size != 25 {
        return Err(Png::Failed);
    }
    let mut pos = 8 + size;
    loop {
        let Some((size, id)) = chunk(pos) else { return Err(Png::Failed) };
        match id {
            b"IDAT" => return Ok(()),
            b"acTL" | b"fcTL" | b"fdAT" | b"CgBI" => return Err(Png::Delegate),
            b"IHDR" => return Err(Png::Failed),
            b"bKGD" if size != 13 && size != 14 && size != 18 => return Err(Png::Failed),
            b"PLTE" | b"tEXt" | b"tRNS" | b"IEND" | b"bKGD" => {}
            _ if size > 8_000_000 => return Err(Png::Delegate),
            _ => {}
        }
        pos += size;
    }
}

struct Source<'a> {
    data: &'a [u8],
    pos: usize,
}

/// libpng's stdio reader, over the bytes: a short read is png_error("Read Error").
unsafe extern "C-unwind" fn read_source(png_ptr: PngPtr, out: *mut u8, len: usize) {
    let lib = LIB_FOR_CALLBACK.get().copied().flatten().expect("libpng loaded");
    unsafe {
        let src = &mut *((lib.get_io_ptr)(png_ptr) as *mut Source);
        if src.pos + len > src.data.len() {
            (lib.error)(png_ptr, c"Read Error".as_ptr());
            return;
        }
        std::ptr::copy_nonoverlapping(src.data.as_ptr().add(src.pos), out, len);
        src.pos += len;
    }
}

static LIB_FOR_CALLBACK: OnceLock<Option<&'static LibPng>> = OnceLock::new();

struct PngAbort;

/// Where OpenCV's setjmp lands: libpng has already printed "libpng error: ..." as the default handler does.
unsafe extern "C-unwind" fn png_abort(_buf: *mut c_void, _val: c_int) {
    std::panic::resume_unwind(Box::new(PngAbort));
}

fn png(bytes: &[u8], gray: bool, site_packages: &str) -> Png {
    if let Err(e) = png_prescan(bytes) {
        return e;
    }
    let Some(lib) = libpng(site_packages) else { return Png::Delegate };
    let _ = LIB_FOR_CALLBACK.set(Some(lib));
    let mut src = Source { data: bytes, pos: 0 };
    unsafe {
        let mut png_ptr = (lib.create_read)(c"1.6.58".as_ptr(), std::ptr::null_mut(), None, None);
        if png_ptr.is_null() {
            return Png::Failed;
        }
        let mut info = (lib.create_info)(png_ptr);
        if info.is_null() {
            (lib.destroy_read)(&mut png_ptr, std::ptr::null_mut(), std::ptr::null_mut());
            return Png::Failed;
        }
        (lib.set_longjmp_fn)(png_ptr, png_abort, 200);
        (lib.set_read_fn)(png_ptr, &mut src as *mut Source as *mut c_void, read_source);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Png {
            (lib.read_info)(png_ptr, info);
            let (mut w, mut h, mut depth, mut colour) = (0u32, 0u32, 0, 0);
            (lib.get_ihdr)(png_ptr, info, &mut w, &mut h, &mut depth, &mut colour, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
            let (w, h) = (w as usize, h as usize);
            if !sane(w, h) {
                return Png::Delegate;
            }
            if depth == 16 {
                (lib.set_strip_16)(png_ptr);
            } else {
                (lib.set_swap)(png_ptr);
            }
            (lib.set_strip_alpha)(png_ptr);
            if colour == 3 {
                (lib.set_palette_to_rgb)(png_ptr);
            }
            if colour & 2 == 0 && depth < 8 {
                (lib.set_expand_gray_1_2_4_to_8)(png_ptr);
            }
            if colour & 2 != 0 && !gray {
                (lib.set_bgr)(png_ptr);
            } else if !gray {
                (lib.set_gray_to_rgb)(png_ptr);
            } else {
                (lib.set_rgb_to_gray)(png_ptr, 1, 0.299, 0.587);
            }
            (lib.set_interlace_handling)(png_ptr);
            (lib.read_update_info)(png_ptr, info);
            let cn = if gray { 1 } else { 3 };
            if (lib.get_rowbytes)(png_ptr, info) != w * cn {
                return Png::Delegate;
            }
            let mut data = vec![0u8; w * h * cn];
            let mut rows: Vec<*mut u8> = data.chunks_exact_mut(w * cn).map(|r| r.as_mut_ptr()).collect();
            (lib.read_image)(png_ptr, rows.as_mut_ptr());
            (lib.read_end)(png_ptr, info);
            let mut orientation = None;
            const PNG_INFO_EXIF: u32 = 0x10000;
            if (lib.get_valid)(png_ptr, info, PNG_INFO_EXIF) != 0 {
                let (mut n, mut p) = (0u32, std::ptr::null_mut());
                (lib.get_exif_1)(png_ptr, info, &mut n, &mut p);
                if !p.is_null() && n > 0 {
                    let mut exif = Exif::default();
                    exif.parse(std::slice::from_raw_parts(p, n as usize));
                    orientation = exif.orientation;
                }
            }
            Png::Ok(Mat { w, h, cn, data }, orientation)
        }));
        (lib.destroy_read)(&mut png_ptr, &mut info, std::ptr::null_mut());
        match result {
            Ok(r) => r,
            Err(e) if e.is::<PngAbort>() => Png::Failed,
            Err(e) => std::panic::resume_unwind(e),
        }
    }
}
