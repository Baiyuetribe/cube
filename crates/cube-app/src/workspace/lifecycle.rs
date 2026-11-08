use super::tab_presentation::editor_tab_title;
use super::*;
use crate::editor_tab_actions::reusable_editor_tab_index;
use crate::file_tree_view::OpenFileInEditorTab;

fn horizontal_tab_strip_target(orientation: TabsOrientation, tab_count: usize) -> f32 {
    if orientation == TabsOrientation::Horizontal && tab_count > 1 {
        1.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tab_strip_restore_tests {
    use super::{MotionValue, TabsOrientation, horizontal_tab_strip_target};

    #[test]
    fn restored_vertical_tabs_never_start_an_empty_horizontal_tray() {
        for count in [0, 1, 2, 12] {
            let motion = MotionValue::new(horizontal_tab_strip_target(
                TabsOrientation::Vertical,
                count,
            ));
            assert_eq!(motion.current(), 0.0);
            assert!(!motion.is_animating());
        }
        assert_eq!(
            horizontal_tab_strip_target(TabsOrientation::Horizontal, 1),
            0.0
        );
        assert_eq!(
            horizontal_tab_strip_target(TabsOrientation::Horizontal, 2),
            1.0
        );
    }
}

impl CubeWorkspace {
    #[cfg(target_os = "macos")]
    pub fn from_session(
        config: Config,
        session: Session,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::from_session_with_initial_terminal(config, session, None, None, window, cx)
    }

    pub fn from_session_with_initial_terminal(
        config: Config,
        session: Session,
        initial_working_directory: Option<std::path::PathBuf>,
        initial_command: Option<crate::startup_args::TerminalCommand>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let window_width = window.bounds().size.width.as_f32();
        let agent_panel_outer_width = 0.0;
        let initial_sidebar_width = SessionSidebar::clamped_panel_width(
            session.left_panel_width.unwrap_or(PANEL_MIN_WIDTH * 1.25),
            max_sidebar_panel_width(window_width, agent_panel_outer_width),
        );
        let sidebar = cx.new(|cx| {
            let mut s = SessionSidebar::new(cx);
            s.set_panel_width(initial_sidebar_width, cx);
            s.set_pinned(session.vertical_tabs_pinned.unwrap_or(false), cx);
            s
        });
        let terminal_font_family = sanitize_terminal_font_family(&config.terminal.font_family);
        let terminal_font_fallback =
            sanitize_terminal_font_fallback(&config.terminal.font_fallback, &terminal_font_family);
        let ui_font_family = config.appearance.ui_font_family.clone();
        let ui_font_size = config.appearance.ui_font_size;
        let font_size = config.terminal.font_size;
        let terminal_cursor_style = config.terminal.cursor_style.clone();
        let terminal_opacity = Self::effective_terminal_opacity(config.appearance.terminal_opacity);
        let terminal_blur = Self::effective_terminal_blur(config.appearance.terminal_blur);
        let ui_opacity = Self::clamp_ui_opacity(config.appearance.ui_opacity);
        let tab_accent_inactive_alpha = config.appearance.tab_accent_inactive_alpha;
        let tab_accent_inactive_hover_alpha = config.appearance.tab_accent_inactive_hover_alpha;
        let effective_ui_opacity = Self::effective_ui_opacity(ui_opacity);
        sidebar.update(cx, |s, cx| {
            s.set_tab_accent_alphas(
                tab_accent_inactive_alpha,
                tab_accent_inactive_hover_alpha,
                cx,
            );
        });
        let background_image = config.appearance.background_image.clone();
        let background_image_opacity =
            Self::clamp_background_image_opacity(config.appearance.background_image_opacity);
        let background_image_position = config.appearance.background_image_position.clone();
        let background_image_fit = config.appearance.background_image_fit.clone();
        let background_image_repeat = config.appearance.background_image_repeat;
        // Resolve the palette from the color scheme without touching the
        // stored config: `terminal.theme` is the user's saved value, and
        // rewriting it here would leak the resolved name into the next save.
        crate::theme::apply_window_color_scheme(config.appearance.color_scheme, cx);
        let scheme_is_dark = config
            .appearance
            .color_scheme
            .resolve(crate::theme::system_is_dark(window, cx));
        let scheme_theme = config
            .appearance
            .theme_for_scheme(scheme_is_dark)
            .to_string();
        let terminal_theme = TerminalTheme::by_name(&scheme_theme).unwrap_or_else(|| {
            log::warn!("Configured scheme theme {scheme_theme:?} was not found; using fallback");
            TerminalTheme::default()
        });
        let colors = theme_to_ghostty_colors(&terminal_theme);
        let ghostty_app = cube_ghostty::GhosttyApp::new(
            if cfg!(target_os = "macos") {
                None
            } else {
                Some(&colors)
            },
            config.terminal.shell.as_deref(),
            if cfg!(target_os = "macos") {
                None
            } else {
                Some(&terminal_font_family)
            },
            if cfg!(target_os = "macos") {
                None
            } else {
                Some(&terminal_font_fallback)
            },
            if cfg!(target_os = "macos") {
                Some(TerminalConfig::default().font_size)
            } else {
                Some(font_size)
            },
            if cfg!(target_os = "macos") {
                Some(AppearanceConfig::default().terminal_opacity)
            } else {
                Some(terminal_opacity)
            },
            Some(terminal_blur),
            Some(&terminal_cursor_style),
            background_image.as_deref(),
            Some(background_image_opacity),
            Some(&background_image_position),
            Some(&background_image_fit),
            Some(background_image_repeat),
            config.terminal.clipboard_write,
        )
        .map(std::sync::Arc::new)
        .unwrap_or_else(|e| panic!("Fatal: failed to initialize Ghostty: {}", e));
        #[cfg(target_os = "macos")]
        let (terminal_theme, font_size, terminal_opacity) = {
            let native_text =
                native_config_for_theme(&config, &scheme_theme).unwrap_or_else(|error| {
                    panic!("Fatal: failed to render native Ghostty configuration: {error}")
                });
            let base_dir = native_config_base_dir(&config).unwrap_or_else(|error| {
                panic!("Fatal: failed to prepare native Ghostty configuration: {error}")
            });
            let effective = ghostty_app
                .apply_native_config(native_text, &base_dir)
                .unwrap_or_else(|error| {
                    panic!("Fatal: invalid native Ghostty configuration: {error}")
                });
            let terminal_theme = native_appearance_theme(
                scheme_theme.clone(),
                effective.foreground,
                effective.background,
                &effective.palette,
            );
            (
                terminal_theme,
                effective.font_size,
                effective.background_opacity as f32,
            )
        };
        let runtime = std::sync::Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("failed to build tokio runtime"),
        );
        let session_save_tx = spawn_session_save_worker();
        let (control_request_tx, control_request_rx) = crossbeam_channel::unbounded();
        let control_socket =
            match cube_core::spawn_control_socket_server(runtime.clone(), control_request_tx) {
                Ok(handle) => Some(handle),
                Err(err) => {
                    log::error!("Failed to start con control socket: {}", err);
                    None
                }
            };
        let restore_terminal_text = config.appearance.restore_terminal_text;
        let handoff_menu_entry_enabled = config.experimental.handoff;
        let initial_working_directory = RefCell::new(initial_working_directory);
        let initial_command = RefCell::new(initial_command);
        let make_terminal = |cwd: Option<&str>,
                             restored_screen_text: Option<&[String]>,
                             force_restored_screen_text: bool,
                             window: &mut Window,
                             cx: &mut Context<Self>|
         -> TerminalPane {
            let restored_screen_text = if restore_terminal_text || force_restored_screen_text {
                restored_screen_text
            } else {
                None
            };
            let working_directory = initial_working_directory.borrow_mut().take();
            let command = initial_command.borrow_mut().take();
            make_ghostty_terminal(
                &ghostty_app,
                cwd,
                restored_screen_text,
                working_directory,
                command,
                font_size,
                handoff_menu_entry_enabled,
                window,
                cx,
            )
        };

        let mut tabs: Vec<Tab> = session
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab_state)| {
                let pane_tree = if let Some(layout) = &tab_state.layout {
                    let mut restore_terminal =
                        |restore_cwd: Option<&str>,
                         restored_screen_text: Option<&[String]>,
                         force_restored_screen_text: bool| {
                            make_terminal(
                                restore_cwd,
                                restored_screen_text,
                                force_restored_screen_text,
                                window,
                                cx,
                            )
                        };
                    PaneTree::from_state(layout, tab_state.focused_pane_id, &mut restore_terminal)
                } else {
                    let cwd = tab_state.cwd.as_deref();
                    PaneTree::new(make_terminal(cwd, None, false, window, cx))
                };
                Tab {
                    pane_tree,
                    server: None,
                    title: if tab_state.title.is_empty() {
                        format!("Terminal {}", i + 1)
                    } else {
                        tab_state.title.clone()
                    },
                    user_label: tab_state.user_label.clone(),
                    agent_cli: None,
                    color: tab_state.color,
                    needs_attention: false,
                    summary_id: i as u64,
                    runtime_trackers: RefCell::new(HashMap::new()),
                    runtime_cache: RefCell::new(HashMap::new()),
                    shell_history: Self::restore_shell_history(tab_state),
                }
            })
            .collect();
        if tabs.is_empty() {
            let terminal = make_terminal(None, None, false, window, cx);
            tabs.push(Tab {
                pane_tree: PaneTree::new(terminal),
                server: None,
                title: "Terminal".to_string(),
                user_label: None,
                agent_cli: None,
                color: None,
                needs_attention: false,
                summary_id: 0,
                runtime_trackers: RefCell::new(HashMap::new()),
                runtime_cache: RefCell::new(HashMap::new()),
                shell_history: HashMap::new(),
            });
        }
        let active_tab = session.active_tab.min(tabs.len() - 1);
        // Seed the shared tab metadata before the first `sync_sidebar`
        // call, which only fires when the live terminal title or tab set
        // changes.
        {
            let entries: Vec<SessionEntry> = tabs
                .iter()
                .enumerate()
                .map(|(i, tab)| {
                    let is_editor_only = tab.pane_tree.pane_terminals().is_empty();
                    let presentation = smart_tab_presentation(
                        tab.user_label.as_deref(),
                        None,
                        None,
                        tab.agent_cli,
                        None,
                        Some(tab.title.as_str()),
                        None,
                        i,
                        is_editor_only,
                    );
                    let pane_count = tab.pane_tree.pane_count();
                    SessionEntry {
                        id: tab.summary_id,
                        name: presentation.name,
                        subtitle: presentation.subtitle,
                        is_ssh: presentation.is_ssh,
                        needs_attention: false,
                        status: None,
                        status_detail: None,
                        terminal_titles: Vec::new(),
                        icon: presentation.icon,
                        pane_icons: Vec::new(),
                        has_user_label: tab.user_label.is_some(),
                        pane_count,
                        color: tab.color,
                    }
                })
                .collect();
            sidebar.update(cx, |s, cx| {
                s.sync_sessions(entries, active_tab, cx);
            });
        }
        let persisted_history = GlobalHistoryState::load().unwrap_or_else(|err| {
            log::warn!("Failed to load command history: {}", err);
            GlobalHistoryState::default()
        });
        let global_shell_history = Self::restore_global_shell_history(&session, &tabs);
        let global_shell_history =
            Self::merge_shell_histories(global_shell_history, &persisted_history);
        let global_input_history =
            Self::restore_global_input_history(&session, &persisted_history, &global_shell_history);
        let settings_panel = cx.new(|cx| SettingsPanel::new(&config, window, cx));
        let command_palette = cx.new(|cx| CommandPalette::new(window, cx));
        let server_picker = cx.new(|cx| ServerPicker::new(window, cx));
        sidebar.update(cx, |s, cx| s.set_ui_opacity(effective_ui_opacity, cx));
        command_palette.update(cx, |palette, _cx| {
            palette.set_ui_opacity(effective_ui_opacity)
        });
        server_picker.update(cx, |picker, _cx| {
            picker.set_ui_opacity(effective_ui_opacity)
        });
        cx.subscribe_in(&settings_panel, window, Self::on_settings_saved)
            .detach();
        cx.subscribe_in(&settings_panel, window, Self::on_theme_preview)
            .detach();
        cx.subscribe_in(&settings_panel, window, Self::on_appearance_preview)
            .detach();
        // Re-render workspace when settings panel visibility changes (e.g. X close button)
        cx.observe(&settings_panel, |_, _, cx| cx.notify()).detach();
        cx.subscribe_in(&command_palette, window, Self::on_palette_select)
            .detach();
        cx.subscribe_in(&command_palette, window, Self::on_palette_dismissed)
            .detach();
        cx.subscribe_in(&server_picker, window, Self::on_server_selected)
            .detach();
        cx.observe(&server_picker, |_, _, cx| cx.notify()).detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_select)
            .detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_new_session)
            .detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_close_tab)
            .detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_rename)
            .detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_duplicate)
            .detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_reorder)
            .detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_pane_to_tab)
            .detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_close_others)
            .detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_set_color)
            .detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_open_tool_slot)
            .detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_show_sessions)
            .detach();
        cx.observe(&sidebar, |_this, _sidebar, cx| {
            cx.notify();
        })
        .detach();
        cx.observe_window_activation(window, Self::on_window_activation_changed)
            .detach();
        // Child animations dirty rendered ancestors too, but do not notify this
        // entity's observers. Only workspace notifications invalidate these inputs.
        cx.observe_self(|this, _| this.chrome_preparation_dirty = true)
            .detach();

        // Activity bar: sync file/search drawer state back to workspace on click.
        let activity_bar_entity = cx.new(|_cx| ActivityBar::new());
        // Sync initial state from session.
        {
            let initial_slot =
                ActivitySlot::from_str(session.activity_slot.as_deref().unwrap_or("files"));
            let initial_open = !config.appearance.tabs_orientation.is_vertical()
                && session.left_panel_open.unwrap_or(true);
            activity_bar_entity.update(cx, |bar, _cx| {
                bar.active_slot = initial_slot;
                bar.left_panel_open = initial_open;
            });
        }
        cx.subscribe_in(
            &activity_bar_entity,
            window,
            |this, _bar, event: &ActivitySlotChanged, _window, cx| {
                this.activity_slot = event.slot;
                this.left_panel_open = true;
                this.sidebar_tools_open = true;
                this.save_session(cx);
                cx.notify();
            },
        )
        .detach();
        cx.subscribe_in(
            &activity_bar_entity,
            window,
            |this, _bar, _event: &ActivityTogglePanel, _window, cx| {
                if this.vertical_tabs_enabled() {
                    this.sidebar_tools_open = !this.sidebar_tools_open;
                } else {
                    this.left_panel_open = !this.left_panel_open;
                    this.sidebar_tools_open = this.left_panel_open;
                }
                this.save_session(cx);
                cx.notify();
            },
        )
        .detach();

        // Sidebar content views.
        let file_tree_entity = cx.new(|_cx| FileTreeView::new());
        let search_view_entity = cx.new(|cx| SidebarSearchView::new(window, cx));
        let git_view_entity = cx.new(|cx| crate::git_view::GitView::new(window, cx));

        // File tree: open file in the tab's shared editor pane when user clicks a file row.
        cx.subscribe_in(
            &file_tree_entity,
            window,
            |this, _tree, event: &OpenFile, window, cx| {
                this.open_path_in_active_editor_with_mode(
                    event.path.clone(),
                    None,
                    !event.keep_open,
                    window,
                    cx,
                );
            },
        )
        .detach();
        cx.subscribe_in(
            &search_view_entity,
            window,
            |this, _search, event: &OpenFile, window, cx| {
                this.open_path_in_active_editor(event.path.clone(), window, cx);
            },
        )
        .detach();
        cx.subscribe_in(
            &git_view_entity,
            window,
            |this, _git, event: &crate::git_view::OpenDiff, window, cx| {
                this.open_diff_in_active_editor(
                    crate::editor_view::DiffRequest {
                        path: std::path::PathBuf::new(),
                        title: event.title.clone(),
                        raw_diff: event.text.clone(),
                        root: event.root.clone(),
                        change: event.change.clone(),
                        is_working_tree: event.is_working_tree,
                    },
                    window,
                    cx,
                );
            },
        )
        .detach();

        // File tree: open file in a dedicated editor tab when user clicks "open in editor tab" button.
        cx.subscribe_in(
            &file_tree_entity,
            window,
            |this, _tree, event: &OpenFileInEditorTab, window, cx| {
                this.open_path_in_editor_tab(event.path.clone(), window, cx);
            },
        )
        .detach();

        // Restart must save sessions and release native surfaces in every window,
        // just like Quit. GPUI's platform restart bypasses our Quit action.
        let workspace_handle = cx.weak_entity();
        App::on_app_restart(cx, move |cx| {
            let _ = workspace_handle.update(cx, |workspace, cx| {
                workspace.prepare_app_exit(cx);
            });
        })
        .detach();

        let workspace_handle = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            // Two shutdown paths, two behaviours:
            //
            // macOS — run prepare + return true so NSApp destroys the
            // window. Background tasks riding the main runloop finish
            // naturally; NSApp keeps the process alive without windows
            // per platform convention, so no explicit quit is needed.
            //
            // Windows — returning true tears the HWND down inside the
            // same WM_CLOSE iteration that fired this callback. That
            // races with pending async window tasks (e.g.
            // `handle_activate_msg::async_block$0`) which then run on
            // a dead HWND and log "Invalid window handle" / "window
            // not found". Defer the cleanup so in-flight tasks drain
            // first, then remove the window and quit (closing the
            // last window does NOT auto-terminate the process on
            // Windows). This mirrors `close_window_from_last_tab`,
            // which the user confirmed shuts down cleanly from the
            // pane-exit path.
            #[cfg(target_os = "windows")]
            {
                // `cx.defer_in` delays by a single event-loop tick, which
                // isn't enough: GPUI's `handle_activate_msg` spawns an
                // async task on the same executor when WM_ACTIVATE fires
                // during close, and those tasks outlive one tick — they
                // then run `window.update()` on a freshly-destroyed HWND
                // and log `window not found` with a backtrace. Spawn a
                // small timer instead so every in-flight window task
                // has time to drain before we tear the HWND down.
                //
                // The 120ms timer is a probability reduction, not a
                // guarantee: Windows can still deliver `WM_ACTIVATE` /
                // `WM_PAINT` to a closing HWND and surface the same log
                // error via GPUI's `async_context::update_window`. That
                // residual noise is benign — `prepare_window_close`
                // runs, sessions save, surfaces shut down, and the
                // process exits cleanly. See
                // `postmortem/2026-04-21-windows-x-close-log-noise.md`
                // for the full analysis and the upstream GPUI fix that
                // would eliminate the noise.
                //
                // `cx` inside this callback is `&mut App`, which has
                // `spawn` (not `spawn_in`), so reach the window via its
                // handle inside the spawned task.
                let handle = workspace_handle.clone();
                let window_handle = window.window_handle();
                cx.spawn(async move |cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(120))
                        .await;
                    let _ = window_handle.update(cx, |_, window, cx| {
                        let _ = handle.update(cx, |workspace, cx| {
                            workspace.prepare_window_close(cx);
                        });
                        let should_quit = cx.windows().len() <= 1;
                        window.remove_window();
                        if should_quit {
                            cx.quit();
                        }
                    });
                })
                .detach();
                return false;
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = window;
                let _ = workspace_handle.update(cx, |workspace, cx| {
                    workspace.prepare_window_close(cx);
                });
                true
            }
        });

        // Poll terminal presentation and control-plane requests.
        let appearance_window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let agent_cli_poll_interval = std::time::Duration::from_millis(300);
            let mut last_agent_cli_refresh: Option<std::time::Instant> = None;
            // 系统浅/深色外观的最后一次核对时间。GPUI 的外观回调会被终端
            // host NSView 挡掉，不能指望它；这里按秒核对一次
            // `effectiveAppearance`，保证系统切换后界面能跟上。
            let mut last_system_appearance_check: Option<std::time::Instant> = None;
            // 连续空闲轮数。空闲越久睡眠越长，避免空转烧 CPU；
            // 任何一次有变化都会立刻回到 4ms 的响应档位。
            let mut idle_rounds: u32 = 0;
            loop {
                let mut got_event = false;

                this.update(cx, |workspace, cx| {
                    while let Ok(request) = workspace.control_request_rx.try_recv() {
                        got_event = true;
                        workspace.handle_control_request(request, cx);
                    }

                    if workspace.pump_ghostty_views(cx) {
                        got_event = true;
                        let now = std::time::Instant::now();
                        if should_refresh_agent_cli(
                            last_agent_cli_refresh,
                            now,
                            agent_cli_poll_interval,
                        ) {
                            workspace.refresh_agent_cli_detection(cx);
                            last_agent_cli_refresh = Some(now);
                        }
                        cx.notify();
                    }
                    let now = std::time::Instant::now();
                    if should_refresh_agent_cli(last_agent_cli_refresh, now, Duration::from_secs(1))
                    {
                        workspace.refresh_agent_cli_detection(cx);
                        last_agent_cli_refresh = Some(now);
                    }
                })
                .ok();

                // 系统外观每秒核对一次，放在 `this.update` 之外：`sync_color_scheme_appearance`
                // 需要 window，而窗口更新不能嵌套在实体更新的闭包里。
                // 该方法内部短路，主题没变时不会 notify，不会造成空转。
                {
                    let now = std::time::Instant::now();
                    let due = last_system_appearance_check.is_none_or(|last| {
                        now.saturating_duration_since(last) >= Duration::from_secs(1)
                    });
                    if due {
                        last_system_appearance_check = Some(now);
                        let _ = appearance_window_handle.update(cx, |_, window, cx| {
                            let _ = this.update(cx, |workspace, cx| {
                                workspace.sync_color_scheme_appearance(window, cx);
                            });
                        });
                    }
                }

                {
                    // Yield even while busy: an always-ready agent/PTY producer
                    // must not keep this foreground task running without await.
                    // The previous 16ms cap meant a PTY
                    // chunk arriving 1ms after the loop entered the
                    // sleep had to wait the full 15ms before any
                    // GPUI repaint was even scheduled — visible as
                    // a stall between "user hits Enter on htop" and
                    // "htop's alt-screen actually paints" on Linux,
                    // because Linux drives the renderer through this
                    // loop instead of through libghostty's own
                    // NSView pump (macOS) or D3D11 swapchain
                    // (Windows). 8ms keeps the work bounded —
                    // `pump_ghostty_views` short-circuits on
                    // unchanged `wake_generation` — while halving
                    // the worst-case PTY-to-frame latency. Refresh
                    // is still capped at the GPUI vsync rate
                    // (typically 60 Hz) so this doesn't actually
                    // double the paint work.
                    // 有事件时立刻回到 4ms：PTY 输出到上屏的延迟要尽量短。
                    // 空闲时逐级拉长到 100ms —— 用户看不到任何变化，
                    // 但每秒唤醒次数从 125 降到 10，空闲 CPU 明显下降。
                    // 100ms 仍是 1 秒 agent 检测间隔的十倍余量。
                    let delay_ms = if got_event {
                        idle_rounds = 0;
                        4
                    } else {
                        idle_rounds = idle_rounds.saturating_add(1);
                        match idle_rounds {
                            0..=3 => 8,
                            4..=15 => 16,
                            16..=63 => 40,
                            _ => 100,
                        }
                    };
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(delay_ms))
                        .await;
                }
            }
        })
        .detach();

        // Focus the initial terminal so the user can start typing immediately
        if let Some(initial_terminal) = tabs[active_tab].pane_tree.try_focused_terminal().cloned() {
            initial_terminal.focus(window, cx);
            Self::schedule_terminal_bootstrap_reassert(
                &initial_terminal,
                true,
                window.window_handle(),
                cx.weak_entity(),
                cx,
            );
        }

        // Hide non-active tabs' ghostty NSViews so only the active tab is visible
        for (i, tab) in tabs.iter().enumerate() {
            if i != active_tab {
                for terminal in tab.pane_tree.all_surface_terminals() {
                    terminal.set_native_view_visible(false, cx);
                }
            }
        }

        let initial_tab_strip_target =
            horizontal_tab_strip_target(config.appearance.tabs_orientation, tabs.len());
        let last_ghostty_wake_generation = ghostty_app.wake_generation();

        let next_tab_summary_id_init = tabs.len() as u64;
        let mut workspace = Self {
            config: config.clone(),
            sidebar,
            tabs,
            terminal_presentation: Default::default(),
            tab_activity: cx.new(|cx| crate::tab_activity::TabActivity::new(window, cx)),
            chrome_preparation_dirty: true,
            active_tab,
            last_editor_tab_id: None,
            is_quick_terminal: false,
            is_window_floating: false,
            terminal_font_family,
            terminal_font_fallback,
            ui_font_family,
            ui_font_size,
            font_size,
            terminal_cursor_style,
            terminal_opacity,
            terminal_blur,
            ui_opacity,
            app_icon: sanitize_app_icon(&config.appearance.app_icon),
            tab_accent_inactive_alpha,
            tab_accent_inactive_hover_alpha,
            background_image,
            background_image_opacity,
            background_image_position,
            background_image_fit,
            background_image_repeat,
            settings_panel,
            settings_window: None,
            settings_window_panel: None,
            command_palette,
            server_picker,
            global_shell_history,
            global_input_history,
            pane_scope_picker_open: false,
            #[cfg(target_os = "macos")]
            caption_dock: frame::CaptionLayout::new(0.0),
            #[cfg(target_os = "macos")]
            canvas_depth_progress: 0.0,
            #[cfg(target_os = "macos")]
            canvas_surround: frame::CanvasSurround::default(),
            left_panel_motion: SidebarMotion::new(if session.left_panel_open.unwrap_or(true) {
                1.0
            } else {
                0.0
            }),
            tab_strip_motion: MotionValue::new(initial_tab_strip_target),
            tab_slot_motion: Default::default(),
            input_bar_visible: false,
            input_bar_motion: SidebarMotion::new(0.0),
            modal_was_open: false,
            ghostty_hidden: false,
            #[cfg(target_os = "macos")]
            native_tab_handoff_pending: false,
            sidebar_drag: None,
            terminal_theme,
            ghostty_app,
            last_ghostty_wake_generation,
            #[cfg(target_os = "macos")]
            chrome_transition_underlay_until: None,
            #[cfg(target_os = "linux")]
            linux_window_shape_signature: None,
            pending_create_pane_requests: Vec::new(),
            pending_window_control_requests: Vec::new(),
            surface_rename: None,
            control_request_rx,
            control_socket,
            next_tab_summary_id: next_tab_summary_id_init,
            runtime,
            agent_panel_open: false,
            agent_panel_width: 0.0,
            agent_panel_motion: SidebarMotion::new(0.0),
            agent_panel_drag: None,
            #[cfg(target_os = "macos")]
            agent_panel_snap_guard_until: None,
            #[cfg(target_os = "macos")]
            agent_panel_release_cover_until: None,
            window_handle: window.window_handle(),
            workspace_handle: cx.weak_entity(),
            window_close_prepared: false,
            session_save_tx,
            tab_rename: None,
            tab_rename_cancelled_generation: None,
            tab_rename_generation: 0,
            tab_strip_drop_slot: None,
            tab_drag_target: None,
            active_dragged_tab_session_id: std::sync::Arc::new(std::sync::Mutex::new(None)),
            tab_drag_preview: std::sync::Arc::new(std::sync::Mutex::new(None)),
            #[cfg(target_os = "macos")]
            top_bar_should_move: false,
            pane_title_drag: None,
            pane_content_bounds: std::sync::Arc::new(std::sync::Mutex::new(None)),
            tab_strip_tab_bounds: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            horizontal_tab_scroll: gpui::ScrollHandle::new(),
            horizontal_tab_reveal_key: Default::default(),
            horizontal_tab_edges: Default::default(),
            pane_title_drag_tab_bounds: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),

            // ── Code editor (Phase 1) ──────────────────────────────────────
            activity_bar: activity_bar_entity,
            activity_slot: ActivitySlot::from_str(
                session.activity_slot.as_deref().unwrap_or("files"),
            ),
            left_panel_open: session.left_panel_open.unwrap_or(true),
            sidebar_tools_open: !config.appearance.tabs_orientation.is_vertical()
                && session.left_panel_open.unwrap_or(true),
            file_tree_view: file_tree_entity,
            search_view: search_view_entity,
            git_view: git_view_entity,
            workspace_focus: cx.focus_handle(),
        };

        workspace.sync_color_scheme_appearance(window, cx);
        cx.observe_window_appearance(window, |workspace: &mut Self, window, cx| {
            workspace.sync_color_scheme_appearance(window, cx);
        })
        .detach();
        workspace
    }

    /// Create a new Ghostty terminal pane.
    pub(super) fn create_terminal(
        &mut self,
        cwd: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> TerminalPane {
        make_ghostty_terminal(
            &self.ghostty_app,
            cwd,
            None,
            None,
            None,
            self.font_size,
            self.config.experimental.handoff,
            window,
            cx,
        )
    }

    /// 分屏用的终端。
    ///
    /// 如果当前标签是远程连接打开的，新 pane 会用同一个连接再开一个**独立**
    /// ssh 会话，与本地分屏的行为保持一致：分屏是在当前状态基础上新建分支，
    /// 而不是退回本地 shell。
    ///
    /// 返回的 `bool` 表示新 pane 是否以 ssh 启动，供调用方上报运行时意图。
    pub(super) fn create_terminal_for_split(
        &mut self,
        cwd: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (TerminalPane, bool) {
        let server = self
            .tabs
            .get(self.active_tab)
            .and_then(|tab| tab.server.clone());
        let terminal = self.create_terminal(cwd, window, cx);
        let Some(server) = server else {
            return (terminal, false);
        };
        // 密码脚本写不出来时退回本地 shell，但要说一声：否则用户会以为
        // 分屏连上了服务器，实际拿到的是一个本地终端。
        let Some(command) = cube_core::connections::connect_command(&server) else {
            use gpui_component::{WindowExt as _, notification::Notification};
            window.push_notification(
                Notification::new()
                    .title("新分屏未能连接服务器")
                    .message(format!(
                        "无法准备 {} 的密码登录，已退回本地终端。\n请检查临时目录权限。",
                        server.name
                    )),
                cx,
            );
            return (terminal, false);
        };
        // surface 还没建好，这条会进 pending_write，初始化时冲刷到 PTY。
        terminal.write(format!("{command}\n").as_bytes(), cx);
        (terminal, true)
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn on_terminal_file_requested(
        &mut self,
        source: &Entity<GhosttyView>,
        link: &crate::terminal_file_link::TerminalFileLink,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Ignore stale/background events rather than opening in an unrelated tab.
        if self.tabs.get(self.active_tab).is_some_and(|tab| {
            tab.pane_tree
                .pane_id_for_entity(source.entity_id())
                .is_some()
        }) {
            self.open_path_at_in_active_editor(link.path.clone(), link.position, window, cx);
        }
    }

    pub(super) fn open_path_in_active_editor(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_path_in_active_editor_with_mode(path, None, true, window, cx);
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn open_path_at_in_active_editor(
        &mut self,
        path: PathBuf,
        position: Option<(usize, usize)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_path_in_active_editor_with_mode(path, position, false, window, cx);
    }

    fn open_path_in_active_editor_with_mode(
        &mut self,
        path: PathBuf,
        position: Option<(usize, usize)>,
        temporary: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.has_active_tab() {
            return;
        }

        let active_tab = self.active_tab;
        let (pane_id, editor_view) =
            if let Some(pane_id) = self.tabs[active_tab].pane_tree.find_editor_pane() {
                let Some(editor_view) = self.tabs[active_tab]
                    .pane_tree
                    .editor_view_for_pane(pane_id)
                else {
                    return;
                };
                (pane_id, editor_view)
            } else {
                let editor_font_size = self.font_size;
                let editor_view = cx.new(|cx| EditorView::new_with_font_size(editor_font_size, cx));
                cx.subscribe_in(
                    &editor_view,
                    window,
                    |this, _editor, _event: &ActiveFileChanged, _window, cx| {
                        this.sync_file_tree_from_active_focus(cx);
                    },
                )
                .detach();
                cx.subscribe_in(
                    &editor_view,
                    window,
                    |this, editor, _event: &EditorEmptied, window, cx| {
                        let Some((tab_index, pane_id)) =
                            this.tabs.iter().enumerate().find_map(|(tab_index, tab)| {
                                tab.pane_tree
                                    .pane_id_for_editor_view(editor)
                                    .map(|pane_id| (tab_index, pane_id))
                            })
                        else {
                            return;
                        };
                        if !this.close_pane_in_tab(tab_index, pane_id, window, cx) {
                            this.close_tab_by_index(tab_index, window, cx);
                        }
                    },
                )
                .detach();
                let Some(pane_id) = self.tabs[active_tab]
                    .pane_tree
                    .split_with_editor(editor_view.clone())
                else {
                    return;
                };
                (pane_id, editor_view)
            };

        self.tabs[active_tab].pane_tree.focus_pane(pane_id);
        let editor_focus = editor_view.read(cx).focus_handle(cx).clone();
        editor_view.update(cx, |editor: &mut EditorView, cx| {
            if temporary {
                editor.open_temporary_file(path.clone(), cx);
            } else {
                editor.open_file_at(path.clone(), position, cx);
            }
        });
        editor_focus.focus(window, cx);
        self.sync_file_tree_from_active_focus(cx);
        self.sync_sidebar(cx);
        cx.notify();
    }

    /// 在活动编辑区里打开一个 git diff 标签。
    ///
    /// 记住仓库根和变更信息，行号才能点回源文件。
    /// `is_working_tree` 决定是否启用自动刷新：提交 diff 是历史快照，不刷新。
    pub(super) fn open_diff_in_active_editor(
        &mut self,
        request: crate::editor_view::DiffRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.has_active_tab() {
            return;
        }
        let active_tab = self.active_tab;
        let (pane_id, editor_view) =
            if let Some(pane_id) = self.tabs[active_tab].pane_tree.find_editor_pane() {
                let Some(editor_view) = self.tabs[active_tab]
                    .pane_tree
                    .editor_view_for_pane(pane_id)
                else {
                    return;
                };
                (pane_id, editor_view)
            } else {
                let editor_view = self.create_editor_view_subscribed(window, cx);
                let Some(pane_id) = self.tabs[active_tab]
                    .pane_tree
                    .split_with_editor(editor_view.clone())
                else {
                    return;
                };
                (pane_id, editor_view)
            };

        self.tabs[active_tab].pane_tree.focus_pane(pane_id);
        let editor_focus = editor_view.read(cx).focus_handle(cx).clone();
        // 快照不对应磁盘文件，但标签需要一个不冲突的 key：同一个提交的
        // 不同文件、以及重复打开同一 diff 都可能撞名，加时间戳保证唯一。
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let key = std::path::PathBuf::from(format!("{}#{stamp}", request.title));
        editor_view.update(cx, |editor: &mut EditorView, cx| {
            // 走 diff 专用标签：增删行着色、新旧行号分栏，
            // 而不是把 unified diff 当普通文本显示。
            editor.open_diff_with_context(
                crate::editor_view::DiffRequest {
                    path: key,
                    ..request
                },
                cx,
            );
        });
        editor_focus.focus(window, cx);
        self.sync_sidebar(cx);
        cx.notify();
    }

    /// Create a new EditorView with font_size and subscribe to ActiveFileChanged and EditorEmptied events.
    pub(super) fn create_editor_view_subscribed(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<EditorView> {
        let editor_font_size = self.font_size;
        let editor_view = cx.new(|cx| EditorView::new_with_font_size(editor_font_size, cx));
        cx.subscribe_in(
            &editor_view,
            window,
            |this, editor, _event: &ActiveFileChanged, _window, cx| {
                if let Some(tab_index) = this
                    .tabs
                    .iter()
                    .position(|tab| tab.pane_tree.pane_id_for_editor_view(editor).is_some())
                {
                    // 快照的 path 是合成 key，直接拿它当标签标题会显示成
                    // 「某提交 某文件#时间戳」，保留原来的标题。
                    if !editor.read(cx).active_tab_is_snapshot() {
                        let active_file = editor.read(cx).active_path().map(Path::to_path_buf);
                        this.tabs[tab_index].title = editor_tab_title(active_file.as_deref());
                    }
                    this.sync_sidebar(cx);
                    this.save_session(cx);
                }
                this.sync_file_tree_from_active_focus(cx);
            },
        )
        .detach();
        cx.subscribe_in(
            &editor_view,
            window,
            |this, editor, _event: &EditorEmptied, window, cx| {
                let Some((tab_index, pane_id)) =
                    this.tabs.iter().enumerate().find_map(|(tab_index, tab)| {
                        tab.pane_tree
                            .pane_id_for_editor_view(editor)
                            .map(|pane_id| (tab_index, pane_id))
                    })
                else {
                    return;
                };
                if !this.close_pane_in_tab(tab_index, pane_id, window, cx) {
                    this.close_tab_by_index(tab_index, window, cx);
                }
            },
        )
        .detach();
        editor_view
    }

    /// Open a path in an editor tab, reusing existing editor tabs or creating a new one.
    pub(super) fn open_path_in_editor_tab(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab_ids: Vec<u64> = self.tabs.iter().map(|tab| tab.summary_id).collect();
        let is_editor_tab: Vec<bool> = self
            .tabs
            .iter()
            .map(|tab| tab.pane_tree.pane_terminals().is_empty())
            .collect();

        let active_tab = self.active_tab;
        if let Some(idx) = reusable_editor_tab_index(
            &tab_ids,
            &is_editor_tab,
            active_tab,
            self.last_editor_tab_id,
        ) {
            self.activate_tab(idx, window, cx);
            let pane_id = self.tabs[idx].pane_tree.find_editor_pane().unwrap();
            self.tabs[idx].pane_tree.focus_pane(pane_id);
            let editor_view = self.tabs[idx]
                .pane_tree
                .editor_view_for_pane(pane_id)
                .unwrap();
            let editor_focus = editor_view.read(cx).focus_handle(cx).clone();
            editor_view.update(cx, |editor: &mut EditorView, cx| {
                editor.open_file(path.clone(), cx);
            });
            editor_focus.focus(window, cx);
            self.sync_file_tree_from_active_focus(cx);
            self.save_session(cx);
        } else {
            self.new_editor_tab(path, window, cx);
        }
    }

    pub(super) fn horizontal_tabs_visible(&self) -> bool {
        horizontal_tab_strip_target(self.config.appearance.tabs_orientation, self.tabs.len()) > 0.5
    }

    pub(super) fn vertical_tabs_enabled(&self) -> bool {
        self.config.appearance.tabs_orientation == TabsOrientation::Vertical
    }

    pub(super) fn sync_tab_strip_motion(&mut self) -> bool {
        let target =
            horizontal_tab_strip_target(self.config.appearance.tabs_orientation, self.tabs.len());
        let changed = (self.tab_strip_motion.current() - target).abs() > 0.001;
        self.tab_strip_motion.set_target(
            target,
            Self::terminal_adjacent_chrome_duration(target > 0.5, 180, 180),
        );
        changed
    }

    pub(super) fn current_top_bar_height(&self) -> f32 {
        #[cfg(target_os = "macos")]
        {
            TOP_BAR_COMPACT_HEIGHT * self.caption_dock.header
        }
        #[cfg(not(target_os = "macos"))]
        if self.tab_strip_motion.is_animating() || self.horizontal_tabs_visible() {
            TOP_BAR_TABS_HEIGHT
        } else {
            TOP_BAR_COMPACT_HEIGHT
        }
    }

    #[allow(dead_code)]
    pub(super) fn active_terminal(&self) -> &TerminalPane {
        self.tabs[self.active_tab].pane_tree.focused_terminal()
    }

    pub(super) fn try_active_terminal(&self) -> Option<&TerminalPane> {
        self.tabs[self.active_tab].pane_tree.try_focused_terminal()
    }

    pub(super) fn refocus_active_terminal(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_active_tab() {
            self.focus_terminal(window, cx);
        }
    }

    pub(super) fn sync_active_terminal_focus_states(&self, cx: &mut App) {
        if !self.has_active_tab() {
            return;
        }

        let pane_tree = &self.tabs[self.active_tab].pane_tree;
        let surface_terminals = pane_tree.surface_terminals();
        let focused_id = pane_tree.focused_pane_id();

        let target_ids: HashSet<usize> = [focused_id].into_iter().collect();

        for (pane_id, is_active_surface, terminal) in surface_terminals {
            terminal.set_focus_state(is_active_surface && target_ids.contains(&pane_id), cx);
        }
    }

    pub(super) fn clear_terminal_focus_states_for_active_tab(&self, cx: &mut App) {
        if !self.has_active_tab() {
            return;
        }
        for (_, _, terminal) in self.tabs[self.active_tab].pane_tree.surface_terminals() {
            terminal.set_focus_state(false, cx);
        }
    }

    pub(super) fn sync_tab_native_view_visibility(
        &self,
        tab_index: usize,
        visible: bool,
        cx: &App,
    ) {
        let Some(tab) = self.tabs.get(tab_index) else {
            return;
        };
        let zoomed_pane_id = tab.pane_tree.zoomed_pane_id();

        for surface in tab.pane_tree.surface_infos(None) {
            let pane_visible =
                visible && zoomed_pane_id.is_none_or(|zoomed| zoomed == surface.pane_id);
            surface
                .terminal
                .set_native_view_visible(pane_visible && surface.is_active, cx);
        }
    }

    pub(super) fn sync_active_tab_native_view_visibility(&self, cx: &App) {
        if self.has_active_tab() {
            self.sync_tab_native_view_visibility(self.active_tab, true, cx);
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn commit_native_tab_handoff(&mut self, cx: &App) {
        if !std::mem::take(&mut self.native_tab_handoff_pending) {
            return;
        }
        // The incoming pane tree has completed native layout in this prepaint
        // pass. Retire all other tabs before paint, not one frame later.
        for (index, _) in self.tabs.iter().enumerate() {
            if index != self.active_tab {
                self.sync_tab_native_view_visibility(index, false, cx);
            }
        }
        if !self.ghostty_hidden && !self.is_modal_open(cx) {
            self.sync_active_tab_native_view_visibility(cx);
        } else {
            self.sync_tab_native_view_visibility(self.active_tab, false, cx);
        }
    }

    pub(super) fn hide_active_tab_native_views_not_visible_after_layout(&self, cx: &App) {
        if !self.has_active_tab() {
            return;
        }
        let tab = &self.tabs[self.active_tab];
        let zoomed_pane_id = tab.pane_tree.zoomed_pane_id();
        let focused_pane_id = tab.pane_tree.focused_pane_id();

        for surface in tab.pane_tree.surface_infos(None) {
            let pane_visible = zoomed_pane_id.is_none_or(|zoomed| zoomed == surface.pane_id);
            // Same-pane surface switches should keep the old surface visible
            // until the newly active one has a committed frame. Hiding it here
            // creates a one-frame blank pane. Active surfaces in other visible
            // panes should also stay visible; they are not leaving the layout.
            if !pane_visible || (!surface.is_active && surface.pane_id != focused_pane_id) {
                surface.terminal.set_native_view_visible(false, cx);
            }
        }
    }

    pub(super) fn notify_tab_terminal_views(&self, tab_index: usize, cx: &mut App) {
        let Some(tab) = self.tabs.get(tab_index) else {
            return;
        };
        for terminal in tab.pane_tree.all_terminals() {
            terminal.notify(cx);
        }
    }

    pub(super) fn notify_active_tab_terminal_views(&self, cx: &mut App) {
        if self.has_active_tab() {
            self.notify_tab_terminal_views(self.active_tab, cx);
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn mark_tab_terminal_native_layout_pending(&self, tab_index: usize, cx: &mut App) {
        let Some(tab) = self.tabs.get(tab_index) else {
            return;
        };
        for terminal in tab.pane_tree.all_terminals() {
            terminal.mark_native_layout_pending(cx);
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn mark_active_tab_terminal_native_layout_pending(&self, cx: &mut App) {
        if self.has_active_tab() {
            self.mark_tab_terminal_native_layout_pending(self.active_tab, cx);
        }
    }

    pub(super) fn sync_active_tab_native_view_visibility_after_layout(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Native Ghostty NSViews live outside GPUI's element tree. Hide
        // surfaces that are definitely no longer visible immediately, but
        // delay revealing newly-visible surfaces until GPUI has committed one
        // layout frame so they do not flash at stale split coordinates.
        self.hide_active_tab_native_views_not_visible_after_layout(cx);
        cx.on_next_frame(window, |_workspace, window, cx| {
            cx.notify();
            cx.on_next_frame(window, |workspace, _window, cx| {
                if workspace.has_active_tab()
                    && !workspace.ghostty_hidden
                    && !workspace.is_modal_open(cx)
                {
                    workspace.sync_active_tab_native_view_visibility(cx);
                }
            });
        });
    }

    pub(super) fn sync_active_tab_native_view_visibility_after_zoom_layout(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Zoom/unzoom should not hide anything before GPUI has produced the
        // new pane frames. Keeping the old visual state for one frame is less
        // visible than flashing the matte/fallback background.
        cx.on_next_frame(window, |workspace, _window, cx| {
            if workspace.has_active_tab()
                && !workspace.ghostty_hidden
                && !workspace.is_modal_open(cx)
            {
                workspace.sync_active_tab_native_view_visibility(cx);
            }
        });
    }

    pub(super) fn sync_active_tab_native_view_visibility_now_or_after_layout(
        &self,
        was_zoomed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if was_zoomed
            || self
                .tabs
                .get(self.active_tab)
                .and_then(|tab| tab.pane_tree.zoomed_pane_id())
                .is_some()
        {
            self.sync_active_tab_native_view_visibility_after_zoom_layout(window, cx);
            return;
        }
        self.sync_active_tab_native_view_visibility_after_layout(window, cx);
    }

    pub(super) fn pump_ghostty_views(&mut self, cx: &mut Context<Self>) -> bool {
        let started = perf_trace_enabled().then(std::time::Instant::now);
        let mut changed = false;
        let mut terminal_count = 0usize;
        let mut drain_count = 0usize;

        for tab in &self.tabs {
            for terminal in tab.pane_tree.all_surface_terminals() {
                terminal_count += 1;
                changed |= terminal.pump_surface_deferred_work(cx);
            }
        }

        let generation = self.ghostty_app.wake_generation();
        if generation == self.last_ghostty_wake_generation {
            for tab in &self.tabs {
                for terminal in tab.pane_tree.all_surface_terminals() {
                    drain_count += 1;
                    changed |= terminal.drain_surface_state_with_native_scroll(false, cx);
                }
            }
            if let Some(started) = started
                && changed
            {
                log::info!(
                    target: "con::perf",
                    "pump_ghostty_views generation_unchanged terminals={} drains={} changed=1 elapsed_ms={:.3}",
                    terminal_count,
                    drain_count,
                    started.elapsed().as_secs_f64() * 1000.0
                );
            }
            return changed;
        }

        self.last_ghostty_wake_generation = generation;
        for (tab_index, tab) in self.tabs.iter().enumerate() {
            let sync_native_scroll = tab_index == self.active_tab;
            for terminal in tab.pane_tree.all_surface_terminals() {
                drain_count += 1;
                changed |= terminal.drain_surface_state_with_native_scroll(sync_native_scroll, cx);
            }
        }

        if let Some(started) = started {
            log::info!(
                target: "con::perf",
                "pump_ghostty_views generation={} terminals={} drains={} changed={} elapsed_ms={:.3}",
                generation,
                terminal_count,
                drain_count,
                changed,
                started.elapsed().as_secs_f64() * 1000.0
            );
        }

        changed
    }

    pub(super) fn schedule_terminal_bootstrap_reassert(
        terminal: &TerminalPane,
        should_focus: bool,
        window_handle: AnyWindowHandle,
        workspace_handle: WeakEntity<Self>,
        cx: &mut Context<Self>,
    ) {
        let terminal = terminal.clone();
        cx.spawn(async move |_, cx| {
            for attempt in 0..8 {
                let delay_ms = if attempt == 0 { 16 } else { 250 };
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(delay_ms))
                    .await;
                let ready = window_handle
                    .update(cx, |_root, window, cx| {
                        let Some(workspace) = workspace_handle.upgrade() else {
                            return false;
                        };
                        workspace.update(cx, |_workspace, cx| {
                            let terminal_entity_id = terminal.entity_id();
                            let terminal_still_owned = _workspace.tabs.iter().any(|tab| {
                                tab.pane_tree
                                    .all_surface_terminals()
                                    .iter()
                                    .any(|candidate| candidate.entity_id() == terminal_entity_id)
                            });
                            if !terminal_still_owned {
                                terminal.set_focus_state(false, cx);
                                return true;
                            }

                            terminal.ensure_surface(window, cx);
                            terminal.notify(cx);
                            _workspace.sync_active_tab_native_view_visibility(cx);
                            if should_focus {
                                let still_active_terminal = _workspace
                                    .tabs
                                    .get(_workspace.active_tab)
                                    .is_some_and(|tab| {
                                        tab.pane_tree.all_terminals().iter().any(|candidate| {
                                            candidate.entity_id() == terminal_entity_id
                                        })
                                    });
                                if still_active_terminal {
                                    _workspace.sync_active_terminal_focus_states(cx);
                                    terminal.focus(window, cx);
                                } else {
                                    terminal.set_focus_state(false, cx);
                                }
                            } else {
                                terminal.set_focus_state(false, cx);
                            }
                            if terminal.surface_ready(cx) {
                                terminal.recover_shell_prompt_state(cx);
                                true
                            } else {
                                false
                            }
                        })
                    })
                    .unwrap_or(false);
                if ready {
                    break;
                }
            }
        })
        .detach();
    }

    pub(super) fn schedule_pending_create_pane_flush(&self, cx: &mut Context<Self>) {
        let window_handle = self.window_handle;
        let workspace_handle = self.workspace_handle.clone();
        cx.defer(move |cx| {
            let result = window_handle.update(cx, |_root, window, cx| {
                if let Some(workspace) = workspace_handle.upgrade() {
                    workspace.update(cx, |workspace, cx| {
                        workspace.flush_pending_window_control_requests(window, cx);
                        workspace.flush_pending_create_pane_requests(window, cx);
                    });
                }
            });
            if let Err(err) = result {
                log::warn!(
                    "[control] failed to flush deferred pane creation in a window-aware context: {err}"
                );
            }
        });
    }
}
