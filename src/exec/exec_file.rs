// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

use std::fs;
use std::os::unix::process::CommandExt;

use crate::app::log;
use crate::cli::Cli;
use crate::script::quote::shell_quote;
use crate::su::{build_su_cmd, SuInfo};

use super::script_mode::write_temp_script;

pub fn run(su: &SuInfo, bash: &str, script: &str, cli: &Cli, use_mount_master: bool) -> i32 {
    let path = match write_temp_script(script) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("rsudo: failed to write script: {e}");
            return 1;
        }
    };
    let path_s = path.to_string_lossy().into_owned();
    log(cli, 2, &format!("exec script: {path_s}"));

    let inner = format!(
        "{bash} --noprofile --norc {script}; __rsudo_rc=$?; rm -f -- {script}; exit $__rsudo_rc",
        bash = shell_quote(bash),
        script = shell_quote(&path_s),
    );

    let mut cmd = build_su_cmd(su, bash, &inner, use_mount_master, false);
    let err = cmd.exec();
    eprintln!("rsudo: exec failed: {err}");
    let _ = fs::remove_file(&path);
    1
}
