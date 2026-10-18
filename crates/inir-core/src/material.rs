//! `generate_colors_material.py`, ported byte for byte (test/parity.py material, test/material_stages.py,
//! test/material_pixels.py). The shim answers only for the exact script and library versions it was transcribed
//! from; anything else, and the few inputs the port leaves alone, run the original.

mod hct;
mod pixels;
mod planes;
mod py;
mod quantize;
mod scheme;
mod script;

/// The exit code that tells the shim to run the original script instead.
pub const DELEGATE: i32 = 125;

/// Stage probe for test/parity.py while the port grows: `celebi <rgb file>` reads raw r,g,b bytes (what
/// `list(image.getdata())` hands the quantizer) and prints `argb:count` pairs and the seed `Score` picks.
pub fn probe(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("celebi") => {
            let Some(bytes) = args.get(1).and_then(|p| std::fs::read(p).ok()) else { return 2 };
            let px: Vec<[u8; 3]> = bytes.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
            let map = quantize::quantize_celebi(&px, 128);
            let pairs: Vec<String> = map.iter().map(|(a, c)| format!("{a}:{c}")).collect();
            println!("{}", pairs.join(" "));
            println!("{}", quantize::score_first(&map));
            0
        }
        Some("hct") => {
            // hct <hue> <chroma> <tone>...: the argb Hct.from_hct solves and the HCT read back from it.
            let v: Vec<f64> = args[1..].iter().filter_map(|s| s.parse().ok()).collect();
            for t in v.chunks_exact(3) {
                let h = hct::Hct::from_hct(t[0], t[1], t[2]);
                println!("{} {} {} {}", h.argb, py::float_repr(h.hue), py::float_repr(h.chroma), py::float_repr(h.tone));
            }
            0
        }
        Some("pixels") => {
            // pixels <image> <size>: what the quantizer gets (size, channels, FNV-1a of the bytes), then the
            // auto-scheme statistics and decision.
            let (Some(path), Some(size)) = (args.get(1), args.get(2).and_then(|s| s.parse::<i64>().ok())) else { return 2 };
            let im = match pixels::open(path) {
                pixels::Load::Ok(im) => im,
                pixels::Load::Io(_) => return 3,
                pixels::Load::Delegate => return DELEGATE,
            };
            let (nw, nh) = pixels::optimal_size(im.width, im.height, size);
            let im = if nw < im.width || nh < im.height { pixels::resize(&im, nw, nh) } else { im };
            let mut hash: u64 = 0xcbf29ce484222325;
            for &b in &im.data {
                hash = (hash ^ b as u64).wrapping_mul(0x100000001b3);
            }
            let (c, s, h) = pixels::scheme_stats(&im);
            println!("{} {} {} {}", im.width, im.height, im.channels, hash);
            println!("{} {} {} {}", py::float_repr(c), py::float_repr(s), py::float_repr(h), pixels::auto_scheme(&im));
            0
        }
        Some("scheme") => {
            // scheme <scheme name> <0|1 dark> <argb>...: per seed, the 59 colours in script order, then the six
            // palettes at the tones the script reads back.
            let (Some(name), Some(dark)) = (args.get(1), args.get(2)) else { return 2 };
            let variant = scheme::Variant::from_scheme_name(name);
            for seed in args[3..].iter().filter_map(|a| a.parse::<u32>().ok()) {
                let s = scheme::Scheme::new(hct::Hct::from_int(seed), variant, dark == "1", 0.0);
                let colours: Vec<String> = scheme::ROLES.iter().map(|&r| s.argb(r).to_string()).collect();
                let mut tones = Vec::new();
                for p in [&s.primary, &s.secondary, &s.tertiary, &s.neutral, &s.neutral_variant, &s.error] {
                    for t in PROBE_TONES {
                        tones.push(p.get_hct(t).argb.to_string());
                    }
                }
                println!("{} | {}", colours.join(" "), tones.join(" "));
            }
            0
        }
        _ => 2,
    }
}

const PROBE_TONES: [f64; 16] = [0.0, 4.0, 10.0, 20.0, 25.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 95.0, 98.0, 99.0, 100.0];

/// The script the port was transcribed from: any other text is the original's to run.
const SCRIPT_FNV: u64 = 0x4affdddb02445603;

/// Versions the transcription matches (Pillow's resampler and warnings, numpy's sums, materialyoucolor's maths).
const LIBRARIES: [&str; 3] = ["materialyoucolor-3.0.4.dist-info", "pillow-12.3.0.dist-info", "numpy-2.5.4.dist-info"];

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    h
}

/// The venv the shim stands in, as Python would find its prefix: the directory above the `bin/` it was called from
/// when that is a venv (how switchwall.sh calls it), else `INIR_VENV` (the session may hand it with `$HOME`
/// unexpanded, as the script's own shebang `eval`s it), else the shell's default.
fn venv_dir() -> std::path::PathBuf {
    if let Some(dir) = std::env::args_os().next().map(std::path::PathBuf::from).and_then(|p| p.parent()?.parent().map(|p| p.to_path_buf())) {
        if dir.join("pyvenv.cfg").is_file() {
            return dir;
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    if let Ok(v) = std::env::var("INIR_VENV") {
        if !v.is_empty() {
            return v.replace("${HOME}", &home).replace("$HOME", &home).into();
        }
    }
    std::path::PathBuf::from(format!("{home}/.local/state/quickshell/.venv"))
}

/// `site-packages` of the venv when it carries the exact libraries, as the original imports them.
fn site_packages() -> Option<String> {
    let lib = venv_dir().join("lib");
    let site = std::fs::read_dir(&lib)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path().join("site-packages"))
        .find(|p| p.join("materialyoucolor").is_dir())?;
    LIBRARIES.iter().all(|d| site.join(d).is_dir()).then(|| site.to_string_lossy().into_owned())
}

/// The venv shim's entry: `script` is the path Python was asked to run.
pub fn run_script(script: &str, args: &[String]) -> i32 {
    match std::fs::read(script) {
        Ok(text) if fnv1a(&text) == SCRIPT_FNV => {}
        _ => return DELEGATE,
    }
    run_cli(args)
}

/// `inir-accel material-colors ...`, the script's arguments without the script check.
pub fn run_cli(args: &[String]) -> i32 {
    let Some(site_packages) = site_packages() else { return DELEGATE };
    match script::run(args, &script::Context { site_packages }) {
        script::Run::Exit(code) => code,
        script::Run::Delegate => DELEGATE,
    }
}

/// The QML plugin's in-process entry (`InirNative.generateMaterialColors`), which nothing in the shell calls: the
/// script's stdout and effects belong to a process, so it stays an error rather than a second, diverging pipeline.
#[derive(Clone, Debug)]
pub struct MaterialGenConfig {
    pub path: Option<String>,
    pub size: u32,
    pub color: Option<String>,
    pub mode: String,
    pub scheme: String,
    pub render_templates: Option<String>,
}

impl Default for MaterialGenConfig {
    fn default() -> Self {
        Self { path: None, size: 128, color: None, mode: "dark".into(), scheme: "vibrant".into(), render_templates: None }
    }
}

pub fn run_material_pipeline(_cfg: MaterialGenConfig) -> Result<String, String> {
    Err("run scripts/colors/generate_colors_material.py (inir-accel answers it through the venv shim)".into())
}
