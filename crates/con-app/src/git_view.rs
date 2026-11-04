//! Git 面板：变更列表、提交历史、diff 预览、commit。
//!
//! 只读浏览为主，唯一的写操作是用户显式触发的 commit。删除/推送/切分支
//! 等仍然交给终端里的 git 命令——在这个面板里复刻它们只会增加出错面积。

use std::path::PathBuf;

use con_core::git::{self, GitChange, GitCommit, GitStatus};
use gpui::{
    AnyElement, Bounds, ClipboardItem, Context, Entity, EventEmitter, FontWeight, Hsla,
    IntoElement, MouseButton, ParentElement, Pixels, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, div, prelude::*, px, svg,
};
use gpui_component::input::InputState;
use gpui_component::{
    ActiveTheme, ElementExt as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::Input,
    menu::{ContextMenuExt as _, PopupMenuItem},
};

use crate::file_icons::icon_for_path;
use crate::file_tree_view::render_file_icon;
use crate::git_graph_element::GitGraphElement;
use crate::ui_scale::ui_px;

/// 用户在 Git 面板里要打开某个 diff。
pub struct OpenDiff {
    pub title: String,
    pub text: String,
}

/// 用户在变更列表里请求放弃某处改动。
///
/// 用事件而不是直接执行：放弃不可撤销，需要先弹确认。面板自己持有待确认
/// 状态，确认后才真正调用 git。
pub struct DiscardChangeRequested {
    pub path: PathBuf,
    pub untracked: bool,
}

pub struct GitView {
    root: Option<PathBuf>,
    changes: Vec<GitChange>,
    commits: Vec<GitCommit>,
    /// 点进某个提交后，显示它改动了哪些文件。
    selected_commit: Option<GitCommit>,
    commit_files: Vec<GitChange>,
    message: Entity<InputState>,
    commit_error: Option<String>,
    /// 最近一次读取失败的原因，面板里直接显示。
    error: Option<String>,
    /// 每次刷新递增，用来丢弃过期的后台结果。
    generation: u64,
    /// 提交历史的泳道布局，随 commits 一起刷新。
    graph: con_core::git_graph::GitGraphLayout,
    /// 等待用户确认的放弃改动请求。
    pending_discard: Option<DiscardChangeRequested>,
    /// 两个分组各自的展开状态。
    staged_open: bool,
    unstaged_open: bool,
    /// 上方变更区占整体的比例（0.15~0.85），由中间的分割线拖动。
    split_ratio: f32,
    /// 拖拽分割线时记录「上一帧的指针 y」，每次按增量推进比例。
    /// 用增量而不是起点基准：分割线本身会跟着比例移动，起点基准会让
    /// 指针与线的相对位置不断变化，产生抖动。
    split_drag: Option<f32>,
    /// 上一次绘制时面板的总高度，用于把拖动像素换算成比例。
    split_area_height: f32,
}

const ROW_HEIGHT: f32 = 24.0;
/// 历史列表行高，右侧行与左侧泳道图必须一致。
const HISTORY_ROW_HEIGHT: f32 = 26.0;
/// 单条车道的宽度。
const HISTORY_LANE_WIDTH: f32 = 14.0;
const HISTORY_LIMIT: usize = 200;

impl EventEmitter<OpenDiff> for GitView {}
impl EventEmitter<DiscardChangeRequested> for GitView {}

