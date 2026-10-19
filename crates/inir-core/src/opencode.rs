//! `scripts/colors/opencode_themegen/main.go`, line for line: `inir-opencode-themegen [scss [palette [terminal [outdir]]]]`.
//! This code was written by: Hikari.
use crate::gotheme::{blend, marshal_indent, merge, parse_scss, pick, read_string_map_json, Colors, J};
use crate::jmap;
use std::fs;
use std::path::Path;

pub fn run_cli(args: &[String]) -> i32 {
    let home = std::env::var("HOME").unwrap_or_default();
    let generated = format!("{home}/.local/state/quickshell/user/generated");
    let scss_path = args.first().cloned().unwrap_or(format!("{generated}/material_colors.scss"));
    let palette_path = args.get(1).cloned().unwrap_or(format!("{generated}/palette.json"));
    let terminal_path = args.get(2).cloned().unwrap_or(format!("{generated}/terminal.json"));
    let output_dir = args.get(3).cloned().unwrap_or(format!("{home}/.config/opencode/themes"));

    let Ok(scss) = parse_scss(&scss_path) else {
        eprintln!("Error: {scss_path} not found");
        return 1;
    };
    let palette = read_string_map_json(&palette_path).unwrap_or_default();
    let terminal = read_string_map_json(&terminal_path).unwrap_or_default();
    let colors = merge(&scss, &[&palette, &terminal]);

    let theme = generate(&colors);
    if let Err(e) = fs::create_dir_all(&output_dir) {
        eprintln!("mkdir {output_dir}: {}", go_err(&e));
        return 1;
    }
    let output_path = Path::new(&output_dir).join("inir.json");
    let mut data = marshal_indent(&theme, "", "  ");
    data.push('\n');
    if let Err(e) = fs::write(&output_path, data) {
        eprintln!("open {}: {}", output_path.display(), go_err(&e));
        return 1;
    }
    println!("[opencode] Theme generated: {}", output_path.display());
    0
}

