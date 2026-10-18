// This code was written by: Hikari.
//! Native QML singleton `InirNative` (`import Inir.Native 1.0`) for Quickshell and `qs-rs`.
//!
//! Provides zero-spawn, sub-millisecond access from QML to:
//! - Washi v30 theme derivation (`InirNative.solveWashi(json)`)
//! - Material You image color extraction + contrast solver (`InirNative.generateMaterialColors(...)`)
//! - Wallpaper Luma Grid & least-busy region sampling (`InirNative.computeLumaGridJson(...)`)
//! - Fast thumbnail generation (`InirNative.makeThumbnail(...)`)
//! - Hardware sensor discovery and `/proc` + `sysfs` telemetry (`InirNative.refreshResources()`)
//! - Desktop entry fuzzy search & window icon resolution (`InirNative.fuzzySearchAppsJson(...)`, `InirNative.guessIcon(...)`)
//! - Bar widget & plugin manifest scanning (`InirNative.scanWidgetsJson(...)`, `InirNative.scanPluginsJson(...)`)
//! - Parallel QML VFS pre-warming (`InirNative.prewarmVfsJson(...)`)

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element = "InirNative"]
        #[qml_singleton]
        #[qproperty(QString, version, cxx_name = "version", READ = version, CONSTANT)]
        #[qproperty(f64, mem_total_kb, cxx_name = "memTotalKb")]
        #[qproperty(f64, mem_used_kb, cxx_name = "memUsedKb")]
        #[qproperty(f64, mem_used_ratio, cxx_name = "memUsedRatio")]
        #[qproperty(f64, swap_total_kb, cxx_name = "swapTotalKb")]
        #[qproperty(f64, swap_used_kb, cxx_name = "swapUsedKb")]
        #[qproperty(f64, swap_used_ratio, cxx_name = "swapUsedRatio")]
        #[qproperty(f64, cpu_usage_ratio, cxx_name = "cpuUsageRatio")]
        #[qproperty(i32, cpu_temp_c, cxx_name = "cpuTempC")]
        #[qproperty(i32, gpu_temp_c, cxx_name = "gpuTempC")]
        #[qproperty(f64, max_cpu_freq_ghz, cxx_name = "maxCpuFreqGhz")]
        #[qproperty(QString, cpu_arch, cxx_name = "cpuArch")]
        #[qproperty(f64, disk_total_bytes, cxx_name = "diskTotalBytes")]
        #[qproperty(f64, disk_used_bytes, cxx_name = "diskUsedBytes")]
        #[qproperty(f64, disk_used_ratio, cxx_name = "diskUsedRatio")]
        type InirNative = super::InirNativeRust;

        fn version(self: &InirNative) -> QString;

        #[qinvokable]
        #[cxx_name = "refreshResources"]
        fn refresh_resources(self: Pin<&mut InirNative>) -> QString;

        #[qinvokable]
        #[cxx_name = "detectSensorsJson"]
        fn detect_sensors_json(self: &InirNative) -> QString;

        #[qinvokable]
        #[cxx_name = "solveWashi"]
        fn solve_washi(self: &InirNative, input_json: &QString) -> QString;

        #[qinvokable]
        #[cxx_name = "generateMaterialColors"]
        fn generate_material_colors(
            self: &InirNative,
            image_path: &QString,
            mode: &QString,
            scheme: &QString,
            size: u32,
            color_hex: &QString,
            render_templates_dir: &QString,
        ) -> QString;

        #[qinvokable]
        #[cxx_name = "computeLumaGridJson"]
        fn compute_luma_grid_json(
            self: &InirNative,
            image_path: &QString,
            cell: i32,
            screen_width: i32,
            screen_height: i32,
            screen_mode: &QString,
        ) -> QString;

        #[qinvokable]
        #[cxx_name = "makeThumbnail"]
        fn make_thumbnail(self: &InirNative, file_path: &QString, size_name: &QString) -> bool;

        #[qinvokable]
        #[cxx_name = "fuzzySearchAppsJson"]
        fn fuzzy_search_apps_json(self: &InirNative, query: &QString, sloppy: bool) -> QString;

        #[qinvokable]
        #[cxx_name = "guessIcon"]
        fn guess_icon(self: &InirNative, window_class: &QString) -> QString;

        #[qinvokable]
        #[cxx_name = "resolveIconTheme"]
        fn resolve_icon_theme(self: &InirNative, config_theme: &QString, mode: &QString) -> QString;

        #[qinvokable]
        #[cxx_name = "scanPluginsJson"]
        fn scan_plugins_json(self: &InirNative) -> QString;

        #[qinvokable]
        #[cxx_name = "scanWidgetsJson"]
        fn scan_widgets_json(self: &InirNative, widgets_dir: &QString) -> QString;

        #[qinvokable]
        #[cxx_name = "prewarmVfsJson"]
        fn prewarm_vfs_json(self: &InirNative, shell_dir: &QString) -> QString;
    }
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;
use inir_core::{apps, material, prewarm, region, sensors, thumbgen, washi};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