impl GitView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let message = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_placeholder("提交信息", window, cx);
            state
        });
        Self {
            root: None,
            changes: Vec::new(),
            commits: Vec::new(),
            selected_commit: None,
            commit_files: Vec::new(),
            message,
            commit_error: None,
            error: None,
            generation: 0,
            graph: Default::default(),
            pending_discard: None,
            staged_open: true,
            unstaged_open: true,
            split_ratio: 0.5,
            split_drag: None,
            split_area_height: 0.0,
        }
    }

    /// 切换工作区根。不是 git 仓库时清空并显示空状态。
    pub fn set_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        let resolved = git::repository_root(&root);
        if self.root == resolved {
            self.refresh(cx);
            return;
        }
        self.root = resolved;
        self.selected_commit = None;
        self.commit_files.clear();
        self.refresh(cx);
    }

    /// 重新读取当前分页需要的数据。
    ///
    /// git 命令会阻塞，放到后台线程；`generation` 保证只有最新一次的结果
    /// 会落到界面上，避免快速切换时旧结果覆盖新结果。
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;

        let Some(root) = self.root.clone() else {
            self.changes.clear();
            self.commits.clear();
            self.error = None;
            cx.notify();
            return;
        };

        let commit = self.selected_commit.clone();

        cx.spawn(async move |this, cx| {
            let read =
                move || -> Result<(Vec<GitChange>, Vec<GitCommit>, Vec<GitChange>), String> {
                    // 变更和历史上下同屏，两部分一起读。
                    let changes = git::status(&root)?;
                    let commits = git::log(&root, HISTORY_LIMIT)?;
                    let files = match &commit {
                        Some(commit) => git::commit_files(&root, &commit.hash)?,
                        None => Vec::new(),
                    };
                    Ok((changes, commits, files))
                };
            let result = cx.background_executor().spawn(async move { read() }).await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match result {
                    Ok((changes, commits, files)) => {
                        this.changes = changes;
                        this.commits = commits;
                        this.commit_files = files;
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open_commit(&mut self, commit: GitCommit, cx: &mut Context<Self>) {
        self.selected_commit = Some(commit);
        self.refresh(cx);
    }

    fn back_to_history(&mut self, cx: &mut Context<Self>) {
        self.selected_commit = None;
        self.commit_files.clear();
        cx.notify();
    }

    fn open_working_diff(&self, change: &GitChange, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let path = change.path.clone();
        let title = format!("{} (工作区)", path.display());
        cx.spawn(async move |this, cx| {
            let diff = cx
                .background_executor()
                .spawn(async move { git::working_diff(&root, &path) })
                .await;
            let _ = this.update(cx, |_, cx| {
                if let Ok(diff) = diff {
                    cx.emit(OpenDiff {
                        title,
                        text: diff.text,
                    });
                }
            });
        })
        .detach();
    }

    fn open_commit_diff(&self, path: Option<PathBuf>, cx: &mut Context<Self>) {
        let (Some(root), Some(commit)) = (self.root.clone(), self.selected_commit.clone()) else {
            return;
        };
        let title = match &path {
            Some(path) => format!("{} {}", commit.short_hash, path.display()),
            None => format!("提交 {}", commit.short_hash),
        };
        cx.spawn(async move |this, cx| {
            let hash = commit.hash.clone();
            let diff = cx
                .background_executor()
                .spawn(async move { git::commit_diff(&root, &hash, path.as_deref()) })
                .await;
            let _ = this.update(cx, |_, cx| {
                if let Ok(diff) = diff {
                    cx.emit(OpenDiff {
                        title,
                        text: diff.text,
                    });
                }
            });
        })
        .detach();
    }

    /// 提交。暂存区为空时自动提交全部已跟踪文件的改动。
    ///
    /// 对应 VSCode 的 smart commit：没有暂存内容时不必先手动 add。
    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let message = self.message.read(cx).value().trim().to_string();
        if message.is_empty() {
            self.commit_error = Some("提交信息不能为空".to_string());
            cx.notify();
            return;
        }
        // 暂存区为空就走 `git commit -a`，否则只提交已暂存的部分。
        let stage_all = !self.changes.iter().any(GitChange::is_staged);

        self.commit_error = None;
        // 提交成功后清空输入框需要 Window，而 Window 不能跨 await 持有，
        // 先在这里记下句柄再交给异步任务。
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { git::commit(&root, &message, stage_all) })
                .await;
            let succeeded = result.is_ok();
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(_) => {
                        this.commit_error = None;
                        this.refresh(cx);
                    }
                    Err(error) => this.commit_error = Some(error),
                }
                cx.notify();
            });
            if succeeded {
                // 清空输入框需要 Window，只能回到窗口上下文里做。
                let _ = window_handle.update(cx, |_, window, cx| {
                    let _ = this.update(cx, |this, cx| {
                        this.message
                            .update(cx, |input, cx| input.set_value("", window, cx));
                    });
                });
            }
        })
        .detach();
    }
}