fn go_err(e: &std::io::Error) -> String {
    let s = e.to_string();
    // Go prints the bare errno text ("permission denied"); Rust appends " (os error N)" and capitalises.
    let s = s.split(" (os error").next().unwrap_or(&s).to_string();
    let mut c = s.chars();
    c.next().map(|f| f.to_lowercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

fn generate(colors: &Colors) -> J {
    let p = |k: &str, f: &str| pick(colors, k, f);
    let p2 = |a: &str, b: &str, f: &str| pick(colors, a, &pick(colors, b, f));
    let primary = p("primary", "#7aa2f7");
    let on_primary = p2("onPrimary", "on_primary", "#1a1b26");
    let primary_container = p2("primaryContainer", "primary_container", "#004c6b");
    let on_primary_container = p2("onPrimaryContainer", "on_primary_container", "#c6e7ff");
    let secondary = p("secondary", "#bb9af7");
    let secondary_container = p2("secondaryContainer", "secondary_container", "#394b58");
    let tertiary = p("tertiary", "#9ece6a");
    let tertiary_container = p2("tertiaryContainer", "tertiary_container", "#958bb1");
    let mut surface = p("surface", "#0f1417");
    let surface_dim = p2("surfaceDim", "surface_dim", "#0f1417");
    let surface_bright = p2("surfaceBright", "surface_bright", "#353a3d");
    let mut surface_container = p2("surfaceContainer", "surface_container", "#1c2024");
    let surface_container_low = p2("surfaceContainerLow", "surface_container_low", "#181c1f");
    let mut surface_container_high = p2("surfaceContainerHigh", "surface_container_high", "#262b2e");
    let surface_container_highest = p2("surfaceContainerHighest", "surface_container_highest", "#313539");
    let mut on_surface = p2("onSurface", "on_surface", "#dfe3e7");
    let mut on_surface_variant = p2("onSurfaceVariant", "on_surface_variant", "#c1c7ce");
    let outline = p("outline", "#8b9298");
    let outline_variant = p2("outlineVariant", "outline_variant", "#41484d");
    let shadow = p("shadow", "#000000");
    let error_col = p("error", "#ffb4ab");
    let error_container = p2("errorContainer", "error_container", "#93000a");
    let success = p("success", "#b5ccba");
    let success_container = p2("successContainer", "success_container", "#374b3e");
    let inverse_surface = p2("inverseSurface", "inverse_surface", "#dfe3e7");
    let inverse_on_surface = p2("inverseOnSurface", "inverse_on_surface", "#2c3135");
    let inverse_primary = p2("inversePrimary", "inverse_primary", &primary);

    let term: Vec<String> = (0..16).map(|i| p(&format!("term{i}"), "#888888")).collect();
    if colors.contains_key("term15") {
        on_surface = term[15].clone();
        on_surface_variant = term[7].clone();
        surface = term[0].clone();
        surface_container = blend(&term[0], &term[15], 0.05);
        surface_container_high = blend(&term[0], &term[15], 0.10);
    }

    let diff_added_bg = blend(&surface_container, &success, 0.15);
    let diff_removed_bg = blend(&surface_container, &error_col, 0.15);
    let diff_added_line_bg = blend(&surface_container, &success, 0.10);
    let diff_removed_line_bg = blend(&surface_container, &error_col, 0.10);
    let warning = p("term3", &term[3]);

    let light_primary = inverse_primary.clone();
    let light_secondary = blend(&secondary, &inverse_surface, 0.25);
    let light_accent = blend(&tertiary, &inverse_surface, 0.25);
    let light_error = blend(&error_col, &inverse_surface, 0.35);
    let light_warning = blend(&warning, &inverse_surface, 0.40);
    let light_success = blend(&success, &inverse_surface, 0.35);
    let light_text_muted = blend(&inverse_on_surface, &inverse_surface, 0.35);
    let light_background_panel = blend(&inverse_surface, "#ffffff", 0.06);
    let light_background_element = blend(&inverse_surface, "#ffffff", 0.12);
    let light_background_menu = blend(&inverse_surface, "#ffffff", 0.08);
    let light_border = blend(&inverse_on_surface, &inverse_surface, 0.55);
    let light_border_subtle = blend(&inverse_on_surface, &inverse_surface, 0.70);
    let light_diff_added_bg = blend(&inverse_surface, &light_success, 0.18);
    let light_diff_removed_bg = blend(&inverse_surface, &light_error, 0.18);
    let light_diff_added_line_bg = blend(&inverse_surface, &light_success, 0.12);
    let light_diff_removed_line_bg = blend(&inverse_surface, &light_error, 0.12);
    let light_markdown_link = blend(&tertiary, &inverse_surface, 0.20);
    let light_markdown_code = blend(&term[2], &inverse_surface, 0.20);
    let light_syntax_keyword = blend(&term[5], &inverse_surface, 0.25);
    let light_syntax_function = blend(&term[4], &inverse_surface, 0.25);
    let light_syntax_variable = blend(&term[6], &inverse_surface, 0.20);
    let light_syntax_string = blend(&term[2], &inverse_surface, 0.25);
    let light_syntax_number = blend(&term[13], &inverse_surface, 0.25);
    let light_syntax_type = blend(&term[3], &inverse_surface, 0.25);
    let light_selected_list_item_text = inverse_surface.clone();

    let tv = |dark: &str, light: &str| jmap! { "dark" => dark, "light" => light };

    let defs = jmap! {
        "m3Primary" => &primary, "m3OnPrimary" => &on_primary, "m3PrimaryContainer" => &primary_container,
        "m3OnPrimaryContainer" => &on_primary_container, "m3Secondary" => &secondary,
        "m3SecondaryContainer" => &secondary_container, "m3Tertiary" => &tertiary,
        "m3TertiaryContainer" => &tertiary_container, "m3Surface" => &surface, "m3SurfaceDim" => &surface_dim,
        "m3SurfaceBright" => &surface_bright, "m3SurfaceContainer" => &surface_container,
        "m3SurfaceContainerLow" => &surface_container_low, "m3SurfaceContainerHigh" => &surface_container_high,
        "m3SurfaceContainerHighest" => &surface_container_highest, "m3OnSurface" => &on_surface,
        "m3OnSurfaceVariant" => &on_surface_variant, "m3Outline" => &outline, "m3OutlineVariant" => &outline_variant,
        "m3Error" => &error_col, "m3ErrorContainer" => &error_container, "m3Success" => &success,
        "m3SuccessContainer" => &success_container, "m3Shadow" => &shadow, "m3InverseSurface" => &inverse_surface,
        "m3InverseOnSurface" => &inverse_on_surface, "m3InversePrimary" => &inverse_primary,
        "ansiRed" => &term[1], "ansiGreen" => &term[2], "ansiYellow" => &term[3], "ansiBlue" => &term[4],
        "ansiMagenta" => &term[5], "ansiCyan" => &term[6], "ansiBrightRed" => &term[9],
        "ansiBrightGreen" => &term[10], "ansiBrightYellow" => &term[11], "ansiBrightBlue" => &term[12],
        "ansiBrightMagenta" => &term[13], "ansiBrightCyan" => &term[14],
    };

    let mut theme = jmap! {
        "primary" => tv("m3Primary", "m3InversePrimary"),
        "secondary" => tv("m3Secondary", &light_secondary),
        "accent" => tv("m3Tertiary", &light_accent),
        "error" => tv("m3Error", &light_error),
        "warning" => tv(&warning, &light_warning),
        "success" => tv("m3Success", &light_success),
        "info" => tv("m3Primary", &light_primary),
        "text" => tv("m3OnSurface", "m3InverseOnSurface"),
        "textMuted" => tv("m3OnSurfaceVariant", &light_text_muted),
        "selectedListItemText" => tv("m3Surface", &light_selected_list_item_text),
        "background" => tv("m3Surface", "m3InverseSurface"),
        "backgroundPanel" => tv("m3SurfaceContainer", &light_background_panel),
        "backgroundElement" => tv("m3SurfaceContainerHigh", &light_background_element),
        "backgroundMenu" => tv("m3SurfaceContainerHigh", &light_background_menu),
        "border" => tv("m3OutlineVariant", &light_border),
        "borderActive" => tv("m3Primary", "m3InversePrimary"),
        "borderSubtle" => tv(&blend(&outline_variant, &surface, 0.3), &light_border_subtle),
        "diffAdded" => tv("m3Success", &light_success),
        "diffRemoved" => tv("m3Error", &light_error),
        "diffContext" => tv("m3OnSurfaceVariant", &light_text_muted),
        "diffHunkHeader" => tv("m3Outline", &light_border),
        "diffHighlightAdded" => tv("ansiBrightGreen", "ansiGreen"),
        "diffHighlightRemoved" => tv("ansiBrightRed", "ansiRed"),
        "diffAddedBg" => tv(&diff_added_bg, &light_diff_added_bg),
        "diffRemovedBg" => tv(&diff_removed_bg, &light_diff_removed_bg),
        "diffContextBg" => tv("m3SurfaceContainer", &light_background_panel),
        "diffLineNumber" => tv("m3Outline", &light_border),
        "diffAddedLineNumberBg" => tv(&diff_added_line_bg, &light_diff_added_line_bg),
        "diffRemovedLineNumberBg" => tv(&diff_removed_line_bg, &light_diff_removed_line_bg),
        "markdownText" => tv("m3OnSurface", "m3InverseOnSurface"),
        "markdownHeading" => tv("m3Primary", "m3InversePrimary"),
        "markdownLink" => tv("m3Tertiary", &light_markdown_link),
        "markdownLinkText" => tv("ansiCyan", "ansiBlue"),
        "markdownCode" => tv("ansiGreen", &light_markdown_code),
        "markdownBlockQuote" => tv("m3OnSurfaceVariant", &light_text_muted),
        "markdownEmph" => tv("ansiYellow", &light_warning),
        "markdownStrong" => tv("ansiBrightYellow", &warning),
        "markdownHorizontalRule" => tv("m3OutlineVariant", &light_border),
        "markdownListItem" => tv("m3Primary", "m3InversePrimary"),
        "markdownListEnumeration" => tv("m3Secondary", &light_secondary),
        "markdownImage" => tv("ansiMagenta", &light_accent),
        "markdownImageText" => tv("ansiBrightMagenta", "ansiMagenta"),
        "markdownCodeBlock" => tv("m3OnSurface", "m3InverseOnSurface"),
        "syntaxComment" => tv("m3Outline", &light_text_muted),
        "syntaxKeyword" => tv("ansiMagenta", &light_syntax_keyword),
        "syntaxFunction" => tv("ansiBlue", &light_syntax_function),
        "syntaxVariable" => tv("ansiCyan", &light_syntax_variable),
        "syntaxString" => tv("ansiGreen", &light_syntax_string),
        "syntaxNumber" => tv("ansiBrightMagenta", &light_syntax_number),
        "syntaxType" => tv("ansiYellow", &light_syntax_type),
        "syntaxOperator" => tv("m3OnSurfaceVariant", &light_text_muted),
        "syntaxPunctuation" => tv("m3OnSurfaceVariant", &light_text_muted),
    };
    if let J::Map(m) = &mut theme {
        m.insert("thinkingOpacity".into(), J::Num(0.6));
    }

    let mut root = jmap! { "$schema" => "https://opencode.ai/theme.json" };
    if let J::Map(m) = &mut root {
        m.insert("defs".into(), defs);
        m.insert("theme".into(), theme);
    }
    root
}
