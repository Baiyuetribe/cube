//! Git diff 查看器。
//!
//! 只读，支持单栏（unified）和并排（side-by-side）两种排布。比直接打开原始 diff 文本友好：
//! 增删行有背景色和左侧色条、行号分新旧两栏、文件段有标题和统计、
//! 连续未改动的上下文自动折叠。
//!
//! 不做编辑、光标、选区——这些在 diff 里没有意义。

use cube_core::git::{DiffFile, DiffLine, DiffLineKind, InlineSpan, ParsedDiff};
use gpui::{
    AnyElement, Context, EventEmitter, FontWeight, Hsla, IntoElement, ParentElement, Render,
    SharedString, Styled, TextRun, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme, Sizable as _,
    button::{Button, ButtonVariants as _},
};

use std::path::PathBuf;

use notify::Watcher as _;

use crate::ui_scale::ui_px;

/// 用户关闭 diff 视图。
pub struct CloseDiff;

/// 用户请求跳到源文件。
///
/// `line` 是新版行号；删除行没有新行号，用旧行号兜底，点开的是改动之前的位置。
pub struct JumpToSource {
    pub path: PathBuf,
    pub line: usize,
}

/// 行高基准值。实际渲染时按 UI 字号缩放，见 [`line_height`]。
const LINE_HEIGHT: f32 = 18.0;
/// 文件段标题行高基准。跳转时要按实际高度累加，不能统一用行高。
const FILE_HEADER_HEIGHT: f32 = 26.0;
/// 行号列宽度基准（新旧两栏）。
const GUTTER_WIDTH: f32 = 76.0;
/// 并排视图里单侧行号列的宽度基准。
const SINGLE_GUTTER_WIDTH: f32 = 48.0;
/// 并排视图的最小窗口宽度。低于它就退回单栏。
const SPLIT_MIN_WIDTH: f32 = 760.0;
/// 连续未改动超过这个行数就折叠，只留头尾各两行做上下文。
const COLLAPSE_THRESHOLD: usize = 6;
/// 折叠时保留的上下文行数（上下各留这么多）。
const CONTEXT_KEPT: usize = 2;

// 上面这些尺寸都跟着 UI 字号缩放。写死像素的话，用户把字号调大会出现
// 文字撑出固定行高、行号被挤掉的问题。

fn line_height(theme: &gpui_component::Theme) -> gpui::Pixels {
    ui_px(theme, LINE_HEIGHT)
}

fn file_header_height(theme: &gpui_component::Theme) -> gpui::Pixels {
    ui_px(theme, FILE_HEADER_HEIGHT)
}

fn gutter_width(theme: &gpui_component::Theme) -> gpui::Pixels {
    ui_px(theme, GUTTER_WIDTH)
}

fn single_gutter_width(theme: &gpui_component::Theme) -> gpui::Pixels {
    ui_px(theme, SINGLE_GUTTER_WIDTH)
}

pub struct DiffView {
    title: String,
    /// 仓库根目录。diff 里的路径是相对它的，跳转源文件要用。
    root: Option<PathBuf>,
    /// 打开时的默认路径，单文件 diff 从标题里推不出来时兜底。
    source_path: Option<PathBuf>,
    diff: ParsedDiff,
    /// 打平后的所有行，列表按这个渲染。
    rows: Vec<FlatRow>,
    /// 算高亮时用的主题特征。切换主题（浅/深或换配色）后要重算，
    /// 因为 TextRun 里的颜色是算好后固定下来的。
    highlighted_with: Option<(bool, String)>,
    /// 新版内容的语法高亮，首次渲染时按主题算一次。
    highlights: Vec<Vec<Vec<TextRun>>>,
    /// 旧版内容的语法高亮，只给删除行用。
    old_highlights: Vec<Vec<Vec<TextRun>>>,
    /// 已展开的折叠区块（key = 文件下标 + 区块起始行号）。
    expanded: std::collections::HashSet<(usize, usize)>,
    /// 滚动位置，供「跳到上一个/下一个改动」使用。
    scroll_handle: gpui::ScrollHandle,
    /// 当前排布：单栏还是并排。
    layout: DiffLayout,
    /// 并排视图的行，由 `rows` 推导而来。
    split_rows: Vec<SplitRow>,
    /// 自身的弱引用，行号点击回调用。首次渲染时填入。
    self_weak: gpui::WeakEntity<DiffView>,
    /// 重新读取 diff 的来源。只有能定位到仓库和文件时才自动刷新。
    reload: Option<DiffReload>,
    /// 轮询任务是否已启动，避免重复 spawn。
    reload_timer_started: bool,
    /// 用户试图在窄窗口切并排时的提示。下次切换排布时清掉。
    hint_too_narrow: bool,
    /// 上一次读取到的原始 diff 文本。自动刷新时用来跳过无变化的重算。
    last_raw: Option<String>,
}

/// 自动刷新 diff 所需的信息。
///
/// 只对工作区改动有效：提交 diff 是历史快照，不会变。
#[derive(Clone)]
pub struct DiffReload {
    pub root: PathBuf,
    /// 目标文件信息。`None` 表示整个仓库的工作区 diff。
    pub change: Option<cube_core::git::GitChange>,
    /// 文件变更信号。监视线程推、DiffView 拉。
    pub signal: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// 行号栏的显示方式。
///
/// 单栏要同时显示新旧两个行号；并排时每一侧只需要自己那一个，省下的宽度留给正文。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Gutter {
    /// 旧号 + 新号两栏。
    Both,
    /// 只有旧号（并排视图左侧）。
    Old,
    /// 只有新号（并排视图右侧）。
    New,
}

/// diff 的排布方式。
#[derive(Clone, Copy, PartialEq, Eq)]
enum DiffLayout {
    /// 删一行、增一行，上下排列。
    Unified,
    /// 左边旧内容、右边新内容，左右对齐。
    SideBySide,
}

/// 并排视图里的一行。
#[derive(Clone)]
enum SplitRow {
    FileHeader {
        path: String,
        old_path: Option<String>,
        added: usize,
        removed: usize,
        is_binary: bool,
    },
    /// hunk 头、`\ No newline` 标记这类整行展示的内容。
    Band { file: usize, line: DiffLine },
    Collapsed {
        key: (usize, usize),
        count: usize,
        is_expanded: bool,
    },
    /// 左旧右新。某一侧为 None 表示那边是空白填充。
    Pair {
        file: usize,
        left: Option<SplitSide>,
        right: Option<SplitSide>,
    },
}

/// 并排视图里的一侧内容。
#[derive(Clone)]
struct SplitSide {
    line: DiffLine,
    inline: Vec<InlineSpan>,
}

/// 打平后的一行。
#[derive(Clone)]
enum FlatRow {
    FileHeader {
        path: String,
        old_path: Option<String>,
        added: usize,
        removed: usize,
        is_binary: bool,
    },
    Line {
        file: usize,
        line: DiffLine,
        /// 行内改动区间（字节偏移），空表示整行没有字符级差异。
        inline: Vec<InlineSpan>,
    },
    /// 被折叠的一批上下文行（或展开后用于收起的横条）。
    Collapsed {
        /// 文件下标 + 该区间第一行的新行号，展开时据此判断。
        key: (usize, usize),
        count: usize,
        /// true 表示这一条是「收起」，内容已经铺在它前面。
        is_expanded: bool,
    },
}

impl EventEmitter<CloseDiff> for DiffView {}
impl EventEmitter<JumpToSource> for DiffView {}

