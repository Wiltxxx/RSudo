// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

// src/script/interactive.rs
use std::path::Path;

use crate::cli::{Cli, PathLayout};
use crate::context::TermuxEnv;

use super::env_block::build_env_block;
use super::quote::shell_quote;

#[derive(Clone, Copy, PartialEq)]
enum ShellKind {
    Bash,
    Zsh,
    Fish,
    Posix,
    Sh,
    Unknown,
}

fn shell_kind_from_bn(bn: &str) -> ShellKind {
    match bn {
        "bash" => ShellKind::Bash,
        "zsh" => ShellKind::Zsh,
        "fish" => ShellKind::Fish,
        "dash" | "ash" | "mksh" | "pdksh" | "oksh" | "lksh" | "loksh" | "ksh" | "ksh93"
        | "ksh2020" => ShellKind::Posix,
        "sh" => ShellKind::Sh,
        _ => ShellKind::Unknown,
    }
}

fn shell_kind_name(k: ShellKind) -> &'static str {
    match k {
        ShellKind::Bash => "bash",
        ShellKind::Zsh => "zsh",
        ShellKind::Fish => "fish",
        ShellKind::Posix => "posix",
        ShellKind::Sh => "sh",
        ShellKind::Unknown => "unknown",
    }
}

fn bash_rc_hint(env: &TermuxEnv) -> String {
    let user_rc = format!("{}/.bashrc", env.root_home);
    if Path::new(&user_rc).is_file() {
        format!("--rcfile {}", shell_quote(&user_rc))
    } else {
        "--norc".into()
    }
}

fn emit_zsh_rcfile(env: &TermuxEnv) -> String {
    let zdot = format!("{}/.config/rsudo/zsh", env.root_home);
    let zshrc = format!("{zdot}/.zshrc");
    format!(
        r#"__rsudo_zdot={zdot}
__rsudo_zshrc={zshrc}
if [ ! -d "$__rsudo_zdot" ]; then
  mkdir -p "$__rsudo_zdot" && chown root:root "$__rsudo_zdot" && chmod 700 "$__rsudo_zdot"
fi
if [ ! -f "$__rsudo_zshrc" ]; then
  : > "$__rsudo_zshrc" && chown root:root "$__rsudo_zshrc" && chmod 600 "$__rsudo_zshrc"
fi
export ZDOTDIR="$__rsudo_zdot"
unset __rsudo_zdot __rsudo_zshrc
"#,
        zdot = shell_quote(&zdot),
        zshrc = shell_quote(&zshrc),
    )
}

fn emit_fish_rcfile(env: &TermuxEnv) -> String {
    let base_cfg = format!("{}/.config/rsudo", env.root_home);
    let cfg_dir = format!("{base_cfg}/fish");
    let cfg_file = format!("{cfg_dir}/config.fish");
    let base_data = format!("{}/.local/share/rsudo", env.root_home);
    let data_dir = format!("{base_data}/fish");
    let base_cache = format!("{}/.cache/rsudo", env.root_home);
    let cache_dir = format!("{base_cache}/fish");
    let runtime = format!("{base_cache}/run");
    format!(
        r#"__rsudo_fish_base_cfg={base_cfg}
__rsudo_fish_base_data={base_data}
__rsudo_fish_base_cache={base_cache}
__rsudo_fish_cfg={cfg_dir}
__rsudo_fish_data={data_dir}
__rsudo_fish_cache={cache_dir}
__rsudo_fish_runtime={runtime}
for __rsudo_d in "$__rsudo_fish_cfg" "$__rsudo_fish_data" "$__rsudo_fish_cache" "$__rsudo_fish_runtime"; do
  if [ ! -d "$__rsudo_d" ]; then
    mkdir -p "$__rsudo_d" && chown root:root "$__rsudo_d" && chmod 700 "$__rsudo_d"
  fi
done
__rsudo_cfg_file={cfg_file}
if [ ! -f "$__rsudo_cfg_file" ]; then
  cat > "$__rsudo_cfg_file" <<'RSUDO_FISH_EOF'
function export
    if [ $argv ]
        set var (echo $argv | cut -f1 -d=)
        set val (echo $argv | cut -f2 -d=)
        set -g -x $var $val
    end
end
function unset
    set --erase $argv
end
function tpath
    set -gx PATH $SUDO__TERMUX_PRIORITY_PATH
    set -gx LD_LIBRARY_PATH $SUDO__TERMUX_PRIORITY_LD_LIBRARY_PATH
    set -gx LD_PRELOAD $SUDO__TERMUX_LD_PRELOAD
end
function apath
    set -gx PATH $SUDO__ANDROID_PRIORITY_PATH
    set -gx LD_LIBRARY_PATH $SUDO__ANDROID_PRIORITY_LD_LIBRARY_PATH
    set -e LD_PRELOAD
end
function fish_prompt
    set_color --bold red
    printf '[%s]' (prompt_pwd)
    set_color normal
    printf '# '
end
RSUDO_FISH_EOF
  chown root:root "$__rsudo_cfg_file" && chmod 600 "$__rsudo_cfg_file"
fi
export XDG_CONFIG_HOME="$__rsudo_fish_base_cfg"
export XDG_DATA_HOME="$__rsudo_fish_base_data"
export XDG_CACHE_HOME="$__rsudo_fish_base_cache"
export XDG_RUNTIME_DIR="$__rsudo_fish_runtime"
unset __rsudo_fish_base_cfg __rsudo_fish_base_data __rsudo_fish_base_cache \
      __rsudo_fish_cfg __rsudo_fish_data __rsudo_fish_cache \
      __rsudo_fish_runtime __rsudo_cfg_file __rsudo_d
"#,
        base_cfg = shell_quote(&base_cfg),
        base_data = shell_quote(&base_data),
        base_cache = shell_quote(&base_cache),
        cfg_dir = shell_quote(&cfg_dir),
        data_dir = shell_quote(&data_dir),
        cache_dir = shell_quote(&cache_dir),
        runtime = shell_quote(&runtime),
        cfg_file = shell_quote(&cfg_file),
    )
}

