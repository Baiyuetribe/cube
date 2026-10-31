use gpui::{
    Action, AnyElement, App, Context, IntoElement as _, ParentElement as _, SharedString,
    Styled as _, Window, div, px,
};
use gpui_component::ActiveTheme as _;
use gpui_component::menu::{PopupMenu, PopupMenuItem};

fn action_item(label: &'static str, action: Box<dyn Action>) -> PopupMenuItem {
    let render_action = action.boxed_clone();
    PopupMenuItem::element(move |window, cx| action_row(label, render_action.as_ref(), window, cx))
        .action(action)
}

fn action_row(
    label: &'static str,
    action: &dyn Action,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let theme = cx.theme();
    let shortcut = crate::keycaps::first_action_keystroke(action, window);

    div()
        .flex()
        .w_full()
        .min_w(px(188.0))
        .items_center()
        .justify_between()
        .gap(px(24.0))
        .font_family(theme.font_family.clone())
        .child(
            div()
                .min_w_0()
                .text_size(px(13.0))
                .line_height(px(18.0))
                .child(SharedString::from(label)),
        )
        .child(
            div().flex().min_w(px(54.0)).justify_end().children(
                shortcut.map(|stroke| crate::keycaps::keycaps_for_stroke(&stroke, theme)),
            ),
        )
        .into_any_element()
}

/// The right-click entry follows the same experimental gate as the toolbar
/// button and the command palette; the workspace passes its in-memory config
/// value so the menu never re-reads persisted config on the UI thread.
pub(crate) fn terminal_context_menu(
    menu: PopupMenu,
    handoff_enabled: bool,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let menu = if cfg!(target_os = "macos") && handoff_enabled {
        menu.item(action_item("移交 Agent…", Box::new(crate::HandoffAgent)))
    } else {
        menu
    };
    menu.min_w(px(240.0))
        .item(action_item("粘贴", Box::new(crate::Paste)))
        .item(action_item("复制", Box::new(crate::Copy)))
        .item(action_item("清空终端", Box::new(crate::ClearTerminal)))
        .separator()
        .submenu("拆分分屏", window, cx, |menu, _window, _cx| {
            menu.min_w(px(190.0))
                .item(action_item("向右拆分", Box::new(crate::SplitRight)))
                .item(action_item("向下拆分", Box::new(crate::SplitDown)))
                .item(action_item("向左拆分", Box::new(crate::SplitLeft)))
                .item(action_item("向上拆分", Box::new(crate::SplitUp)))
        })
        .item(action_item("切换分屏缩放", Box::new(crate::TogglePaneZoom)))
        .item(action_item("关闭分屏", Box::new(crate::ClosePane)))
        .separator()
        .submenu("Surface", window, cx, |menu, _window, _cx| {
            menu.min_w(px(250.0))
                .item(action_item(
                    "新建 Surface 标签",
                    Box::new(crate::NewSurface),
                ))
                .item(action_item(
                    "向右新建 Surface 分屏",
                    Box::new(crate::NewSurfaceSplitRight),
                ))
                .item(action_item(
                    "向下新建 Surface 分屏",
                    Box::new(crate::NewSurfaceSplitDown),
                ))
                .separator()
                .item(action_item(
                    "下一个 Surface 标签",
                    Box::new(crate::NextSurface),
                ))
                .item(action_item(
                    "上一个 Surface 标签",
                    Box::new(crate::PreviousSurface),
                ))
                .item(action_item("重命名当前", Box::new(crate::RenameSurface)))
                .item(action_item("关闭当前", Box::new(crate::CloseSurface)))
        })
        .separator()
        .item(action_item(
            "设置",
            Box::new(crate::settings_panel::ToggleSettings),
        ))
        .item(action_item(
            "命令面板",
            Box::new(crate::command_palette::ToggleCommandPalette),
        ))
}
