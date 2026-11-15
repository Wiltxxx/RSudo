// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

//! Client side of the daemon protocol.

use std::io::{ErrorKind, IsTerminal, Read, Write};
use std::os::unix::io::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use libc::Ioctl;
use crate::cli::{Cli, Kind, PathLayout};
use crate::context::TermuxEnv;
use crate::decisions;
use crate::env;
use crate::path;
use crate::pty_config::PtyConfig;
use crate::script;
use crate::script::env_block;
use crate::script::quote::shell_quote;
use crate::su::SuInfo;

use super::lifecycle;
use super::protocol::{self, TtySpec};

const SPAWN_WAIT: Duration = Duration::from_millis(8000);
const SU_EXIT_GRACE: Duration = Duration::from_millis(300);
const PTY_POLL_MS: libc::c_int = 2;
const STDIN_POLL_MS: libc::c_int = 50;

static WINCH_FLAG: AtomicBool = AtomicBool::new(false);
static PTY_STDIN_DONE: AtomicBool = AtomicBool::new(false);

extern "C" fn on_winch(_sig: libc::c_int) {
    WINCH_FLAG.store(true, Ordering::Relaxed);
}

pub fn run_lazy_su(cli: &Cli, kind: Kind, args: &[String], t_env: &TermuxEnv) -> Option<i32> {
    if !matches!(kind, Kind::Exec | Kind::Script) {
        return None;
    }
    if args.is_empty() {
        return None;
    }
    if lifecycle::is_disabled() {
        return None;
    }

    let socket_path = cli
        .daemon_socket
        .clone()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(env::daemon_socket_path);

    let conn = match UnixStream::connect(&socket_path) {
        Ok(c) => c,
        Err(_) => {
            let su = crate::su::detect().ok()?;
            let child_pid = spawn_daemon(cli, &su, &socket_path)?;
            connect_retry(&socket_path, child_pid)?
        }
    };

    send_and_recv(cli, kind, args, t_env, conn)
}