impl DiffView {
    pub fn new(document: crate::editor_view::DiffDocument) -> Self {
        let title = document.title;
        let diff = document.diff;
        // 记住打开时的文本，自动刷新第一次比对就能命中，不会白重建一次。
        let last_raw = Some(document.raw.clone());
        let mut view = Self {
            title,
            root: document.root,
            source_path: document.source_path,
            diff,
            rows: Vec::new(),
            highlighted_with: None,
            highlights: Vec::new(),
            old_highlights: Vec::new(),
            expanded: std::collections::HashSet::new(),
            scroll_handle: gpui::ScrollHandle::new(),
            layout: DiffLayout::Unified,
            split_rows: Vec::new(),
            self_weak: gpui::WeakEntity::new_invalid(),
            reload: None,
            reload_timer_started: false,
            hint_too_narrow: false,
            last_raw,
        };
        view.rebuild_rows();
        view
    }

    /// 按当前展开状态重建打平后的行。
    fn rebuild_rows(&mut self) {
        let mut rows = Vec::new();
        for (file_index, file) in self.diff.files.iter().enumerate() {
            rows.push(FlatRow::FileHeader {
                path: file.path.clone(),
                old_path: file.old_path.clone(),
                added: file.added,
                removed: file.removed,
                is_binary: file.is_binary,
            });

            // 先收集本文件的有效行（跳过文件头）。
            let content: Vec<&DiffLine> = file
                .lines
                .iter()
                .filter(|line| line.kind != DiffLineKind::FileHeader)
                .collect();
            // 每个下标的行内改动区间。相邻的「删 N / 增 M」按 min(N, M) 配对，
            // 这是 git 逐行 diff 的常见形态。
            let inline = inline_spans_for(&content);

            let mut index = 0;
            while index < content.len() {
                let line = content[index];

                if line.kind != DiffLineKind::Context {
                    rows.push(FlatRow::Line {
                        file: file_index,
                        line: line.clone(),
                        inline: inline[index].clone(),
                    });
                    index += 1;
                    continue;
                }

                // 连续上下文：超过阈值就折叠中间部分，头尾各留几行。
                let run_start = index;
                while index < content.len() && content[index].kind == DiffLineKind::Context {
                    index += 1;
                }
                let run_len = index - run_start;

                let keep = if run_len > COLLAPSE_THRESHOLD {
                    CONTEXT_KEPT.min(run_len / 2)
                } else {
                    run_len
                };

                // 头部上下文。
                for offset in run_start..(run_start + keep) {
                    rows.push(FlatRow::Line {
                        file: file_index,
                        line: content[offset].clone(),
                        inline: inline[offset].clone(),
                    });
                }

                if run_len > COLLAPSE_THRESHOLD {
                    let hidden_start = run_start + keep;
                    let hidden_end = index - keep;
                    let hidden = hidden_end.saturating_sub(hidden_start);
                    if hidden > 0 {
                        // 文件下标也要编进 key，否则多个文件里同号的行会一起展开。
                        let key = (file_index, line.new_line.unwrap_or(0) as usize);
                        if self.expanded.contains(&key) {
                            // 收起条画在展开内容之后，两头都能点到。
                            rows.push(FlatRow::Collapsed {
                                key,
                                count: hidden,
                                is_expanded: true,
                            });
                            for offset in hidden_start..hidden_end {
                                rows.push(FlatRow::Line {
                                    file: file_index,
                                    line: content[offset].clone(),
                                    inline: inline[offset].clone(),
                                });
                            }
                        } else {
                            rows.push(FlatRow::Collapsed {
                                key,
                                count: hidden,
                                is_expanded: false,
                            });
                        }
                    }
                    // 尾部上下文。
                    for offset in (index - keep)..index {
                        rows.push(FlatRow::Line {
                            file: file_index,
                            line: content[offset].clone(),
                            inline: inline[offset].clone(),
                        });
                    }
                }
            }
        }
        self.split_rows = build_split_rows(&rows);
        self.rows = rows;
    }

    /// 按主题算一次语法高亮。
    ///
    /// 行号是文件内的绝对行号，所以按行号索引即可；重建内容时用空行补齐
    /// 跳过的区块，下标正好等于 行号 - 1。
    fn ensure_highlights(&mut self, theme: &gpui_component::Theme) {
        let signature = (theme.is_dark(), theme.highlight_theme.name.clone());
        if self.highlighted_with.as_ref() == Some(&signature) {
            return;
        }
        self.highlighted_with = Some(signature);
        let mono_font = theme.mono_font_family.clone();
        let font_size = ui_px(theme, 11.0);
        let mut highlights = Vec::with_capacity(self.diff.files.len());
        let mut old_highlights = Vec::with_capacity(self.diff.files.len());
        for file in &self.diff.files {
            let new_lines = cube_core::git::reconstructed_new_lines(file);
            let old_lines = cube_core::git::reconstructed_old_lines(file);
            highlights.push(highlight_lines(
                file, &new_lines, theme, &mono_font, font_size,
            ));
            old_highlights.push(highlight_lines(
                file, &old_lines, theme, &mono_font, font_size,
            ));
        }
        self.highlights = highlights;
        self.old_highlights = old_highlights;
    }

    /// 取一行的高亮样式。
    ///
    /// 新增行和上下文行走新版内容，删除行走旧版内容——两边行号体系不同，
    /// 混用会取到别的行的样式。
    fn line_runs(&self, file: usize, line: &DiffLine) -> Vec<TextRun> {
        let (source, number) = if line.kind == DiffLineKind::Removed {
            (&self.old_highlights, line.old_line)
        } else {
            (&self.highlights, line.new_line)
        };
        let Some(number) = number else {
            return Vec::new();
        };
        source
            .get(file)
            .and_then(|lines| lines.get((number as usize).saturating_sub(1)))
            .cloned()
            .unwrap_or_default()
    }

    /// 并排视图里按「这一侧是谁」取高亮。
    ///
    /// 左侧是旧文件、右侧是新文件；上下文行两边都存在，但行号可以不同，
    /// 必须按各自的行号体系取，否则跨 hunk 时会拿到别处的样式。
    fn side_runs(&self, file: usize, side: &SplitSide, is_left: bool) -> Vec<TextRun> {
        let (source, number) = match (is_left, side.line.kind) {
            // 左侧的删除行来自旧文件。
            (true, DiffLineKind::Removed) => (&self.old_highlights, side.line.old_line),
            // 上下文行两边文本相同，走新版内容即可；旧版内容里跳过的区块
            // 是空行，反而会丢掉高亮。
            // 其余情况按新版内容取；新增行只有新行号。
            _ => (&self.highlights, side.line.new_line),
        };
        let Some(number) = number else {
            return Vec::new();
        };
        source
            .get(file)
            .and_then(|lines| lines.get((number as usize).saturating_sub(1)))
            .cloned()
            .unwrap_or_default()
    }

    /// 构造传给 `render_line` 的跳转目标。解析不出路径时返回 None，行号栏不可点。
    fn jump_target(&self, file: usize) -> Option<(&gpui::WeakEntity<DiffView>, usize)> {
        self.resolve_path(file)?;
        Some((&self.self_weak, file))
    }

    /// 设置自动刷新来源。只有工作区 diff 能自动刷新，提交 diff 不传。
    pub fn set_reload(&mut self, reload: Option<DiffReload>) {
        self.reload = reload;
    }

