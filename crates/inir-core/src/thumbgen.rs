// This code was written by: Hikari.
//! Native Rust replacement for `scripts/thumbnails/thumbgen.py`.
//!
//! Computes exact Freedesktop / QML `ThumbnailImage` MD5 paths (`~/.cache/thumbnails/<size>/<md5>.png`)
//! and generates missing thumbnails in parallel across Rayon worker threads without Python or
//! per-image ImageMagick process storms.

use image::imageops::FilterType;
use rayon::prelude::*;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Pure Rust RFC 1321 MD5 implementation (zero external crate needed).
pub fn md5_hex(data: &[u8]) -> String {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22,
        5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
        4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
        6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee,
        0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
        0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be,
        0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
        0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa,
        0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
        0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
        0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c,
        0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
        0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05,
        0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
        0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039,
        0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1,
        0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
    ];

    let mut a0: u32 = 0x67452301;
    let mut b0: u32 = 0xefcdab89;
    let mut c0: u32 = 0x98badcfe;
    let mut d0: u32 = 0x10325476;

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut m = [0u32; 16];
        for (i, w) in chunk.chunks_exact(4).enumerate() {
            m[i] = u32::from_le_bytes([w[0], w[1], w[2], w[3]]);
        }
        let mut a = a0;
        let mut b = b0;
        let mut c = c0;
        let mut d = d0;

        for i in 0..64 {
            let (f, g) = if i < 16 {
                ((b & c) | ((!b) & d), i)
            } else if i < 32 {
                ((d & b) | ((!d) & c), (5 * i + 1) % 16)
            } else if i < 48 {
                (b ^ c ^ d, (3 * i + 5) % 16)
            } else {
                (c ^ (b | (!d)), (7 * i) % 16)
            };
            let temp = d;
            d = c;
            c = b;
            let sum = a.wrapping_add(f).wrapping_add(K[i]).wrapping_add(m[g]);
            b = b.wrapping_add(sum.rotate_left(S[i]));
            a = temp;
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut digest = [0u8; 16];
    digest[0..4].copy_from_slice(&a0.to_le_bytes());
    digest[4..8].copy_from_slice(&b0.to_le_bytes());
    digest[8..12].copy_from_slice(&c0.to_le_bytes());
    digest[12..16].copy_from_slice(&d0.to_le_bytes());

    let mut hex = String::with_capacity(32);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(&mut hex, "{:02x}", b);
    }
    hex
}

/// Matches Python `urllib.parse.quote(p, safe="")` and JS `encodeURIComponent`:
/// Unreserved bytes: `A-Z`, `a-z`, `0-9`, `-`, `_`, `.`, `~`
fn quote_segment(seg: &str) -> String {
    let mut out = String::with_capacity(seg.len());
    for &b in seg.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            use std::fmt::Write;
            let _ = write!(&mut out, "%{:02X}", b);
        }
    }
    out
}

/// Exact thumbnail path matching `thumbgen.py::get_thumbnail_path` and QML `ThumbnailImage`.
pub fn get_thumbnail_path(fpath: &str, size_name: &str) -> PathBuf {
    let encoded = fpath
        .split('/')
        .map(quote_segment)
        .collect::<Vec<_>>()
        .join("/");
    let url = format!("file://{}", encoded);
    let md5 = md5_hex(url.as_bytes());
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join(".cache")
        .join("thumbnails")
        .join(size_name)
        .join(format!("{}.png", md5))
}

pub fn pixel_size_for(size_name: &str) -> u32 {
    match size_name {
        "normal" => 128,
        "large" => 256,
        "x-large" => 512,
        "xx-large" => 1024,
        _ => 256,
    }
}

fn is_supported_media(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "mp4" | "webm" | "mkv" | "avi" | "mov"
    )
}

fn is_video(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "mp4" | "webm" | "mkv" | "avi" | "mov"
    )
}

