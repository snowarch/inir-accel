//! Native Rust replacements for iNiR's persistent Python daemons:
//! 1. `scripts/daemon/keyboard_lock_state_daemon.py` (`evdev` CapsLock/NumLock monitor)
//! 2. `scripts/daemon/dev_hot_reload.py` (`inotify` QML hot-reload watcher)
//! 3. `scripts/daemon/inir_super_overview_daemon.py` (`evdev` Super tap -> overview toggle, opt-in)

use serde_json::json;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::fs::{self, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// Linux input subsystem constants (<linux/input.h>, <linux/input-event-codes.h>)
const EV_KEY: u16 = 0x01;
const EV_LED: u16 = 0x11;

const KEY_CAPSLOCK: usize = 58;
const KEY_NUMLOCK: usize = 69;

const LED_NUML: usize = 0x00;
const LED_CAPSL: usize = 0x01;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct InputEvent {
    tv_sec: libc::time_t,
    tv_usec: libc::suseconds_t,
    type_: u16,
    code: u16,
    value: i32,
}

const fn ioc(dir: libc::c_ulong, type_: libc::c_ulong, nr: libc::c_ulong, size: libc::c_ulong) -> libc::c_ulong {
    (dir << 30) | (size << 16) | (type_ << 8) | nr
}

const IOC_READ: libc::c_ulong = 2;

const fn eviocgname(len: usize) -> libc::c_ulong {
    ioc(IOC_READ, b'E' as libc::c_ulong, 0x06, len as libc::c_ulong)
}

const fn eviocgled(len: usize) -> libc::c_ulong {
    ioc(IOC_READ, b'E' as libc::c_ulong, 0x19, len as libc::c_ulong)
}

const fn eviocgbit(ev: u16, len: usize) -> libc::c_ulong {
    ioc(IOC_READ, b'E' as libc::c_ulong, 0x20 + (ev as libc::c_ulong), len as libc::c_ulong)
}

#[inline]
fn test_bit(mask: &[u8], bit: usize) -> bool {
    let byte_idx = bit / 8;
    let bit_idx = bit % 8;
    byte_idx < mask.len() && (mask[byte_idx] & (1 << bit_idx)) != 0
}

fn monitor_paths() -> (PathBuf, PathBuf) {
    let uid = unsafe { libc::getuid() };
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"));
    let prefix = if runtime_dir != Path::new("/tmp") {
        "inir-keyboard-lock-state".to_string()
    } else {
        format!("inir-{uid}-keyboard-lock-state")
    };
    (
        runtime_dir.join(format!("{prefix}.pid")),
        runtime_dir.join(format!("{prefix}.lock")),
    )
}

fn is_owned_monitor(pid: i32) -> bool {
    if pid <= 1 || pid == (std::process::id() as i32) {
        return false;
    }
    let Ok(bytes) = fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    let cmdline = String::from_utf8_lossy(&bytes).replace('\0', " ");
    (cmdline.contains("keyboard_lock_state_daemon.py") || cmdline.contains("keyboard-lock-daemon"))
        && !cmdline.contains("--once")
}

fn claim_monitor_process() -> Option<PathBuf> {
    let (pid_path, lock_path) = monitor_paths();
    if let Some(parent) = lock_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let Ok(lock_file) = OpenOptions::new().create(true).append(true).open(&lock_path) else {
        return None;
    };

    unsafe {
        libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX);
    }

    let old_pid = fs::read_to_string(&pid_path)
        .ok()
        .and_then(|s| s.trim().parse::<i32>().ok())
        .unwrap_or(0);

    if is_owned_monitor(old_pid) {
        unsafe {
            libc::kill(old_pid, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(1);
        while is_owned_monitor(old_pid) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
        if is_owned_monitor(old_pid) {
            unsafe {
                libc::kill(old_pid, libc::SIGKILL);
            }
        }
    }

    let _ = fs::write(&pid_path, format!("{}\n", std::process::id()));
    Some(pid_path)
}

fn list_event_devices() -> Vec<PathBuf> {
    let mut devices = Vec::new();
    if let Ok(entries) = fs::read_dir("/dev/input") {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("event") {
                    devices.push(path);
                }
            }
        }
    }
    devices.sort();
    devices
}

