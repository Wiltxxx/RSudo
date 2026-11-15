@ rsudo

@> 为 Termux 打造的高性能 {{sudo}} 替代品 —— Rust 实现，支持 su 直连与常驻守护进程两种模式。

{{rsudo}} 让你在 Termux / Android 环境下以 root 身份执行命令、打开交互式 shell、运行脚本。它完全兼容 Termux 的路径体系与 {{su}}（Magisk / KernelSU / SuperSU 等）的 CLI，同时提供一个可选的常驻守护进程，把每次调用的延迟压到最低。

@--

@@ 目录

@- %[特性](#特性)
@- %[环境要求](#环境要求)
@- %[构建与安装](#构建与安装)
@- %[快速开始](#快速开始)
@- %[命令模式](#命令模式)
@- %[路径解析：Termux 还是 Android？](#路径解析termux-还是-android)
@- %[环境变量导出](#环境变量导出)
@- %[Mount master](#mount-master)
@- %[守护进程（daemon）](#守护进程daemon)
@- %[Sudoers 授权](#sudoers-授权)
@- %[审计日志](#审计日志)
@- %[PTY 控制](#pty-控制)
@- %[配置文件](#配置文件)
@- %[安全模型](#安全模型)
@- %[工作原理](#工作原理)
@- %[设计取舍](#设计取舍)
@- %[许可证](#许可证)

@--

@@ 特性

@- %%两种执行模式%%
@- %%su 直连%%：不需要后台进程，完全依赖系统 {{su}}，随时可用。
@- %%daemon 常驻%%：首次调用自动拉起守护进程，之后通过 Unix socket 直接执行，无需每次弹 Magisk 授权框，显著降低延迟。
@- %%与 {{su}} 兼容的命令行%%：支持 {{--mount-master}}、{{-c}} 等参数（视 {{su}} 实现而定），自动探测能力并降级。
@- %%Termux / Android 双路径体系%%：可选 {{-t}}（Termux 优先）、{{-a}}（Android 优先），并通过 {{-T/-TT/-A/-AA}} 精确锁定 PATH 范围。
@- %%交互式与脚本模式%%：{{rsudo su}}、{{rsudo asu}}、{{rsudo -i}}、{{rsudo -s <script>}} 覆盖常见场景。
@- %%脚本生成与 {{--dry-run}}%%：可查看实际运行的 shell 脚本，方便调试与审计。
@- %%按 uid 授权的 sudoers%%：{{allow <uid>}}，与 {{user ALL=(ALL) ALL}} 语义等价。
@- %%内置审计日志%%：记录每次 root 执行的 uid、pid、argv 与时间戳。
@- %%PTY 支持%%：对需要 tty 的命令自动分配 pty；支持窗口大小调整、stdin 管道输入。
@- %%安全的进程间协议%%：{{SO_PEERCRED}} 校验客户端 uid，{{MAGIC}} + 版本号握手，帧格式长度前缀校验。

@--

@@ 环境要求

@- %%Android 设备%%，已安装以下之一：
@- %[Magisk](https://github.com/topjohnwu/Magisk)
@- %[KernelSU](https://github.com/tiann/KernelSU)
@- 其他提供标准 {{su}} CLI 的 root 方案
@- %%Termux%%（推荐），或任何能运行 Rust 编译产物的 Termux 环境。
@- %%Android SDK%%：代码根据 {{ro.build.version.sdk}} 选择 Android 侧 PATH，缺省按 SDK 28 处理。
@- %%Rust 工具链%%：{{cargo}}、{{rustc}}（建议 1.70+；项目使用 {{edition = 2021}}）。

@--

@@ 构建与安装

@code bash
git clone https://github.com/<your>/rsudo.git
cd rsudo
cargo build --release
@endcode

产物体积已通过 {{Cargo.toml}} 中的发布配置优化：

@code toml
[profile.release]
strip = true
lto = "fat"
opt-level = 3
panic = "abort"
codegen-units = 1
@endcode

安装到 {{$PREFIX/bin}}：

@code bash
cp target/release/rsudo "$PREFIX/bin/rsudo"
chmod +x "$PREFIX/bin/rsudo"
@endcode

可选：创建 {{sudo}} 软链（注意 %%与系统 {{su}} 的语义差异%%，建议先熟悉 {{rsudo}}）：

@code bash
ln -sf "$PREFIX/bin/rsudo" "$PREFIX/bin/sudo"
@endcode

@--

@@ 快速开始

@code bash
# 以 root 执行单条命令
rsudo id

# 交互式 root shell（默认 Termux 优先）
rsudo su

# 交互式 root shell（Android 优先）
rsudo asu

# 指定 shell
rsudo -i --shell zsh

# 执行一段脚本
rsudo -s 'echo hello from root; id'

# 查看将要执行的脚本（不实际运行）
rsudo --dry-run --mount-master id
@endcode

@--

@@ 命令模式

{{rsudo}} 会根据输入自行判定运行模式（{{Kind}}）：

@| 输入 | 模式 | 说明 |
@|---|---|---|
@| {{rsudo <command> [args...]}} | {{Exec}} | 解析并执行命令 |
@| {{rsudo su}} | {{Su}} | 交互式 shell，%%Termux 优先%% |
@| {{rsudo asu}} | {{Asu}} | 交互式 shell，%%Android 优先%% |
@| {{rsudo -i [--shell SHELL]}} | {{Interactive}} | 交互式 shell |
@| {{rsudo -s <script> [args...]}} | {{Script}} | 脚本模式 |
@| {{rsudo -p <command> ...}} | {{Exec}} | 强制把第一个参数当路径，禁用 {{su}}/{{asu}} 识别 |

@@@ 相关 CLI 参数

@| 参数 | 说明 |
@|---|---|
@| {{-i, --interactive}} | 打开交互式 shell |
@| {{-s, --script}} | 脚本模式 |
@| {{-p, --path}} | 强制命令模式，禁用 {{su}}/{{asu}} 关键字 |
@| {{--shell <SHELL>}} | 交互/脚本模式使用的 shell（默认 {{bash}}） |
@| {{--shell-home <PATH>}} | root 用户的 HOME（默认 {{$HOME/.suroot}}） |
@| {{--dry-run}} | 打印生成的脚本而不执行 |
@| {{-v / -vv / -vvv}} | 详细日志（路径决策 / 模式与参数 / 完整脚本） |

@> %%注意%%：交互式模式下如果同时给了命令，{{-i}} 会被忽略并给出提示；命令模式下 {{--shell}} 同样被忽略。两种提示都可用 {{--force-path}} 关闭。

@--

@@ 路径解析：Termux 还是 Android？

{{rsudo}} 内部把 PATH 策略抽象为 {{PathLayout}}：

@| 枚举值 | 触发条件 | PATH 内容 |
@|---|---|---|
@| {{TermuxFull}} | 默认（命令位于 {{$ROOTFS}} 下）或 {{-t}} | Termux 全量 PATH |
@| {{TermuxMinimal}} | {{-T}} | {{$PREFIX/bin}} + applets |
@| {{TermuxBinOnly}} | {{-TT}} | 仅 {{$PREFIX/bin}} |
@| {{AndroidFull}} | 默认（命令位于 {{$ROOTFS}} 外）或 {{-a}} | Android 全量 PATH |
@| {{AndroidSystemOnly}} | {{-A}} | 仅 {{/system/bin}} |
@| {{AndroidBinOnly}} | {{-AA}} | 全部 Android bin 目录 |

@@@ 命令行参数

@| 参数 | 作用 |
@|---|---|
@| {{-t, --termux-priority}} | 优先 {{$PREFIX/bin}} |
@| {{-T, --termux-only}} | 收紧为 Termux 范围；{{-TT}} 进一步收紧 |
@| {{-a, --android-priority}} | 优先 Android bin |
@| {{-A, --android-only}} | 收紧为 Android 范围；{{-AA}} 进一步收紧 |

{{-T/-A}} 使用 {{ArgAction::Count}}，可通过重复参数分级。

@@@ 命令解析

{{Exec}} 模式下，{{rsudo}} 会按当前 {{SearchMode}} 在 PATH 中查找命令：

@- {{TermuxFirst}}：{{$PREFIX/bin}} 优先；
@- {{AndroidFirst}}：Android bin 优先；
@- {{AndroidOnly}}：只在 Android bin 中查找。

解析成功后，会根据命令是否落在 {{$TERMUX__ROOTFS_DIR}} 下自动决定 {{PathLayout}}（除非用户显式指定了 {{-t/-a/-T/-A}}）。

@> 若 {{su}} 的 {{--mount-master}} 是必需的（例如执行 {{./su}} 相关操作），{{rsudo}} 会在检测到 root home 位于 {{/system/...}} 时自动降级到 su 路径，并加 {{--mount-master}}。

@--

@@ 环境变量导出

{{rsudo}} 默认构造一份干净的 root 环境（{{PATH}}、{{TERMUX__PREFIX}}、{{TERMUX__HOME}}、{{HOME}}、{{TMPDIR}}、{{TERM}}、{{LANG}} 等）。通过以下参数追加来自调用者的环境：

@| 参数 | 作用 |
@|---|---|
@| {{-P, --export-all-path}} | 把调用者 {{$PATH}} 追加到生成的 {{PATH}} |
@| {{-L, --export-all-ld-lib-path}} | 把调用者 {{$LD_LIBRARY_PATH}} 追加到生成的 {{LD_LIBRARY_PATH}} |
@| {{--export-paths <PATHS>}} | 额外冒号分隔路径追加到 {{PATH}} |
@| {{--export-ld-lib-paths <PATHS>}} | 额外冒号分隔路径追加到 {{LD_LIBRARY_PATH}} |

@--

@@ Mount master

某些场景（例如挂载命名空间相关的操作）需要 {{su}} 的 {{--mount-master}}。

@| 参数 | 作用 |
@|---|---|
@| {{--mount-master}} | 强制开启 |
@| {{--no-mount-master}} | 强制关闭 |

默认策略：

@- 交互式 shell：{{Termux}} 优先级时通常启用；
@- {{Exec}} 模式：如果 root home 位于 {{/system/...}}，{{mount::requires_su_path}} 会要求使用 {{--mount-master}}，并因此%%禁用 daemon%%，回退到 su。

@--

@@ 守护进程（daemon）

daemon 是本项目性能的关键。启用后，{{rsudo}} 在首次调用时（通过 {{su}}）拉起一个常驻进程，之后所有命令都走 Unix socket，绕开 {{su}} 的启动与授权延迟。

@@@ 生命周期

@code text
首次调用 ──▶ 检测 socket ──▶ 不存在 → spawn daemon（su -c …）
│                 │
└───▶ 连接 ◀─────┘
│
▼
发送 Request / 接收 Frame
@endcode

@- 单实例锁：{{/data/local/tmp/rsudo.lock}}（{{flock}}）。
@- 禁用标记：{{/data/local/tmp/rsudo.disabled}}（兼容旧路径 {{/data/adb/rsudo/disabled}}）。
@- socket：{{/data/local/tmp/rsudo.sock}}。
@- 日志：{{/data/local/tmp/rsudo-daemon.log}}。
@- 空闲超时：默认无限制，可通过 {{--daemon-idle-timeout <SECS>}} 设置。

@@@ 控制命令

@| 参数 | 作用 |
@|---|---|
@| {{--daemon-boot}} | 安装启动脚本并立即启动 daemon |
@| {{--daemon-enable}} | 重新启用 daemon（清除 disabled 标记） |
@| {{--daemon-disable}} | 禁用 daemon，之后的调用全部回退 su 路径 |
@| {{--daemon-restart}} | 重启 daemon（重新加载 sudoers） |
@| {{--daemon-status}} | 显示 daemon 状态（pid、socket、sudoers、audit 等） |

所有控制命令都通过 {{su -c '<exe> …'}} 重新以 root 执行，因此只会看到%%一次%% Magisk 授权提示。

@@@ daemon 何时不使用

以下情况会%%自动降级%%到 su 路径：

@- {{--daemon-disable}} 或已存在 disabled 标记；
@- {{--dry-run}}（daemon 直接 exec，不生成脚本）；
@- {{Kind::Su}} / {{Kind::Asu}}；
@- {{-i}} / {{--shell}} 显式指定；
@- {{--mount-master}} 必需但 root home 位于 {{/system/...}}；
@- 连接失败且 daemon 启动超时。

@@@ 通信协议

@- {{MAGIC = 0x5253_5544}}（"RSUD"），{{VERSION = 6}}。
@- 帧格式：{{[kind: u8][len: u32 BE][payload]}}，{{len}} 上限 16 MiB。
@- 请求帧类型（客户端 → daemon）：
@- {{FRAME_CANCEL}}：取消当前子进程；
@- {{FRAME_STDIN}}：喂给子进程的 stdin；
@- {{FRAME_RESIZE}}：{{[rows: u16 BE][cols: u16 BE]}}，pty 窗口尺寸变更。
@- 响应帧类型（daemon → 客户端）：
@- {{FRAME_STDOUT}} / {{FRAME_STDERR}}：流式输出；
@- {{FRAME_EXIT}}：{{[rc: i32 BE]}}；
@- {{FRAME_ERROR}}：错误文本；
@- {{FRAME_LOG}}：{{--debug}} 调试日志。

@> 每次连接都会执行 {{SO_PEERCRED}} 校验客户端 uid。

@--

@@ Sudoers 授权

默认路径：{{/data/adb/rsudo/sudoers.conf}}。

%%格式%%（按 uid 授权，语义等价于 {{user ALL=(ALL) ALL}}）：

@code text
# 注释
allow 0
allow 10123
@endcode

@@@ 管理命令

@| 参数 | 作用 |
@|---|---|
@| {{--sudoers-add <UID>}} | 追加 {{allow <uid>}}（幂等） |
@| {{--sudoers-del <UID>}} | 删除对应行 |
@| {{--sudoers-list}} | 打印当前列表 |

添加 / 删除后会提示是否需要 {{--daemon-restart}} 重新加载。

@@@ 加载校验

{{Sudoers::load}} 会校验：

@- {{O_NOFOLLOW}} 打开，拒绝符号链接；
@- 必须是普通文件；
@- owner 必须是 {{root}}（uid 0）；
@- 权限必须是 {{0600}}。

任何不满足都会%%拒绝启动 daemon%%。

@--

@@ 审计日志

默认路径：{{/data/adb/rsudo/audit.log}}。

每次成功通过 daemon 执行的命令都会追加一行：

@code text
2026-01-01T00:00:00Z uid=10123 pid=12345 argv=["id"]
@endcode

查看最近 N 条：

@code bash
rsudo --audit          # 默认 20 条
rsudo --audit 100      # 最近 100 条
@endcode

@--

@@ PTY 控制

部分命令（例如 {{ssh}}、{{top}}、交互式 TUI）在非 tty 下行为异常。daemon 路径通过 {{PtyConfig}} 决定是否为某条命令分配 pty：

@- {{--edit-pty}}：打开交互式 PTY 配置编辑器（模式 + 白/黑名单）。
@- 客户端根据 {{pty_cfg.decide(cmd)}} 决定是否进入 pty 会话。
@- 若调用者 stdin %%不是 tty%%，客户端会把 stdin 全部读入，作为 {{Request.stdin}} 发给 daemon，并由 daemon 以 %%EOT%%（{{0x04}}）关闭 pty 输入，避免 {{echo hi | rsudo cat}} 之类的管道 hang 住。
@- pty 模式下窗口大小变化通过 {{SIGWINCH}} → {{FRAME_RESIZE}} 转发。

@> daemon 侧在 {{stdin_eof = true}} 时会关闭 pty 的 {{ECHO/ECHOE/ECHOK/ECHONL}}，防止管道输入被回显成双份。

@--

@@ 配置文件

路径：{{$PREFIX/etc/rsudo.conf}}

每行 {{key = value}}，{{#}} 开始为注释。支持的键（含 {{SUDO_}} 与 {{SUDO__}} 两种前缀）：

@| 键 | 作用 |
@|---|---|
@| {{SUDO_SHELL_HOME}} | 等价 {{--shell-home}} |
@| {{SUDO_SHELL}} | 等价 {{--shell}} |
@| {{SUDO_ADDITIONAL_PATHS_TO_EXPORT}} | 等价 {{--export-paths}} |
@| {{SUDO_ADDITIONAL_LD_LIBRARY_PATHS_TO_EXPORT}} | 等价 {{--export-ld-lib-paths}} |
@| {{SUDO_LOG_LEVEL}} | 等价 {{-v}} 的级别 |

CLI 显式参数优先级更高。

@--

@@ 安全模型

{{rsudo}} 的目标是%%简化 root 授权体验%%，而不是在 su 之上再加一层权限系统。请理解：

@- %%sudoers 校验按 uid%%，一旦某个 uid 被加入 {{allow}}，它就能以 root 执行%%任意命令%%（与 {{sudo}} 的 {{ALL}} 等价）。
@- %%sudoers 文件必须 owner=root、mode=0600%%，否则拒绝加载；打开时使用 {{O_NOFOLLOW}}，避免被替换成符号链接。
@- %%daemon 会 SO_PEERCRED 校验%%每个连接的 uid，但无法防止同一 uid 内部进程冒充（这是 su 的固有语义）。
@- %%审计日志%%只是记录，不构成强访问控制；真正的 gating 仍然在 daemon 侧（{{Sudoers::is_allowed}}）。
@- %%disabled 标记%%在 {{/data/local/tmp}}，任何本进程都可读；不要把它当成安全边界。

如果需要更细的粒度，建议：

@1. 只给必要 uid 授权；
@2. 对 daemon socket 目录做文件权限约束；
@3. 定期审计 {{audit.log}}；
@4. 不需要时用 {{--daemon-disable}} 完全关闭。

@--

@@ 工作原理

@code text
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
@endcode

@@@ 关键模块

@- {{src/app.rs}}：顶层流程编排（加载配置 → 检测环境 → 决策 → 执行）。
@- {{src/cli.rs}}：clap 定义。
@- {{src/decisions.rs}}：{{kind}}、{{PathLayout}}、{{SearchMode}}、交互式判定。
@- {{src/env.rs}}：环境变量探测、SDK 缓存、各类路径常量。
@- {{src/context.rs}}：{{TermuxEnv}}（prefix / home / rootfs / sdk / cwd）。
@- {{src/daemon/}}：客户端、服务端、协议、执行、sudoers、生命周期。
@- {{src/exec/}}：{{exec_file}} / {{interactive}} / {{script_mode}} 三种执行器（su 路径）。
@- {{src/script/}}：脚本生成（{{env_block}}、{{quote}}、{{build_*}}）。
@- {{src/su.rs}}：{{su}} 能力探测（{{--mount-master}}、{{-c}}、{{--preserve-environment}} 等）。
@- {{src/pty_config.rs}}：pty 决策（模式 + 白/黑名单）。
@- {{src/path.rs}}：命令解析与路径比较。

@--

@@ 设计取舍

@- %%daemon 是可选加速层，不是必需组件%%。任何命令都能在 su 模式下跑通；daemon 只在满足条件时接管。
@- %%{{--dry-run}} 永远走 su%%，因为 daemon 直接 exec，不生成可打印脚本。
@- %%PTY 仅在 daemon 路径提供%%；su 路径依赖 {{su}} 自身的 tty 行为。
@- %%sudoers 按 uid 而非按命令%%：降低管理复杂度，代价是与 {{sudo}} 的 {{Cmnd_Alias}} 行为不同。
@- %%{{--shell-home}} 与 daemon 兼容%%：daemon 会自动把 {{HOME}} 设为 {{t_env.root_home}}，并在必要时 {{ensure_home}} 创建目录、改 {{chown root:root}}。

@--

@@ 许可证

本项目使用 %%Apache License 2.0%% 发布，详见 %[LICENSE](./LICENSE) 与 %[NOTICE](./NOTICE)。

@code text
rsudo
Copyright 2026 GappleX
Copyright 2026 Wiltxxx
@endcode

@--

@@ 相关链接

@- Termux：%[https://termux.dev/](https://termux.dev/)
@- Magisk：%[https://github.com/topjohnwu/Magisk](https://github.com/topjohnwu/Magisk)
@- KernelSU：%[https://github.com/tiann/KernelSU](https://github.com/tiann/KernelSU)
@- Apache 2.0：%[https://www.apache.org/licenses/LICENSE-2.0](https://www.apache.org/licenses/LICENSE-2.0)

@--

@> README 描述基于当前源码（{{VERSION = 6}}）。如果你发现行为与文档不符，欢迎提 issue 或 PR。