impl Render for GitView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();

        if self.root.is_none() {
            return empty_state("不是 git 仓库", &theme);
        }

        if let Some(error) = &self.error {
            return empty_state_owned(error.clone(), &theme);
        }

        // 上下分栏：变更在上、历史在下，中间一条可拖动的分割线。
        // 用 flex_basis 按比例分配，分割线本身不占高度。
        let ratio = self.split_ratio.clamp(0.15, 0.85);
        let top = self.render_changes(&theme, cx);
        let bottom = match self.selected_commit.clone() {
            Some(commit) => self.render_commit_detail(&commit, &theme, cx),
            None => self.render_history(&theme, cx),
        };

        if self.root.is_none() {
            return empty_state("不是 git 仓库", &theme);
        }

        if let Some(error) = &self.error {
            return empty_state_owned(error.clone(), &theme);
        }

        // 上下分栏：变更在上、历史在下，中间一条可拖动的分割线。
        //
        // 两区高度按像素显式指定（比例 × 面板高度），而不是用 flex_grow。
        // flex 比例分配会被子内容的 flex_basis 争抢，交界位置和按比例定位的
        // 分割线对不上，拖到偏上偏下时线看起来就「篡位」了；显式像素高度下
        // 分割线在哪，交界就在哪，永远一致。
        let ratio = self.split_ratio.clamp(0.15, 0.85);
        let area_height = self.split_area_height.max(200.0);
        let top_height = px((area_height * ratio).max(80.0));
        let bottom_height = px((area_height * (1.0 - ratio)).max(80.0));
        let top = self.render_changes(&theme, cx);
        let bottom = match self.selected_commit.clone() {
            Some(commit) => self.render_commit_detail(&commit, &theme, cx),
            None => self.render_history(&theme, cx),
        };

        div()
            .id("git-view")
            .size_full()
            .relative()
            .flex()
            .flex_col()
            // 拖动分割线时的 move / up。挂在这一层而不是分割线上：
            // 根层的 hitbox 覆盖整个面板且不随 ratio 移动，指针拖多远
            // 事件都不会断。
            .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                let Some(last_y) = this.split_drag else {
                    return;
                };
                // 左键已松开却还残留拖拽状态（mouse_up 偶发被吞时），
                // 立即停止，否则面板会一直跟随指针移动。
                if event.pressed_button != Some(MouseButton::Left) {
                    this.split_drag = None;
                    cx.notify();
                    return;
                }
                if this.split_area_height <= 0.0 {
                    return;
                }
                let current_y = event.position.y.as_f32();
                // 按增量推进：每次只加「这一帧指针移动的距离」，
                // 线和指针保持同速，不会互相追赶。
                let delta = (current_y - last_y) / this.split_area_height;
                this.split_drag = Some(current_y);
                let next = (this.split_ratio + delta).clamp(0.15, 0.85);
                if (next - this.split_ratio).abs() > f32::EPSILON {
                    this.split_ratio = next;
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseUpEvent, _, cx| {
                    if this.split_drag.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            // 记下可用高度，拖动分割线时把像素位移换算成比例。
            .on_prepaint({
                let entity = cx.entity().downgrade();
                move |bounds: Bounds<Pixels>, _, cx: &mut gpui::App| {
                    let height = bounds.size.height.as_f32();
                    let _ = entity.update(cx, |this, _| {
                        // 只记录不 notify：绘制阶段回调里重绘会造成循环。
                        this.split_area_height = height;
                    });
                }
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .flex_shrink_0()
                    .h(top_height)
                    .child(top),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .flex_shrink_0()
                    .h(bottom_height)
                    .child(bottom),
            )
            // 分割线绝对定位盖在两区交界处：flex 子项会被上下滚动区抢占
            // 命中测试，收不到鼠标事件（sidebar 的拖拽条也是这么做的）。
            .child(self.render_splitter(&theme, top_height, cx))
            .into_any_element()
    }
}

impl GitView {
    /// 可拖动的水平分割线，绝对定位在上下两区交界处。
    ///
    /// 拖动距离换算成比例时用上一次绘制记下的可用高度；首次绘制前
    /// `split_area_height` 为 0，拖动不会生效，但那时用户还不会去拖。
    fn render_splitter(
        &self,
        theme: &gpui_component::Theme,
        top_height: Pixels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id("git-splitter")
            .absolute()
            .left_0()
            .right_0()
            // 和上区高度用同一个值：线的中线正好压在两区交界上。
            .top(top_height)
            .mt(px(-12.0))
            .h(px(24.0))
            .cursor_row_resize()
            .group("git-splitter")
            .flex()
            .items_center()
            .occlude()
            // 命中区本身完全透明，hover 高亮只画在中间那条 1px 细线上，
            // 视觉宽度和其它分屏线一致；24px 的透明命中区只是为了让拖动
            // 时事件不断流。
            .child(
                div()
                    .w_full()
                    .h(px(1.0))
                    .bg(theme.border.opacity(0.6))
                    .group_hover("git-splitter", |style| style.bg(theme.primary.opacity(0.7))),
            )
            // 只处理按下。move / up 挂在 git-view 根层：这个带子会跟着
            // ratio 移动，拖动时 hitbox 也在动，指针很容易跑出去，事件
            // 就断了；根层不动，指针始终在它内部。
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                    this.split_drag = Some(event.position.y.as_f32());
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
    }

    fn render_changes(
        &mut self,
        theme: &gpui_component::Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let staged: Vec<GitChange> = self
            .changes
            .iter()
            .filter(|change| change.is_staged())
            .cloned()
            .collect();
        let unstaged: Vec<GitChange> = self
            .changes
            .iter()
            .filter(|change| change.is_unstaged())
            .cloned()
            .collect();
        let total = staged.len() + unstaged.len();

        let mut list = div()
            .id("git-changes")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .py(px(4.0))
            .overflow_y_scroll();

        if total == 0 {
            list = list.child(empty_state("工作区干净", theme));
        }

        // 已暂存 / 未暂存各自成组，标题可折叠，并显示条目数。
        if !staged.is_empty() {
            list = list.child(self.section_header("已暂存", staged.len(), true, theme, cx));
            if self.staged_open {
                for change in staged {
                    list = list.child(self.change_row(change, true, theme, cx));
                }
            }
        }
        if !unstaged.is_empty() {
            list = list.child(self.section_header("未暂存", unstaged.len(), false, theme, cx));
            if self.unstaged_open {
                for change in unstaged {
                    list = list.child(self.change_row(change, false, theme, cx));
                }
            }
        }

        let mut body = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            // 提交框放顶部：写完信息立刻能提交，不必把列表滚到底。
            .child(self.render_commit_box(theme, cx));
        if let Some(confirm) = self.render_discard_confirm(theme, cx) {
            body = body.child(confirm);
        }
        body.child(list).into_any_element()
    }

    /// 分组标题：折叠箭头 + 名称 + 条目数徽章。
    fn section_header(
        &self,
        title: &'static str,
        count: usize,
        staged: bool,
        theme: &gpui_component::Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = if staged {
            self.staged_open
        } else {
            self.unstaged_open
        };
        div()
            .id(if staged {
                "git-section-staged"
            } else {
                "git-section-unstaged"
            })
            .flex()
            .items_center()
            .gap(px(4.0))
            .h(px(24.0))
            .px(px(8.0))
            .cursor_pointer()
            .hover(|style| style.bg(theme.foreground.opacity(0.04)))
            .child(
                svg()
                    .path(if open {
                        "phosphor/caret-down.svg"
                    } else {
                        "phosphor/caret-right.svg"
                    })
                    .size(px(9.0))
                    .flex_shrink_0()
                    .text_color(theme.muted_foreground),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(ui_px(theme, 10.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .font_family(theme.font_family.clone())
                    .text_color(theme.muted_foreground.opacity(0.85))
                    .child(title),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .min_w(px(16.0))
                    .px(px(4.0))
                    .rounded(px(7.0))
                    .bg(theme.foreground.opacity(0.08))
                    .text_size(ui_px(theme, 9.5))
                    .text_color(theme.muted_foreground)
                    .child(count.to_string()),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if staged {
                    this.staged_open = !this.staged_open;
                } else {
                    this.unstaged_open = !this.unstaged_open;
                }
                cx.notify();
            }))
            .into_any_element()
    }

    /// 放弃改动的确认条。
    fn render_discard_confirm(
        &self,
        theme: &gpui_component::Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let pending = self.pending_discard.as_ref()?;
        let verb = if pending.untracked {
            "删除这个未跟踪文件"
        } else {
            "放弃这个文件的改动"
        };
        let name = pending
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| pending.path.display().to_string());

        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .p(px(8.0))
                .flex_shrink_0()
                .bg(theme.danger.opacity(0.08))
                .child(
                    div()
                        .text_size(ui_px(theme, 10.5))
                        .font_family(theme.font_family.clone())
                        .text_color(theme.foreground)
                        .child(format!("{verb}？{name} 的改动将不可恢复。")),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(6.0))
                        .child(
                            Button::new("discard-cancel")
                                .label("取消")
                                .small()
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.pending_discard = None;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("discard-confirm")
                                .label("确认放弃")
                                .small()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.discard_pending(cx);
                                })),
                        ),
                )
                .into_any_element(),
        )
    }

    /// 执行待确认的放弃请求。
    fn discard_pending(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.pending_discard.take() else {
            return;
        };
        let Some(root) = self.root.clone() else {
            return;
        };
        let path = pending.path.clone();
        let untracked = pending.untracked;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { git::discard_change(&root, &path, untracked) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.error = Some(format!("放弃改动失败：{error}"));
                }
                this.refresh(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn change_row(
        &mut self,
        change: GitChange,
        staged: bool,
        theme: &gpui_component::Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let status = if staged {
            change.staged
        } else {
            change.unstaged
        };
        let (letter, color) = status.map_or((' ', theme.muted_foreground), |status| {
            (status.letter(), status_color(status, theme))
        });
        let name = change
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| change.path.display().to_string());
        let dir = change
            .path
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned())
            .unwrap_or_default();

        let change_for_click = change.clone();
        div()
            .id(SharedString::from(format!(
                "git-change-{}-{}",
                if staged { "s" } else { "u" },
                change.path.display()
            )))
            .h(px(ROW_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .gap(px(6.0))
            .pl(px(8.0))
            .pr(px(8.0))
            .cursor_pointer()
            .hover(|style| style.bg(theme.foreground.opacity(0.05)))
            // 文件图标在最左，和文件树里的一致。
            .child(render_file_icon(
                icon_for_path(&change.path, false, false),
                ui_px(theme, 12.0),
                theme.muted_foreground,
            ))
            .child(
                div()
                    .flex_shrink_0()
                    .max_w(px(140.0))
                    .truncate()
                    .text_size(ui_px(theme, 11.5))
                    .font_family(theme.font_family.clone())
                    .text_color(theme.foreground)
                    .child(name),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .text_size(ui_px(theme, 10.5))
                    .font_family(theme.font_family.clone())
                    .text_color(theme.muted_foreground.opacity(0.7))
                    .child(dir),
            )
            // 状态字母移到最右，和 VSCode 一样，扫一眼就能看出改动类型。
            .child(
                div()
                    .w(px(10.0))
                    .flex_shrink_0()
                    .text_size(ui_px(theme, 10.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color)
                    .child(letter.to_string()),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_working_diff(&change_for_click, cx);
            }))
            .context_menu({
                // 未跟踪文件的放弃方式是「删除」，已跟踪的是「还原」。
                let untracked = matches!(status, Some(GitStatus::Untracked));
                let path = change.path.clone();
                let label = if untracked {
                    "删除未跟踪文件"
                } else {
                    "放弃更改"
                };
                // WeakEntity 先取出再进闭包：闭包要求 'static，不能借用 cx。
                let entity = cx.entity().downgrade();
                move |menu, _window, _cx| {
                    let path = path.clone();
                    let entity = entity.clone();
                    menu.item(PopupMenuItem::new(label).on_click(move |_, _, app| {
                        let path = path.clone();
                        let _ = entity.update(app, |this, cx| {
                            this.pending_discard = Some(DiscardChangeRequested {
                                path: path.clone(),
                                untracked,
                            });
                            cx.notify();
                        });
                    }))
                }
            })
            .into_any_element()
    }

    fn render_commit_box(
        &mut self,
        theme: &gpui_component::Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // 只要有任何改动就能提交（未暂存内容会走 commit -a）。
        let can_commit = !self.changes.is_empty();
        let mut box_ = div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .p(px(8.0))
            .flex_shrink_0()
            // ⌘Enter 直接提交：输入框有焦点时由它消费回车，这里接住带
            // 修饰键的组合。
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "enter" && event.keystroke.modifiers.platform {
                    this.commit(window, cx);
                }
            }))
            .child(Input::new(&self.message).small());

        if let Some(error) = &self.commit_error {
            box_ = box_.child(
                div()
                    .text_size(ui_px(theme, 10.5))
                    .text_color(theme.danger)
                    .child(error.clone()),
            );
        }

        // 主按钮占满整行，和 VSCode 一致：只有一个动作，不需要按钮组。
        let mut button = Button::new("git-commit")
            .label("提交")
            .small()
            .primary()
            .w_full();
        if can_commit {
            button = button.on_click(cx.listener(|this, _, window, cx| this.commit(window, cx)));
        } else {
            button = button.opacity(0.5);
        }
        box_.child(button).into_any_element()
    }

    /// 历史列表：左侧泳道图，右侧提交摘要与分支徽章。
    ///
    /// 泳道图是独立的自绘元素，行高必须和右侧每一行严格一致，否则圆点会和
    /// 文字错位。两边共用 [`HISTORY_ROW_HEIGHT`]。
    fn render_history(
        &mut self,
        theme: &gpui_component::Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.commits.is_empty() {
            return empty_state("没有提交记录", theme);
        }

        // 布局只在提交集合变化时重算，渲染帧里直接取。
        if self.graph.rows.len() != self.commits.len() {
            self.graph = con_core::git_graph::layout(&self.commits);
        }

        let graph = GitGraphElement::new(
            self.graph.clone(),
            HISTORY_ROW_HEIGHT,
            HISTORY_LANE_WIDTH,
            theme.clone(),
        );

        let mut rows = div()
            .id("git-history-rows")
            .flex()
            .flex_col()
            .flex_shrink_0();

        for commit in self.commits.clone() {
            let commit_for_click = commit.clone();
            let mut title_row = div()
                .flex()
                .items_center()
                .gap(px(5.0))
                .min_w(px(0.0))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .truncate()
                        .text_size(ui_px(theme, 11.5))
                        .font_family(theme.font_family.clone())
                        .text_color(theme.foreground)
                        .child(commit.summary.clone()),
                );

            // 分支/标签徽章，和 VSCode 一样跟在摘要后面。
            for label in &commit.refs {
                let (bg, ink) = ref_badge_colors(label, theme);
                title_row = title_row.child(
                    div()
                        .flex_shrink_0()
                        .px(px(5.0))
                        .py(px(0.5))
                        .rounded(px(4.0))
                        .bg(bg)
                        .text_size(ui_px(theme, 9.5))
                        .font_weight(FontWeight::MEDIUM)
                        .font_family(theme.font_family.clone())
                        .text_color(ink)
                        .child(label.clone()),
                );
            }

            rows = rows.child(
                div()
                    .id(SharedString::from(format!("git-commit-{}", commit.hash)))
                    .h(px(HISTORY_ROW_HEIGHT))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .pr(px(8.0))
                    .cursor_pointer()
                    .hover(|style| style.bg(theme.foreground.opacity(0.05)))
                    .child(title_row)
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .text_size(ui_px(theme, 10.0))
                            .font_family(theme.font_family.clone())
                            .text_color(theme.muted_foreground.opacity(0.65))
                            .child(commit.short_hash.clone())
                            .child(div().child("·"))
                            .child(div().child(commit.author.clone()))
                            .child(div().child("·"))
                            .child(div().child(relative_time(commit.timestamp))),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_commit(commit_for_click.clone(), cx);
                    }))
                    // 右键菜单：复制哈希。菜单闭包会被多次调用，所以哈希要
                    // 各自 clone 一份。
                    .context_menu({
                        let full_hash = commit.hash.clone();
                        let short_hash = commit.short_hash.clone();
                        move |menu, _window, _cx| {
                            let full = full_hash.clone();
                            let short = short_hash.clone();
                            menu.item(PopupMenuItem::new("复制提交哈希").on_click(
                                move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(full.clone()));
                                },
                            ))
                            .item(
                                PopupMenuItem::new("复制短哈希").on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(short.clone()));
                                }),
                            )
                        }
                    }),
            );
        }

        div()
            .id("git-history")
            .flex()
            // 顶部对齐：图形元素和文字列高度各自精确，垂直居中会让圆点
            // 整体错开半个行高。
            .items_start()
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .child(graph)
            .child(rows)
            .into_any_element()
    }

    fn render_commit_detail(
        &mut self,
        commit: &GitCommit,
        theme: &gpui_component::Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let list = div()
            .id("git-commit-detail")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .px(px(8.0))
                    .py(px(6.0))
                    .flex_shrink_0()
                    .child(
                        Button::new("git-back")
                            .icon(Icon::default().path("phosphor/caret-left.svg"))
                            .ghost()
                            .small()
                            .tooltip("返回历史")
                            .on_click(cx.listener(|this, _, _, cx| this.back_to_history(cx))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .truncate()
                            .text_size(ui_px(theme, 11.5))
                            .font_family(theme.font_family.clone())
                            .text_color(theme.foreground)
                            .child(commit.summary.clone()),
                    ),
            )
            .child(
                div()
                    .id("git-commit-files")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_y_scroll()
                    .child(section_label("改动的文件", theme))
                    .children(self.commit_files.clone().into_iter().map(|change| {
                        let status = change.staged;
                        let (letter, color) = status
                            .map_or((' ', theme.muted_foreground), |status| {
                                (status.letter(), status_color(status, theme))
                            });
                        let path = change.path.clone();
                        let name = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.display().to_string());
                        let path_for_click = path.clone();
                        div()
                            .id(SharedString::from(format!(
                                "git-commit-file-{}",
                                path.display()
                            )))
                            .h(px(ROW_HEIGHT))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .pl(px(12.0))
                            .pr(px(8.0))
                            .cursor_pointer()
                            .hover(|style| style.bg(theme.foreground.opacity(0.05)))
                            .child(
                                div()
                                    .w(px(10.0))
                                    .flex_shrink_0()
                                    .text_size(ui_px(theme, 10.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(color)
                                    .child(letter.to_string()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .truncate()
                                    .text_size(ui_px(theme, 11.5))
                                    .font_family(theme.font_family.clone())
                                    .text_color(theme.foreground)
                                    .child(name),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_commit_diff(Some(path_for_click.clone()), cx);
                            }))
                    })),
            );
        list.into_any_element()
    }
}

