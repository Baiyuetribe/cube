use con_core::{
    Config,
    config::{
        APP_ICON_GROUPS, APP_ICONS, AppearanceConfig, DEFAULT_TERMINAL_FONT_FAMILY,
        MAX_UI_FONT_SIZE, MIN_UI_FONT_SIZE, TabsOrientation, is_bundled_terminal_font_family,
        is_gpui_pseudo_font_family, sanitize_terminal_font_fallback, sanitize_terminal_font_family,
    },
};
use gpui::prelude::FluentBuilder;
use gpui::*;

use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_component::input::InputState;
use gpui_component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_component::slider::{Slider, SliderEvent, SliderState};
use gpui_component::switch::Switch;
use gpui_component::{ActiveTheme, Disableable, Icon, IndexPath, Sizable as _, input::Input};

use crate::motion::{MotionValue, vertical_reveal_offset};
use crate::ui_scale::{ui_density_scale, ui_icon_px, ui_px};
use url::Url;

mod configuration;
use configuration::{ConfigurationImport, ConfigurationImportStatus};
mod window_chrome;
use window_chrome::SETTINGS_HEADER_HEIGHT;
pub(crate) use window_chrome::settings_titlebar_options;
#[cfg(target_os = "macos")]
pub(crate) use window_chrome::traffic_light_position;

actions!(settings, [ToggleSettings, SaveSettings, DismissSettings]);

/// Emitted when the user selects a different terminal theme for live preview.
pub struct ThemePreview(pub String);

/// Emitted for lightweight appearance changes that should be visible
/// immediately but should not persist/rebuild the full agent config.
pub struct AppearancePreview;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponsiveMode {
    Mobile,
    Narrow,
    Compact,
    Regular,
}

impl ResponsiveMode {
    fn from_width(width: f32) -> Self {
        if width < 600.0 {
            Self::Mobile
        } else if width < 840.0 {
            Self::Narrow
        } else if width < 980.0 {
            Self::Compact
        } else {
            Self::Regular
        }
    }

    fn is_mobile(self) -> bool {
        matches!(self, Self::Mobile)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsSection {
    General,
    Appearance,
    Keys,
    Configuration,
}

impl SettingsSection {
    fn label(&self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Keys => "Keys",
            Self::Configuration => "Config",
        }
    }

    fn icon(&self) -> &'static str {
        match self {
            Self::General => "phosphor/sliders.svg",
            Self::Appearance => "phosphor/sun.svg",
            Self::Keys => "phosphor/keyboard.svg",
            Self::Configuration => "phosphor/file-text.svg",
        }
    }
}

const ALL_SECTIONS: &[SettingsSection] = &[
    SettingsSection::General,
    SettingsSection::Appearance,
    SettingsSection::Keys,
    SettingsSection::Configuration,
];

pub struct SettingsPanel {
    visible: bool,
    standalone: bool,
    config: Config,
    preview_snapshot: Option<Config>,
    icon_preview_owner: u64,
    focus_handle: FocusHandle,
    active_section: SettingsSection,
    overlay_motion: MotionValue,

    terminal_font_select: Entity<SelectState<SearchableVec<String>>>,
    shell_input: Entity<InputState>,
    terminal_fallback_select: Entity<SelectState<SearchableVec<String>>>,
    terminal_font_families: Vec<String>,
    ui_font_select: Entity<SelectState<SearchableVec<String>>>,
    cursor_style_select: Entity<SelectState<Vec<String>>>,
    font_size_input: Entity<InputState>,
    ui_font_size_input: Entity<InputState>,
    terminal_opacity_slider: Entity<SliderState>,
    terminal_blur: bool,
    ui_opacity_slider: Entity<SliderState>,
    tab_accent_inactive_alpha_slider: Entity<SliderState>,
    tab_accent_inactive_hover_alpha_slider: Entity<SliderState>,
    background_image_input: Entity<InputState>,
    background_image_opacity_slider: Entity<SliderState>,
    background_image_position_select: Entity<SelectState<Vec<String>>>,
    background_image_fit_select: Entity<SelectState<Vec<String>>>,
    background_image_repeat: bool,
    hide_pane_title_bar: bool,
    close_to_quit: bool,
    save_error: Option<String>,
    save_error_kind: Option<SettingsSaveErrorKind>,
    last_saved_at: Option<std::time::SystemTime>,
    close_confirmation_visible: bool,

    configuration_sources: Option<Vec<std::path::PathBuf>>,
    configuration_import: ConfigurationImport,

    // Theme import
    custom_theme_name_input: Entity<InputState>,
    custom_theme_preview: Option<con_terminal::TerminalTheme>,
    custom_theme_status: Option<String>,

    // Keybindings — which binding is being recorded (field name, e.g. "new_tab")
    recording_key: Option<String>,
    #[cfg(target_os = "macos")]
    recording_resume_keybindings: Option<con_core::config::KeybindingConfig>,

    // Network / proxy
    http_proxy_input: Entity<InputState>,
    https_proxy_input: Entity<InputState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsSaveErrorKind {
    KeybindingConflict,
    Other,
}

const BACKGROUND_IMAGE_POSITIONS: &[&str] = &[
    "top-left",
    "top-center",
    "top-right",
    "center-left",
    "center",
    "center-right",
    "bottom-left",
    "bottom-center",
    "bottom-right",
];

const BACKGROUND_IMAGE_FITS: &[&str] = &["contain", "cover", "stretch", "none"];
impl SettingsPanel {
    fn clamp_ui_font_size(value: f32) -> f32 {
        value.clamp(MIN_UI_FONT_SIZE, MAX_UI_FONT_SIZE)
    }

    fn clamp_terminal_opacity(value: f32) -> f32 {
        value.clamp(0.25, 1.0)
    }

    fn clamp_ui_opacity(value: f32) -> f32 {
        value.clamp(0.35, 1.0)
    }

    fn terminal_blur_supported() -> bool {
        !cfg!(target_os = "linux")
    }

    fn effective_terminal_blur(value: bool) -> bool {
        value && Self::terminal_blur_supported()
    }

    fn terminal_opacity_value(&self) -> f32 {
        Self::clamp_terminal_opacity(self.config.appearance.terminal_opacity)
    }

    fn ui_opacity_value(&self) -> f32 {
        Self::clamp_ui_opacity(self.config.appearance.ui_opacity)
    }

    fn clamp_tab_accent_inactive_alpha(value: f32) -> f32 {
        if value.is_finite() {
            value.clamp(
                AppearanceConfig::MIN_TAB_ACCENT_ALPHA,
                AppearanceConfig::MAX_TAB_ACCENT_INACTIVE_ALPHA,
            )
        } else {
            crate::tab_colors::TAB_ACCENT_INACTIVE_ALPHA
        }
    }

    fn clamp_tab_accent_inactive_hover_alpha(value: f32, inactive: f32) -> f32 {
        let value = if value.is_finite() {
            value.clamp(
                AppearanceConfig::MIN_TAB_ACCENT_ALPHA,
                AppearanceConfig::MAX_TAB_ACCENT_INACTIVE_HOVER_ALPHA,
            )
        } else {
            crate::tab_colors::TAB_ACCENT_INACTIVE_HOVER_ALPHA
        };
        value.max(inactive)
    }

    fn tab_accent_inactive_alpha_value(&self) -> f32 {
        Self::clamp_tab_accent_inactive_alpha(self.config.appearance.tab_accent_inactive_alpha)
    }

    fn tab_accent_inactive_hover_alpha_value(&self) -> f32 {
        Self::clamp_tab_accent_inactive_hover_alpha(
            self.config.appearance.tab_accent_inactive_hover_alpha,
            self.tab_accent_inactive_alpha_value(),
        )
    }

    fn clamp_background_image_opacity(value: f32) -> f32 {
        value.clamp(0.0, 1.0)
    }

    fn background_image_opacity_value(&self) -> f32 {
        Self::clamp_background_image_opacity(self.config.appearance.background_image_opacity)
    }

    fn make_string_select(
        options: &[&str],
        current_value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SelectState<Vec<String>>> {
        let items: Vec<String> = options.iter().map(|value| (*value).to_string()).collect();
        let selected_index = items
            .iter()
            .position(|item| item == current_value)
            .map(IndexPath::new);
        cx.new(|cx| SelectState::new(items, selected_index, window, cx))
    }

    fn cursor_style_label(value: &str) -> &'static str {
        match value.trim().to_ascii_lowercase().as_str() {
            "block" => "Block",
            "underline" => "Underline",
            "block_hollow" | "block-hollow" | "hollow" => "Hollow Block",
            _ => "Bar",
        }
    }

    fn cursor_style_from_label(label: &str) -> &'static str {
        match label {
            "Block" => "block",
            "Underline" => "underline",
            "Hollow Block" => "block_hollow",
            _ => "bar",
        }
    }

    fn prepare_terminal_font_families(
        config: &Config,
        mut font_families: Vec<String>,
    ) -> Vec<String> {
        font_families.sort_by_key(|name| name.to_lowercase());
        font_families.dedup();

        let mut preferred = Vec::new();
        let sanitized_terminal_family = sanitize_terminal_font_family(&config.terminal.font_family);
        for family in [
            DEFAULT_TERMINAL_FONT_FAMILY,
            sanitized_terminal_family.as_str(),
        ]
        .into_iter()
        .chain(config.terminal.font_fallback.iter().map(String::as_str))
        {
            if !family.is_empty() && !preferred.iter().any(|existing| existing == family) {
                preferred.push(family.to_string());
            }
        }

        for family in font_families {
            if !is_gpui_pseudo_font_family(&family)
                && !preferred.iter().any(|existing| existing == &family)
            {
                preferred.push(family);
            }
        }
        preferred
    }

    fn prepare_ui_font_families(config: &Config, mut font_families: Vec<String>) -> Vec<String> {
        font_families.sort_by_key(|name| name.to_lowercase());
        font_families.dedup();

        let mut preferred = Vec::new();
        for family in [
            ".SystemUIFont",
            config.appearance.ui_font_family.as_str(),
            DEFAULT_TERMINAL_FONT_FAMILY,
        ] {
            if !family.is_empty() && !preferred.iter().any(|existing| existing == family) {
                preferred.push(family.to_string());
            }
        }

        for family in font_families {
            if !preferred.iter().any(|existing| existing == &family) {
                preferred.push(family);
            }
        }
        preferred
    }