    /// 启动自动刷新：盯着仓库工作区，文件一变就重拉 diff。
    ///
    /// 只在工作区 diff 上启用；提交 diff 是历史快照，重拉没有意义。
    pub(crate) fn start_auto_reload(&mut self, cx: &mut Context<Self>) {
        let Some(reload) = self.reload.clone() else {
            return;
        };
        if self.reload_timer_started {
            return;
        }
        self.reload_timer_started = true;

        // 用 notify 监视仓库，事件只置一个标志，由轮询任务节流后重拉。
        // 直接对每个事件重跑 git 会在保存时打好几次 diff。
        let signal = reload.signal.clone();
        let watch_target = reload
            .change
            .as_ref()
            .map(|change| reload.root.join(&change.path))
            .unwrap_or_else(|| reload.root.clone());

        // 监视线程靠 stop 标志退出：标签关掉后轮询任务结束，
        // 线程下一次醒来就能收尾，不会一直挂着 watcher。
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let watcher_stop = stop.clone();
        std::thread::spawn(move || {
            let Ok(mut watcher) =
                notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                    if event.is_ok() {
                        signal.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                })
            else {
                return;
            };
            // 单文件就盯文件本身；整仓库盯根目录。
            let mode = if watch_target.is_file() {
                notify::RecursiveMode::NonRecursive
            } else {
                notify::RecursiveMode::Recursive
            };
            if watcher.watch(&watch_target, mode).is_err() {
                return;
            }
            // watcher 必须活着事件才会投递；用短睡眠轮询停止标志。
            while !watcher_stop.load(std::sync::atomic::Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        });

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(400))
                    .await;
                let fired = reload
                    .signal
                    .swap(false, std::sync::atomic::Ordering::Relaxed);
                let alive = this.update(cx, |this, cx| {
                    if fired {
                        this.reload_now(cx);
                    }
                });
                if alive.is_err() {
                    // 视图已经销毁，通知监视线程退出。
                    stop.store(true, std::sync::atomic::Ordering::Relaxed);
                    return;
                }
            }
        })
        .detach();
    }

    /// 重新从 git 读当前 diff 并重建行。
    fn reload_now(&mut self, cx: &mut Context<Self>) {
        let Some(reload) = self.reload.clone() else {
            return;
        };
        // 记住展开状态和滚动位置，刷新后尽量保持原地。
        let scroll = self.scroll_handle.offset();
        cx.spawn(async move |this, cx| {
            let root = reload.root.clone();
            let change = reload.change.clone();
            let text = cx
                .background_executor()
                .spawn(async move {
                    match change {
                        Some(change) => {
                            cube_core::git::working_diff(&root, &change).map(|d| d.text)
                        }
                        None => cube_core::git::working_diff_all(&root),
                    }
                })
                .await;
            let Ok(text) = text else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                // 文本没变就不重算：语法高亮和 rebuild_rows 都不便宜，
                // 而且重建会把滚动位置打回原处。
                if this.last_raw.as_deref() == Some(text.as_str()) {
                    return;
                }
                this.last_raw = Some(text.clone());
                this.diff = cube_core::git::parse_diff(&text);
                this.highlighted_with = None;
                this.rebuild_rows();
                // 行数变了，滚动位置可能越界，让 gpui 自己收拢。
                this.scroll_handle
                    .set_offset(gpui::point(scroll.x, scroll.y));
                cx.notify();
            });
        })
        .detach();
    }

    /// 每个文件段在当前排布里的起始行下标。
    fn file_row_indices(&self) -> Vec<usize> {
        match self.layout {
            DiffLayout::Unified => self
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| matches!(row, FlatRow::FileHeader { .. }))
                .map(|(index, _)| index)
                .collect(),
            DiffLayout::SideBySide => self
                .split_rows
                .iter()
                .enumerate()
                .filter(|(_, row)| matches!(row, SplitRow::FileHeader { .. }))
                .map(|(index, _)| index)
                .collect(),
        }
    }

    /// 滚到下一个文件段；已经是最后一个就绕回开头。
    ///
    /// 偏移按当前排布的行高累加，和 `jump_to_change` 用同一套坐标。
    fn scroll_to_next_file(&mut self, cx: &mut Context<Self>) {
        let files = self.file_row_indices();
        if files.is_empty() {
            return;
        }
        let offsets = self.row_offsets(cx.theme());
        let scroll_top = -self.scroll_handle.offset().y.as_f32();

        // 当前顶端所在的文件段，往下走一个。
        let current = files
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                let offset = offsets.get(**row).copied().unwrap_or(0.0) as f64;
                offset <= f64::from(scroll_top) + 1.0
            })
            .map(|(slot, _)| slot)
            .next_back()
            .unwrap_or(0);
        let next = (current + 1) % files.len();

        let offset_y = px(-(offsets.get(files[next]).copied().unwrap_or(0.0)));
        self.scroll_handle
            .set_offset(gpui::point(px(0.0), offset_y));
        cx.notify();
    }

    /// 目标行号：新行号优先，删除行退回旧行号。
    ///
    /// 删除行在新文件里已经没有位置，退到旧行号至少能落到改动附近。
    fn target_line(line: &DiffLine) -> Option<u32> {
        line.new_line.or(line.old_line)
    }

    /// 把 diff 里的相对路径解析成绝对路径。
    ///
    /// 优先用仓库根；没有根时退回打开时的源文件路径（单文件 diff 常见）。
    fn resolve_path(&self, file: usize) -> Option<PathBuf> {
        let entry = self.diff.files.get(file)?;
        // 有仓库根时按 diff 里的相对路径拼，多文件 diff 每个文件各自正确。
        if let Some(root) = self.root.as_ref() {
            return Some(root.join(entry.path.as_str()));
        }
        // 没有根时只能用打开时的单个文件路径。
        if self.diff.files.len() > 1 {
            return None;
        }
        self.source_path.clone()
    }

    /// 跳到某个文件的某一行。
    fn jump_to_source(&self, file: usize, line: Option<u32>, cx: &mut Context<Self>) {
        let Some(path) = self.resolve_path(file) else {
            return;
        };
        let Some(line) = line else {
            return;
        };
        cx.emit(JumpToSource {
            path,
            line: line as usize,
        });
    }

    /// 所有改动行在当前排布里的下标（hunk 头也算，方便看清区块边界）。
    fn change_row_indices(&self) -> Vec<usize> {
        match self.layout {
            DiffLayout::Unified => self
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| {
                    matches!(
                        row,
                        FlatRow::Line { line, .. }
                            if matches!(
                                line.kind,
                                DiffLineKind::Added
                                    | DiffLineKind::Removed
                                    | DiffLineKind::HunkHeader
                            )
                    )
                })
                .map(|(index, _)| index)
                .collect(),
            DiffLayout::SideBySide => self
                .split_rows
                .iter()
                .enumerate()
                .filter(|(_, row)| match row {
                    SplitRow::Band { .. } => true,
                    // 两侧都是上下文才算没改。
                    SplitRow::Pair { left, right, .. } => !matches!(
                        (left, right),
                        (Some(left), Some(right)) if left.line.kind == DiffLineKind::Context
                    ),
                    _ => false,
                })
                .map(|(index, _)| index)
                .collect(),
        }
    }

    /// 当前排布下每个行下标对应的累计高度。
    ///
    /// 文件标题比普通行高，不能统一乘行高；折叠条和普通行等高。
    fn row_offsets(&self, theme: &gpui_component::Theme) -> Vec<f32> {
        let row_height = line_height(theme).as_f32();
        let header_height = file_header_height(theme).as_f32();
        let is_header = |header: bool| if header { header_height } else { row_height };
        let mut offsets = Vec::new();
        let mut cursor = 0.0;
        match self.layout {
            DiffLayout::Unified => {
                for row in &self.rows {
                    offsets.push(cursor);
                    cursor += is_header(matches!(row, FlatRow::FileHeader { .. }));
                }
            }
            DiffLayout::SideBySide => {
                for row in &self.split_rows {
                    offsets.push(cursor);
                    cursor += is_header(matches!(row, SplitRow::FileHeader { .. }));
                }
            }
        }
        offsets
    }

    /// 跳到下一个改动。`forward` 为 false 时反向。
    ///
    /// 位置按各行的实际高度累加，不依赖布局完成，所以点击即可跳。
    fn jump_to_change(&mut self, forward: bool, cx: &mut Context<Self>) {
        let changes = self.change_row_indices();
        if changes.is_empty() {
            return;
        }
        let offsets = self.row_offsets(cx.theme());
        if offsets.is_empty() {
            return;
        }

        // 用滚动偏移找当前顶端那一行。offsets 单调递增，线性找足够。
        let scroll_top = -self.scroll_handle.offset().y.as_f32();
        let current_top = offsets
            .iter()
            .rposition(|offset| *offset <= scroll_top + 0.5)
            .unwrap_or(0);

        let target = if forward {
            changes
                .iter()
                .copied()
                .find(|index| *index > current_top)
                // 已经在最后一个改动之后，绕回开头。
                .unwrap_or(changes[0])
        } else {
            changes
                .iter()
                .rev()
                .copied()
                .find(|index| *index < current_top)
                // 已经在第一个改动之前，绕回末尾。
                .unwrap_or(*changes.last().unwrap_or(&changes[0]))
        };

        let offset_y = px(-offsets.get(target).copied().unwrap_or(0.0));
        self.scroll_handle
            .set_offset(gpui::point(px(0.0), offset_y));
        cx.notify();
    }

    fn added_text(&self) -> String {
        format!("+{}", self.diff.added)
    }

    fn removed_text(&self) -> String {
        format!("−{}", self.diff.removed)
    }
}

