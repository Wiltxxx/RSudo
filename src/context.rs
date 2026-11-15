// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

use std::path::PathBuf;

use crate::cli::Cli;
use crate::env;
use crate::home;

/// All Termux / Android environment facts gathered once at startup.
pub struct TermuxEnv {
    pub prefix: String,
    pub termux_home: String,
    pub rootfs: String,
    pub root_home: String,
    pub sdk: u32,
    pub cwd: PathBuf,
}

impl TermuxEnv {
    pub fn detect(cli: &Cli) -> Result<Self, String> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        Self::detect_with_cwd(cli, cwd)
    }

    pub fn detect_with_cwd(cli: &Cli, cwd: PathBuf) -> Result<Self, String> {
        let prefix = env::get_prefix();
        let termux_home = env::get_termux_home();
        let rootfs = env::get_rootfs();
        let sdk = env::detect_sdk();
        let root_home = home::get_root_home(cli.shell_home.as_deref(), &prefix, &termux_home)?;
        Ok(Self {
            prefix,
            termux_home,
            rootfs,
            root_home,
            sdk,
            cwd,
        })
    }

    pub fn bash(&self) -> String {
        format!("{}/bin/bash", self.prefix)
    }
}