    fn make_searchable_string_select(
        options: &[String],
        current_value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SelectState<SearchableVec<String>>> {
        let items = SearchableVec::new(options.to_vec());
        let selected_index = options
            .iter()
            .position(|item| item == current_value)
            .map(IndexPath::new);
        cx.new(|cx| SelectState::new(items, selected_index, window, cx).searchable(true))
    }

    fn sync_terminal_fallback_select(&self, window: &mut Window, cx: &mut Context<Self>) {
        let candidates = self
            .terminal_font_families
            .iter()
            .filter(|family| {
                !family.eq_ignore_ascii_case(&self.config.terminal.font_family)
                    && !is_bundled_terminal_font_family(family)
                    && !self
                        .config
                        .terminal
                        .font_fallback
                        .iter()
                        .any(|fallback| fallback.eq_ignore_ascii_case(family))
            })
            .cloned()
            .collect::<Vec<_>>();
        self.terminal_fallback_select.update(cx, |select, cx| {
            select.set_items(SearchableVec::new(candidates), window, cx);
            select.set_selected_index(None, window, cx);
        });
    }

    fn card_opacity(&self) -> f32 {
        0.74
    }

    pub fn new(config: &Config, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe_global::<ConfigurationImportStatus>(|_, cx| cx.notify())
            .detach();
        let mut config = config.clone();
        config.appearance.terminal_blur =
            Self::effective_terminal_blur(config.appearance.terminal_blur);
        let all_font_families = cx.text_system().all_font_names();
        let terminal_font_families =
            Self::prepare_terminal_font_families(&config, all_font_families.clone());
        let ui_font_families = Self::prepare_ui_font_families(&config, all_font_families);
        let terminal_font_family = sanitize_terminal_font_family(&config.terminal.font_family);
        let terminal_font_select = Self::make_searchable_string_select(
            &terminal_font_families,
            &terminal_font_family,
            window,
            cx,
        );
        let fallback_candidates = terminal_font_families
            .iter()
            .filter(|family| {
                !family.eq_ignore_ascii_case(&terminal_font_family)
                    && !is_bundled_terminal_font_family(family)
                    && !config
                        .terminal
                        .font_fallback
                        .iter()
                        .any(|fallback| fallback.eq_ignore_ascii_case(family))
            })
            .cloned()
            .collect::<Vec<_>>();
        let terminal_fallback_select =
            Self::make_searchable_string_select(&fallback_candidates, "", window, cx);
        let ui_font_select = Self::make_searchable_string_select(
            &ui_font_families,
            &config.appearance.ui_font_family,
            window,
            cx,
        );
        let cursor_style_select = Self::make_string_select(
            &["Bar", "Block", "Underline", "Hollow Block"],
            Self::cursor_style_label(&config.terminal.cursor_style),
            window,
            cx,
        );
        let shell_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_placeholder(
                if cfg!(target_os = "windows") {
                    "Auto (Windows Terminal default)"
                } else {
                    "Auto ($SHELL)"
                },
                window,
                cx,
            );
            s.set_value(
                config.terminal.shell.clone().unwrap_or_default(),
                window,
                cx,
            );
            s
        });
        let font_size_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_placeholder("14.0", window, cx);
            s.set_value(config.terminal.font_size.to_string(), window, cx);
            s
        });
        let ui_font_size_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_placeholder("16.0", window, cx);
            s.set_value(
                Self::clamp_ui_font_size(config.appearance.ui_font_size).to_string(),
                window,
                cx,
            );
            s
        });
        let terminal_opacity_slider = cx.new(|_| {
            SliderState::new()
                .min(0.25)
                .max(1.0)
                .step(0.01)
                .default_value(Self::clamp_terminal_opacity(
                    config.appearance.terminal_opacity,
                ))
        });
        let ui_opacity_slider = cx.new(|_| {
            SliderState::new()
                .min(0.35)
                .max(1.0)
                .step(0.01)
                .default_value(Self::clamp_ui_opacity(config.appearance.ui_opacity))
        });
        let tab_accent_inactive_alpha_slider = cx.new(|_| {
            SliderState::new()
                .min(AppearanceConfig::MIN_TAB_ACCENT_ALPHA)
                .max(AppearanceConfig::MAX_TAB_ACCENT_INACTIVE_ALPHA)
                .step(0.01)
                .default_value(Self::clamp_tab_accent_inactive_alpha(
                    config.appearance.tab_accent_inactive_alpha,
                ))
        });
        let tab_accent_inactive_hover_alpha_slider = cx.new(|_| {
            let inactive =
                Self::clamp_tab_accent_inactive_alpha(config.appearance.tab_accent_inactive_alpha);
            SliderState::new()
                .min(AppearanceConfig::MIN_TAB_ACCENT_ALPHA)
                .max(AppearanceConfig::MAX_TAB_ACCENT_INACTIVE_HOVER_ALPHA)
                .step(0.01)
                .default_value(Self::clamp_tab_accent_inactive_hover_alpha(
                    config.appearance.tab_accent_inactive_hover_alpha,
                    inactive,
                ))
        });
        let background_image_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_placeholder("~/Pictures/wallpaper.jpg", window, cx);
            s.set_value(
                config
                    .appearance
                    .background_image
                    .clone()
                    .unwrap_or_default(),
                window,
                cx,
            );
            s
        });
        let background_image_opacity_slider = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(1.0)
                .step(0.01)
                .default_value(Self::clamp_background_image_opacity(
                    config.appearance.background_image_opacity,
                ))
        });
        let background_image_position_select = Self::make_string_select(
            BACKGROUND_IMAGE_POSITIONS,
            &config.appearance.background_image_position,
            window,
            cx,
        );
        let background_image_fit_select = Self::make_string_select(
            BACKGROUND_IMAGE_FITS,
            &config.appearance.background_image_fit,
            window,
            cx,
        );
        let custom_theme_name_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_placeholder("Save as, e.g. flexoki-amber", window, cx);
            s
        });
        let http_proxy_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            let val = config.network.http_proxy.clone().unwrap_or_default();
            s.set_value(val, window, cx);
            s.set_placeholder("http://127.0.0.1:1086", window, cx);
            s
        });
        let https_proxy_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            let val = config.network.https_proxy.clone().unwrap_or_default();
            s.set_value(val, window, cx);
            s.set_placeholder("http://127.0.0.1:1086", window, cx);
            s
        });

        cx.subscribe(
            &terminal_opacity_slider,
            |this, _, event: &SliderEvent, cx| match event {
                SliderEvent::Change(value) => {
                    this.config.appearance.terminal_opacity =
                        Self::clamp_terminal_opacity(value.end());
                    cx.emit(AppearancePreview);
                    cx.notify();
                }
                // Release repeats the last Change value; previewing again would
                // re-apply the native terminal config for no visible change.
                SliderEvent::Release(_) => {}
            },
        )
        .detach();
        cx.subscribe(
            &ui_opacity_slider,
            |this, _, event: &SliderEvent, cx| match event {
                SliderEvent::Change(value) => {
                    this.config.appearance.ui_opacity = Self::clamp_ui_opacity(value.end());
                    cx.emit(AppearancePreview);
                    cx.notify();
                }
                SliderEvent::Release(_) => {}
            },
        )
        .detach();
        cx.subscribe_in(
            &tab_accent_inactive_alpha_slider,
            window,
            |this, _, event: &SliderEvent, window, cx| match event {
                SliderEvent::Change(value) => {
                    let inactive = Self::clamp_tab_accent_inactive_alpha(value.end());
                    this.config.appearance.tab_accent_inactive_alpha = inactive;
                    let hover = Self::clamp_tab_accent_inactive_hover_alpha(
                        this.config.appearance.tab_accent_inactive_hover_alpha,
                        inactive,
                    );
                    this.config.appearance.tab_accent_inactive_hover_alpha = hover;
                    this.tab_accent_inactive_hover_alpha_slider
                        .update(cx, |slider, cx| slider.set_value(hover, window, cx));
                    cx.emit(AppearancePreview);
                    cx.notify();
                }
                SliderEvent::Release(_) => {}
            },
        )
        .detach();
        cx.subscribe_in(
            &tab_accent_inactive_hover_alpha_slider,
            window,
            |this, _, event: &SliderEvent, window, cx| match event {
                SliderEvent::Change(value) => {
                    let inactive = Self::clamp_tab_accent_inactive_alpha(
                        this.config.appearance.tab_accent_inactive_alpha,
                    );
                    let hover = Self::clamp_tab_accent_inactive_hover_alpha(value.end(), inactive);
                    this.config.appearance.tab_accent_inactive_hover_alpha = hover;
                    this.tab_accent_inactive_hover_alpha_slider
                        .update(cx, |slider, cx| slider.set_value(hover, window, cx));
                    cx.emit(AppearancePreview);
                    cx.notify();
                }
                SliderEvent::Release(_) => {}
            },
        )
        .detach();
        cx.subscribe(
            &background_image_opacity_slider,
            |this, _, event: &SliderEvent, cx| match event {
                SliderEvent::Change(value) => {
                    this.config.appearance.background_image_opacity =
                        Self::clamp_background_image_opacity(value.end());
                    cx.emit(AppearancePreview);
                    cx.notify();
                }
                SliderEvent::Release(_) => {}
            },
        )
        .detach();
        cx.subscribe_in(
            &terminal_font_select,
            window,
            |this, _, ev: &SelectEvent<SearchableVec<String>>, window, cx| {
                if let SelectEvent::Confirm(Some(value)) = ev {
                    this.config.terminal.font_family = sanitize_terminal_font_family(value);
                    this.config.terminal.font_fallback = sanitize_terminal_font_fallback(
                        &this.config.terminal.font_fallback,
                        &this.config.terminal.font_family,
                    );
                    this.sync_terminal_fallback_select(window, cx);
                    cx.emit(AppearancePreview);
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &terminal_fallback_select,
            window,
            |this, _, ev: &SelectEvent<SearchableVec<String>>, window, cx| {
                if let SelectEvent::Confirm(Some(value)) = ev {
                    let mut fallback = this.config.terminal.font_fallback.clone();
                    fallback.push(value.clone());
                    this.config.terminal.font_fallback = sanitize_terminal_font_fallback(
                        &fallback,
                        &this.config.terminal.font_family,
                    );
                    this.sync_terminal_fallback_select(window, cx);
                    cx.emit(AppearancePreview);
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &ui_font_select,
            window,
            |this, _, ev: &SelectEvent<SearchableVec<String>>, _, cx| {
                if let SelectEvent::Confirm(Some(value)) = ev {
                    this.config.appearance.ui_font_family = value.clone();
                    cx.emit(AppearancePreview);
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &cursor_style_select,
            window,
            |this, _, ev: &SelectEvent<Vec<String>>, _, cx| {
                if let SelectEvent::Confirm(Some(value)) = ev {
                    this.config.terminal.cursor_style =
                        Self::cursor_style_from_label(value).to_string();
                    cx.emit(AppearancePreview);
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &background_image_position_select,
            window,
            |this, _, ev: &SelectEvent<Vec<String>>, _, cx| {
                if let SelectEvent::Confirm(Some(value)) = ev {
                    this.config.appearance.background_image_position = value.clone();
                    cx.emit(AppearancePreview);
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &background_image_fit_select,
            window,
            |this, _, ev: &SelectEvent<Vec<String>>, _, cx| {
                if let SelectEvent::Confirm(Some(value)) = ev {
                    this.config.appearance.background_image_fit = value.clone();
                    cx.emit(AppearancePreview);
                    cx.notify();
                }
            },
        )
        .detach();

        Self {
            visible: false,
            standalone: false,
            config: config.clone(),
            preview_snapshot: None,
            icon_preview_owner: crate::app_icon::new_preview_owner(),
            focus_handle: cx.focus_handle(),
            active_section: SettingsSection::General,
            overlay_motion: MotionValue::new(0.0),
            terminal_font_select,
            shell_input,
            terminal_fallback_select,
            terminal_font_families,
            ui_font_select,
            cursor_style_select,
            font_size_input,
            ui_font_size_input,
            terminal_opacity_slider,
            terminal_blur: Self::effective_terminal_blur(config.appearance.terminal_blur),
            ui_opacity_slider,
            tab_accent_inactive_alpha_slider,
            tab_accent_inactive_hover_alpha_slider,
            background_image_input,
            background_image_opacity_slider,
            background_image_position_select,
            background_image_fit_select,
            background_image_repeat: config.appearance.background_image_repeat,
            hide_pane_title_bar: config.appearance.hide_pane_title_bar,
            close_to_quit: config.appearance.close_to_quit,
            save_error: None,
            save_error_kind: None,
            last_saved_at: std::fs::metadata(Config::config_path())
                .and_then(|m| m.modified())
                .ok(),
            close_confirmation_visible: false,
            configuration_sources: None,
            configuration_import: ConfigurationImport::Idle,
            custom_theme_name_input,
            custom_theme_preview: None,
            custom_theme_status: None,
            recording_key: None,
            #[cfg(target_os = "macos")]
            recording_resume_keybindings: None,
            http_proxy_input,
            https_proxy_input,
        }
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.standalone = false;
        if self.visible {
            // Closing — apply the same dirty-state guard the standalone
            // window uses (#181), so Esc / ⌘, / global hotkey do not
            // silently drop in-progress edits in the overlay.
            if self.has_unsaved_changes(cx) {
                self.close_confirmation_visible = true;
                cx.notify();
                return;
            }
            self.visible = false;
            self.overlay_motion
                .set_target(0.0, std::time::Duration::from_millis(180));
            // Ensure hotkeys are always re-enabled when the panel closes,
            // even if recording was active when the user dismissed it.
            self.set_recording_key(None);
        } else {
            // Opening — take a fresh snapshot so `has_unsaved_changes`
            // has a baseline to compare against. Without this, the
            // overlay path can never report dirty because
            // `open_standalone` is the only existing snapshot setter.
            self.adopt_saved_app_icon();
            self.preview_snapshot = Some(self.config.clone());
            self.close_confirmation_visible = false;
            self.visible = true;
            self.overlay_motion
                .set_target(1.0, std::time::Duration::from_millis(220));
            self.refresh_controls_from_config(window, cx);
        }
        cx.notify();
    }

    pub fn open_standalone(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.standalone = true;
        self.visible = true;
        self.adopt_saved_app_icon();
        self.preview_snapshot = Some(self.config.clone());
        self.close_confirmation_visible = false;
        self.overlay_motion
            .set_target(1.0, std::time::Duration::ZERO);
        self.refresh_controls_from_config(window, cx);
        cx.notify();
    }

    pub fn revert_standalone_preview(&mut self, cx: &mut Context<Self>) {
        // Renamed conceptually to "revert_preview" — works in both
        // standalone and overlay modes. The standalone-only early return
        // was the root cause of #181: the overlay never had a way to
        // discard edits because its snapshot was never created in the
        // first place, and even when one is, this guard would have
        // skipped the revert.
        self.close_confirmation_visible = false;
        self.set_recording_key(None);
        if let Some(snapshot) = self.preview_snapshot.take() {
            self.release_icon_preview(cx);
            self.config = snapshot;
            self.adopt_saved_app_icon();
            cx.emit(AppearancePreview);
            cx.notify();
        }
    }

    fn refresh_controls_from_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.terminal_font_select.update(cx, |select, cx| {
            select.set_selected_value(
                &sanitize_terminal_font_family(&self.config.terminal.font_family),
                window,
                cx,
            );
        });
        self.shell_input.update(cx, |s, cx| {
            s.set_value(
                self.config.terminal.shell.clone().unwrap_or_default(),
                window,
                cx,
            )
        });
        self.sync_terminal_fallback_select(window, cx);
        self.ui_font_select.update(cx, |select, cx| {
            select.set_selected_value(&self.config.appearance.ui_font_family, window, cx);
        });
        self.cursor_style_select.update(cx, |select, cx| {
            select.set_selected_value(
                &Self::cursor_style_label(&self.config.terminal.cursor_style).to_string(),
                window,
                cx,
            );
        });
        self.font_size_input.update(cx, |s, cx| {
            s.set_value(self.config.terminal.font_size.to_string(), window, cx)
        });
        self.ui_font_size_input.update(cx, |s, cx| {
            s.set_value(
                Self::clamp_ui_font_size(self.config.appearance.ui_font_size).to_string(),
                window,
                cx,
            )
        });
        self.terminal_opacity_slider.update(cx, |slider, cx| {
            slider.set_value(
                Self::clamp_terminal_opacity(self.config.appearance.terminal_opacity),
                window,
                cx,
            );
        });
        self.terminal_blur = Self::effective_terminal_blur(self.config.appearance.terminal_blur);
        self.config.appearance.terminal_blur = self.terminal_blur;
        self.ui_opacity_slider.update(cx, |slider, cx| {
            slider.set_value(
                Self::clamp_ui_opacity(self.config.appearance.ui_opacity),
                window,
                cx,
            );
        });
        self.tab_accent_inactive_alpha_slider
            .update(cx, |slider, cx| {
                slider.set_value(
                    Self::clamp_tab_accent_inactive_alpha(
                        self.config.appearance.tab_accent_inactive_alpha,
                    ),
                    window,
                    cx,
                );
            });
        self.tab_accent_inactive_hover_alpha_slider
            .update(cx, |slider, cx| {
                slider.set_value(
                    Self::clamp_tab_accent_inactive_hover_alpha(
                        self.config.appearance.tab_accent_inactive_hover_alpha,
                        Self::clamp_tab_accent_inactive_alpha(
                            self.config.appearance.tab_accent_inactive_alpha,
                        ),
                    ),
                    window,
                    cx,
                );
            });
        self.background_image_input.update(cx, |s, cx| {
            s.set_value(
                self.config
                    .appearance
                    .background_image
                    .clone()
                    .unwrap_or_default(),
                window,
                cx,
            );
        });
        self.background_image_opacity_slider
            .update(cx, |slider, cx| {
                slider.set_value(
                    Self::clamp_background_image_opacity(
                        self.config.appearance.background_image_opacity,
                    ),
                    window,
                    cx,
                );
            });
        self.background_image_position_select
            .update(cx, |select, cx| {
                select.set_selected_value(
                    &self.config.appearance.background_image_position,
                    window,
                    cx,
                );
            });
        self.background_image_fit_select.update(cx, |select, cx| {
            select.set_selected_value(&self.config.appearance.background_image_fit, window, cx);
        });
        self.background_image_repeat = self.config.appearance.background_image_repeat;
        self.hide_pane_title_bar = self.config.appearance.hide_pane_title_bar;
        self.close_to_quit = self.config.appearance.close_to_quit;
        self.set_recording_key(None);
        // Network / proxy — repopulate so reopening the panel shows current values.
        self.http_proxy_input.update(cx, |s, cx| {
            s.set_value(
                self.config.network.http_proxy.clone().unwrap_or_default(),
                window,
                cx,
            )
        });
        self.https_proxy_input.update(cx, |s, cx| {
            s.set_value(
                self.config.network.https_proxy.clone().unwrap_or_default(),
                window,
                cx,
            )
        });
        self.focus_handle.focus(window, cx);
    }

    /// Parse a ghostty config from clipboard and show a live preview.
    fn paste_theme_from_clipboard(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let text = match cx
            .read_from_clipboard()
            .and_then(|c| c.text().map(|s| s.to_string()))
        {
            Some(t) if !t.trim().is_empty() => t,
            _ => {
                self.custom_theme_status =
                    Some("Error: clipboard is empty. Copy a Ghostty theme first.".into());
                cx.notify();
                return;
            }
        };

        // Get theme name from input, fallback to "custom"
        let name_raw = self.custom_theme_name_input.read(cx).value().to_string();
        let name = if name_raw.trim().is_empty() {
            "custom".to_string()
        } else {
            name_raw.trim().to_lowercase().replace(' ', "-")
        };

        match con_terminal::TerminalTheme::from_ghostty_format(&name, &text) {
            Some(theme) => {
                self.custom_theme_status = Some(format!(
                    "Loaded \"{}\" from the clipboard. {} ANSI colors are ready to preview.",
                    display_theme_name(&name),
                    theme.ansi.len()
                ));
                self.custom_theme_preview = Some(theme);
            }
            None => {
                self.custom_theme_status = Some(
                    "Error: couldn't read a Ghostty theme. Include background, foreground, and palette entries.".into()
                );
                self.custom_theme_preview = None;
            }
        }
        cx.notify();
    }

    fn browse_background_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a background image".into()),
        });

        let input = self.background_image_input.clone();
        cx.spawn_in(window, async move |this, window| {
            let path = paths.await.ok()?.ok()??.into_iter().next()?;
            let path_text = path.to_string_lossy().to_string();

            window
                .update(|window, cx| {
                    input.update(cx, |state, cx| {
                        state.set_value(&path_text, window, cx);
                    });
                    _ = this.update(cx, |panel, cx| {
                        panel.config.appearance.background_image = Some(path_text.clone());
                        cx.emit(AppearancePreview);
                        cx.notify();
                    });
                })
                .ok()?;

            Some(())
        })
        .detach();
    }

    /// Save the custom theme to the user themes directory and apply it.
    fn apply_custom_theme(&mut self, cx: &mut Context<Self>) {
        let preview = match &self.custom_theme_preview {
            Some(t) => t.clone(),
            None => return,
        };

        let dir = con_terminal::TerminalTheme::user_themes_dir();

        // Create directory if needed
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.custom_theme_status = Some(format!("Error: {}", e));
            cx.notify();
            return;
        }

        // Read the config text from clipboard again for saving
        let text = match cx
            .read_from_clipboard()
            .and_then(|c| c.text().map(|s| s.to_string()))
        {
            Some(t) if !t.trim().is_empty() => t,
            _ => {
                self.custom_theme_status =
                    Some("Error: the clipboard changed before save. Load the theme again.".into());
                cx.notify();
                return;
            }
        };

        let file_path = dir.join(&preview.name);
        if let Err(e) = std::fs::write(&file_path, &text) {
            self.custom_theme_status = Some(format!("Error: {}", e));
            cx.notify();
            return;
        }

        // Apply the theme
        self.config.terminal.theme = preview.name.clone();
        cx.emit(ThemePreview(preview.name.clone()));
        self.custom_theme_status = Some(format!("Saved and applied: {}", file_path.display()));
        self.custom_theme_preview = None;
        cx.notify();
    }

    pub fn is_visible(&self) -> bool {
        self.visible || self.overlay_motion.is_animating()
    }

    pub fn is_overlay_visible(&self) -> bool {
        !self.standalone && self.is_visible()
    }

    fn sync_config_from_controls(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let font_size_text = self.font_size_input.read(cx).value().to_string();
        let shell_text = self.shell_input.read(cx).value().trim().to_string();
        let ui_font_size_text = self.ui_font_size_input.read(cx).value().trim().to_string();

        self.config.terminal.font_family =
            sanitize_terminal_font_family(&self.config.terminal.font_family);
        self.config.terminal.font_fallback = sanitize_terminal_font_fallback(
            &self.config.terminal.font_fallback,
            &self.config.terminal.font_family,
        );
        self.config.terminal.font_size = font_size_text.parse().unwrap_or(14.0);
        self.config.terminal.shell = (!shell_text.is_empty()).then_some(shell_text);
        let parsed_ui_font_size = if ui_font_size_text.is_empty() {
            Some(self.config.appearance.ui_font_size)
        } else {
            ui_font_size_text.parse::<f32>().ok()
        };
        let Some(parsed_ui_font_size) = parsed_ui_font_size else {
            return Err(format!(
                "UI Size must be a number between {:.1} and {:.1}.",
                MIN_UI_FONT_SIZE, MAX_UI_FONT_SIZE
            ));
        };
        self.config.appearance.ui_font_size = Self::clamp_ui_font_size(parsed_ui_font_size);
        self.config.appearance.terminal_opacity =
            Self::clamp_terminal_opacity(self.terminal_opacity_slider.read(cx).value().end());
        self.config.appearance.terminal_blur = Self::effective_terminal_blur(self.terminal_blur);
        self.terminal_blur = self.config.appearance.terminal_blur;
        self.config.appearance.ui_opacity =
            Self::clamp_ui_opacity(self.ui_opacity_slider.read(cx).value().end());
        self.config.appearance.tab_accent_inactive_alpha = Self::clamp_tab_accent_inactive_alpha(
            self.tab_accent_inactive_alpha_slider.read(cx).value().end(),
        );
        self.config.appearance.tab_accent_inactive_hover_alpha =
            Self::clamp_tab_accent_inactive_hover_alpha(
                self.tab_accent_inactive_hover_alpha_slider
                    .read(cx)
                    .value()
                    .end(),
                self.config.appearance.tab_accent_inactive_alpha,
            );
        let background_image_text = self
            .background_image_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        self.config.appearance.background_image = if background_image_text.is_empty() {
            None
        } else {
            Some(background_image_text)
        };
        self.config.appearance.background_image_opacity = Self::clamp_background_image_opacity(
            self.background_image_opacity_slider.read(cx).value().end(),
        );
        self.config.appearance.background_image_repeat = self.background_image_repeat;

        // Network / proxy
        // Blank field → None (leave inherited env untouched).
        // Non-empty   → Some(value) (override or clear on next startup).
        let http_proxy_text = self.http_proxy_input.read(cx).value().trim().to_string();
        let https_proxy_text = self.https_proxy_input.read(cx).value().trim().to_string();
        self.config.network.http_proxy = if http_proxy_text.is_empty() {
            None
        } else {
            Some(http_proxy_text)
        };
        self.config.network.https_proxy = if https_proxy_text.is_empty() {
            None
        } else {
            Some(https_proxy_text)
        };

        Ok(())
    }

    fn config_matches(a: &Config, b: &Config) -> bool {
        match (serde_json::to_value(a), serde_json::to_value(b)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    }

    fn has_unsaved_changes(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(snapshot) = self.preview_snapshot.clone() else {
            return false;
        };

        match self.sync_config_from_controls(cx) {
            Ok(()) => !Self::config_matches(&snapshot, &self.config),
            Err(_) => true,
        }
    }

    /// AppKit/GPUI owns destruction after a native close has been approved.
    /// Never remove the window from inside its should-close callback.
    pub(crate) fn approve_native_close(&mut self, cx: &mut Context<Self>) -> bool {
        self.request_close_internal(cx)
    }

    pub fn request_standalone_close(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.request_close_internal(cx) {
            return false;
        }

        if self.standalone {
            window.remove_window();
        } else {
            // Keyboard dismissal must close the overlay as well as the
            // standalone window. The dirty guard only decides whether the
            // close is allowed; it does not perform the mode-specific close.
            self.toggle(window, cx);
        }
        true
    }

    /// Shared close path used by both the standalone window and the
    /// in-workspace overlay. Returns `true` if it is safe to proceed
    /// with closing (no unsaved changes); `false` if the confirmation
    /// prompt is now visible and the caller should NOT close.
    fn request_close_internal(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.has_unsaved_changes(cx) {
            self.close_confirmation_visible = false;
            // No unsaved changes — but `revert_preview` still clears
            // the snapshot to avoid a stale baseline next time we open.
            self.revert_standalone_preview(cx);
            return true;
        }

        self.close_confirmation_visible = true;
        cx.notify();
        false
    }

    fn keep_editing_after_close_prompt(&mut self, cx: &mut Context<Self>) {
        self.close_confirmation_visible = false;
        cx.notify();
    }

    /// Discard pending edits and close. Works in both standalone
    /// (removes the window) and overlay (hides the panel with the
    /// standard close animation) modes.
    fn discard_and_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.revert_standalone_preview(cx);
        if self.standalone {
            window.remove_window();
            return;
        }
        self.visible = false;
        self.overlay_motion
            .set_target(0.0, std::time::Duration::from_millis(180));
        self.set_recording_key(None);
        cx.notify();
    }

    fn save_and_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save(window, cx);
        if self.save_error.is_none() {
            self.close_confirmation_visible = false;
            if self.standalone {
                window.remove_window();
            }
            // Overlay mode: `save()` already set `visible = false` and
            // started the close animation; nothing more to do here.
        } else {
            self.close_confirmation_visible = true;
            cx.notify();
        }
    }

    fn save(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if ConfigurationImportStatus::blocks_settings(cx) {
            self.save_error = Some(
                "Finish the configuration import or restart Con before saving settings.".into(),
            );
            self.save_error_kind = Some(SettingsSaveErrorKind::Other);
            cx.notify();
            return;
        }
        self.set_recording_key(None);

        if let Err(message) = self.sync_config_from_controls(cx) {
            self.save_error = Some(message);
            self.save_error_kind = Some(SettingsSaveErrorKind::Other);
            cx.notify();
            return;
        }

        // Keybindings are updated directly via record_keystroke — no reading needed
        if let Some(message) = keybinding_conflict_message(&self.config.keybindings) {
            self.save_error = Some(message);
            self.save_error_kind = Some(SettingsSaveErrorKind::KeybindingConflict);
            cx.notify();
            return;
        }

        #[cfg(target_os = "macos")]
        {
            let validation =
                crate::workspace::ConWorkspace::validate_native_config_candidate(&self.config);
            if let Err(message) = validation {
                self.save_error = Some(message);
                self.save_error_kind = Some(SettingsSaveErrorKind::Other);
                cx.notify();
                return;
            }
        }

        match self.persist_config(cx) {
            Ok(()) => {
                self.save_error = None;
                self.save_error_kind = None;
                self.last_saved_at = Some(std::time::SystemTime::now());
                self.preview_snapshot = Some(self.config.clone());
                if !self.standalone {
                    self.visible = false;
                    self.overlay_motion
                        .set_target(0.0, std::time::Duration::from_millis(180));
                }
                cx.emit(SaveSettings);
            }
            Err(e) => {
                log::error!("Failed to save config: {}", e);
                self.save_error = Some(e.to_string());
                self.save_error_kind = Some(SettingsSaveErrorKind::Other);
            }
        }
        cx.notify();
    }

    /// Record a keystroke for the binding currently being recorded.
    fn set_recording_key(&mut self, key: Option<String>) {
        #[cfg(target_os = "macos")]
        let was_recording = self.recording_key.is_some();
        #[cfg(target_os = "macos")]
        let will_record = key.is_some();
        self.recording_key = key;

        #[cfg(target_os = "macos")]
        match (was_recording, will_record) {
            (false, true) => {
                let keybindings = con_core::Config::load()
                    .map(|config| config.keybindings)
                    .unwrap_or_else(|err| {
                        log::warn!(
                            "settings: failed to load persisted config before hotkey recording: {err}"
                        );
                        self.config.keybindings.clone()
                    });
                self.recording_resume_keybindings = Some(keybindings.clone());
                crate::global_hotkey::suspend_global_hotkeys(&keybindings);
            }
            (true, false) => {
                if let Some(keybindings) = self.recording_resume_keybindings.take() {
                    crate::global_hotkey::resume_global_hotkeys(&keybindings);
                } else {
                    log::warn!("settings: hotkey recording ended without saved resume keybindings");
                }
            }
            _ => {}
        }
    }

    fn record_keystroke(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        let field = match &self.recording_key {
            Some(f) => f.clone(),
            None => return,
        };

        // Don't record bare modifier keys or escape (used to cancel)
        let key = &keystroke.key;
        if matches!(
            key.as_str(),
            "shift" | "control" | "alt" | "meta" | "fn" | "escape"
        ) {
            if key == "escape" {
                self.set_recording_key(None);
                cx.notify();
            }
            return;
        }

        // Build GPUI binding format: cmd-shift-k
        let binding = keystroke_to_binding(keystroke);

        // Write directly into config
        if let Some(slot) = keybinding_field_mut(&mut self.config.keybindings, &field) {
            *slot = binding;
        }
        self.set_recording_key(None);
        sync_keybinding_conflict_error(
            &mut self.save_error,
            &mut self.save_error_kind,
            &self.config.keybindings,
        );
        cx.notify();
    }

    /// Restore one keybinding to its shipped default. Draft-only, like
    /// recording: changes persist when the settings are saved.
    fn reset_keybinding(&mut self, field: &str, cx: &mut Context<Self>) {
        if let Some(default_binding) = keybinding_default(field) {
            if let Some(slot) = keybinding_field_mut(&mut self.config.keybindings, field) {
                *slot = default_binding;
            }
            sync_keybinding_conflict_error(
                &mut self.save_error,
                &mut self.save_error_kind,
                &self.config.keybindings,
            );
            cx.notify();
        }
    }

    /// Restore every keybinding to its shipped default.
    fn reset_all_keybindings(&mut self, cx: &mut Context<Self>) {
        if self.recording_key.is_some() {
            self.set_recording_key(None);
        }
        self.config.keybindings = con_core::config::KeybindingConfig::default();
        sync_keybinding_conflict_error(
            &mut self.save_error,
            &mut self.save_error_kind,
            &self.config.keybindings,
        );
        cx.notify();
    }

    /// Get the current value of a keybinding by field name.
    fn binding_value(&self, field: &str) -> &str {
        keybinding_field(&self.config.keybindings, field).unwrap_or("")
    }

    /// Updates the settings draft only. Normal Settings edits persist through
    /// the Save button; callers that already persisted this value should use
    /// `set_persisted_restore_terminal_text` to keep close/revert semantics
    /// aligned with disk.
    pub fn set_restore_terminal_text(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.preview_snapshot.is_none() {
            self.preview_snapshot = Some(self.config.clone());
        }
        self.config.appearance.restore_terminal_text = enabled;
        cx.notify();
    }

    pub fn set_persisted_restore_terminal_text(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.config.appearance.restore_terminal_text = enabled;
        if let Some(snapshot) = &mut self.preview_snapshot {
            snapshot.appearance.restore_terminal_text = enabled;
        } else {
            self.preview_snapshot = Some(self.config.clone());
        }
        cx.notify();
    }

    pub fn config(&self) -> &Config {
        &self.config
    }
    pub fn terminal_config(&self) -> &con_core::config::TerminalConfig {
        &self.config.terminal
    }
    pub fn appearance_config(&self) -> &con_core::config::AppearanceConfig {
        &self.config.appearance
    }
    fn persist_config(&mut self, cx: &App) -> anyhow::Result<()> {
        self.adopt_saved_app_icon_for_persist();
        self.config.save()?;
        crate::app_icon::remember_saved(&self.config.appearance.app_icon);
        crate::app_icon::clear_preview_owner(self.icon_preview_owner);
        crate::app_icon::sync_saved_file_icon(cx);
        Ok(())
    }

    pub(crate) fn release_icon_preview(&self, cx: &mut App) {
        crate::app_icon::restore_saved_if_owner(self.icon_preview_owner);
        crate::app_icon::sync_window_icons(cx);
    }

    fn adopt_saved_app_icon(&mut self) {
        self.config.appearance.app_icon = crate::app_icon::saved_id();
    }

    fn adopt_saved_app_icon_for_persist(&mut self) {
        let snapshot = self
            .preview_snapshot
            .as_ref()
            .map(|config| config.appearance.app_icon.as_str());
        self.config.appearance.app_icon =
            crate::app_icon::take_for_save(&self.config.appearance.app_icon, snapshot);
    }

    // ── Section content ──────────────────────────────────────────

    fn render_general(&mut self, window: &Window, cx: &mut Context<Self>) -> Div {
        let mobile = ResponsiveMode::from_width(window.viewport_size().width.as_f32()).is_mobile();
        let card_opacity = self.card_opacity();

        let theme = cx.theme();

        // Build the Updates card (only shown for channels that poll).
        let channel = con_core::release_channel::current();
        // On any target outside macOS / Windows / Linux we have no
        // update backend, so skip the card even if the channel
        // otherwise would poll.
        #[cfg_attr(
            all(
                not(target_os = "macos"),
                not(target_os = "windows"),
                not(target_os = "linux")
            ),
            allow(unused_variables)
        )]
        let show_updates = channel.polls_for_updates();

        #[cfg_attr(
            all(
                not(target_os = "macos"),
                not(target_os = "windows"),
                not(target_os = "linux")
            ),
            allow(unused_mut)
        )]
        let mut container = section_content(
            "General",
            "Terminal defaults and shared app behavior.",
            theme,
        );

        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        if show_updates {
            let updater_status = crate::updater::status();
            let latest_state = crate::updater::latest_check();

            container = container.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(group_label("Updates", theme))
                    .child(
                        card(theme, card_opacity)
                            .child(
                                div()
                                    .flex()
                                    .px(rems(1.0))
                                    .py(px(14.0))
                                    .flex_col()
                                    .gap(px(12.0))
                                    .child(
                                        div()
                                            .flex()
                                            .items_start()
                                            .justify_between()
                                            .gap(px(16.0))
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(8.0))
                                                    .child(
                                                        div()
                                                            .text_size(ui_px(theme, 12.0))
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(
                                                                theme
                                                                    .muted_foreground,
                                                            )
                                                            .child("Channel"),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(ui_px(theme, 13.0))
                                                            .line_height(ui_px(theme, 18.0))
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .child(channel.display_name()),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .items_end()
                                                    .gap(px(8.0))
                                                    .child(
                                                        div()
                                                            .text_size(ui_px(theme, 12.0))
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(
                                                                theme
                                                                    .muted_foreground,
                                                            )
                                                            .child("Version"),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(ui_px(theme, 12.0))
                                                            .line_height(ui_px(theme, 18.0))
                                                            .font_family(
                                                                theme.mono_font_family.clone(),
                                                            )
                                                            .text_color(
                                                                theme
                                                                    .muted_foreground,
                                                            )
                                                            .child(format!(
                                                                "{} ({})",
                                                                crate::app_display_version(),
                                                                crate::app_build_number()
                                                            )),
                                                    ),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .mx(rems(1.0))
                                    .h(px(1.0))
                                    .bg(theme.muted.opacity(0.10)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_end()
                                    .justify_between()
                                    .gap(px(16.0))
                                    .px(rems(1.0))
                                    .pb(px(14.0))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(3.0))
                                            .max_w(px(420.0))
                                            .child(
                                                div()
                                                    .text_size(ui_px(theme, 12.0))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(
                                                        theme
                                                            .muted_foreground,
                                                    )
                                                    .child("Status"),
                                            )
                                            .child({
                                                let (summary, detail) =
                                                    update_summary_and_detail(
                                                        &latest_state,
                                                        updater_status,
                                                    );
                                                let mut col = div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(3.0))
                                                    .child(
                                                        div()
                                                            .text_size(ui_px(theme, 12.5))
                                                            .line_height(ui_px(theme, 17.0))
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(
                                                                theme.foreground.opacity(0.88),
                                                            )
                                                            .child(summary),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(ui_px(theme, 12.0))
                                                            .line_height(ui_px(theme, 17.0))
                                                            .text_color(
                                                                theme
                                                                    .muted_foreground,
                                                            )
                                                            .child(detail),
                                                    );
                                                if let Some(url) =
                                                    update_download_url(&latest_state)
                                                {
                                                    let label = match &latest_state {
                                                        crate::updater::CheckState::UpdateAvailable { version, .. } =>
                                                            format!("Download v{version}"),
                                                        _ => "Download release".to_string(),
                                                    };
                                                    col = col.child(
                                                        div().pt(px(4.0)).child(
                                                            gpui_component::link::Link::new(
                                                                "update-download-link",
                                                            )
                                                            .href(url)
                                                            .text_size(ui_px(theme, 12.0))
                                                            .child(label),
                                                        ),
                                                    );
                                                }
                                                col
                                            }),
                                    )
                                    .child({
                                        let actions = div().flex().items_center().gap(px(6.0));

                                        // The notify-only updater
                                        // (Windows + Linux) shows
                                        // "Update now" when the
                                        // latest state has a fresh
                                        // version. macOS uses
                                        // Sparkle's own dialog
                                        // instead.
                                        #[cfg(any(target_os = "windows", target_os = "linux"))]
                                        let actions = if matches!(
                                            &latest_state,
                                            crate::updater::CheckState::UpdateAvailable { .. }
                                        ) {
                                            actions.child(
                                                Button::new("apply-update")
                                                    .small()
                                                    .primary()
                                                    .label("Update now")
                                                    .on_click(cx.listener(
                                                        |_this, _, _window, _cx| {
                                                            crate::updater::apply_update_in_place();
                                                        },
                                                    )),
                                            )
                                        } else {
                                            actions
                                        };

                                        actions.child(
                                            Button::new("check-updates")
                                                .small()
                                                .ghost()
                                                .disabled(!updater_status.can_check_manually())
                                                .label("Check for Updates")
                                                .on_click(cx.listener(
                                                    |_this, _, _window, _cx| {
                                                        crate::updater::check_for_updates();
                                                    },
                                                )),
                                        )
                                    }),
                            ),
                    ),
            );
        }

        container
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(group_label("Terminal", theme))
                .child(card(theme, card_opacity).child(row_input_with_hint(
                    "Default Shell",
                    "Command used by new panes after restarting Con. Leave blank for automatic detection.",
                    &self.shell_input,
                    theme,
                mobile))),
        )
        // Continuity
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(group_label("Continuity", theme))
                .child(card(theme, card_opacity).child(toggle_row(
                    "Restore Terminal Text",
                    "Keep terminal output when you reopen Con.",
                    Switch::new("restore-terminal-text-toggle")
                        .checked(self.config.appearance.restore_terminal_text)
                        .small()
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.set_restore_terminal_text(*checked, cx);
                        })),
                    theme,
                mobile))),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(group_label("Security", theme))
                .child(card(theme, card_opacity).child(toggle_row(
                    "Clipboard Writes",
                    "Allow terminal programs to copy plain text to the system clipboard.",
                    Switch::new("clipboard-write-toggle")
                        .checked(self.config.terminal.clipboard_write)
                        .small()
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.config.terminal.clipboard_write = *checked;
                            cx.notify();
                        })),
                    theme,
                mobile))),
        )
        // Network / proxy
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(group_label("Network", theme))
                .child(
                    card(theme, card_opacity)
                        .child(row_field("HTTP Proxy", &self.http_proxy_input, mobile))
                        .child(row_separator(theme))
                        .child(row_field("HTTPS Proxy", &self.https_proxy_input, mobile)),
                ),
        )
    }

    fn move_terminal_fallback(&mut self, index: usize, offset: isize) {
        let target = index as isize + offset;
        if index < self.config.terminal.font_fallback.len()
            && target >= 0
            && (target as usize) < self.config.terminal.font_fallback.len()
        {
            self.config
                .terminal
                .font_fallback
                .swap(index, target as usize);
        }
    }

    fn render_terminal_fallback_list(&self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let foreground = cx.theme().foreground;
        let hover = cx.theme().muted.opacity(0.08);
        let count = self.config.terminal.font_fallback.len();
        let mut list = div().flex().flex_col();

        if count == 0 {
            return list.child(
                div()
                    .px(rems(1.0))
                    .py(px(10.0))
                    .text_size(ui_px(cx.theme(), 12.0))
                    .text_color(muted)
                    .child(
                        "No preferred fallbacks. Bundled icons and system fallback remain enabled.",
                    ),
            );
        }

        for (index, family) in self.config.terminal.font_fallback.iter().enumerate() {
            let move_up = Button::new(SharedString::from(format!("terminal-fallback-up-{index}")))
                .icon(Icon::default().path("phosphor/caret-up.svg"))
                .small()
                .ghost()
                .disabled(index == 0)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.move_terminal_fallback(index, -1);
                    cx.emit(AppearancePreview);
                    cx.notify();
                }));
            let move_down = Button::new(SharedString::from(format!(
                "terminal-fallback-down-{index}"
            )))
            .icon(Icon::default().path("phosphor/caret-down.svg"))
            .small()
            .ghost()
            .disabled(index + 1 == count)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.move_terminal_fallback(index, 1);
                cx.emit(AppearancePreview);
                cx.notify();
            }));
            let remove = Button::new(SharedString::from(format!(
                "terminal-fallback-remove-{index}"
            )))
            .icon(Icon::default().path("phosphor/trash.svg"))
            .small()
            .ghost()
            .on_click(cx.listener(move |this, _, window, cx| {
                if index < this.config.terminal.font_fallback.len() {
                    this.config.terminal.font_fallback.remove(index);
                    this.sync_terminal_fallback_select(window, cx);
                    cx.emit(AppearancePreview);
                    cx.notify();
                }
            }));

            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .min_h(px(34.0))
                    .px(rems(1.0))
                    .hover(|style| style.bg(hover))
                    .child(
                        div()
                            .text_size(ui_px(cx.theme(), 12.0))
                            .text_color(foreground)
                            .child(family.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.0))
                            .text_color(muted)
                            .child(move_up)
                            .child(move_down)
                            .child(remove),
                    ),
            );
        }
        list.child(
            div()
                .px(rems(1.0))
                .pb(px(10.0))
                .text_size(ui_px(cx.theme(), 12.0))
                .text_color(muted)
                .child(
                    "Con's bundled Nerd Font and the system cascade are appended automatically.",
                ),
        )
    }

    fn render_appearance(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let mobile = ResponsiveMode::from_width(window.viewport_size().width.as_f32()).is_mobile();
        let current_theme = self.config.terminal.theme.clone();
        let terminal_font_select = self.terminal_font_select.clone();
        let terminal_fallback_select = self.terminal_fallback_select.clone();
        let terminal_fallback_list = self.render_terminal_fallback_list(cx);
        let ui_font_select = self.ui_font_select.clone();
        let font_size_input = self.font_size_input.clone();
        let ui_font_size_input = self.ui_font_size_input.clone();
        let terminal_opacity_slider = self.terminal_opacity_slider.clone();
        let ui_opacity_slider = self.ui_opacity_slider.clone();
        let tab_accent_inactive_alpha_slider = self.tab_accent_inactive_alpha_slider.clone();
        let tab_accent_inactive_hover_alpha_slider =
            self.tab_accent_inactive_hover_alpha_slider.clone();
        let background_image_input = self.background_image_input.clone();
        let background_image_opacity_slider = self.background_image_opacity_slider.clone();
        let background_image_position_select = self.background_image_position_select.clone();
        let background_image_fit_select = self.background_image_fit_select.clone();
        let terminal_opacity = self.terminal_opacity_value();
        let ui_opacity = self.ui_opacity_value();
        let tab_accent_inactive_alpha = self.tab_accent_inactive_alpha_value();
        let tab_accent_inactive_hover_alpha = self.tab_accent_inactive_hover_alpha_value();
        let background_image_opacity = self.background_image_opacity_value();
        let card_opacity = self.card_opacity();
        let image_repeat_toggle = Switch::new("background-image-repeat")
            .checked(self.background_image_repeat)
            .small()
            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                this.background_image_repeat = *checked;
                this.config.appearance.background_image_repeat = *checked;
                cx.emit(AppearancePreview);
                cx.notify();
            }));
        let all_themes = con_terminal::TerminalTheme::all_available();

        // Split into built-in and user themes
        let builtin_names: Vec<&str> = con_terminal::TerminalTheme::available().to_vec();
        let mut builtin_themes = Vec::new();
        let mut user_themes = Vec::new();
        for t in all_themes.iter() {
            if builtin_names.contains(&t.name.as_str()) {
                builtin_themes.push(t);
            } else {
                user_themes.push(t);
            }
        }
        let has_user_themes = !user_themes.is_empty();
        let total_count = builtin_themes.len() + user_themes.len();

        // Build theme grids first (these need &mut cx for listeners)
        let builtin_grid = self.render_theme_grid(&builtin_themes, &current_theme, cx);
        let user_grid = if has_user_themes {
            Some(self.render_theme_grid(&user_themes, &current_theme, cx))
        } else {
            None
        };

        // Build import section
        let custom_theme_name_input = self.custom_theme_name_input.clone();
        let paste_btn = Button::new("paste-theme-btn")
            .label("Load from Clipboard")
            .icon(Icon::default().path("phosphor/clipboard-text.svg"))
            .small()
            .ghost()
            .on_click(cx.listener(|this, _, window, cx| {
                this.paste_theme_from_clipboard(window, cx);
            }));
        let browse_background_image_btn = Button::new("browse-background-image")
            .label("Browse…")
            .icon(Icon::default().path("phosphor/folder-open.svg"))
            .small()
            .ghost()
            .on_click(cx.listener(|this, _, window, cx| {
                this.browse_background_image(window, cx);
            }));
        let open_catalog_btn = Button::new("theme-catalog-link")
            .label("Browse Themes")
            .icon(Icon::default().path("phosphor/arrow-square-out.svg"))
            .small()
            .ghost()
            .on_click(cx.listener(|_, _, _, cx| {
                cx.open_url("https://ghostty-style.vercel.app/");
            }));
        let preview_card = self
            .custom_theme_preview
            .as_ref()
            .map(|preview| self.render_single_theme_card(preview, false, cx));

        let preview_actions: Option<AnyElement> = if let Some(card) = preview_card {
            let apply_btn = Button::new("apply-custom-theme")
                .label("Save & Apply")
                .icon(Icon::default().path("phosphor/check.svg"))
                .small()
                .primary()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.apply_custom_theme(cx);
                }));
            let preview_btn = Button::new("preview-custom-theme")
                .label("Preview")
                .icon(Icon::default().path("phosphor/eye.svg"))
                .small()
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(ref preview) = this.custom_theme_preview {
                        cx.emit(ThemePreview(preview.name.clone()));
                    }
                }));
            Some(
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.0))
                    .child(card)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .child(apply_btn)
                            .child(preview_btn),
                    )
                    .into_any_element(),
            )
        } else {
            None
        };

        let app_icon_picker = self.render_app_icon_picker(card_opacity, cx);

        // Now all mutable borrows are done — get theme for pure layout
        let theme = cx.theme();

        let mut import_section = div()
            .px(px(18.0))
            .py(px(16.0))
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child("Import Theme"),
            )
            .child(
                div()
                    .text_size(ui_px(theme, 12.0))
                    .line_height(ui_px(theme, 18.0))
                    .text_color(theme.muted_foreground)
                    .child("Browse community Ghostty themes, copy, and paste here."),
            )
            // Name input
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(
                        div()
                            .text_size(ui_px(theme, 12.0))
                            .text_color(theme.muted_foreground)
                            .child("Theme name"),
                    )
                    .child(Input::new(&custom_theme_name_input)),
            )
            // Action buttons — compact row
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(paste_btn)
                    .child(open_catalog_btn),
            );

        // Preview card with save/preview actions
        if let Some(preview) = preview_actions {
            import_section = import_section.child(div().pt(px(4.0)).child(preview));
        }

        if let Some(ref status) = self.custom_theme_status {
            import_section = import_section.child(
                div()
                    .px(px(10.0))
                    .py(px(8.0))
                    .rounded(px(6.0))
                    .bg(if status.starts_with("Error") {
                        theme.danger.opacity(0.08)
                    } else {
                        theme.success.opacity(0.08)
                    })
                    .text_size(ui_px(theme, 12.0))
                    .line_height(ui_px(theme, 17.0))
                    .text_color(if status.starts_with("Error") {
                        theme.danger
                    } else {
                        theme.success
                    })
                    .child(status.clone()),
            );
        }

        // ── Built-in themes ──
        let mut theme_card_inner = div()
            .debug_selector(|| "settings-terminal-theme".into())
            .px(rems(1.0))
            .py(px(12.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .mb(px(12.0))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Terminal Theme"),
                    )
                    .child(
                        div()
                            .text_size(ui_px(theme, 12.0))
                            .text_color(theme.muted_foreground)
                            .child(format!("{total_count} themes")),
                    ),
            )
            .child(
                div()
                    .text_size(ui_px(theme, 12.0))
                    .text_color(theme.muted_foreground)
                    .mb(px(10.0))
                    .child("You can also import community-maintained Ghostty styles."),
            )
            .child(builtin_grid);

        // User-installed themes
        if let Some(user_grid) = user_grid {
            theme_card_inner = theme_card_inner.child(
                div()
                    .mt(px(16.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .mb(px(10.0))
                            .child(
                                svg()
                                    .path("phosphor/folder.svg")
                                    .size(ui_icon_px(theme, 12.0))
                                    .text_color(theme.muted_foreground),
                            )
                            .child(
                                div()
                                    .text_size(ui_px(theme, 12.0))
                                    .text_color(theme.muted_foreground)
                                    .child("Installed"),
                            ),
                    )
                    .child(user_grid),
            );
        }

        let mut content =
            section_content("Appearance", "Fonts, themes, and window appearance.", theme);

        content = content.child(
            card(theme, card_opacity)
                .flex_shrink_0()
                .child(theme_card_inner),
        );

        content = content.child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(group_label("Fonts", theme))
                .child(
                    card(theme, card_opacity)
                        .child(font_select_row(
                            "Terminal Font",
                            "Terminal and mono UI like code blocks.",
                            &terminal_font_select,
                            "Search fonts…",
                            theme,
                            cx,
                        ))
                        .child(row_separator(theme))
                        .child(searchable_select_row(
                            "Add Fallback",
                            "Preferred fonts for missing CJK, symbols, and other glyphs.",
                            &terminal_fallback_select,
                            "Search installed fonts…",
                            theme,
                            mobile,
                        ))
                        .child(terminal_fallback_list)
                        .child(row_separator(theme))
                        .child(font_select_row(
                            "UI Font",
                            "Settings, prose, and other UI.",
                            &ui_font_select,
                            "Search fonts…",
                            theme,
                            cx,
                        ))
                        .child(row_separator(theme))
                        .child(row_field("UI Size", &ui_font_size_input, mobile))
                        .child(row_separator(theme))
                        .child(row_field("Terminal Size", &font_size_input, mobile)),
                ),
        );

        content = content.child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(group_label("Cursor", theme))
                .child(
                    card(theme, card_opacity).child(div().px(rems(1.0)).child(select_row(
                        "Cursor Style",
                        "Choose how the terminal insertion point is drawn.",
                        &self.cursor_style_select,
                        theme,
                        mobile,
                    ))),
                ),
        );

        content = content.child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(group_label("Transparency", theme))
                .child(
                    card(theme, card_opacity)
                        .child(slider_row(
                            "Terminal Glass",
                            "Shared opacity for the terminal, tab bar, sidebar, and input bar.",
                            &terminal_opacity_slider,
                            terminal_opacity,
                            theme,
                        mobile))
                        .child(row_separator(theme))
                        .child(toggle_row(
                            "Terminal Blur",
                            if cfg!(target_os = "linux") {
                                "Disabled on Linux until rounded compositor blur regions are available."
                            } else {
                                "Blur the desktop behind the terminal and window chrome."
                            },
                            {
                                let terminal_blur_supported = Self::terminal_blur_supported();
                                let mut toggle = Switch::new("terminal-blur-toggle")
                                    .checked(Self::effective_terminal_blur(self.terminal_blur))
                                    .small()
                                    .disabled(!terminal_blur_supported);

                                if terminal_blur_supported {
                                    toggle = toggle.on_click(cx.listener(
                                        |this, checked: &bool, _, cx| {
                                            this.terminal_blur = *checked;
                                            this.config.appearance.terminal_blur = *checked;
                                            cx.emit(AppearancePreview);
                                            cx.notify();
                                        },
                                    ));
                                }

                                toggle
                            },
                            theme,
                        mobile))
                        .child(row_separator(theme))
                        .child(slider_row(
                            "Panels and Popups",
                            "Independent opacity for agent panels, popups, and selection surfaces.",
                            &ui_opacity_slider,
                            ui_opacity,
                            theme,
                        mobile)),
                ),
        );

        content = content.child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(group_label("Pane", theme))
                .child(
                    card(theme, card_opacity)
                        .child(toggle_row(
                            "Vertical Tabs",
                            "Use the left sidebar for workspace tabs.",
                            Switch::new("vertical-tabs-toggle")
                                .checked(
                                    self.config.appearance.tabs_orientation
                                        == TabsOrientation::Vertical,
                                )
                                .small()
                                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                    this.config.appearance.tabs_orientation = if *checked {
                                        TabsOrientation::Vertical
                                    } else {
                                        TabsOrientation::Horizontal
                                    };
                                    cx.emit(AppearancePreview);
                                    cx.notify();
                                })),
                            theme,
                        mobile))
                        .child(row_separator(theme))
                        .child(toggle_row(
                            "Hide Pane Title Bar",
                            "Hide the title bar on split panes.",
                            Switch::new("hide-pane-title-bar-toggle")
                                .checked(self.hide_pane_title_bar)
                                .small()
                                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                    let previous = this.config.appearance.hide_pane_title_bar;
                                    this.hide_pane_title_bar = *checked;
                                    this.config.appearance.hide_pane_title_bar = *checked;
                                    if let Err(err) = this.persist_config(cx) {
                                        this.hide_pane_title_bar = previous;
                                        this.config.appearance.hide_pane_title_bar = previous;
                                        log::warn!(
                                            "settings: persist hide_pane_title_bar failed: {err}"
                                        );
                                        this.save_error = Some(err.to_string());
                                        this.save_error_kind = Some(SettingsSaveErrorKind::Other);
                                        cx.notify();
                                        return;
                                    }
                                    if let Some(snapshot) = &mut this.preview_snapshot {
                                        snapshot.appearance.hide_pane_title_bar = *checked;
                                        snapshot.appearance.app_icon =
                                            this.config.appearance.app_icon.clone();
                                    }
                                    this.save_error = None;
                                    this.save_error_kind = None;
                                    cx.emit(AppearancePreview);
                                    cx.notify();
                                })),
                            theme,
                        mobile))
                        .child(row_separator(theme))
                        .child(toggle_row(
                            "Quit on Last Tab Close",
                            "Close the window when the last tab is closed. When off, a fresh tab opens instead.",
                            Switch::new("close-to-quit-toggle")
                                .checked(self.close_to_quit)
                                .small()
                                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                    let previous = this.config.appearance.close_to_quit;
                                    this.close_to_quit = *checked;
                                    this.config.appearance.close_to_quit = *checked;
                                    if let Err(err) = this.persist_config(cx) {
                                        this.close_to_quit = previous;
                                        this.config.appearance.close_to_quit = previous;
                                        log::warn!(
                                            "settings: persist close_to_quit failed: {err}"
                                        );
                                        this.save_error = Some(err.to_string());
                                        this.save_error_kind = Some(SettingsSaveErrorKind::Other);
                                        cx.notify();
                                        return;
                                    }
                                    if let Some(snapshot) = &mut this.preview_snapshot {
                                        snapshot.appearance.close_to_quit = *checked;
                                        snapshot.appearance.app_icon =
                                            this.config.appearance.app_icon.clone();
                                    }
                                    this.save_error = None;
                                    this.save_error_kind = None;
                                    cx.emit(AppearancePreview);
                                    cx.notify();
                                })),
                            theme,
                        mobile))
                        .child(row_separator(theme))
                        .child(slider_row(
                            "Inactive Accent",
                            "Accent strength for inactive tabs and unfocused pane titles.",
                            &tab_accent_inactive_alpha_slider,
                            tab_accent_inactive_alpha,
                            theme,
                        mobile))
                        .child(row_separator(theme))
                        .child(slider_row(
                            "Hover Accent",
                            "Accent strength when hovering inactive tabs.",
                            &tab_accent_inactive_hover_alpha_slider,
                            tab_accent_inactive_hover_alpha,
                            theme,
                        mobile)),
                ),
        );

        content = content.child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(group_label("Background Image", theme))
                .child(
                    card(theme, card_opacity)
                        .child(
                            div()
                                .px(rems(1.0))
                                .pt(px(12.0))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(6.0))
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .gap(px(2.0))
                                                .child(
                                                    div()
                                                        .text_size(ui_px(theme, 12.0))
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .child("Image Path"),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(ui_px(theme, 12.0))
                                                        .line_height(ui_px(theme, 18.0))
                                                        .text_color(
                                                            theme.muted_foreground,
                                                        )
                                                        .child(
                                                            "Choose a PNG or JPEG. The image is applied per terminal.",
                                                        ),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(8.0))
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .child(Input::new(&background_image_input)),
                                                )
                                                .child(browse_background_image_btn),
                                        ),
                                ),
                        )
                        .child(row_separator(theme))
                        .child(
                            div()
                                .px(rems(1.0))
                                .child(
                                    select_row(
                                        "Fit",
                                        "Choose how the image fills the terminal.",
                                        &background_image_fit_select,
                                        theme,
                                    mobile),
                                ),
                        )
                        .child(row_separator(theme))
                        .child(
                            div()
                                .px(rems(1.0))
                                .child(
                                    select_row(
                                        "Position",
                                        "Anchor if not filling the full surface.",
                                        &background_image_position_select,
                                        theme,
                                    mobile),
                                ),
                        )
                        .child(row_separator(theme))
                        .child(
                            toggle_row(
                                "Repeat",
                                "Tile if the fit leaves empty space around it.",
                                image_repeat_toggle,
                                theme,
                            mobile),
                        )
                        .child(row_separator(theme))
                        .child(slider_row(
                            "Image Strength",
                            "Blend more softly or let come forward.",
                            &background_image_opacity_slider,
                            background_image_opacity,
                            theme,
                        mobile))
                        .child(row_separator(theme))
                        .child(
                            div()
                                .px(rems(1.0))
                                .pb(px(12.0))
                                .text_size(ui_px(theme, 12.0))
                                .line_height(ui_px(theme, 18.0))
                                .text_color(theme.muted_foreground)
                                .child(
                                    "Ghostty renders the image per terminal.",
                                ),
                        ),
                ),
        );

        content = content.child(card(theme, card_opacity).child(import_section));
        content = content.child(app_icon_picker);

        content
    }

    /// Keep icon families together without adding a second text inset.
    fn render_app_icon_picker(&self, card_opacity: f32, cx: &mut Context<Self>) -> Div {
        let selected = self.config.appearance.app_icon.clone();
        let mut group_grids = Vec::new();
        for group in APP_ICON_GROUPS {
            let members = APP_ICONS
                .iter()
                .filter(|icon| icon.group == *group)
                .collect::<Vec<_>>();
            let mut grid = div().flex().flex_wrap().gap(rems(0.5));
            for choice in &members {
                grid = grid.child(self.render_app_icon_choice(choice, selected == choice.id, cx));
            }
            let show_heading = members.len() != 1 || members[0].label != *group;
            group_grids.push((*group, show_heading, grid));
        }

        let theme = cx.theme();
        let hint = if cfg!(target_os = "macos") {
            "Changes the Dock and Cmd-Tab icon while con is running."
        } else if cfg!(target_os = "windows") {
            "Changes running window icons. Pinned shortcuts keep the installed icon."
        } else {
            "Previews X11 windows. Save to change the launcher (except Flatpak); your desktop may cache the old icon."
        };

        let mut groups = div().flex().flex_col().gap(rems(1.0));
        for (group, show_heading, grid) in group_grids {
            groups = groups.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(rems(0.375))
                    .when(show_heading, |this| {
                        this.child(
                            group_label(group, theme)
                                .px(px(0.0))
                                .pb(px(0.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.muted_foreground),
                        )
                    })
                    .child(grid),
            );
        }

        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(group_label("App Icon", theme))
            .child(
                card(theme, card_opacity)
                    .child(
                        div()
                            .px(rems(1.0))
                            .pt(px(12.0))
                            .text_size(ui_px(theme, 12.0))
                            .line_height(ui_px(theme, 18.0))
                            .text_color(theme.muted_foreground)
                            .child(hint.to_string()),
                    )
                    .child(div().px(rems(1.0)).pt(px(10.0)).pb(px(14.0)).child(groups)),
            )
    }

    fn render_app_icon_choice(
        &self,
        choice: &con_core::config::AppIconChoice,
        is_sel: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let icon_id = choice.id.to_string();
        let name = if choice.group == choice.label {
            choice.label.to_string()
        } else {
            format!("{} {}", choice.group, choice.label)
        };
        appearance_image_choice(
            format!("app-icon-{icon_id}").into(),
            choice.label,
            choice.asset,
            is_sel,
            cx,
        )
        .accessibility_label(format!(
            "{}{}",
            name,
            if is_sel { ", selected" } else { "" }
        ))
        .tooltip(name)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.config.appearance.app_icon = icon_id.clone();
            crate::app_icon::apply_preview(this.icon_preview_owner, &icon_id);
            crate::app_icon::sync_window_icons(cx);
            cx.emit(AppearancePreview);
            cx.notify();
        }))
    }

    fn render_theme_grid(
        &self,
        themes: &[&con_terminal::TerminalTheme],
        current_theme: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut grid = div().flex().flex_wrap().gap(px(10.0));
        for term_theme in themes.iter() {
            let is_sel = term_theme.name.as_str() == current_theme;
            grid = grid.child(self.render_single_theme_card(term_theme, is_sel, cx));
        }
        grid
    }

    /// Render a single theme preview card.
    fn render_single_theme_card(
        &self,
        term_theme: &con_terminal::TerminalTheme,
        is_sel: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = cx.theme();
        let name = term_theme.name.clone();
        let theme_name = name.clone();
        let bg = term_theme.background;
        let fg = term_theme.foreground;
        let bg_gpui = gpui::rgb(bg.to_u32());
        let fg_gpui = gpui::rgb(fg.to_u32());
        let green = gpui::rgb(term_theme.ansi[2].to_u32());
        let cyan = gpui::rgb(term_theme.ansi[6].to_u32());
        let blue = gpui::rgb(term_theme.ansi[4].to_u32());
        let yellow = gpui::rgb(term_theme.ansi[3].to_u32());
        let red = gpui::rgb(term_theme.ansi[1].to_u32());
        let magenta = gpui::rgb(term_theme.ansi[5].to_u32());

        let terminal_preview = div()
            .flex()
            .flex_col()
            .bg(bg_gpui)
            .rounded_t(px(8.0))
            .px(px(8.0))
            .pt(px(6.0))
            .pb(px(6.0))
            .gap(px(1.0))
            .font_family(theme.mono_font_family.clone())
            .text_size(px(8.0))
            .line_height(px(12.0))
            .child(
                div()
                    .flex()
                    .gap(px(3.0))
                    .pb(px(4.0))
                    .child(div().size(px(5.0)).rounded_full().bg(red))
                    .child(div().size(px(5.0)).rounded_full().bg(yellow))
                    .child(div().size(px(5.0)).rounded_full().bg(green)),
            )
            .child(
                div()
                    .flex()
                    .gap(px(3.0))
                    .child(div().text_color(green).child("$"))
                    .child(div().text_color(cyan).child("git"))
                    .child(div().text_color(fg_gpui).child("log --oneline")),
            )
            .child(
                div()
                    .flex()
                    .gap(px(3.0))
                    .child(div().text_color(yellow).child("a1b2c3d"))
                    .child(div().text_color(fg_gpui).child("feat: init")),
            )
            .child(
                div()
                    .flex()
                    .gap(px(3.0))
                    .child(div().text_color(yellow).child("e4f5g6h"))
                    .child(div().text_color(fg_gpui).child("fix: theme")),
            )
            .child(
                div()
                    .flex()
                    .gap(px(3.0))
                    .child(div().text_color(green).child("$"))
                    .child(div().text_color(blue).child("ls"))
                    .child(div().text_color(fg_gpui).child("src/")),
            )
            .child(
                div()
                    .flex()
                    .gap(px(4.0))
                    .child(div().text_color(blue).child("lib/"))
                    .child(div().text_color(magenta).child("main.rs"))
                    .child(div().text_color(fg_gpui).child("README")),
            );

        let mut palette_strip = div().flex().h(px(4.0));
        for idx in 0..16 {
            let c = term_theme.ansi[idx];
            palette_strip = palette_strip.child(div().flex_1().h_full().bg(gpui::rgb(c.to_u32())));
        }

        let display_name = display_theme_name(&name);

        div()
            .id(SharedString::from(format!("term-theme-{name}")))
            .cursor_pointer()
            .w(ui_px(theme, 150.0))
            .flex()
            .flex_col()
            .rounded(px(10.0))
            .overflow_hidden()
            .bg(if is_sel {
                theme.primary.opacity(0.10)
            } else {
                theme.muted.opacity(0.04)
            })
            .hover(|s| s.bg(theme.primary.opacity(0.06)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.config.terminal.theme = theme_name.clone();
                    cx.emit(ThemePreview(theme_name.clone()));
                    cx.notify();
                }),
            )
            .child(terminal_preview)
            .child(palette_strip)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(4.0))
                    .min_h(rems(1.75))
                    .text_xs()
                    .font_weight(if is_sel {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .text_color(if is_sel {
                        theme.primary
                    } else {
                        theme.muted_foreground
                    })
                    .children(if is_sel {
                        Some(
                            svg()
                                .path("phosphor/check.svg")
                                .size(ui_icon_px(theme, 10.0))
                                .text_color(theme.primary),
                        )
                    } else {
                        None
                    })
                    .child(display_name),
            )
    }

    fn render_keys(&mut self, window: &Window, cx: &mut Context<Self>) -> Div {
        let mobile = ResponsiveMode::from_width(window.viewport_size().width.as_f32()).is_mobile();
        let recording = self.recording_key.clone();
        let card_opacity = self.card_opacity();

        // Editable keybinding definitions: (label, field_name)
        let general_keys: &[(&str, &str)] = &[
            ("New Window", "new_window"),
            ("New Tab", "new_tab"),
            ("Next Tab", "next_tab"),
            ("Previous Tab", "previous_tab"),
            ("Close Tab", "close_tab"),
            ("Settings", "settings"),
            ("Command Palette", "command_palette"),
            ("Toggle Agent", "toggle_agent"),
            ("Toggle Input Bar", "toggle_input_bar"),
            ("Toggle Input / Terminal", "focus_input"),
            ("Ask AI About Selection", "ask_ai"),
            ("Cycle Input Mode", "cycle_input_mode"),
            ("Toggle Pane Scope", "toggle_pane_scope"),
            ("Toggle Left Sidebar", "toggle_left_panel"),
            ("Focus Files", "focus_files"),
            ("Search Files", "search_files"),
            ("Hide Left Sidebar", "collapse_sidebar"),
            ("Quit", "quit"),
        ];

        let pane_keys: &[(&str, &str)] = &[
            #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
            ("Find in Terminal", "find_in_terminal"),
            ("Split Right", "split_right"),
            ("Split Down", "split_down"),
            ("Toggle Pane Zoom", "toggle_pane_zoom"),
            ("Close Pane", "close_pane"),
        ];

        let surface_keys: &[(&str, &str)] = &[
            ("New Surface Tab", "new_surface"),
            ("New Surface Pane Right", "new_surface_split_right"),
            ("New Surface Pane Down", "new_surface_split_down"),
            ("Next Surface Tab", "next_surface"),
            ("Previous Surface Tab", "previous_surface"),
            ("Rename Surface", "rename_surface"),
            ("Close Surface", "close_surface"),
        ];

        let build_card = |keys: &[(&str, &str)],
                          recording: &Option<String>,
                          this: &mut Self,
                          cx: &mut Context<Self>|
         -> Div {
            let theme = cx.theme();
            let mut c = card(theme, card_opacity);
            for (i, (label, field)) in keys.iter().enumerate() {
                if i > 0 {
                    c = c.child(row_separator(theme));
                }
                let value = this.binding_value(field).to_string();
                let is_recording = recording.as_deref() == Some(*field);
                let badge = if is_recording {
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .child("Press shortcut…")
                        .into_any_element()
                } else {
                    crate::keycaps::keycaps_for_binding(&value, theme)
                };
                let field_str = field.to_string();
                let reset_field = field.to_string();
                let show_reset = !is_recording
                    && keybinding_default(field).is_some_and(|default| default != value);
                let reset_button = if show_reset {
                    Some(
                        Button::new(SharedString::from(format!("key-reset-{field}")))
                            .icon(Icon::default().path("phosphor/arrow-clockwise.svg"))
                            .ghost()
                            .small()
                            .tooltip("Reset to default")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.reset_keybinding(&reset_field, cx);
                            })),
                    )
                } else {
                    None
                };
                let badge_and_reset = div()
                    .flex()
                    .items_center()
                    .when(mobile, |this| this.w_full().flex_wrap().justify_start())
                    .gap(px(2.0))
                    .child(
                        div()
                            .id(SharedString::from(format!("key-badge-{field}")))
                            .min_h(px(23.0))
                            .px(px(4.0))
                            .flex()
                            .items_center()
                            .rounded(px(5.0))
                            .cursor_pointer()
                            .bg(if is_recording {
                                theme.primary.opacity(0.12)
                            } else {
                                theme.transparent
                            })
                            .text_color(if is_recording {
                                theme.primary
                            } else {
                                theme.muted_foreground
                            })
                            .hover(|s| s.bg(theme.muted.opacity(0.055)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    this.set_recording_key(Some(field_str.clone()));
                                    cx.notify();
                                }),
                            )
                            .child(badge),
                    );
                let badge_and_reset = if let Some(reset_button) = reset_button {
                    badge_and_reset.child(reset_button)
                } else {
                    badge_and_reset
                };
                c = c.child(
                    div()
                        .id(SharedString::from(format!("key-{field}")))
                        .flex()
                        .items_center()
                        .justify_between()
                        .when(mobile, |this| this.flex_col().items_start().h_auto())
                        .min_h(rems(2.125))
                        .px(rems(1.0))
                        .hover(|s| s.bg(theme.muted.opacity(0.025)))
                        .child(
                            div()
                                .when(mobile, |this| this.min_w_0().w_full())
                                .min_w_0()
                                .text_sm()
                                .line_height(rems(1.25))
                                .text_color(theme.foreground)
                                .child(label.to_string()),
                        )
                        .child(badge_and_reset),
                );
            }
            c
        };

        let general_card = build_card(general_keys, &recording, self, cx);
        let pane_card_keys = pane_keys;
        let pane_card = build_card(pane_card_keys, &recording, self, cx);
        let surface_card = build_card(surface_keys, &recording, self, cx);
        let global_summon_enabled = self.config.keybindings.global_summon_enabled;
        let global_summon_value = self.config.keybindings.global_summon.clone();
        let global_summon_recording = recording.as_deref() == Some("global_summon");
        #[cfg(target_os = "macos")]
        let quick_terminal_enabled = self.config.keybindings.quick_terminal_enabled;
        #[cfg(target_os = "macos")]
        let quick_terminal_value = self.config.keybindings.quick_terminal.clone();
        #[cfg(target_os = "macos")]
        let quick_terminal_recording = recording.as_deref() == Some("quick_terminal");
        let theme = cx.theme();

        let fixed_tab_card = card(theme, card_opacity).child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .when(mobile, |this| this.flex_col().items_start())
                .gap(px(16.0))
                .px(rems(1.0))
                .min_h(rems(2.125))
                .hover(|s| s.bg(theme.muted.opacity(0.025)))
                .child(
                    div()
                        .text_sm()
                        .line_height(rems(1.25))
                        .text_color(theme.foreground)
                        .child("Select Tab by Number"),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(crate::keycaps::keycaps_for_binding("secondary-1", theme))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground.opacity(0.55))
                                .child("…"),
                        )
                        .child(crate::keycaps::keycaps_for_binding("secondary-9", theme)),
                ),
        );
        #[cfg(target_os = "macos")]
        let fixed_tab_card = fixed_tab_card
            .child(row_separator(theme))
            .child(key_row("Minimize Window", "cmd-m", theme))
            .child(row_separator(theme))
            .child(key_row("Next Window", "cmd-`", theme))
            .child(row_separator(theme))
            .child(key_row("Previous Window", "cmd-shift-`", theme));

        let global_summon_badge = if global_summon_recording {
            div()
                .min_h(px(28.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .rounded(px(8.0))
                .bg(theme.primary.opacity(0.10))
                .text_color(theme.primary)
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .child("Press shortcut…")
                .into_any_element()
        } else if !global_summon_value.trim().is_empty() {
            crate::keycaps::keycaps_for_binding(&global_summon_value, theme)
        } else {
            div()
                .min_h(px(28.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .rounded(px(8.0))
                .bg(theme.muted.opacity(0.08))
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.muted_foreground)
                .child("Not set")
                .into_any_element()
        };

        let global_summon_card = card(theme, card_opacity).child(
            div()
                .px(rems(1.0))
                .py(px(13.0))
                .flex()
                .items_start()
                .justify_between()
                .when(mobile, |this| this.flex_col().w_full())
                .gap(px(16.0))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .flex_1()
                        .when(mobile, |this| this.min_w_0().w_full())
                        .when(!mobile, |this| this.max_w(px(430.0)))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child("Global Hotkey"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .line_height(rems(1.125))
                                .text_color(theme.muted_foreground)
                                .child(
                                    "Show Con from anywhere in macOS. Press it again while Con is frontmost to hide the app.",
                                ),
                        ),
                )
                .child(
                    div().pt(px(1.0)).child(
                        Switch::new("global-summon-enabled")
                            .checked(global_summon_enabled)
                            .small()
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.config.keybindings.global_summon_enabled = *checked;
                                if *checked
                                    && this.config.keybindings.global_summon.trim().is_empty()
                                {
                                    this.config.keybindings.global_summon =
                                        "alt-space".to_string();
                                }
                                sync_keybinding_conflict_error(
                                    &mut this.save_error,
                                    &mut this.save_error_kind,
                                    &this.config.keybindings,
                                );
                                cx.notify();
                            })),
                    ),
                )
        )
        .child(row_separator(theme))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .when(mobile, |this| this.flex_col().items_start().w_full())
                .gap(px(16.0))
                .px(rems(1.0))
                .py(px(11.0))
                .hover(|s| s.bg(theme.muted.opacity(0.035)))
                .text_color(if global_summon_enabled {
                    theme.foreground
                } else {
                    theme.muted_foreground
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .min_w_0()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .child("Shortcut"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .line_height(rems(1.125))
                                .text_color(theme.muted_foreground)
                                .child(if global_summon_enabled {
                                    "Use a low-conflict system shortcut. Option-Space is familiar, but may collide with launchers."
                                } else {
                                    "Off by default to avoid conflicts with other global shortcuts."
                                }),
                        ),
                )
                .child(
                    div()
                        .id("key-badge-global-summon")
                        .when(!mobile, |this| this.min_w(px(112.0)).justify_end())
                        .flex()
                        .when(mobile, |this| this.w_full().flex_wrap().justify_start())
                        .opacity(if global_summon_enabled { 1.0 } else { 0.45 })
                        .cursor_pointer()
                        .rounded(px(7.0))
                        .px(px(4.0))
                        .py(px(3.0))
                        .hover(|s| s.bg(theme.muted.opacity(0.08)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                if this.config.keybindings.global_summon_enabled {
                                    this.set_recording_key(Some("global_summon".to_string()));
                                    cx.notify();
                                }
                            }),
                        )
                        .child(global_summon_badge),
                ),
        );

        #[cfg(target_os = "macos")]
        let quick_terminal_badge = if quick_terminal_recording {
            div()
                .min_h(px(28.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .rounded(px(8.0))
                .bg(theme.primary.opacity(0.10))
                .text_color(theme.primary)
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .child("Press shortcut…")
                .into_any_element()
        } else if !quick_terminal_value.trim().is_empty() {
            crate::keycaps::keycaps_for_binding(&quick_terminal_value, theme)
        } else {
            div()
                .min_h(px(28.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .rounded(px(8.0))
                .bg(theme.muted.opacity(0.08))
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.muted_foreground)
                .child("Not set")
                .into_any_element()
        };

        #[cfg(target_os = "macos")]
        let quick_terminal_card = card(theme, card_opacity)
            .child(
                div()
                    .px(rems(1.0))
                    .py(px(13.0))
                    .flex()
                    .items_start()
                    .justify_between()
                    .when(mobile, |this| this.flex_col().w_full())
                    .gap(px(16.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .flex_1()
                            .max_w(px(430.0))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("Quick Terminal"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .line_height(rems(1.125))
                                    .text_color(theme.muted_foreground)
                                    .child("Show a dedicated floating Con window that slides down from the top of the screen."),
                            ),
                    )
                    .child(
                        div().pt(px(1.0)).child(
                            Switch::new("hotkey-window-enabled")
                                .checked(quick_terminal_enabled)
                                .small()
                                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                    this.config.keybindings.quick_terminal_enabled = *checked;
                                    if *checked
                                        && this.config.keybindings.quick_terminal.trim().is_empty()
                                    {
                                        this.config.keybindings.quick_terminal = "cmd-\\".to_string();
                                    }
                                    sync_keybinding_conflict_error(
                                        &mut this.save_error,
                                        &mut this.save_error_kind,
                                        &this.config.keybindings,
                                    );
                                    cx.notify();
                                })),
                        ),
                    ),
            )
            .child(row_separator(theme))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .when(mobile, |this| this.flex_col().items_start().w_full())
                    .gap(px(16.0))
                    .px(rems(1.0))
                    .py(px(11.0))
                    .text_color(if quick_terminal_enabled {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(3.0))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("Shortcut"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .line_height(rems(1.125))
                                    .text_color(theme.muted_foreground)
                                    .child("Use a low-conflict macOS shortcut. Cmd-Backslash matches the requested default."),
                            ),
                    )
                    .child(
                        div()
                            .id("key-badge-hotkey-window")
                            .when(!mobile, |this| this.min_w(px(112.0)).justify_end())
                            .flex()
                            .when(mobile, |this| this.w_full().flex_wrap().justify_start())
                            .opacity(if quick_terminal_enabled { 1.0 } else { 0.45 })
                            .cursor_pointer()
                            .rounded(px(7.0))
                            .px(px(4.0))
                            .py(px(3.0))
                            .hover(|s| s.bg(theme.muted.opacity(0.08)))
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                                if this.config.keybindings.quick_terminal_enabled {
                                    this.set_recording_key(Some("quick_terminal".to_string()));
                                    cx.notify();
                                }
                            }))
                            .child(quick_terminal_badge),
                    ),
            );

        let shortcut_groups = div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(group_label("Global", theme))
            .child(global_summon_card);
        #[cfg(target_os = "macos")]
        let shortcut_groups = shortcut_groups.child(quick_terminal_card);
        let shortcut_groups = shortcut_groups
            .child(div().h(px(8.0)))
            .child(group_label("General", theme))
            .child(general_card);

        let reset_all_btn = Button::new("keybindings-reset-all")
            .label("Reset All")
            .icon(Icon::default().path("phosphor/arrow-clockwise.svg"))
            .small()
            .ghost()
            .on_click(cx.listener(|this, _, _, cx| {
                this.reset_all_keybindings(cx);
            }));

        section_content_with_trailing(
            "Keyboard Shortcuts",
            "Click a shortcut to record a new key combination.",
            Some(reset_all_btn.into_any_element()),
            theme,
        )
        .child(shortcut_groups)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(group_label("Fixed Shortcuts", theme))
                .child(fixed_tab_card),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(group_label("Panes", theme))
                .child(pane_card),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(group_label("Surfaces", theme))
                .child(surface_card),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(group_label("Terminal", theme))
                .child(
                    card(theme, card_opacity)
                        // Terminal clipboard uses ⌘C/V on macOS and the
                        // Windows-Terminal-standard Ctrl+Shift+C/V on
                        // Windows (plain Ctrl+C would raise SIGINT in
                        // the shell). `secondary-` would collapse to
                        // Ctrl-only on Windows, so we branch explicitly.
                        .child(key_row(
                            "Copy",
                            if cfg!(target_os = "macos") {
                                "cmd-c"
                            } else {
                                "ctrl-shift-c"
                            },
                            theme,
                        ))
                        .child(row_separator(theme))
                        .child(key_row(
                            "Paste",
                            if cfg!(target_os = "macos") {
                                "cmd-v"
                            } else {
                                "ctrl-shift-v"
                            },
                            theme,
                        ))
                        .child(row_separator(theme))
                        .child(key_row("Select All", "secondary-a", theme)),
                ),
        )
    }
}

