//! 远程服务器连接浮层。
//!
//! 连接列表由 Cube 自己维护，存在 `connections.json`。选中一项会开一个新标签
//! 执行 `ssh`。认证走密钥或密码；密码按用户选择明文存盘，连接时经
//! `SSH_ASKPASS` 传给 ssh，不进入命令行。

use cube_core::connections::{ConnectionBook, SshConnection};
use gpui::*;
use gpui_component::input::{Input, InputState};
use gpui_component::scroll::ScrollableElement;
use gpui_component::{
    ActiveTheme, Icon, Sizable as _, button::Button, button::ButtonVariants as _,
};

use crate::motion::{MotionValue, vertical_reveal_offset};
use crate::ui_scale::ui_icon_px;

actions!(server_picker, [ToggleServerPicker]);

/// 用户在浮层里选中了一条连接，需要开一个新终端标签。
pub struct ServerSelected {
    /// 可直接写入 shell 的单行命令。
    pub command: String,
    pub name: String,
    /// 完整连接信息。标签分屏时用它再开一个独立会话。
    pub connection: SshConnection,
}

/// 当前编辑的是新条目还是列表里的第几条。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorTarget {
    New,
    Existing(usize),
}

#[derive(Clone)]
struct EditorForm {
    target: EditorTarget,
    name: Entity<InputState>,
    host: Entity<InputState>,
    port: Entity<InputState>,
    user: Entity<InputState>,
    identity_file: Entity<InputState>,
    password: Entity<InputState>,
    group: Entity<InputState>,
    /// 分组候选列表是否展开。
    group_menu_open: bool,
}

pub struct ServerPicker {
    visible: bool,
    book: ConnectionBook,
    query_text: String,
    query: Entity<InputState>,
    selected_index: usize,
    focus_handle: FocusHandle,
    scroll_handle: ScrollHandle,
    ui_opacity: f32,
    overlay_motion: MotionValue,
    editor: Option<EditorForm>,
    /// 分组输入框的订阅句柄。每次打开编辑器都重新订阅，旧的要先释放，
    /// 否则回调会越积越多。
    group_observer: Option<Subscription>,
}