fn is_candidate_keyboard(fd: i32) -> bool {
    let mut name_buf = [0u8; 256];
    let rc = unsafe { libc::ioctl(fd, eviocgname(name_buf.len()), name_buf.as_mut_ptr()) };
    if rc < 0 {
        return false;
    }
    let name_len = name_buf.iter().position(|&b| b == 0).unwrap_or(name_buf.len());
    let name = String::from_utf8_lossy(&name_buf[..name_len]).to_lowercase();
    if name.contains("ydotool") || name.contains("virtual") {
        return false;
    }

    let mut key_bits = [0u8; 96];
    let mut led_bits = [0u8; 8];
    let rc_key = unsafe { libc::ioctl(fd, eviocgbit(EV_KEY, key_bits.len()), key_bits.as_mut_ptr()) };
    let rc_led = unsafe { libc::ioctl(fd, eviocgbit(EV_LED, led_bits.len()), led_bits.as_mut_ptr()) };
    if rc_key < 0 || rc_led < 0 {
        return false;
    }

    let has_keys = test_bit(&key_bits, KEY_CAPSLOCK) || test_bit(&key_bits, KEY_NUMLOCK);
    let has_leds = test_bit(&led_bits, LED_CAPSL) || test_bit(&led_bits, LED_NUML);
    has_keys && has_leds
}

fn read_device_leds(fd: i32) -> Option<(bool, bool)> {
    let mut leds = [0u8; 8];
    let rc = unsafe { libc::ioctl(fd, eviocgled(leds.len()), leds.as_mut_ptr()) };
    if rc < 0 {
        return None;
    }
    Some((test_bit(&leds, LED_CAPSL), test_bit(&leds, LED_NUML)))
}

fn aggregate_bool(values: &[bool], previous: Option<bool>) -> bool {
    if values.is_empty() {
        return previous.unwrap_or(false);
    }
    let true_count = values.iter().filter(|&&v| v).count();
    let false_count = values.len() - true_count;
    if true_count == false_count {
        return previous.unwrap_or(false);
    }
    true_count > false_count
}