pub struct InirNativeRust {
    mem_total_kb: f64,
    mem_used_kb: f64,
    mem_used_ratio: f64,
    swap_total_kb: f64,
    swap_used_kb: f64,
    swap_used_ratio: f64,
    cpu_usage_ratio: f64,
    cpu_temp_c: i32,
    gpu_temp_c: i32,
    max_cpu_freq_ghz: f64,
    cpu_arch: QString,
    disk_total_bytes: f64,
    disk_used_bytes: f64,
    disk_used_ratio: f64,
    prev_cpu: Mutex<Option<(u64, u64)>>,
    detected_sensors: Mutex<Option<BTreeMap<String, String>>>,
    cached_apps: Mutex<Option<Vec<apps::DesktopEntryInfo>>>,
}

impl Default for InirNativeRust {
    fn default() -> Self {
        Self {
            mem_total_kb: 1.0,
            mem_used_kb: 0.0,
            mem_used_ratio: 0.0,
            swap_total_kb: 1.0,
            swap_used_kb: 0.0,
            swap_used_ratio: 0.0,
            cpu_usage_ratio: 0.0,
            cpu_temp_c: 0,
            gpu_temp_c: 0,
            max_cpu_freq_ghz: 0.0,
            cpu_arch: QString::from(std::env::consts::ARCH),
            disk_total_bytes: 1.0,
            disk_used_bytes: 0.0,
            disk_used_ratio: 0.0,
            prev_cpu: Mutex::new(None),
            detected_sensors: Mutex::new(None),
            cached_apps: Mutex::new(None),
        }
    }
}

impl qobject::InirNative {
    fn version(&self) -> QString {
        QString::from(env!("CARGO_PKG_VERSION"))
    }

    fn refresh_resources(mut self: Pin<&mut Self>) -> QString {
        let detected = {
            let mut guard = self.rust().detected_sensors.lock().unwrap();
            if guard.is_none() {
                *guard = Some(sensors::detect_sensors("/sys"));
            }
            guard.clone().unwrap()
        };
        let snap = sensors::sample_resources(&detected);
        let cpu_ratio = {
            let mut prev_guard = self.rust().prev_cpu.lock().unwrap();
            let ratio = if let Some((pt, pi)) = *prev_guard {
                let dt = snap.cpu_total_jiffies.saturating_sub(pt);
                let di = snap.cpu_idle_jiffies.saturating_sub(pi);
                if dt > 0 {
                    (dt.saturating_sub(di)) as f64 / dt as f64
                } else {
                    0.0
                }
            } else {
                0.0
            };
            *prev_guard = Some((snap.cpu_total_jiffies, snap.cpu_idle_jiffies));
            ratio
        };

        self.as_mut().set_mem_total_kb(snap.memory_total_kb as f64);
        self.as_mut().set_mem_used_kb(snap.memory_used_kb as f64);
        self.as_mut().set_mem_used_ratio(snap.memory_used_percentage);
        self.as_mut().set_swap_total_kb(snap.swap_total_kb as f64);
        self.as_mut().set_swap_used_kb(snap.swap_used_kb as f64);
        self.as_mut().set_swap_used_ratio(snap.swap_used_percentage);
        self.as_mut().set_cpu_usage_ratio(cpu_ratio);
        self.as_mut().set_cpu_temp_c(snap.cpu_temp_celsius);
        self.as_mut().set_gpu_temp_c(snap.gpu_temp_celsius);
        if snap.cpu_frequency_mhz > 0.0 {
            self.as_mut().set_max_cpu_freq_ghz(snap.cpu_frequency_mhz / 1000.0);
        }
        self.as_mut().set_disk_total_bytes(snap.disk_total_bytes as f64);
        self.as_mut().set_disk_used_bytes(snap.disk_used_bytes as f64);
        self.as_mut().set_disk_used_ratio(snap.disk_used_percentage);

        let json = serde_json::to_string(&snap).unwrap_or_else(|_| "{}".to_string());
        QString::from(json.as_str())
    }

    fn detect_sensors_json(&self) -> QString {
        let detected = sensors::detect_sensors("/sys");
        if let Ok(mut guard) = self.rust().detected_sensors.lock() {
            *guard = Some(detected.clone());
        }
        let json = serde_json::to_string(&detected).unwrap_or_else(|_| "{}".to_string());
        QString::from(json.as_str())
    }

    fn solve_washi(&self, input_json: &QString) -> QString {
        let s = input_json.to_string();
        let req = if s.trim().is_empty() {
            washi::default_request()
        } else {
            serde_json::from_str::<washi::Request>(&s).unwrap_or_else(|_| washi::default_request())
        };
        let out = washi::solve_all(&req, &s);
        let json = serde_json::to_string(&out).unwrap_or_else(|_| "{}".to_string());
        QString::from(json.as_str())
    }