impl ServerPicker {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_placeholder("搜索服务器…", window, cx);
            state
        });
        // 搜索框内容变化时重绘自身，过滤列表才会实时跟随输入。
        cx.observe(&query, |_, _, cx| cx.notify()).detach();
        Self {
            visible: false,
            book: ConnectionBook::default(),
            query_text: String::new(),
            query,
            selected_index: 0,
            focus_handle: cx.focus_handle(),
            scroll_handle: ScrollHandle::new(),
            ui_opacity: 1.0,
            overlay_motion: MotionValue::new(0.0),
            editor: None,
            group_observer: None,
        }
    }

    pub fn set_ui_opacity(&mut self, opacity: f32) {
        self.ui_opacity = opacity.clamp(0.0, 1.0);
    }

    pub fn is_visible(&self) -> bool {
        self.visible || self.overlay_motion.is_animating()
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.visible {
            self.dismiss(cx);
            return;
        }
        // 每次打开都重新读取，手改 connections.json 后不必重启。
        self.book = ConnectionBook::load();
        self.query_text.clear();
        self.selected_index = 0;
        self.editor = None;
        self.group_observer = None;
        self.query.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.visible = true;
        self.overlay_motion
            .set_target(1.0, std::time::Duration::from_millis(160));
        // 焦点交给搜索框，键事件会冒泡到卡片的 on_key_down。
        self.query.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.editor = None;
        self.group_observer = None;
        self.overlay_motion
            .set_target(0.0, std::time::Duration::from_millis(140));
        cx.notify();
    }

    /// 当前查询过滤后的连接下标（排序与过滤在 con-core 里做，可单测）。
    fn visible_indices(&self) -> Vec<usize> {
        self.book.visible_indices(&self.query_text)
    }

    fn connect(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(connection) = self.book.connections.get(index) else {
            return;
        };
        // 写不出密码脚本就没有可用的登录方式，静默不连会让人以为按钮没反应。
        let Some(command) = cube_core::connections::connect_command(connection) else {
            use gpui_component::{WindowExt as _, notification::Notification};
            window.push_notification(
                Notification::new()
                    .title("无法准备密码登录")
                    .message("请检查临时目录权限后重试。"),
                cx,
            );
            return;
        };
        cx.emit(ServerSelected {
            command,
            name: connection.name.clone(),
            connection: connection.clone(),
        });
        self.visible = false;
        self.editor = None;
        self.group_observer = None;
        self.overlay_motion
            .set_target(0.0, std::time::Duration::from_millis(140));
        cx.notify();
    }

    fn connect_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let indices = self.visible_indices();
        let Some(&index) = indices.get(self.selected_index) else {
            return;
        };
        self.connect(index, window, cx);
    }

    fn open_editor(&mut self, target: EditorTarget, window: &mut Window, cx: &mut Context<Self>) {
        let existing = match target {
            EditorTarget::Existing(index) => self.book.connections.get(index).cloned(),
            EditorTarget::New => None,
        };
        let value =
            |field: fn(&SshConnection) -> String| existing.as_ref().map(field).unwrap_or_default();
        let port = existing
            .as_ref()
            .and_then(|connection| connection.port)
            .map(|port| port.to_string())
            .unwrap_or_default();
        let make = |window: &mut Window,
                    cx: &mut Context<Self>,
                    placeholder: &'static str,
                    text: String| {
            cx.new(|cx| {
                let mut state = InputState::new(window, cx);
                state.set_placeholder(placeholder, window, cx);
                if !text.is_empty() {
                    state.set_value(text, window, cx);
                }
                state
            })
        };
        // 密码框用掩码显示，避免截图或旁人直接看到。
        let make_masked = |window: &mut Window,
                           cx: &mut Context<Self>,
                           placeholder: &'static str,
                           text: String| {
            cx.new(|cx| {
                let mut state = InputState::new(window, cx);
                state.set_placeholder(placeholder, window, cx);
                if !text.is_empty() {
                    state.set_value(text, window, cx);
                }
                state.set_masked(true, window, cx);
                state
            })
        };

        self.editor = Some(EditorForm {
            target,
            name: make(
                window,
                cx,
                "名称，例如 香港-博客",
                value(|c| c.name.clone()),
            ),
            host: make(window, cx, "IP 或域名", value(|c| c.host.clone())),
            port: make(window, cx, "22", port),
            user: make(
                window,
                cx,
                "root",
                value(|c| c.user.clone().unwrap_or_default()),
            ),
            identity_file: make(
                window,
                cx,
                "~/.ssh/id_ed25519（可选）",
                value(|c| c.identity_file.clone().unwrap_or_default()),
            ),
            password: make_masked(
                window,
                cx,
                "密码（可选）",
                value(|c| c.password.clone().unwrap_or_default()),
            ),
            group: make(
                window,
                cx,
                "分组，例如 生产（可选）",
                value(|c| c.group.clone().unwrap_or_default()),
            ),
            group_menu_open: false,
        });
        if let Some(form) = &self.editor {
            form.name.update(cx, |input, cx| input.focus(window, cx));
            // 分组输入框变化后要重绘，候选里的「当前选中」勾才会跟着动。
            self.group_observer = Some(cx.observe(&form.group, |_, _, cx| cx.notify()));
        }
        cx.notify();
    }

    fn close_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor = None;
        self.group_observer = None;
        self.focus_search(window, cx);
        cx.notify();
    }

    fn focus_search(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.query.update(cx, |input, cx| input.focus(window, cx));
    }

    fn save_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.editor.clone() else {
            return;
        };
        let text = |input: &Entity<InputState>| input.read(cx).value().trim().to_string();

        // 编辑已有条目时沿用它的 id：改名字不该让这一行在 UI 里变成新元素。
        let existing_id = match form.target {
            EditorTarget::Existing(index) => self
                .book
                .connections
                .get(index)
                .map(|connection| connection.id.clone())
                .unwrap_or_default(),
            EditorTarget::New => String::new(),
        };

        let mut connection = SshConnection {
            id: existing_id,
            name: text(&form.name),
            host: text(&form.host),
            port: text(&form.port).parse().ok(),
            user: Some(text(&form.user)).filter(|value| !value.is_empty()),
            identity_file: Some(text(&form.identity_file)).filter(|value| !value.is_empty()),
            // 密码不 trim：首尾空格可能就是密码的一部分。
            password: Some(form.password.read(cx).value().to_string())
                .filter(|value| !value.is_empty()),
            group: Some(text(&form.group)).filter(|value| !value.is_empty()),
        };
        connection.normalize();
        connection.ensure_id(self.book.connections.len() as u64);

        // 名称和主机是必填项；缺少时保留编辑器，用户可以继续补齐。
        if !connection.is_valid() {
            return;
        }

        let saved_name = connection.name.clone();
        match form.target {
            EditorTarget::Existing(index) if index < self.book.connections.len() => {
                self.book.connections[index] = connection;
            }
            EditorTarget::Existing(_) => return,
            EditorTarget::New => self.book.connections.push(connection),
        }

        // 写盘失败的提示必须可见：编辑器一关，用户的修改就只活在内存里，
        // 下次打开浮层重新 load 时会悄无声息地丢掉。
        if let Err(error) = self.book.save() {
            log::error!("connections: failed to save: {error}");
            use gpui_component::{WindowExt as _, notification::Notification};
            window.push_notification(
                Notification::new()
                    .title("服务器列表保存失败")
                    .message(format!(
                        "{error}\n修改只存在于本次会话，请检查配置文件权限。"
                    )),
                cx,
            );
            return;
        }

        self.book = ConnectionBook::load();
        self.editor = None;
        self.group_observer = None;
        // selected_index 是「可见列表中的位置」，不是底层数组下标：有分组时
        // 列表按分组重排，用数组长度会指到别的条目上。
        self.selected_index =
            cube_core::connections::visible_position_of(&self.book, &saved_name).unwrap_or(0);
        self.focus_search(window, cx);
        cx.notify();
    }

    fn delete(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.book.connections.len() {
            return;
        }
        self.book.connections.remove(index);
        if let Err(error) = self.book.save() {
            log::error!("connections: failed to save after delete: {error}");
            use gpui_component::{WindowExt as _, notification::Notification};
            window.push_notification(
                Notification::new()
                    .title("删除未写入磁盘")
                    .message(format!("{error}\n重新打开浮层后这条记录会回来。")),
                cx,
            );
        }
        self.editor = None;
        self.group_observer = None;
        // 与 save 同理：selected_index 是可见位置，用可见条目数收敛。
        let remaining = self.visible_indices().len();
        self.selected_index = self.selected_index.min(remaining.saturating_sub(1));
        cx.notify();
    }

    fn field(label: &'static str, input: &Entity<InputState>, muted: Hsla) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                div()
                    .text_size(px(11.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(muted)
                    .child(label),
            )
            .child(Input::new(input).small())
    }
}