fn section_label(text: &'static str, theme: &gpui_component::Theme) -> AnyElement {
    div()
        .px(px(12.0))
        .pt(px(8.0))
        .pb(px(3.0))
        .text_size(ui_px(theme, 10.0))
        .font_weight(FontWeight::SEMIBOLD)
        .font_family(theme.font_family.clone())
        .text_color(theme.muted_foreground.opacity(0.75))
        .child(text)
        .into_any_element()
}

/// 分支徽章的配色：当前分支（带 HEAD）用主色，远程分支用中性色，标签用警告色。
fn ref_badge_colors(label: &str, theme: &gpui_component::Theme) -> (Hsla, Hsla) {
    if label.contains("(HEAD)") {
        (theme.primary.opacity(0.18), theme.primary)
    } else if label.starts_with("origin/") || label == "origin" {
        (theme.foreground.opacity(0.10), theme.muted_foreground)
    } else if label.starts_with("tag:") || label.contains('/') {
        (theme.warning.opacity(0.16), theme.warning)
    } else {
        (theme.success.opacity(0.16), theme.success)
    }
}

fn status_color(status: GitStatus, theme: &gpui_component::Theme) -> Hsla {
    match status {
        GitStatus::Added | GitStatus::Untracked => theme.success,
        GitStatus::Deleted | GitStatus::Unmerged => theme.danger,
        GitStatus::Modified | GitStatus::TypeChanged => theme.warning,
        GitStatus::Renamed | GitStatus::Copied => theme.primary,
    }
}

