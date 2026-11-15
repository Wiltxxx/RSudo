// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

//! Run a command as root, stream its output back over the socket, and
//! terminate the child when the client sends FRAME_CANCEL.

use std::io::{ErrorKind, Read, Write};
use std::os::fd::FromRawFd;
use std::os::unix::io::AsRawFd;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;

use super::protocol::{self, TtySpec};

const BUF_SIZE: usize = 8192;
const PTY_POLL_MS: libc::c_int = 2;
const DRAIN_POLL_MS: libc::c_int = 50;
const EOT: u8 = 0x04;

pub fn run(
    argv: &[String],
    cwd: &Path,
    env: &[(String, String)],
    umask: u32,
    debug: bool,
    tty: Option<TtySpec>,
    stdin: &[u8],
    stdin_eof: bool,
    conn: &mut (impl Read + Write + AsRawFd),
) -> std::io::Result<i32> {
    if argv.is_empty() {
        return Err(std::io::Error::new(ErrorKind::InvalidInput, "empty argv"));
    }
    match tty {
        Some(t) => run_pty(argv, cwd, env, umask, debug, t, stdin, stdin_eof, conn),
        None => run_pipe(argv, cwd, env, umask, debug, stdin, conn),
    }
}

fn build_command(argv: &[String], cwd: &Path, env: &[(String, String)]) -> Command {
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]).current_dir(cwd);
    cmd.env_clear();
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd
}

fn exit_code_from_status(status: std::process::ExitStatus) -> i32 {
    match (status.code(), status.signal()) {
        (Some(code), _) => code,
        (None, Some(sig)) => 128 + sig,
        (None, None) => 128,
    }
}

fn terminate(pid: libc::pid_t, force: bool) {
    unsafe {
        libc::kill(pid, if force { libc::SIGKILL } else { libc::SIGINT });
    }
}

// ---------------------------------------------------------------------------
// pipe mode
// ---------------------------------------------------------------------------

fn run_pipe(
    argv: &[String],
    cwd: &Path,
    env: &[(String, String)],
    umask: u32,
    debug: bool,
    stdin: &[u8],
    conn: &mut (impl Read + Write + AsRawFd),
) -> std::io::Result<i32> {
    if debug {
        protocol::write_frame(
            conn,
            protocol::FRAME_LOG,
            format!("exec {} args={:?}", argv[0], &argv[1..]).as_bytes(),
        )?;
        conn.flush()?;
    }

    let mut cmd = build_command(argv, cwd, env);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    unsafe {
        cmd.pre_exec(move || {
            libc::umask(umask as libc::mode_t);
            Ok(())
        });
    }

    let mut child = cmd.spawn()?;
    let child_pid = child.id() as libc::pid_t;

    let stdin_thread = child.stdin.take().map(|mut si| {
        let data = stdin.to_vec();
        thread::spawn(move || {
            let _ = si.write_all(&data);
        })
    });

    let stdout_fd = child.stdout.as_ref().map(|s| s.as_raw_fd());
    let stderr_fd = child.stderr.as_ref().map(|s| s.as_raw_fd());
    let conn_fd = conn.as_raw_fd();

    let mut out_open = stdout_fd.is_some();
    let mut err_open = stderr_fd.is_some();
    let mut cancel_sent = false;
    let mut buf = [0u8; BUF_SIZE];

    while out_open || err_open {
        let mut fds: Vec<libc::pollfd> = Vec::with_capacity(3);
        fds.push(libc::pollfd {
            fd: conn_fd,
            events: libc::POLLIN,
            revents: 0,
        });
        let out_idx = if out_open {
            fds.push(libc::pollfd {
                fd: stdout_fd.unwrap(),
                events: libc::POLLIN,
                revents: 0,
            });
            Some(fds.len() - 1)
        } else {
            None
        };
        let err_idx = if err_open {
            fds.push(libc::pollfd {
                fd: stderr_fd.unwrap(),
                events: libc::POLLIN,
                revents: 0,
            });
            Some(fds.len() - 1)
        } else {
            None
        };

        let ret = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, -1) };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == ErrorKind::Interrupted {
                continue;
            }
            terminate(child_pid, true);
            if let Some(t) = stdin_thread {
                let _ = t.join();
            }
            return Err(err);
        }

        if fds[0].revents != 0 {
            match protocol::read_frame(conn) {
                Ok((protocol::FRAME_CANCEL, _)) => {
                    terminate(child_pid, cancel_sent);
                    cancel_sent = true;
                }
                Ok(_) => {}
                Err(_) => {
                    terminate(child_pid, true);
                    break;
                }
            }
        }

        if let Some(i) = out_idx {
            if fds[i].revents != 0 {
                match child.stdout.as_mut().unwrap().read(&mut buf) {
                    Ok(0) => {
                        out_open = false;
                        child.stdout.take();
                    }
                    Ok(n) => {
                        protocol::write_frame(conn, protocol::FRAME_STDOUT, &buf[..n])?;
                        conn.flush()?;
                    }
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    Err(_) => {
                        out_open = false;
                        child.stdout.take();
                    }
                }
            }
        }

        if let Some(i) = err_idx {
            if fds[i].revents != 0 {
                match child.stderr.as_mut().unwrap().read(&mut buf) {
                    Ok(0) => {
                        err_open = false;
                        child.stderr.take();
                    }
                    Ok(n) => {
                        protocol::write_frame(conn, protocol::FRAME_STDERR, &buf[..n])?;
                        conn.flush()?;
                    }
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    Err(_) => {
                        err_open = false;
                        child.stderr.take();
                    }
                }
            }
        }
    }

    if let Some(t) = stdin_thread {
        let _ = t.join();
    }
    let status = child.wait()?;
    Ok(exit_code_from_status(status))
}