impl EventEmitter<ServerSelected> for ServerPicker {}

impl Focusable for ServerPicker {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ServerPicker {
    fn render_group_label(label: String, top_padding: Pixels, ink: Hsla) -> impl IntoElement {
        div()
            .px(px(10.0))
            .pt(top_padding)
            .pb(px(4.0))
            .text_size(px(11.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(ink)
            .child(label)
    }

    fn render_list(
        &mut self,
        theme: &gpui_component::Theme,
        selected: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut list = div()
            .id("server-picker-list")
            .role(Role::ListBox)
            .flex()
            .flex_col()
            .py(px(6.0))
            .px(px(6.0))
            .max_h(px(420.0))
            .overflow_y_scroll()
            .track_scroll(&self.scroll_handle);

        let indices = self.visible_indices();
        if indices.is_empty() {
            let message = if self.book.connections.is_empty() {
                "还没有保存的服务器，点右上角 + 添加"
            } else {
                "没有匹配的服务器"
            };
            list = list.child(
                div()
                    .px(px(14.0))
                    .py(px(18.0))
                    .text_size(px(13.0))
                    .text_color(theme.muted_foreground.opacity(0.75))
                    .child(message),
            );
        }

        let selected_bg = theme.foreground.opacity(0.075);
        let hover_bg = theme.foreground.opacity(0.055);
        let transparent = theme.transparent;
        let primary = theme.primary;
        let muted = theme.muted_foreground;
        let name_ink = theme.foreground.opacity(0.92);
        let target_ink = theme.muted_foreground.opacity(0.80);
        let group_ink = theme.muted_foreground.opacity(0.72);
        let mono_font = theme.mono_font_family.clone();

        let mut has_group_header = false;
        let mut last_group: Option<String> = None;
        for (position, &index) in indices.iter().enumerate() {
            let connection = self.book.connections[index].clone();
            if connection.group != last_group {
                if let Some(label) = connection.group.as_ref() {
                    let top_padding = if has_group_header { px(12.0) } else { px(4.0) };
                    list = list.child(Self::render_group_label(
                        label.clone(),
                        top_padding,
                        group_ink,
                    ));
                    has_group_header = true;
                }
                last_group = connection.group.clone();
            }

            let is_selected = position == selected;
            let target = connection.display_target();
            let edit_tip = SharedString::from(format!("编辑 {}", connection.name));
            let delete_tip = SharedString::from(format!("删除 {}", connection.name));
            // 元素身份必须跟随条目本身，而不是它在数组里的位置：删掉一行后
            // 下标会整体前移，用下标做 ElementId 会把上一帧的行状态错配到
            // 新的一行上，表现为选中高亮与悬停区错位。
            let row_id = connection.id.clone();
            // 每行一个独立组名。共用一个组名时，命中区栈会把不同行的悬停
            // 状态串起来，删除一行后更容易错位。
            let row_group = SharedString::from(format!("server-row-{row_id}"));
            list = list.child(
                div()
                    .id(SharedString::from(format!("server-row-{row_id}")))
                    .role(Role::ListBoxOption)
                    .aria_selected(is_selected)
                    .aria_label(SharedString::from(format!(
                        "{} {}",
                        connection.name, target
                    )))
                    .group(row_group.clone())
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .h(px(38.0))
                    .px(px(10.0))
                    .rounded(px(8.0))
                    .cursor_pointer()
                    .bg(if is_selected {
                        selected_bg
                    } else {
                        transparent
                    })
                    .hover(move |style| style.bg(hover_bg))
                    .child(
                        svg()
                            .path("phosphor/terminal-window.svg")
                            .size(ui_icon_px(theme, 14.0))
                            .flex_shrink_0()
                            .text_color(if is_selected { primary } else { muted }),
                    )
                    .child(
                        div()
                            .min_w(px(110.0))
                            .max_w(px(180.0))
                            .truncate()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(name_ink)
                            .child(connection.name.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.0))
                            .font_family(mono_font.clone())
                            .text_color(target_ink)
                            .child(target),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.0))
                            .invisible()
                            .group_hover(row_group.clone(), |style| style.visible())
                            .child(
                                Button::new(SharedString::from(format!("server-edit-{row_id}")))
                                    .icon(Icon::default().path("phosphor/pencil-simple.svg"))
                                    .ghost()
                                    .small()
                                    .tooltip(edit_tip)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.open_editor(EditorTarget::Existing(index), window, cx);
                                    })),
                            )
                            .child(
                                Button::new(SharedString::from(format!("server-delete-{row_id}")))
                                    .icon(Icon::default().path("phosphor/trash.svg"))
                                    .ghost()
                                    .small()
                                    .tooltip(delete_tip)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.delete(index, window, cx);
                                    })),
                            ),
                    )
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if this.selected_index != position {
                            this.selected_index = position;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.connect(index, window, cx);
                    })),
            );
        }

        list.into_any_element()
    }

    fn render_editor(
        &mut self,
        theme: &gpui_component::Theme,
        form: EditorForm,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = theme.muted_foreground.opacity(0.78);
        let hint_ink = theme.muted_foreground.opacity(0.75);
        let title = if form.target == EditorTarget::New {
            "添加服务器"
        } else {
            "编辑服务器"
        };

        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(6.0))
                    .text_size(px(14.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .px(px(16.0))
                    .py(px(8.0))
                    .child(Self::field("名称", &form.name, muted))
                    .child(Self::field("主机 / IP", &form.host, muted))
                    .child(
                        div()
                            .flex()
                            .gap(px(10.0))
                            .child(div().flex_1().child(Self::field("端口", &form.port, muted)))
                            .child(
                                div()
                                    .flex_1()
                                    .child(Self::field("用户名", &form.user, muted)),
                            ),
                    )
                    .child(Self::field("私钥路径", &form.identity_file, muted))
                    .child(Self::field("密码", &form.password, muted))
                    .child(self.render_group_field(&form, theme, cx)),
            )
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(10.0))
                    .text_size(px(11.0))
                    .line_height(px(16.0))
                    .text_color(hint_ink)
                    .child("密码以自己的账户权限明文保存在 connections.json，连接时经 SSH_ASKPASS 传入，不进命令行。私钥与密码二选一。"),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.0))
                    .px(px(16.0))
                    .py(px(14.0))
                    .child(
                        Button::new("server-editor-cancel")
                            .label("取消")
                            .ghost()
                            .small()
                            .on_click(
                                cx.listener(|this, _, window, cx| this.close_editor(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("server-editor-save")
                            .label("保存")
                            .primary()
                            .small()
                            .on_click(
                                cx.listener(|this, _, window, cx| this.save_editor(window, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// 分组字段：输入框 + 已有分组的下拉候选。
    ///
    /// 分组在数据层只是每条连接上的一个字符串，没有独立的分组表。所以这里
    /// 不引入 Select 那套 delegate：直接从现有连接里汇总出候选，点一下填进
    /// 输入框；输入框本身仍然自由输入，写个新名字保存后就等于新建了分组。
    fn render_group_field(
        &self,
        form: &EditorForm,
        theme: &gpui_component::Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = theme.muted_foreground.opacity(0.78);
        let candidates = self.book.group_names();
        let current = form.group.read(cx).value().to_string();

        let mut container = div().flex().flex_col().gap(px(4.0)).child(
            div()
                .text_size(px(11.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(muted)
                .child("分组"),
        );

        let mut row = div().flex().items_center().gap(px(6.0)).child(
            div()
                .flex_1()
                .min_w_0()
                .child(Input::new(&form.group).small()),
        );

        // 没有候选就没什么可下拉的，只留自由输入。
        if !candidates.is_empty() {
            row = row.child(
                Button::new("server-group-menu")
                    .icon(Icon::default().path("phosphor/caret-down.svg"))
                    .ghost()
                    .small()
                    .tooltip("选择已有分组")
                    .on_click(cx.listener(|this, _, window, cx| {
                        let mut opened = false;
                        if let Some(form) = this.editor.as_mut() {
                            form.group_menu_open = !form.group_menu_open;
                            opened = form.group_menu_open;
                        }
                        // 候选展开时移走输入框焦点：否则方向键、回车会被
                        // 输入框吃掉，卡片的 on_key_down 收不到。
                        if opened {
                            window.blur(cx);
                        }
                        cx.notify();
                    })),
            );
        }
        container = container.child(row);

        if form.group_menu_open && !candidates.is_empty() {
            let mut menu = div()
                .id("server-group-candidates")
                .role(Role::ListBox)
                .flex()
                .flex_col()
                .p(px(4.0))
                .rounded(px(10.0))
                .bg(theme.popover);

            for candidate in candidates {
                let is_current = candidate == current;
                let label = candidate.clone();
                menu = menu.child(
                    div()
                        .id(SharedString::from(format!("server-group-{candidate}")))
                        .role(Role::ListBoxOption)
                        .aria_selected(is_current)
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .h(px(30.0))
                        .px(px(8.0))
                        .rounded(px(6.0))
                        .cursor_pointer()
                        .text_size(px(12.0))
                        .hover(|style| style.bg(theme.foreground.opacity(0.06)))
                        .child(div().flex_1().truncate().child(label))
                        .children(is_current.then(|| {
                            svg()
                                .path("phosphor/check.svg")
                                .size(px(12.0))
                                .flex_shrink_0()
                                .text_color(theme.primary)
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            let value = candidate.clone();
                            if let Some(form) = this.editor.as_mut() {
                                form.group.update(cx, |input, cx| {
                                    input.set_value(value.clone(), window, cx);
                                    input.focus(window, cx);
                                });
                                form.group_menu_open = false;
                            }
                            cx.notify();
                        })),
                );
            }

            container = container.child(menu);
        }

        container.into_any_element()
    }

    fn render_search(&self, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> AnyElement {
        let search_bg = theme.foreground.opacity(0.055);
        let search_ink = theme.muted_foreground.opacity(0.72);
        let input_ink = theme.foreground.opacity(0.90);
        div()
            .p(px(12.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .h(px(44.0))
                    .px(px(12.0))
                    .rounded(px(12.0))
                    .bg(search_bg)
                    .child(
                        svg()
                            .path("phosphor/magnifying-glass.svg")
                            .size(ui_icon_px(theme, 15.0))
                            .flex_shrink_0()
                            .text_color(search_ink),
                    )
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.query)
                                .appearance(false)
                                .text_size(px(15.0))
                                .text_color(input_ink),
                        ),
                    ),
            )
            .child(
                Button::new("server-add")
                    .icon(Icon::default().path("phosphor/plus.svg"))
                    .ghost()
                    .small()
                    .tooltip("添加服务器")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_editor(EditorTarget::New, window, cx);
                    })),
            )
            .into_any_element()
    }
}

impl Render for ServerPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let progress = self.overlay_motion.value(window, cx);
        if !self.visible && progress <= 0.001 {
            return div().id("server-picker-overlay");
        }

        let previous_query = self.query_text.clone();
        self.query_text = self.query.read(cx).value().to_string();
        if self.query_text != previous_query {
            self.selected_index = 0;
        }

        let theme = cx.theme().clone();
        let viewport = window.viewport_size();
        let top = (viewport.height.as_f32() * 0.10).clamp(12.0, 72.0);
        let selected = {
            let count = self.visible_indices().len();
            self.selected_index.min(count.saturating_sub(1))
        };

        let backdrop = div()
            .id("server-picker-backdrop")
            .occlude()
            .absolute()
            .size_full()
            .bg(theme.background.opacity(0.72 * progress))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.dismiss(cx)),
            );

        let body = match self.editor.clone() {
            Some(form) => self.render_editor(&theme, form, cx),
            None => {
                let search = self.render_search(&theme, cx);
                let list = self.render_list(&theme, selected, cx);
                div()
                    .flex()
                    .flex_col()
                    .child(search)
                    .child(
                        div()
                            .relative()
                            .min_h(px(0.0))
                            .child(list)
                            .vertical_scrollbar(&self.scroll_handle),
                    )
                    .into_any_element()
            }
        };

        let card = div()
            .absolute()
            .top(px(top))
            .left_0()
            .right_0()
            .mx_auto()
            .w(px(560.0))
            .max_w((viewport.width - px(32.0)).max(px(0.0)))
            .rounded(px(16.0))
            .bg(theme.popover.opacity(self.ui_opacity))
            .opacity(progress)
            .flex()
            .flex_col()
            .overflow_hidden()
            .occlude()
            .font_family(theme.font_family.clone())
            .pt(vertical_reveal_offset(progress, 16.0))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if let Some(form) = this.editor.as_ref() {
                    // 分组候选展开时，Esc 先收起候选而不是关掉整个编辑器。
                    if form.group_menu_open {
                        match event.keystroke.key.as_str() {
                            "escape" => {
                                if let Some(form) = this.editor.as_mut() {
                                    form.group_menu_open = false;
                                    form.group.update(cx, |input, cx| {
                                        input.focus(window, cx);
                                    });
                                }
                                cx.notify();
                                return;
                            }
                            "enter" => {
                                // 展开候选时回车不该顺手提交整张表单。
                                return;
                            }
                            _ => {}
                        }
                    }
                    match event.keystroke.key.as_str() {
                        "escape" => this.close_editor(window, cx),
                        "enter" => this.save_editor(window, cx),
                        _ => {}
                    }
                    return;
                }
                let count = this.visible_indices().len();
                match event.keystroke.key.as_str() {
                    "escape" => this.dismiss(cx),
                    "enter" => this.connect_selected(window, cx),
                    "up" if count > 0 => {
                        this.selected_index = if this.selected_index == 0 {
                            count - 1
                        } else {
                            this.selected_index - 1
                        };
                        this.scroll_handle.scroll_to_item(this.selected_index);
                        cx.notify();
                    }
                    "down" if count > 0 => {
                        this.selected_index = (this.selected_index + 1) % count;
                        this.scroll_handle.scroll_to_item(this.selected_index);
                        cx.notify();
                    }
                    _ => {}
                }
            }))
            .child(body);

        div()
            .id("server-picker-overlay")
            .absolute()
            .size_full()
            .child(backdrop)
            .child(card)
    }
}
