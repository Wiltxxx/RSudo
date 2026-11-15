// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

use std::fs;
use std::io::IsTerminal;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::env;
use crate::mount;

#[derive(Debug, Clone, Default)]
pub struct SuInfo {
    pub path: String,
    pub supports_interactive: bool,
    pub supports_shell: bool,
    pub supports_preserve_env: bool,
    pub supports_mount_master: bool,
}

fn flags_str(info: &SuInfo) -> String {
    let mut f = String::new();
    if info.supports_interactive {
        f.push('I');
    }
    if info.supports_shell {
        f.push('S');
    }
    if info.supports_preserve_env {
        f.push('P');
    }
    if info.supports_mount_master {
        f.push('M');
    }
    f
}

fn parse_flags(path: &str, flags: &str) -> SuInfo {
    SuInfo {
        path: path.to_string(),
        supports_interactive: flags.contains('I'),
        supports_shell: flags.contains('S'),
        supports_preserve_env: flags.contains('P'),
        supports_mount_master: flags.contains('M'),
    }
}

pub fn detect() -> Result<SuInfo, String> {
    let cache_path = env::cache_dir().join("su_info_v2");
    if let Ok(content) = fs::read_to_string(&cache_path) {
        // Format: `path|flags|mtime_secs`.  Legacy `path|flags` entries
        // have no mtime; treat them as invalid so a Magisk upgrade (which
        // can change supported flags without changing the path) is picked
        // up on the next call.
        let parts: Vec<&str> = content.trim().split('|').collect();
        if parts.len() >= 3 {
            let p = parts[0];
            let f = parts[1];
            let cached_mtime: Option<i64> = parts[2].parse().ok();
            if let Ok(meta) = fs::metadata(p) {
                if meta.is_file() && cached_mtime == Some(meta.mtime()) {
                    return Ok(parse_flags(p, f));
                }
            }
        }
    }

    let path = find_su_path()?;

    let mut info = SuInfo {
        path: path.clone(),
        supports_shell: true,
        supports_preserve_env: true,
        ..Default::default()
    };

    let help = Command::new(&path)
        .arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map(|o| {
            let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
            s.push_str(&String::from_utf8_lossy(&o.stderr));
            s
        })
        .unwrap_or_default();
    info.supports_interactive = help.contains("--interactive");
    info.supports_shell = help.contains("--shell") || help.contains("-c");
    info.supports_preserve_env = help.contains("--preserve-environment")
        || help.contains("-p ")
        || help.contains("--preserve");
    info.supports_mount_master = help.contains("--mount-master");
    if !info.supports_mount_master {
        info.supports_mount_master = mount::detect_mount_master_support(&info);
    }

    let mtime = fs::metadata(&info.path).map(|m| m.mtime()).unwrap_or(0);
    let _ = fs::create_dir_all(env::cache_dir());
    let _ = fs::write(
        &cache_path,
        format!("{}|{}|{}", info.path, flags_str(&info), mtime),
    );

    Ok(info)
}

fn find_su_path() -> Result<String, String> {
    for c in [
        "/debug_ramdisk/su",
        "/system/bin/su",
        "/system/xbin/su",
        "/sbin/su",
        "/sbin/bin/su",
        "/system/sbin/su",
        "/su/xbin/su",
        "/su/bin/su",
        "/magisk/.core/bin/su",
    ] {
        if Path::new(c).is_file() {
            return Ok(c.to_string());
        }
    }
    Err("no su binary found (searched 9 standard paths)".into())
}

fn is_foreground() -> bool {
    if !std::io::stdin().is_terminal() {
        return false;
    }
    unsafe {
        let pgrp = libc::getpgrp();
        let tcpgrp = libc::tcgetpgrp(libc::STDIN_FILENO);
        tcpgrp != -1 && tcpgrp == pgrp
    }
}

pub fn build_su_cmd(
    su: &SuInfo,
    bash: &str,
    script: &str,
    use_mount_master: bool,
    want_pty: bool,
) -> Command {
    let mut cmd = Command::new(&su.path);
    if use_mount_master && su.supports_mount_master {
        cmd.arg("--mount-master");
    }
    if want_pty && su.supports_interactive && is_foreground() {
        cmd.arg("--interactive");
    }
    if su.supports_preserve_env {
        cmd.arg("--preserve-environment");
    }
    if su.supports_shell {
        cmd.arg("--shell").arg(bash);
    }
    cmd.arg("-c").arg(script);
    cmd
}