/// 把单栏的行重排成并排的行。
///
/// 一段连续的删除和紧随其后的新增按顺序左右对齐；多出来的一侧填空白。
/// 这样左右两栏行号能对上，扫一眼就知道哪一行被替换成了什么。
fn build_split_rows(rows: &[FlatRow]) -> Vec<SplitRow> {
    let mut split = Vec::new();
    let mut index = 0;
    while index < rows.len() {
        match &rows[index] {
            FlatRow::FileHeader {
                path,
                old_path,
                added,
                removed,
                is_binary,
            } => {
                split.push(SplitRow::FileHeader {
                    path: path.clone(),
                    old_path: old_path.clone(),
                    added: *added,
                    removed: *removed,
                    is_binary: *is_binary,
                });
                index += 1;
            }
            FlatRow::Collapsed {
                key,
                count,
                is_expanded,
            } => {
                split.push(SplitRow::Collapsed {
                    key: *key,
                    count: *count,
                    is_expanded: *is_expanded,
                });
                index += 1;
            }
            FlatRow::Line { file, line, .. } if line.kind == DiffLineKind::Context => {
                let side = SplitSide {
                    line: line.clone(),
                    inline: Vec::new(),
                };
                split.push(SplitRow::Pair {
                    file: *file,
                    left: Some(side.clone()),
                    right: Some(side),
                });
                index += 1;
            }
            // hunk 头、`\ No newline` 标记：左右都占满，整行展示。
            FlatRow::Line { file, line, .. } => {
                let file = *file;
                let kind = line.kind;
                // 下面两条收集循环按 file 过滤，避免跨文件误配。
                if kind != DiffLineKind::Removed && kind != DiffLineKind::Added {
                    split.push(SplitRow::Band {
                        file,
                        line: line.clone(),
                    });
                    index += 1;
                    continue;
                }

                // 收集一段连续的删除，再收集紧跟的一段连续新增。
                let mut removed: Vec<SplitSide> = Vec::new();
                while index < rows.len() {
                    match &rows[index] {
                        FlatRow::Line {
                            file: f,
                            line,
                            inline,
                        } if *f == file && line.kind == DiffLineKind::Removed => {
                            removed.push(SplitSide {
                                line: line.clone(),
                                inline: inline.clone(),
                            });
                            index += 1;
                        }
                        _ => break,
                    }
                }
                let mut added: Vec<SplitSide> = Vec::new();
                while index < rows.len() {
                    match &rows[index] {
                        FlatRow::Line {
                            file: f,
                            line,
                            inline,
                        } if *f == file && line.kind == DiffLineKind::Added => {
                            added.push(SplitSide {
                                line: line.clone(),
                                inline: inline.clone(),
                            });
                            index += 1;
                        }
                        _ => break,
                    }
                }

                let pair_count = removed.len().max(added.len());
                for pair in 0..pair_count {
                    split.push(SplitRow::Pair {
                        file,
                        left: removed.get(pair).cloned(),
                        right: added.get(pair).cloned(),
                    });
                }
            }
        }
    }
    split
}

/// 渲染并排视图的一行：左旧右新，中间一条分隔线。
fn render_split_row(
    theme: &gpui_component::Theme,
    mono_font: &SharedString,
    left: Option<&SplitSide>,
    left_runs: Vec<TextRun>,
    right: Option<&SplitSide>,
    right_runs: Vec<TextRun>,
    jump: Option<(&gpui::WeakEntity<DiffView>, usize)>,
) -> AnyElement {
    let half = |side: Option<&SplitSide>, runs: Vec<TextRun>, gutter: Gutter| -> AnyElement {
        let Some(side) = side else {
            // 空侧：画一条淡淡的底色，让「这里没有对应行」一眼可见。
            return div()
                .flex_1()
                .min_w(px(0.0))
                .h_full()
                .bg(theme
                    .foreground
                    .opacity(if theme.is_dark() { 0.025 } else { 0.02 }))
                .into_any_element();
        };
        render_line(
            theme,
            mono_font,
            &side.line,
            runs,
            &side.inline,
            gutter,
            jump,
        )
    };

    div()
        .h(line_height(theme))
        .w_full()
        .flex()
        .items_center()
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .h_full()
                .flex()
                .items_center()
                .overflow_hidden()
                .child(half(left, left_runs, Gutter::Old)),
        )
        .child(
            div()
                .w(px(1.0))
                .h_full()
                .flex_shrink_0()
                .bg(theme.border.opacity(0.7)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .h_full()
                .flex()
                .items_center()
                .overflow_hidden()
                .child(half(right, right_runs, Gutter::New)),
        )
        .into_any_element()
}

/// 为每个内容行算出它的行内改动区间。
///
/// git 的逐行 diff 里，一段删除后面常常紧跟同样数量的新增。把这样的
/// 「删 N / 增 M」按 min(N, M) 一一配对做字符级比较；配不上的那些整行
/// 算改动，这是最直观的呈现。
fn inline_spans_for(content: &[&DiffLine]) -> Vec<Vec<InlineSpan>> {
    let mut spans: Vec<Vec<InlineSpan>> = vec![Vec::new(); content.len()];
    let mut index = 0;
    while index < content.len() {
        if content[index].kind != DiffLineKind::Removed {
            index += 1;
            continue;
        }
        let removed_start = index;
        while index < content.len() && content[index].kind == DiffLineKind::Removed {
            index += 1;
        }
        let removed_end = index;
        let added_start = index;
        while index < content.len() && content[index].kind == DiffLineKind::Added {
            index += 1;
        }
        let added_end = index;

        let removed_count = removed_end - removed_start;
        let added_count = added_end - added_start;
        let pairs = removed_count.min(added_count);
        for pair in 0..pairs {
            let old = &content[removed_start + pair];
            let new = &content[added_start + pair];
            let (old_spans, new_spans) = cube_core::git::inline_diff(&old.text, &new.text);
            // 配对不看内容，两个不相干的行也会被强行对应，字符级高亮铺满整行
            // 反而看不出重点。未改动部分占比太低就不配对，交给整行着色。
            if !mostly_unchanged(&old_spans, &new_spans, &old.text, &new.text) {
                continue;
            }
            spans[removed_start + pair] = old_spans;
            spans[added_start + pair] = new_spans;
        }
        // 没有配对的行保持为空 —— 渲染时按整行高亮。
    }
    spans
}

