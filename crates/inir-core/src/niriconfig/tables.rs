//! Tables copied from niri-config.py by a generator, so no entry is mistyped.

use crate::material::py::Json;

pub(super) const DEFAULT_NIRI_FILES: &[&str] = &["config.kdl", "config.d/10-input-and-cursor.kdl", "config.d/20-layout-and-overview.kdl", "config.d/30-window-rules.kdl", "config.d/40-environment.kdl", "config.d/50-startup.kdl", "config.d/60-animations.kdl", "config.d/70-binds.kdl", "config.d/80-layer-rules.kdl", "config.d/90-user-extra.kdl"];
pub(super) const ANIMATION_TYPES: &[&str] = &["workspace-switch", "window-open", "window-close", "horizontal-view-movement", "window-movement", "window-resize", "config-notification-open-close", "exit-confirmation-open-close", "screenshot-ui-open", "overview-open-close", "recent-windows-close"];

/// `dict(ANIMATION_DEFAULTS[t])`.
pub(super) fn animation_default(t: &str) -> Json {
    let pairs: &[(&str, Json)] = &match t {
        "workspace-switch" => [("mode", Json::Str("spring".into())), ("damping_ratio", Json::Float(0.98)), ("stiffness", Json::Int(300)), ("epsilon", Json::Float(0.0001))],
        "window-open" => [("mode", Json::Str("spring".into())), ("damping_ratio", Json::Float(0.98)), ("stiffness", Json::Int(300)), ("epsilon", Json::Float(0.0001))],
        "window-close" => [("mode", Json::Str("spring".into())), ("damping_ratio", Json::Float(0.18)), ("stiffness", Json::Int(300)), ("epsilon", Json::Float(0.0001))],
        "horizontal-view-movement" => [("mode", Json::Str("spring".into())), ("damping_ratio", Json::Float(0.98)), ("stiffness", Json::Int(300)), ("epsilon", Json::Float(0.0001))],
        "window-movement" => [("mode", Json::Str("spring".into())), ("damping_ratio", Json::Float(0.98)), ("stiffness", Json::Int(900)), ("epsilon", Json::Float(0.0001))],
        "window-resize" => [("mode", Json::Str("spring".into())), ("damping_ratio", Json::Float(0.98)), ("stiffness", Json::Int(300)), ("epsilon", Json::Float(0.0001))],
        "config-notification-open-close" => [("mode", Json::Str("spring".into())), ("damping_ratio", Json::Float(0.98)), ("stiffness", Json::Int(300)), ("epsilon", Json::Float(0.0001))],
        "exit-confirmation-open-close" => [("mode", Json::Str("spring".into())), ("damping_ratio", Json::Float(0.6)), ("stiffness", Json::Int(500)), ("epsilon", Json::Float(0.01))],
        "screenshot-ui-open" => [("mode", Json::Str("spring".into())), ("damping_ratio", Json::Float(0.98)), ("stiffness", Json::Int(300)), ("epsilon", Json::Float(0.0001))],
        "overview-open-close" => [("mode", Json::Str("spring".into())), ("damping_ratio", Json::Float(1.0)), ("stiffness", Json::Int(800)), ("epsilon", Json::Float(0.0001))],
        "recent-windows-close" => [("mode", Json::Str("spring".into())), ("damping_ratio", Json::Float(1.0)), ("stiffness", Json::Int(800)), ("epsilon", Json::Float(0.001))],
        _ => return Json::Obj(Vec::new()),
    };
    Json::Obj(pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect())
}

