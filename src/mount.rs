// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

use std::fs;
use std::process::{Command, Stdio};

use crate::cli::Cli;
use crate::env;
use crate::su::SuInfo;

pub fn partition_of(path: &str) -> Option<&'static str> {
    if path
        .strip_prefix("/system/")
        .map(|r| !r.is_empty())
        .unwrap_or(false)
    {
        return Some("/system");
    }
    None
}

pub fn decide_mount_master(cli: &Cli, root_home: &str) -> bool {
    if cli.mount_master_force {
        return true;
    }
    if cli.mount_master_disable {
        return false;
    }
    partition_of(root_home).is_some()
}

pub fn requires_su_path(cli: &Cli, root_home: &str) -> bool {
    decide_mount_master(cli, root_home)
}

pub fn detect_mount_master_support(su: &SuInfo) -> bool {
    let cache = env::cache_dir().join("mount_master");
    if let Ok(s) = fs::read_to_string(&cache) {
        return s.trim() == "1";
    }
    let ok = Command::new(&su.path)
        .args(["--mount-master", "-c", "true"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let _ = fs::create_dir_all(env::cache_dir());
    let _ = fs::write(&cache, if ok { "1" } else { "0" });
    ok
}