/// Native replacement for `scripts/daemon/keyboard_lock_state_daemon.py`.
/// Returns exit code (0 on success, 1 if no readable evdev keyboards exist).
pub fn run_keyboard_lock_daemon(once: bool) -> Result<i32, String> {
    let pid_file_guard = if !once {
        claim_monitor_process()
    } else {
        None
    };

    let mut open_devices: HashMap<PathBuf, i32> = HashMap::new();
    let mut inspected: HashSet<PathBuf> = HashSet::new();
    let mut last_state: Option<(bool, bool)> = None;

    let refresh_devices = |open_devices: &mut HashMap<PathBuf, i32>, inspected: &mut HashSet<PathBuf>| {
        let mut discovered: HashMap<PathBuf, i32> = HashMap::new();
        let mut next_inspected: HashSet<PathBuf> = HashSet::new();

        for path in list_event_devices() {
            let Ok(cpath) = CString::new(path.as_os_str().as_bytes()) else {
                continue;
            };
            let fd = unsafe {
                libc::open(
                    cpath.as_ptr(),
                    libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                continue;
            }
            next_inspected.insert(path.clone());
            if is_candidate_keyboard(fd) {
                discovered.insert(path, fd);
            } else {
                unsafe {
                    libc::close(fd);
                }
            }
        }

        let removed: Vec<PathBuf> = open_devices
            .keys()
            .filter(|k| !discovered.contains_key(*k))
            .cloned()
            .collect();
        for path in removed {
            if let Some(fd) = open_devices.remove(&path) {
                unsafe {
                    libc::close(fd);
                }
            }
        }

        for (path, fd) in discovered {
            if open_devices.contains_key(&path) {
                unsafe {
                    libc::close(fd);
                }
            } else {
                open_devices.insert(path, fd);
            }
        }
        *inspected = next_inspected;
    };

    let emit_state = |open_devices: &HashMap<PathBuf, i32>,
                      last_state: &mut Option<(bool, bool)>,
                      force: bool| {
        let mut caps_vals = Vec::new();
        let mut num_vals = Vec::new();
        for &fd in open_devices.values() {
            if let Some((caps, num)) = read_device_leds(fd) {
                caps_vals.push(caps);
                num_vals.push(num);
            }
        }
        if caps_vals.is_empty() && num_vals.is_empty() {
            return;
        }
        let prev_caps = last_state.map(|s| s.0);
        let prev_num = last_state.map(|s| s.1);
        let caps = aggregate_bool(&caps_vals, prev_caps);
        let num = aggregate_bool(&num_vals, prev_num);
        let next = (caps, num);
        if force || Some(next) != *last_state {
            let msg = json!({
                "type": "state",
                "caps": caps,
                "num": num,
                "devices": open_devices.len(),
            });
            println!("{msg}");
            let _ = std::io::stdout().flush();
        }
        *last_state = Some(next);
    };

    refresh_devices(&mut open_devices, &mut inspected);
    if open_devices.is_empty() {
        return Ok(1);
    }

    emit_state(&open_devices, &mut last_state, true);
    if once {
        for (_, fd) in open_devices {
            unsafe {
                libc::close(fd);
            }
        }
        return Ok(0);
    }

    let mut last_hotplug_check = Instant::now();
    loop {
        let paths_and_fds: Vec<(PathBuf, i32)> = open_devices.iter().map(|(p, &fd)| (p.clone(), fd)).collect();
        let mut pollfds: Vec<libc::pollfd> = paths_and_fds
            .iter()
            .map(|(_, fd)| libc::pollfd {
                fd: *fd,
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();

        let rc = unsafe { libc::poll(pollfds.as_mut_ptr(), pollfds.len() as libc::nfds_t, 5000) };
        if rc < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }

        let mut need_emit = false;
        let mut need_delayed_emit = false;
        let mut broken_paths = Vec::new();

        for (idx, pfd) in pollfds.iter().enumerate() {
            let (ref path, fd) = paths_and_fds[idx];
            if (pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL)) != 0 {
                broken_paths.push(path.clone());
                continue;
            }
            if (pfd.revents & libc::POLLIN) != 0 {
                let mut ev_buf = [0u8; std::mem::size_of::<InputEvent>() * 16];
                loop {
                    let n = unsafe { libc::read(fd, ev_buf.as_mut_ptr() as *mut libc::c_void, ev_buf.len()) };
                    if n < (std::mem::size_of::<InputEvent>() as isize) {
                        if n < 0 {
                            let err = std::io::Error::last_os_error();
                            if err.kind() != std::io::ErrorKind::WouldBlock {
                                broken_paths.push(path.clone());
                            }
                        }
                        break;
                    }
                    let count = (n as usize) / std::mem::size_of::<InputEvent>();
                    let events = unsafe {
                        std::slice::from_raw_parts(ev_buf.as_ptr() as *const InputEvent, count)
                    };
                    for ev in events {
                        if ev.type_ == EV_LED && (ev.code as usize == LED_CAPSL || ev.code as usize == LED_NUML) {
                            need_emit = true;
                        } else if ev.type_ == EV_KEY
                            && (ev.code as usize == KEY_CAPSLOCK || ev.code as usize == KEY_NUMLOCK)
                            && ev.value == 0
                        {
                            need_delayed_emit = true;
                        }
                    }
                }
            }
        }

        for path in broken_paths {
            if let Some(fd) = open_devices.remove(&path) {
                unsafe {
                    libc::close(fd);
                }
            }
            inspected.remove(&path);
        }

        if need_delayed_emit {
            thread::sleep(Duration::from_millis(30));
            need_emit = true;
        }
        if need_emit && !open_devices.is_empty() {
            emit_state(&open_devices, &mut last_state, false);
        }

        if last_hotplug_check.elapsed() >= Duration::from_secs(5) {
            last_hotplug_check = Instant::now();
            let current_set: HashSet<PathBuf> = list_event_devices().into_iter().collect();
            if current_set != inspected {
                refresh_devices(&mut open_devices, &mut inspected);
            }
            if !open_devices.is_empty() {
                emit_state(&open_devices, &mut last_state, false);
            }
        }
    }

    for (_, fd) in open_devices {
        unsafe {
            libc::close(fd);
        }
    }
    if let Some(pid_path) = pid_file_guard {
        if let Ok(content) = fs::read_to_string(&pid_path) {
            if content.trim() == std::process::id().to_string() {
                let _ = fs::remove_file(&pid_path);
            }
        }
    }
    Ok(0)
}

// ============================================================================
// 2. QML Dev Hot-Reload Watcher (`scripts/daemon/dev_hot_reload.py`)
// ============================================================================

fn scanner_reach(root: &Path) -> HashSet<PathBuf> {
    let mut files = HashSet::new();
    let mut dirs = HashSet::new();

    fn scan_file(
        path: &Path,
        root: &Path,
        files: &mut HashSet<PathBuf>,
        dirs: &mut HashSet<PathBuf>,
    ) {
        if files.contains(path) {
            return;
        }
        files.insert(path.to_path_buf());
        let mut imports = Vec::new();
        if let Ok(content) = fs::read_to_string(path) {
            for raw_line in content.lines() {
                let line = raw_line.trim();
                if line.starts_with("import") {
                    if let Some(at) = line.find(" qs.") {
                        let tail = &line[at + 4..];
                        let name: String = tail
                            .chars()
                            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
                            .collect();
                        if !name.is_empty() {
                            imports.push(root.join(name.replace('.', "/")));
                        }
                    } else if let Some(first_q) = line.find('"') {
                        let rest = &line[first_q + 1..];
                        if let Some(second_q) = rest.find('"') {
                            let r = &rest[..second_q];
                            if let Some(stripped) = r.strip_prefix("root:") {
                                imports.push(root.join(stripped.trim_start_matches('/')));
                            } else if let Some(parent) = path.parent() {
                                imports.push(parent.join(r));
                            }
                        }
                    }
                } else if line.contains('{') {
                    break;
                }
            }
        }
        if let Some(parent) = path.parent() {
            scan_dir(parent, root, files, dirs);
        }
        for target in imports {
            if target.is_dir() {
                if let Ok(canon) = fs::canonicalize(&target) {
                    scan_dir(&canon, root, files, dirs);
                } else {
                    scan_dir(&target, root, files, dirs);
                }
            }
        }
    }

    fn scan_dir(
        dir: &Path,
        root: &Path,
        files: &mut HashSet<PathBuf>,
        dirs: &mut HashSet<PathBuf>,
    ) {
        if dirs.contains(dir) {
            return;
        }
        dirs.insert(dir.to_path_buf());
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut sorted: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        sorted.sort();
        for full in sorted {
            if let Some(name) = full.file_name().and_then(|n| n.to_str()) {
                if name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                    && name.ends_with(".qml")
                    && full.is_file()
                {
                    scan_file(&full, root, files, dirs);
                }
            }
        }
    }

    let shell_qml = root.join("shell.qml");
    if shell_qml.exists() {
        scan_file(&shell_qml, root, &mut files, &mut dirs);
    }

    for d in &dirs {
        if let Ok(entries) = fs::read_dir(d) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) == Some("js") {
                    files.insert(p);
                }
            }
        }
    }
    files
}

