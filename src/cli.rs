// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "rsudo",
    version = "0.1.0",
    about = "Fast Termux sudo replacement",
    override_usage = "rsudo [OPTIONS] <command> [args...]
       rsudo [OPTIONS] su
       rsudo [OPTIONS] asu
       rsudo [OPTIONS] -i [--shell SHELL] [--shell-home PATH]
       rsudo [OPTIONS] -s <script> [args...]
       rsudo [OPTIONS] --dry-run [COMMAND]...
       rsudo --daemon-boot | --daemon-disable | --daemon-enable
       rsudo --daemon-restart | --daemon-status
       rsudo --sudoers-add <UID> | --sudoers-del <UID> | --sudoers-list
       rsudo --audit [N]
       rsudo --edit-pty",
    long_about = None,
)]
pub struct Cli {
    // ---- path resolution ----
    /// Prefer $PREFIX/bin over Android bins when resolving the command
    #[arg(
        short = 't',
        long = "termux-priority",
        help_heading = "Path resolution"
    )]
    pub termux_priority: bool,

    /// Restrict PATH: -T = $PREFIX/bin only, -TT = $PREFIX/bin + applets
    #[arg(
        short = 'T',
        long = "termux-only",
        action = clap::ArgAction::Count,
        help_heading = "Path resolution"
    )]
    pub termux_only: u8,

    /// Prefer Android bins over $PREFIX/bin when resolving the command
    #[arg(
        short = 'a',
        long = "android-priority",
        help_heading = "Path resolution"
    )]
    pub android_priority: bool,

    /// Restrict PATH: -A = /system/bin only, -AA = all Android bins
    #[arg(
        short = 'A',
        long = "android-only",
        action = clap::ArgAction::Count,
        help_heading = "Path resolution"
    )]
    pub android_only: u8,

    // ---- mode ----
    /// Treat the first argument as a path, never as `su` / `asu`
    #[arg(short = 'p', long = "path", help_heading = "Mode")]
    pub force_path: bool,

    /// Run an interactive root shell (default shell: bash)
    #[arg(short = 'i', long = "interactive", help_heading = "Mode")]
    pub interactive: bool,

    /// Run <script> as root (see -s <script> usage)
    #[arg(short = 's', long = "script", help_heading = "Mode")]
    pub script_mode: bool,

    // ---- shell ----
    /// Shell to launch in interactive / script mode (default: bash)
    #[arg(long = "shell", value_name = "SHELL", help_heading = "Shell")]
    pub shell: Option<String>,

    /// Root home for the shell (default: $TERMUX_HOME/.suroot)
    #[arg(long = "shell-home", value_name = "PATH", help_heading = "Shell")]
    pub shell_home: Option<String>,

    // ---- environment ----
    /// Append the caller's $PATH to the generated PATH
    #[arg(short = 'P', long = "export-all-path", help_heading = "Environment")]
    pub export_all_path: bool,

    /// Append the caller's $LD_LIBRARY_PATH to the generated LD_LIBRARY_PATH
    #[arg(
        short = 'L',
        long = "export-all-ld-lib-path",
        help_heading = "Environment"
    )]
    pub export_all_ld_lib: bool,

    /// Extra colon-separated paths appended to PATH
    #[arg(
        long = "export-paths",
        value_name = "PATHS",
        help_heading = "Environment"
    )]
    pub export_paths: Option<String>,

    /// Extra colon-separated paths appended to LD_LIBRARY_PATH
    #[arg(
        long = "export-ld-lib-paths",
        value_name = "PATHS",
        help_heading = "Environment"
    )]
    pub export_ld_lib_paths: Option<String>,

    // ---- mount ----
    /// Force --mount-master on the su command (share the global mount ns)
    #[arg(long = "mount-master", help_heading = "Mount")]
    pub mount_master_force: bool,

    /// Force --mount-master off, even if the root home is on /system
    #[arg(long = "no-mount-master", help_heading = "Mount")]
    pub mount_master_disable: bool,

    // ---- output ----
    /// Increase verbosity (-v for path decisions, -vv for kind/args, -vvv for the script)
    #[arg(
        short = 'v',
        long = "verbose",
        action = clap::ArgAction::Count,
        help_heading = "Output"
    )]
    pub verbose: u8,

    /// Print the generated shell script instead of running it
    #[arg(long = "dry-run", help_heading = "Output")]
    pub dry_run: bool,

    /// Show daemon-side diagnostics for this invocation only
    #[arg(long = "debug", help_heading = "Output")]
    pub debug: bool,

    // ---- pty ----
    /// Open the interactive pty config editor (mode + white/black lists)
    #[arg(long = "edit-pty", help_heading = "PTY control")]
    pub edit_pty: bool,

    // ---- daemon control ----
    /// Install the boot script and start the daemon now
    #[arg(long = "daemon-boot", help_heading = "Daemon control")]
    pub daemon_boot: bool,

    /// Re-enable the daemon after --daemon-disable
    #[arg(long = "daemon-enable", help_heading = "Daemon control")]
    pub daemon_enable: bool,

    /// Disable the daemon; all invocations fall back to the su path
    #[arg(long = "daemon-disable", help_heading = "Daemon control")]
    pub daemon_disable: bool,

    /// Restart the running daemon (reloads sudoers)
    #[arg(long = "daemon-restart", help_heading = "Daemon control")]
    pub daemon_restart: bool,

    /// Show daemon state (pid, socket, sudoers, disabled, boot script)
    #[arg(long = "daemon-status", help_heading = "Daemon control")]
    pub daemon_status: bool,

    // ---- sudoers ----
    /// Add a uid to the sudoers list (implies --daemon-restart)
    #[arg(long = "sudoers-add", value_name = "UID", help_heading = "Sudoers")]
    pub sudoers_add: Option<u32>,

    /// Remove a uid from the sudoers list (implies --daemon-restart)
    #[arg(long = "sudoers-del", value_name = "UID", help_heading = "Sudoers")]
    pub sudoers_del: Option<u32>,

    /// Print the current sudoers list
    #[arg(long = "sudoers-list", help_heading = "Sudoers")]
    pub sudoers_list: bool,

    /// Show the last N audit entries (default: 20)
    #[arg(
        long = "audit",
        value_name = "N",
        num_args = 0..=1,
        default_missing_value = "20",
        help_heading = "Sudoers"
    )]
    pub audit: Option<u32>,

    // ---- misc ----
    /// Clear the su / mount-master / sdk detection cache
    #[arg(long = "clear-cache", help_heading = "Misc")]
    pub clear_cache: bool,

    // ---- internal: daemon and sudoers wiring ----
    #[arg(long = "daemon", hide = true)]
    pub daemon_mode: bool,

    #[arg(long = "daemon-socket", value_name = "PATH", hide = true)]
    pub daemon_socket: Option<String>,

    #[arg(long = "daemon-idle-timeout", value_name = "SECS", hide = true)]
    pub daemon_idle_timeout: Option<u64>,

    #[arg(long = "sudoers", value_name = "PATH", hide = true)]
    pub sudoers_path: Option<String>,

    // ---- trailing command ----
    /// Command and arguments (or `su` / `asu` / script argument)
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        help_heading = "Arguments"
    )]
    pub command: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    Termux,
    Android,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathLayout {
    TermuxFull,
    TermuxMinimal,
    TermuxBinOnly,
    AndroidFull,
    AndroidSystemOnly,
    AndroidBinOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Su,
    Asu,
    Exec,
    Script,
}

#[derive(Debug, Clone, Copy)]
pub enum SearchMode {
    TermuxFirst,
    AndroidFirst,
    AndroidOnly,
}
