# 参与 con 开发

`con` 的贡献者速查地图。

先读这些：
- `README.md` — 项目公开概览
- `AGENTS.md` — 开发约定
- `DESIGN.md` — 架构与产品方向
- `docs/README.md` — 文档索引

## 工作区地图

终端相关 crate 不依赖 UI，Agent crate 也不依赖特定的终端后端。

| Crate | 职责 |
|-------|------|
| `con` | GPUI 外壳：窗口、标签页、分屏、Agent UI、设置、命令界面 |
| `con-core` | 配置、会话、共享应用逻辑、harness 接线 |
| `con-terminal` | 跨后端共享的主题 + 调色板辅助 |
| `con-ghostty` | 各平台终端后端：macOS 内嵌 libghostty + Metal，Windows libghostty-vt + ConPTY + D3D11/DirectWrite，Linux Unix PTY + libghostty-vt + GPUI 自绘的 `StyledText` 渲染 |
| `con-agent` | Rig 集成、工具、钩子、对话、技能 |
| `con-cli` | CLI + 套接字客户端，用于本地实时控制面 |

每个平台都从 `con-ghostty` 暴露相同的 `GhosttyApp` / `GhosttyTerminal` /
`TerminalColors` 类型名，因此工作区其他部分消费后端时无需在每个调用点加 `cfg` 分支。
各平台方案及通向长期 GPU 加速网格渲染器的路径见 `docs/impl/{linux,windows}-port.md`。

## Agent 模型

- 每个窗口共享一个 Tokio 运行时
- 每个标签页有独立的 Agent 会话
- 工具与模型事件通过 channel 流向 UI
- Rig 的 `PromptHook` 驱动流式输出、审批和生命周期事件

完整拆解见 `docs/impl/agent-harness.md`。

## 前置条件

- Rust（stable，edition 2024）
- `cmake`
- **Zig**：完整终端构建必须使用 **精确的 Zig 0.16.0**。
  锁定的 Ghostty 版本要求此版本；更旧的 Zig 无法构建它，更新的版本可能再次改动构建 API。

### 使用 mise 快速配置（推荐）

