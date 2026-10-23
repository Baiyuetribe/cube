# con：内置 AI 助手的原生终端模拟器

## 愿景

一个开源、GPU 加速的终端模拟器，把 AI Agent 视为一等公民。它追求高度的终端正确性、原生性能和深度集成的 Agent 助手 —— 全部用 Rust 实现。

**con 为何存在：**

- 现有终端把 AI 当成事后补丁
- Agent 原生工作流值得深度终端集成，而不是包装器式的权宜之计
- 终端是 AI 时代最后一个尚未被重新发明的无 IDE 界面

---

## 架构总览

```
┌─────────────────────────────────────────────────────────┐
│                     con（二进制）                        │
│                                                         │
│  ┌─────────────┐  ┌──────────────┐  ┌───────────────┐  │
│  │  GPUI 外壳   │  │  Agent 面板   │  │  命令栏        │  │
│  │（窗口、      │  │（对话、工具    │  │（面板、        │  │
│  │  标签页、    │  │  调用、       │  │  搜索、        │  │
│  │  分屏）      │  │  上下文）     │  │  操作）        │  │
│  └──────┬───────┘  └──────┬───────┘  └──────┬────────┘  │
│         │                 │                  │           │
│  ┌──────┴─────────────────┴──────────────────┴────────┐  │
│  │                    con-core                        │  │
│  │  ┌────────────┐ ┌────────────┐ ┌────────────────┐  │  │
│  │  │ 终端        │ │ Agent      │ │ 会话            │  │  │
│  │  │ 管理器      │ │ 助手        │ │ 管理器          │  │  │
│  │  │            │ │（共享）     │ │                │  │  │
│  │  │            │ │ + 每标签页  │ │                │  │  │
│  │  │            │ │   会话      │ │                │  │  │
│  │  └─────┬──────┘ └─────┬──────┘ └────────────────┘  │  │
│  └────────┼──────────────┼────────────────────────────┘  │
│           │              │                               │
│  ┌────────┴────────┐  ┌──┴──────────────┐               │
│  │ con-ghostty     │  │ con-agent       │               │
│  │（平台终端        │  │（rig 0.34，    │               │
│  │  后端）          │  │  多提供商）     │               │
│  │                 │  │                 │               │
│  ├─────────────────┤  └─────────────────┘               │
│  │ con-terminal    │                                    │
│  │（主题 +         │                                    │
│  │  调色板数据）    │                                    │
│  └─────────────────┘                                    │
└─────────────────────────────────────────────────────────┘
```

---

## Crate 结构

```
kingston/
├── Cargo.toml                 # 工作区根
├── crates/
│   ├── con/                   # 主二进制 —— GPUI 应用外壳
│   │   └── src/
│   │       ├── main.rs         # 应用启动 + 快捷键绑定
│   │       ├── workspace/      # 窗口、标签页、分屏、渲染、会话接线
│   │       ├── ghostty_view.rs  # 内嵌 Ghostty surface 视图
│   │       ├── agent_panel.rs  # AI 对话 / 工具输出的侧面板
│   │       ├── input_bar.rs    # 智能输入栏（NLP/shell/技能模式）
│   │       ├── pane_tree.rs    # 分屏布局树
│   │       ├── settings_panel.rs # 提供商配置 UI（Cmd+,）
│   │       ├── sidebar.rs      # 垂直标签页侧边栏（轨道 / 悬停速览 / 固定）
│   │       └── theme.rs        # Flexoki 主题（默认 Light，可用 Dark）
│   │
│   ├── con-core/              # 共享逻辑，不依赖 UI
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── harness.rs     # AgentHarness（共享）+ AgentSession（每标签页）
│   │       ├── session.rs     # 持久化 / 恢复工作区状态
│   │       ├── config.rs      # con 配置（TOML）
│   │       └── suggestions.rs # AI shell 命令建议
│   │
│   ├── con-terminal/          # 终端主题与调色板辅助
│   │   └── src/
│   │       ├── lib.rs
│   │       └── theme.rs       # 内置主题 + Ghostty 主题导入
│   │
│   ├── con-ghostty/           # ghostty FFI —— macOS 主后端
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── ffi.rs         # C API 绑定（libghostty 类型、函数）
│   │       └── terminal.rs    # GhosttyApp、GhosttyTerminal、TerminalState、动作回调
│   │
│   ├── con-agent/             # AI Agent 助手（Rig 0.40）
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── provider.rs    # 多提供商 Rig Agent 构建与流式输出
│   │       ├── hook.rs        # ConHook —— AgentHook 生命周期桥接
│   │       ├── tools.rs       # 终端、tmux、文件、工作区的 Rig Tool 实现
│   │       ├── context.rs     # 为 Agent 提取终端上下文
│   │       ├── conversation.rs # 对话状态 + Rig Message 转换
│   │       └── skills.rs      # 技能注册表 + AGENTS.md 解析器
│   │
│   └── con-cli/               # 无头 CLI + 套接字客户端（桩）
│       └── src/main.rs
│
├── postmortem/                # 事故与集成复盘
│
└── docs/
    ├── study/                 # 技术研究笔记
    └── impl/                  # 实现笔记
```

