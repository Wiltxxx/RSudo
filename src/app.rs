// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

use crate::cli::{Cli, Kind};
use crate::config::Config;
use crate::context::TermuxEnv;
use crate::daemon;
use crate::decisions;
use crate::env;
use crate::exec;
use crate::mount;
use crate::path;
use crate::run_command;
use crate::script;
use crate::su::{self, SuInfo};

pub(crate) fn log(cli: &Cli, level: u8, msg: &str) {
    if cli.verbose >= level {
        eprintln!("[rsudo] {msg}");
    }
}

pub fn run(cli_in: &Cli) -> i32 {
    let mut cli_owned = cli_in.clone();
    let config = Config::load();
    config.apply(&mut cli_owned);
    let cli = &cli_owned;

    if cli.clear_cache {
        env::clear_cache();
        log(cli, 1, "cache cleared");
    }

    let t_env = match TermuxEnv::detect(cli) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("rsudo: {e}");
            return 1;
        }
    };
    log(
        cli,
        2,
        &format!("sdk={} prefix={}", t_env.sdk, t_env.prefix),
    );
    log(cli, 1, &format!("root_home={}", t_env.root_home));

    let (kind, raw_args) = decisions::decide_kind(cli);
    log(cli, 2, &format!("kind={kind:?} args={raw_args:?}"));

    let processed = run_command::process(raw_args);
    let args: &[String] = &processed;

    if matches!(kind, Kind::Su | Kind::Asu) && !args.is_empty() {
        eprintln!(
            "rsudo: warning: {} ignores extra arguments: {:?}",
            match kind {
                Kind::Su => "su",
                Kind::Asu => "asu",
                _ => unreachable!(),
            },
            args
        );
    }

    if decisions::decide_interactive(cli, kind, args) {
        let su = match su::detect() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("rsudo: {e}");
                return 1;
            }
        };
        run_interactive(cli, kind, &t_env, &su)
    } else {
        run_non_interactive(cli, kind, args, &t_env)
    }
}

fn run_interactive(cli: &Cli, kind: Kind, t_env: &TermuxEnv, su: &SuInfo) -> i32 {
    let priority = decisions::decide_interactive_priority(cli, kind);
    let layout = decisions::decide_layout(cli, kind, true);
    log(
        cli,
        2,
        &format!("interactive priority={priority:?} layout={layout:?}"),
    );

    let shell = match path::resolve_shell(cli.shell.as_deref(), &t_env.prefix) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("rsudo: {e}");
            return 1;
        }
    };
    log(cli, 1, &format!("shell={shell}"));

    let use_mm = mount::decide_mount_master(cli, &t_env.root_home);
    log(cli, 1, &format!("mount_master={use_mm}"));

    let script = script::build_interactive_script(&shell, layout, t_env, cli);
    if cli.dry_run {
        print!("{script}");
        return 0;
    }
    log(cli, 3, &format!("--- script ---\n{script}--- end ---"));
    exec::interactive::run(su, &t_env.bash(), &script, use_mm)
}

fn run_non_interactive(cli: &Cli, kind: Kind, args: &[String], t_env: &TermuxEnv) -> i32 {
    if !cli.force_path && cli.interactive {
        eprintln!("rsudo: warning: '-i' ignored because a command was given");
    }
    if !cli.force_path && cli.shell.is_some() && kind != Kind::Script {
        eprintln!("rsudo: warning: '--shell' ignored because a command was given");
    }

    if args.is_empty() && kind != Kind::Script {
        print_usage();
        return 1;
    }

    if should_use_daemon(cli, kind, t_env) {
        log(cli, 1, "attempting daemon path");
        if let Some(rc) = daemon::client::run_lazy_su(cli, kind, args, t_env) {
            return rc;
        }
        log(cli, 1, "daemon unavailable, falling back to su path");
    }

    let su = match su::detect() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("rsudo: {e}");
            return 1;
        }
    };
    log(
        cli,
        1,
        &format!(
            "su={} interactive={} shell={} preserve={} mm={}",
            su.path,
            su.supports_interactive,
            su.supports_shell,
            su.supports_preserve_env,
            su.supports_mount_master
        ),
    );

    let use_mm = mount::decide_mount_master(cli, &t_env.root_home);

    match kind {
        Kind::Script => run_script_mode(cli, args, t_env, &su, use_mm),
        Kind::Exec | Kind::Su | Kind::Asu => run_exec_mode(cli, kind, args, t_env, &su, use_mm),
    }
}

