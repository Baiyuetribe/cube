//! Git diff 查看器。
//!
//! 只读，支持单栏（unified）和并排（side-by-side）两种排布。比直接打开原始 diff 文本友好：
//! 增删行有背景色和左侧色条、行号分新旧两栏、文件段有标题和统计、
//! 连续未改动的上下文自动折叠。
//!
//! 不做编辑、光标、选区——这些在 diff 里没有意义。

use con_core::git::{DiffFile, DiffLine, DiffLineKind, InlineSpan, ParsedDiff};
use gpui::{
    AnyElement, Context, EventEmitter, FontWeight, Hsla, IntoElement, ParentElement, Render,
    SharedString, Styled, TextRun, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme, Sizable as _,
    button::{Button, ButtonVariants as _},
};

use crate::ui_scale::ui_px;

/// 用户关闭 diff 视图。
pub struct CloseDiff;

/// 行高。
const LINE_HEIGHT: f32 = 18.0;
/// 文件段标题行高。跳转时要按实际高度累加，不能统一用行高。
const FILE_HEADER_HEIGHT: f32 = 26.0;
/// 行号列宽度（新旧两栏）。
const GUTTER_WIDTH: f32 = 76.0;
/// 并排视图里单侧行号列的宽度。
const SINGLE_GUTTER_WIDTH: f32 = 48.0;
/// 连续未改动超过这个行数就折叠，只留头尾各两行做上下文。
const COLLAPSE_THRESHOLD: usize = 6;
/// 折叠时保留的上下文行数（上下各留这么多）。
const CONTEXT_KEPT: usize = 2;

pub struct DiffView {
    title: String,
    diff: ParsedDiff,
    /// 打平后的所有行，列表按这个渲染。
    rows: Vec<FlatRow>,
    /// 是否已经算过高亮。用显式标记而不是判空：全是二进制文件时
    /// `highlights` 会一直为空，否则每次渲染都要重算一遍。
    has_highlights: bool,
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
}

/// 行号栏的显示方式。
///
/// 单栏要同时显示新旧两个行号；并排时每一侧只需要自己那一个，省下的宽度留给正文。
#[derive(Clone, Copy, PartialEq, Eq)]
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
        added: usize,
        removed: usize,
    },
    /// hunk 头、`\ No newline` 标记这类整行展示的内容。
    Band {
        line: DiffLine,
    },
    Collapsed {
        key: (usize, usize),
        count: usize,
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
        added: usize,
        removed: usize,
    },
    Line {
        file: usize,
        line: DiffLine,
        /// 行内改动区间（字节偏移），空表示整行没有字符级差异。
        inline: Vec<InlineSpan>,
    },
    /// 被折叠的一批上下文行。
    Collapsed {
        /// 文件下标 + 该区间第一行的新行号，展开时据此判断。
        key: (usize, usize),
        count: usize,
    },
}

impl EventEmitter<CloseDiff> for DiffView {}