fn connect_retry(path: &Path, child_pid: libc::pid_t) -> Option<UnixStream> {
    let deadline = std::time::Instant::now() + SPAWN_WAIT;
    let mut child_exited_at: Option<std::time::Instant> = None;

    while std::time::Instant::now() < deadline {
        if let Ok(c) = UnixStream::connect(path) {
            return Some(c);
        }
        if child_exited_at.is_none() {
            let mut status: libc::c_int = 0;
            let ret = unsafe { libc::waitpid(child_pid, &mut status, libc::WNOHANG) };
            if ret == child_pid {
                child_exited_at = Some(std::time::Instant::now());
            }
        }
        if let Some(t) = child_exited_at {
            if t.elapsed() > SU_EXIT_GRACE {
                return None;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

fn spawn_daemon(cli: &Cli, su: &SuInfo, socket_path: &Path) -> Option<libc::pid_t> {
    if lifecycle::is_disabled() {
        return None;
    }
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return None,
    };
    let sudoers = cli
        .sudoers_path
        .clone()
        .unwrap_or_else(|| env::default_sudoers_path().display().to_string());

    let mut daemon_cmd = format!(
        "{} --daemon --sudoers {} --daemon-socket {}",
        shell_quote(&exe.to_string_lossy()),
        shell_quote(&sudoers),
        shell_quote(&socket_path.to_string_lossy()),
    );
    if let Some(t) = cli.daemon_idle_timeout {
        daemon_cmd.push_str(&format!(" --daemon-idle-timeout {t}"));
    }

    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return None;
    }
    if pid == 0 {
        use std::os::unix::process::CommandExt;
        let _ = std::process::Command::new(&su.path)
            .arg("-c")
            .arg(&daemon_cmd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .exec();
        unsafe { libc::_exit(1) };
    }
    Some(pid)
}

fn get_umask() -> u32 {
    unsafe {
        let old = libc::umask(0o022);
        libc::umask(old);
        old as u32
    }
}

fn get_winsize(fd: RawFd) -> (u16, u16) {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let ret = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ as Ioctl, &mut ws) };
    if ret == 0 && ws.ws_row > 0 && ws.ws_col > 0 {
        (ws.ws_row, ws.ws_col)
    } else {
        (24, 80)
    }
}

fn set_raw_mode(fd: RawFd) -> std::io::Result<libc::termios> {
    let mut orig: libc::termios = unsafe { std::mem::zeroed() };
    unsafe {
        if libc::tcgetattr(fd, &mut orig) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut raw = orig;
        libc::cfmakeraw(&mut raw);
        if libc::tcsetattr(fd, libc::TCSANOW, &raw) != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(orig)
}

fn restore_termios(fd: RawFd, orig: &libc::termios) {
    unsafe {
        libc::tcsetattr(fd, libc::TCSANOW, orig);
    }
}

fn install_winch_handler() {
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = on_winch as *const () as libc::sighandler_t;
        sa.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(libc::SIGWINCH, &sa, std::ptr::null_mut());
    }
}

fn send_and_recv(
    cli: &Cli,
    kind: Kind,
    args: &[String],
    t_env: &TermuxEnv,
    conn: UnixStream,
) -> Option<i32> {
    let (argv, layout, pty_cmd) = match kind {
        Kind::Exec => {
            let (search_mode, _) = decisions::decide_search_mode(cli);
            let argv0 = match path::resolve_command(&args[0], &t_env.prefix, t_env.sdk, search_mode)
            {
                Ok(p) => p,
                Err(_) => return None,
            };
            let under_rootfs = path::path_starts_with(&argv0, &t_env.rootfs);
            let layout = decisions::decide_layout(cli, Kind::Exec, under_rootfs);
            let mut v = Vec::with_capacity(args.len());
            v.push(argv0.to_string_lossy().into_owned());
            v.extend_from_slice(&args[1..]);
            (v, layout, Some(args[0].as_str()))
        }
        Kind::Script => {
            let shell = script::script_mode::script_shell(cli, t_env).ok()?;
            let layout = decisions::decide_layout(cli, Kind::Script, true);
            let script_text = args.get(0)?;
            let extra = &args[1..];
            let body = script::script_mode::build_script_mode_script(
                script_text,
                extra,
                layout,
                t_env,
                cli,
                &shell,
            );
            (vec![shell, "-c".to_string(), body], layout, None)
        }
        _ => return None,
    };

    let env = compute_daemon_env(layout, t_env, cli);

    let pty_cfg = PtyConfig::load();
    let want_tty = pty_cmd.map(|cmd| pty_cfg.decide(cmd)).unwrap_or(false);
    if cli.debug {
        eprintln!(
            "[rsudo debug] pty mode={} trigger={} cmd={:?} want_tty={} stdin_tty={} stdout_tty={}",
            pty_cfg.mode_name(),
            pty_cfg.trigger_name(),
            pty_cmd,
            want_tty,
            std::io::stdin().is_terminal(),
            std::io::stdout().is_terminal(),
        );
    }

    let stdin_tty = std::io::stdin().is_terminal();

    if want_tty {
        let (rows, cols) = get_winsize(0);
        // If stdin is a tty, the daemon streams via FRAME_STDIN and the
        // child gets a real interactive pty.  If not, we read the whole
        // stdin now and let the daemon write it to the pty followed by
        // an EOT, so `echo hi | rsudo cat` terminates instead of hanging.
        let (stdin_data, stdin_eof) = if stdin_tty {
            (Vec::new(), false)
        } else {
            (read_stdin_best_effort(), true)
        };
        let req = protocol::Request {
            argv,
            cwd: t_env.cwd.to_string_lossy().into_owned(),
            env,
            umask: get_umask(),
            debug: cli.debug,
            tty: Some(TtySpec { rows, cols }),
            stdin: stdin_data,
            stdin_eof,
        };
        run_pty_session(conn, req)
    } else {
        let stdin_data = read_stdin_best_effort();
        let req = protocol::Request {
            argv,
            cwd: t_env.cwd.to_string_lossy().into_owned(),
            env,
            umask: get_umask(),
            debug: cli.debug,
            tty: None,
            stdin: stdin_data,
            stdin_eof: true,
        };
        run_pipe_session(conn, req)
    }
}

fn run_pipe_session(mut conn: UnixStream, req: protocol::Request) -> Option<i32> {
    let _ = conn.set_read_timeout(None);
    if protocol::write_request(&mut conn, &req).is_err() {
        return None;
    }

    install_cancel_signal_handlers();

    let conn_fd = conn.as_raw_fd();
    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    let mut exit_code: Option<i32> = None;

    loop {
        let mut pfd = libc::pollfd {
            fd: conn_fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let ret = unsafe { libc::poll(&mut pfd, 1, -1) };
        if ret < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == ErrorKind::Interrupted {
                let _ = protocol::write_frame(&mut conn, protocol::FRAME_CANCEL, &[]);
                continue;
            }
            return exit_code.or(Some(1));
        }

        let (kind, data) = match protocol::read_frame(&mut conn) {
            Ok(f) => f,
            Err(e) if e.kind() == ErrorKind::Interrupted => {
                let _ = protocol::write_frame(&mut conn, protocol::FRAME_CANCEL, &[]);
                continue;
            }
            Err(_) => return exit_code.or(Some(1)),
        };
        match kind {
            protocol::FRAME_STDOUT => {
                let _ = stdout.write_all(&data);
                let _ = stdout.flush();
            }
            protocol::FRAME_STDERR => {
                let _ = stderr.write_all(&data);
                let _ = stderr.flush();
            }
            protocol::FRAME_LOG => {
                let _ = stderr.write_all(b"[rsudo debug] ");
                let _ = stderr.write_all(&data);
                let _ = stderr.write_all(b"\n");
                let _ = stderr.flush();
            }
            protocol::FRAME_EXIT => {
                if data.len() == 4 {
                    exit_code = Some(i32::from_be_bytes([data[0], data[1], data[2], data[3]]));
                }
                return exit_code.or(Some(1));
            }
            protocol::FRAME_ERROR => {
                let _ = stderr.write_all(&data);
                let _ = stderr.write_all(b"\n");
                let _ = stderr.flush();
            }
            _ => return exit_code.or(Some(1)),
        }
    }
}

fn run_pty_session(mut conn: UnixStream, req: protocol::Request) -> Option<i32> {
    // Raw mode requires a tty.  When `stdin_eof` is true, stdin was
    // already drained into req.stdin; skip raw mode and the reader
    // thread entirely.
    let orig: Option<libc::termios> = set_raw_mode(0).ok();
    if orig.is_some() {
        install_winch_handler();
    }

    if protocol::write_request(&mut conn, &req).is_err() {
        if let Some(o) = &orig {
            restore_termios(0, o);
        }
        return None;
    }

    PTY_STDIN_DONE.store(false, Ordering::Relaxed);
    let reader_conn = if !req.stdin_eof {
        match conn.try_clone() {
            Ok(c) => Some(c),
            Err(_) => {
                if let Some(o) = &orig {
                    restore_termios(0, o);
                }
                return None;
            }
        }
    } else {
        None
    };
    let stdin_thread = reader_conn.map(|c| {
        std::thread::spawn(move || {
            let mut c = c;
            let mut buf = [0u8; 4096];
            loop {
                if PTY_STDIN_DONE.load(Ordering::Relaxed) {
                    break;
                }
                let mut pfd = libc::pollfd {
                    fd: libc::STDIN_FILENO,
                    events: libc::POLLIN,
                    revents: 0,
                };
                let ret = unsafe { libc::poll(&mut pfd, 1, STDIN_POLL_MS) };
                if ret < 0 {
                    let e = std::io::Error::last_os_error();
                    if e.kind() == ErrorKind::Interrupted {
                        continue;
                    }
                    break;
                }
                if ret == 0 {
                    continue;
                }
                match std::io::stdin().read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if protocol::write_frame(&mut c, protocol::FRAME_STDIN, &buf[..n]).is_err()
                        {
                            break;
                        }
                    }
                    Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        })
    });

    let conn_fd = conn.as_raw_fd();
    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    let mut exit_code: Option<i32> = None;

    loop {
        if WINCH_FLAG.swap(false, Ordering::Relaxed) && orig.is_some() {
            let (rows, cols) = get_winsize(0);
            let mut data = [0u8; 4];
            data[0..2].copy_from_slice(&rows.to_be_bytes());
            data[2..4].copy_from_slice(&cols.to_be_bytes());
            let _ = protocol::write_frame(&mut conn, protocol::FRAME_RESIZE, &data);
        }

        let mut pfd = libc::pollfd {
            fd: conn_fd,
            events: libc::POLLIN | libc::POLLHUP,
            revents: 0,
        };
        let ret = unsafe { libc::poll(&mut pfd, 1, PTY_POLL_MS) };
        if ret < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if ret == 0 {
            continue;
        }

        let (kind, data) = match protocol::read_frame(&mut conn) {
            Ok(f) => f,
            Err(_) => break,
        };
        match kind {
            protocol::FRAME_STDOUT | protocol::FRAME_STDERR => {
                let _ = stdout.write_all(&data);
                let _ = stdout.flush();
            }
            protocol::FRAME_LOG => {
                let _ = stderr.write_all(b"[rsudo debug] ");
                let _ = stderr.write_all(&data);
                let _ = stderr.write_all(b"\n");
                let _ = stderr.flush();
            }
            protocol::FRAME_EXIT => {
                if data.len() == 4 {
                    exit_code = Some(i32::from_be_bytes([data[0], data[1], data[2], data[3]]));
                }
                break;
            }
            protocol::FRAME_ERROR => {
                let _ = stderr.write_all(&data);
                let _ = stderr.write_all(b"\n");
                let _ = stderr.flush();
            }
            _ => {}
        }
    }

    PTY_STDIN_DONE.store(true, Ordering::Relaxed);
    if let Some(o) = &orig {
        restore_termios(0, o);
    }
    if let Some(t) = stdin_thread {
        let _ = t.join();
    }

    exit_code.or(Some(1))
}

extern "C" fn on_cancel_signal(_sig: libc::c_int) {}

fn install_cancel_signal_handlers() {
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = on_cancel_signal as *const () as libc::sighandler_t;
        sa.sa_flags = 0;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(libc::SIGINT, &sa, std::ptr::null_mut());
        libc::sigaction(libc::SIGTERM, &sa, std::ptr::null_mut());
    }
}

