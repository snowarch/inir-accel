//! Parallel VFS Builder & QML Pre-warmer / Cache Accelerator for iNiR on Quickshell / qs-rs.
//! Mirrors `inir-qs-rust/crates/qs-rs/src/scan.rs` in parallel via Rayon and pre-warms VFS + OS page cache.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone, Default)]
pub struct FileInfo {
    pub singleton: bool,
    pub internal: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrewarmReport {
    pub root_dir: String,
    pub vfs_dir: String,
    pub directories_scanned: usize,
    pub qml_files_scanned: usize,
    pub rewritten_files: usize,
    pub synthesized_qmldirs: usize,
    pub elapsed_ms: u128,
}

/// Parallel VFS scanner and builder compatible with `qs-rs`'s `$XDG_RUNTIME_DIR/quickshell/by-shell/inir/vfs/qs`
/// and `$XDG_RUNTIME_DIR/qs-rs/vfs-inir`.
pub fn build_and_prewarm_vfs(shell_root: &Path, vfs_target: Option<&Path>) -> io::Result<PrewarmReport> {
    let start = Instant::now();
    let root = shell_root.canonicalize()?;

    // Collect all non-hidden directories containing QML files
    let mut all_dirs = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        let mut has_qml = false;
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() && !name.starts_with('.') {
                stack.push(e.path());
            } else if ft.is_file() && name.ends_with(".qml") {
                has_qml = true;
            }
        }
        if has_qml {
            all_dirs.push(dir);
        }
    }
    all_dirs.sort();

    // Process every directory in parallel using Rayon
    let dir_results: Vec<(
        PathBuf,
        Vec<PathBuf>,
        Option<(PathBuf, String)>,
        Vec<(PathBuf, String)>,
    )> = all_dirs
        .par_iter()
        .map(|dir| {
            let mut scanned_files = Vec::new();
            let mut intercepts = Vec::new();
            let mut entries = Vec::new();

            let Ok(rd) = fs::read_dir(dir) else {
                return (dir.clone(), scanned_files, None, intercepts);
            };
            let mut names: Vec<String> = rd
                .flatten()
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();

            for name in &names {
                if name.ends_with(".qml") {
                    let fpath = dir.join(name);
                    if let Some((info, rewritten)) = scan_single_qml(&fpath, &root) {
                        scanned_files.push(fpath.clone());
                        if let Some(new_text) = rewritten {
                            intercepts.push((fpath, new_text));
                        }
                        if name.starts_with(|c: char| c.is_uppercase()) {
                            entries.push((name.clone(), info));
                        }
                    }
                }
            }

            let qmldir_intercept = if !names.iter().any(|n| n == "qmldir") {
                let mut qmldir = String::new();
                if let Ok(rel) = dir.strip_prefix(&root) {
                    let rel = rel.to_string_lossy();
                    if rel.chars().all(|c| c == '/' || c.is_ascii_alphanumeric() || c == '_') {
                        let module = if rel.is_empty() {
                            String::new()
                        } else {
                            format!(".{}", rel.replace('/', "."))
                        };
                        qmldir.push_str(&format!("module qs{module}\n"));
                    }
                }
                for (name, info) in entries {
                    if info.internal {
                        qmldir.push_str("internal ");
                    }
                    if info.singleton {
                        qmldir.push_str("singleton ");
                    }
                    qmldir.push_str(&format!("{} 1.0 {}\n", &name[..name.len() - 4], name));
                }
                Some((dir.join("qmldir"), qmldir))
            } else {
                None
            };

            (dir.clone(), scanned_files, qmldir_intercept, intercepts)
        })
        .collect();

    let mut files = BTreeSet::new();
    let mut intercepts = BTreeMap::new();
    let mut synthesized_qmldirs = 0;
    let mut rewritten_files = 0;

    for (_, f_list, qmldir_opt, rw_list) in dir_results {
        files.extend(f_list);
        if let Some((qpath, qcontent)) = qmldir_opt {
            intercepts.insert(qpath, qcontent);
            synthesized_qmldirs += 1;
        }
        for (rpath, rcontent) in rw_list {
            intercepts.insert(rpath, rcontent);
            rewritten_files += 1;
        }
    }

    let vfs_dir = match vfs_target {
        Some(p) => p.to_path_buf(),
        None => {
            let rt = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(rt).join("quickshell/by-shell/inir/vfs/qs")
        }
    };

    write_vfs(&root, &all_dirs, &intercepts, &vfs_dir)?;

    Ok(PrewarmReport {
        root_dir: root.to_string_lossy().into_owned(),
        vfs_dir: vfs_dir.to_string_lossy().into_owned(),
        directories_scanned: all_dirs.len(),
        qml_files_scanned: files.len(),
        rewritten_files,
        synthesized_qmldirs,
        elapsed_ms: start.elapsed().as_millis(),
    })
}

