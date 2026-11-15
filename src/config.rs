// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

use std::collections::HashMap;
use std::fs;

use crate::cli::Cli;
use crate::env;

#[derive(Debug, Default, Clone)]
pub struct Config {
    values: HashMap<String, String>,
}

impl Config {
    pub fn load() -> Self {
        let path = format!("{}/etc/rsudo.conf", env::get_prefix());
        let mut values = HashMap::new();
        if let Ok(content) = fs::read_to_string(&path) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    values.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
        }
        Self { values }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(|s| s.as_str())
    }

    fn get_any(&self, keys: &[&str]) -> Option<&str> {
        keys.iter().find_map(|k| self.get(k))
    }

    pub fn apply(&self, cli: &mut Cli) {
        if cli.shell_home.is_none() {
            if let Some(v) = self.get_any(&["SUDO_SHELL_HOME", "SUDO__SHELL_HOME"]) {
                cli.shell_home = Some(v.to_string());
            }
        }
        if cli.shell.is_none() {
            if let Some(v) = self.get_any(&["SUDO_SHELL", "SUDO__SHELL"]) {
                cli.shell = Some(v.to_string());
            }
        }
        if cli.export_paths.is_none() {
            if let Some(v) = self.get_any(&[
                "SUDO_ADDITIONAL_PATHS_TO_EXPORT",
                "SUDO__ADDITIONAL_PATHS_TO_EXPORT",
            ]) {
                cli.export_paths = Some(v.to_string());
            }
        }
        if cli.export_ld_lib_paths.is_none() {
            if let Some(v) = self.get_any(&[
                "SUDO_ADDITIONAL_LD_LIBRARY_PATHS_TO_EXPORT",
                "SUDO__ADDITIONAL_LD_LIBRARY_PATHS_TO_EXPORT",
            ]) {
                cli.export_ld_lib_paths = Some(v.to_string());
            }
        }
        if cli.verbose == 0 {
            if let Some(v) = self.get_any(&["SUDO_LOG_LEVEL", "SUDO__LOG_LEVEL"]) {
                if let Ok(n) = v.parse::<u8>() {
                    cli.verbose = n;
                }
            }
        }
    }
}
