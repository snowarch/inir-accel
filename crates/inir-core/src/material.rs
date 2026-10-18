//! `generate_colors_material.py` is not ported yet. Until a port matches it byte for byte (test/parity.py), the
//! venv shim hands every call to the real Python, so this machine wears the original Material colours.

mod hct;
mod planes;
mod py;
mod quantize;

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
        _ => 2,
    }
}

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
    Err("Material is not ported yet: run scripts/colors/generate_colors_material.py".into())
}

pub fn run_cli(_args: &[String]) -> i32 {
    DELEGATE
}
