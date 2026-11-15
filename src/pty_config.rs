// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

//! pty mode configuration.
//!
//! Two orthogonal axes:
//!
//!   mode     filters commands, produces a candidate (pty or pipe)
//!     white   candidate pty iff command is in the whitelist
//!     black   candidate pty iff command is NOT in the blacklist
//!     always  candidate pty for every command
//!     never   candidate pipe for every command
//!
//!   trigger  can only downgrade a pty candidate to pipe
//!     iotty   pty only if stdin and stdout are both ttys, else pipe
//!     none    no downgrade: candidate is the answer
//!
//! The asymmetry is deliberate: a command that the top-level policy
//! rejected must not become pty just because stdin is a tty.
//!
//! Examples:
//!   white + iotty   whitelisted command, but only when both fds are ttys
//!   white + none    whitelisted command runs on a pty even from a pipe
//!   always + iotty  "pty whenever both fds are ttys" (the old iotty mode)
//!   always + none   unconditional pty
//!   never + any     unconditional pipe
//!
//! The config lives in $PREFIX/etc/rsudo-pty.conf (Termux user-writable;
//! no root needed).  `rsudo --edit-pty` opens a small line editor.

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::env;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtyMode {
    White,
    Black,
    Always,
    Never,
}

impl PtyMode {
    fn as_str(self) -> &'static str {
        match self {
            PtyMode::White => "white",
            PtyMode::Black => "black",
            PtyMode::Always => "always",
            PtyMode::Never => "never",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "white" => Some(PtyMode::White),
            "black" => Some(PtyMode::Black),
            "always" => Some(PtyMode::Always),
            "never" => Some(PtyMode::Never),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    IoTty,
    None,
}

impl Trigger {
    fn as_str(self) -> &'static str {
        match self {
            Trigger::IoTty => "iotty",
            Trigger::None => "none",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "iotty" => Some(Trigger::IoTty),
            "none" => Some(Trigger::None),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PtyConfig {
    pub mode: PtyMode,
    pub trigger: Trigger,
    pub whitelist: Vec<String>,
    pub blacklist: Vec<String>,
}

impl Default for PtyConfig {
    fn default() -> Self {
        Self {
            mode: PtyMode::White,
            trigger: Trigger::IoTty,
            whitelist: vec![
                // shells
                "bash",
                "sh",
                "zsh",
                "fish",
                "dash",
                "ash",
                "ksh",
                "ksh93",
                "mksh",
                "pdksh",
                "oksh",
                "lksh",
                "loksh",
                "elvish",
                "nu",
                "xonsh",
                // editors
                "vim",
                "vi",
                "nvim",
                "nano",
                "emacs",
                "pico",
                "micro",
                "kak",
                "helix",
                "hx",
                // pagers
                "less",
                "more",
                "most",
                "man",
                "info",
                // monitors
                "top",
                "htop",
                "btop",
                "atop",
                "iotop",
                "nvtop",
                "watch",
                // file managers / TUIs
                "ncdu",
                "ranger",
                "mc",
                "tig",
                "lazygit",
                "lazydocker",
                // remote
                "ssh",
                "mosh",
                "telnet",
                "ftp",
                "sftp",
                // multiplexers
                "tmux",
                "screen",
                "byobu",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            blacklist: Vec::new(),
        }
    }
}

fn config_path() -> PathBuf {
    PathBuf::from(env::get_prefix()).join("etc/rsudo-pty.conf")
}

impl PtyConfig {
    pub fn mode_name(&self) -> &'static str {
        self.mode.as_str()
    }

    pub fn trigger_name(&self) -> &'static str {
        self.trigger.as_str()
    }
    pub fn load() -> Self {
        let content = match std::fs::read_to_string(config_path()) {
            Ok(c) => c,
            Err(_) => return Self::default(),
        };
        let mut cfg = Self::default();
        for line in content.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let (k, v) = match line.split_once('=') {
                Some(kv) => kv,
                None => continue,
            };
            let k = k.trim();
            let v = v.trim();
            match k {
                "mode" => {
                    if let Some(m) = PtyMode::parse(v) {
                        cfg.mode = m;
                    }
                }
                "trigger" => {
                    if let Some(t) = Trigger::parse(v) {
                        cfg.trigger = t;
                    }
                }
                "white" => {
                    cfg.whitelist = v.split_whitespace().map(String::from).collect();
                }
                "black" => {
                    cfg.blacklist = v.split_whitespace().map(String::from).collect();
                }
                _ => {}
            }
        }
        cfg
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = format!(
            "# rsudo pty config.  Edited by `rsudo --edit-pty`.\n\
             #\n\
             # mode     white | black | always | never   (filters commands)\n\
             # trigger  iotty | none                     (may downgrade pty to pipe)\n\
             #\n\
             # Examples:\n\
             #   mode = white, trigger = iotty   whitelist, only when both fds are ttys\n\
             #   mode = always, trigger = iotty  pty whenever both fds are ttys\n\
             #   mode = always, trigger = none   unconditional pty\n\
             #   mode = never,  trigger = any    unconditional pipe\n\
             mode = {}\n\
             trigger = {}\n\
             white = {}\n\
             black = {}\n",
            self.mode.as_str(),
            self.trigger.as_str(),
            self.whitelist.join(" "),
            self.blacklist.join(" "),
        );
        std::fs::write(&path, content)
    }