impl EventEmitter<SaveSettings> for SettingsPanel {}
impl EventEmitter<ThemePreview> for SettingsPanel {}
impl EventEmitter<AppearancePreview> for SettingsPanel {}

impl Focusable for SettingsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SettingsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let overlay_progress = self.overlay_motion.value(window, cx);
        if overlay_progress <= 0.001 && !self.visible {
            return div().id("settings-overlay");
        }

        let active = if ConfigurationImportStatus::blocks_settings(cx) {
            SettingsSection::Configuration
        } else {
            self.active_section
        };

        // Render content first (AI needs &mut self)
        let content = match active {
            SettingsSection::General => self.render_general(window, cx),
            SettingsSection::Appearance => self.render_appearance(window, cx),
            SettingsSection::Keys => self.render_keys(window, cx),
            SettingsSection::Configuration => self.render_configuration(window, cx),
        };

        let has_unsaved_changes = self.standalone && self.has_unsaved_changes(cx);
        let theme = cx.theme();
        let viewport = window.viewport_size();
        let viewport_w = viewport.width.as_f32();
        let viewport_h = viewport.height.as_f32();
        let mode = ResponsiveMode::from_width(viewport_w);
        let mobile = mode.is_mobile();
        let compact = matches!(mode, ResponsiveMode::Compact);
        let narrow = matches!(mode, ResponsiveMode::Narrow | ResponsiveMode::Mobile);
        let sidebar_w = if narrow {
            px(48.0)
        } else if compact {
            ui_px(theme, 144.0)
        } else {
            ui_px(theme, 160.0)
        };
        let content_pad = if narrow {
            px(14.0)
        } else if compact {
            px(18.0)
        } else {
            px(24.0)
        };
        // Uniform width for all sections — prevents position jumping when switching tabs
        let card_width = px(((viewport_w * 0.76).clamp(680.0, 980.0)).min(viewport_w - 32.0));
        let card_height = {
            let target = match active {
                SettingsSection::Appearance => (viewport_h * 0.82).clamp(440.0, 780.0),
                _ => (viewport_h * 0.76).clamp(420.0, 720.0),
            };
            px(target.min(viewport_h - 32.0))
        };

        // Sidebar
        let mut sidebar = div()
            .id("settings-navigation-scroll")
            .debug_selector(|| "settings-navigation".into())
            .flex()
            .flex_col()
            .w(sidebar_w)
            .bg(theme
                .foreground
                .opacity(if theme.is_dark() { 0.035 } else { 0.025 }))
            .pt(px(8.0))
            .pb(px(12.0))
            .px(if narrow { px(4.0) } else { px(8.0) })
            .gap(px(2.0))
            .min_h_0()
            .overflow_y_scroll()
            .flex_shrink_0();

        for section in ALL_SECTIONS {
            let is_active = *section == active;
            let section_val = *section;
            let mut nav_item = div()
                .id(SharedString::from(format!("nav-{}", section.label())))
                .flex()
                .items_center()
                .h(rems(2.25))
                .rounded(px(8.0))
                .cursor_pointer()
                .bg(if is_active {
                    theme
                        .foreground
                        .opacity(if theme.is_dark() { 0.09 } else { 0.065 })
                } else {
                    theme.transparent
                })
                .text_color(if is_active {
                    theme.foreground
                } else {
                    theme.muted_foreground
                })
                .font_weight(if is_active {
                    FontWeight::MEDIUM
                } else {
                    FontWeight::NORMAL
                })
                .hover(|s| {
                    if is_active {
                        s
                    } else {
                        s.bg(theme.foreground.opacity(0.04))
                    }
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.active_section = section_val;
                        if section_val == SettingsSection::Configuration {
                            this.discover_configuration_sources(cx);
                        }
                        cx.notify();
                    }),
                );

            if narrow {
                // Icon-only mode: centered icon, no label
                nav_item = nav_item.justify_center().size(px(36.0)).mx_auto().child(
                    svg()
                        .path(section.icon())
                        .size(ui_icon_px(theme, 16.0))
                        .text_color(if is_active {
                            theme.foreground
                        } else {
                            theme.muted_foreground
                        }),
                );
            } else {
                nav_item = nav_item
                    .gap(px(8.0))
                    .px(px(10.0))
                    .text_size(ui_px(theme, 13.0))
                    .child(
                        svg()
                            .path(section.icon())
                            .size(ui_icon_px(theme, 15.0))
                            .flex_shrink_0()
                            .text_color(if is_active {
                                theme.foreground
                            } else {
                                theme.muted_foreground
                            }),
                    )
                    .child(div().min_w_0().truncate().child(section.label()));
            }

            sidebar = sidebar.child(nav_item);
        }

        let content_scroll = div()
            .id("settings-content-scroll")
            .debug_selector(|| "settings-content-scroll".into())
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .overflow_y_scroll()
            .p(content_pad)
            .child(content);
        let save_button_tint = theme.foreground.opacity(if has_unsaved_changes {
            if theme.is_dark() { 0.88 } else { 0.82 }
        } else {
            if theme.is_dark() { 0.62 } else { 0.52 }
        });
        let config_button_tone =
            theme
                .foreground
                .opacity(if theme.is_dark() { 0.66 } else { 0.54 });
        let save_button_style = ButtonCustomVariant::new(cx)
            .color(save_button_tint.opacity(if has_unsaved_changes { 0.11 } else { 0.04 }))
            .foreground(save_button_tint)
            .hover(save_button_tint.opacity(if has_unsaved_changes { 0.16 } else { 0.04 }))
            .active(save_button_tint.opacity(if has_unsaved_changes { 0.20 } else { 0.04 }));
        let header_density = ui_density_scale(theme);
        let surface_rounding = if self.standalone { px(0.0) } else { px(12.0) };
        let header_left_padding = if self.standalone && cfg!(target_os = "macos") {
            px(78.0)
        } else {
            px(20.0)
        };
        let mut header_title_area = div()
            .id("settings-titlebar-drag-area")
            .debug_selector(|| "settings-titlebar-drag-area".into())
            .flex()
            .items_center()
            .h_full()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .pl(header_left_padding)
            .pr(px(12.0))
            .child(
                div()
                    .debug_selector(|| "settings-window-title".into())
                    .flex_shrink_0()
                    .whitespace_nowrap()
                    .text_size(ui_px(theme, 13.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .child("Settings"),
            );
        if self.standalone && cfg!(target_os = "macos") {
            header_title_area = header_title_area
                .window_control_area(WindowControlArea::Drag)
                .on_click(|event, window, _cx| {
                    if event.click_count() == 2 {
                        window.titlebar_double_click();
                    }
                });
        }
        let surface = div()
            .id("settings-card")
            .w(if self.standalone {
                px(viewport_w)
            } else {
                card_width
            })
            .h(if self.standalone {
                px(viewport_h)
            } else {
                card_height
            })
            .rounded(surface_rounding)
            .bg(theme.title_bar)
            .overflow_hidden()
            .flex()
            .flex_col()
            .occlude()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                // If recording a keybinding, capture the keystroke.
                if this.recording_key.is_some() {
                    this.record_keystroke(&event.keystroke, cx);
                    return;
                }
                match event.keystroke.key.as_str() {
                    "escape" => {
                        this.request_standalone_close(window, cx);
                    }
                    "enter" if event.keystroke.modifiers.platform => {
                        this.save(window, cx);
                    }
                    "s" if event.keystroke.modifiers.platform => {
                        this.save(window, cx);
                    }
                    "w" if event.keystroke.modifiers.platform => {
                        this.request_standalone_close(window, cx);
                    }
                    _ => {}
                }
            }))
            // Header
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_shrink_0()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .h(SETTINGS_HEADER_HEIGHT)
                            .child(header_title_area)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.0))
                                    .flex_shrink_0()
                                    .pr(px(20.0))
                                    .children((self.standalone && !mobile).then(|| {
                                        let (icon, label, tone) = if has_unsaved_changes {
                                            (
                                                "phosphor/warning.svg",
                                                "Unsaved",
                                                theme.warning.opacity(if theme.is_dark() {
                                                    0.96
                                                } else {
                                                    0.92
                                                }),
                                            )
                                        } else {
                                            (
                                                "phosphor/check-circle-fill.svg",
                                                "Saved",
                                                theme.foreground.opacity(if theme.is_dark() {
                                                    0.52
                                                } else {
                                                    0.42
                                                }),
                                            )
                                        };
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(5.0))
                                            .min_w(px(64.0))
                                            .justify_end()
                                            .px(px(7.0))
                                            .h(px(24.0))
                                            .rounded(px(6.0))
                                            .bg(if has_unsaved_changes {
                                                theme.warning.opacity(if theme.is_dark() {
                                                    0.095
                                                } else {
                                                    0.070
                                                })
                                            } else {
                                                theme.transparent
                                            })
                                            .child(
                                                svg()
                                                    .path(icon)
                                                    .size(ui_icon_px(theme, 12.0))
                                                    .text_color(tone),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .line_height(rems(1.0))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(tone)
                                                    .whitespace_nowrap()
                                                    .child(label),
                                            )
                                    }))
                                    .child(
                                        Button::new("settings-open-config")
                                            .ghost()
                                            .small()
                                            .compact()
                                            .h(px(28.0 * header_density))
                                            .w(px(28.0 * header_density))
                                            .rounded(px(7.0 * header_density))
                                            .tooltip(format!(
                                                "Open {}",
                                                con_paths::CONFIG_FILE_NAME
                                            ))
                                            .child(
                                                svg()
                                                    .path("phosphor/file-text.svg")
                                                    .size(ui_icon_px(theme, 15.0))
                                                    .text_color(config_button_tone),
                                            )
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.open_configuration_file(cx);
                                            })),
                                    )
                                    .children(self.standalone.then(|| {
                                        Button::new("settings-apply")
                                            .small()
                                            .compact()
                                            .custom(save_button_style)
                                            .disabled(!has_unsaved_changes)
                                            .h(px(28.0 * header_density))
                                            .px(px(10.0 * header_density))
                                            .rounded(px(8.0 * header_density))
                                            .gap(px(5.0 * header_density))
                                            .tooltip("Save settings")
                                            .child(
                                                svg()
                                                    .path("phosphor/check.svg")
                                                    .size(ui_icon_px(theme, 12.0))
                                                    .text_color(save_button_tint),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .line_height(rems(1.0))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(save_button_tint)
                                                    .whitespace_nowrap()
                                                    .child("Save"),
                                            )
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.save(window, cx);
                                            }))
                                    })),
                            ),
                    )
                    .child(div().h(px(1.0)).bg(theme.muted.opacity(0.10))),
            )
            .children(
                (self.close_confirmation_visible && has_unsaved_changes).then(|| {
                    div()
                        .id("settings-close-confirmation")
                        .debug_selector(|| "settings-close-confirmation".into())
                        .flex()
                        .items_center()
                        .justify_between()
                        .when(mobile, |this| this.flex_col().items_start().w_full())
                        .gap(px(12.0))
                        .min_h(px(42.0))
                        .px(px(20.0))
                        .bg(theme
                            .warning
                            .opacity(if theme.is_dark() { 0.075 } else { 0.055 }))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .min_w_0()
                                .child(
                                    svg()
                                        .path("phosphor/warning.svg")
                                        .size(ui_icon_px(theme, 14.0))
                                        .text_color(theme.warning.opacity(if theme.is_dark() {
                                            0.96
                                        } else {
                                            0.90
                                        })),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .line_height(rems(1.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.foreground.opacity(if theme.is_dark() {
                                            0.84
                                        } else {
                                            0.76
                                        }))
                                        .child("Save changes before closing?"),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .when(mobile, |this| this.flex_wrap().w_full().justify_start())
                                .debug_selector(|| "settings-close-actions".into())
                                .gap(px(6.0))
                                .child(
                                    Button::new("settings-close-prompt-keep-editing")
                                        .ghost()
                                        .small()
                                        .compact()
                                        .h(px(26.0 * header_density))
                                        .px(px(8.0 * header_density))
                                        .rounded(px(7.0 * header_density))
                                        .child(
                                            div()
                                                .text_xs()
                                                .line_height(rems(1.0))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(theme.muted_foreground)
                                                .whitespace_nowrap()
                                                .child("Keep Editing"),
                                        )
                                        .on_click(cx.listener(|this, _, _window, cx| {
                                            this.keep_editing_after_close_prompt(cx);
                                        })),
                                )
                                .child(
                                    Button::new("settings-close-prompt-discard")
                                        .ghost()
                                        .small()
                                        .compact()
                                        .h(px(26.0 * header_density))
                                        .px(px(8.0 * header_density))
                                        .rounded(px(7.0 * header_density))
                                        .child(
                                            div()
                                                .text_xs()
                                                .line_height(rems(1.0))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(theme.danger.opacity(0.86))
                                                .whitespace_nowrap()
                                                .child("Discard"),
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.discard_and_close(window, cx);
                                        })),
                                )
                                .child(
                                    Button::new("settings-close-prompt-save")
                                        .small()
                                        .compact()
                                        .custom(save_button_style)
                                        .h(px(26.0 * header_density))
                                        .px(px(9.0 * header_density))
                                        .rounded(px(7.0 * header_density))
                                        .gap(px(5.0 * header_density))
                                        .child(
                                            svg()
                                                .path("phosphor/check.svg")
                                                .size(ui_icon_px(theme, 12.0))
                                                .text_color(save_button_tint),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .line_height(rems(1.0))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(save_button_tint)
                                                .whitespace_nowrap()
                                                .child("Save and Close"),
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.save_and_close(window, cx);
                                        })),
                                ),
                        )
                }),
            )
            // Error banner
            .children(self.save_error.as_ref().map(|err| {
                let message =
                    if self.save_error_kind == Some(SettingsSaveErrorKind::KeybindingConflict) {
                        err.to_string()
                    } else {
                        format!("Save failed: {err}")
                    };
                div()
                    .id("settings-save-error")
                    .debug_selector(|| "settings-save-error".into())
                    .max_h(px(120.0))
                    .flex_shrink_0()
                    .overflow_scroll()
                    .px_4()
                    .py_2()
                    .mx_4()
                    .mt_2()
                    .rounded_md()
                    .bg(theme.danger)
                    .text_color(theme.danger_foreground)
                    .text_xs()
                    .child(message)
            }))
            // Body
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(sidebar)
                    .child(
                        div()
                            .debug_selector(|| "settings-navigation-divider".into())
                            .w(px(1.0))
                            .h_full()
                            .flex_shrink_0()
                            .bg(theme.foreground.opacity(if theme.is_dark() {
                                0.10
                            } else {
                                0.08
                            })),
                    )
                    .child(content_scroll),
            );

        if self.standalone {
            return div()
                .id("settings-window")
                .size_full()
                .font_family(theme.font_family.clone())
                .bg(theme.background)
                .child(surface);
        }

        let backdrop = div()
            .id("settings-backdrop")
            .occlude()
            .absolute()
            .size_full()
            .bg(theme.background.opacity(0.6 * overlay_progress))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    // #181: clicking outside the panel previously
                    // auto-saved in-progress edits without prompting.
                    // Route through the same dirty guard as Esc / ⌘W so
                    // the user gets a chance to keep editing or
                    // discard explicitly.
                    this.request_standalone_close(window, cx);
                }),
            );

        let card = div()
            .id("settings-card-shell")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .opacity(overlay_progress)
            .child(
                div()
                    .pt(vertical_reveal_offset(overlay_progress, 18.0))
                    .opacity(overlay_progress)
                    .child(surface),
            );

        div()
            .id("settings-overlay")
            .absolute()
            .size_full()
            .font_family(theme.font_family.clone())
            .child(backdrop)
            .child(card)
    }
}