fn scan_single_qml(path: &Path, root: &Path) -> Option<(FileInfo, Option<String>)> {
    let text = fs::read_to_string(path).ok()?;
    let mut info = FileInfo::default();
    let mut rewritten = false;
    let mut header = Vec::new();
    let base = path.parent()?;

    for raw in text.lines() {
        let line = raw.trim();
        if line == "pragma Singleton" {
            info.singleton = true;
        } else if line == "//@ pragma Internal" {
            info.internal = true;
        } else if line.starts_with("import") {
            if let (Some(a), Some(b)) = (line.find('"'), line.rfind('"')) {
                if b > a + 1 {
                    let target = &line[a + 1..b];
                    if let Some(r) = target.strip_prefix("root:") {
                        let r = r.trim_start_matches('/');
                        let depth = base.strip_prefix(root).map(|p| p.components().count()).unwrap_or(0);
                        let rel = format!("{}{r}", "../".repeat(depth));
                        let rel = if rel.is_empty() { ".".to_string() } else { rel };
                        header.push(raw.replacen(&format!("\"{target}\""), &format!("\"{rel}\""), 1));
                        rewritten = true;
                        continue;
                    }
                }
            }
        } else if line.contains('{') {
            break;
        }
        header.push(raw.to_string());
    }

    let rewritten_text = if rewritten {
        let rest: Vec<&str> = text.lines().skip(header.len()).collect();
        let mut out = header.join("\n");
        out.push('\n');
        out.push_str(&rest.join("\n"));
        out.push('\n');
        Some(out)
    } else {
        None
    };

    Some((info, rewritten_text))
}

fn write_vfs(
    root: &Path,
    dirs: &[PathBuf],
    intercepts: &BTreeMap<PathBuf, String>,
    vfs: &Path,
) -> io::Result<()> {
    let mut real: BTreeSet<PathBuf> = BTreeSet::new();
    for d in dirs {
        if let Ok(rel) = d.strip_prefix(root) {
            let mut p = root.to_path_buf();
            real.insert(p.clone());
            for c in rel.components() {
                p.push(c);
                real.insert(p.clone());
            }
        }
    }

    let mut keep: BTreeSet<PathBuf> = BTreeSet::new();
    for dir in &real {
        let out = vfs.join(dir.strip_prefix(root).unwrap());
        if !fs::symlink_metadata(&out).is_ok_and(|m| m.is_dir()) {
            remove_any(&out);
            fs::create_dir_all(&out)?;
        }
        keep.insert(out.clone());
        for entry in fs::read_dir(dir)?.flatten() {
            let src = entry.path();
            if real.contains(&src) || intercepts.contains_key(&src) {
                continue;
            }
            let dst = out.join(entry.file_name());
            if fs::read_link(&dst).is_ok_and(|t| t == src) {
                keep.insert(dst);
                continue;
            }
            remove_any(&dst);
            std::os::unix::fs::symlink(&src, &dst)?;
            keep.insert(dst);
        }
    }

    for (path, content) in intercepts {
        if let Ok(rel) = path.strip_prefix(root) {
            let dst = vfs.join(rel);
            let same = fs::symlink_metadata(&dst).is_ok_and(|m| m.is_file())
                && fs::read(&dst).is_ok_and(|c| c == content.as_bytes());
            if !same {
                remove_any(&dst);
                fs::write(&dst, content)?;
            }
            keep.insert(dst);
        }
    }

    prune(vfs, &keep)
}

fn remove_any(p: &Path) {
    match fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => drop(fs::remove_dir_all(p)),
        Ok(_) => drop(fs::remove_file(p)),
        Err(_) => {}
    }
}

fn prune(dir: &Path, keep: &BTreeSet<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)?.flatten() {
        let p = entry.path();
        if !keep.contains(&p) {
            remove_any(&p);
        } else if entry.file_type()?.is_dir() {
            prune(&p, keep)?;
        }
    }
    Ok(())
}
