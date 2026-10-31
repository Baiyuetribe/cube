//! 标签页图标词汇表。
//!
//! 图标由关键字映射到 Phosphor SVG 路径；这是标签呈现的唯一图标来源，
//! 不使用 emoji，也不接受自由形式的图片引用。

/// 标签页图标种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabIconKind {
    Terminal,
    Code,
    Pulse,
    BookOpen,
    FileCode,
    Globe,
}

impl TabIconKind {
    /// `assets/icons/phosphor/` 下的 SVG 路径，跨调用方保持稳定。
    pub fn svg_path(&self) -> &'static str {
        match self {
            TabIconKind::Terminal => "phosphor/terminal.svg",
            TabIconKind::Code => "phosphor/code.svg",
            TabIconKind::Pulse => "phosphor/pulse.svg",
            TabIconKind::BookOpen => "phosphor/book-open.svg",
            TabIconKind::FileCode => "phosphor/file-code.svg",
            TabIconKind::Globe => "phosphor/globe.svg",
        }
    }

    /// 从进程/标题关键字推断图标种类。
    pub fn from_keyword(s: &str) -> Option<TabIconKind> {
        match s.trim().to_ascii_lowercase().as_str() {
            "terminal" | "shell" | "console" => Some(TabIconKind::Terminal),
            "code" | "editor" | "vim" | "nvim" | "emacs" | "nano" | "helix" | "kakoune" => {
                Some(TabIconKind::Code)
            }
            "pulse" | "monitor" | "htop" | "top" | "btop" => Some(TabIconKind::Pulse),
            "book" | "book-open" | "pager" | "less" | "more" | "man" => Some(TabIconKind::BookOpen),
            "file" | "file-code" | "git" | "lazygit" | "tig" => Some(TabIconKind::FileCode),
            "globe" | "ssh" | "remote" | "network" => Some(TabIconKind::Globe),
            _ => None,
        }
    }
}
