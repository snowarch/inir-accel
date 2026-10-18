//! `generate_colors_material.py` is not ported yet. Until a port matches it byte for byte (test/parity.py), the
//! venv shim hands every call to the real Python, so this machine wears the original Material colours.

/// The exit code that tells the shim to run the original script instead.
pub const DELEGATE: i32 = 125;

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