impl DiffView {
    pub fn new(document: crate::editor_view::DiffDocument) -> Self {
        let title = document.title;
        let diff = document.diff;
        let mut view = Self {
            title,
            diff,
            rows: Vec::new(),
            has_highlights: false,
            highlights: Vec::new(),
            old_highlights: Vec::new(),
            expanded: std::collections::HashSet::new(),
            scroll_handle: gpui::ScrollHandle::new(),
            layout: DiffLayout::Unified,
            split_rows: Vec::new(),
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
                added: file.added,
                removed: file.removed,
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
                            for offset in hidden_start..hidden_end {
                                rows.push(FlatRow::Line {
                                    file: file_index,
                                    line: content[offset].clone(),
                                    inline: inline[offset].clone(),
                                });
                            }
                        } else {
                            rows.push(FlatRow::Collapsed { key, count: hidden });
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
        if self.has_highlights {
            return;
        }
        self.has_highlights = true;
        let mono_font = theme.mono_font_family.clone();
        let font_size = ui_px(theme, 11.0);
        let mut highlights = Vec::with_capacity(self.diff.files.len());
        let mut old_highlights = Vec::with_capacity(self.diff.files.len());
        for file in &self.diff.files {
            let new_lines = con_core::git::reconstructed_new_lines(file);
            let old_lines = con_core::git::reconstructed_old_lines(file);
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
    fn row_offsets(&self) -> Vec<f32> {
        let is_header = |header: bool| {
            if header {
                FILE_HEADER_HEIGHT
            } else {
                LINE_HEIGHT
            }
        };
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
        let offsets = self.row_offsets();
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
                added,
                removed,
            } => {
                split.push(SplitRow::FileHeader {
                    path: path.clone(),
                    added: *added,
                    removed: *removed,
                });
                index += 1;
            }
            FlatRow::Collapsed { key, count } => {
                split.push(SplitRow::Collapsed {
                    key: *key,
                    count: *count,
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
                    split.push(SplitRow::Band { line: line.clone() });
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
    left: Option<SplitSide>,
    left_runs: Vec<TextRun>,
    right: Option<SplitSide>,
    right_runs: Vec<TextRun>,
) -> AnyElement {
    let half = |side: Option<SplitSide>, runs: Vec<TextRun>, gutter: Gutter| -> AnyElement {
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
        render_line(theme, mono_font, side.line, runs, side.inline, gutter)
    };

    div()
        .h(px(LINE_HEIGHT))
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
            let (old_spans, new_spans) = con_core::git::inline_diff(&old.text, &new.text);
            spans[removed_start + pair] = old_spans;
            spans[added_start + pair] = new_spans;
        }
        // 没有配对的行保持为空 —— 渲染时按整行高亮。
    }
    spans
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
        px(LINE_HEIGHT),
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();

        if self.diff.files.is_empty() {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(ui_px(&theme, 12.0))
                .text_color(theme.muted_foreground)
                .child("这个文件没有文本差异（可能是二进制或纯重命名）")
                .into_any_element();
        }

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

        match self.layout {
            DiffLayout::Unified => {
                for row_index in 0..self.rows.len() {
                    rows = rows.child(match self.rows[row_index].clone() {
                        FlatRow::FileHeader {
                            path,
                            added,
                            removed,
                        } => render_file_header(&theme, path, added, removed),
                        FlatRow::Collapsed { key, count } => {
                            render_collapsed(&theme, key, count, cx)
                        }
                        FlatRow::Line { file, line, inline } => {
                            let runs = self.line_runs(file, &line);
                            render_line(&theme, &mono_font, line, runs, inline, Gutter::Both)
                        }
                    });
                }
            }
            DiffLayout::SideBySide => {
                for row_index in 0..self.split_rows.len() {
                    rows = rows.child(match self.split_rows[row_index].clone() {
                        SplitRow::FileHeader {
                            path,
                            added,
                            removed,
                        } => render_file_header(&theme, path, added, removed),
                        SplitRow::Collapsed { key, count } => {
                            render_collapsed(&theme, key, count, cx)
                        }
                        SplitRow::Band { line } => render_line(
                            &theme,
                            &mono_font,
                            line,
                            Vec::new(),
                            Vec::new(),
                            Gutter::Both,
                        ),
                        SplitRow::Pair { file, left, right } => {
                            let left_runs = left
                                .as_ref()
                                .map(|side| self.line_runs(file, &side.line))
                                .unwrap_or_default();
                            let right_runs = right
                                .as_ref()
                                .map(|side| self.line_runs(file, &side.line))
                                .unwrap_or_default();
                            render_split_row(&theme, &mono_font, left, left_runs, right, right_runs)
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
            .child(self.render_toolbar(&theme, cx))
            .child(rows)
            .into_any_element()
    }
}

impl DiffView {
    fn render_toolbar(&self, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> AnyElement {
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
            // 单栏 / 并排切换。
            .child(
                Button::new("diff-layout")
                    .label(match self.layout {
                        DiffLayout::Unified => "并排",
                        DiffLayout::SideBySide => "单栏",
                    })
                    .ghost()
                    .small()
                    .tooltip(match self.layout {
                        DiffLayout::Unified => "切换为左右并排对比",
                        DiffLayout::SideBySide => "切换为上下单栏对比",
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.layout = match this.layout {
                            DiffLayout::Unified => DiffLayout::SideBySide,
                            DiffLayout::SideBySide => DiffLayout::Unified,
                        };
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
    path: String,
    added: usize,
    removed: usize,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .h(px(26.0))
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
                .child(path),
        )
        .child(
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
        .into_any_element()
}

/// 折叠区块：一条可点击的横条，展开后显示被隐藏的上下文。
fn render_collapsed(
    theme: &gpui_component::Theme,
    key: (usize, usize),
    count: usize,
    cx: &mut Context<DiffView>,
) -> AnyElement {
    div()
        .id(SharedString::from(format!(
            "diff-collapsed-{}-{}",
            key.0, key.1
        )))
        .flex()
        .items_center()
        .h(px(LINE_HEIGHT))
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
                .child(format!("⋯ 展开 {count} 行未改动的内容")),
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.expanded.insert(key);
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

    let wrapper = div()
        .flex_1()
        .min_w(px(0.0))
        .flex()
        .items_center()
        .overflow_hidden()
        .text_size(font_size)
        .font_family(mono_font.clone());

    // 没有字符级差异（或拿不到行内容）时整行一个样式。
    if inline.is_empty() || text.is_empty() {
        let content = text_content(base_color, runs, text);
        return wrapper.child(content).into_any_element();
    }

    // 按改动区间切段，区间之外是未改动部分。
    let mut segments: Vec<(usize, usize, bool)> = Vec::new();
    let mut cursor = 0;
    for span in inline {
        let span_start = span.start.min(text.len());
        let span_end = span.end.min(text.len());
        if span_start >= span_end {
            continue;
        }
        if span_start > cursor {
            segments.push((cursor, span_start, false));
        }
        segments.push((span_start, span_end, true));
        cursor = span_end;
    }
    if cursor < text.len() {
        segments.push((cursor, text.len(), false));
    }

    let mut row = wrapper;
    for (start, end, is_changed) in segments {
        // 切分点可能落在多字节字符中间，向前后收拢到最近的字符边界。
        let start = floor_boundary(text, start);
        let end = floor_boundary(text, end);
        if start >= end {
            continue;
        }
        let slice = &text[start..end];
        let segment_runs = slice_runs(runs, start, end);
        let content = text_content(base_color, &segment_runs, slice);
        // 不能加左右 padding：背景是画在 div 上的，padding 会把后面的字符推走，
        // 等宽对齐就断了。
        row = row.child(
            div()
                .when(is_changed, |style| style.bg(mark).rounded(px(2.0)))
                .child(content),
        );
    }
    row.into_any_element()
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
    line: DiffLine,
    runs: Vec<TextRun>,
    inline: Vec<InlineSpan>,
    gutter: Gutter,
) -> AnyElement {
    let background = row_background(line.kind, theme);

    // hunk 头单独一行，用灰色小字，不加行号。
    if line.kind == DiffLineKind::HunkHeader {
        return div()
            .h(px(LINE_HEIGHT))
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
                    .child(line.text),
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
    let content = render_line_content(theme, mono_font, &line.text, &runs, &inline, mark);

    div()
        .h(px(LINE_HEIGHT))
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
        .child(match gutter {
            // 单栏：旧号 | 新号。增行只显示新号，删行只显示旧号。
            Gutter::Both => div()
                .w(px(GUTTER_WIDTH))
                .flex_shrink_0()
                .flex()
                .items_center()
                .text_size(ui_px(theme, 9.5))
                .font_family(mono_font.clone())
                .text_color(theme.muted_foreground.opacity(0.55))
                .child(div().w(px(32.0)).pr(px(6.0)).text_right().child(old_no))
                .child(div().w(px(32.0)).pr(px(6.0)).text_right().child(new_no))
                .into_any_element(),
            // 并排：每侧只留一个行号列，正文能多出 28px。
            Gutter::Old | Gutter::New => div()
                .w(px(SINGLE_GUTTER_WIDTH))
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
                })
                .into_any_element(),
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
    use con_core::git::parse_diff;

    fn make(raw: &str) -> DiffView {
        let diff = parse_diff(raw);
        let document = crate::editor_view::DiffDocument {
            title: "t".into(),
            diff,
        };
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
        assert!(view.rows.len() > before, "展开后行数应增加");
        assert!(
            !view
                .rows
                .iter()
                .any(|row| matches!(row, FlatRow::Collapsed { .. }))
        );
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