    pub fn decide(&self, cmd_name: &str) -> bool {
        let basename = Path::new(cmd_name)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(cmd_name);

        // Step 1: top-level decision filters the command.  A rejection
        // here is final; the trigger cannot upgrade it.
        let candidate = match self.mode {
            PtyMode::White => self.whitelist.iter().any(|s| s == basename),
            PtyMode::Black => !self.blacklist.iter().any(|s| s == basename),
            PtyMode::Always => true,
            PtyMode::Never => false,
        };
        if !candidate {
            return false;
        }

        // Step 2: trigger may only downgrade pty to pipe.
        match self.trigger {
            Trigger::None => true,
            Trigger::IoTty => std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        }
    }
}

pub fn edit_repl() -> i32 {
    let mut cfg = PtyConfig::load();
    println!("rsudo pty config editor");
    println!("config file: {}", config_path().display());
    println!("type `help` for commands, `exit` to quit. changes are saved immediately.");
    println!();

    let stdin = std::io::stdin();
    let mut stdin_lock = stdin.lock();
    let mut line = String::new();

    loop {
        print!("rsudo-pty> ");
        std::io::stdout().flush().ok();
        line.clear();
        if stdin_lock.read_line(&mut line).is_err() {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        match parts[0] {
            "exit" | "quit" | "q" => break,
            "help" | "h" | "?" => print_help(),
            "show" | "s" => show(&cfg),
            "mode" => {
                if parts.len() == 1 {
                    println!("mode: {}", cfg.mode.as_str());
                } else if parts.len() == 2 {
                    match PtyMode::parse(parts[1]) {
                        Some(m) => {
                            cfg.mode = m;
                            save_and_report(&cfg);
                        }
                        None => {
                            println!("unknown mode: {} (use white|black|always|never)", parts[1])
                        }
                    }
                } else {
                    println!("usage: mode [white|black|always|never]");
                }
            }
            "trigger" | "tr" => {
                if parts.len() == 1 {
                    println!("trigger: {}", cfg.trigger.as_str());
                } else if parts.len() == 2 {
                    match Trigger::parse(parts[1]) {
                        Some(t) => {
                            cfg.trigger = t;
                            save_and_report(&cfg);
                        }
                        None => println!("unknown trigger: {} (use iotty|none)", parts[1]),
                    }
                } else {
                    println!("usage: trigger [iotty|none]");
                }
            }
            "white" | "w" => {
                if handle_list(&mut cfg.whitelist, &parts, "whitelist") {
                    save_and_report(&cfg);
                }
            }
            "black" | "b" => {
                if handle_list(&mut cfg.blacklist, &parts, "blacklist") {
                    save_and_report(&cfg);
                }
            }
            _ => println!("unknown command: {} (try `help`)", parts[0]),
        }
    }
    println!("bye.");
    0
}

fn show(cfg: &PtyConfig) {
    println!("mode:    {}", cfg.mode.as_str());
    println!("trigger: {}", cfg.trigger.as_str());
    println!(
        "white:   {}",
        if cfg.whitelist.is_empty() {
            "(empty)".to_string()
        } else {
            cfg.whitelist.join(" ")
        }
    );
    println!(
        "black:   {}",
        if cfg.blacklist.is_empty() {
            "(empty)".to_string()
        } else {
            cfg.blacklist.join(" ")
        }
    );
}

fn save_and_report(cfg: &PtyConfig) {
    match cfg.save() {
        Ok(()) => println!("saved to {}", config_path().display()),
        Err(e) => eprintln!("save failed: {e}"),
    }
}

fn handle_list(list: &mut Vec<String>, parts: &[&str], name: &str) -> bool {
    if parts.len() == 1 {
        if list.is_empty() {
            println!("{name}: (empty)");
        } else {
            println!("{name}: {}", list.join(" "));
        }
        return false;
    }
    match parts[1] {
        "add" => {
            if parts.len() < 3 {
                println!("usage: {name} add <cmd> [cmd2 ...]");
                return false;
            }
            let mut added = 0;
            for cmd in &parts[2..] {
                if !list.iter().any(|s| s == cmd) {
                    list.push(cmd.to_string());
                    added += 1;
                }
            }
            if added > 0 {
                true
            } else {
                println!("no changes (all already present)");
                false
            }
        }
        "rm" | "remove" => {
            if parts.len() < 3 {
                println!("usage: {name} rm <cmd> [cmd2 ...]");
                return false;
            }
            let before = list.len();
            list.retain(|s| !parts[2..].contains(&s.as_str()));
            if list.len() < before {
                true
            } else {
                println!("no changes (none were present)");
                false
            }
        }
        "clear" => {
            if list.is_empty() {
                println!("{name} already empty");
                false
            } else {
                list.clear();
                true
            }
        }
        _ => {
            println!("usage: {name} [add|rm|clear] [cmd...]");
            false
        }
    }
}

fn print_help() {
    println!("commands:");
    println!("  show                       show current config");
    println!("  mode                       show current mode");
    println!("  mode <m>                   set mode (white|black|always|never)");
    println!("  trigger                    show current trigger");
    println!("  trigger <t>                set trigger (iotty|none)");
    println!("  white                      show whitelist");
    println!("  white add <cmd> [cmd2…]    add to whitelist");
    println!("  white rm <cmd> [cmd2…]     remove from whitelist");
    println!("  white clear                empty whitelist");
    println!("  black …                    same operations on blacklist");
    println!("  help                       this help");
    println!("  exit / quit                leave the editor");
    println!();
    println!("mode (top-level, filters commands):");
    println!("  white   candidate pty iff cmd is in the whitelist");
    println!("  black   candidate pty iff cmd is NOT in the blacklist");
    println!("  always  candidate pty for every command");
    println!("  never   candidate pipe for every command");
    println!();
    println!("trigger (bottom-level, can only downgrade pty → pipe):");
    println!("  iotty   pty only if stdin and stdout are both ttys");
    println!("  none    no downgrade: candidate is the answer");
    println!();
    println!("common combinations:");
    println!("  white  + iotty   whitelisted cmds, only when both fds are ttys (default)");
    println!("  white  + none    whitelisted cmds always on a pty");
    println!("  always + iotty   pty whenever both fds are ttys (old \"iotty\" mode)");
    println!("  always + none    unconditional pty");
    println!("  never  + any     unconditional pipe");
    println!();
    println!("changes are written to the config file immediately.");
}
