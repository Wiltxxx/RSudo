// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

mod app;
mod cli;
mod config;
mod context;
mod daemon;
mod decisions;
mod env;
mod exec;
mod home;
mod mount;
mod path;
mod pty_config;
mod run_command;
mod script;
mod su;

use clap::Parser;

fn main() {
    let cli = cli::Cli::parse();

    if cli.daemon_mode {
        std::process::exit(daemon::server::run(&cli));
    }
    if cli.edit_pty {
        std::process::exit(pty_config::edit_repl());
    }
    if cli.daemon_boot
        || cli.daemon_enable
        || cli.daemon_disable
        || cli.daemon_restart
        || cli.daemon_status
        || cli.sudoers_add.is_some()
        || cli.sudoers_del.is_some()
        || cli.sudoers_list
        || cli.audit.is_some()
    {
        std::process::exit(daemon::control::run(&cli));
    }

    std::process::exit(app::run(&cli));
}