/// 两行的未改动部分是否占多数。
///
/// `inline_diff` 给的区间是字节偏移，直接按字节算比例即可——比例判断
/// 不需要精确到字符，多字节字符在两侧的权重是一致的。
fn mostly_unchanged(
    old_spans: &[InlineSpan],
    new_spans: &[InlineSpan],
    old: &str,
    new: &str,
) -> bool {
    let longest = old.len().max(new.len());
    if longest == 0 {
        return false;
    }
    let changed_old: usize = old_spans.iter().map(|span| span.end - span.start).sum();
    let changed_new: usize = new_spans.iter().map(|span| span.end - span.start).sum();
    // 两侧各取「未改动字节」的较大者，和最长行比。
    let unchanged_old = old.len().saturating_sub(changed_old);
    let unchanged_new = new.len().saturating_sub(changed_new);
    unchanged_old.max(unchanged_new) * 10 >= longest * 3
}

/// 给一批按行号对齐的内容算逐行语法高亮。
fn highlight_lines(
    file: &DiffFile,
    lines: &[String],
    theme: &gpui_component::Theme,
    mono_font: &SharedString,
    font_size: gpui::Pixels,
) -> Vec<Vec<TextRun>> {
    if lines.is_empty() {
        return Vec::new();
    }
    let language = crate::editor_syntax::language_for_path(std::path::Path::new(&file.path));
    let text = lines.join("\n");
    crate::editor_syntax::highlighted_line_runs(
        &text,
        lines,
        language,
        theme,
        mono_font.clone(),
        font_size,
        line_height(theme),
    )
}

/// 增删行的背景色。暗色主题下用低透明度，避免糊住文字。
fn row_background(kind: DiffLineKind, theme: &gpui_component::Theme) -> Hsla {
    let is_dark = theme.is_dark();
    match kind {
        DiffLineKind::Added => theme.success.opacity(if is_dark { 0.14 } else { 0.11 }),
        DiffLineKind::Removed => theme.danger.opacity(if is_dark { 0.14 } else { 0.10 }),
        DiffLineKind::HunkHeader => theme.foreground.opacity(if is_dark { 0.05 } else { 0.035 }),
        _ => theme.transparent,
    }
}

/// 增删行左侧色条。比背景更饱和，让改动边界一眼可辨。
fn stripe_color(kind: DiffLineKind, theme: &gpui_component::Theme) -> Hsla {
    match kind {
        DiffLineKind::Added => theme.success.opacity(0.8),
        DiffLineKind::Removed => theme.danger.opacity(0.8),
        _ => theme.transparent,
    }
}

impl Render for DiffView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        // 并排视图每侧至少要有这么多像素才排得下代码；窄了自动退回单栏。
        let available_width = window.bounds().size.width.as_f32();
        let is_too_narrow = available_width < SPLIT_MIN_WIDTH;
        let effective_layout = if self.layout == DiffLayout::SideBySide && is_too_narrow {
            DiffLayout::Unified
        } else {
            self.layout
        };
        // 窗口重新变宽后提示就该消失。
        if !is_too_narrow {
            self.hint_too_narrow = false;
        }

        if self.diff.files.is_empty() {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(ui_px(&theme, 12.0))
                .text_color(theme.muted_foreground)
                .child("没有可显示的差异")
                .into_any_element();
        }

        // 首次渲染时缓存自身弱引用，供行号点击回调使用。
        self.self_weak = cx.weak_entity();
        self.ensure_highlights(&theme);

        // 直接铺开所有行。虚拟列表放进实体化子视图后算不出可见范围
        // （表现为整片空白）；diff 通常几百到几千行，普通 flex 足够。
        let mono_font = theme.mono_font_family.clone();
        let mut rows = div()
            .id("diff-rows")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll_handle);

        match effective_layout {
            DiffLayout::Unified => {
                for row in &self.rows {
                    rows = rows.child(match row {
                        FlatRow::FileHeader {
                            path,
                            old_path,
                            added,
                            removed,
                            is_binary,
                        } => render_file_header(
                            &theme,
                            path,
                            old_path.as_deref(),
                            *added,
                            *removed,
                            *is_binary,
                        ),
                        FlatRow::Collapsed {
                            key,
                            count,
                            is_expanded,
                        } => render_collapsed(&theme, *key, *count, *is_expanded, cx),
                        FlatRow::Line { file, line, inline } => {
                            let runs = self.line_runs(*file, line);
                            render_line(
                                &theme,
                                &mono_font,
                                line,
                                runs,
                                inline,
                                Gutter::Both,
                                self.jump_target(*file),
                            )
                        }
                    });
                }
            }
            DiffLayout::SideBySide => {
                for row in &self.split_rows {
                    rows = rows.child(match row {
                        SplitRow::FileHeader {
                            path,
                            old_path,
                            added,
                            removed,
                            is_binary,
                        } => render_file_header(
                            &theme,
                            path,
                            old_path.as_deref(),
                            *added,
                            *removed,
                            *is_binary,
                        ),
                        SplitRow::Collapsed {
                            key,
                            count,
                            is_expanded,
                        } => render_collapsed(&theme, *key, *count, *is_expanded, cx),
                        SplitRow::Band { file, line } => render_line(
                            &theme,
                            &mono_font,
                            line,
                            Vec::new(),
                            &[],
                            Gutter::Both,
                            self.jump_target(*file),
                        ),
                        SplitRow::Pair { file, left, right } => {
                            let left_runs = left
                                .as_ref()
                                .map(|side| self.side_runs(*file, side, true))
                                .unwrap_or_default();
                            let right_runs = right
                                .as_ref()
                                .map(|side| self.side_runs(*file, side, false))
                                .unwrap_or_default();
                            render_split_row(
                                &theme,
                                &mono_font,
                                left.as_ref(),
                                left_runs,
                                right.as_ref(),
                                right_runs,
                                self.jump_target(*file),
                            )
                        }
                    });
                }
            }
        }

        div()
            .id("diff-view")
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .child(self.render_toolbar(&theme, effective_layout, cx))
            .child(rows)
            .into_any_element()
    }
}