pub(super) const KB_ACTION_MAP: &[(&str, &str)] = &[
    ("toggle-overview", "Niri Overview"),
    ("quit", "Quit Niri"),
    ("toggle-keyboard-shortcuts-inhibit", "Toggle shortcuts inhibit"),
    ("power-off-monitors", "Power off monitors"),
    ("show-hotkey-overlay", "Niri hotkey overlay"),
    ("close-window", "Close window"),
    ("maximize-column", "Maximize column"),
    ("maximize-window-to-edges", "Maximize to edges"),
    ("fullscreen-window", "Fullscreen"),
    ("toggle-window-floating", "Toggle floating"),
    ("switch-focus-between-floating-and-tiling", "Switch float/tile focus"),
    ("center-column", "Center column"),
    ("center-visible-columns", "Center visible columns"),
    ("expand-column-to-available-width", "Expand to available width"),
    ("consume-or-expel-window-left", "Consume/expel left"),
    ("consume-or-expel-window-right", "Consume/expel right"),
    ("expel-window-from-column", "Expel from column"),
    ("consume-window-into-column", "Consume into column"),
    ("switch-preset-column-width", "Cycle column width"),
    ("switch-preset-window-height", "Cycle window height"),
    ("reset-window-height", "Reset window height"),
    ("toggle-column-tabbed-display", "Toggle tabbed display"),
    ("focus-column-left", "Focus left"),
    ("focus-column-right", "Focus right"),
    ("focus-window-up", "Focus up"),
    ("focus-window-down", "Focus down"),
    ("focus-column-first", "Focus first column"),
    ("focus-column-last", "Focus last column"),
    ("focus-monitor-left", "Focus monitor left"),
    ("focus-monitor-right", "Focus monitor right"),
    ("focus-monitor-up", "Focus monitor up"),
    ("focus-monitor-down", "Focus monitor down"),
    ("move-column-left", "Move left"),
    ("move-column-right", "Move right"),
    ("move-window-up", "Move up"),
    ("move-window-down", "Move down"),
    ("move-column-to-first", "Move to first"),
    ("move-column-to-last", "Move to last"),
    ("move-column-to-monitor-left", "Move to monitor left"),
    ("move-column-to-monitor-right", "Move to monitor right"),
    ("move-column-to-monitor-up", "Move to monitor up"),
    ("move-column-to-monitor-down", "Move to monitor down"),
    ("focus-workspace-up", "Previous workspace"),
    ("focus-workspace-down", "Next workspace"),
    ("move-column-to-workspace-up", "Move to prev workspace"),
    ("move-column-to-workspace-down", "Move to next workspace"),
    ("move-workspace-up", "Move workspace up"),
    ("move-workspace-down", "Move workspace down"),
    ("screenshot", "Screenshot"),
    ("screenshot-screen", "Screenshot screen"),
    ("screenshot-window", "Screenshot window"),
];

pub(super) const KB_IPC_MAP: &[(&str, &str, &str)] = &[
    ("altSwitcher", "next", "Next window"),
    ("altSwitcher", "previous", "Previous window"),
    ("overlay", "toggle", "iNiR Overlay"),
    ("overview", "toggle", "iNiR Overview"),
    ("clipboard", "toggle", "Clipboard"),
    ("lock", "activate", "Lock screen"),
    ("region", "screenshot", "Screenshot region"),
    ("region", "ocr", "OCR region"),
    ("region", "search", "Reverse image search"),
    ("wallpaperSelector", "toggle", "Wallpaper selector"),
    ("settings", "open", "Settings"),
    ("cheatsheet", "toggle", "Cheatsheet"),
    ("panelFamily", "cycle", "Cycle panel style"),
    ("session", "toggle", "Session dialog"),
    ("browser", "open", "Browser"),
    ("audio", "volumeUp", "Volume up"),
    ("audio", "volumeDown", "Volume down"),
    ("audio", "mute", "Mute audio"),
    ("audio", "micMute", "Mute microphone"),
    ("brightness", "increment", "Brightness up"),
    ("brightness", "decrement", "Brightness down"),
    ("mpris", "playPause", "Play/Pause"),
    ("mpris", "next", "Next track"),
    ("mpris", "previous", "Previous track"),
    ("notifications", "clearAll", "Clear notifications"),
    ("gamemode", "toggle", "Toggle game mode"),
    ("launcher", "terminal", "Terminal"),
    ("launcher", "close-window", "Close window"),
];
pub(super) const KB_TERMINALS: &[&str] = &["foot", "kitty", "alacritty", "wezterm", "ghostty", "konsole", "gnome-terminal"];
pub(super) const KB_FILE_MANAGERS: &[&str] = &["dolphin", "nautilus", "thunar", "nemo", "pcmanfm", "ranger"];
pub(super) const KB_BROWSERS: &[&str] = &["firefox", "zen-browser", "chromium", "brave", "vivaldi"];
pub(super) const KB_CONFIG_ONLY_ACTIONS: &[&str] = &["next-window", "previous-window"];

pub(super) const VRR_WINDOW_RULE: &str = "\n// On-demand VRR: variable refresh rate only turns on for windows matched here.\n// Add your own app-ids to extend it, or remove the block to keep VRR always off.\nwindow-rule {\n    match app-id=\"^(gamescope|steam_app_[0-9]+|lutris|heroic|com\\\\.heroicgameslauncher\\\\.hgl|mpv|vlc)$\"\n\n    variable-refresh-rate true\n}\n";
pub(super) const BLUR_STRENGTH: &[(&str, &str, &str)] = &[("light", "2", "2.0"), ("balanced", "3", "3.0"), ("strong", "4", "4.5")];
pub(super) const BACKDROP_SHADOW_OVERRIDE_START: &str = "// >>> inir-backdrop-only >>>";
pub(super) const BACKDROP_SHADOW_OVERRIDE_END: &str = "// <<< inir-backdrop-only <<<";
pub(super) const ALT_TAB_OVERRIDE_START: &str = "// >>> inir-alt-tab >>>";
pub(super) const ALT_TAB_OVERRIDE_END: &str = "// <<< inir-alt-tab <<<";
