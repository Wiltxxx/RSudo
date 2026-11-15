// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

//! Pre-process the command vector: expand `/proc/self/fd/N` (N >= 3) by
//! reading the fd.  Mirrors sudo's default behaviour.

use std::fs;

pub fn process(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    for a in args {
        let s = if let Some(fd_str) = a.strip_prefix("/proc/self/fd/") {
            if let Ok(n) = fd_str.parse::<u32>() {
                if n >= 3 {
                    fs::read_to_string(format!("/proc/self/fd/{n}")).unwrap_or_else(|_| a.clone())
                } else {
                    a.clone()
                }
            } else {
                a.clone()
            }
        } else {
            a.clone()
        };
        out.push(s);
    }
    out
}