工作区模块归属图见 `docs/impl/workspace-modules.md`。

---

## 关键技术决策

### 1. UI 框架：GPUI

**为什么选 GPUI 而非其他方案（Tauri、Iced、egui、Slint）：**

| 评判标准 | GPUI | Tauri | Iced | egui |
|----------|------|-------|------|------|
| GPU 渲染 | Metal + Blade（OpenGL） | WebView | wgpu | wgpu |
| 文本质量 | 生产级（Zed 水准） | Web 字体 | 良好 | 基础 |
| 当前产品目标 | macOS 原生应用 | 全平台 | 全平台 | 全平台 |
| 成熟度 | Zed 生产环境 + 23+ 款应用 | 非常成熟 | 成长中 | 成熟 |
| 终端画布 | canvas() API | DOM 技巧 | 自定义控件 | 自绘 |
| Rust 原生 | 是 | JS/Rust 混合 | 是 | 是 |

GPUI 提供了 Zed 级的文本渲染质量（对终端至关重要）以及经过验证的 canvas API 用于自定义网格绘制。awesome-gpui 中的 `termy` 项目已证明终端嵌入可行。

### 2. 终端核心：纯 Ghostty 运行时

con 现在只运行一个终端运行时：通过其 C API 嵌入的完整 Ghostty。

- Ghostty 负责 PTY 生命周期、VT 解析、回滚缓冲和渲染
- con 在每个窗口内嵌一个共享的 Ghostty 应用，并在 GPUI 布局中为每个分屏内嵌一个 Ghostty surface
- `con-ghostty` 保持轻薄：应用生命周期、surface 生命周期、回调，以及安全的 Rust 访问器
- `con-terminal` 现在只负责驱动 Ghostty 配置所需的主题与调色板数据

Ghostty 原生分屏并不意味着单一的整体 surface。Ghostty 自己的 macOS 架构同样使用宿主管理的 surface 树，每个分屏一个 surface。因此 con 正确的迁移路径是 Ghostty 驱动的 surface 树控制器，而不是把整个窗口塞进一个终端 surface。

`TerminalPane` 仍然存在，但它现在是对单类分屏的产品抽象，而不是多后端兼容层。

### 3. 分屏运行时观察器

长期的产品需求不是"检测 tmux"，而是持久的分屏运行时可观测性。

一个分屏内可能包含嵌套栈：

- 本地 shell
- SSH 连接
- 远程 shell
- tmux
- 另一个 shell
- Agent CLI / vim / htop

架构必须显式建模这一结构。

**三层设计：**

1. **后端事实** —— Ghostty 动作回调、进程退出状态、标题、PWD、命令完成事件、PTY 输入代次、可见文本
2. **分屏运行时追踪器** —— 对后端观测、类型化 shell 探测和 con 发起的动作做归约；命令边界新鲜度；作用域栈推断；置信度
3. **消费方** —— Agent 提示词、`list_panes`、审批、侧边栏、通知、恢复界面