impl DiffView {
    fn render_toolbar(
        &self,
        theme: &gpui_component::Theme,
        effective_layout: DiffLayout,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .h(px(32.0))
            .px(px(10.0))
            .flex_shrink_0()
            .border_b_1()
            .border_color(theme.border.opacity(0.6))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .text_size(ui_px(theme, 11.5))
                    .font_family(theme.font_family.clone())
                    .text_color(theme.foreground)
                    .child(self.title.clone()),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(ui_px(theme, 10.5))
                    .font_family(theme.mono_font_family.clone())
                    .text_color(theme.success)
                    .child(self.added_text()),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(ui_px(theme, 10.5))
                    .font_family(theme.mono_font_family.clone())
                    .text_color(theme.danger)
                    .child(self.removed_text()),
            )
            // 多文件 diff 的文件导航。点击逐个跳到下一个文件段，
            // 比在一屏几百行里滚要靠得住。
            .when(self.diff.files.len() > 1, |bar| {
                bar.child(
                    Button::new("diff-files")
                        .label(format!("{} 个文件", self.diff.files.len()))
                        .ghost()
                        .small()
                        .tooltip("跳到下一个文件")
                        .on_click(cx.listener(|this, _, _, cx| this.scroll_to_next_file(cx))),
                )
            })
            .when(self.hint_too_narrow, |bar| {
                bar.child(
                    div()
                        .flex_shrink_0()
                        .text_size(ui_px(theme, 9.5))
                        .font_family(theme.font_family.clone())
                        .text_color(theme.muted_foreground.opacity(0.85))
                        .child("窗口太窄，已退回单栏"),
                )
            })
            // 单栏 / 并排切换。窗口太窄时并排会被降级，按钮如实反映当前排布。
            .child(
                Button::new("diff-layout")
                    .label(match effective_layout {
                        DiffLayout::Unified => "并排",
                        DiffLayout::SideBySide => "单栏",
                    })
                    .ghost()
                    .small()
                    .tooltip(match (self.layout, effective_layout) {
                        (DiffLayout::SideBySide, DiffLayout::Unified) => "窗口太窄，已退回单栏",
                        (DiffLayout::Unified, _) => "切换为左右并排对比",
                        _ => "切换为上下单栏对比",
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        // 按用户真正存储的排布切换，而不是按被降级后的显示状态，
                        // 否则窄窗口下点一次会看起来毫无反应。
                        this.layout = match this.layout {
                            DiffLayout::Unified => DiffLayout::SideBySide,
                            DiffLayout::SideBySide => DiffLayout::Unified,
                        };
                        this.hint_too_narrow = false;
                        // 窄窗口下切到并排会被立刻降级，提示一下。
                        if this.layout == DiffLayout::SideBySide
                            && window.bounds().size.width.as_f32() < SPLIT_MIN_WIDTH
                        {
                            this.hint_too_narrow = true;
                        }
                        // 两种排布行数不同，滚动位置要回到顶部免得看到半截。
                        this.scroll_handle.set_offset(gpui::point(px(0.0), px(0.0)));
                        cx.notify();
                    })),
            )
            // 跳到上一个 / 下一个改动。长 diff 里比手动滚动快得多。
            .child(
                Button::new("diff-prev-change")
                    .icon(gpui_component::Icon::default().path("phosphor/caret-up.svg"))
                    .ghost()
                    .small()
                    .tooltip("上一个改动")
                    .on_click(cx.listener(|this, _, _, cx| this.jump_to_change(false, cx))),
            )
            .child(
                Button::new("diff-next-change")
                    .icon(gpui_component::Icon::default().path("phosphor/caret-down.svg"))
                    .ghost()
                    .small()
                    .tooltip("下一个改动")
                    .on_click(cx.listener(|this, _, _, cx| this.jump_to_change(true, cx))),
            )
            .child(
                Button::new("diff-close")
                    .label("关闭")
                    .ghost()
                    .small()
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(CloseDiff))),
            )
            .into_any_element()
    }
}

fn render_file_header(
    theme: &gpui_component::Theme,
    path: &str,
    old_path: Option<&str>,
    added: usize,
    removed: usize,
    is_binary: bool,
) -> AnyElement {
    // 重命名要把旧路径也显示出来，否则只看到一个陌生的新名字。
    let title = match old_path {
        Some(old) if old != path => format!("{old} → {path}"),
        _ => path.to_string(),
    };

    let badge = if is_binary {
        Some(("二进制", theme.muted_foreground))
    } else if added == 0 && removed == 0 {
        Some(("无文本变化", theme.muted_foreground))
    } else {
        None
    };

    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .h(file_header_height(theme))
        .px(px(10.0))
        .bg(theme
            .foreground
            .opacity(if theme.is_dark() { 0.06 } else { 0.04 }))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_size(ui_px(theme, 11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .font_family(theme.font_family.clone())
                .text_color(theme.foreground)
                .child(title),
        )
        .when_some(badge, |row, (label, color)| {
            row.child(
                div()
                    .flex_shrink_0()
                    .px(px(5.0))
                    .rounded(px(3.0))
                    .bg(theme.foreground.opacity(0.08))
                    .text_size(ui_px(theme, 9.0))
                    .font_family(theme.font_family.clone())
                    .text_color(color.opacity(0.9))
                    .child(label),
            )
        })
        .when(!is_binary, |row| {
            row.child(
                div()
                    .flex_shrink_0()
                    .text_size(ui_px(theme, 10.0))
                    .font_family(theme.mono_font_family.clone())
                    .text_color(theme.success)
                    .child(format!("+{added}")),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(ui_px(theme, 10.0))
                    .font_family(theme.mono_font_family.clone())
                    .text_color(theme.danger)
                    .child(format!("−{removed}")),
            )
        })
        .into_any_element()
}

/// 折叠区块：一条可点击的横条。
///
/// `is_expanded` 为 true 时画在展开后的区块下方，点击可重新收起。
fn render_collapsed(
    theme: &gpui_component::Theme,
    key: (usize, usize),
    count: usize,
    is_expanded: bool,
    cx: &mut Context<DiffView>,
) -> AnyElement {
    let label = if is_expanded {
        format!("⋯ 收起 {count} 行未改动的内容")
    } else {
        format!("⋯ 展开 {count} 行未改动的内容")
    };
    div()
        .id(SharedString::from(format!(
            "diff-collapsed-{}-{}",
            key.0, key.1
        )))
        .flex()
        .items_center()
        .h(line_height(theme))
        .w_full()
        .px(px(10.0))
        .cursor_pointer()
        .bg(theme
            .foreground
            .opacity(if theme.is_dark() { 0.04 } else { 0.03 }))
        .hover(|style| style.bg(theme.primary.opacity(0.10)))
        .child(
            div()
                .text_size(ui_px(theme, 10.0))
                .font_family(theme.mono_font_family.clone())
                .text_color(theme.primary.opacity(0.85))
                .child(label),
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            if is_expanded {
                this.expanded.remove(&key);
            } else {
                this.expanded.insert(key);
            }
            this.rebuild_rows();
            cx.notify();
        }))
        .into_any_element()
}

/// 按字节区间裁剪语法高亮的 TextRun。
///
/// `runs` 是连续覆盖整行的，长度之和等于行字节数；这里切出 `[start, end)` 的部分。
fn slice_runs(runs: &[TextRun], start: usize, end: usize) -> Vec<TextRun> {
    let mut sliced = Vec::new();
    let mut cursor = 0;
    for run in runs {
        let run_start = cursor;
        let run_end = cursor + run.len;
        cursor = run_end;
        let lo = run_start.max(start);
        let hi = run_end.min(end);
        if lo >= hi {
            continue;
        }
        let mut part = run.clone();
        part.len = hi - lo;
        sliced.push(part);
    }
    sliced
}

/// 渲染一行正文，把行内改动片段标上底色。
///
/// 有字符级差异时把行切成「未改动 / 改动 / 未改动」若干段，改动段加底色，
/// 这是 VSCode 那种「这一行里具体改了哪几个字」的效果。
fn render_line_content(
    theme: &gpui_component::Theme,
    mono_font: &SharedString,
    text: &str,
    runs: &[TextRun],
    inline: &[InlineSpan],
    mark: Hsla,
) -> AnyElement {
    let base_color = theme.foreground.opacity(0.92);
    let font_size = ui_px(theme, 11.0);

    // 行内标记靠 TextRun 的背景色表达：gpui 的文本绘制会先画每个 run 的背景，
    // 所以整行仍然是一次 layout、一个字符串，等宽对齐天然成立。
    let styled_runs = if inline.is_empty() || text.is_empty() {
        runs.to_vec()
    } else {
        marked_runs(text, runs, inline, mark)
    };

    let content = text_content(base_color, &styled_runs, text);
    div()
        .flex_1()
        .min_w(px(0.0))
        .flex()
        .items_center()
        .overflow_hidden()
        .text_size(font_size)
        .font_family(mono_font.clone())
        .child(content)
        .into_any_element()
}