fn file_digest(path: &Path) -> Option<(u64, u64)> {
    let mut f = fs::File::open(path).ok()?;
    let meta = f.metadata().ok()?;
    if meta.len() == 0 {
        return None;
    }
    let mut buf = Vec::with_capacity(meta.len() as usize);
    f.read_to_end(&mut buf).ok()?;
    let mut hasher = DefaultHasher::new();
    buf.hash(&mut hasher);
    Some((meta.len(), hasher.finish()))
}

fn shell_alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

fn reload_shell(root: &Path) {
    let _ = Command::new("qs")
        .args(["-p", &root.to_string_lossy(), "ipc", "call", "dev", "reload"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    thread::sleep(Duration::from_millis(2500));
}

/// Native replacement for `scripts/daemon/dev_hot_reload.py <shell_root> <shell_pid>`.
pub fn run_dev_hot_reload(root_dir: &Path, shell_pid: i32) -> Result<(), String> {
    let root = fs::canonicalize(root_dir).unwrap_or_else(|_| root_dir.to_path_buf());
    let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
    if fd < 0 {
        return Err("inotify_init1 failed".to_string());
    }

    let mut wd_dirs: HashMap<i32, PathBuf> = HashMap::new();
    let skip_names = [".git", "node_modules", "prebuilt", "native"];

    fn add_tree(fd: i32, top: &Path, wd_dirs: &mut HashMap<i32, PathBuf>, skip_names: &[&str]) {
        if !top.is_dir() {
            return;
        }
        if let Ok(cpath) = CString::new(top.as_os_str().as_bytes()) {
            let wd = unsafe {
                libc::inotify_add_watch(
                    fd,
                    cpath.as_ptr(),
                    libc::IN_CLOSE_WRITE | libc::IN_MOVED_TO | libc::IN_CREATE,
                )
            };
            if wd >= 0 {
                wd_dirs.insert(wd, top.to_path_buf());
            }
        }
        if let Ok(entries) = fs::read_dir(top) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if !skip_names.contains(&name) {
                        add_tree(fd, &path, wd_dirs, skip_names);
                    }
                }
            }
        }
    }

    for tree in ["modules", "services"] {
        add_tree(fd, &root.join(tree), &mut wd_dirs, &skip_names);
    }

    let mut reach = scanner_reach(&root);
    let mut hashes: HashMap<PathBuf, u64> = HashMap::new();
    for p in &reach {
        if let Some((_, h)) = file_digest(p) {
            hashes.insert(p.clone(), h);
        }
    }

    let debounce = Duration::from_millis(350);
    let qs_grace = Duration::from_millis(1200);
    let mut pending_since: Option<Instant> = None;
    let mut watched_since: Option<(Instant, f64)> = None;

    loop {
        if !shell_alive(shell_pid) {
            break;
        }

        let timeout_ms = if let Some(t) = pending_since {
            debounce.saturating_sub(t.elapsed()).as_millis() as i32
        } else if let Some((t, _)) = watched_since {
            qs_grace.saturating_sub(t.elapsed()).as_millis() as i32
        } else {
            2500
        };

        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let rc = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if rc < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }

        if (pfd.revents & libc::POLLIN) != 0 {
            let mut buf = [0u8; 65536];
            let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            if n > 0 {
                let data = &buf[..n as usize];
                let mut offset = 0usize;
                while offset + 16 <= data.len() {
                    let wd = i32::from_ne_bytes(data[offset..offset + 4].try_into().unwrap());
                    let mask = u32::from_ne_bytes(data[offset + 4..offset + 8].try_into().unwrap());
                    let len = u32::from_ne_bytes(data[offset + 12..offset + 16].try_into().unwrap()) as usize;
                    let name_end = (offset + 16 + len).min(data.len());
                    let raw_name = &data[offset + 16..name_end];
                    let name_len = raw_name.iter().position(|&b| b == 0).unwrap_or(raw_name.len());
                    let name = String::from_utf8_lossy(&raw_name[..name_len]);
                    offset = name_end;

                    let Some(dir) = wd_dirs.get(&wd) else {
                        continue;
                    };
                    let path = dir.join(name.as_ref());
                    if (mask & libc::IN_ISDIR) != 0 {
                        if (mask & libc::IN_CREATE) != 0 {
                            add_tree(fd, &path, &mut wd_dirs, &skip_names);
                        }
                        continue;
                    }
                    if !(name.ends_with(".qml") || name.ends_with(".js") || name == "qmldir") {
                        continue;
                    }
                    let Some((_, new_hash)) = file_digest(&path) else {
                        continue;
                    };
                    if hashes.get(&path) == Some(&new_hash) {
                        continue;
                    }
                    hashes.insert(path.clone(), new_hash);
                    if reach.contains(&path) {
                        if watched_since.is_none() {
                            let now_wall = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs_f64();
                            watched_since = Some((Instant::now(), now_wall));
                        }
                    } else {
                        pending_since = Some(Instant::now());
                    }
                }
            }
        }

        if let Some(t) = pending_since {
            if t.elapsed() >= debounce {
                pending_since = None;
                watched_since = None;
                reload_shell(&root);
                reach = scanner_reach(&root);
            }
        } else if let Some((t, _wall_since)) = watched_since {
            if t.elapsed() >= qs_grace {
                watched_since = None;
                reload_shell(&root);
                reach = scanner_reach(&root);
            }
        }
    }

    unsafe {
        libc::close(fd);
    }
    Ok(())
}