**重要约束：** shell 元数据与可见应用身份不是一回事。如果 con 无法证明前台运行时，必须报告 `unknown`，而不是把标题或屏幕模式猜测提升为产品状态。

**当前基础：** Ghostty 已为 con 提供强大的终端事实，但内嵌 C API 尚未暴露前台进程身份、权威命令文本、备用屏状态、远程主机身份，或 Ghostty 更丰富的内部语义提示 / 运行时状态。因此 con 需要自己的分屏运行时追踪器，而不是把更多产品逻辑塞进提示符启发式。

**归约模型：** 现在每个分屏都有一个有状态追踪器，合并三类输入：

- 来自 Ghostty 的后端观测
- 类型化 shell 作用域探测，例如 `probe_shell_context`
- con 发起的动作历史，例如分屏创建、可见 shell exec、原始输入、进程退出

这样 con 可以保留因果历史，而不会把它误当作当前前台真相。一个新鲜的 shell 提示符会清除活跃的 tmux/应用身份，除非新的类型化探测重新确立它。历史动作仍作为证据可见，说明这个分屏是如何走到当前状态的。

远程工作现在有了一层重要的中间层：**con 管理的 SSH 连续性**。当 con 创建或最近驱动过一个 SSH 分屏，且当前屏幕仍看起来像提示符、没有 tmux 或 TUI 矛盾时，即使缺少新鲜的 shell 集成，该分屏仍可被复用为远程工作区。这不会提升为前台真相，但足以在后续轮次中防止重复创建远程分屏。

**长期方向：** 如果 con 需要进程组身份或更丰富的语义提示导出，正确做法是向上游扩展 libghostty 的 C API，而不是在本地重新引入第二个终端运行时。

见 `docs/impl/pane-runtime-observer.md`。

### 3b. Agent 运行时控制面

仅有可观测性还不够。

Agent 还需要一个显式控制面，用于区分：

- con 分屏地址
- 嵌套运行时目标，例如 SSH 作用域、tmux 会话、tmux 分屏和编辑器
- 控制通道，例如可见 shell exec、本地隐藏 exec、tmux 控制和原始 TUI 输入

这是防止以下故障的架构边界：

- 为远程任务运行本地隐藏命令
- 把 tmux 分屏误认为 con 分屏
- 把 shell 命令输入到 `nvim` 里

因此长期的工具模型应按控制层拆分，而不只是按"分屏加命令"拆分。见 `docs/impl/agent-runtime-control-plane.md`。

不存在类似 Chrome DevTools Protocol 的终端级统一协议。
因此 con 需要一个分层控制面，并显式挂接各协议：

- Ghostty surface 与 VT 事实
- shell 提示符挂接
- tmux 控制挂接
- 应用原生 RPC 挂接
- 操作系统与 PTY 进程事实

Ghostty surface 之后的第一个具体挂接是经过验证的 shell 提示符挂接。con 在这里可以安全地暴露只读 shell 探测，例如 `$TMUX`、`$SSH_CONNECTION`、tmux 会话 / 窗口 / 分屏 id，以及编辑器套接字提示。

助手现在只自动预加载**静默**挂接。可见的 shell 探测仍作为显式工具调用。这样首轮终端保持安静，同时仍让 con 在后续轮次复用 tmux 状态和远程工作区锚点。

见 `docs/study/terminal-control-plane.md`。

### 4. AI Agent 助手：Rig