/// 按改动区间切分高亮 run，给改动段加上背景色。
///
/// 输出仍然连续覆盖整行、字节数之和等于 `text.len()`，可以直接交给
/// `StyledText` 一次性排版。
fn marked_runs(text: &str, runs: &[TextRun], inline: &[InlineSpan], mark: Hsla) -> Vec<TextRun> {
    // 边界收拢到字符边界，避免切出非法切片。
    let mut bounds: Vec<(usize, usize)> = Vec::new();
    for span in inline {
        let start = floor_boundary(text, span.start);
        let end = floor_boundary(text, span.end);
        if start < end {
            bounds.push((start, end));
        }
    }
    if bounds.is_empty() {
        return runs.to_vec();
    }

    let mut marked = Vec::with_capacity(runs.len() + bounds.len());
    let mut cursor = 0;
    for (start, end) in bounds {
        if start > cursor {
            marked.extend(slice_runs(runs, cursor, start));
        }
        marked.extend(slice_runs(runs, start, end).into_iter().map(|mut run| {
            run.background_color = Some(mark);
            run
        }));
        cursor = end;
    }
    if cursor < text.len() {
        marked.extend(slice_runs(runs, cursor, text.len()));
    }
    marked
}

/// 把下标向前收拢到最近的字符边界，避免切出非法 str 切片。
fn floor_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// 一段文本；有语法高亮就用 `StyledText`，否则用基础色。
fn text_content(base_color: Hsla, runs: &[TextRun], text: &str) -> AnyElement {
    let text = SharedString::from(text.to_string());
    if runs.is_empty() {
        div().text_color(base_color).child(text).into_any_element()
    } else {
        div()
            .child(gpui::StyledText::new(text).with_runs(runs.to_vec()))
            .into_any_element()
    }
}

