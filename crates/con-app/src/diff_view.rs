//! Git diff 查看器。
//!
//! 只读、单栏（unified）。比直接打开原始 diff 文本友好：
//! 增删行有背景色、行号分新旧两栏、文件段有标题和统计。
//!
//! 不做编辑、光标、选区——这些在 diff 里没有意义，交给纯文本更直接。

use con_core::git::{DiffLine, DiffLineKind, ParsedDiff};
use gpui::{
    AnyElement, Context, EventEmitter, FontWeight, Hsla, IntoElement, ParentElement, Render,
    SharedString, Styled, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme, Sizable as _,
    button::{Button, ButtonVariants as _},
};

use crate::ui_scale::ui_px;

/// 用户关闭 diff 视图。
pub struct CloseDiff;

/// 行高，与等宽字体的大小匹配。
const LINE_HEIGHT: f32 = 18.0;
/// 行号列宽度（新旧两栏，中间一条分隔）。
const GUTTER_WIDTH: f32 = 76.0;

pub struct DiffView {
    title: String,
    diff: ParsedDiff,
    /// 打平后的所有行，虚拟列表按这个渲染。
    rows: Vec<FlatRow>,
}

/// 打平后的一行：要么是文件标题，要么是 diff 行。
#[derive(Clone)]
enum FlatRow {
    FileHeader {
        path: String,
        added: usize,
        removed: usize,
    },
    Line {
        line: DiffLine,
    },
}

impl EventEmitter<CloseDiff> for DiffView {}

impl DiffView {
    pub fn new(document: crate::editor_view::DiffDocument) -> Self {
        let title = document.title;
        let diff = document.diff;
        let mut rows = Vec::new();
        for file in &diff.files {
            rows.push(FlatRow::FileHeader {
                path: file.path.clone(),
                added: file.added,
                removed: file.removed,
            });
            for line in &file.lines {
                // 文件头在 diff 里已经以汇总行呈现，逐行重复太啰嗦。
                if line.kind == DiffLineKind::FileHeader {
                    continue;
                }
                rows.push(FlatRow::Line { line: line.clone() });
            }
        }
        Self { title, diff, rows }
    }

    fn added_text(&self) -> String {
        format!("+{}", self.diff.added)
    }

    fn removed_text(&self) -> String {
        format!("−{}", self.diff.removed)
    }
}

/// 增删行的背景色。暗色主题下用低透明度，避免糊住文字。
fn row_background(kind: DiffLineKind, theme: &gpui_component::Theme) -> Hsla {
    let is_dark = theme.is_dark();
    match kind {
        DiffLineKind::Added => theme.success.opacity(if is_dark { 0.16 } else { 0.13 }),
        DiffLineKind::Removed => theme.danger.opacity(if is_dark { 0.16 } else { 0.11 }),
        DiffLineKind::HunkHeader => theme.foreground.opacity(if is_dark { 0.05 } else { 0.035 }),
        _ => theme.transparent,
    }
}

/// 行内容的颜色：删行整体压暗，其余用正常前景色。
fn text_color(kind: DiffLineKind, theme: &gpui_component::Theme) -> Hsla {
    match kind {
        DiffLineKind::Added => theme.success,
        DiffLineKind::Removed => theme.danger,
        DiffLineKind::HunkHeader => theme.muted_foreground,
        _ => theme.foreground.opacity(0.92),
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

        // 直接铺开所有行，不用 uniform_list。
        //
        // 虚拟列表要求父级有稳定的布局上下文，放进实体化的子视图里反复
        // 算不出可见范围（表现为整片空白）。diff 通常只有几百到几千行，
        // 普通 flex 列的渲染开销可以接受。
        let mono_font = theme.mono_font_family.clone();
        let mut rows = div()
            .id("diff-rows")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .overflow_y_scroll();

        for row in &self.rows {
            rows = rows.child(match row {
                FlatRow::FileHeader {
                    path,
                    added,
                    removed,
                    ..
                } => Self::render_file_header(&theme, path.clone(), *added, *removed),
                FlatRow::Line { line, .. } => Self::render_line(&theme, &mono_font, line.clone()),
            });
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
            .child(
                Button::new("diff-close")
                    .label("关闭")
                    .ghost()
                    .small()
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(CloseDiff))),
            )
            .into_any_element()
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

    fn render_line(
        theme: &gpui_component::Theme,
        mono_font: &SharedString,
        line: DiffLine,
    ) -> AnyElement {
        let background = row_background(line.kind, theme);
        let ink = text_color(line.kind, theme);

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

        div()
            .h(px(LINE_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .bg(background)
            .child(
                // 行号栏：旧号 | 新号，右对齐。增行只显示新号，删行只显示旧号，
                // 这比统一显示「该行在哪个文件里」更贴近 git 的语义。
                div()
                    .w(px(GUTTER_WIDTH))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .text_size(ui_px(theme, 9.5))
                    .font_family(mono_font.clone())
                    .text_color(theme.muted_foreground.opacity(0.55))
                    .child(div().w(px(32.0)).pr(px(6.0)).text_right().child(old_no))
                    .child(div().w(px(32.0)).pr(px(6.0)).text_right().child(new_no)),
            )
            .child(
                // 符号列固定宽度，正文左边缘才能对齐。
                div()
                    .w(px(12.0))
                    .flex_shrink_0()
                    .text_size(ui_px(theme, 11.0))
                    .font_family(mono_font.clone())
                    .text_color(ink.opacity(0.7))
                    .child(sign),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .text_size(ui_px(theme, 11.0))
                    .font_family(mono_font.clone())
                    .text_color(ink)
                    .child(line.text),
            )
            .into_any_element()
    }
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

    /// 打平后的行必须包含所有非文件头的 diff 行，否则列表里一片空白。
    #[test]
    fn flat_rows_contain_diff_lines() {
        let raw = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,2 +1,2 @@
 keep
-old
+new
";
        let view = make(raw);

        // 1 个文件头 + 1 个 hunk 头 + 1 上下文 + 1 删除 + 1 新增 = 5
        assert_eq!(view.rows.len(), 5, "row 数不对");
        assert!(matches!(view.rows[0], FlatRow::FileHeader { .. }));
        assert!(view.rows.iter().any(|row| matches!(
            row,
            FlatRow::Line { line, .. } if line.kind == DiffLineKind::Added
        )));
    }

    #[test]
    fn empty_diff_has_no_rows() {
        let view = make("");
        assert!(view.rows.is_empty());
        assert!(view.diff.files.is_empty());
    }
}
