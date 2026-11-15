// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

//! `rsudo --daemon-*`, `rsudo --sudoers-*`, `rsudo --audit`.
//!
//! Every command that touches root-owned state runs via su.  The
//! reexec_as_root path serialises the original CLI flags into a single
//! `su -c '<exe> ...'` command, so the user only sees one Magisk prompt.

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::cli::Cli;
use crate::env;
use crate::script::quote::shell_quote;

use super::lifecycle;

pub fn run(cli: &Cli) -> i32 {
    if unsafe { libc::getuid() } != 0 {
        return reexec_as_root(cli);
    }

    if cli.daemon_disable {
        return disable();
    }
    if cli.daemon_enable {
        return enable();
    }
    if cli.daemon_status {
        return status();
    }
    if cli.daemon_restart {
        return restart(cli);
    }
    if cli.daemon_boot {
        return boot(cli);
    }
    if let Some(uid) = cli.sudoers_add {
        return sudoers_add(uid);
    }
    if let Some(uid) = cli.sudoers_del {
        return sudoers_del(uid);
    }
    if cli.sudoers_list {
        return sudoers_list();
    }
    if let Some(n) = cli.audit {
        return audit_tail(n);
    }
    1
}

fn reexec_as_root(cli: &Cli) -> i32 {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("rsudo: current_exe: {e}");
            return 1;
        }
    };
    let mut cmd = shell_quote(&exe.to_string_lossy());

    if cli.daemon_boot {
        cmd.push_str(" --daemon-boot");
    } else if cli.daemon_enable {
        cmd.push_str(" --daemon-enable");
    } else if cli.daemon_disable {
        cmd.push_str(" --daemon-disable");
    } else if cli.daemon_restart {
        cmd.push_str(" --daemon-restart");
    } else if cli.daemon_status {
        cmd.push_str(" --daemon-status");
    } else if let Some(uid) = cli.sudoers_add {
        cmd.push_str(&format!(" --sudoers-add {uid}"));
    } else if let Some(uid) = cli.sudoers_del {
        cmd.push_str(&format!(" --sudoers-del {uid}"));
    } else if cli.sudoers_list {
        cmd.push_str(" --sudoers-list");
    } else if let Some(n) = cli.audit {
        cmd.push_str(&format!(" --audit {n}"));
    }

    if let Some(p) = &cli.sudoers_path {
        cmd.push_str(&format!(" --sudoers {}", shell_quote(p)));
    }
    if let Some(s) = &cli.daemon_socket {
        cmd.push_str(&format!(" --daemon-socket {}", shell_quote(s)));
    }
    if let Some(t) = cli.daemon_idle_timeout {
        cmd.push_str(&format!(" --daemon-idle-timeout {t}"));
    }

    let status = std::process::Command::new("su")
        .arg("-c")
        .arg(&cmd)
        .status();
    match status {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("rsudo: su: {e}");
            1
        }
    }
}

fn restart_hint() {
    if lifecycle::find_pid().is_some() {
        println!("rsudo: daemon running; `rsudo --daemon-restart` to reload sudoers");
    }
}

fn disable() -> i32 {
    if let Err(e) = lifecycle::set_disabled() {
        eprintln!("rsudo: write disabled flag: {e}");
        return 1;
    }
    if lifecycle::kill_daemon() {
        println!("rsudo: daemon disabled (stopped running instance)");
    } else {
        println!("rsudo: daemon disabled (no running instance)");
    }
    // Remove the socket explicitly.  If kill_daemon fell back to SIGKILL,
    // the daemon did not get a chance to unlink it, and the leftover file
    // makes the next client connect wait on a connection that nobody is
    // listening for.
    let _ = fs::remove_file(env::daemon_socket_path());
    println!("rsudo: all future invocations will use the su path");
    println!("rsudo: re-enable with `rsudo --daemon-enable`");
    0
}

fn enable() -> i32 {
    if let Err(e) = lifecycle::clear_disabled() {
        eprintln!("rsudo: clear disabled flag: {e}");
        return 1;
    }
    println!("rsudo: daemon enabled");
    println!("rsudo: next invocation will auto-start it");
    0
}

fn status() -> i32 {
    let pid = lifecycle::find_pid();
    let disabled = lifecycle::is_disabled();
    let socket = env::daemon_socket_path();
    let sudoers = env::default_sudoers_path();
    let audit = env::audit_log_path();

    match pid {
        Some(p) => println!("daemon:      running (pid {p})"),
        None => println!("daemon:      not running"),
    }
    println!("disabled:    {}", if disabled { "yes" } else { "no" });
    println!(
        "socket:      {} ({})",
        socket.display(),
        if socket.exists() { "present" } else { "absent" }
    );
    println!(
        "sudoers:     {} ({})",
        sudoers.display(),
        if sudoers.exists() {
            "present"
        } else {
            "absent"
        }
    );
    println!(
        "audit:       {} ({})",
        audit.display(),
        if audit.exists() { "present" } else { "absent" }
    );
    
    0
}