如果你使用 [mise](https://mise.jdx.dev/)，仓库根目录的 `mise.toml` 已锁定精确的 Zig 版本。运行：

```bash
mise install              # 安装锁定的工具链
mise exec -- just build   # 使用 PATH 中的 Zig 0.16.0 运行
```

请通过 `mise exec --` 运行下面的 `just` 命令，这样即使未配置 mise shell 激活，锁定的工具也可用。例如
`mise exec -- just test` 会运行当前平台对应的测试集。如果你的 shell 已激活 mise，可以直接使用 `just`。

### 手动配置

如果你不使用 mise，请自行安装前置条件：

- **Zig**：从 `https://ziglang.org/download/0.16.0/` 下载官方 0.16.0 归档，将目录加入
  `PATH`，或设置 `CON_ZIG_BIN=/path/to/zig`。
- **macOS**：`cmake` 加 Zig 0.16.0。macOS 发布工作流会在构建内嵌 libghostty 前显式安装 Zig 0.16.0。
- **Windows**：Zig 0.16.0、Visual Studio 2022 Build Tools 及 Windows 10/11 SDK。请在
  _Developer Command Prompt for VS 2022_ 中运行完整构建，以确保 `rc.exe` 在 `PATH` 上。如果开启了 Windows Defender，请为仓库目录添加排除项或关闭实时扫描 —— Zig 的子构建可执行文件会被 MpEngine 短暂锁定，导致启动时报 `FileNotFound`。
- **Linux**：Zig 0.16.0，以及 CI 任务已安装的 GPUI 运行时 apt 依赖：
  ```sh
  sudo apt-get install -y --no-install-recommends \
    libxcb-composite0-dev libxcb-dri2-0-dev libxcb-glx0-dev \
    libxcb-present-dev libxcb-xfixes0-dev libxkbcommon-x11-dev \
    libwayland-dev libvulkan-dev libfreetype-dev libfontconfig1-dev \
    mesa-vulkan-drivers
  ```
  其中 `mesa-vulkan-drivers` 提供软件 ICD（llvmpipe），作为无头 / 虚拟机环境的回退方案；在有硬件 GPU 的真实桌面上可以跳过。

CI 刻意与此保持一致：
- `release-macos.yml`、`release-linux.yml` 和 `release-windows.yml` 会在发布构建前安装 Zig 0.16.0。
- macOS 应用打包会设置 `CON_GHOSTTY_PREFETCH_DEPS=1`，通过 curl/git 和本地 `zig fetch`
  填充 Zig 的包缓存后再构建。这规避了发布 CI 中可复现的 Zig 直接 Git 抓取失败。普通
  开发构建不做预取，会保留失败重试机制。诊断本地包问题时，可设置
  `CON_GHOSTTY_PREFETCH_DEPS=0` 退出预取。
- `ci-macos-ghostty.yml` 在构建路径发生变化时，会在 Intel runner 上构建原生 Ghostty 绑定。其手动触发的流水线也会构建未签名的完整发布二进制。
- `ci-portable.yml` 中的 Linux PR 冒烟检查也会安装 Zig 0.16.0，因为它要用 `libghostty-vt` 对 `con-ghostty` 做类型检查。
- Windows 和 Linux 的 PR 任务会构建并链接真实的 libghostty-vt 后端，然后运行 `con-ghostty` 测试。不要用 `CON_SKIP_GHOSTTY_VT` 或仅做检查的覆盖来替代：否则被删除的符号和调用约定漂移会一直隐藏到发布时才暴露。

## 构建

```bash
# macOS / Linux
cargo build

# Windows — 必须使用 `w*` 别名。默认的 `con` 二进制无法在 Windows 上存在，
# 因为 `CON` 是 DOS 保留设备名，所以 Windows 构建通过特性门控的别名 bin target
# 发布为 `con-app.exe`。`cargo wbuild` 等价于 `cargo build --no-default-features
# --features con/bin-con-app`；`wrun`、`wcheck`、`wtest` 与之对应。
cargo wbuild -p con --release          # → target\release\con-app.exe
```

如果你安装了 `just`，根目录的 `justfile` 封装了常见本地流程：

```bash
just build          # 当前平台的 debug 构建
just run            # 从源码运行
just test           # 当前平台对应的测试集
just check          # 快速类型检查
just install        # 构建并安装到当前平台的本地安装路径
```

在 Windows 上，这些默认配方通过上述 `cargo w*` 别名分发，因此会产出并运行 `con-app.exe`，而不是尝试构建保留名 `con.exe`。此外还有各平台的发布辅助命令，例如 `just channel=beta macos-release`、
`just channel=beta linux-release`、`just arch=x86_64 macos-bundle` 和
`just windows-build-release`。

## 运行

```bash
# macOS / Linux
cargo run -p con

# Windows
cargo wrun -p con
```

带可选参数：

```bash
RUST_LOG=con_agent::flow=info,con_agent=warn,con_core=warn,con::suggestions=debug,con_core::suggestions=debug cargo run -p con
```

### 可迁移的 macOS 调试预览版

普通 debug 构建会从源码检出目录读取 RustEmbed 资源。把原生 debug 应用复制到另一台同架构 Mac 之前，请先内嵌资源：

```sh
cargo build -p con --bin con --features rust-embed/debug-embed
```

这只会构建可执行文件，不是完整的已签名应用包。请把 `con-cli` 放在它旁边，另行准备应用包，并在测试主机上隔离配置与会话路径。传输后检查启动日志中是否有缺失资源。复制同架构二进制可以避免在共享 CI runner 上进行繁重构建。

## 测试

```bash
cargo test --workspace            # macOS / Linux
cargo wtest -p con-core -p con-cli -p con-agent -p con-terminal   # Windows（仅可移植 crate）
```

## 发布构建

```bash
cargo build --release -p con                  # macOS / Linux
cargo build --release -p con-cli              # 控制面 CLI
cargo wbuild -p con --release                 # Windows → target\release\con-app.exe
cargo build --release -p con-cli              # Windows → target\release\con-cli.exe
```

要生成已签名的 macOS 发布产物，请使用：

```bash
./scripts/macos/release.sh
```

macOS 应用包同时包含 `Contents/MacOS/con` 和
`Contents/MacOS/con-cli`；如果缺少 CLI，发布校验会失败。Homebrew cask 和 Unix 安装脚本会把包内的
`con-cli` 暴露到 PATH，这样 `pi-interactive-subagents` 之类的编排器就无需单独检出源码。

发布 CI 还有最终晋级门禁。各平台任务在上传前校验产物形态，而 `release-finalize.yml` 会让
GitHub Release 保持草稿状态，除非同一 tag 的所有预期产物、appcast 和
gh-pages 安装脚本均已就位。有问题的产物应在私下失败，而不是变成 `/releases/latest`。内部
`v*-dev.*` 冒烟 tag 属于预发布，永远不会更新公开的
stable/beta appcast 或 Homebrew cask，不内嵌 Sparkle feed URL，
且只校验产物与安装脚本形态。

要生成 Linux 发布 tarball（未签名；与 Windows
预览版的发行形态一致），请使用：

```bash
CON_RELEASE_VERSION=0.1.0-beta.X CON_RELEASE_CHANNEL=beta \
  ./scripts/linux/release.sh
```

输出位于 `dist/con-<version>-linux-<arch>.tar.gz`，旁边附带
SHA256 校验和。`.github/workflows/release-linux.yml` 中的 CI 工作流会在每个
`v*` tag 上运行同一脚本，把 tarball 附加到共享的 GitHub release，并
更新 `https://con-releases.nowledge.co/appcast/<channel>-linux-x86_64.xml`
处 Sparkle 形态的 appcast，供应用内仅通知型更新器轮询。

## 常用路径

- `crates/con-app/src` — 应用外壳与 GPUI 界面（目录名为 `con-app`，因为 `con` 是 Windows 上的 DOS 保留设备名；在 macOS 上 Cargo 包与二进制仍名为 `con`，所以 `cargo run -p con` 照旧可用 —— 见 `docs/impl/windows-port.md`）
- `crates/con-core/src` — 共享应用逻辑
- `crates/con-agent/src` — Agent 提供商、钩子、工具、技能
- `docs/design` — 设计交接集
- `docs/impl` — 实现说明
- `postmortem` — 问题复盘与经验教训
