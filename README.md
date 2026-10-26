<p align="center">
   <a href="https://con.nowledge.co"><img src="assets/app-icons/con-raccoon-tail-A6.png" width="128" alt="con 标志"></a>
</p>
<h1 align="center">con</h1>
<p align="center"><strong>带 AI 助手的终端模拟器，仅此而已</strong></p>

<p align="center">
  开源。GPU 加速。终端优先。
</p>
<p align="center">
  为 SSH、tmux 和 Agent 原生工作流而构建。
</p>


## 为什么选 con？

`con` 面向这样的人：他们想要一个正经的终端，只在 AI 真正配得上它的位置时才接受 AI 帮助。

如果你是老派终端用户，只需要在必要时有个够用的 AI 助手，不多不少，那 `con` 就是为你做的。

## 它能做什么

- 一个快速且优雅的终端
- 内置 AI 助手，能读取上下文、动手前先询问，并直接在你眼前可见的终端里工作
- 面向 CLI 工作的终端原生工作流，支持 `ssh`、`tmux` 以及感知编码 Agent 的编排

## 状态

`con` 正处于活跃的 beta 开发阶段。

- **macOS** 完整支持，beta。
- **Windows** 早期 beta。追踪：[#34](https://github.com/nowledge-co/con-terminal/issues/34)。
- **Linux** 预览。追踪：[#18](https://github.com/nowledge-co/con-terminal/issues/18)。

## 截图

<p align="center">
  <a href="docs/screenshots.md">
    <img width="1080" alt="con 截图" src="https://github.com/user-attachments/assets/389898d6-56bf-46aa-9279-65e59a57ed23" />
  </a>
</p>

<p align="center">
  <a href="docs/screenshots.md">查看完整截图画廊</a>
</p>

## 快捷操作

<p align="center">
  <video controls muted playsinline width="100%" src="https://github.com/user-attachments/assets/2b6f6145-e400-4a74-a951-cd8221493a17"></video>
</p>

| 操作 | macOS | Windows / Linux |
| --- | --- | --- |
| 在终端与输入框之间切换焦点 | <kbd>⌘</kbd> <kbd>I</kbd> | <kbd>⌃</kbd> <kbd>⇧</kbd> <kbd>I</kbd> |
| 显示或隐藏底部输入栏 | <kbd>⌃</kbd> <kbd>\`</kbd> | <kbd>⌃</kbd> <kbd>\`</kbd> |
| 显示或隐藏 Agent 面板 | <kbd>⌘</kbd> <kbd>L</kbd> | <kbd>⌃</kbd> <kbd>⇧</kbd> <kbd>L</kbd> |
| 循环切换底部栏模式 | <kbd>⌘</kbd> <kbd>;</kbd> | <kbd>⌃</kbd> <kbd>;</kbd> |
| 显示或隐藏快捷终端（Quick Terminal，需先启用） | <kbd>⌘</kbd> <kbd>Backslash</kbd> | 不可用 |

- 智能模式会判断你输入的文本是 shell 命令还是 Agent 请求。
- 命令模式执行 shell 命令。存在多个分屏时，分屏小地图可让你选择聚焦分屏、全部分屏或指定的一组分屏。
- Agent 模式将文本直接发送给内置 Agent。

快捷终端默认关闭。在 macOS 上，可在 设置 -> 按键（Settings -> Keys）中启用其全局快捷键，或在 Con 位于前台时从命令面板 / 视图菜单打开。

## 安装

**macOS，Homebrew**

```sh
brew install --cask nowledge-co/tap/con-beta
```

这会安装应用，并将 `con-cli` 暴露到 PATH 上以便自动化调用。

**macOS**

```sh
curl -fsSL https://con-releases.nowledge.co/install.sh | sh
```

这会把应用安装到 `/Applications`，并把 `con-cli` 链接到 `~/.local/bin`。也可以直接从 [Releases](https://github.com/nowledge-co/con-terminal/releases) 下载 DMG。

**Linux**（预览版，官方安装脚本）

```sh
curl -fsSL https://con-releases.nowledge.co/install.sh | sh
```

这会把 `con` 和 `con-cli` 都安装到 `~/.local/bin`。也可以从最新 [Release](https://github.com/nowledge-co/con-terminal/releases) 下载 `con-<version>-linux-x86_64.tar.gz`。

**Arch Linux / AUR**（社区维护）

```sh
yay -S con-bin
```

`con-bin` 由 [`czyt`](https://aur.archlinux.org/account/czyt) 在 AUR 上维护，`paru -S con-bin` 同样可用。如果你需要在发布后立刻拿到最新 beta，请使用上面的官方安装脚本。

**Windows**（官方安装脚本）

```powershell
irm https://con-releases.nowledge.co/install.ps1 | iex
```

这会把 `con-app.exe` 和 `con-cli.exe` 安装到同一个 PATH 目录。也可以从最新 [Release](https://github.com/nowledge-co/con-terminal/releases) 下载 `con-windows-x86_64.zip`。

**Windows / Scoop**（社区维护）

```powershell
scoop bucket add jam https://github.com/EFLKumo/jam
scoop install jam/con-terminal
```

Scoop 清单由 [`EFLKumo`](https://github.com/EFLKumo) 在 [`jam`](https://github.com/EFLKumo/jam) bucket 中维护。它会安装便携版 Con，并将 `con-app` 与 `con-cli` 加入 PATH。

从源码构建请见 `HACKING.md`。

## 文档

从这里开始：

- [安装](docs/install.md)：在 macOS、Windows 或 Linux 上获取 con。
- [配置参考](docs/impl/configuration.md)：在 `con.conf`（Windows 上为 `con-terminal.conf`）中配置 Ghostty 终端设置和 Con 专属行为。
- [快捷操作](docs/quick-controls.md)：焦点切换、Agent 面板和命令模式。
- [快捷终端](docs/quick-terminal.md)：设置可选的 macOS 下拉式终端。
- [终端工作流](docs/terminal-workflows.md)：标签页、分屏、广播、分屏缩放、链接与 surface。
- [内置 Agent](docs/agent.md)：不离开终端即可使用 AI 帮助。
- [设置](docs/settings.md)：选择提供商、主题、建议、技能和快捷键。
- [技能与工作流](docs/skills-and-workflows.md)：把重复的终端流程变成项目级或个人级的斜杠命令。
- [工作区布局配置](docs/workspace-layout-profiles-guide.md)：保存项目布局、日后重新打开，或与团队共享。
- [con-cli 与 surface](docs/con-cli.md)：在 con 之上构建脚本、测试运行器和外部 Agent 编排器。
- [截图画廊](docs/screenshots.md)：以视觉方式浏览应用。
- [发行说明](CHANGELOG.md)：每个 beta 版本的变更内容。

面向贡献者：

- [贡献者快速上手](HACKING.md)：从源码构建、测试和发布。
- [架构](DESIGN.md)：产品方向与系统设计。
- `docs/impl/` 与 `docs/study/`：实现记录与研究笔记。

## 许可证

[MIT](LICENSE)

## 致谢 ♥️

`con` 依赖以下我们直接使用并深怀敬意的上游项目：

- [Ghostty](https://github.com/ghostty-org/ghostty)：提供终端运行时与渲染基础，支撑我们的内嵌终端 surface。
- [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui)：来自 Zed 团队的原生 GPU UI 框架，我们用它构建外壳。
- [gpui-component](https://github.com/longbridge/gpui-component)：来自 Longbridge 团队的组件库，加速了大部分 UI 层开发。
- [Rig](https://github.com/0xPlaygrounds/rig)：优秀的 Rust Agent 框架，支撑 `con` 的 AI 助手。
- [Phosphor Icons](https://phosphoricons.com/)：全应用使用的图标系统。
- [Flexoki](https://stephango.com/flexoki)：默认视觉主题方向。
- [Iosevka](https://typeof.net/Iosevka/) 与 [Ioskeley Mono](https://github.com/jewlexx/ioskeley)：终端外框与代码密集型 UI 所用的等宽字体基础。

`con` 最初受 [warp.dev](http://warp.dev/) 启发，但做得比 warp 少；如果你需要更多功能，应该去用 warp。