// ── Update status helpers ─────────────────────────────────────────

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn update_summary_and_detail(
    state: &crate::updater::CheckState,
    status: crate::updater::UpdaterStatus,
) -> (String, String) {
    use crate::updater::CheckState;
    match state {
        CheckState::Checking => (
            "Checking for updates…".to_string(),
            "Fetching the release feed.".to_string(),
        ),
        CheckState::UpdateAvailable { version, .. } => (
            format!("Update available: {version}"),
            "A newer build has been published.".to_string(),
        ),
        CheckState::UpToDate => (
            "Up to date".to_string(),
            format!(
                "Running {} — latest published build.",
                crate::app_display_version()
            ),
        ),
        CheckState::Error(e) => ("Update check failed".to_string(), e.clone()),
        CheckState::Idle => (status.summary().to_string(), status.detail().to_string()),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn update_download_url(state: &crate::updater::CheckState) -> Option<String> {
    match state {
        crate::updater::CheckState::UpdateAvailable { url, .. } => Some(url.clone()),
        _ => None,
    }
}

// ── Reusable building blocks ──────────────────────────────────────

fn appearance_image_choice(
    id: SharedString,
    label: &str,
    asset: &'static str,
    selected: bool,
    cx: &App,
) -> Button {
    let theme = cx.theme();
    let ink = theme.foreground;
    let fill = if selected {
        if theme.is_dark() { 0.10 } else { 0.075 }
    } else {
        0.0
    };
    let image_size = ui_px(theme, 44.0);
    let debug_id = format!("settings-choice-{id}");
    let image_id = format!("{debug_id}-image");
    let label_id = format!("{debug_id}-label");
    let selected_id = format!("{debug_id}-selected");
    Button::new(id)
        .accessibility_label(format!(
            "{label}{}",
            if selected { ", selected" } else { "" }
        ))
        .tooltip(label.to_string())
        .custom(
            ButtonCustomVariant::new(cx)
                .color(ink.opacity(fill))
                .foreground(if selected {
                    ink
                } else {
                    theme.muted_foreground
                })
                .hover(ink.opacity(if selected { fill + 0.025 } else { 0.04 }))
                .active(ink.opacity(0.14)),
        )
        .compact()
        .w(rems(6.0))
        .h_auto()
        .p(px(0.0))
        .rounded(px(8.0))
        .child(
            div()
                .relative()
                .w_full()
                .flex()
                .flex_col()
                .p(rems(0.25))
                .debug_selector(move || debug_id)
                .child(
                    div()
                        .h(rems(3.5))
                        .flex()
                        .items_center()
                        .justify_center()
                        .debug_selector(move || image_id)
                        .child(
                            img(crate::assets::png_preview(asset, image_size.as_f32()))
                                .size(image_size)
                                .object_fit(ObjectFit::Contain),
                        ),
                )
                .child(
                    div()
                        .h(rems(2.0))
                        .px(rems(0.25))
                        .flex()
                        .items_center()
                        .justify_center()
                        .debug_selector(move || label_id)
                        .text_xs()
                        .line_height(rems(1.0))
                        .text_align(TextAlign::Center)
                        .font_weight(if selected {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .child(div().min_w_0().child(label.to_string())),
                )
                .when(selected, |this| {
                    this.child(
                        div()
                            .absolute()
                            .top(rems(0.25))
                            .right(rems(0.25))
                            .debug_selector(move || selected_id)
                            .size(rems(1.0))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(theme.primary.opacity(0.12))
                            .child(
                                svg()
                                    .path("phosphor/check.svg")
                                    .size(rems(0.625))
                                    .text_color(theme.primary),
                            ),
                    )
                }),
        )
}

fn section_content(title: &str, subtitle: &str, theme: &gpui_component::Theme) -> Div {
    section_content_with_trailing(title, subtitle, None, theme)
}

fn section_content_with_trailing(
    title: &str,
    subtitle: &str,
    trailing: Option<AnyElement>,
    theme: &gpui_component::Theme,
) -> Div {
    let mut title_row = div()
        .flex()
        .items_start()
        .justify_between()
        .gap(rems(1.0))
        .child(
            div()
                .min_w_0()
                .text_size(rems(1.1875))
                .line_height(rems(1.5))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.to_string()),
        );
    if let Some(trailing) = trailing {
        title_row = title_row.child(trailing);
    }

    div()
        .w_full()
        .max_w(px(840.0))
        .mx_auto()
        .flex()
        .flex_col()
        .gap(rems(1.5))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(rems(0.375))
                .px(rems(1.0))
                .pb(rems(0.25))
                .child(title_row)
                .child(
                    div()
                        .max_w(px(520.0))
                        .text_xs()
                        .line_height(rems(1.1875))
                        .text_color(theme.muted_foreground)
                        .child(subtitle.to_string()),
                ),
        )
}

fn group_label(text: &str, theme: &gpui_component::Theme) -> Div {
    div()
        .debug_selector(|| format!("settings-group-{text}"))
        .text_xs()
        .line_height(rems(1.125))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.foreground)
        .px(rems(1.0))
        .pb(rems(0.125))
        .child(text.to_string())
}

fn card(theme: &gpui_component::Theme, opacity: f32) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .rounded(px(10.0))
        .overflow_hidden()
        .bg(theme
            .foreground
            .opacity((if theme.is_dark() { 0.045 } else { 0.025 }) * opacity.clamp(0.35, 1.0)))
}

