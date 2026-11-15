// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub fn get_prefix() -> String {
    env::var("TERMUX__PREFIX")
        .or_else(|_| env::var("PREFIX"))
        .unwrap_or_else(|_| "/data/data/com.termux/files/usr".into())
}

pub fn get_termux_home() -> String {
    env::var("TERMUX__HOME")
        .or_else(|_| env::var("HOME"))
        .unwrap_or_else(|_| "/data/data/com.termux/files/home".into())
}

pub fn get_rootfs() -> String {
    env::var("TERMUX__ROOTFS_DIR").unwrap_or_else(|_| "/data/data/com.termux/files".into())
}

pub fn cache_dir() -> PathBuf {
    PathBuf::from(get_prefix()).join("var/cache/rsudo")
}

pub fn script_dir() -> PathBuf {
    PathBuf::from(get_prefix()).join("var/run/rsudo")
}

pub fn clear_cache() {
    let _ = fs::remove_dir_all(cache_dir());
}

pub fn detect_sdk() -> u32 {
    if let Ok(s) = env::var("ANDROID__BUILD_VERSION_SDK") {
        if let Ok(n) = s.trim().parse::<u32>() {
            return n;
        }
    }
    let cache = cache_dir().join("sdk");
    if let Ok(s) = fs::read_to_string(&cache) {
        if let Ok(n) = s.trim().parse::<u32>() {
            return n;
        }
    }
    let n = Command::new("getprop")
        .arg("ro.build.version.sdk")
        .stdin(Stdio::null())
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(28);
    let _ = fs::create_dir_all(cache_dir());
    let _ = fs::write(&cache, n.to_string());
    n
}

pub fn android_paths(sdk: u32) -> &'static str {
    match sdk {
        0..=23 => "/sbin:/vendor/bin:/system/sbin:/system/bin:/system/xbin",
        24..=27 => "/sbin:/system/sbin:/system/bin:/system/xbin:/vendor/bin:/vendor/xbin",
        28 => "/sbin:/system/sbin:/system/bin:/system/xbin:/odm/bin:/vendor/bin:/vendor/xbin",
        29 => "/sbin:/system/sbin:/product/bin:/apex/com.android.runtime/bin:/system/bin:/system/xbin:/odm/bin:/vendor/bin:/vendor/xbin",
        _ => "/product/bin:/apex/com.android.runtime/bin:/apex/com.android.art/bin:/system_ext/bin:/system/bin:/system/xbin:/odm/bin:/vendor/bin:/vendor/xbin",
    }
}

pub fn daemon_socket_path() -> PathBuf {
    PathBuf::from("/data/local/tmp/rsudo.sock")
}

pub fn daemon_lock_path() -> PathBuf {
    PathBuf::from("/data/local/tmp/rsudo.lock")
}

pub fn daemon_log_path() -> PathBuf {
    PathBuf::from("/data/local/tmp/rsudo-daemon.log")
}

pub fn daemon_disabled_path() -> PathBuf {
    PathBuf::from("/data/local/tmp/rsudo.disabled")
}

pub fn default_sudoers_path() -> PathBuf {
    PathBuf::from("/data/adb/rsudo/sudoers.conf")
}

pub fn audit_log_path() -> PathBuf {
    PathBuf::from("/data/adb/rsudo/audit.log")
}
