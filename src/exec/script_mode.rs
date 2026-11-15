// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::app::log;
use crate::cli::Cli;
use crate::env;
use crate::script::quote::shell_quote;
use crate::su::{build_su_cmd, SuInfo};

pub fn run(su: &SuInfo, bash: &str, script: &str, cli: &Cli, use_mount_master: bool) -> i32 {
    let path = match write_temp_script(script) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("rsudo: failed to write script: {e}");
            return 1;
        }
    };
    let path_s = path.to_string_lossy().into_owned();
    log(cli, 2, &format!("script-mode script: {path_s}"));

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

pub(crate) fn write_temp_script(content: &str) -> io::Result<PathBuf> {
    let dir = env::script_dir();
    fs::create_dir_all(&dir)?;
    let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = dir.join(format!("s-{pid}-{nanos}"));
    let mut f = fs::File::create(&path)?;
    f.write_all(content.as_bytes())?;
    f.flush()?;
    drop(f);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    Ok(path)
}
