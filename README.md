# con

GPU 加速的终端模拟器，使用 Rust 编写。

本仓库是个人自用分支：上游的内置 AI 助手（Agent 面板、AI 补全、会话交接）已全部移除，只保留纯终端能力。

## 现状

| 平台 | 状态 |
|------|------|
| macOS | 主要开发与使用平台，已跑通内嵌 libghostty |
| Windows | 代码路径保留（libghostty-vt + ConPTY + D3D11），未验证 |
| Linux | 代码路径保留（libghostty-vt + Unix PTY + GPUI 自绘），未验证 |

## 功能

- 单窗口多标签页，垂直/水平标签栏
- 分屏（pane tree，任意嵌套）
- 内嵌 Ghostty 完整 VT 解析、回滚、Kitty 键盘协议、OSC 133 shell 集成
- 256 色 / 真彩色，Flexoki Light / Dark 主题
- 集成编辑器（语法高亮、LSP、Markdown 预览、文件树）
- 命令面板、快捷键自定义、工作区布局保存与恢复
- 会话持久化（标签页、cwd、shell 历史）
- `con-cli`：通过 Unix socket 暴露控制面（`tabs.*` / `panes.*` / `surfaces.*` / `tmux.*`）

## 架构

```
con (二进制, crates/con-app)
  ├── GPUI 外壳        窗口、标签页、分屏、编辑器、设置
  ├── con-core         配置、会话、控制面协议、终端状态
  ├── con-terminal     终端领域类型：pane 状态/控制/protocol、tmux、shell probe、主题
  ├── con-ghostty      平台终端后端（macOS 内嵌 libghostty）
  ├── con-paths        应用路径
  ├── con-process      OS 进程观测
  └── con-cli          socket 控制面客户端 + pty-bridge + ssh 辅助
```

约束：

- `con-terminal` 不依赖 UI，也不依赖任何终端后端实现
- `con-ghostty` 通过统一的 `GhosttyApp` / `GhosttyTerminal` 接口遮蔽平台差异
- `con-core` 不依赖 GPUI

## 环境要求

- Rust stable（edition 2024）
- `cmake`
- **Zig 0.16.0（精确版本）** —— `crates/con-ghostty/build.rs` 会校验，版本不符直接 panic

Zig 可通过 [mise](https://mise.jdx.dev/) 安装，或从 <https://ziglang.org/download/0.16.0/> 下载后加入 `PATH`，也可用 `CON_ZIG_BIN=/path/to/zig` 指定。

> 注意：仓库根目录已无 `mise.toml`，版本锁由使用者自行保证。

## 构建与运行

```bash
just build     # 构建 con + con-cli
just run       # 从源码运行（会先构建 con-cli）
just test      # 工作区测试
just check     # 类型检查
just lint      # clippy -D warnings
```

不使用 `just` 时：

```bash
cargo build -p con -p con-cli
cargo run -p con
cargo test --workspace
```

Windows 上 `CON` 是 DOS 保留设备名，须用 `.cargo/config.toml` 里的 `cargo w*` 别名：

```bash
cargo wbuild -p con     # → con-app.exe
cargo wrun -p con
```

`con-cli` 必须与 app 可执行文件放在同一目录，否则控制面无法启动。

首次构建会克隆 Ghostty 源码（`crates/con-ghostty/build.rs` 中固定 revision），并调用 Zig 编译完整 libghostty，耗时数分钟且需要能访问 GitHub。

## 配置

macOS: `~/Library/Application Support/con/con.conf`
Windows: `%APPDATA%\con-terminal\con-terminal.conf`

采用 Ghostty 行内语法，Con 自有配置放在 `con.*` 下：

```ini
font-family = "JetBrains Mono"
font-size = 14
theme = "flexoki-light"
cursor-style = block

con.appearance.tabs_orientation = "vertical"
con.keybindings.command_palette = "secondary-shift-p"
con.keybindings.new_tab = "secondary-t"
```

配置文件损坏时应用会进入恢复界面，不会覆盖原文件。

## 已知限制

- 构建依赖 GitHub 可达（首次拉取 Ghostty 源码）
- Windows / Linux 路径未在本分支验证
- `just` 中 `macos-*` / `linux-*` / `flatpak-*` 打包配方依赖已删除的 `scripts/`，不可用
- `cargo clippy -- -D warnings` 存在若干既有告警

## 上游

本项目派生自 [nowledge-co/con-terminal](https://github.com/nowledge-co/con-terminal)，移除了全部 AI 相关能力。上游采用 MIT 许可。

## 致谢

- [Ghostty](https://github.com/ghostty-org/ghostty) —— 终端运行时与渲染
- [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) —— GPU UI 框架
- [gpui-component](https://github.com/longbridge/gpui-component) —— 组件库
- [Phosphor Icons](https://phosphoricons.com/) —— 图标
- [Flexoki](https://stephango.com/flexoki) —— 默认配色
- [Iosevka](https://typeof.net/Iosevka/) / [Ioskeley Mono](https://github.com/jewlexx/ioskeley) —— 等宽字体
