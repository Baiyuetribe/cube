# con — justfile
# https://github.com/casey/just
#
# 用法：
#   just          # 列出所有配方
#   just run      # 从源码运行（当前平台）
#   just install  # 构建并安装（当前平台）
#
# `arch` 参数默认为 "" —— 每个 Unix 配方会在 shell 体内通过
# `uname -m` 自动探测。Windows 配方从不引用 arch，因此那里不会调用 `uname`。
# 需要时显式覆盖：just arch=x86_64 macos-bundle

# Windows 上使用 cmd.exe，使配方能在普通 Developer Command Prompt 中运行，
# 无需 PATH 上有 Git Bash、Cygwin 或 sh。
set windows-shell := ["cmd.exe", "/c"]

# ── 默认值 ────────────────────────────────────────────────────────────────────

# macOS/Linux 应用包的发布通道（stable | beta | dev）
channel := "stable"

# 目标架构。为空 = 在每个配方内部自动探测（仅 Unix）。
# Windows 配方从不使用此变量，因此那里永远不会调用 uname。
arch := ""

# ── 列表 ──────────────────────────────────────────────────────────────────────

# 列出所有配方（默认）
default:
    @just --list

# ── 通用开发命令 ──────────────────────────────────────────────────────────────
# 这些命令分发到对应平台的配方。Windows 必须使用 `w*` cargo 别名，
# 因为 `CON` 是 DOS 保留设备名，且 Windows 二进制通过特性门控命名为 `con-app.exe`。

# Debug 构建 —— 当前平台。
# con-cli 必须与 Con 二进制位于同一目录：handoff 启动辅助程序及其
# 分发前协议门禁会在 `current_exe()` 旁边查找 `con-cli`。
build:
    {{ if os() == "windows" { "cargo wbuild -p con && cargo build -p con-cli" } else { "cargo build -p con -p con-cli" } }}

# Release 构建 —— 当前平台
build-release:
    {{ if os() == "windows" { "cargo wbuild -p con --release && cargo build -p con-cli --release" } else { "cargo build --release -p con -p con-cli" } }}

# 从源码运行 —— 当前平台（先构建 con-cli，确保 handoff 能找到它）
run:
    {{ if os() == "windows" { "cargo build -p con-cli && cargo wrun -p con" } else { "cargo build -p con-cli && cargo run -p con" } }}

# 运行当前平台对应的测试集
test:
    {{ if os() == "windows" { "cargo wtest -p con-core -p con-cli -p con-agent -p con-terminal" } else { "cargo test --workspace" } }}

# 只做检查不构建 —— 当前平台
check:
    {{ if os() == "windows" { "cargo wcheck -p con" } else { "cargo check --workspace" } }}

# 运行 clippy —— 当前平台
lint:
    {{ if os() == "windows" { "cargo clippy --workspace --no-default-features --features con/bin-con-app -- -D warnings" } else { "cargo clippy --workspace -- -D warnings" } }}

# 清理 cargo 构建产物
clean:
    cargo clean

# 构建并安装到当前平台的本地开发安装路径
install:
    just channel={{ channel }} arch={{ arch }} {{ if os() == "macos" { "macos-install" } else if os() == "linux" { "linux-install" } else if os() == "windows" { "windows-install" } else { "unsupported-platform" } }}

# 打印当前包 id，含工作区版本
version:
    @cargo pkgid -p con

unsupported-platform:
    @echo "此 justfile 不支持的平台"
    @exit 1

# ── macOS ─────────────────────────────────────────────────────────────────────

# [macOS] 构建本地 .app 包 —— 不签名、不公证
# 输出：dist/macos/{channel}/{arch}/con.app
macos-bundle channel=channel arch=arch:
    #!/usr/bin/env bash
    set -euo pipefail
    resolved_arch="{{ arch }}"
    if [[ -z "${resolved_arch}" ]]; then
        resolved_arch="$(uname -m | sed 's/aarch64/arm64/')"
    fi
    CON_CHANNEL={{ channel }} CON_ARCH="${resolved_arch}" ./scripts/macos/build-app.sh

