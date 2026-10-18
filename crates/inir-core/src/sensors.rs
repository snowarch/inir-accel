//! Fast hardware sensor & system telemetry engine in Rust.
//! Replaces `scripts/detect_sensors.py`, `df`, `lscpu`, and 9 separate FileViews in `ResourceUsage.qml`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const CPU_HWMON: &[&str] = &["coretemp", "k10temp", "zenpower", "fam15h_power", "cpu_thermal", "via_cputemp"];
const GPU_HWMON: &[&str] = &["amdgpu", "radeon", "nouveau", "nvidia", "i915", "xe", "panfrost", "lima", "v3d", "vc4"];
const BOARD_HWMON: &[&str] = &["nct6775", "it87", "w83627ehf", "lm75", "lm78", "lm85"];

fn get_content(path: &Path) -> String {
    fs::read_to_string(path).map(|s| s.trim().to_string()).unwrap_or_default()
}

fn temp_inputs(hwmon_path: &Path) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(hwmon_path) else { return Vec::new() };
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
            name.starts_with("temp") && name.ends_with("_input")
        })
        .collect();
    v.sort();
    v
}

fn label_for(input_path: &Path) -> String {
    let s = input_path.to_string_lossy();
    if let Some(stem) = s.strip_suffix("_input") {
        get_content(Path::new(&format!("{stem}_label")))
    } else {
        String::new()
    }
}

fn explicit_cpu_label(label: &str) -> bool {
    let lower = label.to_ascii_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let forbidden = ["vrm", "vr", "soc", "pch"];
    let wanted = ["cpu", "package", "tdie", "tctl", "processor"];
    !words.iter().any(|w| forbidden.contains(w)) && words.iter().any(|w| wanted.contains(w))
}

fn cpu_sensor_input(hwmon_path: &Path, name: &str) -> Option<(PathBuf, String)> {
    let labelled: Vec<(PathBuf, String)> = temp_inputs(hwmon_path)
        .into_iter()
        .map(|p| {
            let l = label_for(&p);
            (p, l)
        })
        .collect();
    if labelled.is_empty() {
        return None;
    }
    if name == "coretemp" {
        if let Some(found) = labelled.iter().find(|(_, l)| l.to_ascii_lowercase().contains("package")) {
            return Some(found.clone());
        }
        return Some(labelled[0].clone());
    }
    if matches!(name, "k10temp" | "zenpower" | "fam15h_power") {
        for needle in ["tdie", "tctl"] {
            if let Some(found) = labelled.iter().find(|(_, l)| l.to_ascii_lowercase().contains(needle)) {
                return Some(found.clone());
            }
        }
        return Some(labelled[0].clone());
    }
    if let Some(found) = labelled.iter().find(|(_, l)| explicit_cpu_label(l)) {
        return Some(found.clone());
    }
    Some(labelled[0].clone())
}

fn gpu_sensor_input(hwmon_path: &Path) -> Option<(PathBuf, String)> {
    let labelled: Vec<(PathBuf, String)> = temp_inputs(hwmon_path)
        .into_iter()
        .map(|p| {
            let l = label_for(&p);
            (p, l)
        })
        .collect();
    if let Some(edge) = labelled.iter().find(|(_, l)| l.to_ascii_lowercase().contains("edge")) {
        return Some(edge.clone());
    }
    labelled
        .into_iter()
        .find(|(_, l)| matches!(l.trim().to_ascii_lowercase().as_str(), "" | "gpu" | "gpu core" | "core"))
}

#[derive(Clone, Debug)]
struct DrmCard {
    card: PathBuf,
    number: u32,
    device: PathBuf,
    vendor: String,
    busy: bool,
}

fn drm_cards(root: &Path) -> Vec<DrmCard> {
    let drm_dir = root.join("class/drm");
    let Ok(rd) = fs::read_dir(&drm_dir) else { return Vec::new() };
    let mut cards = Vec::new();
    for entry in rd.flatten() {
        let p = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(num_str) = name.strip_prefix("card") else { continue };
        let Ok(number) = num_str.parse::<u32>() else { continue };
        let dev_link = p.join("device");
        let Ok(device) = fs::canonicalize(&dev_link) else { continue };
        let vendor = get_content(&device.join("vendor")).to_ascii_lowercase();
        let busy = device.join("gpu_busy_percent").is_file();
        cards.push(DrmCard {
            card: p,
            number,
            device,
            vendor,
            busy,
        });
    }
    cards.sort_by_key(|c| (c.vendor != "0x10de", !c.busy, c.number));
    cards
}