**为什么选 [Rig](https://rig.rs/) 而非其他方案：**

| 方案 | 结论 |
|------|------|
| **Rig** | 最成熟的 Rust AI Agent 框架。支持多提供商（OpenAI、Anthropic、Cohere 等）。具备工具调用、RAG、Agent 抽象。基准测试中 CPU 占用 24.3%，最高效。 |
| **rust-genai** | 多提供商客户端，但没有 Agent 抽象（工具、链）。只是一个 API 客户端。 |
| **嵌入 Bun + ai-sdk** | 不可行 —— Bun 没有嵌入 API，需要启动子进程。 |
| **AutoAgents** | 侧重多 Agent，较重。适合编排，但对终端 Agent 来说过重。 |
| **裸 HTTP** | 控制力最强，但要花费数月做提供商专属工作。 |

**Rig 提供：**

- `CompletionClient::agent()` 构建器 —— 前导指令 + 工具 + 模型配置一条链搞定
- `Tool` trait —— 类型安全的工具定义，含 `Args`（Deserialize）、`Output`（Serialize）、`Error`
- 带显式 `Vec<Message>` 历史的流式提示请求
- `AgentHook` 事件，用于文本增量、工具审批和工具结果
- `anthropic::Client::new(api_key)` —— 直连 Anthropic 提供商，含模型常量（`CLAUDE_4_SONNET`）
- 自动调用工具的 Agent 循环（最多 N 轮）

**当前集成：** con-agent 实现了 34 个 Rig 工具，覆盖可见终端控制、tmux、本地文件、远程工作区和 Agent CLI 编排。提供商原生客户端与 OpenAI 兼容端点共享同一条助手路径，模型、凭据和 base URL 均可配置。助手运行在共享的 tokio 运行时上。提供商设置可在设置窗口中配置。

**Agent 生命周期（AgentHook）：** 每个 Agent 请求都会构建一个携带其对话历史的流式提示，并在 Agent 上注册 `ConHook`。该钩子把 Rig 的类型化生命周期事件映射为 Con 的 UI 事件，用于工具调用开始、工具结果和文本增量。对于危险工具（`shell_exec`、`terminal_exec`、`file_write`、`edit_file`），钩子在执行前会等待每请求的审批通道。安全工具（`file_read`、`list_files`、`search`）立即执行。5 分钟超时可防止 UI 无响应时无限挂起。Rig 0.40 使用精确的模型调用总预算；Con 通过一个兼容函数转换其现有的 `max_turns` 设置，以保留此前已发布的上限。

**可见终端工具：** `terminal_exec` 工具是 con 的核心差异化能力。它不在隐藏子进程中运行命令，而是把命令写入用户可见的 Ghostty 分屏。输出通过 Ghostty 的命令完成信号捕获，当 shell 集成不可用时回退到有界的近期输出。用户能实时看到 Agent 运行的每一条命令 —— 完全透明。

**流式取消：** Agent 请求可以在流式传输中途取消。助手维护一个 `Arc<AtomicBool>` 取消标志，在流项目之间检查。Agent 面板在流式传输期间显示"停止"按钮。取消会保留已累积的部分响应。

**每请求审批通道：** 每次 `send_message()` 都会创建一对全新的 crossbeam 通道。发送端通过 `ToolApprovalNeeded` 事件交给 UI。接收端由该请求的 `ConHook` 持有。这消除了并发 Agent 请求之间的竞态 —— 每个通道只有一个钩子实例读取。

**每标签页 Agent 会话：** 每个标签页拥有独立的 `AgentSession` —— 自己的对话、事件通道、终端 exec 通道和取消标志。`AgentHarness`（每窗口共享）只持有基础设施：tokio 运行时（2 个工作线程）、配置和技能注册表。用户切换标签页时，Agent 面板通过 `std::mem::replace()` 换入换出各自的 `PanelState`，保留每个标签页的对话历史和进行中的状态。后台标签页继续把 Agent 事件处理进各自缓存的 `PanelState`。终端 exec 请求路由到发起标签页的分屏树，而不是活跃标签页 —— 因此运行在标签页 2 的 Agent 即使在用户切到标签页 1 后，也仍在标签页 2 执行命令。

**对话往返：** 对话状态（`Arc<Mutex<Conversation>>`）在主线程与派生的任务之间共享。Agent 完成后，助手消息会被加回对话，从而支持多轮上下文。每个标签页在重启后仍保留各自的 `conversation_id`。

**逃生通道：** 如果日后需要 ai-sdk 的特性，可以启动 Bun/Node sidecar 进程并通过 IPC 通信。这是务实的后备方案，不是主架构。

### 5. 平台策略

con 现在有三种平台状态：

- macOS：已发布，使用内嵌 libghostty + AppKit 路径
- Windows：beta，使用 `libghostty-vt` + ConPTY + 本地 D3D11/DirectWrite 渲染器
- Linux：预览，使用 Unix PTY + 共享 `libghostty-vt` + GPUI 自有的逐行 `StyledText` 绘制路径。SGR 颜色 / 粗体 / 斜体 / 下划线 / 反显 + 块状光标均可工作，窗口带客户端装饰、透明 ARGB visual、圆角，以及在合成器支持时使用 KWin-Wayland 背景模糊。剩余的 Linux 工作是长期的 GPUI 自有字形图集网格渲染器（与 Windows 使用的 D3D11/DirectWrite 路径对应）。

平台架构不再是"仅 macOS"，但仍不是"到处同一个后端"。

- GPUI 在所有受支持平台提供宿主应用外壳
- 只要其嵌入界面可用，Ghostty 始终是首选的终端真相来源
- 平台专属后端胶水仍然重要：
  - macOS 上为 AppKit
  - Windows 目前为 D3D11/ConPTY
  - Linux 上为 Unix PTY + GPUI 自有渲染器；见 `docs/impl/linux-port.md`

重要推论：

- Windows 证明，当上游嵌入不可用时，con 可以发布本地后端。
- Linux 可行性研究表明，上游 Ghostty 和 GPUI 都有真实的 Linux 技术栈，但它们今天对 con 来说嵌入边界对不上。
- 因此 con 在 Linux 上采取与 Windows 相同的交付立场：发布本地后端，而不是等待上游嵌入钩子。

---

## 核心功能（MVP → v0.1）

### 阶段 1：能用的终端

- [x] 带单一终端分屏的 GPUI 窗口
- [x] 内嵌 Ghostty surface 生命周期与输入转发
- [x] 动态终端尺寸调整（填充可用窗口空间）
- [x] 支持鼠标滚轮导航的回滚缓冲
- [x] 256 色 + 真彩色
- [x] DEC 私有模式：DECCKM、DECAWM、备用屏、括号粘贴
- [x] DA/DSR 响应写回 PTY
- [x] 应用光标键（vim/less/top 的 SS3 模式）
- [x] 文本样式渲染：斜体、下划线、删除线、暗淡、反显
- [x] 基础标签页（Cmd+T / Cmd+W），含标签栏和 OSC 标题
- [x] 从配置读取字号（`con.conf` 中的 `font-size`）
- [x] 从 OSC 7 在输入栏显示 CWD
- [x] 鼠标文本选择（点击拖拽、自动复制、Cmd+C 复制）
- [x] 剪贴板粘贴（Cmd+V），支持括号粘贴模式
- [x] 终端媒体输入：文件拖放 / 文件路径剪贴板粘贴转为带引号的路径，图片剪贴板粘贴通过原始 Ctrl+V 转发给 TUI 应用
- [x] Cmd+1..9 切换标签页
- [x] 会话持久化（标签页、活跃标签页、Agent 面板状态）
- [x] Kitty 键盘协议（CSI u 编码、push/pop/query 标志）
- [x] 分屏（macOS：Cmd+D / Cmd+Shift+D，Windows/Linux：Alt+D / Alt+Shift+D，分屏树）

### 阶段 2：Agent 助手

- [x] AI 对话侧面板（Cmd+L 切换）
- [x] 终端上下文注入（最近 N 行、当前命令、cwd）
- [x] 工具执行：Agent 可在终端中运行 shell 命令
- [x] 通过 Rig 0.40 配置多提供商
- [x] 设置面板（Cmd+,），含提供商选择器和模型配置
- [x] 智能输入栏（shell/agent/智能模式）
- [x] Agent 通知系统（Agent 响应时标签页显示蓝点）

### 阶段 3：Agent 生命周期与工具透明

- [x] AgentHook 集成 —— 工具调用在 Agent 面板中可见
- [x] 工具危险等级分类（安全：file_read、search；危险：shell_exec、file_write）
- [x] 每请求审批通道（无跨请求干扰）
- [x] 审批超时（5 分钟，超时后拒绝）
- [x] 对话往返（助手消息加回以支持多轮）
- [x] Agent 状态指示器（空闲 / 思考中 / 响应中）
- [x] 工具调用渲染（进行中圆点、已完成结果）
- [x] 审批对话框 UI（内联审批卡片，含允许/拒绝）
- [x] 通过 `stream_prompt()` 实现流式输出与实时 token 渲染

### 阶段 4：Agent 对话打磨

- [x] 结构化工具调用卡片（图标、名称、格式化参数、结果）
- [x] 危险工具的内联审批卡片（允许/拒绝按钮）
- [x] 可滚动的 Agent 面板
- [x] 可折叠的步骤时间线（点击展开/折叠）
- [x] 设置 UI 中的自动批准开关（开关即时传播到配置）
- [x] 通过 `stream_prompt()` 实现流式文本渲染

### 阶段 5：深度集成

- [x] OSC 133 命令块追踪（提示符/命令/退出码检测）
- [x] 命令面板（Cmd+Shift+P），含模糊搜索和键盘导航
- [x] Agent 上下文中的命令历史（最近 10 条命令及退出码）
- [x] 内联 AI 建议（带缓存的防抖补全引擎）
- [x] 命令块操作（复制输出、重新运行、通过 Agent 解释）
- [x] 分屏安全的 Agent 执行守卫与类型化控制面状态
- [ ] 内嵌 Ghostty 上的权威 SSH 作用域检测
- [ ] 内嵌 Ghostty 上的权威 tmux / TUI 运行时检测
- [x] 对话历史 + 搜索（保存/加载/列出、新对话、历史面板）

### 阶段 6：打磨

- [x] 会话持久化与恢复
- [x] 光标闪烁（500ms 定时器，按键时重置）
- [x] 回滚指示器（浮动胶囊显示"上方 N 行"）
- [x] Agent 面板自动滚动（消息容器上的 ScrollHandle）
- [x] 双击选词、三击选行
- [x] 主题感知的光标与选区颜色
- [x] 侧边栏与标签页状态同步，新会话按钮
- [x] 居中的设置与命令面板浮层，带阴影
- [x] Agent 面板中的代码块渲染（三反引号围栏）
- [x] shell 模式提交命令后重新聚焦终端
- [x] 统一输入路由（智能模式分类 shell/agent/技能）
- [x] 技能端到端接通（/explain、/fix、/commit、/test、/review）
- [x] 命令面板扩展（清屏、聚焦、切换侧边栏、循环模式）
- [x] 设置 UI 中的终端设置（字号、主题）
- [x] Cmd+A 全选、Cmd+K 清空回滚
- [x] 可配置快捷键（`con.conf` 中的 `con.keybindings.*`）
- [ ] 插件系统（Lua 或 WASM）
- [ ] 自动更新（macOS 上的 Sparkle）
- [ ] CLI 工具（用于脚本化的 `con` 命令）

### 阶段 7：Agent 能力与终端打磨

- [x] Rig 0.36 → 0.40 迁移（hooks v2、结构化工具元数据、统一提示响应）
- [x] 丰富上下文注入（git diff、项目结构、XML 标记的系统提示）
- [x] 可见终端工具（terminal_exec —— 命令在用户可见的 PTY 中执行）
- [x] 外科手术式文件编辑工具（edit_file —— 查找替换，而非整文件覆盖）
- [x] 目录列举工具（list_files —— 感知 .gitignore）
- [x] 流式取消（停止按钮，保留部分响应）
- [x] 可调整大小的 Agent 面板（拖动分隔条，宽度持久化到会话）
- [x] 扩展思考展示（Agent 面板中的可折叠区域）
- [x] 主题可配置（4 个内置主题、实时切换、设置选择器）
- [x] 每标签页 Agent 会话（每个标签页拥有自己的对话、上下文和审批状态）

---

## Agent ↔ 终端集成设计

这才是 con 区别于"终端 + 聊天机器人侧边栏"的地方。

### 终端上下文感知

```
终端输出
     │
     ▼
┌─────────────┐     ┌──────────────┐
│ OSC 133     │────▶│ 命令         │
│ 检测         │     │ 块解析器     │
└─────────────┘     └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │ 上下文        │
                    │ 提取器        │
                    │ - cwd        │
                    │ - 上条命令    │
                    │ - 退出码      │
                    │ - 输出        │
                    │ - git 分支    │
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │ Agent        │
                    │ 助手          │
                    │（Rig）        │
                    └──────────────┘
```

Agent 始终知道：

- 用户所在的目录
- 用户刚运行的命令及其输出
- 是否处于 SSH/tmux/docker 中
- 如果在仓库内，git 状态如何

这是当前基线。下一步是带证据和置信度的运行时作用域栈，让 Agent 能区分 shell 元数据与可见的前台运行时。

### Agent 工具执行

当 Agent 需要运行命令时：

1. Agent 通过 Rig 产生 `ShellExec` 工具调用
2. con-core 创建（或复用）终端分屏
3. 命令写入 PTY
4. 输出通过 Ghostty `COMMAND_FINISHED` 捕获，当 shell 集成不可用时回退到有界的近期输出
5. 结果连同退出码和耗时元数据回传给 Agent

这意味着用户**看得见** Agent 在做什么 —— 没有隐藏子进程。完全透明。

### 与现有 Agent 的兼容性

对于运行在终端*内部*的 Agent CLI 及其他工具：

- con 应通过分屏运行时追踪器检测它们，优先使用强证据：
  - shell/运行时切换
  - 类型化 shell 探测与归约器支撑的动作历史
  - 后端事实，例如命令生命周期及未来的备用屏导出
- con 不得把分屏标题或屏幕结构启发式提升为类型化运行时状态
- 提供增强 UX：通知光环、焦点管理
- *不*试图"接管" —— con 是宿主，不是 Agent
- 可选：把 Agent 的 stderr/通知接入 con 的通知系统

这是产品边界，不是便利特性。con 必须在支持这些工具的同时，不假装 con 本身拥有这个分屏。

---

## 构建与开发

### 前置条件

```bash
# macOS
brew install rustup cmake
```

### 构建

```bash
cargo build                    # debug 构建
cargo build --release          # release 构建
cargo run -p con               # 运行终端
cargo test --workspace         # 测试全部
```

### 构建流水线

1. Cargo 按工作区清单中的声明，从上游 git 源和 crates.io 解析工作区依赖
2. GPUI 在运行时编译 Metal 着色器（`runtime_shaders` 特性 —— 开发时无需 Xcode.app）
3. `cargo build` 产出链接了所有 crate 的 `con` 二进制

---

## 配置

```ini
# macOS 上为 ~/Library/Application Support/con/con.conf
# Linux 上为 ~/.config/con/con.conf

# 原生终端设置使用 Ghostty 名称。
font-family = "JetBrains Mono"
font-size = 14
theme = "catppuccin-mocha"
cursor-style = block

# Con 自有设置位于 con.* 命名空间下。
con.agent.provider = anthropic
con.agent.providers.anthropic.model = claude-sonnet-4-6
con.agent.providers.anthropic.api_key_env = ANTHROPIC_API_KEY
con.agent.providers.anthropic.max_tokens = 4096
con.agent.max_turns = 10
con.agent.auto_context = true
con.agent.auto_approve_tools = false

con.keybindings.toggle_agent = secondary-l
con.keybindings.command_palette = secondary-shift-p
con.keybindings.new_tab = secondary-t
```

Windows 使用 `%APPDATA%\con-terminal\con-terminal.conf`。迁移、导入、原生归属及可移植子集见
[`docs/impl/configuration.md`](docs/impl/configuration.md)。

---

## 为什么这套技术栈胜出

1. **Ghostty 的终端正确性** —— 通过 libghostty 嵌入完整 Ghostty，Metal GPU 渲染，完整 VT 合规，包括 Kitty 键盘和 OSC 133 shell 集成
2. **GPUI 的渲染质量** —— Zed 证明它在大规模文本场景下表现出色
3. **Rig 的 Agent 抽象** —— 可替换提供商、用 Rust 定义工具、流式输出 token
4. **Rust 的性能** —— 亚毫秒输入延迟，内存占用 <100MB
5. **一个运行时，一套行为模型** —— 每个分屏都用 Ghostty，从而保持终端行为与 Agent 集成一致
6. **开源** —— 让产品可审计、可魔改、可持续

---

## 已定决策

### 1. 终端后端：完整 Ghostty

**决策：** 通过 libghostty C API 嵌入完整 Ghostty，作为唯一的终端运行时。

**理由：**

- Ghostty 提供生产级的 VT 合规（Kitty 键盘、sixel、超链接、OSC 133），无需自行重写
- 通过原生 NSView 实现 GPU 加速的 Metal 渲染 —— 性能优于软件光栅化
- 动作回调系统提供 COMMAND_FINISHED（退出码 + 耗时），消除 Agent 命令执行的盲等待超时
- `con-ghostty` crate 是轻薄的 FFI 包装（约 800 行），不是分支 —— 上游 Ghostty 更新可干净地流入
- `TerminalPane` 保留为面向分屏的稳定 API，供工作区层和 Agent 层使用

### 2. GPUI IME：可投入生产

GPUI 实现了完整的 `InputHandler` trait（参照 `NSTextInputClient`）：

- `marked_text_range()` / `replace_and_mark_text_in_range()` 用于 IME 组合
- `bounds_for_range()` 用于候选窗口定位
- GPUI 平台支持更广，con 现在发布 macOS，以及 Windows beta 和 Linux 预览；见 `docs/impl/linux-port.md`。
- 中文输入可用。无阻碍。

### 3. GPU 回退：无需

Ghostty 没有软件渲染器 —— 仅 GPU。con 也是如此。

**理由：** 桌面终端模拟器总是有 GPU。没有的场景：

- **无头服务器** —— 用户通过 SSH 登录，用的是远程机器的终端，不是 con
- **容器** —— con 不运行在 Docker 内，它运行在宿主机上

如果日后需要无头测试，我们会加一个仅测试用的软件光栅化器。这不是用户功能。

### 4. 插件系统：Node.js + Python，通过 sidecar IPC

**决策：** 插件作为外部进程运行。con 通过 Unix 域套接字上的 JSON-RPC 通信。

**为什么选 Node.js + Python：**

- 开发者生态最大 —— 插件作者的摩擦最低
- 运行时由用户自行安装（系统 Node/Python、nvm、pyenv —— 随他们选）
- 无嵌入运行时 = 二进制不膨胀、无版本冲突
- 安全：插件在独立进程中运行，需显式授予能力

**工作方式：**

```
con (Rust)  ─── Unix socket (JSON-RPC) ───  插件进程（Node/Python/任意）
```

- con 暴露套接字 API：`notification.create`、`terminal.write`、`context.get` 等
- 插件清单声明：名称、运行时、入口点、请求的能力
- con 启动插件进程，通过环境变量传递套接字路径
- 插件 SDK：封装 JSON-RPC 协议的轻量 npm 包（`@con/sdk`）和 pip 包（`con-sdk`）

**阶段 4 交付物。** 套接字 API 来自产品计划早期引入的同一套外部自动化工作。

### 5. 许可证：MIT

- **GPUI**：Apache 2.0（与 MIT 兼容，允许再许可）
- **Ghostty libghostty**：MIT
- **Rig**：MIT
- **con**：MIT（LICENSE 中已有）

全部无问题。无 copyleft，无 GPL 污染。

---

## 关键洞察

产品不应依赖内置模型才有用。面向外部 Agent 的套接字控制 API 让系统保持可组合，并让内置自动化只是平台的一个客户端。

**con 应两者兼备：**

1. **内置 Agent**（通过 Rig）—— 面向希望使用原生 AI、不想再装别的东西的用户
2. **套接字 API** —— 供外部 Agent 和插件控制 con

套接字 API 是基础。内置 Agent 只是该 API 的第一个客户端。
