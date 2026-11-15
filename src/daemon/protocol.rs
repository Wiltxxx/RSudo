// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

//! Wire format between rsudo daemon and its clients.
//!
//! All integers are big-endian.  A request is a single header followed by
//! length-prefixed fields; a response is a stream of frames terminated by
//! an `Exit` frame.  Clients may send `FRAME_CANCEL` mid-stream to ask the
//! daemon to terminate the running child.
//!
//! When `Request.debug` is true, the daemon emits `FRAME_LOG` frames with
//! per-request diagnostics before and during execution.
//!
//! When `Request.tty` is `Some`, the daemon allocates a pty and routes
//! stdin/stdout/stderr through it.  `Request.stdin_eof` says whether the
//! pty's stdin should be closed right after `Request.stdin` is written
//! (non-tty caller) or fed from `FRAME_STDIN` frames (interactive caller).

use std::io::{self, Read, Write};

pub const MAGIC: u32 = 0x5253_5544;
pub const VERSION: u16 = 6;

const MAX_ARGC: u32 = 1024;
const MAX_ARG_LEN: u32 = 1 << 20;
const MAX_STDIN_LEN: u32 = 16 << 20;
const MAX_FRAME_LEN: u32 = 16 << 20;
const MAX_ENV_COUNT: u32 = 4096;

pub const FRAME_STDOUT: u8 = 0x01;
pub const FRAME_STDERR: u8 = 0x02;
pub const FRAME_EXIT: u8 = 0x03;
pub const FRAME_ERROR: u8 = 0x04;
pub const FRAME_CANCEL: u8 = 0x05;
pub const FRAME_LOG: u8 = 0x06;
pub const FRAME_STDIN: u8 = 0x07;
pub const FRAME_RESIZE: u8 = 0x08;

#[derive(Debug, Clone, Copy)]
pub struct TtySpec {
    pub rows: u16,
    pub cols: u16,
}

#[derive(Debug, Clone)]
pub struct Request {
    pub argv: Vec<String>,
    pub cwd: String,
    pub env: Vec<(String, String)>,
    pub umask: u32,
    pub debug: bool,
    pub tty: Option<TtySpec>,
    pub stdin: Vec<u8>,
    pub stdin_eof: bool,
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

fn read_u32(r: &mut impl Read) -> io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_be_bytes(b))
}

fn read_sized(r: &mut impl Read, cap: u32) -> io::Result<Vec<u8>> {
    let len = read_u32(r)?;
    if len > cap {
        return Err(invalid("field too large"));
    }
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

fn read_string(r: &mut impl Read, cap: u32) -> io::Result<String> {
    String::from_utf8(read_sized(r, cap)?).map_err(|_| invalid("field not utf8"))
}

fn write_sized(w: &mut impl Write, data: &[u8]) -> io::Result<()> {
    w.write_all(&(data.len() as u32).to_be_bytes())?;
    w.write_all(data)
}

pub fn write_request(w: &mut impl Write, req: &Request) -> io::Result<()> {
    w.write_all(&MAGIC.to_be_bytes())?;
    w.write_all(&VERSION.to_be_bytes())?;

    w.write_all(&(req.argv.len() as u32).to_be_bytes())?;
    for a in &req.argv {
        write_sized(w, a.as_bytes())?;
    }
    write_sized(w, req.cwd.as_bytes())?;

    w.write_all(&(req.env.len() as u32).to_be_bytes())?;
    for (k, v) in &req.env {
        write_sized(w, k.as_bytes())?;
        write_sized(w, v.as_bytes())?;
    }

    w.write_all(&req.umask.to_be_bytes())?;
    w.write_all(&[req.debug as u8])?;

    match req.tty {
        Some(t) => {
            w.write_all(&[1u8])?;
            w.write_all(&t.rows.to_be_bytes())?;
            w.write_all(&t.cols.to_be_bytes())?;
        }
        None => w.write_all(&[0u8])?,
    }

    w.write_all(&[req.stdin_eof as u8])?;
    w.write_all(&(req.stdin.len() as u32).to_be_bytes())?;
    w.write_all(&req.stdin)?;
    w.flush()
}

pub fn read_request(r: &mut impl Read) -> io::Result<Request> {
    if read_u32(r)? != MAGIC {
        return Err(invalid("bad magic"));
    }
    let mut v = [0u8; 2];
    r.read_exact(&mut v)?;
    if u16::from_be_bytes(v) != VERSION {
        return Err(invalid("protocol version mismatch"));
    }

    let argc = read_u32(r)?;
    if argc > MAX_ARGC {
        return Err(invalid("argc too large"));
    }
    let mut argv = Vec::with_capacity(argc as usize);
    for _ in 0..argc {
        argv.push(read_string(r, MAX_ARG_LEN)?);
    }
    let cwd = read_string(r, MAX_ARG_LEN)?;

    let env_count = read_u32(r)?;
    if env_count > MAX_ENV_COUNT {
        return Err(invalid("env count too large"));
    }
    let mut env = Vec::with_capacity(env_count as usize);
    for _ in 0..env_count {
        let k = read_string(r, MAX_ARG_LEN)?;
        let v = read_string(r, MAX_ARG_LEN)?;
        env.push((k, v));
    }

    let umask = read_u32(r)?;
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    let debug = b[0] != 0;

    r.read_exact(&mut b)?;
    let tty = if b[0] != 0 {
        let mut rows_b = [0u8; 2];
        let mut cols_b = [0u8; 2];
        r.read_exact(&mut rows_b)?;
        r.read_exact(&mut cols_b)?;
        Some(TtySpec {
            rows: u16::from_be_bytes(rows_b),
            cols: u16::from_be_bytes(cols_b),
        })
    } else {
        None
    };

    r.read_exact(&mut b)?;
    let stdin_eof = b[0] != 0;
    let stdin = read_sized(r, MAX_STDIN_LEN)?;

    Ok(Request {
        argv,
        cwd,
        env,
        umask,
        debug,
        tty,
        stdin,
        stdin_eof,
    })
}

pub fn write_frame(w: &mut impl Write, kind: u8, data: &[u8]) -> io::Result<()> {
    w.write_all(&[kind])?;
    write_sized(w, data)
}

pub fn read_frame(r: &mut impl Read) -> io::Result<(u8, Vec<u8>)> {
    let mut b1 = [0u8; 1];
    r.read_exact(&mut b1)?;
    let data = read_sized(r, MAX_FRAME_LEN)?;
    Ok((b1[0], data))
}

pub fn write_exit(w: &mut impl Write, code: i32) -> io::Result<()> {
    write_frame(w, FRAME_EXIT, &code.to_be_bytes())
}

pub fn write_error(w: &mut impl Write, msg: &str) -> io::Result<()> {
    write_frame(w, FRAME_ERROR, msg.as_bytes())
}