fn empty_state(text: &'static str, theme: &gpui_component::Theme) -> AnyElement {
    empty_state_owned(text.to_string(), theme)
}

fn empty_state_owned(text: String, theme: &gpui_component::Theme) -> AnyElement {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .p(px(16.0))
        .text_size(ui_px(theme, 11.5))
        .font_family(theme.font_family.clone())
        .text_color(theme.muted_foreground)
        .child(text)
        .into_any_element()
}

/// 把 Unix 时间戳转成「3 小时前」这类相对时间。
fn relative_time(timestamp: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let delta = (now - timestamp).max(0);
    match delta {
        0..=59 => "刚刚".to_string(),
        60..=3599 => format!("{} 分钟前", delta / 60),
        3600..=86399 => format!("{} 小时前", delta / 3600),
        86400..=2_591_999 => format!("{} 天前", delta / 86400),
        2_592_000..=31_535_999 => format!("{} 个月前", delta / 2_592_000),
        _ => format!("{} 年前", delta / 31_536_000),
    }
}

#[cfg(test)]
mod tests {
    use super::relative_time;

    #[test]
    fn relative_time_covers_each_bucket() {
        // 固定「现在」，用相对偏移构造时间戳。
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        assert_eq!(relative_time(now), "刚刚");
        assert_eq!(relative_time(now - 120), "2 分钟前");
        assert_eq!(relative_time(now - 7200), "2 小时前");
        assert_eq!(relative_time(now - 172_800), "2 天前");
        assert_eq!(relative_time(now - 2_592_000 * 2), "2 个月前");
        assert_eq!(relative_time(now - 31_536_000 * 2), "2 年前");
    }

    #[test]
    fn future_timestamps_do_not_go_negative() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        // 时钟偏移导致提交时间在「未来」时，不应显示成负数。
        assert_eq!(relative_time(now + 3600), "刚刚");
    }
}
