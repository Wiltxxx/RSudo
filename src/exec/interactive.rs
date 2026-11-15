// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

use std::os::unix::process::CommandExt;

use crate::su::{build_su_cmd, SuInfo};

pub fn run(su: &SuInfo, bash: &str, script: &str, use_mount_master: bool) -> i32 {
    let mut cmd = build_su_cmd(su, bash, script, use_mount_master, true);
    let err = cmd.exec();
    eprintln!("rsudo: exec failed: {err}");
    1
}