# [macOS] 构建 .app 并复制到 /Applications（替换已有版本）
macos-install channel=channel arch=arch: (macos-bundle channel arch)
    #!/usr/bin/env bash
    set -euo pipefail
    resolved_arch="{{ arch }}"
    if [[ -z "${resolved_arch}" ]]; then
        resolved_arch="$(uname -m | sed 's/aarch64/arm64/')"
    fi
    app_name="con"
    if [[ "{{ channel }}" == "beta" ]]; then app_name="con Beta"; fi
    if [[ "{{ channel }}" == "dev" ]];  then app_name="con Dev";  fi
    src="dist/macos/{{ channel }}/${resolved_arch}/${app_name}.app"
    dst="/Applications/${app_name}.app"
    echo "正在安装 ${src} → ${dst}"
    rm -rf "${dst}"
    cp -R "${src}" "${dst}"
    echo "完成。可从 /Applications 或 Spotlight 启动 ${app_name}。"

# [macOS] Ad-hoc 签名包（无需 Apple 开发者账号；Gatekeeper 会警告一次）
macos-bundle-adhoc channel=channel arch=arch: (macos-bundle channel arch)
    #!/usr/bin/env bash
    set -euo pipefail
    resolved_arch="{{ arch }}"
    if [[ -z "${resolved_arch}" ]]; then
        resolved_arch="$(uname -m | sed 's/aarch64/arm64/')"
    fi
    app_name="con"
    if [[ "{{ channel }}" == "beta" ]]; then app_name="con Beta"; fi
    if [[ "{{ channel }}" == "dev" ]];  then app_name="con Dev";  fi
    bundle="dist/macos/{{ channel }}/${resolved_arch}/${app_name}.app"
    echo "正在对 ${bundle} 做 Ad-hoc 签名"
    codesign --force --deep --sign - "${bundle}"
    echo "已签名（Ad-hoc）：${bundle}"

# [macOS] 将 Ad-hoc 签名的包安装到 /Applications
macos-install-adhoc channel=channel arch=arch: (macos-bundle-adhoc channel arch)
    #!/usr/bin/env bash
    set -euo pipefail
    resolved_arch="{{ arch }}"
    if [[ -z "${resolved_arch}" ]]; then
        resolved_arch="$(uname -m | sed 's/aarch64/arm64/')"
    fi
    app_name="con"
    if [[ "{{ channel }}" == "beta" ]]; then app_name="con Beta"; fi
    if [[ "{{ channel }}" == "dev" ]];  then app_name="con Dev";  fi
    src="dist/macos/{{ channel }}/${resolved_arch}/${app_name}.app"
    dst="/Applications/${app_name}.app"
    echo "正在安装 ${src} → ${dst}"
    rm -rf "${dst}"
    cp -R "${src}" "${dst}"
    echo "完成。可从 /Applications 或 Spotlight 启动 ${app_name}。"

# [macOS] 完整发布：构建 + 签名 + 公证 + DMG
# 需要：APPLE_SIGNING_IDENTITY + APPLE_NOTARY_* 或 APPLE_ID 环境变量
macos-release channel=channel arch=arch:
    #!/usr/bin/env bash
    set -euo pipefail
    resolved_arch="{{ arch }}"
    if [[ -z "${resolved_arch}" ]]; then
        resolved_arch="$(uname -m | sed 's/aarch64/arm64/')"
    fi
    CON_CHANNEL={{ channel }} CON_ARCH="${resolved_arch}" ./scripts/macos/release.sh

# [macOS] 下载 Sparkle.framework 到 .sparkle/（启用应用包内自动更新）
macos-sparkle-download:
    ./scripts/sparkle/download.sh

# [macOS] 在 Finder 中打开已构建的应用包
macos-open channel=channel arch=arch:
    #!/usr/bin/env bash
    resolved_arch="{{ arch }}"
    if [[ -z "${resolved_arch}" ]]; then
        resolved_arch="$(uname -m | sed 's/aarch64/arm64/')"
    fi
    app_name="con"
    if [[ "{{ channel }}" == "beta" ]]; then app_name="con Beta"; fi
    if [[ "{{ channel }}" == "dev" ]];  then app_name="con Dev";  fi
    open "dist/macos/{{ channel }}/${resolved_arch}/${app_name}.app"

# ── Linux ─────────────────────────────────────────────────────────────────────

# [Linux] 构建 release 二进制并打包
# 输出：dist/con-{version}-linux-{arch}.tar.gz
linux-release channel=channel arch=arch:
    #!/usr/bin/env bash
    set -euo pipefail
    resolved_arch="{{ arch }}"
    if [[ -z "${resolved_arch}" ]]; then
        resolved_arch="$(uname -m | sed 's/aarch64/arm64/')"
    fi
    CON_RELEASE_CHANNEL={{ channel }} CON_LINUX_ARCH="${resolved_arch}" ./scripts/linux/release.sh

