// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

//! Daemon lifecycle: single-instance lock, disabled flag, pid lookup,
//! and process termination.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::path::Path;

use crate::env;

/// Legacy location before the flag moved to /data/local/tmp.  Kept so an
/// existing `--daemon-disable` from an older build is still honoured.
fn legacy_disabled_path() -> &'static Path {
    Path::new("/data/adb/rsudo/disabled")
}

pub fn acquire_singleton() -> io::Result<Option<File>> {
    let path = env::daemon_lock_path();
    let f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&path)?;
    let ret = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if ret == 0 {
        Ok(Some(f))
    } else {
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
            Ok(None)
        } else {
            Err(err)
        }
    }
}

/// The disabled flag lives in /data/local/tmp so the client uid can stat
/// it without forking su.  /data/adb is root-only on Android, which made
/// the previous /data/adb/rsudo/disabled invisible to the client.
pub fn is_disabled() -> bool {
    env::daemon_disabled_path().exists() || legacy_disabled_path().exists()
}

pub fn set_disabled() -> io::Result<()> {
    let p = env::daemon_disabled_path();
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&p, b"")?;
    let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o644));
    // Remove the legacy flag so a stale /data/adb/rsudo/disabled does not
    // keep the daemon disabled after --daemon-enable.
    let _ = fs::remove_file(legacy_disabled_path());
    Ok(())
}

pub fn clear_disabled() -> io::Result<()> {
    for p in [
        env::daemon_disabled_path(),
        legacy_disabled_path().to_path_buf(),
    ] {
        match fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Find the running daemon's pid by scanning /proc.  Strict match:
/// argv[0] basename is `rsudo` and argv[1] is exactly `--daemon`.
pub fn find_pid() -> Option<u32> {
    let dir = fs::read_dir("/proc").ok()?;
    for entry in dir.flatten() {
        let pid: u32 = match entry.file_name().to_str().and_then(|s| s.parse().ok()) {
            Some(p) => p,
            None => continue,
        };
        let cmdline = match fs::read_to_string(format!("/proc/{pid}/cmdline")) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let parts: Vec<&str> = cmdline.split('\0').filter(|s| !s.is_empty()).collect();
        if parts.len() < 2 {
            continue;
        }
        let argv0_basename = Path::new(parts[0]).file_name().and_then(|s| s.to_str());
        if argv0_basename != Some("rsudo") {
            continue;
        }
        if parts[1] != "--daemon" {
            continue;
        }
        return Some(pid);
    }
    None
}

pub fn kill_daemon() -> bool {
    let pid = match find_pid() {
        Some(p) => p,
        None => return false,
    };
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
    true
}
