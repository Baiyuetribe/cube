# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目定位

Rust 编写的 GPU 加速终端模拟器，基于 GPUI（Zed 的 UI 框架）+ 内嵌 Ghostty（终端运行时）。

**这是抹除 AI 能力后的个人自用分支**：上游的 Agent 面板、AI 补全、Agent Handoff、`con-agent` / `con-handoff` crate 已全部删除。不要在改动中重新引入 LLM 依赖或 `rig`。

主要开发验证平台是 macOS。Windows / Linux 代码路径保留但未验证。

## 常用命令

```bash
just build        # 构建 con + con-cli
just run          # 构建并运行（con-cli 必须与 app 同目录，故先构建它）
just test         # cargo test --workspace
just check        # cargo check --workspace
just lint         # cargo clippy --workspace -- -D warnings
just clean        # cargo clean
```

单测：

```bash
cargo test -p con-core config::tests::keybinding_conflicts_detect_duplicate_configured_shortcuts
cargo test -p con --bin con workspace::frame     # 按模块过滤
```

格式化（提交前必须通过）：

```bash
cargo fmt --all -- --check
```

Windows 上 `CON` 是 DOS 保留设备名，必须用 `.cargo/config.toml` 里的别名：

```bash
cargo wbuild -p con    # 等价于 --no-default-features --features con/bin-con-app，产出 con-app.exe
cargo wrun -p con
cargo wcheck -p con
cargo wtest
```

`justfile` 中的 `macos-*` / `linux-*` / `flatpak-*` / `windows-install` 配方依赖已删除的 `scripts/` 和 `packaging/`，不可用。

## 构建前置条件

**Zig 0.16.0，版本精确匹配。** `crates/con-ghostty/build.rs:require_zig_version` 会比对 `zig version` 输出，不符直接 panic。可用 `CON_ZIG_BIN=/abs/path/to/zig` 覆盖，或用 `CON_GHOSTTY_SOURCE_DIR` 指向本地 Ghostty 源码避免重复克隆。

首次构建需要访问 GitHub（克隆 `wey-gu/ghostty` 的固定 revision），然后调用 Zig 编译完整 libghostty，耗时数分钟。

## 架构

### Crate 依赖（单向，无环）

```
con-app (二进制 con)  → con-core, con-terminal, con-ghostty, con-paths
con-core              → con-terminal, con-paths
con-ghostty           → con-terminal, con-paths, con-process
con-terminal          → con-paths
con-cli               → con-core, con-terminal, con-paths, con-process
con-paths / con-process / con-test  → 无内部依赖
```

**边界约束，破坏会污染分层**：

- `con-terminal` 不依赖 UI，也不依赖任何终端后端实现。它是终端领域的类型层（pane 状态、控制协议、tmux、shell probe、主题）。
- `con-ghostty` 用统一接口遮蔽平台差异：macOS 内嵌完整 libghostty；Windows/Linux 用 libghostty-vt + 各自 PTY/渲染路径，通过 `#[cfg(target_os = ...)]` 分派。
- `con-core` 不依赖 GPUI。配置、会话、控制面协议、终端状态推导都在这里。
- `con-app` 是唯一的 UI 层，包名是 `con`（目录叫 `con-app` 只是因为 Windows 保留名）。

### con-app 的 workspace 拆分

`ConWorkspace`（`crates/con-app/src/workspace/mod.rs`）是核心结构体，约 90 个字段。它按职责拆成多个 `impl ConWorkspace` 文件：

| 文件 | 职责 |
|------|------|
| `lifecycle.rs` | 构造、事件泵、窗口生命周期 |
| `render.rs` + `render/` | 渲染主循环 |
| `control_requests.rs` | 控制面命令分派（`panes.*` / `tabs.*` 等） |
| `pane_control_requests.rs` | 终端 exec、pane 查询的实际执行 |
| `session_state.rs` | 会话快照与恢复 |
| `tab_actions.rs` / `pane_actions.rs` / `window_actions.rs` | 各类 action 处理器 |
| `types.rs` | `Tab` 等共享类型 |

