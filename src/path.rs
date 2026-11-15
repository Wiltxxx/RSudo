// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::cli::SearchMode;
use crate::env;

pub fn path_starts_with(path: &Path, prefix: &str) -> bool {
    path.starts_with(Path::new(prefix))
}

pub fn is_executable(p: &Path) -> bool {
    fs::metadata(p)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

pub fn resolve_command(
    cmd: &str,
    prefix: &str,
    sdk: u32,
    mode: SearchMode,
) -> Result<PathBuf, String> {
    let p = Path::new(cmd);
    if p.is_absolute() {
        if is_executable(p) {
            return Ok(p.to_path_buf());
        }
        return Err(format!("not executable: {cmd}"));
    }
    if cmd.starts_with("./") || cmd.starts_with("../") {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        let full = cwd.join(cmd);
        if is_executable(&full) {
            return Ok(full);
        }
        return Err(format!("not executable: {cmd}"));
    }
    let tbin = format!("{prefix}/bin");
    let tapp = format!("{prefix}/bin/applets");
    let android: Vec<String> = env::android_paths(sdk)
        .split(':')
        .map(String::from)
        .collect();
    let dirs: Vec<String> = match mode {
        SearchMode::TermuxFirst => {
            let mut v = vec![tbin, tapp];
            v.extend(android);
            v
        }
        SearchMode::AndroidFirst => {
            let mut v = android;
            v.push(tbin);
            v.push(tapp);
            v
        }
        SearchMode::AndroidOnly => vec!["/system/bin".into()],
    };
    for d in &dirs {
        let c = Path::new(d).join(cmd);
        if is_executable(&c) {
            return Ok(c);
        }
    }
    Err(format!("command not found: {cmd}"))
}

pub fn resolve_shell(spec: Option<&str>, prefix: &str) -> Result<String, String> {
    let name = spec.unwrap_or("bash");
    if name.contains('/') {
        let p = PathBuf::from(name);
        if is_executable(&p) {
            return Ok(p.to_string_lossy().into_owned());
        }
        return Err(format!("shell not executable: {name}"));
    }
    let p = format!("{prefix}/bin/{name}");
    if is_executable(Path::new(&p)) {
        return Ok(p);
    }
    Err(format!("shell not found: {name}"))
}