// ---------------------------------------------------------------------------
// pty mode
// ---------------------------------------------------------------------------

fn set_nonblocking(fd: libc::c_int) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags >= 0 {
            libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
    }
}

fn drain_master(
    master: &mut std::fs::File,
    buf: &mut [u8],
    conn: &mut (impl Read + Write + AsRawFd),
) -> std::io::Result<()> {
    let master_raw = master.as_raw_fd();
    let mut empty_polls = 0u8;
    loop {
        match master.read(buf) {
            Ok(0) => return Ok(()),
            Ok(n) => {
                protocol::write_frame(conn, protocol::FRAME_STDOUT, &buf[..n])?;
                empty_polls = 0;
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                if empty_polls >= 1 {
                    return Ok(());
                }
                empty_polls += 1;
                let mut pfd = libc::pollfd {
                    fd: master_raw,
                    events: libc::POLLIN,
                    revents: 0,
                };
                let ret = unsafe { libc::poll(&mut pfd, 1, DRAIN_POLL_MS) };
                if ret <= 0 {
                    return Ok(());
                }
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return Ok(()),
        }
    }
}

fn run_pty(
    argv: &[String],
    cwd: &Path,
    env: &[(String, String)],
    umask: u32,
    debug: bool,
    tty: TtySpec,
    stdin: &[u8],
    stdin_eof: bool,
    conn: &mut (impl Read + Write + AsRawFd),
) -> std::io::Result<i32> {
    let mut master_fd: libc::c_int = -1;
    let mut slave_fd: libc::c_int = -1;
    let ws = libc::winsize {
        ws_row: tty.rows,
        ws_col: tty.cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let ret = unsafe {
        libc::openpty(
            &mut master_fd,
            &mut slave_fd,
            std::ptr::null_mut(),
            std::ptr::null(),
            &ws,
        )
    };
    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }

    // When the caller feeds stdin from a pipe (`stdin_eof`), the pty
    // must not echo: the bytes the caller wrote are not keystrokes, and
    // echoing them would duplicate the output ("hello" twice).
    if stdin_eof {
        let mut t: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(slave_fd, &mut t) } == 0 {
            t.c_lflag &= !(libc::ECHO | libc::ECHOE | libc::ECHOK | libc::ECHONL);
            unsafe { libc::tcsetattr(slave_fd, libc::TCSANOW, &t) };
        }
    }

    if debug {
        protocol::write_frame(
            conn,
            protocol::FRAME_LOG,
            format!(
                "exec pty {} args={:?} size={}x{} stdin_eof={}",
                argv[0],
                &argv[1..],
                tty.rows,
                tty.cols,
                stdin_eof
            )
            .as_bytes(),
        )?;
        conn.flush()?;
    }

    let mut cmd = build_command(argv, cwd, env);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        cmd.pre_exec(move || {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::ioctl(slave_fd, libc::TIOCSCTTY as libc::c_int, 0 as libc::c_int) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::dup2(slave_fd, 0) < 0
                || libc::dup2(slave_fd, 1) < 0
                || libc::dup2(slave_fd, 2) < 0
            {
                return Err(std::io::Error::last_os_error());
            }
            if slave_fd > 2 {
                libc::close(slave_fd);
            }
            libc::umask(umask as libc::mode_t);
            Ok(())
        });
    }

    let mut child = cmd.spawn()?;
    let child_pid = child.id() as libc::pid_t;
    unsafe {
        libc::close(slave_fd);
    }
    let mut master_file = unsafe { std::fs::File::from_raw_fd(master_fd) };
    let master_raw = master_file.as_raw_fd();

    // Feed the buffered stdin and close the direction with EOT.  EOT is
    // honoured by the line discipline in canonical mode, which covers
    // cat / wc / grep / etc.  A slave that switched itself to raw mode
    // (interactive shells) is not in this branch anyway.
    if stdin_eof {
        if !stdin.is_empty() {
            let _ = master_file.write_all(stdin);
        }
        let _ = master_file.write_all(&[EOT]);
    }

    let conn_fd = conn.as_raw_fd();
    let mut buf = [0u8; BUF_SIZE];
    let mut master_eof = false;
    let mut child_exited = false;
    let mut exit_code: Option<i32> = None;
    let mut cancel_sent = false;

    loop {
        if child_exited && master_eof {
            break;
        }

        let mut fds: Vec<libc::pollfd> = Vec::with_capacity(2);
        fds.push(libc::pollfd {
            fd: conn_fd,
            events: libc::POLLIN,
            revents: 0,
        });
        let master_idx = if !master_eof {
            fds.push(libc::pollfd {
                fd: master_raw,
                events: libc::POLLIN | libc::POLLHUP,
                revents: 0,
            });
            Some(fds.len() - 1)
        } else {
            None
        };

        let ret = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, PTY_POLL_MS) };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == ErrorKind::Interrupted {
                continue;
            }
            terminate(child_pid, true);
            return Err(err);
        }

        if fds[0].revents != 0 {
            if fds[0].revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
                terminate(child_pid, true);
                break;
            }
            match protocol::read_frame(conn) {
                Ok((protocol::FRAME_STDIN, data)) => {
                    if !master_eof && !stdin_eof {
                        let _ = master_file.write_all(&data);
                    }
                }
                Ok((protocol::FRAME_RESIZE, data)) => {
                    if data.len() == 4 && !master_eof {
                        let rows = u16::from_be_bytes([data[0], data[1]]);
                        let cols = u16::from_be_bytes([data[2], data[3]]);
                        let ws = libc::winsize {
                            ws_row: rows,
                            ws_col: cols,
                            ws_xpixel: 0,
                            ws_ypixel: 0,
                        };
                        unsafe {
                            libc::ioctl(master_raw, libc::TIOCSWINSZ as libc::c_int, &ws);
                        }
                    }
                }
                Ok((protocol::FRAME_CANCEL, _)) => {
                    terminate(child_pid, cancel_sent);
                    cancel_sent = true;
                }
                Ok(_) => {}
                Err(_) => {
                    terminate(child_pid, true);
                    break;
                }
            }
        }

        if let Some(i) = master_idx {
            if fds[i].revents != 0 {
                match master_file.read(&mut buf) {
                    Ok(0) => master_eof = true,
                    Ok(n) => {
                        protocol::write_frame(conn, protocol::FRAME_STDOUT, &buf[..n])?;
                        conn.flush()?;
                    }
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    Err(_) => master_eof = true,
                }
            }
        }

        if !child_exited {
            if let Some(status) = child.try_wait()? {
                exit_code = Some(exit_code_from_status(status));
                child_exited = true;
                if !master_eof {
                    set_nonblocking(master_raw);
                    let _ = drain_master(&mut master_file, &mut buf, conn);
                    conn.flush()?;
                    master_eof = true;
                }
            }
        }
    }

    if !master_eof {
        set_nonblocking(master_raw);
        let _ = drain_master(&mut master_file, &mut buf, conn);
    }

    if !child_exited {
        let status = child.wait()?;
        exit_code = Some(exit_code_from_status(status));
    }

    Ok(exit_code.unwrap_or(128))
}
