//! Git 面板：变更列表、提交历史、diff 预览、commit。
//!
//! 只读浏览为主，唯一的写操作是用户显式触发的 commit。删除/推送/切分支
//! 等仍然交给终端里的 git 命令——在这个面板里复刻它们只会增加出错面积。

use std::path::PathBuf;

use con_core::git::{self, GitChange, GitCommit, GitStatus};
use gpui::{
    AnyElement, ClipboardItem, Context, Entity, EventEmitter, FontWeight, Hsla, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::*, px, svg,
};
use gpui_component::input::InputState;
use gpui_component::{
    ActiveTheme, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::Input,
    menu::{ContextMenuExt as _, PopupMenuItem},
};

use crate::file_icons::icon_for_path;
use crate::file_tree_view::render_file_icon;
use crate::git_graph_element::GitGraphElement;
use crate::ui_scale::ui_px;

/// 面板当前显示哪个分页。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GitTab {
    Changes,
    History,
}

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
    tab: GitTab,
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
            tab: GitTab::Changes,
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

    fn set_tab(&mut self, tab: GitTab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            self.selected_commit = None;
            self.commit_files.clear();
            self.refresh(cx);
        }
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

        let want_history = self.tab == GitTab::History;
        let commit = self.selected_commit.clone();

        cx.spawn(async move |this, cx| {
            let read =
                move || -> Result<(Vec<GitChange>, Vec<GitCommit>, Vec<GitChange>), String> {
                    let changes = git::status(&root)?;
                    let commits = if want_history {
                        git::log(&root, HISTORY_LIMIT)?
                    } else {
                        Vec::new()
                    };
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
                        if want_history {
                            this.commits = commits;
                        }
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();

        if self.root.is_none() {
            return empty_state("不是 git 仓库", &theme);
        }

        div()
            .id("git-view")
            .size_full()
            .flex()
            .flex_col()
            .child(self.render_tabs(&theme, cx))
            .child(self.render_body(&theme, cx))
            .into_any_element()
    }
}

impl GitView {
    fn render_tabs(
        &self,
        theme: &gpui_component::Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tab_button = |id: &'static str, label: &'static str, tab: GitTab| {
            let is_active = self.tab == tab;
            div()
                .id(id)
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .h(px(26.0))
                .rounded(px(6.0))
                .cursor_pointer()
                .text_size(ui_px(theme, 11.5))
                .font_family(theme.font_family.clone())
                .text_color(if is_active {
                    theme.foreground
                } else {
                    theme.muted_foreground
                })
                .bg(if is_active {
                    theme.foreground.opacity(0.075)
                } else {
                    theme.transparent
                })
                .hover(|style| style.bg(theme.foreground.opacity(0.05)))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| this.set_tab(tab, cx)))
        };

        div()
            .flex()
            .gap(px(3.0))
            .px(px(8.0))
            .py(px(6.0))
            .flex_shrink_0()
            .child(tab_button("git-tab-changes", "变更", GitTab::Changes))
            .child(tab_button("git-tab-history", "历史", GitTab::History))
    }

    fn render_body(&mut self, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> AnyElement {
        if let Some(error) = &self.error {
            return empty_state_owned(error.clone(), theme);
        }
        match self.tab {
            GitTab::Changes => self.render_changes(theme, cx),
            GitTab::History => match self.selected_commit.clone() {
                Some(commit) => self.render_commit_detail(&commit, theme, cx),
                None => self.render_history(theme, cx),
            },
        }
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