fn posix_ps1_rhs() -> &'static str {
    r#"$'\001\033[1;31m\002[''$PWD'$']\001\033[0m\002# '"#
}

pub fn build_interactive_script(
    shell: &str,
    layout: PathLayout,
    env: &TermuxEnv,
    cli: &Cli,
) -> String {
    let mut s = String::new();
    s.push_str(&build_env_block(layout, env, cli));

    let prio_paths = crate::script::env_block::compute_env_paths(layout, &env.prefix, env.sdk, cli);
    s.push_str(&format!(
        "export SUDO__TERMUX_PRIORITY_PATH={}\n",
        shell_quote(&prio_paths.0)
    ));
    s.push_str(&format!(
        "export SUDO__ANDROID_PRIORITY_PATH={}\n",
        shell_quote(&prio_paths.1)
    ));
    s.push_str(&format!(
        "export SUDO__TERMUX_PRIORITY_LD_LIBRARY_PATH={}\n",
        shell_quote(&prio_paths.2)
    ));
    s.push_str(&format!(
        "export SUDO__ANDROID_PRIORITY_LD_LIBRARY_PATH={}\n",
        shell_quote(&prio_paths.3)
    ));
    let tpre = format!("{}/lib/libtermux-exec.so", env.prefix);
    if Path::new(&tpre).is_file() {
        s.push_str(&format!(
            "export SUDO__TERMUX_LD_PRELOAD={}\n",
            shell_quote(&tpre)
        ));
    } else {
        s.push_str("unset SUDO__TERMUX_LD_PRELOAD\n");
    }

    let bn = Path::new(shell)
        .file_name()
        .and_then(|x| x.to_str())
        .unwrap_or("");

    let initial_kind = shell_kind_from_bn(bn);

    s.push_str(&format!(
        "__rsudo_shell_kind={}\n",
        shell_quote(shell_kind_name(initial_kind))
    ));

    if initial_kind == ShellKind::Sh {
        s.push_str(&format!(
            r#"__rsudo_shell={shell}
if "$__rsudo_shell" -c 'functions -q fish_prompt' 2>/dev/null; then
  __rsudo_shell_kind=fish
elif [ -n "$("$__rsudo_shell" -c 'echo $BASH_VERSION' 2>/dev/null)" ]; then
  __rsudo_shell_kind=bash
elif [ -n "$("$__rsudo_shell" -c 'echo $ZSH_VERSION' 2>/dev/null)" ]; then
  __rsudo_shell_kind=zsh
else
  __rsudo_shell_kind=posix
fi
unset __rsudo_shell
"#,
            shell = shell_quote(shell)
        ));
    }

    let bash_ps1 = shell_quote(r"\[\e[1;31m\][\w]\[\e[0m\]# ");
    let zsh_ps1 = shell_quote("%B%F{red}[%~]%f%b# ");
    let posix_ps1 = posix_ps1_rhs();
    let fallback_ps1 = shell_quote(&format!("[{}] [{bn}] # ", r"${PWD}"));

    s.push_str(&format!(
        r#"if [ "$__rsudo_shell_kind" != fish ]; then
  if [ -n "${{RSUDO_PS1:-}}" ]; then
    PS1=$RSUDO_PS1
  else
    case "$__rsudo_shell_kind" in
      bash) PS1={bash_ps1} ;;
      zsh)  PS1={zsh_ps1} ;;
      posix) PS1={posix_ps1} ;;
      *)     PS1={fallback_ps1} ;;
    esac
  fi
  export PS1
fi
"#,
        bash_ps1 = bash_ps1,
        zsh_ps1 = zsh_ps1,
        posix_ps1 = posix_ps1,
        fallback_ps1 = fallback_ps1,
    ));

    s.push_str(&format!(
        "\ncd -- {} || exit 1\n\n",
        shell_quote(&env.cwd.to_string_lossy())
    ));

    let bash_launch = format!(
        "{} --noprofile {} -i",
        shell_quote(shell),
        bash_rc_hint(env)
    );
    let zsh_launch = format!(
        "{}\n{} --no-rcs -i",
        emit_zsh_rcfile(env),
        shell_quote(shell)
    );
    let fish_launch = format!(
        "{}\n{} --login -i",
        emit_fish_rcfile(env),
        shell_quote(shell)
    );
    let default_launch = format!("{} -i", shell_quote(shell));

    s.push_str(&format!(
        r#"case "$__rsudo_shell_kind" in
  bash) {bash_launch} ;;
  zsh)  {zsh_launch} ;;
  fish) {fish_launch} ;;
  *)    {default_launch} ;;
esac
"#,
        bash_launch = bash_launch,
        zsh_launch = zsh_launch,
        fish_launch = fish_launch,
        default_launch = default_launch,
    ));

    s
}
