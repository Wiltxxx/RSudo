# RSudo

[![TUR Package](https://img.shields.io/badge/TUR-rsudo-blueviolet)](https://github.com/termux-user-repository/tur)
[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

> A high-performance `sudo` alternative for Termux, written in Rust. Supports both direct `su` and a resident daemon mode.

`RSudo` allows you to execute commands as root, open interactive shells, and run scripts seamlessly in Termux / Android environments. It is fully compatible with Termux's path system and the CLI of various `su` implementations (Magisk / KernelSU / SuperSU, etc.), while offering an optional resident daemon to minimize invocation latency.

## 📦 Installation (via TUR)

The easiest way to install `rsudo` is through the [Termux User Repository (TUR)](https://github.com/termux-user-repository/tur):

```bash
pkg install tur-repo
pkg install rsudo
```

---

## Table of Contents

- [Features](#features)
- [Requirements](#requirements)
- [Build from Source](#build-from-source)
- [Quick Start](#quick-start)
- [Command Modes](#command-modes)
- [Path Resolution: Termux vs. Android](#path-resolution-termux-vs-android)
- [Environment Variable Export](#environment-variable-export)
- [Mount Master](#mount-master)
- [Daemon Mode](#daemon-mode)
- [Sudoers & Authorization](#sudoers--authorization)
- [Audit Logging](#audit-logging)
- [PTY Control](#pty-control)
- [Configuration File](#configuration-file)
- [Security Model](#security-model)
- [How It Works](#how-it-works)
- [Design Trade-offs](#design-trade-offs)
- [License](#license)

---

## Features

- **Dual Execution Modes:**
  - **Direct `su`:** No background processes, relies entirely on system `su`, ready to use out-of-the-box.
  - **Daemon Mode:** Auto-spawns a resident daemon on first use. Subsequent commands are executed directly via a Unix socket, bypassing repetitive Magisk authorization prompts and significantly reducing latency.
- **`su`-Compatible CLI:** Supports flags like `--mount-master`, `-c`, etc. (depending on the `su` implementation), with automatic capability detection and graceful degradation.
- **Dual Path Resolution (Termux / Android):** Choose between `-t` (Termux priority) and `-a` (Android priority). Finer control with `-T/-TT/-A/-AA` to lock the PATH scope.
- **Interactive & Script Modes:** Covers common scenarios with `rsudo su`, `rsudo asu`, `rsudo -i`, and `rsudo -s <script>`.
- **Script Generation & `--dry-run`:** View the actual shell script that will be executed for easier debugging and auditing.
- **UID-based Sudoers:** `allow <uid>` semantics equivalent to `user ALL=(ALL) ALL`.
- **Built-in Audit Logging:** Records the uid, pid, argv, and timestamp of every root execution.
- **PTY Support:** Automatically allocates a pty for commands requiring a tty; supports window resizing and stdin piping.
- **Secure IPC Protocol:** `SO_PEERCRED` client UID verification, `MAGIC` + version handshake, and length-prefixed frame validation.

---

## Requirements

- **Android Device** with one of the following installed:
  - [Magisk](https://github.com/topjohnwu/Magisk)
  - [KernelSU](https://github.com/tiann/KernelSU)
  - Other root solutions providing standard `su` CLI.
- **Termux** (recommended), or any environment capable of running Rust binaries.
- **Android SDK:** The code determines the Android-side PATH based on `ro.build.version.sdk` (defaults to SDK 28 if absent).
- **Rust Toolchain:** `cargo`, `rustc` (v1.70+ recommended; project uses `edition = 2021`).

---

## Build from Source

```bash
git clone https://github.com/Wiltxxx/Rsudo.git
cd Rsudo
cargo build --release
```

The binary size is highly optimized via the release profile in `Cargo.toml`:

```toml
[profile.release]
strip = true
lto = "fat"
opt-level = 3
panic = "abort"
codegen-units = 1
```

Install to `$PREFIX/bin`:

```bash
cp target/release/rsudo "$PREFIX/bin/rsudo"
chmod +x "$PREFIX/bin/rsudo"
```

Optional: Create a `sudo` symlink (Note: **Semantic differences** exist compared to system `su`; please familiarize yourself with `rsudo` first):

```bash
ln -sf "$PREFIX/bin/rsudo" "$PREFIX/bin/sudo"
```

---

## Quick Start

```bash
# Execute a single command as root
rsudo id

# Interactive root shell (Termux priority by default)
rsudo su

# Interactive root shell (Android priority)
rsudo asu

# Specify a shell
rsudo -i --shell zsh

# Execute a script snippet
rsudo -s 'echo hello from root; id'

# View the script to be executed (without running)
rsudo --dry-run --mount-master id
```

---

## Command Modes

`rsudo` determines the execution mode (`Kind`) based on the input:

| Input | Mode | Description |
|---|---|---|
| `rsudo <command> [args...]` | `Exec` | Parses and executes the command |
| `rsudo su` | `Su` | Interactive shell, **Termux priority** |
| `rsudo asu` | `Asu` | Interactive shell, **Android priority** |
| `rsudo -i [--shell SHELL]` | `Interactive` | Interactive shell |
| `rsudo -s <script> [args...]` | `Script` | Script mode |
| `rsudo -p <command> ...` | `Exec` | Forces the first arg as a path, disables `su`/`asu` detection |

### Relevant CLI Flags

| Flag | Description |
|---|---|
| `-i, --interactive` | Open an interactive shell |
| `-s, --script` | Script mode |
| `-p, --path` | Force command mode, disable `su`/`asu` keywords |
| `--shell <SHELL>` | Shell to use for interactive/script modes (default `bash`) |
| `--shell-home <PATH>` | Root user's HOME (default `$HOME/.suroot`) |
| `--dry-run` | Print the generated script without executing |
| `-v / -vv / -vvv` | Verbose logging (path decisions / mode & args / full script) |

> **Note:** If a command is provided in interactive mode, `-i` is ignored with a warning. In command mode, `--shell` is similarly ignored. Both warnings can be suppressed with `--force-path`.

---

## Path Resolution: Termux vs. Android

`rsudo` abstracts PATH strategies internally as `PathLayout`:

| Enum Value | Trigger Condition | PATH Content |
|---|---|---|
| `TermuxFull` | Default (command under `$ROOTFS`) or `-t` | Full Termux PATH |
| `TermuxMinimal` | `-T` | `$PREFIX/bin` + applets |
| `TermuxBinOnly` | `-TT` | Only `$PREFIX/bin` |
| `AndroidFull` | Default (command outside `$ROOTFS`) or `-a` | Full Android PATH |
| `AndroidSystemOnly` | `-A` | Only `/system/bin` |
| `AndroidBinOnly` | `-AA` | All Android bin directories |

### CLI Flags

| Flag | Action |
|---|---|
| `-t, --termux-priority` | Prioritize `$PREFIX/bin` |
| `-T, --termux-only` | Restrict to Termux scope; `-TT` restricts further |
| `-a, --android-priority` | Prioritize Android bin |
| `-A, --android-only` | Restrict to Android scope; `-AA` restricts further |

`-T/-A` uses `ArgAction::Count`, allowing repetition to grade the restriction.

### Command Resolution

In `Exec` mode, `rsudo` searches for the command in PATH based on the current `SearchMode`:
- `TermuxFirst`: `$PREFIX/bin` priority;
- `AndroidFirst`: Android bin priority;
- `AndroidOnly`: Search only in Android bin.

Upon successful resolution, it automatically determines the `PathLayout` based on whether the command falls under `$TERMUX__ROOTFS_DIR` (unless the user explicitly specified `-t/-a/-T/-A`).

> If `su --mount-master` is required (e.g., for `./su` operations), `rsudo` automatically degrades to the su path and appends `--mount-master` if it detects the root home is located in `/system/...`.

---

## Environment Variable Export

By default, `rsudo` constructs a clean root environment (`PATH`, `TERMUX__PREFIX`, `TERMUX__HOME`, `HOME`, `TMPDIR`, `TERM`, `LANG`, etc.). Use the following flags to append variables from the caller:

| Flag | Action |
|---|---|
| `-P, --export-all-path` | Append caller's `$PATH` to the generated `PATH` |
| `-L, --export-all-ld-lib-path` | Append caller's `$LD_LIBRARY_PATH` to the generated one |
| `--export-paths <PATHS>` | Append colon-separated paths to `PATH` |
| `--export-ld-lib-paths <PATHS>` | Append colon-separated paths to `LD_LIBRARY_PATH` |

---

## Mount Master

Certain scenarios (e.g., mount namespace operations) require `su`'s `--mount-master`.

| Flag | Action |
|---|---|
| `--mount-master` | Force enable |
| `--no-mount-master` | Force disable |

Default policy:
- **Interactive shell:** Usually enabled when `Termux` priority is set.
- **Exec mode:** If the root home is under `/system/...`, `mount::requires_su_path` will mandate `--mount-master`, thereby **disabling the daemon** and falling back to `su`.

---

## Daemon Mode

The daemon is the key to this project's performance. Once enabled, `rsudo` spawns a resident process (via `su`) on the first invocation. All subsequent commands communicate via a Unix socket, bypassing `su`'s startup and authorization latency.

### Lifecycle

```text
First call ──▶ Check socket ──▶ Not found → spawn daemon (su -c …)
│                 │
└───▶ Connect ◀─────┘
│
▼
Send Request / Receive Frame
```

- Single instance lock: `/data/local/tmp/rsudo.lock` (`flock`).
- Disabled marker: `/data/local/tmp/rsudo.disabled` (compatible with legacy `/data/adb/rsudo/disabled`).
- Socket: `/data/local/tmp/rsudo.sock`.
- Logs: `/data/local/tmp/rsudo-daemon.log`.
- Idle timeout: Unlimited by default, configurable via `--daemon-idle-timeout <SECS>`.

### Control Commands

| Flag | Action |
|---|---|
| `--daemon-boot` | Install boot script and start daemon immediately |
| `--daemon-enable` | Re-enable daemon (clears disabled marker) |
| `--daemon-disable` | Disable daemon, all future calls fall back to `su` |
| `--daemon-restart` | Restart daemon (reloads sudoers) |
| `--daemon-status` | Show daemon status (pid, socket, sudoers, audit, etc.) |

All control commands re-execute as root via `su -c '<exe> …'`, so you will only see **one** Magisk authorization prompt.

### When is the Daemon NOT used?

The following cases **automatically degrade** to the su path:
- `--daemon-disable` or disabled marker exists;
- `--dry-run` (daemon execs directly, no script generation);
- `Kind::Su` / `Kind::Asu`;
- `-i` / `--shell` explicitly specified;
- `--mount-master` required but root home is under `/system/...`;
- Connection fails and daemon startup times out.

### IPC Protocol

- `MAGIC = 0x5253_5544` ("RSUD"), `VERSION = 6`.
- Frame format: `[kind: u8][len: u32 BE][payload]`, `len` max 16 MiB.
- Request frames (Client → Daemon): `FRAME_CANCEL`, `FRAME_STDIN`, `FRAME_RESIZE` (`[rows: u16 BE][cols: u16 BE]`).
- Response frames (Daemon → Client): `FRAME_STDOUT`, `FRAME_STDERR`, `FRAME_EXIT` (`[rc: i32 BE]`), `FRAME_ERROR`, `FRAME_LOG`.

> Every connection undergoes `SO_PEERCRED` client UID verification.

---

## Sudoers & Authorization

Default path: `/data/adb/rsudo/sudoers.conf`.

**Format** (UID-based, equivalent to `user ALL=(ALL) ALL`):

```text
# Comment
allow 0
allow 10123
```

### Management Commands

| Flag | Action |
|---|---|
| `--sudoers-add <UID>` | Append `allow <uid>` (idempotent) |
| `--sudoers-del <UID>` | Delete the corresponding line |
| `--sudoers-list` | Print the current list |

After adding/removing, you will be prompted whether to `--daemon-restart` to reload.

### Load Validation

`Sudoers::load` validates the following:
- Opens with `O_NOFOLLOW`, rejects symlinks;
- Must be a regular file;
- Owner must be `root` (uid 0);
- Permissions must be `0600`.

Failure to meet any condition **refuses to start the daemon**.

---

## Audit Logging

Default path: `/data/adb/rsudo/audit.log`.

Every successful command executed via the daemon appends a line:

```text
2026-01-01T00:00:00Z uid=10123 pid=12345 argv=["id"]
```

View recent N entries:

```bash
rsudo --audit          # Default 20
rsudo --audit 100      # Recent 100
```

---

## PTY Control

Some commands (e.g., `ssh`, `top`, interactive TUIs) behave abnormally without a tty. The daemon path uses `PtyConfig` to decide whether to allocate a pty:
- `--edit-pty`: Open an interactive PTY configuration editor (mode + whitelist/blacklist).
- The client decides whether to enter a pty session based on `pty_cfg.decide(cmd)`.
- If the caller's stdin is **not a tty**, the client reads all stdin and sends it as `Request.stdin` to the daemon, which closes the pty input with **EOT** (`0x04`) to prevent hangs like `echo hi | rsudo cat`.
- In pty mode, window resizes are forwarded via `SIGWINCH` → `FRAME_RESIZE`.

> The daemon side disables `ECHO/ECHOE/ECHOK/ECHONL` on the pty when `stdin_eof = true` to prevent piped input from being double-echoed.

---

## Configuration File

Path: `$PREFIX/etc/rsudo.conf`.
Format: `key = value` per line, `#` for comments. Supported keys:

| Key | Action |
|---|---|
| `SUDO_SHELL_HOME` | Equivalent to `--shell-home` |
| `SUDO_SHELL` | Equivalent to `--shell` |
| `SUDO_ADDITIONAL_PATHS_TO_EXPORT` | Equivalent to `--export-paths` |
| `SUDO_ADDITIONAL_LD_LIBRARY_PATHS_TO_EXPORT` | Equivalent to `--export-ld-lib-paths` |
| `SUDO_LOG_LEVEL` | Equivalent to `-v` level |

Explicit CLI arguments take precedence.

---

## Security Model

`rsudo` aims to **simplify the root authorization experience**, not to add another layer of permission systems on top of `su`. Please note:
- **Sudoers are validated by UID.** Once a UID is added to `allow`, it can execute **any command** as root (equivalent to `ALL` in `sudo`).
- **The sudoers file must be owner=root, mode=0600**, otherwise loading is refused. Opening uses `O_NOFOLLOW` to prevent symlink replacement.
- **The daemon validates every connection via `SO_PEERCRED`**, but cannot prevent process impersonation within the same UID (an inherent semantic of `su`).
- **Audit logs** are for recording only, not for strong access control; the real gating is still on the daemon side (`Sudoers::is_allowed`).
- **The disabled marker** is in `/data/local/tmp`, readable by any process of this app; do not treat it as a security boundary.

For finer granularity, it is recommended to:
1. Only authorize necessary UIDs;
2. Restrict file permissions on the daemon socket directory;
3. Regularly audit `audit.log`;
4. Completely disable with `--daemon-disable` when not needed.

---

## How It Works

```text
┌──────────────────┐        ┌─────────────────────┐
│     rsudo CLI    │        │      rsudo daemon    │
│  (client)        │        │  (root, single)      │
│                  │        │                      │
│ 1. parse args    │        │ 1. bind /data/local/ │
│ 2. detect env    │◀──────▶│    tmp/rsudo.sock    │
│ 3. decide kind   │ socket │ 2. SO_PEERCRED       │
│ 4. build script  │        │ 3. Sudoers::is_allowed│
│    or request    │        │ 4. exec::run          │
│ 5. relay stdout/ │        │    ├─ pipe mode      │
│    stderr/exit   │        │    └─ pty mode       │
└──────────────────┘        └─────────────────────┘
│
▼
fallback: su -c '<script>'
```

### Key Modules
- `src/app.rs`: Top-level flow orchestration (load config → detect env → decide → execute).
- `src/cli.rs`: clap definitions.
- `src/decisions.rs`: `kind`, `PathLayout`, `SearchMode`, interactive detection.
- `src/env.rs`: Env var detection, SDK cache, path constants.
- `src/context.rs`: `TermuxEnv` (prefix / home / rootfs / sdk / cwd).
- `src/daemon/`: Client, server, protocol, execution, sudoers, lifecycle.
- `src/exec/`: `exec_file` / `interactive` / `script_mode` executors (su path).
- `src/script/`: Script generation (`env_block`, `quote`, `build_*`).
- `src/su.rs`: `su` capability detection (`--mount-master`, `-c`, `--preserve-environment`, etc.).
- `src/pty_config.rs`: pty decision (mode + whitelist/blacklist).
- `src/path.rs`: Command resolution and path comparison.

---

## Design Trade-offs

- **The daemon is an optional acceleration layer, not a required component.** Any command can run in su mode; the daemon only takes over when conditions are met.
- **`--dry-run` always uses su** because the daemon execs directly without generating printable scripts.
- **PTY is only provided in the daemon path**; the su path relies on `su`'s own tty behavior.
- **Sudoers are by UID, not by command:** Reduces management complexity at the cost of differing from `sudo`'s `Cmnd_Alias` behavior.
- **`--shell-home` is daemon-compatible:** The daemon automatically sets `HOME` to `t_env.root_home`, creates the directory with `ensure_home` if necessary, and `chown root:root`.

---

## License

This project is licensed under the **Apache License 2.0**. See [LICENSE](./LICENSE) and [NOTICE](./NOTICE) for details.

```text
rsudo
Copyright 2026 GappleX
Copyright 2026 Wiltxxx
```

---

## Related Links
- Termux: [https://termux.dev/](https://termux.dev/)
- Magisk: [https://github.com/topjohnwu/Magisk](https://github.com/topjohnwu/Magisk)
- KernelSU: [https://github.com/tiann/KernelSU](https://github.com/tiann/KernelSU)
- Apache 2.0: [https://www.apache.org/licenses/LICENSE-2.0](https://www.apache.org/licenses/LICENSE-2.0)

---

> README description based on current source (`VERSION = 6`). If you find discrepancies between behavior and documentation, feel free to open an issue or PR.
