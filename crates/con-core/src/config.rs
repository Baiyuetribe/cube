use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

pub mod ghostty;
pub mod transfer;

pub const MIN_UI_FONT_SIZE: f32 = 12.0;
pub const MAX_UI_FONT_SIZE: f32 = 24.0;
pub const DEFAULT_TERMINAL_FONT_FAMILY: &str = "Ioskeley Mono";

fn default_font_family() -> String {
    DEFAULT_TERMINAL_FONT_FAMILY.into()
}
fn default_font_size() -> f32 {
    14.0
}
fn default_theme() -> String {
    "flexoki-light".into()
}
fn default_cursor_style() -> String {
    "bar".into()
}
fn default_ui_font_family() -> String {
    ".SystemUIFont".into()
}
fn default_ui_font_size() -> f32 {
    16.0f32.clamp(MIN_UI_FONT_SIZE, MAX_UI_FONT_SIZE)
}
fn default_terminal_opacity() -> f32 {
    0.80
}
fn default_ui_opacity() -> f32 {
    0.90
}
fn default_terminal_blur() -> bool {
    true
}
fn default_background_image_opacity() -> f32 {
    0.55
}
fn default_background_image_position() -> String {
    "center".into()
}
fn default_background_image_fit() -> String {
    "contain".into()
}
fn default_tab_accent_inactive_alpha() -> f32 {
    0.15
}
fn default_tab_accent_inactive_hover_alpha() -> f32 {
    0.22
}

fn sanitize_tab_accent_alpha(value: f32, default: f32, max: f32) -> f32 {
    if value.is_finite() {
        value.clamp(AppearanceConfig::MIN_TAB_ACCENT_ALPHA, max)
    } else {
        default
    }
}

fn default_restore_terminal_text() -> bool {
    true
}

pub fn is_gpui_pseudo_font_family(name: &str) -> bool {
    name.trim_start().starts_with('.')
}

pub fn is_bundled_terminal_font_family(name: &str) -> bool {
    name.chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .eq_ignore_ascii_case("IoskeleyMono")
}