    fn generate_material_colors(
        &self,
        image_path: &QString,
        mode: &QString,
        scheme: &QString,
        size: u32,
        color_hex: &QString,
        render_templates_dir: &QString,
    ) -> QString {
        let img_str = image_path.to_string();
        let color_str = color_hex.to_string();
        let tpl_str = render_templates_dir.to_string();
        let mut cfg = material::MaterialGenConfig::default();
        if !img_str.is_empty() {
            cfg.path = Some(img_str);
        }
        if !color_str.is_empty() {
            cfg.color = Some(color_str);
        }
        cfg.mode = mode.to_string();
        cfg.scheme = scheme.to_string();
        cfg.size = if size == 0 { 128 } else { size };
        if !tpl_str.is_empty() {
            cfg.render_templates = Some(tpl_str);
        }
        match material::run_material_pipeline(cfg) {
            Ok(scss) => QString::from(scss.as_str()),
            Err(e) => QString::from(format!("{{\"error\":{:?}}}", e).as_str()),
        }
    }

    fn compute_luma_grid_json(
        &self,
        image_path: &QString,
        cell: i32,
        screen_width: i32,
        screen_height: i32,
        screen_mode: &QString,
    ) -> QString {
        let path_str = image_path.to_string();
        let mode_str = screen_mode.to_string();
        let mode = if mode_str.is_empty() { "fill" } else { mode_str.as_str() };
        match region::compute_luma_grid(
            Path::new(&path_str),
            cell.max(1) as usize,
            screen_width.max(1) as u32,
            screen_height.max(1) as u32,
            mode,
        ) {
            Ok(grid) => {
                let json = serde_json::to_string(&grid).unwrap_or_else(|_| "{}".to_string());
                QString::from(json.as_str())
            }
            Err(e) => QString::from(format!("{{\"error\":{:?}}}", e).as_str()),
        }
    }

    fn make_thumbnail(&self, file_path: &QString, size_name: &QString) -> bool {
        let fpath = file_path.to_string();
        let sz = size_name.to_string();
        thumbgen::make_thumbnail_one(Path::new(&fpath), if sz.is_empty() { "large" } else { sz.as_str() })
    }

    fn fuzzy_search_apps_json(&self, query: &QString, sloppy: bool) -> QString {
        let apps_list = {
            let mut guard = self.rust().cached_apps.lock().unwrap();
            if guard.is_none() {
                *guard = Some(apps::scan_desktop_entries());
            }
            guard.clone().unwrap()
        };
        let q = query.to_string();
        let res = apps::fuzzy_search_apps(&apps_list, &q, sloppy);
        let json = serde_json::to_string(&res).unwrap_or_else(|_| "[]".to_string());
        QString::from(json.as_str())
    }

    fn guess_icon(&self, window_class: &QString) -> QString {
        let apps_list = {
            let mut guard = self.rust().cached_apps.lock().unwrap();
            if guard.is_none() {
                *guard = Some(apps::scan_desktop_entries());
            }
            guard.clone().unwrap()
        };
        let wc = window_class.to_string();
        let icon = apps::guess_icon(&wc, &apps_list);
        QString::from(icon.as_str())
    }

    fn resolve_icon_theme(&self, config_theme: &QString, mode: &QString) -> QString {
        let ct = config_theme.to_string();
        let m = mode.to_string();
        let resolved = apps::icon_theme_for_mode(&ct, &m);
        QString::from(resolved.as_str())
    }

    fn scan_plugins_json(&self) -> QString {
        let json = apps::scan_plugins_json();
        QString::from(json.as_str())
    }

    fn scan_widgets_json(&self, widgets_dir: &QString) -> QString {
        let dir_str = widgets_dir.to_string();
        let json = apps::scan_widgets_json(&dir_str, None);
        QString::from(json.as_str())
    }

    fn prewarm_vfs_json(&self, shell_dir: &QString) -> QString {
        let dir_str = shell_dir.to_string();
        let path = if dir_str.is_empty() {
            Path::new("/home/user/inir")
        } else {
            Path::new(&dir_str)
        };
        match prewarm::build_and_prewarm_vfs(path, None) {
            Ok(rep) => {
                let json = serde_json::to_string(&rep).unwrap_or_else(|_| "{}".to_string());
                QString::from(json.as_str())
            }
            Err(e) => QString::from(format!("{{\"error\":{:?}}}", e.to_string()).as_str()),
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate cxx_qt_lib;

    #[test]
    fn test_bridge_init() {
        cxx_qt::init_crate!(cxx_qt);
        cxx_qt::init_crate!(cxx_qt_lib);
    }
}