/// Discover stable sysfs paths used by ResourceUsage, identical to `detect_sensors.py`.
pub fn detect_sensors(sysfs_root: &str) -> BTreeMap<String, String> {
    let root = Path::new(sysfs_root);
    let hwmon_dir = root.join("class/hwmon");
    let mut hwmons: Vec<PathBuf> = fs::read_dir(&hwmon_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    hwmons.sort();

    let mut cpu_path: Option<PathBuf> = None;
    let mut cpu_label = String::new();

    for &pref in CPU_HWMON {
        for hw in &hwmons {
            let name = get_content(&hw.join("name")).to_ascii_lowercase();
            if name == pref {
                if let Some((cand, lbl)) = cpu_sensor_input(hw, &name) {
                    cpu_path = Some(cand);
                    cpu_label = lbl;
                    break;
                }
            }
        }
        if cpu_path.is_some() {
            break;
        }
    }

    if cpu_path.is_none() {
        let thermal_dir = root.join("class/thermal");
        let mut zones: Vec<PathBuf> = fs::read_dir(&thermal_dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect();
        zones.sort();
        for z in zones {
            let ztype = get_content(&z.join("type")).to_ascii_lowercase();
            let tfile = z.join("temp");
            if !tfile.is_file() {
                continue;
            }
            let is_cpu_n = ztype.strip_prefix("cpu").is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()));
            if matches!(ztype.as_str(), "x86_pkg_temp" | "cpu" | "cpu_thermal" | "processor" | "package") || is_cpu_n {
                cpu_path = Some(tfile);
                cpu_label = ztype;
                break;
            }
        }
    }

    if cpu_path.is_none() {
        for hw in &hwmons {
            let name = get_content(&hw.join("name")).to_ascii_lowercase();
            if BOARD_HWMON.contains(&name.as_str()) {
                if let Some((cand, lbl)) = cpu_sensor_input(hw, "board") {
                    if explicit_cpu_label(&lbl) {
                        cpu_path = Some(cand);
                        cpu_label = lbl;
                        break;
                    }
                }
            }
        }
    }

    let cards = drm_cards(root);
    let selected = cards.first();
    let mut gpu_path: Option<PathBuf> = None;
    let mut gpu_label = String::new();

    if let Some(sel) = selected {
        let mut matching_hwmons = Vec::new();
        for hw in &hwmons {
            let name = get_content(&hw.join("name")).to_ascii_lowercase();
            if !GPU_HWMON.contains(&name.as_str()) {
                continue;
            }
            if let Ok(dev) = fs::canonicalize(hw.join("device")) {
                if dev == sel.device {
                    matching_hwmons.push(hw.clone());
                }
            }
        }
        matching_hwmons.sort();
        for hw in matching_hwmons {
            if let Some((cand, lbl)) = gpu_sensor_input(&hw) {
                gpu_path = Some(cand);
                gpu_label = lbl;
                break;
            }
        }
    }

    let mut result = BTreeMap::new();
    if let Some(cp) = cpu_path {
        let real = fs::canonicalize(&cp).unwrap_or(cp);
        result.insert("cpu".to_string(), real.to_string_lossy().into_owned());
        if !cpu_label.is_empty() {
            result.insert("cpuLabel".to_string(), cpu_label);
        }
    }
    if let Some(sel) = selected {
        result.insert("gpuDevice".to_string(), sel.device.to_string_lossy().into_owned());
        result.insert("gpuVendor".to_string(), sel.vendor.clone());
        result.insert("gpuCard".to_string(), sel.card.to_string_lossy().into_owned());
        let rt = sel.device.join("power/runtime_status");
        if rt.exists() {
            let real = fs::canonicalize(&rt).unwrap_or(rt);
            result.insert("gpuRuntime".to_string(), real.to_string_lossy().into_owned());
        }
    }
    if let Some(gp) = gpu_path {
        let real = fs::canonicalize(&gp).unwrap_or(gp);
        result.insert("gpu".to_string(), real.to_string_lossy().into_owned());
        if !gpu_label.is_empty() {
            result.insert("gpuLabel".to_string(), gpu_label);
        }
    }
    result
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSnapshot {
    pub memory_total_kb: u64,
    pub memory_available_kb: u64,
    pub memory_used_kb: u64,
    pub memory_used_percentage: f64,
    pub swap_total_kb: u64,
    pub swap_free_kb: u64,
    pub swap_used_kb: u64,
    pub swap_used_percentage: f64,
    pub cpu_total_jiffies: u64,
    pub cpu_idle_jiffies: u64,
    pub cpu_frequency_mhz: f64,
    pub cpu_temp_celsius: i32,
    pub gpu_temp_celsius: i32,
    pub gpu_usage: f64,
    pub gpu_memory_used_gb: f64,
    pub gpu_memory_total_gb: f64,
    pub disk_total_bytes: u64,
    pub disk_used_bytes: u64,
    pub disk_used_percentage: f64,
}

/// Single-call zero-spawn snapshot of `/proc/meminfo`, `/proc/stat`, `/proc/cpuinfo`, sysfs temperatures/GPU, and `statvfs("/")`.
pub fn sample_resources(sensors: &BTreeMap<String, String>) -> ResourceSnapshot {
    let mut snap = ResourceSnapshot {
        cpu_frequency_mhz: -1.0,
        gpu_memory_used_gb: -1.0,
        gpu_memory_total_gb: -1.0,
        ..Default::default()
    };

    if let Ok(mem) = fs::read_to_string("/proc/meminfo") {
        for line in mem.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                snap.memory_total_kb = parse_first_u64(rest);
            } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
                snap.memory_available_kb = parse_first_u64(rest);
            } else if let Some(rest) = line.strip_prefix("SwapTotal:") {
                snap.swap_total_kb = parse_first_u64(rest);
            } else if let Some(rest) = line.strip_prefix("SwapFree:") {
                snap.swap_free_kb = parse_first_u64(rest);
            }
        }
        snap.memory_used_kb = snap.memory_total_kb.saturating_sub(snap.memory_available_kb);
        if snap.memory_total_kb > 0 {
            snap.memory_used_percentage = snap.memory_used_kb as f64 / snap.memory_total_kb as f64;
        }
        snap.swap_used_kb = snap.swap_total_kb.saturating_sub(snap.swap_free_kb);
        if snap.swap_total_kb > 0 {
            snap.swap_used_percentage = snap.swap_used_kb as f64 / snap.swap_total_kb as f64;
        }
    }

    if let Ok(stat) = fs::read_to_string("/proc/stat") {
        if let Some(first) = stat.lines().next() {
            if let Some(rest) = first.strip_prefix("cpu ") {
                let nums: Vec<u64> = rest
                    .split_whitespace()
                    .take(8)
                    .filter_map(|s| s.parse().ok())
                    .collect();
                if nums.len() >= 5 {
                    snap.cpu_total_jiffies = nums.iter().sum();
                    snap.cpu_idle_jiffies = nums[3] + nums[4];
                }
            }
        }
    }

    if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
        for line in cpuinfo.lines() {
            if line.starts_with("cpu MHz") {
                if let Some((_, val)) = line.split_once(':') {
                    if let Ok(mhz) = val.trim().parse::<f64>() {
                        snap.cpu_frequency_mhz = mhz;
                        break;
                    }
                }
            }
        }
    }

    if let Some(cpu_temp_path) = sensors.get("cpu") {
        if let Ok(raw) = fs::read_to_string(cpu_temp_path) {
            if let Ok(millideg) = raw.trim().parse::<f64>() {
                let c = (millideg / 1000.0).round() as i32;
                if c > 0 && c < 150 {
                    snap.cpu_temp_celsius = c;
                }
            }
        }
    }

    let gpu_awake = sensors
        .get("gpuRuntime")
        .and_then(|p| fs::read_to_string(p).ok())
        .map(|s| matches!(s.trim(), "active" | "unsupported"))
        .unwrap_or(true);

    if gpu_awake {
        if let Some(gpu_temp_path) = sensors.get("gpu") {
            if let Ok(raw) = fs::read_to_string(gpu_temp_path) {
                if let Ok(millideg) = raw.trim().parse::<f64>() {
                    let c = (millideg / 1000.0).round() as i32;
                    if c > 0 && c < 150 {
                        snap.gpu_temp_celsius = c;
                    }
                }
            }
        }
        if let Some(gpu_dev) = sensors.get("gpuDevice") {
            let dev_path = Path::new(gpu_dev);
            if let Ok(busy) = fs::read_to_string(dev_path.join("gpu_busy_percent")) {
                if let Ok(pct) = busy.trim().parse::<f64>() {
                    snap.gpu_usage = (pct / 100.0).clamp(0.0, 1.0);
                }
            }
            if let Ok(used) = fs::read_to_string(dev_path.join("mem_info_vram_used")) {
                if let Ok(bytes) = used.trim().parse::<f64>() {
                    snap.gpu_memory_used_gb = bytes / (1024.0 * 1024.0 * 1024.0);
                }
            }
            if let Ok(total) = fs::read_to_string(dev_path.join("mem_info_vram_total")) {
                if let Ok(bytes) = total.trim().parse::<f64>() {
                    snap.gpu_memory_total_gb = bytes / (1024.0 * 1024.0 * 1024.0);
                }
            }
        }
    }

    unsafe {
        let mut vfs: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c"/".as_ptr(), &mut vfs) == 0 {
            let bsize = vfs.f_frsize as u64;
            let total = vfs.f_blocks * bsize;
            let avail = vfs.f_bavail * bsize;
            let used = total.saturating_sub(avail);
            snap.disk_total_bytes = total.max(1);
            snap.disk_used_bytes = used;
            snap.disk_used_percentage = used as f64 / snap.disk_total_bytes as f64;
        }
    }

    snap
}

fn parse_first_u64(s: &str) -> u64 {
    s.split_whitespace()
        .next()
        .and_then(|tok| tok.parse().ok())
        .unwrap_or(0)
}