fn run_exec_mode(
    cli: &Cli,
    kind: Kind,
    args: &[String],
    t_env: &TermuxEnv,
    su: &SuInfo,
    use_mm: bool,
) -> i32 {
    let (search_mode, _forced_priority) = decisions::decide_search_mode(cli);
    let resolved = match path::resolve_command(&args[0], &t_env.prefix, t_env.sdk, search_mode) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("rsudo: {e}");
            return 1;
        }
    };
    let under_rootfs = path::path_starts_with(&resolved, &t_env.rootfs);
    let layout = decisions::decide_layout(cli, kind, under_rootfs);
    log(
        cli,
        1,
        &format!(
            "exec: {} -> {} (layout={layout:?})",
            args[0],
            resolved.display()
        ),
    );

    let script = script::build_exec_script(&resolved, &args[1..], layout, t_env, cli);
    if cli.dry_run {
        print!("{script}");
        return 0;
    }
    log(cli, 3, &format!("--- script ---\n{script}--- end ---"));
    exec::exec_file::run(su, &t_env.bash(), &script, cli, use_mm)
}

fn run_script_mode(
    cli: &Cli,
    args: &[String],
    t_env: &TermuxEnv,
    su: &SuInfo,
    use_mm: bool,
) -> i32 {
    if args.is_empty() {
        eprintln!("rsudo: -s requires a script argument");
        return 1;
    }
    let script_text = &args[0];
    let extra = &args[1..];

    let shell = match script::script_mode::script_shell(cli, t_env) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("rsudo: {e}");
            return 1;
        }
    };
    let layout = decisions::decide_layout(cli, Kind::Script, true);

    let script = script::script_mode::build_script_mode_script(
        script_text,
        extra,
        layout,
        t_env,
        cli,
        &shell,
    );
    if cli.dry_run {
        print!("{script}");
        return 0;
    }
    log(cli, 3, &format!("--- script-mode ---\n{script}--- end ---"));
    exec::script_mode::run(su, &t_env.bash(), &script, cli, use_mm)
}

/// Decide whether this invocation may use the daemon.
///
/// `--shell-home` is *not* an exclusion: daemon's `compute_daemon_env`
/// sets HOME to `t_env.root_home` (same value as the su path), and
/// `ensure_home` creates the directory if missing.  A shell-home under
/// `/system/...` is caught by `mount::requires_su_path` below and
/// downgraded to su; a shell-home under `/data/...` runs through the
/// daemon normally.
fn should_use_daemon(cli: &Cli, kind: Kind, t_env: &TermuxEnv) -> bool {
    if daemon::lifecycle::is_disabled() {
        return false;
    }
    // --dry-run prints the generated script; the daemon has no script to
    // print (it execs argv directly), so dry-run must go through su.
    if cli.dry_run {
        return false;
    }
    if matches!(kind, Kind::Su | Kind::Asu) {
        return false;
    }
    if cli.interactive || cli.shell.is_some() {
        return false;
    }
    if mount::requires_su_path(cli, &t_env.root_home) {
        return false;
    }
    true
}

fn print_usage() {
    eprintln!("rsudo: no command given");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  rsudo [OPTIONS] <command> [args...]      run <command> as root");
    eprintln!(
        "  rsudo [OPTIONS] su                       interactive root shell (termux priority)"
    );
    eprintln!(
        "  rsudo [OPTIONS] asu                      interactive root shell (android priority)"
    );
    eprintln!("  rsudo [OPTIONS] -i [--shell SHELL] ...   interactive root shell (default: bash)");
    eprintln!("  rsudo [OPTIONS] -s <script> [args...]    run <script> as root (script mode)");
    eprintln!(
        "  rsudo [OPTIONS] --dry-run [COMMAND]...   print generated script instead of running"
    );
    eprintln!();
    eprintln!("Daemon control:");
    eprintln!("  rsudo --daemon-boot      install boot script and start daemon");
    eprintln!("  rsudo --daemon-disable   disable daemon (falls back to su)");
    eprintln!("  rsudo --daemon-enable    re-enable daemon");
    eprintln!("  rsudo --daemon-restart   restart daemon (reloads sudoers)");
    eprintln!("  rsudo --daemon-status    show daemon state");
    eprintln!();
    eprintln!("Sudoers management:");
    eprintln!("  rsudo --sudoers-add <UID>   add a uid to the list");
    eprintln!("  rsudo --sudoers-del <UID>   remove a uid from the list");
    eprintln!("  rsudo --sudoers-list        print the current list");
    eprintln!("  rsudo --audit [N]           show last N audit entries (default 20)");
    eprintln!();
    eprintln!("Run 'rsudo --help' for the full option list.");
}