fn row_separator(theme: &gpui_component::Theme) -> Div {
    div().h(px(1.0)).flex_shrink_0().mx(rems(1.0)).bg(theme
        .foreground
        .opacity(if theme.is_dark() { 0.07 } else { 0.055 }))
}

/// One trailing column for fields, menus and sliders; switches stay trailing-aligned.
fn settings_control(mobile: bool) -> Div {
    div()
        .min_w_0()
        .when(mobile, |this| this.w_full())
        .when(!mobile, |this| this.w(relative(0.44)).flex_shrink_0())
}

fn row_field(label: &str, input: &Entity<InputState>, mobile: bool) -> Div {
    div()
        .flex()
        .items_center()
        .when(mobile, |this| this.flex_col().items_start())
        .justify_between()
        .gap(rems(1.0))
        .px(rems(1.0))
        .py(rems(0.625))
        .when(!mobile, |this| this.min_h(rems(2.875)))
        .child(
            div()
                .debug_selector(|| format!("settings-field-{label}-label"))
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .min_w_0()
                .flex_1()
                .child(label.to_string()),
        )
        .child(
            settings_control(mobile)
                .debug_selector(|| format!("settings-field-{label}-control"))
                .child(Input::new(input)),
        )
}

fn row_input_with_hint(
    label: &str,
    hint: &str,
    input: &Entity<InputState>,
    theme: &gpui_component::Theme,
    mobile: bool,
) -> Div {
    div()
        .flex()
        .items_center()
        .when(mobile, |this| this.flex_col().items_start())
        .justify_between()
        .gap(rems(1.0))
        .px(rems(1.0))
        .py(px(10.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .flex_1()
                .min_w_0()
                .when(mobile, |this| this.w_full())
                .child(
                    div()
                        .debug_selector(|| format!("settings-field-{label}-label"))
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .text_xs()
                        .line_height(rems(1.125))
                        .text_color(theme.muted_foreground)
                        .child(hint.to_string()),
                ),
        )
        .child(
            settings_control(mobile)
                .debug_selector(|| format!("settings-field-{label}-control"))
                .child(Input::new(input)),
        )
}

fn slider_row(
    label: &str,
    hint: &str,
    slider: &Entity<SliderState>,
    value: f32,
    theme: &gpui_component::Theme,
    mobile: bool,
) -> Div {
    div()
        .debug_selector(|| format!("settings-row-{label}"))
        .flex()
        .items_center()
        .when(mobile, |this| this.flex_col().items_start())
        .justify_between()
        .gap(rems(1.0))
        .px(rems(1.0))
        .py(px(12.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .min_w_0()
                .flex_1()
                .when(mobile, |this| this.w_full())
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .debug_selector(|| format!("settings-hint-{label}"))
                        .text_xs()
                        .line_height(rems(1.125))
                        .text_color(theme.muted_foreground)
                        .child(hint.to_string()),
                ),
        )
        .child(
            settings_control(mobile)
                .debug_selector(|| format!("settings-control-{label}"))
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(
                    div().flex().justify_end().child(
                        div()
                            .min_w(px(58.0))
                            .px(px(8.0))
                            .py(px(4.0))
                            .rounded(px(999.0))
                            .bg(theme.muted.opacity(0.10))
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_align(TextAlign::Center)
                            .text_color(theme.foreground)
                            .child(format!("{:.0}%", value * 100.0)),
                    ),
                )
                .child(div().w_full().child(Slider::new(slider).w_full())),
        )
}

fn font_select_row(
    label: &str,
    hint: &str,
    select: &Entity<SelectState<SearchableVec<String>>>,
    placeholder: &str,
    theme: &gpui_component::Theme,
    cx: &App,
) -> Div {
    let selected = select.read(cx).selected_value().cloned();
    div()
        .debug_selector(|| format!("settings-row-{label}"))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .px(rems(1.0))
        .py(px(12.0))
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .child(label.to_string()),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(hint.to_string()),
        )
        .child(
            div().w_full().min_w_0().child(
                Select::new(select)
                    .placeholder(placeholder.to_string())
                    .small(),
            ),
        )
        .when_some(selected, |this, selected| {
            this.child(
                div()
                    .debug_selector(|| format!("settings-selected-font-{label}"))
                    .w_full()
                    .min_w_0()
                    .text_xs()
                    .line_height(rems(1.125))
                    .text_color(theme.muted_foreground)
                    .child(selected),
            )
        })
}