// ---------------------------------------------------------------- Super tap

const KEY_LEFTMETA: usize = 125;
const KEY_RIGHTMETA: usize = 126;
const POINTER_BUTTONS: [usize; 7] = [0x110, 0x111, 0x112, 0x113, 0x114, 0x115, 0x116];
// coalesces the same tap arriving from several devices (physical + virtual keyboards)
const SUPER_DEBOUNCE: Duration = Duration::from_millis(250);

struct SuperDevice {
    path: PathBuf,
    fd: i32,
    keyboard: bool,
    super_down: bool,
    chord: bool,
}

// A keyboard with Super or a pointer with buttons, never ydotool's or another virtual device (they echo).
fn open_super_device(path: &Path) -> Option<SuperDevice> {
    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC) };
    if fd < 0 {
        return None;
    }
    let mut name_buf = [0u8; 256];
    let mut key_bits = [0u8; 96];
    let ok = unsafe {
        libc::ioctl(fd, eviocgname(name_buf.len()), name_buf.as_mut_ptr()) >= 0
            && libc::ioctl(fd, eviocgbit(EV_KEY, key_bits.len()), key_bits.as_mut_ptr()) >= 0
    };
    let name_len = name_buf.iter().position(|&b| b == 0).unwrap_or(name_buf.len());
    let name = String::from_utf8_lossy(&name_buf[..name_len]).into_owned();
    let lower = name.to_lowercase();
    let keyboard = test_bit(&key_bits, KEY_LEFTMETA) || test_bit(&key_bits, KEY_RIGHTMETA);
    let pointer = POINTER_BUTTONS.iter().any(|&b| test_bit(&key_bits, b));
    if !ok || lower.contains("ydotool") || lower.contains("virtual") || !keyboard && !pointer {
        unsafe { libc::close(fd) };
        return None;
    }
    println!("[inir-super-daemon] Using {} device {} ({name})", if keyboard { "keyboard" } else { "pointer" }, path.display());
    Some(SuperDevice { path: path.to_path_buf(), fd, keyboard, super_down: false, chord: false })
}