fn compute_daemon_env(layout: PathLayout, t_env: &TermuxEnv, cli: &Cli) -> Vec<(String, String)> {
    let (path_str, ld_lib, ld_pre) = env_block::compute_env(layout, &t_env.prefix, t_env.sdk, cli);

    let mut env: Vec<(String, String)> = Vec::with_capacity(12);
    env.push(("PATH".into(), path_str));
    env.push(("TERMUX__PREFIX".into(), t_env.prefix.clone()));
    env.push(("TERMUX__HOME".into(), t_env.termux_home.clone()));
    env.push(("TERMUX__ROOTFS_DIR".into(), t_env.rootfs.clone()));
    env.push(("PREFIX".into(), t_env.prefix.clone()));
    env.push(("TMPDIR".into(), format!("{}/tmp", t_env.prefix)));
    env.push(("HOME".into(), t_env.root_home.clone()));
    env.push((
        "TERM".into(),
        std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".into()),
    ));
    env.push((
        "LANG".into(),
        std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".into()),
    ));
    if !ld_lib.is_empty() {
        env.push(("LD_LIBRARY_PATH".into(), ld_lib));
    }
    if !ld_pre.is_empty() {
        env.push(("LD_PRELOAD".into(), ld_pre));
    }
    env
}

fn read_stdin_best_effort() -> Vec<u8> {
    if std::io::stdin().is_terminal() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut buf);
    buf
}
