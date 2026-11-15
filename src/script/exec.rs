// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

use std::path::Path;

use crate::cli::{Cli, PathLayout};
use crate::context::TermuxEnv;

use super::env_block::build_env_block;
use super::quote::shell_quote;

pub fn build_exec_script(
    cmd: &Path,
    args: &[String],
    layout: PathLayout,
    env: &TermuxEnv,
    cli: &Cli,
) -> String {
    let mut s = String::new();
    // Trap SIGINT/SIGTERM before anything else.  With `--interactive`,
    // magisk su does not propagate a child that was killed by a signal;
    // it exits 0.  An explicit `exit 130` from our own trap bypasses
    // that and makes `if rsudo sleep 10; then ...` behave correctly.
    s.push_str("trap 'exit 130' INT\n");
    s.push_str("trap 'exit 143' TERM\n");
    s.push_str(&build_env_block(layout, env, cli));
    s.push_str(&format!(
        "\ncd -- {} || exit 1\n\n",
        shell_quote(&env.cwd.to_string_lossy())
    ));
    s.push_str(&shell_quote(&cmd.to_string_lossy()));
    for a in args {
        s.push(' ');
        s.push_str(&shell_quote(a));
    }
    s.push('\n');
    s
}