fn searchable_select_row(
    label: &str,
    hint: &str,
    select: &Entity<SelectState<SearchableVec<String>>>,
    placeholder: &str,
    theme: &gpui_component::Theme,
    mobile: bool,
) -> Div {
    div()
        .debug_selector(|| format!("settings-row-{label}"))
        .flex()
        .when(mobile, |this| this.flex_col().items_start())
        .items_start()
        .justify_between()
        .gap(rems(1.0))
        .px(rems(1.0))
        .py(px(12.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .min_w_0()
                .flex_1()
                .when(mobile, |this| this.w_full())
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .debug_selector(|| format!("settings-hint-{label}"))
                        .text_xs()
                        .line_height(rems(1.125))
                        .text_color(theme.muted_foreground)
                        .child(hint.to_string()),
                ),
        )
        .child(
            settings_control(mobile)
                .debug_selector(|| format!("settings-control-{label}"))
                .child(
                    Select::new(select)
                        .placeholder(placeholder.to_string())
                        .small(),
                ),
        )
}

fn select_row(
    label: &str,
    hint: &str,
    select: &Entity<SelectState<Vec<String>>>,
    theme: &gpui_component::Theme,
    mobile: bool,
) -> Div {
    div()
        .debug_selector(|| format!("settings-row-{label}"))
        .flex()
        .when(mobile, |this| this.flex_col().items_start())
        .items_start()
        .justify_between()
        .gap(rems(1.0))
        .py(px(12.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .min_w_0()
                .flex_1()
                .when(mobile, |this| this.w_full())
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .debug_selector(|| format!("settings-hint-{label}"))
                        .text_xs()
                        .line_height(rems(1.125))
                        .text_color(theme.muted_foreground)
                        .child(hint.to_string()),
                ),
        )
        .child(
            settings_control(mobile)
                .debug_selector(|| format!("settings-control-{label}"))
                .child(Select::new(select).small()),
        )
}