# [Linux] 将 release 二进制安装到 ~/.local/bin
linux-install channel=channel arch=arch: (linux-release channel arch)
    #!/usr/bin/env bash
    set -euo pipefail
    resolved_arch="{{ arch }}"
    if [[ -z "${resolved_arch}" ]]; then
        resolved_arch="$(uname -m | sed 's/aarch64/arm64/')"
    fi
    # scripts/linux/release.sh 会暂存到 dist/con-{version}-linux-{arch}/
    # 使用 || true，避免 glob 无匹配时被 set -e 退出。
    stage_dir="$(ls -d dist/con-*-linux-${resolved_arch} 2>/dev/null | sort -V | tail -1 || true)"
    if [[ -z "${stage_dir}" || ! -f "${stage_dir}/con" ]]; then
        echo "在 dist/con-*-linux-${resolved_arch}/ 下未找到二进制 —— 请先运行 'just linux-release'"
        exit 1
    fi
    mkdir -p "$HOME/.local/bin"
    cp "${stage_dir}/con" "$HOME/.local/bin/con"
    chmod 755 "$HOME/.local/bin/con"
    echo "已安装 ${stage_dir}/con → $HOME/.local/bin/con"
    if [[ -f "${stage_dir}/con-cli" ]]; then
        cp "${stage_dir}/con-cli" "$HOME/.local/bin/con-cli"
        chmod 755 "$HOME/.local/bin/con-cli"
        echo "已安装 ${stage_dir}/con-cli → $HOME/.local/bin/con-cli"
    fi

# [Linux] 使用 AetherPak Zero-Manifest 模式构建 Flatpak 包
# 输出：dist/co.nowledge.con.flatpak
flatpak-build channel=channel arch=arch:
    #!/usr/bin/env bash
    set -euo pipefail
    resolved_arch="{{ arch }}"
    if [[ -z "${resolved_arch}" ]]; then
        resolved_arch="$(uname -m)"
    fi
    case "${resolved_arch}" in
        arm64|aarch64) resolved_arch="aarch64" ;;
        amd64|x86_64) resolved_arch="x86_64" ;;
        *) echo "不支持的 Flatpak 架构：${resolved_arch}" >&2; exit 1 ;;
    esac
    aetherpak_cmd="aetherpak"
    if ! command -v aetherpak &>/dev/null; then
        if [[ -x "$HOME/workspace/aetherpak/cli/bin/aetherpak" ]]; then
            aetherpak_cmd="$HOME/workspace/aetherpak/cli/bin/aetherpak"
        else
            echo "错误：在 PATH 或 ~/workspace/aetherpak/cli/bin/aetherpak 下均未找到 aetherpak CLI" >&2
            exit 1
        fi
    fi
    "$aetherpak_cmd" build --config packaging/flatpak/aetherpak.yaml \
        --arch "${resolved_arch}" \
        --branch "{{ channel }}" \
        --bundle \
        --output-dir dist

# ── Windows（在 Developer Command Prompt for VS 2022 中运行）─────────────────

# [Windows] Debug 构建（con-app.exe —— CON 是 DOS 保留设备名）
windows-build:
    cargo wbuild -p con
    cargo build -p con-cli

# [Windows] Release 构建
windows-build-release:
    cargo wbuild -p con --release
    cargo build -p con-cli --release

# [Windows] 运行
windows-run:
    cargo build -p con-cli
    cargo wrun -p con

# [Windows] 测试
windows-test:
    cargo wtest -p con-core -p con-cli -p con-agent -p con-terminal

# [Windows] 构建并安装本地 release 二进制到用户安装根目录
windows-install: windows-build-release
    if not exist "%LOCALAPPDATA%\Programs\con" mkdir "%LOCALAPPDATA%\Programs\con"
    copy /Y "target\release\con-app.exe" "%LOCALAPPDATA%\Programs\con\con-app.exe"
    copy /Y "target\release\con-cli.exe" "%LOCALAPPDATA%\Programs\con\con-cli.exe"
    echo 已将 con-app.exe 和 con-cli.exe 安装到 %LOCALAPPDATA%\Programs\con

# ── dist 清理 ─────────────────────────────────────────────────────────────────

# 删除所有 dist/ 输出
clean-dist:
    {{ if os() == "windows" { "if exist dist rmdir /s /q dist" } else { "rm -rf dist/" } }}
