// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

//! Daemon main loop: single listener, one thread per connection.

use std::ffi::CString;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::{AsRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::cli::Cli;
use crate::env;

use super::exec;
use super::lifecycle;
use super::protocol;
use super::sudoers::Sudoers;

pub fn run(cli: &Cli) -> i32 {
    // Redirect our own stdio to the log file first, so the client that
    // spawned us can point su's stdio at /dev/null without losing startup
    // diagnostics.
    redirect_stdio_to_log();

    let uid = unsafe { libc::getuid() };
    if uid != 0 {
        eprintln!("rsudo: --daemon must run as root (uid={uid})");
        return 1;
    }

    if lifecycle::is_disabled() {
        eprintln!("rsudo: daemon disabled (run `rsudo --daemon-enable` to re-enable)");
        return 0;
    }

    let lock = match lifecycle::acquire_singleton() {
        Ok(Some(l)) => l,
        Ok(None) => {
            eprintln!("rsudo: daemon already running");
            return 0;
        }
        Err(e) => {
            eprintln!("rsudo: cannot acquire daemon lock: {e}");
            return 1;
        }
    };

    let sudoers_path = cli
        .sudoers_path
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(env::default_sudoers_path);
    let sudoers = match Sudoers::load(&sudoers_path) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("rsudo: {e}");
            return 1;
        }
    };

    let socket_path = cli
        .daemon_socket
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(env::daemon_socket_path);

    eprintln!(
        "rsudo[daemon]: pid={} socket={} sudoers={}",
        std::process::id(),
        socket_path.display(),
        sudoers_path.display()
    );

    let listener = match bind_listener(&socket_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("rsudo: bind {}: {e}", socket_path.display());
            return 1;
        }
    };

    let idle = cli.daemon_idle_timeout.map(Duration::from_secs);
    let mut last_activity = Instant::now();

    loop {
        match accept_with_timeout(&listener, Duration::from_secs(1)) {
            Ok(Some(conn)) => {
                last_activity = Instant::now();
                let s = Arc::clone(&sudoers);
                thread::spawn(move || {
                    if let Err(e) = handle_connection(conn, s) {
                        eprintln!("rsudo[daemon]: connection error: {e}");
                    }
                });
            }
            Ok(None) => {
                if let Some(idle) = idle {
                    if last_activity.elapsed() >= idle {
                        eprintln!("rsudo[daemon]: idle timeout, exiting");
                        break;
                    }
                }
            }
            Err(e) => {
                eprintln!("rsudo[daemon]: accept: {e}");
                break;
            }
        }
    }

    let _ = fs::remove_file(&socket_path);
    drop(lock);
    0
}

fn redirect_stdio_to_log() {
    let log_path = env::daemon_log_path();
    if let Some(parent) = log_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let f = match fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(f) => f,
        Err(_) => return,
    };
    let fd: RawFd = f.as_raw_fd();
    unsafe {
        libc::dup2(fd, libc::STDOUT_FILENO);
        libc::dup2(fd, libc::STDERR_FILENO);
    }
    std::mem::forget(f);
}

fn bind_listener(path: &Path) -> std::io::Result<UnixListener> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o666));
    listener.set_nonblocking(true)?;
    Ok(listener)
}

fn accept_with_timeout(
    listener: &UnixListener,
    timeout: Duration,
) -> std::io::Result<Option<UnixStream>> {
    let fd = listener.as_raw_fd();
    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let ret = unsafe { libc::poll(&mut pfd, 1, timeout.as_millis() as i32) };
    if ret < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if ret == 0 {
        return Ok(None);
    }
    match listener.accept() {
        Ok((conn, _)) => {
            conn.set_nonblocking(false)?;
            Ok(Some(conn))
        }
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
        Err(e) => Err(e),
    }
}

fn peer_cred(conn: &UnixStream) -> std::io::Result<libc::ucred> {
    let fd = conn.as_raw_fd();
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let ret = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(cred)
}

fn ensure_home(env: &[(String, String)]) {
    let home = match env.iter().find(|(k, _)| k == "HOME") {
        Some((_, v)) => v.clone(),
        None => return,
    };
    let home_path = Path::new(&home);
    if home_path.exists() {
        return;
    }
    if fs::create_dir_all(home_path).is_err() {
        return;
    }
    let _ = fs::set_permissions(home_path, fs::Permissions::from_mode(0o700));
    if let Ok(c) = CString::new(home.as_bytes()) {
        unsafe {
            libc::chown(c.as_ptr(), 0, 0);
        }
    }
}

fn timestamp_now() -> String {
    unsafe {
        let mut t: libc::time_t = 0;
        libc::time(&mut t);
        let mut tm: libc::tm = std::mem::zeroed();
        libc::gmtime_r(&t, &mut tm);
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec
        )
    }
}

fn append_audit(uid: u32, pid: i32, argv: &[String]) {
    let path = env::audit_log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let ts = timestamp_now();
    let line = format!("{ts} uid={uid} pid={pid} argv={argv:?}\n");
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

fn send_log(conn: &mut UnixStream, msg: &str) {
    let _ = protocol::write_frame(conn, protocol::FRAME_LOG, msg.as_bytes());
    let _ = conn.flush();
}

fn handle_connection(mut conn: UnixStream, sudoers: Arc<Sudoers>) -> std::io::Result<()> {
    let cred = peer_cred(&conn)?;
    let req = match protocol::read_request(&mut conn) {
        Ok(r) => r,
        Err(e) => {
            let _ = protocol::write_error(&mut conn, &format!("bad request: {e}"));
            return Ok(());
        }
    };

    if req.argv.is_empty() {
        let _ = protocol::write_error(&mut conn, "empty argv");
        return Ok(());
    }

    if req.debug {
        send_log(
            &mut conn,
            &format!(
                "request: uid={} pid={} argv={:?} tty={}",
                cred.uid,
                cred.pid,
                req.argv,
                req.tty
                    .map(|t| format!("{}x{}", t.rows, t.cols))
                    .unwrap_or_else(|| "none".into())
            ),
        );
    }

    if !sudoers.is_allowed(cred.uid) {
        let msg = format!("denied: uid {} is not in the sudoers list", cred.uid);
        eprintln!("rsudo[daemon]: uid={} pid={} {msg}", cred.uid, cred.pid);
        let _ = protocol::write_error(&mut conn, &msg);
        let _ = protocol::write_exit(&mut conn, 126);
        return Ok(());
    }

    let argv0 = &req.argv[0];
    if !Path::new(argv0).is_file() {
        let msg = format!("no such command: {argv0}");
        if req.debug {
            send_log(&mut conn, &msg);
        }
        let _ = protocol::write_error(&mut conn, &msg);
        let _ = protocol::write_exit(&mut conn, 127);
        return Ok(());
    }

    ensure_home(&req.env);
    append_audit(cred.uid, cred.pid, &req.argv);
    eprintln!(
        "rsudo[daemon]: uid={} exec {} args={:?}",
        cred.uid,
        argv0,
        &req.argv[1..]
    );

    let cwd = PathBuf::from(&req.cwd);
    match exec::run(
        &req.argv,
        &cwd,
        &req.env,
        req.umask,
        req.debug,
        req.tty,
        &req.stdin,
        req.stdin_eof,
        &mut conn,
    ) {
        Ok(rc) => protocol::write_exit(&mut conn, rc),
        Err(e) => {
            let _ = protocol::write_error(&mut conn, &format!("exec error: {e}"));
            protocol::write_exit(&mut conn, 127)
        }
    }
}