fn toggle_row(
    label: &str,
    hint: &str,
    toggle: Switch,
    theme: &gpui_component::Theme,
    mobile: bool,
) -> Div {
    div()
        .debug_selector(|| format!("settings-toggle-{label}"))
        .flex()
        .when(mobile, |this| this.flex_col().items_start())
        .items_start()
        .justify_between()
        .gap(rems(1.0))
        .px(rems(1.0))
        .py(px(12.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .min_w_0()
                .flex_1()
                .when(mobile, |this| this.w_full())
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .debug_selector(|| format!("settings-toggle-{label}-hint"))
                        .text_xs()
                        .line_height(rems(1.125))
                        .text_color(theme.muted_foreground)
                        .child(hint.to_string()),
                ),
        )
        .child(div().pt(px(2.0)).flex_shrink_0().child(toggle))
}

/// Access a keybinding config field by settings-panel field name.
fn keybinding_field_mut<'a>(
    kb: &'a mut con_core::config::KeybindingConfig,
    field: &str,
) -> Option<&'a mut String> {
    match field {
        "global_summon" => Some(&mut kb.global_summon),
        "quick_terminal" => Some(&mut kb.quick_terminal),
        "new_window" => Some(&mut kb.new_window),
        "new_tab" => Some(&mut kb.new_tab),
        "close_tab" => Some(&mut kb.close_tab),
        "close_pane" => Some(&mut kb.close_pane),
        "toggle_pane_zoom" => Some(&mut kb.toggle_pane_zoom),
        "next_tab" => Some(&mut kb.next_tab),
        "previous_tab" => Some(&mut kb.previous_tab),
        "settings" => Some(&mut kb.settings),
        "command_palette" => Some(&mut kb.command_palette),
        "toggle_input_bar" => Some(&mut kb.toggle_input_bar),
        "focus_input" => Some(&mut kb.focus_input),
        "cycle_input_mode" => Some(&mut kb.cycle_input_mode),
        "split_right" => Some(&mut kb.split_right),
        "split_down" => Some(&mut kb.split_down),
        "toggle_pane_scope" => Some(&mut kb.toggle_pane_scope),
        "toggle_left_panel" => Some(&mut kb.toggle_left_panel),
        "focus_files" => Some(&mut kb.focus_files),
        "search_files" => Some(&mut kb.search_files),
        "find_in_terminal" => Some(&mut kb.find_in_terminal),
        "collapse_sidebar" => Some(&mut kb.collapse_sidebar),
        "new_surface" => Some(&mut kb.new_surface),
        "new_surface_split_right" => Some(&mut kb.new_surface_split_right),
        "new_surface_split_down" => Some(&mut kb.new_surface_split_down),
        "next_surface" => Some(&mut kb.next_surface),
        "previous_surface" => Some(&mut kb.previous_surface),
        "rename_surface" => Some(&mut kb.rename_surface),
        "close_surface" => Some(&mut kb.close_surface),
        "quit" => Some(&mut kb.quit),
        _ => None,
    }
}

fn keybinding_field<'a>(
    kb: &'a con_core::config::KeybindingConfig,
    field: &str,
) -> Option<&'a str> {
    match field {
        "global_summon" => Some(&kb.global_summon),
        "quick_terminal" => Some(&kb.quick_terminal),
        "new_window" => Some(&kb.new_window),
        "new_tab" => Some(&kb.new_tab),
        "close_tab" => Some(&kb.close_tab),
        "close_pane" => Some(&kb.close_pane),
        "toggle_pane_zoom" => Some(&kb.toggle_pane_zoom),
        "next_tab" => Some(&kb.next_tab),
        "previous_tab" => Some(&kb.previous_tab),
        "settings" => Some(&kb.settings),
        "command_palette" => Some(&kb.command_palette),
        "toggle_input_bar" => Some(&kb.toggle_input_bar),
        "focus_input" => Some(&kb.focus_input),
        "cycle_input_mode" => Some(&kb.cycle_input_mode),
        "split_right" => Some(&kb.split_right),
        "split_down" => Some(&kb.split_down),
        "toggle_pane_scope" => Some(&kb.toggle_pane_scope),
        "toggle_left_panel" => Some(&kb.toggle_left_panel),
        "focus_files" => Some(&kb.focus_files),
        "search_files" => Some(&kb.search_files),
        "find_in_terminal" => Some(&kb.find_in_terminal),
        "collapse_sidebar" => Some(&kb.collapse_sidebar),
        "new_surface" => Some(&kb.new_surface),
        "new_surface_split_right" => Some(&kb.new_surface_split_right),
        "new_surface_split_down" => Some(&kb.new_surface_split_down),
        "next_surface" => Some(&kb.next_surface),
        "previous_surface" => Some(&kb.previous_surface),
        "rename_surface" => Some(&kb.rename_surface),
        "close_surface" => Some(&kb.close_surface),
        "quit" => Some(&kb.quit),
        _ => None,
    }
}

/// The shipped default binding for a settings field, if the field exists.
fn keybinding_default(field: &str) -> Option<String> {
    let mut defaults = con_core::config::KeybindingConfig::default();
    keybinding_field_mut(&mut defaults, field).cloned()
}

/// Convert a GPUI Keystroke to the binding format string (e.g. "cmd-shift-d").
fn keystroke_to_binding(ks: &gpui::Keystroke) -> String {
    let mut parts = Vec::new();
    if ks.modifiers.platform {
        parts.push("cmd");
    }
    if ks.modifiers.control {
        parts.push("ctrl");
    }
    if ks.modifiers.alt {
        parts.push("alt");
    }
    if ks.modifiers.shift {
        parts.push("shift");
    }
    parts.push(&ks.key);
    parts.join("-")
}

fn keybinding_conflict_message(kb: &con_core::config::KeybindingConfig) -> Option<String> {
    let conflicts = kb.shortcut_conflicts(&reserved_keybinding_shortcuts());
    let conflict = conflicts.first()?;
    Some(format!(
        "Shortcut conflict: {} is assigned to {}. Pick a different shortcut before saving.",
        conflict.binding,
        human_join(&conflict.actions)
    ))
}

fn sync_keybinding_conflict_error(
    save_error: &mut Option<String>,
    save_error_kind: &mut Option<SettingsSaveErrorKind>,
    kb: &con_core::config::KeybindingConfig,
) {
    match keybinding_conflict_message(kb) {
        Some(message) => {
            *save_error = Some(message);
            *save_error_kind = Some(SettingsSaveErrorKind::KeybindingConflict);
        }
        None if *save_error_kind == Some(SettingsSaveErrorKind::KeybindingConflict) => {
            *save_error = None;
            *save_error_kind = None;
        }
        None => {}
    }
}

fn reserved_keybinding_shortcuts() -> Vec<(&'static str, &'static str)> {
    crate::fixed_app_keybinding_shortcuts()
}

fn human_join(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [first, second] => format!("{first} and {second}"),
        _ => {
            let mut text = items[..items.len() - 1].join(", ");
            text.push_str(", and ");
            text.push_str(&items[items.len() - 1]);
            text
        }
    }
}