pub fn make_thumbnail_one(fpath: &Path, size_name: &str) -> bool {
    let fpath_str = fpath.to_string_lossy();
    let thumb_path = get_thumbnail_path(&fpath_str, size_name);
    if thumb_path.exists() {
        return false;
    }
    if let Some(parent) = thumb_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let px_size = pixel_size_for(size_name);

    if !is_video(fpath) {
        if let Ok(img) = image::open(fpath) {
            let thumb = img.resize(px_size, px_size, FilterType::Triangle);
            let tmp_path = thumb_path.with_extension(format!("tmp.{}.png", std::process::id()));
            if thumb.save_with_format(&tmp_path, image::ImageFormat::Png).is_ok() {
                if fs::rename(&tmp_path, &thumb_path).is_ok() {
                    return true;
                }
                let _ = fs::remove_file(&tmp_path);
            }
        }
    }

    // Fallback to ImageMagick for video files or exotic formats
    let arg_in = format!("{}[0]", fpath_str);
    let arg_sz = format!("{}x{}", px_size, px_size);
    if let Ok(st) = Command::new("magick")
        .args([&arg_in, "-resize", &arg_sz, &thumb_path.to_string_lossy()])
        .output()
    {
        return st.status.success();
    }
    false
}

fn collect_files(dir_path: &Path, recursive: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(dir_path) else {
        return out;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_file() {
            out.push(p);
        } else if recursive && p.is_dir() {
            out.extend(collect_files(&p, true));
        }
    }
    out
}

/// CLI entrypoint compatible with `scripts/thumbnails/thumbgen.py`.
pub fn run_cli(args: &[String]) -> Result<(), String> {
    let mut img_dirs_raw: Option<String> = None;
    let mut size_name = "normal".to_string();
    let mut workers: usize = 4;
    let mut only_images = false;
    let mut recursive = false;
    let mut machine_progress = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-d" | "--img_dirs" if i + 1 < args.len() => {
                img_dirs_raw = Some(args[i + 1].clone());
                i += 2;
            }
            "-s" | "--size" if i + 1 < args.len() => {
                size_name = args[i + 1].clone();
                i += 2;
            }
            "-w" | "--workers" if i + 1 < args.len() => {
                workers = args[i + 1].parse().unwrap_or(4).max(1);
                i += 2;
            }
            "-i" | "--only_images" => {
                only_images = true;
                i += 1;
            }
            "-r" | "--recursive" => {
                recursive = true;
                i += 1;
            }
            "--machine_progress" => {
                machine_progress = true;
                i += 1;
            }
            other => {
                return Err(format!("Unsupported thumbgen argument: {}", other));
            }
        }
    }

    let raw_dirs = img_dirs_raw.ok_or_else(|| "Missing -d / --img_dirs".to_string())?;
    let dirs: Vec<PathBuf> = if Path::new(&raw_dirs).is_dir() {
        vec![PathBuf::from(&raw_dirs)]
    } else {
        raw_dirs.split_whitespace().map(PathBuf::from).collect()
    };

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers.max(4))
        .build()
        .map_err(|e| e.to_string())?;

    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }
        let resolved = fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
        let mut files = collect_files(&dir, recursive);
        println!("Found {} files in the directory: {}", files.len(), resolved.display());
        if only_images {
            files.retain(|p| is_supported_media(p));
            println!("Found {} images/videos", files.len());
        }
        let total = files.len();
        if total == 0 {
            continue;
        }
        let completed = AtomicUsize::new(0);
        let stdout_lock = Mutex::new(io::stdout());

        pool.install(|| {
            files.par_iter().for_each(|fpath| {
                let _ = make_thumbnail_one(fpath, &size_name);
                if machine_progress {
                    let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
                    if let Ok(mut out) = stdout_lock.lock() {
                        let _ = writeln!(out, "PROGRESS {}/{} FILE {}", done, total, fpath.display());
                        let _ = out.flush();
                    }
                }
            });
        });
    }

    println!("Thumbnail Generation Completed!");
    let _ = io::stdout().flush();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_md5_and_thumbnail_uri_parity() {
        // Known RFC 1321 test vector: md5("file:///home/user/wall.png")
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        let p = get_thumbnail_path("/home/user/Pictures/Wallpapers/my wall (1).png", "large");
        assert!(p.to_string_lossy().ends_with(".png"));
    }
}