fn render_line(
    theme: &gpui_component::Theme,
    mono_font: &SharedString,
    line: &DiffLine,
    runs: Vec<TextRun>,
    inline: &[InlineSpan],
    gutter: Gutter,
    jump: Option<(&gpui::WeakEntity<DiffView>, usize)>,
) -> AnyElement {
    let background = row_background(line.kind, theme);

    // 二进制说明行：只有一行占位文字，没有行号也没有增删符号。
    if line.kind == DiffLineKind::Binary {
        return div()
            .h(line_height(theme))
            .w_full()
            .flex()
            .items_center()
            .px(px(10.0))
            .bg(background)
            .child(
                div()
                    .min_w(px(0.0))
                    .truncate()
                    .text_size(ui_px(theme, 10.0))
                    .font_family(mono_font.clone())
                    .text_color(theme.muted_foreground.opacity(0.75))
                    .child(line.text.clone()),
            )
            .into_any_element();
    }

    // hunk 头单独一行，用灰色小字，不加行号。
    if line.kind == DiffLineKind::HunkHeader {
        return div()
            .h(line_height(theme))
            .w_full()
            .flex()
            .items_center()
            .px(px(10.0))
            .bg(background)
            .child(
                div()
                    .min_w(px(0.0))
                    .truncate()
                    .text_size(ui_px(theme, 10.0))
                    .font_family(mono_font.clone())
                    .text_color(theme.muted_foreground.opacity(0.8))
                    .child(line.text.clone()),
            )
            .into_any_element();
    }

    let old_no = line.old_line.map(|n| n.to_string()).unwrap_or_default();
    let new_no = line.new_line.map(|n| n.to_string()).unwrap_or_default();
    let sign = match line.kind {
        DiffLineKind::Added => "+",
        DiffLineKind::Removed => "-",
        _ => " ",
    };
    let sign_color = match line.kind {
        DiffLineKind::Added => theme.success,
        DiffLineKind::Removed => theme.danger,
        _ => theme.muted_foreground,
    };

    // 正文分三段渲染：改动前 / 改动中 / 改动后。改动中的片段加一层底色，
    // 这是 VSCode 那种「这一行里具体改了哪几个字」的效果。
    let mark = match line.kind {
        DiffLineKind::Added => theme
            .success
            .opacity(if theme.is_dark() { 0.34 } else { 0.26 }),
        DiffLineKind::Removed => theme
            .danger
            .opacity(if theme.is_dark() { 0.34 } else { 0.22 }),
        _ => theme.transparent,
    };
    let content = render_line_content(theme, mono_font, &line.text, &runs, inline, mark);

    div()
        .h(line_height(theme))
        .w_full()
        .flex()
        .items_center()
        .bg(background)
        .child(
            div()
                .w(px(3.0))
                .h_full()
                .flex_shrink_0()
                .bg(stripe_color(line.kind, theme)),
        )
        .child({
            // 行号栏可点击：跳到源文件对应位置。没有路径信息时退回成纯文本。
            let clickable = jump.filter(|_| line.old_line.is_some() || line.new_line.is_some());
            let target_line = DiffView::target_line(line);
            let gutter_element = match gutter {
                // 单栏：旧号 | 新号。增行只显示新号，删行只显示旧号。
                Gutter::Both => div()
                    .w(gutter_width(theme))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .text_size(ui_px(theme, 9.5))
                    .font_family(mono_font.clone())
                    .text_color(theme.muted_foreground.opacity(0.55))
                    .child(div().w(px(32.0)).pr(px(6.0)).text_right().child(old_no))
                    .child(div().w(px(32.0)).pr(px(6.0)).text_right().child(new_no)),
                // 并排：每侧只留一个行号列，正文能多出 28px。
                Gutter::Old | Gutter::New => div()
                    .w(single_gutter_width(theme))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_end()
                    .pr(px(6.0))
                    .text_size(ui_px(theme, 9.5))
                    .font_family(mono_font.clone())
                    .text_color(theme.muted_foreground.opacity(0.55))
                    .child(if gutter == Gutter::Old {
                        old_no
                    } else {
                        new_no
                    }),
            };
            match (clickable, target_line) {
                (Some((view, file)), Some(line_no)) => {
                    let id = SharedString::from(format!("diff-gutter-{file}-{line_no}-{gutter:?}"));
                    let view = view.clone();
                    gutter_element
                        .id(id)
                        .cursor_pointer()
                        .hover(|style| style.text_color(theme.primary.opacity(0.9)))
                        .on_click(move |_, _, cx| {
                            let _ = view.update(cx, |this, cx| {
                                this.jump_to_source(file, Some(line_no), cx)
                            });
                        })
                        .into_any_element()
                }
                _ => gutter_element.into_any_element(),
            }
        })
        .child(
            div()
                .w(px(12.0))
                .flex_shrink_0()
                .text_size(ui_px(theme, 11.0))
                .font_family(mono_font.clone())
                .text_color(sign_color.opacity(0.8))
                .child(sign),
        )
        .child(content)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cube_core::git::parse_diff;

    fn make(raw: &str) -> DiffView {
        let diff = parse_diff(raw);
        let document = crate::editor_view::DiffDocument::new("t".into(), diff);
        DiffView::new(document)
    }

    #[test]
    fn empty_diff_has_no_rows() {
        let view = make("");
        assert!(view.rows.is_empty());
        assert!(view.diff.files.is_empty());
    }

    /// 短上下文不折叠。
    #[test]
    fn short_context_runs_are_not_collapsed() {
        let raw = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,3 +1,3 @@
 one
 two
-old
+new
";
        let view = make(raw);
        assert!(
            !view
                .rows
                .iter()
                .any(|row| matches!(row, FlatRow::Collapsed { .. })),
            "只有 2 行上下文，不该折叠"
        );
    }

    /// 连续 10 行上下文应当折叠中间部分。
    #[test]
    fn long_context_runs_collapse() {
        let mut raw =
            String::from("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,11 +1,11 @@\n");
        for i in 0..10 {
            raw.push_str(&format!(" ctx{i}\n"));
        }
        raw.push_str("-old\n+new\n");

        let view = make(&raw);
        let collapsed = view
            .rows
            .iter()
            .filter(|row| matches!(row, FlatRow::Collapsed { .. }))
            .count();
        assert_eq!(collapsed, 1, "10 行上下文应折叠成 1 个区块");
    }

    /// 展开后折叠行应当变回普通行。
    #[test]
    fn expanding_restores_hidden_lines() {
        let mut raw =
            String::from("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,11 +1,11 @@\n");
        for i in 0..10 {
            raw.push_str(&format!(" ctx{i}\n"));
        }
        raw.push_str("-old\n+new\n");

        let mut view = make(&raw);
        let before = view.rows.len();
        let key = view
            .rows
            .iter()
            .find_map(|row| match row {
                FlatRow::Collapsed { key, .. } => Some(*key),
                _ => None,
            })
            .expect("应当有折叠区块");

        view.expanded.insert(key);
        view.rebuild_rows();
        // 展开后多出一批上下文行，还多一条「收起」横条。
        assert!(view.rows.len() > before, "展开后行数应增加");
        assert!(
            view.rows.iter().any(|row| matches!(
                row,
                FlatRow::Collapsed {
                    is_expanded: true,
                    ..
                }
            )),
            "展开后应留一条收起横条"
        );
        assert!(
            view.rows.iter().any(|row| matches!(
                row,
                FlatRow::Line { line, .. } if line.text == "ctx1"
            )),
            "被折叠的行应当重新出现"
        );
    }

    /// 多文件 diff 的文件段下标必须与 FileHeader 数量一致。
    #[test]
    fn file_row_indices_track_each_file_header() {
        let raw = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,1 +1,1 @@
-old
+new
diff --git a/b.rs b/b.rs
--- a/b.rs
+++ b/b.rs
@@ -1,1 +1,1 @@
-x
+y
";
        let mut view = make(raw);
        let unified = view.file_row_indices();
        assert_eq!(unified.len(), 2);
        assert_eq!(unified[0], 0, "第一个文件段从第 0 行开始");
        assert!(matches!(view.rows[unified[1]], FlatRow::FileHeader { .. }));

        view.layout = DiffLayout::SideBySide;
        let split = view.file_row_indices();
        assert_eq!(split.len(), 2);
        assert!(matches!(
            view.split_rows[split[0]],
            SplitRow::FileHeader { .. }
        ));
    }

    /// 完全不相干的两行不该被强行配对（否则整行都被标成改动）。
    #[test]
    fn unrelated_lines_are_not_paired_for_inline_marks() {
        let raw = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,1 +1,1 @@
-fn completely_different_thing() {}
+let x = 42;
";
        let view = make(raw);
        // 没有配对 → 两行都没有行内区间。
        let has_inline = view.rows.iter().any(|row| {
            matches!(
                row,
                FlatRow::Line { inline, .. } if !inline.is_empty()
            )
        });
        assert!(!has_inline, "不相似的行不应产生字符级高亮");
    }

    /// 只改了几个字符的行应当配对。
    #[test]
    fn similar_lines_are_paired_for_inline_marks() {
        let raw = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,1 +1,1 @@
-let value = 1;
+let value = 2;
";
        let view = make(raw);
        let inline_count = view
            .rows
            .iter()
            .filter(|row| matches!(row, FlatRow::Line { inline, .. } if !inline.is_empty()))
            .count();
        assert_eq!(inline_count, 2, "删行和增行都应带字符级高亮");
    }

    /// 展开后重新收起，行数回到展开前。
    #[test]
    fn collapsing_again_restores_the_hidden_rows() {
        let mut raw =
            String::from("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,11 +1,11 @@\n");
        for i in 0..10 {
            raw.push_str(&format!(" ctx{i}\n"));
        }
        raw.push_str("-old\n+new\n");

        let mut view = make(&raw);
        let collapsed_rows = view.rows.len();
        let key = view
            .rows
            .iter()
            .find_map(|row| match row {
                FlatRow::Collapsed { key, .. } => Some(*key),
                _ => None,
            })
            .expect("应当有折叠区块");

        view.expanded.insert(key);
        view.rebuild_rows();
        assert_ne!(view.rows.len(), collapsed_rows);

        view.expanded.remove(&key);
        view.rebuild_rows();
        assert_eq!(view.rows.len(), collapsed_rows, "收起后应回到初始行数");
    }

    /// 两个文件里同号的上下文区块，折叠 key 必须不同，否则会一起展开。
    #[test]
    fn collapse_keys_are_unique_across_files() {
        let body = |path: &str| {
            let mut raw = format!(
                "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1,11 +1,11 @@\n"
            );
            for i in 0..10 {
                raw.push_str(&format!(" ctx{i}\n"));
            }
            raw.push_str("-old\n+new\n");
            raw
        };
        let raw = format!("{}{}", body("a.rs"), body("b.rs"));
        let view = make(&raw);

        let keys: Vec<_> = view
            .rows
            .iter()
            .filter_map(|row| match row {
                FlatRow::Collapsed { key, .. } => Some(*key),
                _ => None,
            })
            .collect();
        assert_eq!(keys.len(), 2, "两个文件各有一个折叠区块");
        assert_ne!(keys[0], keys[1], "跨文件的折叠 key 不能撞");
    }

    /// 连续删 N 增 M 在并排视图里左右配对，多出来的一侧留空。
    #[test]
    fn split_rows_pair_removed_and_added() {
        let raw = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,3 +1,2 @@
 ctx
-old1
-old2
+new1
";
        let view = make(raw);
        let pairs: Vec<_> = view
            .split_rows
            .iter()
            .filter_map(|row| match row {
                SplitRow::Pair { left, right, .. } => Some((left.clone(), right.clone())),
                _ => None,
            })
            .collect();

        // 一行上下文 + 两行配对。
        assert_eq!(pairs.len(), 3);
        assert!(pairs[0].0.is_some() && pairs[0].1.is_some());
        assert_eq!(pairs[1].0.as_ref().unwrap().line.text, "old1");
        assert_eq!(pairs[1].1.as_ref().unwrap().line.text, "new1");
        // 第二对只有左侧。
        assert_eq!(pairs[2].0.as_ref().unwrap().line.text, "old2");
        assert!(pairs[2].1.is_none());
    }

    /// 并排视图里的改动下标跳过纯上下文行。
    #[test]
    fn split_change_indices_skip_context_pairs() {
        let raw = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,2 +1,2 @@
 ctx
-old
+new
";
        let mut view = make(raw);
        view.layout = DiffLayout::SideBySide;
        let indices = view.change_row_indices();
        // hunk 头算一个改动点，配对的删增算一个；上下文行不算。
        assert_eq!(indices.len(), 2);
        assert!(matches!(view.split_rows[indices[0]], SplitRow::Band { .. }));
        assert!(matches!(view.split_rows[indices[1]], SplitRow::Pair { .. }));
    }
}
