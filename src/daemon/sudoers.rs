// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

//! Sudoers list: uid-level authorisation.
//!
//! Format:
//!
//! ```text
//! # comment
//! allow <uid>
//! ```
//!
//! Authorisation is per-uid, not per-command: the su prompt already
//! captured the user's intent at uid granularity.  Mirrors sudoers
//! `user ALL=(ALL) ALL`, not `Cmnd_Alias`.
//!
//! File opened O_NOFOLLOW, fstat'd for owner root / mode 0600 / regular
//! file.

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

#[derive(Debug, Default, Clone)]
pub struct Sudoers {
    allowed_uids: HashSet<u32>,
}

impl Sudoers {
    pub fn load(path: &Path) -> Result<Self, String> {
        let f = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|e| format!("open sudoers {}: {e}", path.display()))?;
        let meta = f.metadata().map_err(|e| format!("fstat sudoers: {e}"))?;
        if !meta.is_file() {
            return Err("sudoers is not a regular file".into());
        }
        if meta.uid() != 0 {
            return Err(format!(
                "sudoers owner is uid {}, expected root",
                meta.uid()
            ));
        }
        if meta.mode() & 0o777 != 0o600 {
            return Err(format!(
                "sudoers mode is {:o}, expected 600",
                meta.mode() & 0o777
            ));
        }

        let mut s = Sudoers::default();
        let reader = BufReader::new(f);
        for (lineno, line) in reader.lines().enumerate() {
            let line = line.map_err(|e| format!("read sudoers: {e}"))?;
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() != 2 || parts[0] != "allow" {
                return Err(format!(
                    "sudoers line {}: expected 'allow <uid>'",
                    lineno + 1
                ));
            }
            let uid: u32 = parts[1]
                .parse()
                .map_err(|_| format!("sudoers line {}: bad uid", lineno + 1))?;
            s.allowed_uids.insert(uid);
        }
        Ok(s)
    }

    pub fn is_allowed(&self, uid: u32) -> bool {
        self.allowed_uids.contains(&uid)
    }
}