fn key_row(action: &str, shortcut: &str, theme: &gpui_component::Theme) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .px(rems(1.0))
        .min_h(rems(2.125))
        .hover(|s| s.bg(theme.muted.opacity(0.025)))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .text_sm()
                .line_height(rems(1.25))
                .text_color(theme.foreground)
                .child(action.to_string()),
        )
        .child(
            div()
                .flex_shrink_0()
                .child(crate::keycaps::keycaps_for_binding(shortcut, theme)),
        )
}

fn display_theme_name(name: &str) -> String {
    match name {
        "flexoki-dark" => "Flexoki Dark".into(),
        "flexoki-light" => "Flexoki Light".into(),
        "catppuccin-mocha" => "Catppuccin".into(),
        "tokyonight" => "Tokyo Night".into(),
        "rose-pine" => "Rose Pine".into(),
        "gruvbox-dark" => "Gruvbox Dark".into(),
        "solarized-dark" => "Solarized Dark".into(),
        "solarized-light" => "Solarized Light".into(),
        "one-half-dark" => "One Half Dark".into(),
        "kanagawa-wave" => "Kanagawa Wave".into(),
        "everforest-dark" => "Everforest Dark".into(),
        "everforest-light" => "Everforest Light".into(),
        "paper-light" => "Paper Light".into(),
        // User themes: convert kebab-case to Title Case
        other => other
            .split('-')
            .map(|word| {
                let mut c = word.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().to_string() + c.as_str(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ResponsiveMode, SettingsPanel, keybinding_default, keybinding_field, keybinding_field_mut,
    };

    #[gpui::test]
    fn appearance_image_choices_keep_geometry_when_selection_changes(
        cx: &mut gpui::TestAppContext,
    ) {
        con_core::release_channel::init();
        cx.update(gpui_component::init);
        for (width, font_size, mode) in [
            (920.0, 16.0, gpui_component::ThemeMode::Light),
            (920.0, 24.0, gpui_component::ThemeMode::Dark),
            (375.0, 16.0, gpui_component::ThemeMode::Dark),
            (375.0, 24.0, gpui_component::ThemeMode::Light),
        ] {
            let _runtime = std::sync::Arc::new(tokio::runtime::Runtime::new().unwrap());
            let (_panel, view) = cx.add_window_view(|window, cx| {
                gpui_component::Theme::change(mode, Some(window), cx);
                gpui_component::Theme::update(cx, |theme| theme.font_size = gpui::px(font_size));
                window.set_rem_size(gpui::px(font_size));
                let mut panel = SettingsPanel::new(&con_core::Config::default(), window, cx);
                panel.standalone = true;
                panel.visible = true;
                panel.active_section = super::SettingsSection::Appearance;
                panel
            });
            view.simulate_resize(gpui::size(gpui::px(width), gpui::px(680.0)));
            let mut tile_size = None;
            for (tile, image, name) in [
                (
                    "settings-choice-app-icon-default",
                    "settings-choice-app-icon-default-image",
                    "settings-choice-app-icon-default-label",
                ),
                (
                    "settings-choice-app-icon-raccoon-tail-a6",
                    "settings-choice-app-icon-raccoon-tail-a6-image",
                    "settings-choice-app-icon-raccoon-tail-a6-label",
                ),
            ] {
                let tile = view
                    .debug_bounds(tile)
                    .unwrap_or_else(|| panic!("missing {tile}"));
                let image = view.debug_bounds(image).expect("image frame");
                let name = view.debug_bounds(name).expect("name frame");
                if let Some(size) = tile_size {
                    assert_eq!(tile.size, size);
                }
                tile_size = Some(tile.size);
                assert!(tile.left() >= gpui::px(0.0) && tile.right() <= gpui::px(width));
                assert!(image.top() >= tile.top() && image.bottom() <= name.top());
                assert!(name.bottom() <= tile.bottom());
                assert_eq!(image.center().x, name.center().x);
            }
            assert!(
                view.debug_bounds("settings-choice-app-icon-raccoon-tail-a6-selected")
                    .is_some()
            );
        }
    }

    #[gpui::test]
    fn native_close_approval_keeps_windows_alive_until_platform_close(
        cx: &mut gpui::TestAppContext,
    ) {
        con_core::release_channel::init();
        cx.update(gpui_component::init);
        let main_window = cx
            .add_empty_window()
            .update(|window, _| window.window_handle());
        let _runtime = std::sync::Arc::new(tokio::runtime::Runtime::new().unwrap());
        let (panel, view) = cx.add_window_view(|window, cx| {
            let mut panel = SettingsPanel::new(&con_core::Config::default(), window, cx);
            panel.open_standalone(window, cx);
            panel
        });
        view.update(|window, cx| {
            let panel = panel.clone();
            window.on_window_should_close(cx, move |_, cx| {
                panel.update(cx, |panel, cx| panel.approve_native_close(cx))
            });
        });

        // This invokes the native decision callback without the later platform close.
        // Removing the window here also makes simulate_close panic when restoring it.
        assert!(view.simulate_close());
        assert_eq!(view.update(|_, cx| cx.windows().len()), 2);
        panel.update(view, |panel, cx| {
            assert!(!panel.close_confirmation_visible);
            panel.preview_snapshot = Some(panel.config.clone());
            panel
                .preview_snapshot
                .as_mut()
                .unwrap()
                .appearance
                .restore_terminal_text = !panel.config.appearance.restore_terminal_text;
            cx.notify();
        });
        assert!(!view.simulate_close());
        panel.update(view, |panel, _| assert!(panel.close_confirmation_visible));
        assert_eq!(view.update(|_, cx| cx.windows().len()), 2);

        // Explicit keyboard dismissal still performs removal, unlike native approval.
        panel.update(view, |panel, cx| {
            panel.preview_snapshot = Some(panel.config.clone());
            cx.notify();
        });
        view.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                assert!(panel.request_standalone_close(window, cx));
            });
        });
        assert_eq!(cx.update(|cx| cx.windows()), vec![main_window]);
    }

    #[gpui::test]
    fn selected_font_names_wrap_without_clipping(cx: &mut gpui::TestAppContext) {
        con_core::release_channel::init();
        cx.update(gpui_component::init);
        let family = "JetBrainsMonoNL Nerd Font Complete Very Long Family Name";
        for width in [920.0, 375.0] {
            let _runtime = std::sync::Arc::new(tokio::runtime::Runtime::new().unwrap());
            let (panel, view) = cx.add_window_view(|window, cx| {
                let mut config = con_core::Config::default();
                config.terminal.font_family = family.to_string();
                let mut panel = SettingsPanel::new(&config, window, cx);
                panel.standalone = true;
                panel.visible = true;
                panel.active_section = super::SettingsSection::Appearance;
                panel
            });
            view.simulate_resize(gpui::size(gpui::px(width), gpui::px(680.0)));
            let row = view.debug_bounds("settings-row-Terminal Font").unwrap();
            let name = view
                .debug_bounds("settings-selected-font-Terminal Font")
                .unwrap();
            assert!(name.size.height > gpui::px(0.0));
            assert!(name.left() >= row.left() && name.right() <= row.right());
            assert!(name.bottom() <= row.bottom());
            panel.read_with(view, |panel, cx| {
                assert_eq!(
                    panel
                        .terminal_font_select
                        .read(cx)
                        .selected_value()
                        .unwrap(),
                    family
                );
            });
        }
    }

    #[gpui::test]
    fn long_save_errors_leave_settings_content_visible(cx: &mut gpui::TestAppContext) {
        con_core::release_channel::init();
        cx.update(gpui_component::init);
        let _runtime = std::sync::Arc::new(tokio::runtime::Runtime::new().unwrap());
        let (_panel, cx) = cx.add_window_view(|window, cx| {
            let mut panel = SettingsPanel::new(&con_core::Config::default(), window, cx);
            panel.standalone = true;
            panel.visible = true;
            panel.save_error =
                Some("generated native configuration:2:invalid-key: unknown field\n".repeat(40));
            panel
        });
        for (width, height) in [(920.0, 680.0), (375.0, 360.0)] {
            cx.simulate_resize(gpui::size(gpui::px(width), gpui::px(height)));
            let error = cx
                .debug_bounds("settings-save-error")
                .expect("error banner");
            let content = cx
                .debug_bounds("settings-content-scroll")
                .expect("settings content");
            assert!(error.size.height > gpui::px(0.0));
            assert!(error.size.height <= gpui::px(120.0), "{error:?}");
            assert!(content.size.height >= gpui::px(100.0), "{content:?}");
            assert!(content.top() >= error.bottom(), "{error:?} {content:?}");
        }
    }

    #[gpui::test]
    fn settings_fields_align_and_scale_across_window_sizes(cx: &mut gpui::TestAppContext) {
        con_core::release_channel::init();
        cx.update(gpui_component::init);
        let mut label_heights = Vec::new();

        for (font_size, width) in [
            (16.0, 920.0),
            (24.0, 920.0),
            (16.0, 840.0),
            (24.0, 840.0),
            (16.0, 600.0),
            (24.0, 600.0),
            (16.0, 375.0),
            (24.0, 375.0),
        ] {
            let _runtime = std::sync::Arc::new(tokio::runtime::Runtime::new().unwrap());
            let (panel, view) = cx.add_window_view(move |window, cx| {
                gpui_component::Theme::update(cx, |theme| {
                    theme.font_size = gpui::px(font_size);
                });
                window.set_rem_size(gpui::px(font_size));
                let mut panel = SettingsPanel::new(&con_core::Config::default(), window, cx);
                panel.standalone = true;
                panel.visible = true;
                panel
            });
            view.simulate_resize(gpui::size(gpui::px(width), gpui::px(680.0)));

            let navigation = view
                .debug_bounds("settings-navigation")
                .expect("navigation");
            let divider = view
                .debug_bounds("settings-navigation-divider")
                .expect("navigation divider");
            let body = view
                .debug_bounds("settings-content-scroll")
                .expect("content");
            assert_eq!(divider.size.width, gpui::px(1.0));
            assert_eq!(navigation.right(), divider.left());
            assert_eq!(divider.right(), body.left());
            assert_eq!(navigation.top(), divider.top());
            assert_eq!(navigation.bottom(), divider.bottom());

            let title = view
                .debug_bounds("settings-window-title")
                .expect("window title");
            let title_area = view
                .debug_bounds("settings-titlebar-drag-area")
                .expect("title area");
            assert!(
                title.right() <= title_area.right(),
                "title clipped at {font_size}px / {width}px"
            );

            let http_label = view
                .debug_bounds("settings-field-HTTP Proxy-label")
                .expect("HTTP proxy label");
            let https_label = view
                .debug_bounds("settings-field-HTTPS Proxy-label")
                .expect("HTTPS proxy label");
            let http_control = view
                .debug_bounds("settings-field-HTTP Proxy-control")
                .expect("HTTP proxy control");
            let https_control = view
                .debug_bounds("settings-field-HTTPS Proxy-control")
                .expect("HTTPS proxy control");

            let clipboard_row = view
                .debug_bounds("settings-toggle-Clipboard Writes")
                .expect("clipboard row");
            let clipboard_hint = view
                .debug_bounds("settings-toggle-Clipboard Writes-hint")
                .expect("clipboard hint");
            assert!(clipboard_hint.size.height > gpui::px(0.0));
            assert!(
                clipboard_hint.bottom() <= clipboard_row.bottom() - gpui::px(12.0),
                "hint clipped at {font_size}px / {width}px: {clipboard_hint:?} {clipboard_row:?}",
            );

            let group = view
                .debug_bounds("settings-group-Network")
                .expect("Network heading");
            label_heights.push((font_size, width, group.size.height));
            if width >= 600.0 {
                assert_eq!(http_label.size.width, https_label.size.width);
                assert_eq!(http_control.left(), https_control.left());
            } else {
                assert_eq!(http_label.left(), http_control.left());
                assert_eq!(https_label.left(), https_control.left());
                assert!(http_control.top() >= http_label.bottom());
                assert!(https_control.top() >= https_label.bottom());
                assert_eq!(http_control.size.width, https_control.size.width);
            }

            panel.update(view, |panel, cx| {
                panel.active_section = super::SettingsSection::Appearance;
                cx.notify();
            });
            let ordered_sections = [
                "settings-terminal-theme",
                "settings-group-Fonts",
                "settings-group-Cursor",
                "settings-group-App Icon",
            ]
            .map(|selector| view.debug_bounds(selector).expect("appearance section"));
            for pair in ordered_sections.windows(2) {
                assert!(
                    pair[0].bottom() <= pair[1].top(),
                    "appearance sections out of order: {pair:?}"
                );
            }
            let controls = [
                "settings-field-UI Size-control",
                "settings-field-Terminal Size-control",
                "settings-control-Add Fallback",
                "settings-control-Cursor Style",
                "settings-control-Terminal Glass",
            ]
            .map(|selector| view.debug_bounds(selector).expect("appearance control"));
            for control in &controls[1..] {
                assert!(
                    (control.left() - controls[0].left()).abs() <= gpui::px(1.0),
                    "control columns differ at {font_size}px / {width}px: {controls:?}"
                );
                assert!(
                    (control.right() - controls[0].right()).abs() <= gpui::px(1.0),
                    "control trailing edges differ at {font_size}px / {width}px: {controls:?}"
                );
            }
            for (row_selector, hint_selector, control_selector) in [
                (
                    "settings-row-Add Fallback",
                    "settings-hint-Add Fallback",
                    "settings-control-Add Fallback",
                ),
                (
                    "settings-row-Cursor Style",
                    "settings-hint-Cursor Style",
                    "settings-control-Cursor Style",
                ),
                (
                    "settings-row-Terminal Glass",
                    "settings-hint-Terminal Glass",
                    "settings-control-Terminal Glass",
                ),
            ] {
                let row = view.debug_bounds(row_selector).expect("appearance row");
                let hint = view.debug_bounds(hint_selector).expect("appearance hint");
                let control = view
                    .debug_bounds(control_selector)
                    .expect("appearance control");
                assert!(control.size.width > gpui::px(0.0));
                assert!(
                    control.right() <= row.right() && row.right() <= gpui::px(width),
                    "control overflow at {font_size}px / {width}px: {control:?} {row:?}"
                );
                if width >= 600.0 {
                    assert!(
                        hint.right() <= control.left(),
                        "hint/control overlap: {hint:?} {control:?}"
                    );
                }
                assert!(hint.size.height > gpui::px(0.0));
                // Allow one pixel of layout rounding on the 12px bottom padding.
                assert!(
                    hint.bottom() <= row.bottom() - gpui::px(11.0),
                    "{hint_selector} clipped at {font_size}px / {width}px: {hint:?} {row:?}"
                );
            }
            panel.update(view, |panel, cx| {
                panel.preview_snapshot = Some(panel.config.clone());
                panel.config.terminal.theme = "flexoki-dark".into();
                panel.close_confirmation_visible = true;
                cx.notify();
            });
            let prompt = view
                .debug_bounds("settings-close-confirmation")
                .expect("close prompt");
            let actions = view
                .debug_bounds("settings-close-actions")
                .expect("close actions");
            assert!(
                actions.right() <= prompt.right() && actions.bottom() <= prompt.bottom(),
                "close actions overflow at {font_size}px / {width}px: {actions:?} {prompt:?}"
            );
        }

        for width in [920.0, 840.0, 600.0, 375.0] {
            let normal = label_heights
                .iter()
                .find(|(font_size, row_width, _)| *font_size == 16.0 && *row_width == width)
                .unwrap()
                .2;
            let large = label_heights
                .iter()
                .find(|(font_size, row_width, _)| *font_size == 24.0 && *row_width == width)
                .unwrap()
                .2;
            assert!(large > normal, "label did not scale at width {width}");
        }
    }

    #[test]
    fn responsive_modes_match_settings_breakpoints() {
        assert_eq!(ResponsiveMode::from_width(359.0), ResponsiveMode::Mobile);
        assert_eq!(ResponsiveMode::from_width(599.0), ResponsiveMode::Mobile);
        assert_eq!(ResponsiveMode::from_width(600.0), ResponsiveMode::Narrow);
        assert_eq!(ResponsiveMode::from_width(839.0), ResponsiveMode::Narrow);
        assert_eq!(ResponsiveMode::from_width(840.0), ResponsiveMode::Compact);
        assert_eq!(ResponsiveMode::from_width(979.0), ResponsiveMode::Compact);
        assert_eq!(ResponsiveMode::from_width(980.0), ResponsiveMode::Regular);
    }

    #[test]
    fn config_matches_returns_true_for_identical_configs() {
        use con_core::Config;
        let a = Config::default();
        let b = Config::default();
        assert!(SettingsPanel::config_matches(&a, &b));
    }

    #[test]
    fn config_matches_returns_false_when_a_field_diverges() {
        use con_core::Config;
        let a = Config::default();
        let mut b = Config::default();
        b.appearance.close_to_quit = !a.appearance.close_to_quit;
        assert!(!SettingsPanel::config_matches(&a, &b));
    }

    #[test]
    fn config_matches_ignores_field_ordering_or_whitespace_diffs() {
        // Compare typed values, not source formatting or provenance.
        // If this assertion ever fires, the
        // `has_unsaved_changes` prompt would start firing on every
        // open even for a clean user, which is the #181 inverse.
        use con_core::Config;
        let a = Config::default();
        let b = Config::default();
        assert!(SettingsPanel::config_matches(&a, &b));
    }

    #[test]
    fn keybinding_default_returns_shipped_binding_for_known_fields() {
        let defaults = con_core::config::KeybindingConfig::default();
        assert_eq!(
            keybinding_default("new_tab").as_deref(),
            keybinding_field(&defaults, "new_tab"),
        );
        assert!(keybinding_default("rename_surface").is_some());
        assert!(keybinding_default("not_a_real_field").is_none());
    }

    #[test]
    fn keybinding_field_mut_writes_same_field_that_binding_value_reads() {
        use con_core::config::KeybindingConfig;
        let mut kb = KeybindingConfig::default();
        if let Some(slot) = keybinding_field_mut(&mut kb, "rename_surface") {
            *slot = "custom-chord".to_string();
        }
        assert_eq!(
            keybinding_field(&kb, "rename_surface"),
            Some("custom-chord")
        );
        // Resetting through the accessor restores the shipped default.
        *keybinding_field_mut(&mut kb, "rename_surface").unwrap() =
            keybinding_default("rename_surface").unwrap();
        assert_eq!(
            keybinding_field(&kb, "rename_surface"),
            keybinding_field(&KeybindingConfig::default(), "rename_surface"),
        );
    }
}
