//! `scripts/images/least_busy_region.py`, ported byte for byte against opencv-contrib-python 5.0.0 and numpy 2.5.4
//! (test/parity.py region, test/region_stages.py) and the code since inir dropped Python. Like the Material port, the
//! shim answers only for the exact script it was transcribed from.

mod decode;
mod kmeans;
mod resize;
mod script;

use serde::{Deserialize, Serialize};
use std::path::Path;

/// The exit code that tells the shim to run the original script instead (a script it wasn't transcribed from).
pub const DELEGATE: i32 = 125;

/// An 8-bit image as cv2 hands it to numpy: rows of `w * cn` bytes, BGR or grey.
pub struct Mat {
    pub w: usize,
    pub h: usize,
    pub cn: usize,
    pub data: Vec<u8>,
}

/// The script the port was transcribed from: any other text is the original's to run.
const SCRIPT_FNV: u64 = 0x31b77c448c00ce51;

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    h
}

/// `site-packages` of a venv with cv2, whose libpng the decoder takes while it is there (the system's otherwise).
fn site_packages() -> Option<String> {
    let lib = crate::material::venv_dir().join("lib");
    let site = std::fs::read_dir(&lib).ok()?.filter_map(|e| e.ok()).map(|e| e.path().join("site-packages")).find(|p| p.join("cv2").is_dir())?;
    Some(site.to_string_lossy().into_owned())
}

/// The venv shim's entry: `script` is the path Python was asked to run.
pub fn run_script(script: &str, args: &[String]) -> i32 {
    match std::fs::read(script) {
        Ok(text) if fnv1a(&text) == SCRIPT_FNV => {}
        _ => return DELEGATE,
    }
    run_cli(args)
}

/// `inir-accel region ...`, the script's arguments without the script check.
pub fn run_cli(args: &[String]) -> i32 {
    script::run(args, &script::Context { site_packages: site_packages() })
}

/// Stage probe for test/region_stages.py: `imread <path> <0|1 gray>` and `screen <path> <0|1> <w> <h> <mode>` print
/// the size and the raw bytes' FNV-1a; `luma <path> <cell> <w> <h> <mode>` prints the grid's JSON.
pub fn probe(args: &[String]) -> i32 {
    let site = site_packages();
    let report = |m: &Mat| println!("{} {} {} {}", m.w, m.h, m.cn, script::sha1_hex(&m.data));
    let num = |i: usize| args.get(i).and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
    match args.first().map(String::as_str) {
        Some("imread") => match decode::imread(&args[1], args.get(2).is_some_and(|g| g == "1"), site.as_deref()) {
            decode::Read::Ok(m) => {
                report(&m);
                0
            }
            decode::Read::None => {
                println!("none");
                0
            }
        },
        Some("screen") => match decode::imread(&args[1], args.get(2).is_some_and(|g| g == "1"), site.as_deref()) {
            decode::Read::Ok(m) => {
                report(&script::screen_image(&m, num(3), num(4), &args[5]));
                0
            }
            decode::Read::None => {
                println!("none");
                0
            }
        },
        _ => 2,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LumaGridOutput {
    pub cols: usize,
    pub rows: usize,
    pub cell: usize,
    pub width: usize,
    pub height: usize,
    pub mean: Vec<i64>,
    pub sq: Vec<i64>,
    pub r: Vec<i64>,
    pub g: Vec<i64>,
    pub b: Vec<i64>,
}

/// The `--luma-grid` reading without the script's cache, for the QML plugin and the bench.
pub fn compute_luma_grid(image_path: &Path, cell: usize, screen_width: u32, screen_height: u32, screen_mode: &str) -> Result<LumaGridOutput, String> {
    let site = site_packages();
    let path = image_path.to_str().ok_or("path is not UTF-8")?;
    let img = script::screen_image_of(path, screen_width.max(1) as usize, screen_height.max(1) as usize, screen_mode, site.as_deref()).ok_or("image not readable")?;
    let g = script::luma_grid(&img, cell.max(1));
    Ok(LumaGridOutput { cols: g.cols, rows: g.rows, cell: g.cell, width: g.width, height: g.height, mean: g.mean, sq: g.sq, r: g.r, g: g.g, b: g.b })
}
