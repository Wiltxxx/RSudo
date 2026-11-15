// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

use std::path::Path;

use crate::cli::{Cli, PathLayout};
use crate::context::TermuxEnv;

use super::env_block::build_env_block;
use super::quote::shell_quote;

const HEREDOC_WORD: &str = "RSUDO_INLINE_EOF";

pub fn script_shell(cli: &Cli, env: &TermuxEnv) -> Result<String, String> {
    let name = cli.shell.as_deref().unwrap_or("bash");
    if name.contains('/') {
        if Path::new(name).is_file() {
            Ok(name.to_string())
        } else {
            Err(format!("script shell not found: {name}"))
        }
    } else {
        let p = format!("{}/bin/{name}", env.prefix);
        if Path::new(&p).is_file() {
            Ok(p)
        } else {
            Err(format!("script shell not found: {name}"))
        }
    }
}

enum Strategy {
    /// Write the script to a temp file inside `$HOME/.rsudo.tmp` and run
    /// it.  Only used when the user's script happens to contain the
    /// inline heredoc terminator, which would break the heredoc form.
    TempFile,
    Heredoc,
}

pub fn build_script_mode_script(
    script_arg: &str,
    extra_args: &[String],
    layout: PathLayout,
    env: &TermuxEnv,
    cli: &Cli,
    shell: &str,
) -> String {
    let mut s = String::new();
    s.push_str("trap 'exit 130' INT\n");
    s.push_str("trap 'exit 143' TERM\n");
    s.push_str(&build_env_block(layout, env, cli));
    s.push_str(&format!(
        "\ncd -- {} || exit 1\n\n",
        shell_quote(&env.cwd.to_string_lossy())
    ));

    let strategy = if script_arg.contains(HEREDOC_WORD) {
        Strategy::TempFile
    } else {
        Strategy::Heredoc
    };

    let mut tail = String::new();
    for a in extra_args {
        tail.push(' ');
        tail.push_str(&shell_quote(a));
    }

    match strategy {
        Strategy::TempFile => {
            let tmp_dir = format!("{}/.rsudo.tmp", env.root_home);
            let tmp_file = format!("{tmp_dir}/rsudo_script_{}", std::process::id());
            s.push_str(&format!(
                "mkdir -p {} || {{ echo 'rsudo: cannot create temp dir' >&2; exit 1; }}\n",
                shell_quote(&tmp_dir)
            ));
            s.push_str(&format!(
                "cat > {} <<'RSUDO_FILE_EOF'\n{}\nRSUDO_FILE_EOF\n",
                shell_quote(&tmp_file),
                script_arg
            ));
            s.push_str(&format!("chmod 700 {}\n", shell_quote(&tmp_file)));
            s.push_str(&format!(
                "{} {}",
                shell_quote(shell),
                shell_quote(&tmp_file)
            ));
            s.push_str(&tail);
            s.push('\n');
            s.push_str("__rsudo_rc=$?\n");
            s.push_str(&format!("rm -f {}\n", shell_quote(&tmp_file)));
            s.push_str("exit $__rsudo_rc\n");
        }
        Strategy::Heredoc => {
            if tail.is_empty() {
                s.push_str(&format!(
                    "{} -s <<'{w}'\n{body}\n{w}\n",
                    shell_quote(shell),
                    w = HEREDOC_WORD,
                    body = script_arg
                ));
            } else {
                s.push_str(&format!(
                    "{} -s --{} <<'{w}'\n{body}\n{w}\n",
                    shell_quote(shell),
                    tail,
                    w = HEREDOC_WORD,
                    body = script_arg
                ));
            }
        }
    }

    s
}