改这些文件前先确认 `ConWorkspace` 的相关字段在 `mod.rs` 的哪个位置。

### 配置层

配置是**双层**的：

1. **文件层**（`con-core/src/config/ghostty.rs`）：Ghostty 行内语法 `key = value`，无损编辑——保留注释、空行、未知原生键的顺序。`NATIVE` 常量表把原生键名（如 `font-size`）映射到 Rust 字段路径（`terminal.font_size`）。
2. **结构层**（`con-core/src/config.rs`）：`Config` 结构体，serde 序列化。

Con 自有配置全部在 `con.*` 命名空间下。`config.rs` 中的 `normalize()` 负责清洗非法值。

**路径**：macOS 是 `~/Library/Application Support/con/con.conf`，Windows 是 `%APPDATA%\con-terminal\con-terminal.conf`（`con-paths` 里的 `APP_DIR_NAME` / `CONFIG_FILE_NAME` 决定）。

**重要**：已移除功能的旧键（`con.agent.*`、`con.skills.*`、`con.appearance.agent_avatar`、`con.keybindings.toggle_agent` 等）在 `ghostty.rs:is_retired_con_key` 里白名单放行并丢弃，保证升级不炸配置加载。新增已移除键时记得加进去。

### 控制面（con-cli ↔ app）

`con-core/src/control.rs` 定义 JSON-RPC over Unix socket（调试构建用 `/tmp/con-debug.sock`）。方法族：

- `system.identify` / `system.capabilities`
- `tabs.list` / `tabs.new` / `tabs.close`
- `panes.list` / `panes.read` / `panes.exec` / `panes.send_keys` / `panes.create` / `panes.wait` / `panes.probe_shell`
- `surfaces.*`（surface 是 pane 内的标签）
- `tmux.*`（inspect / list / capture / send_keys / run）
- `tree.get`

`con-cli` 是客户端。它还额外承担两个运行时职责，删不得：`pty-bridge`（Linux 上 `con-ghostty/linux/pty.rs` 主动探测并调用）和 `+ssh` / `+ssh-cache`（Ghostty shell 集成辅助）。

### 终端状态推导

`con-terminal/src/pane_state.rs` 是终端运行时状态机：`PaneRuntimeTracker` 归约三类输入（后端观测、shell probe、con 自身动作历史），产出 `PaneRuntimeState`。核心不变量：**无法证明的前台运行时必须报 `unknown`，不能把标题/屏幕启发式提升为事实**。

### 工作区布局持久化

`.con/workspace.ghostty`（v2，`WORKSPACE_LAYOUT_VERSION = 2`）。旧格式 `.con/workspace.toml` v1 只读兼容。

## 依赖版本约束

- `gpui-pre-*` 和 `gpui-component` / `gpui-kit-assets` **精确锁定（`=`)并成组升级**。混用两个 GPUI 快照会产生不兼容类型。
- `[patch.crates-io]` 里有指向 `wey-gu/zed` 的个人 fork（固定 revision），用于 macOS 原生呈现修复。不要替换成不兼容的官方 Zed 包。

## UI 约束

- 优先用 `gpui-component` 的控件，而不是手写按钮/输入框/下拉。
- 终端与代码上下文用等宽字体（IoskeleyMono），散文/设置用系统 UI 字体。终端后端必须传**具体字体族名**，不能传 GPUI 别名如 `.SystemUIFont`。
- 默认 Flexoki Light，支持 Flexoki Dark。Con 自绘表面无边框无阴影，用透明度填充分层。
- 图标用 `assets/icons/phosphor/`，不要手绘替代品。每个 SVG 都要显式 `.text_color()`。

## 已知问题

- `cargo clippy -- -D warnings` 有若干既有告警（`chunks_exact` 常量参数、`useless_conversion` 等），非本次改动引入。
- `con-process::macos::tests::observes_current_process_and_group` 在 `target` 是符号链接（mbx 缓存）时必然失败，因为它比对 `current_exe()` 与进程记录的路径。