// The running shell: stock Quickshell (`qs -c inir`, `qs -p <path>/inir`) or the runtime's own `inir-shell`.
fn find_shell_pid() -> Option<i32> {
    for entry in fs::read_dir("/proc").ok()?.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<i32>().ok()) else { continue };
        let Ok(raw) = fs::read(format!("/proc/{pid}/cmdline")) else { continue };
        let args: Vec<String> = raw.split(|&b| b == 0).filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
        let exe = args.first().and_then(|a| Path::new(a).file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if args.len() < 2 || !matches!(exe.as_str(), "qs" | "quickshell" | "inir-shell") {
            continue;
        }
        if args.len() >= 3 && args[1] == "-c" && args[2] == "inir" {
            return Some(pid);
        }
        if let Some(i) = args.iter().skip(1).position(|a| a == "-p").map(|i| i + 1) {
            if args.get(i + 1).is_some_and(|p| p.trim_end_matches('/').ends_with("/inir") || p.contains("/inir/")) {
                return Some(pid);
            }
        }
    }
    None
}

// What the IPC call needs from the shell's session, cached while its pid stays.
fn shell_env(cache: &mut Option<(i32, Vec<(String, String)>)>) -> Vec<(String, String)> {
    let Some(pid) = find_shell_pid() else {
        println!("[inir-super-daemon] inir not running, cannot import env");
        *cache = None;
        return Vec::new();
    };
    if let Some((_, env)) = cache.as_ref().filter(|(p, env)| *p == pid && !env.is_empty()) {
        return env.clone();
    }
    let raw = fs::read(format!("/proc/{pid}/environ")).unwrap_or_default();
    let env: Vec<(String, String)> = String::from_utf8_lossy(&raw)
        .split('\0')
        .filter_map(|e| e.split_once('='))
        .filter(|(k, _)| matches!(*k, "WAYLAND_DISPLAY" | "XDG_RUNTIME_DIR" | "QT_QPA_PLATFORM" | "NIRI_SOCKET"))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    println!("[inir-super-daemon] Imported env from inir pid={pid}: {env:?}");
    *cache = Some((pid, env.clone()));
    env
}