pub fn sanitize_terminal_font_family(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() || is_gpui_pseudo_font_family(trimmed) {
        DEFAULT_TERMINAL_FONT_FAMILY.to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn sanitize_terminal_font_fallback(fonts: &[String], primary: &str) -> Vec<String> {
    let primary = sanitize_terminal_font_family(primary);
    let mut sanitized = Vec::new();
    for font in fonts {
        let font = font.trim();
        if font.is_empty()
            || is_gpui_pseudo_font_family(font)
            || is_bundled_terminal_font_family(font)
            || font.eq_ignore_ascii_case(&primary)
            || sanitized
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(font))
        {
            continue;
        }
        sanitized.push(font.to_string());
    }
    sanitized
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalConfig {
    /// Shell command used for newly created terminal panes. When unset, Con
    /// follows the platform default (for example the Windows Terminal default
    /// profile or `$SHELL` on Unix).
    pub shell: Option<String>,
    pub font_family: String,
    /// Ordered preferred fallback families. Con's bundled icon font and the
    /// platform/system cascade are appended after this list.
    pub font_fallback: Vec<String>,
    pub font_size: f32,
    pub theme: String,
    pub cursor_style: String,
    /// Allow terminal programs to copy plain text, matching Ghostty's default.
    /// This does not grant clipboard read access.
    pub clipboard_write: bool,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            shell: None,
            font_family: default_font_family(),
            font_fallback: Vec::new(),
            font_size: default_font_size(),
            theme: default_theme(),
            cursor_style: default_cursor_style(),
            clipboard_write: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceConfig {
    pub terminal_opacity: f32,
    pub terminal_blur: bool,
    /// Independent panel/popup opacity. Terminal-adjacent base surfaces follow
    /// the terminal's effective background-opacity instead.
    pub ui_opacity: f32,
    pub ui_font_family: String,
    pub ui_font_size: f32,
    pub background_image: Option<String>,
    pub background_image_opacity: f32,
    pub background_image_position: String,
    pub background_image_fit: String,
    pub background_image_repeat: bool,
    /// Accent color alpha for inactive tabs and unfocused pane titles.
    pub tab_accent_inactive_alpha: f32,
    /// Accent color alpha when hovering inactive tabs.
    pub tab_accent_inactive_hover_alpha: f32,
    /// Keep bounded private terminal text so restart continuity can show what
    /// was on screen. This is never exported to workspace layout profiles.
    pub restore_terminal_text: bool,
    /// Hide the per-pane title bar when there are multiple panes. Defaults to
    /// `false` (title bar visible). When `true` the title bar is suppressed
    /// even in split layouts; the fullscreen/close buttons are also hidden.
    pub hide_pane_title_bar: bool,
    /// Workspace tab presentation. Vertical is the default; horizontal remains
    /// available for users who prefer a top tab strip.
    pub tabs_orientation: TabsOrientation,
    /// When `true` (default), closing the last tab closes the window (and
    /// quits the app on Windows/Linux). When `false`, closing the last tab
    /// opens a fresh tab instead so the window stays open.
    pub close_to_quit: bool,
    /// Selectable app icon id from [`APP_ICONS`]. Unknown values fall back to
    /// [`DEFAULT_APP_ICON`].
    pub app_icon: String,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            terminal_opacity: default_terminal_opacity(),
            terminal_blur: default_terminal_blur(),
            ui_opacity: default_ui_opacity(),
            ui_font_family: default_ui_font_family(),
            ui_font_size: default_ui_font_size(),
            background_image: None,
            background_image_opacity: default_background_image_opacity(),
            background_image_position: default_background_image_position(),
            background_image_fit: default_background_image_fit(),
            background_image_repeat: false,
            tab_accent_inactive_alpha: default_tab_accent_inactive_alpha(),
            tab_accent_inactive_hover_alpha: default_tab_accent_inactive_hover_alpha(),
            restore_terminal_text: default_restore_terminal_text(),
            hide_pane_title_bar: false,
            tabs_orientation: TabsOrientation::Vertical,
            close_to_quit: true,
            app_icon: default_app_icon(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TabsOrientation {
    Horizontal,
    #[default]
    Vertical,
}

impl TabsOrientation {
    pub fn is_vertical(self) -> bool {
        matches!(self, Self::Vertical)
    }
}

impl AppearanceConfig {
    pub const MIN_TAB_ACCENT_ALPHA: f32 = 0.05;
    pub const MAX_TAB_ACCENT_INACTIVE_ALPHA: f32 = 0.30;
    pub const MAX_TAB_ACCENT_INACTIVE_HOVER_ALPHA: f32 = 0.40;

    pub fn normalize(&mut self) {
        self.tab_accent_inactive_alpha = sanitize_tab_accent_alpha(
            self.tab_accent_inactive_alpha,
            default_tab_accent_inactive_alpha(),
            Self::MAX_TAB_ACCENT_INACTIVE_ALPHA,
        );
        self.tab_accent_inactive_hover_alpha = sanitize_tab_accent_alpha(
            self.tab_accent_inactive_hover_alpha,
            default_tab_accent_inactive_hover_alpha(),
            Self::MAX_TAB_ACCENT_INACTIVE_HOVER_ALPHA,
        )
        .max(self.tab_accent_inactive_alpha);
        self.app_icon = sanitize_app_icon(&self.app_icon);
    }
}

pub const DEFAULT_APP_ICON: &str = "raccoon-tail-a6";
pub const DEFAULT_APP_ICON_ASSET: &str = "app-icons/con-raccoon-tail-A6.png";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppIconChoice {
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub asset: &'static str,
}

pub const APP_ICON_GROUPS: &[&str] = &["Classic", "Peek", "Suit", "Tail"];

pub const APP_ICONS: &[AppIconChoice] = &[
    AppIconChoice {
        id: "default",
        label: "Classic",
        group: "Classic",
        asset: "Con-macOS-Dark-256x256@2x.png",
    },
    AppIconChoice {
        id: "raccoon-a1",
        label: "A1",
        group: "Peek",
        asset: "app-icons/con-raccoon-A1.png",
    },
    AppIconChoice {
        id: "raccoon-a2",
        label: "A2",
        group: "Peek",
        asset: "app-icons/con-raccoon-A2.png",
    },
    AppIconChoice {
        id: "raccoon-b1",
        label: "B1",
        group: "Peek",
        asset: "app-icons/con-raccoon-B1.png",
    },
    AppIconChoice {
        id: "raccoon-b2",
        label: "B2",
        group: "Peek",
        asset: "app-icons/con-raccoon-B2.png",
    },
    AppIconChoice {
        id: "raccoon-c1",
        label: "C1",
        group: "Peek",
        asset: "app-icons/con-raccoon-C1.png",
    },
    AppIconChoice {
        id: "raccoon-c2",
        label: "C2",
        group: "Peek",
        asset: "app-icons/con-raccoon-C2.png",
    },
    AppIconChoice {
        id: "raccoon-suit-a1",
        label: "A1",
        group: "Suit",
        asset: "app-icons/con-raccoon-suit-A1.png",
    },
    AppIconChoice {
        id: "raccoon-suit-a2",
        label: "A2",
        group: "Suit",
        asset: "app-icons/con-raccoon-suit-A2.png",
    },
    AppIconChoice {
        id: "raccoon-suit-a3",
        label: "A3",
        group: "Suit",
        asset: "app-icons/con-raccoon-suit-A3.png",
    },
    AppIconChoice {
        id: "raccoon-suit-a4",
        label: "A4",
        group: "Suit",
        asset: "app-icons/con-raccoon-suit-A4.png",
    },
    AppIconChoice {
        id: "raccoon-suit-a5",
        label: "A5",
        group: "Suit",
        asset: "app-icons/con-raccoon-suit-A5.png",
    },
    AppIconChoice {
        id: "raccoon-suit-a6",
        label: "A6",
        group: "Suit",
        asset: "app-icons/con-raccoon-suit-A6.png",
    },
    AppIconChoice {
        id: "raccoon-tail-a1",
        label: "A1",
        group: "Tail",
        asset: "app-icons/con-raccoon-tail-A1.png",
    },
    AppIconChoice {
        id: "raccoon-tail-a2",
        label: "A2",
        group: "Tail",
        asset: "app-icons/con-raccoon-tail-A2.png",
    },
    AppIconChoice {
        id: "raccoon-tail-a3",
        label: "A3",
        group: "Tail",
        asset: "app-icons/con-raccoon-tail-A3.png",
    },
    AppIconChoice {
        id: "raccoon-tail-a4",
        label: "A4",
        group: "Tail",
        asset: "app-icons/con-raccoon-tail-A4.png",
    },
    AppIconChoice {
        id: "raccoon-tail-a5",
        label: "A5",
        group: "Tail",
        asset: "app-icons/con-raccoon-tail-A5.png",
    },
    AppIconChoice {
        id: "raccoon-tail-a6",
        label: "A6",
        group: "Tail",
        asset: "app-icons/con-raccoon-tail-A6.png",
    },
];

fn default_app_icon() -> String {
    DEFAULT_APP_ICON.to_string()
}

pub fn app_icon_by_id(id: &str) -> Option<&'static AppIconChoice> {
    APP_ICONS.iter().find(|icon| icon.id == id)
}

pub fn sanitize_app_icon(id: &str) -> String {
    let trimmed = id.trim();
    if app_icon_by_id(trimmed).is_some() {
        trimmed.to_string()
    } else {
        DEFAULT_APP_ICON.to_string()
    }
}

pub fn app_icon_asset(id: &str) -> &'static str {
    app_icon_by_id(id)
        .map(|icon| icon.asset)
        .unwrap_or(DEFAULT_APP_ICON_ASSET)
}

// Default keybindings are chosen per platform. On macOS the `secondary-`
// modifier token (⌘) is the right primary: Cmd+<letter> doesn't collide
// with anything the terminal expects. On Windows/Linux `secondary-`
// resolves to `Ctrl`, and bare Ctrl+<letter> often has shell meaning
// (Ctrl+L = clear, Ctrl+C = SIGINT, Ctrl+I = Tab, ...), so most app
// actions avoid that space. Pane-management shortcuts therefore stay in
// app-level modifier space instead of borrowing terminal control
// characters like Ctrl+D, which terminals consume as EOF before Con's
// keybindings ever see them.
fn default_command_palette() -> String {
    // Ctrl+Shift+P on Windows / Linux, Cmd+Shift+P on macOS.
    "secondary-shift-p".into()
}
#[cfg(target_os = "macos")]
fn default_focus_files() -> String {
    // Cmd+Shift+E collides with the embedded terminal/AppKit search-selection
    // path on macOS, so keep the E mnemonic on an option chord that survives
    // native terminal focus.
    "secondary-alt-e".into()
}
#[cfg(not(target_os = "macos"))]
fn default_focus_files() -> String {
    // Matches the common editor convention: Ctrl+Shift+E.
    "secondary-shift-e".into()
}
fn default_search_files() -> String {
    // Matches the common editor convention: Cmd/Ctrl+Shift+F.
    "secondary-shift-f".into()
}
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn default_find_in_terminal() -> String {
    "secondary-f".into()
}
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn default_find_in_terminal() -> String {
    String::new()
}

#[cfg(target_os = "macos")]
fn default_new_tab() -> String {
    "secondary-t".into()
}
#[cfg(not(target_os = "macos"))]
fn default_new_tab() -> String {
    "ctrl-shift-t".into()
}

#[cfg(target_os = "macos")]
fn default_new_window() -> String {
    "secondary-n".into()
}
#[cfg(not(target_os = "macos"))]
fn default_new_window() -> String {
    "ctrl-shift-n".into()
}

#[cfg(target_os = "macos")]
fn default_close_tab() -> String {
    "secondary-w".into()
}
#[cfg(not(target_os = "macos"))]
fn default_close_tab() -> String {
    "ctrl-shift-w".into()
}

#[cfg(target_os = "macos")]
fn default_close_pane() -> String {
    "secondary-alt-w".into()
}
#[cfg(not(target_os = "macos"))]
fn default_close_pane() -> String {
    "alt-shift-w".into()
}

#[cfg(target_os = "macos")]
fn default_toggle_pane_zoom() -> String {
    "secondary-shift-enter".into()
}
#[cfg(not(target_os = "macos"))]
fn default_toggle_pane_zoom() -> String {
    "alt-shift-enter".into()
}

fn default_next_tab() -> String {
    "ctrl-tab".into()
}
fn default_previous_tab() -> String {
    "ctrl-shift-tab".into()
}

fn default_settings() -> String {
    // Ctrl+, is the cross-editor convention (VSCode, IntelliJ, Windows
    // Terminal) and doesn't produce a control character, so it works
    // the same on both platforms via `secondary-`.
    "secondary-,".into()
}

#[cfg(target_os = "macos")]
fn default_quit() -> String {
    "secondary-q".into()
}
#[cfg(not(target_os = "macos"))]
fn default_quit() -> String {
    // Alt+F4 is the Windows platform convention for "close the app
    // window". Ctrl+Q is XOFF / pwsh's quoted-insert, so it can't be
    // used without stealing it from the shell.
    "alt-f4".into()
}

#[cfg(target_os = "macos")]
fn default_split_right() -> String {
    "secondary-d".into()
}
#[cfg(not(target_os = "macos"))]
fn default_split_right() -> String {
    "alt-d".into()
}

#[cfg(target_os = "macos")]
fn default_split_down() -> String {
    "secondary-shift-d".into()
}
#[cfg(not(target_os = "macos"))]
fn default_split_down() -> String {
    "alt-shift-d".into()
}

#[cfg(target_os = "macos")]
fn default_focus_input() -> String {
    "secondary-i".into()
}
#[cfg(not(target_os = "macos"))]
fn default_focus_input() -> String {
    // Ctrl+I is the Tab character (0x09). Ctrl+Shift+I stays free.
    "ctrl-shift-i".into()
}

fn default_toggle_input_bar() -> String {
    "ctrl-`".into()
}
fn default_cycle_input_mode() -> String {
    "secondary-;".into()
}
fn default_toggle_pane_scope() -> String {
    "secondary-'".into()
}
#[cfg(target_os = "macos")]
fn default_toggle_left_panel() -> String {
    // Cmd+B is the established macOS/editor convention for showing or
    // hiding the left sidebar, and Cmd chords do not steal terminal input.
    "secondary-b".into()
}
#[cfg(not(target_os = "macos"))]
fn default_toggle_left_panel() -> String {
    // Avoid bare Ctrl+B on Windows/Linux: it is tmux's prefix and a
    // real terminal control character. Ctrl+Shift+B stays app-level.
    "ctrl-shift-b".into()
}
#[cfg(target_os = "macos")]
fn default_collapse_sidebar() -> String {
    "secondary-shift-b".into()
}
#[cfg(not(target_os = "macos"))]
fn default_collapse_sidebar() -> String {
    "ctrl-alt-b".into()
}

#[cfg(target_os = "macos")]
fn default_new_surface() -> String {
    "secondary-alt-t".into()
}
#[cfg(not(target_os = "macos"))]
fn default_new_surface() -> String {
    "alt-shift-t".into()
}

#[cfg(target_os = "macos")]
fn default_new_surface_split_right() -> String {
    "secondary-alt-d".into()
}
#[cfg(not(target_os = "macos"))]
fn default_new_surface_split_right() -> String {
    "alt-shift-right".into()
}

#[cfg(target_os = "macos")]
fn default_new_surface_split_down() -> String {
    "secondary-alt-shift-d".into()
}
#[cfg(not(target_os = "macos"))]
fn default_new_surface_split_down() -> String {
    "alt-shift-down".into()
}

#[cfg(target_os = "macos")]
fn default_next_surface() -> String {
    "secondary-ctrl-]".into()
}
#[cfg(not(target_os = "macos"))]
fn default_next_surface() -> String {
    "alt-shift-]".into()
}

#[cfg(target_os = "macos")]
fn default_previous_surface() -> String {
    "secondary-ctrl-[".into()
}
#[cfg(not(target_os = "macos"))]
fn default_previous_surface() -> String {
    "alt-shift-[".into()
}

#[cfg(target_os = "macos")]
fn default_rename_surface() -> String {
    "secondary-alt-r".into()
}
#[cfg(not(target_os = "macos"))]
fn default_rename_surface() -> String {
    "alt-shift-r".into()
}

#[cfg(target_os = "macos")]
fn default_close_surface() -> String {
    "secondary-alt-shift-w".into()
}
#[cfg(not(target_os = "macos"))]
fn default_close_surface() -> String {
    "alt-shift-x".into()
}

fn default_global_summon() -> String {
    "alt-space".into()
}
fn default_global_summon_enabled() -> bool {
    false
}
fn default_quick_terminal_enabled() -> bool {
    false
}
fn default_quick_terminal() -> String {
    "cmd-\\".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindingConfig {
    pub command_palette: String,
    pub new_window: String,
    pub new_tab: String,
    pub close_tab: String,
    pub close_pane: String,
    pub toggle_pane_zoom: String,
    pub next_tab: String,
    pub previous_tab: String,
    pub settings: String,
    pub quit: String,
    pub split_right: String,
    pub split_down: String,
    pub focus_input: String,
    pub cycle_input_mode: String,
    pub toggle_input_bar: String,
    pub toggle_pane_scope: String,
    #[serde(alias = "toggle_vertical_tabs")]
    pub toggle_left_panel: String,
    pub focus_files: String,
    pub search_files: String,
    pub find_in_terminal: String,
    pub collapse_sidebar: String,
    pub new_surface: String,
    pub new_surface_split_right: String,
    pub new_surface_split_down: String,
    pub next_surface: String,
    pub previous_surface: String,
    pub rename_surface: String,
    pub close_surface: String,
    pub global_summon_enabled: bool,
    pub global_summon: String,
    pub quick_terminal_enabled: bool,
    pub quick_terminal: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeybindingConflict {
    pub binding: String,
    pub actions: Vec<String>,
}

impl KeybindingConfig {
    pub fn normalize(&mut self) {
        #[cfg(target_os = "macos")]
        {
            // Option+bracket resolves to a Unicode punctuation character on
            // macOS before GPUI can treat it as a shortcut, so migrate only
            // the exact former defaults to the current reachable defaults.
            if canonical_keybinding(&self.next_surface) == canonical_keybinding("secondary-alt-]") {
                self.next_surface = default_next_surface();
            }
            if canonical_keybinding(&self.previous_surface)
                == canonical_keybinding("secondary-alt-[")
            {
                self.previous_surface = default_previous_surface();
            }
        }
    }

    pub fn active_shortcuts(&self) -> Vec<(&'static str, &str)> {
        let mut shortcuts = vec![
            ("新建窗口", self.new_window.as_str()),
            ("新建标签", self.new_tab.as_str()),
            ("下一个标签", self.next_tab.as_str()),
            ("上一个标签", self.previous_tab.as_str()),
            ("关闭标签", self.close_tab.as_str()),
            ("关闭分屏", self.close_pane.as_str()),
            ("切换分屏缩放", self.toggle_pane_zoom.as_str()),
            ("设置", self.settings.as_str()),
            ("命令面板", self.command_palette.as_str()),
            ("切换输入栏", self.toggle_input_bar.as_str()),
            ("切换输入 / 终端", self.focus_input.as_str()),
            ("切换输入模式", self.cycle_input_mode.as_str()),
            ("向右拆分", self.split_right.as_str()),
            ("向下拆分", self.split_down.as_str()),
            ("切换分屏范围", self.toggle_pane_scope.as_str()),
            ("切换左侧边栏", self.toggle_left_panel.as_str()),
            ("聚焦文件", self.focus_files.as_str()),
            ("搜索文件", self.search_files.as_str()),
            #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
            ("在终端中查找", self.find_in_terminal.as_str()),
            ("折叠/展开侧边栏", self.collapse_sidebar.as_str()),
            ("新建 Surface 标签", self.new_surface.as_str()),
            (
                "向右新建 Surface 分屏",
                self.new_surface_split_right.as_str(),
            ),
            (
                "向下新建 Surface 分屏",
                self.new_surface_split_down.as_str(),
            ),
            ("下一个 Surface 标签", self.next_surface.as_str()),
            ("上一个 Surface 标签", self.previous_surface.as_str()),
            ("重命名 Surface", self.rename_surface.as_str()),
            ("关闭 Surface", self.close_surface.as_str()),
            ("退出", self.quit.as_str()),
        ];

        if self.global_summon_enabled {
            shortcuts.push(("唤起 / 隐藏 Con", self.global_summon.as_str()));
        }
        if self.quick_terminal_enabled {
            shortcuts.push(("快速终端", self.quick_terminal.as_str()));
        }

        shortcuts
            .into_iter()
            .filter(|(_, binding)| !binding.trim().is_empty())
            .collect()
    }

    pub fn shortcut_conflicts(
        &self,
        reserved_shortcuts: &[(&'static str, &'static str)],
    ) -> Vec<KeybindingConflict> {
        let mut seen = std::collections::BTreeMap::<String, (String, Vec<String>)>::new();

        for (label, binding) in self
            .active_shortcuts()
            .into_iter()
            .chain(reserved_shortcuts.iter().copied())
        {
            let Some(canonical) = canonical_keybinding(binding) else {
                continue;
            };
            let entry = seen
                .entry(canonical.clone())
                .or_insert_with(|| (canonical, Vec::new()));
            if !entry.1.iter().any(|existing| existing == label) {
                entry.1.push(label.to_string());
            }
        }

        seen.into_values()
            .filter_map(|(binding, actions)| {
                (actions.len() > 1).then_some(KeybindingConflict { binding, actions })
            })
            .collect()
    }
}

impl Default for KeybindingConfig {
    fn default() -> Self {
        Self {
            command_palette: default_command_palette(),
            new_window: default_new_window(),
            new_tab: default_new_tab(),
            close_tab: default_close_tab(),
            close_pane: default_close_pane(),
            toggle_pane_zoom: default_toggle_pane_zoom(),
            next_tab: default_next_tab(),
            previous_tab: default_previous_tab(),
            settings: default_settings(),
            quit: default_quit(),
            split_right: default_split_right(),
            split_down: default_split_down(),
            focus_input: default_focus_input(),
            cycle_input_mode: default_cycle_input_mode(),
            toggle_input_bar: default_toggle_input_bar(),
            toggle_pane_scope: default_toggle_pane_scope(),
            toggle_left_panel: default_toggle_left_panel(),
            focus_files: default_focus_files(),
            search_files: default_search_files(),
            find_in_terminal: default_find_in_terminal(),
            collapse_sidebar: default_collapse_sidebar(),
            new_surface: default_new_surface(),
            new_surface_split_right: default_new_surface_split_right(),
            new_surface_split_down: default_new_surface_split_down(),
            next_surface: default_next_surface(),
            previous_surface: default_previous_surface(),
            rename_surface: default_rename_surface(),
            close_surface: default_close_surface(),
            global_summon_enabled: default_global_summon_enabled(),
            global_summon: default_global_summon(),
            quick_terminal_enabled: default_quick_terminal_enabled(),
            quick_terminal: default_quick_terminal(),
        }
    }
}

pub fn canonical_keybinding(binding: &str) -> Option<String> {
    let strokes = binding
        .split_whitespace()
        .filter_map(canonical_keystroke)
        .collect::<Vec<_>>();
    (!strokes.is_empty()).then(|| strokes.join(" "))
}

fn canonical_keystroke(stroke: &str) -> Option<String> {
    let mut modifiers = Vec::<&'static str>::new();
    // A trailing separator is the literal minus key, e.g. "ctrl--"
    // means Control-Minus rather than a missing key token.
    let is_minus_key = stroke.ends_with('-');
    let mut key = is_minus_key.then(|| "-".to_string());
    let mut parts = stroke
        .split('-')
        .map(|part| part.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    if is_minus_key {
        parts.pop();
    }

    for raw in parts {
        if raw.is_empty() {
            continue;
        }
        match raw.as_str() {
            "cmd" | "command" | "meta" => push_unique_modifier(&mut modifiers, "cmd"),
            "platform" | "secondary" => {
                push_unique_modifier(&mut modifiers, platform_modifier_name())
            }
            "ctrl" | "control" => push_unique_modifier(&mut modifiers, "ctrl"),
            "alt" | "option" => push_unique_modifier(&mut modifiers, "alt"),
            "shift" => push_unique_modifier(&mut modifiers, "shift"),
            "fn" => push_unique_modifier(&mut modifiers, "fn"),
            "return" => key = Some("enter".to_string()),
            "esc" => key = Some("escape".to_string()),
            other => key = Some(other.to_string()),
        }
    }

    let key = key?;
    let mut parts = ["cmd", "ctrl", "alt", "shift", "fn"]
        .into_iter()
        .filter(|modifier| modifiers.contains(modifier))
        .map(str::to_string)
        .collect::<Vec<_>>();
    parts.push(key);
    Some(parts.join("-"))
}

fn platform_modifier_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    }
}

fn push_unique_modifier(modifiers: &mut Vec<&'static str>, modifier: &'static str) {
    if !modifiers.contains(&modifier) {
        modifiers.push(modifier);
    }
}

/// Network proxy configuration.
///
/// When set, these values override any `HTTP_PROXY` / `HTTPS_PROXY` environment
/// variables that may have been inherited from the shell.  Leave empty to rely
/// on the environment (the default).
///
/// # Example
/// ```toml
/// [network]
/// http_proxy  = "http://127.0.0.1:1086"
/// https_proxy = "http://127.0.0.1:1086"
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    /// HTTP proxy URL, e.g. `http://127.0.0.1:1086`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_proxy: Option<String>,
    /// HTTPS proxy URL, e.g. `http://127.0.0.1:1086`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub https_proxy: Option<String>,
}

impl NetworkConfig {
    /// Apply the configured proxy values to the current process environment so
    /// that all downstream `reqwest` clients (Rig, model registry, updater)
    /// pick them up automatically.
    ///
    /// - `Some(non_empty)` → sets the env var (overrides shell-inherited value).
    /// - `Some("")` / `None` → no-op (leave whatever the env has).
    ///
    /// # Safety
    ///
    /// Must be called from a single-threaded context (e.g. early in `main`)
    /// before any other threads are spawned, because `std::env::set_var` is
    /// not thread-safe.
    pub unsafe fn apply_to_env(&self) {
        unsafe {
            Self::apply_one("HTTP_PROXY", "http_proxy", self.http_proxy.as_deref());
            Self::apply_one("HTTPS_PROXY", "https_proxy", self.https_proxy.as_deref());
        }
    }

    /// # Safety
    /// Same as `apply_to_env` — must be called single-threaded.
    unsafe fn apply_one(upper: &str, lower: &str, value: Option<&str>) {
        if let Some(v) = value
            && !v.is_empty()
        {
            unsafe {
                std::env::set_var(upper, v);
                std::env::set_var(lower, v);
            }
            log::info!("network: {upper} set from config");
        }
        // None or empty string → leave the environment untouched.
    }
}

/// Experimental features guarded by individual switches.
///
/// # Example
/// ```toml
/// [experimental]
/// handoff = true
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExperimentalConfig {
    /// Agent Handoff: hand a running agent session off to another agent tab.
    /// Ships as an experimental feature; disable to hide all entry points.
    pub handoff: bool,
}

impl Default for ExperimentalConfig {
    fn default() -> Self {
        Self { handoff: true }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub terminal: TerminalConfig,
    pub appearance: AppearanceConfig,
    pub keybindings: KeybindingConfig,
    pub network: NetworkConfig,
    pub experimental: ExperimentalConfig,
    /// Authored source/provenance used for lossless, collision-safe saves.
    #[serde(skip)]
    source: std::sync::Mutex<ghostty::SourceState>,
}

impl Clone for Config {
    fn clone(&self) -> Self {
        Self {
            terminal: self.terminal.clone(),
            appearance: self.appearance.clone(),
            keybindings: self.keybindings.clone(),
            network: self.network.clone(),
            experimental: self.experimental.clone(),
            source: std::sync::Mutex::new(
                self.source
                    .lock()
                    .expect("config source mutex poisoned")
                    .clone(),
            ),
        }
    }
}

/// Configuration for skill discovery paths.
///
impl Config {
    pub fn normalize(&mut self) {
        self.terminal.shell = self
            .terminal
            .shell
            .take()
            .map(|shell| shell.trim().to_string())
            .filter(|shell| !shell.is_empty());
        self.terminal.font_family = sanitize_terminal_font_family(&self.terminal.font_family);
        self.terminal.font_fallback = sanitize_terminal_font_fallback(
            &self.terminal.font_fallback,
            &self.terminal.font_family,
        );
        self.appearance.normalize();
        self.keybindings.normalize();
    }

    pub fn load() -> Result<Self> {
        Self::load_from_paths(Self::config_path(), con_paths::legacy_config_file())
    }

    /// Pure load/parse entry point. It never performs credential discovery.
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !config_path_exists(path)? {
            let config = Self::default();
            config
                .source
                .lock()
                .expect("config source mutex poisoned")
                .path = Some(path.to_owned());
            return Ok(config);
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read configuration `{}`", path.display()))?;
        ghostty::parse(&content, Some(path.to_owned()))
    }

    /// Parse in-memory Ghostty syntax without resolving includes or auth state.
    pub fn parse_ghostty(source: &str) -> Result<Self> {
        ghostty::parse(source, None)
    }

    /// Load the primary file, or migrate an older sibling without modifying it.
    pub fn load_from_paths(path: impl AsRef<Path>, legacy: impl AsRef<Path>) -> Result<Self> {
        let (path, legacy) = (path.as_ref(), legacy.as_ref());
        if config_path_exists(path)? {
            return Self::load_from_path(path);
        }
        let previous = path.with_file_name("config.ghostty");
        if previous != path && config_path_exists(&previous)? {
            let content = std::fs::read_to_string(&previous)
                .with_context(|| format!("cannot read configuration `{}`", previous.display()))?;
            // The directory does not change, so relative includes and resources
            // keep their meaning. Preserve comments and native syntax verbatim.
            ghostty::parse(&content, Some(path.to_owned()))?;
            write_private_atomic_no_clobber(path, content.as_bytes())?;
            return Self::load_from_path(path);
        }
        if !config_path_exists(legacy)? {
            return Self::load_from_path(path);
        }
        let content = std::fs::read_to_string(legacy)
            .with_context(|| format!("cannot read configuration `{}`", legacy.display()))?;
        let document: toml::Value = toml::from_str(&content).map_err(|_| {
            anyhow::anyhow!("invalid legacy TOML configuration; original file was not modified")
        })?;
        let mut config: Config = document.try_into().map_err(|_| {
            anyhow::anyhow!(
                "invalid legacy configuration value type; original file was not modified"
            )
        })?;
        config.normalize();
        config
            .source
            .lock()
            .expect("config source mutex poisoned")
            .path = Some(path.to_owned());
        let rendered = ghostty::render(&config)?;
        // Validate the exact generated representation before publishing it.
        ghostty::parse(&rendered, Some(path.to_owned()))?;
        write_private_atomic_no_clobber(path, rendered.as_bytes())?;
        ghostty::parse(&rendered, Some(path.to_owned()))
    }

    pub fn native_entries(&self) -> Vec<ghostty::NativeEntry> {
        self.source
            .lock()
            .expect("config source mutex poisoned")
            .native
            .clone()
    }

    /// Original native Ghostty text with all `con.*` settings excluded.
    pub fn native_config_text(&self) -> Result<String> {
        Ok(ghostty::native_text_from_str(&ghostty::render(self)?))
    }

    pub fn base_path(&self) -> Option<PathBuf> {
        self.source
            .lock()
            .expect("config source mutex poisoned")
            .path
            .clone()
    }

    /// Explicit keys present in the authored document.
    pub fn authored_keys(&self) -> Vec<String> {
        let source = self.source.lock().expect("config source mutex poisoned");
        source
            .source
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                let (key, _) = line.split_once('=')?;
                Some(key.trim().to_owned())
            })
            .collect()
    }

    /// Keys whose typed values differ from the last loaded or saved snapshot.
    pub fn changed_keys(&self) -> Result<Vec<String>> {
        ghostty::changed_keys(self)
    }

    /// Zero-touch ChatGPT Subscription sign-in applied on every config load.
    ///
    /// 1. Sync the ChatGPT OAuth cache from Codex's `~/.codex/auth.json` (cheap
    ///    and idempotent — a fingerprint check short-circuits when nothing
    ///    changed), so a user already signed in to Codex never has to do a
    ///    manual device login.
    /// 2. Recompute an automatic provider choice from current credentials:
    ///    configured Anthropic wins; otherwise ChatGPT is used while an API-key
    ///    override or its OAuth cache is ready. Explicit choices are never
    ///    changed.
    ///
    /// Best-effort: failures are logged, never fatal. Conservative: a user who
    /// configured Anthropic or picked another provider is never redirected
    pub fn config_path() -> PathBuf {
        con_paths::config_file()
    }

    pub fn save(&self) -> Result<()> {
        self.save_to_path(Self::config_path())
    }

    pub fn save_to_path(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if path
            .extension()
            .is_some_and(|extension| extension == "toml")
        {
            anyhow::bail!(
                "TOML configuration is read-only; save as {}",
                con_paths::CONFIG_FILE_NAME
            );
        }
        let rendered = ghostty::render(self)?;
        // Validate before touching the user's file.
        let reparsed = ghostty::parse(&rendered, Some(path.to_owned()))?;
        {
            let source = self.source.lock().expect("config source mutex poisoned");
            if source.path.as_deref() == Some(path) {
                if path.exists() {
                    let disk = std::fs::read_to_string(path)?;
                    if disk != source.source && disk != rendered {
                        anyhow::bail!("configuration changed on disk; reload before saving");
                    }
                } else if !source.source.is_empty() {
                    anyhow::bail!("configuration was removed on disk; reload before saving");
                }
            } else if path.exists() {
                anyhow::bail!("destination already exists; load it before saving");
            }
        }
        write_private_atomic(path, rendered.as_bytes())?;
        let new_source = reparsed
            .source
            .lock()
            .expect("config source mutex poisoned")
            .clone();
        *self.source.lock().expect("config source mutex poisoned") = new_source;
        Ok(())
    }
}

// A dangling dotfile symlink is an existing configuration, not permission to
// fall back to defaults or a lower-priority file. Propagate lookup errors too.
fn config_path_exists(path: &Path) -> std::io::Result<bool> {
    match path.symlink_metadata() {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn write_private_atomic_no_clobber(path: &Path, content: &[u8]) -> Result<()> {
    if config_path_exists(path)? {
        anyhow::bail!("configuration appeared during migration");
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let tmp = path.with_extension(format!("tmp.{}.{}.migration", std::process::id(), unique));
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> Result<()> {
        let mut f = options.open(&tmp)?;
        f.write_all(content)?;
        f.sync_all()?;
        std::fs::hard_link(&tmp, path)?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&tmp);
    result
}

pub(crate) fn write_private_atomic(path: &Path, content: &[u8]) -> Result<()> {
    // Preserve dotfile-manager links. Renaming over a symlink would replace the
    // link itself, silently disconnecting the user's managed configuration.
    // Resolve only an existing symlink; ordinary paths keep their authored
    // location and dangling links remain errors.
    let destination = match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_symlink() => path
            .canonicalize()
            .with_context(|| format!("cannot resolve configuration link `{}`", path.display()))?,
        Ok(_) => path.to_owned(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => path.to_owned(),
        Err(error) => return Err(error.into()),
    };
    let path = destination.as_path();

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp_path = {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        path.with_extension(format!("tmp.{}.{}", std::process::id(), unique))
    };

    let write_result = (|| -> Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp_path)?;
        file.write_all(content)?;
        file.sync_all()?;
        Ok(())
    })();

    if let Err(err) = write_result {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }

    replace_file(&tmp_path, path)?;
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(tmp_path: &Path, path: &Path) -> Result<()> {
    std::fs::rename(tmp_path, path)?;
    Ok(())
}

#[cfg(windows)]
fn replace_file(tmp_path: &Path, path: &Path) -> Result<()> {
    if !path.exists() {
        std::fs::rename(tmp_path, path)?;
        return Ok(());
    }

    let backup_path = {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        path.with_extension(format!("bak.{}.{}", std::process::id(), unique))
    };

    std::fs::rename(path, &backup_path)?;
    match std::fs::rename(tmp_path, path) {
        Ok(()) => {
            let _ = std::fs::remove_file(&backup_path);
            Ok(())
        }
        Err(err) => {
            let _ = std::fs::rename(&backup_path, path);
            Err(err.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        APP_ICONS, Config, DEFAULT_APP_ICON, DEFAULT_APP_ICON_ASSET, DEFAULT_TERMINAL_FONT_FAMILY,
        NetworkConfig, TabsOrientation, app_icon_asset, sanitize_terminal_font_fallback,
        sanitize_terminal_font_family,
    };

    #[cfg(unix)]
    #[test]
    fn dangling_config_links_never_trigger_defaults_or_legacy_fallback() {
        for name in ["config.ghostty", con_paths::CONFIG_FILE_NAME, "config.toml"] {
            let root = std::env::temp_dir().join(format!("con-config-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&root).unwrap();
            let target = root.join(con_paths::CONFIG_FILE_NAME);
            let legacy = root.join("config.toml");
            let link = root.join(name);
            let dotfile = root.join("dotfile");
            if name != "config.toml" {
                std::fs::write(&legacy, "[terminal]\nfont_size = 11\n").unwrap();
            }
            std::os::unix::fs::symlink(&dotfile, &link).unwrap();
            assert!(Config::load_from_paths(&target, &legacy).is_err(), "{name}");
            assert!(!target.exists(), "must not publish a fallback for {name}");
            if name == con_paths::CONFIG_FILE_NAME {
                assert!(Config::load_from_path(&target).is_err());
            }
            // Valid dotfile symlinks still load and migrate normally.
            std::fs::write(
                &dotfile,
                if name == "config.toml" {
                    "[terminal]\nfont_size = 19\n"
                } else {
                    "font-size = 19\n"
                },
            )
            .unwrap();
            assert_eq!(
                Config::load_from_paths(&target, &legacy)
                    .unwrap()
                    .terminal
                    .font_size,
                19.0
            );
            assert!(
                std::fs::symlink_metadata(&link)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn saving_through_a_config_link_preserves_the_link() {
        let root = std::env::temp_dir().join(format!("con-config-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let dotfile = root.join("managed.conf");
        let link = root.join(con_paths::CONFIG_FILE_NAME);
        std::fs::write(&dotfile, "font-size = 12\n").unwrap();
        std::os::unix::fs::symlink(&dotfile, &link).unwrap();

        let mut config = Config::load_from_path(&link).unwrap();
        config.terminal.font_size = 18.0;
        config.save_to_path(&link).unwrap();

        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(
            std::fs::read_to_string(&dotfile)
                .unwrap()
                .contains("font-size = 18")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migration_preserves_native_bytes_and_prefers_native_over_toml() {
        let root = std::env::temp_dir().join(format!("con-config-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let target = root.join(con_paths::CONFIG_FILE_NAME);
        let previous = root.join("config.ghostty");
        let toml = root.join("config.toml");
        let source = "# keep comment\nfont-size = 19\nconfig-file = colors.conf\ncon.appearance.ui_opacity = 0.8\n";
        std::fs::write(&previous, source).unwrap();
        std::fs::write(&toml, "[terminal]\nfont_size = 11\n").unwrap();
        std::fs::write(root.join("colors.conf"), "background = 123456\n").unwrap();
        let config = Config::load_from_paths(&target, &toml).unwrap();
        assert_eq!(config.terminal.font_size, 19.0);
        assert_eq!(config.source.lock().unwrap().path.as_ref(), Some(&target));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), source);
        assert_eq!(std::fs::read_to_string(&previous).unwrap(), source);
        assert!(
            config
                .native_entries()
                .iter()
                .any(|entry| entry.key == "config-file" && entry.value == "colors.conf")
        );
        std::fs::write(&target, "font-size = 23\n").unwrap();
        assert_eq!(
            Config::load_from_paths(&target, &toml)
                .unwrap()
                .terminal
                .font_size,
            23.0
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_higher_priority_config_never_falls_back() {
        let root = std::env::temp_dir().join(format!("con-config-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let target = root.join(con_paths::CONFIG_FILE_NAME);
        let previous = root.join("config.ghostty");
        let toml = root.join("config.toml");
        std::fs::write(&toml, "[terminal]\nfont_size = 11\n").unwrap();
        std::fs::write(&previous, "con.terminal.font_size = invalid\n").unwrap();
        assert!(Config::load_from_paths(&target, &toml).is_err());
        assert!(!target.exists());
        std::fs::write(&previous, "font-size = 19\n").unwrap();
        std::fs::write(&target, "con.terminal.font_size = invalid\n").unwrap();
        assert!(Config::load_from_paths(&target, &toml).is_err());
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "con.terminal.font_size = invalid\n"
        );
        std::fs::remove_file(&target).unwrap();
        std::fs::remove_file(&previous).unwrap();
        assert_eq!(
            Config::load_from_paths(&target, &toml)
                .unwrap()
                .terminal
                .font_size,
            11.0
        );
        assert!(toml.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clipboard_writes_default_to_allowed_but_preserve_explicit_policy() {
        assert!(Config::default().terminal.clipboard_write);
        for source in ["", "[terminal]\nfont_size = 16.0"] {
            let config: Config = toml::from_str(source).unwrap();
            assert!(config.terminal.clipboard_write);
        }
        for enabled in [false, true] {
            let source = format!("[terminal]\nclipboard_write = {enabled}");
            let config: Config = toml::from_str(&source).unwrap();
            assert_eq!(config.terminal.clipboard_write, enabled);
            let encoded = toml::to_string(&config).unwrap();
            let decoded: Config = toml::from_str(&encoded).unwrap();
            assert_eq!(decoded.terminal.clipboard_write, enabled);
        }
    }

    #[test]
    fn terminal_font_sanitizer_rejects_gpui_pseudo_families() {
        assert_eq!(
            sanitize_terminal_font_family(".ZedMono"),
            DEFAULT_TERMINAL_FONT_FAMILY
        );
        assert_eq!(
            sanitize_terminal_font_family(" .SystemUIFont "),
            DEFAULT_TERMINAL_FONT_FAMILY
        );
        assert_eq!(
            sanitize_terminal_font_family("JetBrains Mono"),
            "JetBrains Mono"
        );
    }

    #[test]
    fn terminal_shell_trims_and_empty_values_restore_platform_default() {
        let mut configured: Config = toml::from_str(
            r#"
[terminal]
shell = "  pwsh.exe -NoLogo  "
"#,
        )
        .unwrap();
        configured.normalize();
        assert_eq!(
            configured.terminal.shell.as_deref(),
            Some("pwsh.exe -NoLogo")
        );

        configured.terminal.shell = Some("   ".to_string());
        configured.normalize();
        assert!(configured.terminal.shell.is_none());
    }

    #[test]
    fn terminal_font_fallback_sanitizer_preserves_order_and_removes_invalid_entries() {
        let fonts = vec![
            " Sarasa Mono SC ".to_string(),
            "jetbrains mono".to_string(),
            ".SystemUIFont".to_string(),
            "IoskeleyMono".to_string(),
            "SARASA MONO SC".to_string(),
            "Segoe UI Emoji".to_string(),
        ];
        assert_eq!(
            sanitize_terminal_font_fallback(&fonts, "JetBrains Mono"),
            vec!["Sarasa Mono SC", "Segoe UI Emoji"]
        );
    }

    #[test]
    fn legacy_terminal_config_defaults_to_no_preferred_fallbacks() {
        let config: Config = toml::from_str(
            r#"
[terminal]
font_family = "JetBrains Mono"
font_size = 15.0
"#,
        )
        .unwrap();
        assert!(config.terminal.font_fallback.is_empty());
    }

    #[test]
    fn terminal_font_fallback_round_trips_in_order_and_normalizes() {
        let mut config: Config = toml::from_str(
            r#"
[terminal]
font_family = " JetBrains Mono "
font_fallback = [" Sarasa Mono SC ", "Segoe UI Emoji", "sarasa mono sc", ".SystemUIFont"]
"#,
        )
        .unwrap();
        config.normalize();
        assert_eq!(config.terminal.font_family, "JetBrains Mono");
        assert_eq!(
            config.terminal.font_fallback,
            vec!["Sarasa Mono SC", "Segoe UI Emoji"]
        );

        let encoded = toml::to_string(&config).unwrap();
        let decoded: Config = toml::from_str(&encoded).unwrap();
        assert_eq!(
            decoded.terminal.font_fallback,
            config.terminal.font_fallback
        );
    }

    #[test]
    fn new_configs_enable_restore_terminal_text_by_default() {
        assert!(Config::default().appearance.restore_terminal_text);
    }

    #[test]
    fn loaded_legacy_configs_inherit_restore_terminal_text_default() {
        let content = r#"
[appearance]
terminal_opacity = 0.8
"#;
        let config: Config = toml::from_str(content).unwrap();

        assert!(config.appearance.restore_terminal_text);
    }

    #[test]
    fn loaded_configs_preserve_explicit_restore_terminal_text() {
        let content = r#"
[appearance]
restore_terminal_text = false
"#;
        let config: Config = toml::from_str(content).unwrap();

        assert!(!config.appearance.restore_terminal_text);
    }

    #[test]
    fn default_keybindings_include_quick_terminal_fields() {
        let config = Config::default();
        assert!(!config.keybindings.quick_terminal_enabled);
        assert_eq!(config.keybindings.quick_terminal, "cmd-\\");
    }

    #[test]
    fn default_keybindings_include_file_sidebar_shortcuts() {
        let config = Config::default();
        let expected_focus = if cfg!(target_os = "macos") {
            "secondary-alt-e"
        } else {
            "secondary-shift-e"
        };

        assert_eq!(config.keybindings.focus_files, expected_focus);
        assert_eq!(config.keybindings.search_files, "secondary-shift-f");

        let shortcuts = config.keybindings.active_shortcuts();
        assert!(shortcuts.contains(&("聚焦文件", expected_focus)));
        assert!(shortcuts.contains(&("搜索文件", "secondary-shift-f")));
    }

    #[test]
    fn default_keybindings_enable_terminal_find_on_supported_platforms() {
        let config = Config::default();
        let shortcuts = config.keybindings.active_shortcuts();

        if cfg!(any(
            target_os = "macos",
            target_os = "linux",
            target_os = "windows"
        )) {
            assert_eq!(config.keybindings.find_in_terminal, "secondary-f");
            assert!(shortcuts.contains(&("在终端中查找", "secondary-f")));
        } else {
            assert!(config.keybindings.find_in_terminal.is_empty());
            assert!(!shortcuts.iter().any(|(label, _)| *label == "在终端中查找"));
        }
    }

    #[test]
    fn default_keybindings_include_reachable_surface_cycle_shortcuts() {
        let config = Config::default();
        let (expected_next, expected_previous) = if cfg!(target_os = "macos") {
            ("secondary-ctrl-]", "secondary-ctrl-[")
        } else {
            ("alt-shift-]", "alt-shift-[")
        };

        assert_eq!(config.keybindings.next_surface, expected_next);
        assert_eq!(config.keybindings.previous_surface, expected_previous);

        let shortcuts = config.keybindings.active_shortcuts();
        assert!(shortcuts.contains(&("下一个 Surface 标签", expected_next)));
        assert!(shortcuts.contains(&("上一个 Surface 标签", expected_previous)));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_normalize_migrates_broken_option_bracket_surface_defaults() {
        let mut config = Config::default();
        config.keybindings.next_surface = "secondary-alt-]".to_string();
        config.keybindings.previous_surface = "secondary-alt-[".to_string();

        config.normalize();

        assert_eq!(config.keybindings.next_surface, "secondary-ctrl-]");
        assert_eq!(config.keybindings.previous_surface, "secondary-ctrl-[");
    }

    #[test]
    fn serialized_config_omits_removed_quick_terminal_always_on_top_field() {
        let config = Config::default();
        let serialized = toml::to_string(&config).unwrap();
        assert!(!serialized.contains("quick_terminal_always_on_top"));
    }

    #[test]
    fn legacy_configs_receive_quick_terminal_defaults() {
        let content = r#"
[keybindings]
global_summon_enabled = true
global_summon = "alt-space"
"#;
        let config: Config = toml::from_str(content).unwrap();

        assert!(!config.keybindings.quick_terminal_enabled);
        assert_eq!(config.keybindings.quick_terminal, "cmd-\\");
    }

    #[test]
    fn legacy_configs_receive_file_sidebar_shortcut_defaults() {
        let content = r#"
[keybindings]
command_palette = "secondary-shift-p"
"#;
        let config: Config = toml::from_str(content).unwrap();
        let expected_focus = if cfg!(target_os = "macos") {
            "secondary-alt-e"
        } else {
            "secondary-shift-e"
        };

        assert_eq!(config.keybindings.focus_files, expected_focus);
        assert_eq!(config.keybindings.search_files, "secondary-shift-f");
        assert_eq!(
            config.keybindings.find_in_terminal,
            if cfg!(any(
                target_os = "macos",
                target_os = "linux",
                target_os = "windows"
            )) {
                "secondary-f"
            } else {
                ""
            }
        );
    }

    #[test]
    fn loaded_configs_preserve_explicit_file_sidebar_shortcuts() {
        let content = r#"
[keybindings]
focus_files = "alt-e"
search_files = "alt-f"
"#;
        let config: Config = toml::from_str(content).unwrap();

        assert_eq!(config.keybindings.focus_files, "alt-e");
        assert_eq!(config.keybindings.search_files, "alt-f");

        let shortcuts = config.keybindings.active_shortcuts();
        assert!(shortcuts.contains(&("聚焦文件", "alt-e")));
        assert!(shortcuts.contains(&("搜索文件", "alt-f")));
    }

    #[test]
    fn omitted_tabs_orientation_defaults_to_vertical() {
        let content = r#"
[appearance]
terminal_opacity = 0.8
"#;
        let config: Config = toml::from_str(content).unwrap();

        assert_eq!(
            config.appearance.tabs_orientation,
            TabsOrientation::Vertical
        );
        assert_eq!(
            Config::default().appearance.tabs_orientation,
            TabsOrientation::Vertical
        );
    }

    #[test]
    fn tabs_orientation_config_is_preserved() {
        let content = r#"
[appearance]
tabs_orientation = "vertical"

[keybindings]
toggle_vertical_tabs = "secondary-b"
"#;
        let config: Config = toml::from_str(content).unwrap();

        assert_eq!(
            config.appearance.tabs_orientation,
            TabsOrientation::Vertical
        );
        assert_eq!(config.keybindings.toggle_left_panel, "secondary-b");

        let serialized = toml::to_string(&config).unwrap();
        assert!(serialized.contains("tabs_orientation = \"vertical\""));
        assert!(!serialized.contains("toggle_vertical_tabs"));
        assert!(serialized.contains("toggle_left_panel"));
    }

    #[test]
    fn explicit_horizontal_tabs_orientation_is_preserved() {
        let content = r#"
[appearance]
tabs_orientation = "horizontal"
"#;
        let config: Config = toml::from_str(content).unwrap();

        assert_eq!(
            config.appearance.tabs_orientation,
            TabsOrientation::Horizontal
        );
    }

    #[test]
    fn close_to_quit_defaults_to_true() {
        let config = Config::default();
        assert!(config.appearance.close_to_quit);
    }

    #[test]
    fn omitted_close_to_quit_defaults_to_true() {
        let content = r#"
[appearance]
terminal_opacity = 0.8
"#;
        let config: Config = toml::from_str(content).unwrap();
        assert!(config.appearance.close_to_quit);
    }

    #[test]
    fn close_to_quit_is_preserved_when_set_false() {
        let content = r#"
[appearance]
close_to_quit = false
"#;
        let config: Config = toml::from_str(content).unwrap();
        assert!(!config.appearance.close_to_quit);

        // Round-trip through serialization.
        let serialized = toml::to_string(&config).unwrap();
        assert!(serialized.contains("close_to_quit = false"));
        let reparsed: Config = toml::from_str(&serialized).unwrap();
        assert!(!reparsed.appearance.close_to_quit);
    }

    #[test]
    fn new_configs_use_tail_a6_app_icon_by_default() {
        assert_eq!(Config::default().appearance.app_icon, DEFAULT_APP_ICON);
        assert_eq!(DEFAULT_APP_ICON, "raccoon-tail-a6");
    }

    #[test]
    fn loaded_legacy_configs_inherit_tail_a6_app_icon_default() {
        let content = r#"
[appearance]
terminal_opacity = 0.8
"#;
        let mut config: Config = toml::from_str(content).unwrap();
        config.normalize();

        assert_eq!(config.appearance.app_icon, DEFAULT_APP_ICON);
    }

    #[test]
    fn loaded_configs_preserve_explicit_app_icon() {
        let content = r#"
[appearance]
app_icon = "raccoon-suit-a1"
"#;
        let mut config: Config = toml::from_str(content).unwrap();
        config.normalize();

        assert_eq!(config.appearance.app_icon, "raccoon-suit-a1");
    }

    #[test]
    fn normalize_falls_back_to_tail_a6_app_icon_for_unknown_ids() {
        let mut config = Config::default();
        config.appearance.app_icon = "not-a-real-raccoon".into();
        config.normalize();

        assert_eq!(config.appearance.app_icon, DEFAULT_APP_ICON);
    }

    #[test]
    fn app_icon_catalog_covers_every_choice_asset_path() {
        assert!(APP_ICONS.iter().any(|icon| icon.id == DEFAULT_APP_ICON));
        assert_eq!(APP_ICONS.len(), 19);
        for icon in APP_ICONS {
            assert!(!icon.asset.is_empty());
            assert_eq!(app_icon_asset(icon.id), icon.asset);
        }
        assert_eq!(app_icon_asset("missing"), DEFAULT_APP_ICON_ASSET);
    }

    #[test]
    fn explicit_classic_choice_keeps_its_original_artwork() {
        let mut config: Config = toml::from_str("[appearance]\napp_icon = \"default\"\n").unwrap();
        config.normalize();
        assert_eq!(config.appearance.app_icon, "default");
        assert_eq!(app_icon_asset("default"), "Con-macOS-Dark-256x256@2x.png");
        assert_ne!(app_icon_asset("default"), DEFAULT_APP_ICON_ASSET);
    }

    #[test]
    fn loaded_configs_preserve_explicit_quick_terminal_fields() {
        let content = r#"
[keybindings]
quick_terminal_enabled = true
quick_terminal = "cmd-\\"
"#;
        let config: Config = toml::from_str(content).unwrap();

        assert!(config.keybindings.quick_terminal_enabled);
        assert_eq!(config.keybindings.quick_terminal, "cmd-\\");
    }

    #[test]
    fn keybinding_conflicts_detect_duplicate_configured_shortcuts() {
        let mut config = Config::default();
        config.keybindings.command_palette = "ctrl-shift-p".to_string();
        config.keybindings.new_tab = "control-shift-p".to_string();

        let conflicts = config.keybindings.shortcut_conflicts(&[]);

        assert_eq!(conflicts.len(), 1);
        assert_eq!(
            conflicts[0].actions,
            vec!["新建标签".to_string(), "命令面板".to_string()]
        );
    }

    #[test]
    fn keybinding_conflicts_match_secondary_to_platform_modifier() {
        let mut config = Config::default();
        config.keybindings.command_palette = "secondary-shift-p".to_string();
        config.keybindings.new_tab = if cfg!(target_os = "macos") {
            "cmd-shift-p".to_string()
        } else {
            "ctrl-shift-p".to_string()
        };

        let conflicts = config.keybindings.shortcut_conflicts(&[]);

        assert_eq!(conflicts.len(), 1);
        assert_eq!(
            conflicts[0].actions,
            vec!["新建标签".to_string(), "命令面板".to_string()]
        );
    }

    #[test]
    fn keybinding_conflicts_include_minus_key_shortcuts() {
        let mut config = Config::default();
        config.keybindings.command_palette = "ctrl--".to_string();
        config.keybindings.new_tab = "control--".to_string();

        let conflicts = config.keybindings.shortcut_conflicts(&[]);

        assert_eq!(conflicts.len(), 1);
        assert_eq!(
            conflicts[0].actions,
            vec!["新建标签".to_string(), "命令面板".to_string()]
        );
    }

    #[test]
    fn keybinding_conflicts_ignore_disabled_global_shortcuts() {
        let mut config = Config::default();
        config.keybindings.global_summon_enabled = false;
        config.keybindings.global_summon = config.keybindings.command_palette.clone();

        assert!(config.keybindings.shortcut_conflicts(&[]).is_empty());
    }

    #[test]
    fn keybinding_conflicts_include_reserved_shortcuts() {
        let mut config = Config::default();
        config.keybindings.command_palette = if cfg!(target_os = "macos") {
            "cmd-m".to_string()
        } else {
            "ctrl-m".to_string()
        };

        let conflicts = config
            .keybindings
            .shortcut_conflicts(&[("最小化窗口", "secondary-m")]);

        assert_eq!(conflicts.len(), 1);
        assert_eq!(
            conflicts[0].actions,
            vec!["命令面板".to_string(), "最小化窗口".to_string()]
        );
    }

    #[test]
    fn network_config_defaults_to_empty() {
        let config = NetworkConfig::default();
        assert!(config.http_proxy.is_none());
        assert!(config.https_proxy.is_none());
    }

    #[test]
    fn network_config_deserializes_from_toml() {
        let content = r#"
[network]
http_proxy  = "http://127.0.0.1:1086"
https_proxy = "http://127.0.0.1:1086"
"#;
        let config: Config = toml::from_str(content).unwrap();
        assert_eq!(
            config.network.http_proxy.as_deref(),
            Some("http://127.0.0.1:1086")
        );
        assert_eq!(
            config.network.https_proxy.as_deref(),
            Some("http://127.0.0.1:1086")
        );
    }

    #[test]
    fn network_config_missing_section_is_default() {
        let content = r#"
[terminal]
font_size = 14.0
"#;
        let config: Config = toml::from_str(content).unwrap();
        assert!(config.network.http_proxy.is_none());
        assert!(config.network.https_proxy.is_none());
    }

    #[test]
    fn network_config_partial_fields() {
        let content = r#"
[network]
http_proxy = "http://proxy.example.com:8080"
"#;
        let config: Config = toml::from_str(content).unwrap();
        assert_eq!(
            config.network.http_proxy.as_deref(),
            Some("http://proxy.example.com:8080")
        );
        assert!(config.network.https_proxy.is_none());
    }

    #[test]
    fn network_config_round_trips_through_serialization() {
        let original = NetworkConfig {
            http_proxy: Some("http://127.0.0.1:1086".to_string()),
            https_proxy: Some("http://127.0.0.1:1087".to_string()),
        };
        let serialized = toml::to_string(&original).unwrap();
        let deserialized: NetworkConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(original.http_proxy, deserialized.http_proxy);
        assert_eq!(original.https_proxy, deserialized.https_proxy);
    }

    #[test]
    fn network_config_empty_strings_are_preserved() {
        // Empty strings are valid TOML values and deserialize to Some("").
        // apply_to_env treats them as no-op (same as None).
        let content = r#"
[network]
http_proxy  = ""
https_proxy = ""
"#;
        let config: Config = toml::from_str(content).unwrap();
        assert_eq!(config.network.http_proxy.as_deref(), Some(""));
        assert_eq!(config.network.https_proxy.as_deref(), Some(""));
    }
}