fn boot(cli: &Cli) -> i32 {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("rsudo: current_exe: {e}");
            return 1;
        }
    };

    let sudoers = cli
        .sudoers_path
        .clone()
        .unwrap_or_else(|| env::default_sudoers_path().display().to_string());
    let socket = cli
        .daemon_socket
        .clone()
        .unwrap_or_else(|| env::daemon_socket_path().display().to_string());
    let idle = cli.daemon_idle_timeout.unwrap_or(1800);

    if !std::path::Path::new(&sudoers).is_file() {
        eprintln!("rsudo: sudoers file missing: {sudoers}");
        eprintln!("rsudo: create it with `rsudo --sudoers-add {}`", unsafe {
            libc::getuid()
        });
        return 1;
    }

    if lifecycle::find_pid().is_some() {
        println!("rsudo: daemon already running, skipping start");
        return 0;
    }

    match std::process::Command::new(&exe)
        .arg("--daemon")
        .arg("--sudoers")
        .arg(&sudoers)
        .arg("--daemon-socket")
        .arg(&socket)
        .arg("--daemon-idle-timeout")
        .arg(idle.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_) => {
            std::thread::sleep(std::time::Duration::from_millis(300));
            println!("rsudo: daemon started");
            0
        }
        Err(e) => {
            eprintln!("rsudo: start daemon: {e}");
            1
        }
    }
}

fn restart(cli: &Cli) -> i32 {
    lifecycle::kill_daemon();
    std::thread::sleep(std::time::Duration::from_millis(200));

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("rsudo: current_exe: {e}");
            return 1;
        }
    };
    let sudoers = cli
        .sudoers_path
        .clone()
        .unwrap_or_else(|| env::default_sudoers_path().display().to_string());
    let socket = cli
        .daemon_socket
        .clone()
        .unwrap_or_else(|| env::daemon_socket_path().display().to_string());

    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("--daemon")
        .arg("--sudoers")
        .arg(&sudoers)
        .arg("--daemon-socket")
        .arg(&socket);
    if let Some(t) = cli.daemon_idle_timeout {
        cmd.arg("--daemon-idle-timeout").arg(t.to_string());
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    match cmd.spawn() {
        Ok(_) => {
            std::thread::sleep(std::time::Duration::from_millis(300));
            match lifecycle::find_pid() {
                Some(p) => {
                    println!("rsudo: daemon restarted (pid {p})");
                    0
                }
                None => {
                    eprintln!(
                        "rsudo: daemon did not stay up; check {}",
                        env::daemon_log_path().display()
                    );
                    1
                }
            }
        }
        Err(e) => {
            eprintln!("rsudo: start daemon: {e}");
            1
        }
    }
}

// ---------------------------------------------------------------------------
// sudoers
// ---------------------------------------------------------------------------

fn write_sudoers_atomic(path: &Path, content: &str) -> io::Result<()> {
    let parent = match path.parent() {
        Some(p) => p,
        None => return Err(io::Error::new(io::ErrorKind::InvalidInput, "no parent")),
    };
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".sudoers.tmp.{}", std::process::id()));
    {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)?;
        use std::io::Write;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    let c = std::ffi::CString::new(tmp.as_os_str().to_string_lossy().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path nul"))?;
    unsafe {
        libc::chown(c.as_ptr(), 0, 0);
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

fn sudoers_add(uid: u32) -> i32 {
    let path = env::default_sudoers_path();
    let existing = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            eprintln!("rsudo: read {}: {e}", path.display());
            return 1;
        }
    };
    let needle = format!("allow {uid}");
    if existing.lines().any(|l| l.trim() == needle) {
        println!("rsudo: uid {uid} already present in {}", path.display());
        restart_hint();
        return 0;
    }
    let mut new_content = existing;
    if !new_content.is_empty() && !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push_str(&needle);
    new_content.push('\n');
    if let Err(e) = write_sudoers_atomic(&path, &new_content) {
        eprintln!("rsudo: write {}: {e}", path.display());
        return 1;
    }
    println!("rsudo: added uid {uid} to {}", path.display());
    restart_hint();
    0
}

fn sudoers_del(uid: u32) -> i32 {
    let path = env::default_sudoers_path();
    let existing = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("rsudo: read {}: {e}", path.display());
            return 1;
        }
    };
    let needle = format!("allow {uid}");
    let mut found = false;
    let mut new_content = String::with_capacity(existing.len());
    for line in existing.lines() {
        if line.trim() == needle {
            found = true;
            continue;
        }
        new_content.push_str(line);
        new_content.push('\n');
    }
    if !found {
        println!("rsudo: uid {uid} not present in {}", path.display());
        return 1;
    }
    if let Err(e) = write_sudoers_atomic(&path, &new_content) {
        eprintln!("rsudo: write {}: {e}", path.display());
        return 1;
    }
    println!("rsudo: removed uid {uid} from {}", path.display());
    restart_hint();
    0
}

fn sudoers_list() -> i32 {
    let path = env::default_sudoers_path();
    match fs::read_to_string(&path) {
        Ok(c) => {
            print!("{c}");
            0
        }
        Err(e) => {
            eprintln!("rsudo: read {}: {e}", path.display());
            1
        }
    }
}

// ---------------------------------------------------------------------------
// audit
// ---------------------------------------------------------------------------

fn audit_tail(n: u32) -> i32 {
    let path = env::audit_log_path();
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("rsudo: read {}: {e}", path.display());
            return 1;
        }
    };
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(n as usize);
    for line in &lines[start..] {
        println!("{line}");
    }
    0
}