fn inir_launcher() -> String {
    if let Ok(path) = std::env::var("INIR_LAUNCHER_PATH") {
        return path;
    }
    std::env::var_os("PATH")
        .and_then(|paths| std::env::split_paths(&paths).map(|d| d.join("inir")).find(|p| p.is_file()))
        .map_or_else(|| "inir".to_string(), |p| p.to_string_lossy().into_owned())
}

/// A tap of Super with no other key or click toggles the inir overview. Devices are rescanned every 5 s, so one
/// unplugged comes back and one plugged in later is watched.
pub fn run_super_overview_daemon() -> i32 {
    let mut devices: Vec<SuperDevice> = Vec::new();
    let mut children: Vec<std::process::Child> = Vec::new();
    let mut env_cache = None;
    let (mut super_global, mut interaction, mut tap_handled) = (false, false, false);
    let mut last_toggle: Option<Instant> = None;
    let mut next_scan = Instant::now();
    let event_size = std::mem::size_of::<InputEvent>();
    loop {
        if Instant::now() >= next_scan {
            next_scan = Instant::now() + Duration::from_secs(5);
            for path in list_event_devices() {
                if !devices.iter().any(|d| d.path == path) {
                    devices.extend(open_super_device(&path));
                }
            }
            if !devices.iter().any(|d| d.keyboard) {
                println!("[inir-super-daemon] No keyboards with Super yet, retrying in 5s");
            }
            children.retain_mut(|c| matches!(c.try_wait(), Ok(None)));
        }
        let mut pollfds: Vec<libc::pollfd> = devices.iter().map(|d| libc::pollfd { fd: d.fd, events: libc::POLLIN, revents: 0 }).collect();
        let timeout = next_scan.saturating_duration_since(Instant::now()).as_millis() as i32;
        if unsafe { libc::poll(pollfds.as_mut_ptr(), pollfds.len() as libc::nfds_t, timeout) } <= 0 {
            continue;
        }
        let mut gone = Vec::new();
        for (idx, pfd) in pollfds.iter().enumerate() {
            if pfd.revents & libc::POLLIN == 0 {
                if pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                    gone.push(idx);
                }
                continue;
            }
            let device = &mut devices[idx];
            let mut buf = [0u8; std::mem::size_of::<InputEvent>() * 16];
            loop {
                let n = unsafe { libc::read(device.fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
                if n < event_size as isize {
                    // unplugged or asleep: the next rescan adopts it again
                    if n == 0 || n < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::WouldBlock {
                        gone.push(idx);
                    }
                    break;
                }
                let events = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const InputEvent, n as usize / event_size) };
                for ev in events.iter().filter(|ev| ev.type_ == EV_KEY) {
                    let (code, down) = (ev.code as usize, ev.value == 1);
                    if !device.keyboard {
                        if POINTER_BUTTONS.contains(&code) && down && super_global {
                            interaction = true;
                        }
                        continue;
                    }
                    if code != KEY_LEFTMETA && code != KEY_RIGHTMETA {
                        device.chord |= device.super_down && down;
                        interaction |= super_global && down;
                        continue;
                    }
                    if down {
                        (device.super_down, device.chord, super_global, interaction, tap_handled) = (true, false, true, false, false);
                    } else if ev.value == 0 {
                        let tap = device.super_down && !device.chord && !interaction && !tap_handled;
                        if tap && last_toggle.is_none_or(|t| t.elapsed() >= SUPER_DEBOUNCE) {
                            last_toggle = Some(Instant::now());
                            tap_handled = true;
                            println!("[inir-super-daemon] Super tap detected, toggling inir overview");
                            let env = shell_env(&mut env_cache);
                            if env.is_empty() {
                                println!("[inir-super-daemon] No inir env available, skipping toggle");
                            } else {
                                match Command::new(inir_launcher()).args(["overview", "toggle"]).envs(env).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
                                    Ok(child) => children.push(child),
                                    Err(e) => println!("[inir-super-daemon] Error running toggle command: {e}"),
                                }
                            }
                        }
                        (device.super_down, device.chord, super_global, interaction) = (false, false, false, false);
                    }
                }
            }
        }
        gone.dedup();
        for idx in gone.into_iter().rev() {
            unsafe { libc::close(devices.remove(idx).fd) };
        }
    }
}
