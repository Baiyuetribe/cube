use super::*;

impl ConWorkspace {
    pub(super) fn activate_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.tabs.len() {
            return;
        }
        self.horizontal_tab_reveal_key.set(None);
        if index == self.active_tab {
            cx.notify();
            return;
        }
        let old_active = self.active_tab;
        self.active_tab = index;
        self.tabs[index].needs_attention = false;

        if self.tabs[index].pane_tree.pane_terminals().is_empty() {
            self.last_editor_tab_id = Some(self.tabs[index].summary_id);
        }

        #[cfg(target_os = "macos")]
        for terminal in self.tabs[old_active].pane_tree.all_surface_terminals() {
            terminal.set_focus_state(false, cx);
        }

        // Show new tab's ghostty NSViews and focus active surface
        for terminal in self.tabs[index].pane_tree.all_terminals() {
            terminal.ensure_surface(window, cx);
        }
        #[cfg(target_os = "macos")]
        {
            // Keep the outgoing tab until the incoming native layout is ready,
            // then exchange visibility before GPUI paints this frame.
            self.native_tab_handoff_pending = true;
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.sync_tab_native_view_visibility(index, true, cx);
            for terminal in self.tabs[old_active].pane_tree.all_surface_terminals() {
                terminal.set_focus_state(false, cx);
            }
            let old_terminals: Vec<TerminalPane> = self.tabs[old_active]
                .pane_tree
                .all_surface_terminals()
                .into_iter()
                .cloned()
                .collect();
            cx.on_next_frame(window, move |_workspace, _window, cx| {
                for terminal in &old_terminals {
                    terminal.set_native_view_visible(false, cx);
                }
            });
        }
        if let Some(focused) = self.tabs[index].pane_tree.try_focused_terminal() {
            focused.focus(window, cx);
            self.sync_active_terminal_focus_states(cx);
            Self::schedule_terminal_bootstrap_reassert(
                focused,
                true,
                self.window_handle,
                self.workspace_handle.clone(),
                cx,
            );
        } else {
            self.focus_active_editor_or_workspace(window, cx);
        }

        self.sync_file_tree_from_active_focus(cx);
        self.sync_sidebar(cx);
        self.save_session(cx);
        cx.notify();
    }

    pub(super) fn activate_tab_by_id(
        &mut self,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.tab_index_for_summary_id(tab_id) {
            self.activate_tab(index, window, cx);
        }
    }

    pub(super) fn next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1 {
            return;
        }
        let next = (self.active_tab + 1) % self.tabs.len();
        self.activate_tab(next, window, cx);
    }

    pub(super) fn previous_tab(
        &mut self,
        _: &PreviousTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tabs.len() <= 1 {
            return;
        }
        let prev = if self.active_tab == 0 {
            self.tabs.len() - 1
        } else {
            self.active_tab - 1
        };
        self.activate_tab(prev, window, cx);
    }

    pub(super) fn select_tab_index(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.tabs.len() || index == self.active_tab {
            return;
        }
        self.activate_tab(index, window, cx);
    }

    pub(super) fn select_tab_1(
        &mut self,
        _: &SelectTab1,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_tab_index(0, window, cx);
    }

    pub(super) fn select_tab_2(
        &mut self,
        _: &SelectTab2,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_tab_index(1, window, cx);
    }

    pub(super) fn select_tab_3(
        &mut self,
        _: &SelectTab3,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_tab_index(2, window, cx);
    }

    pub(super) fn select_tab_4(
        &mut self,
        _: &SelectTab4,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_tab_index(3, window, cx);
    }

    pub(super) fn select_tab_5(
        &mut self,
        _: &SelectTab5,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_tab_index(4, window, cx);
    }

    pub(super) fn select_tab_6(
        &mut self,
        _: &SelectTab6,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_tab_index(5, window, cx);
    }

    pub(super) fn select_tab_7(
        &mut self,
        _: &SelectTab7,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_tab_index(6, window, cx);
    }

    pub(super) fn select_tab_8(
        &mut self,
        _: &SelectTab8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_tab_index(7, window, cx);
    }

    pub(super) fn select_tab_9(
        &mut self,
        _: &SelectTab9,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_tab_index(8, window, cx);
    }

    /// Focus the active terminal (used after modal close, etc.)
    pub(super) fn focus_terminal(&self, window: &mut Window, cx: &mut App) {
        if let Some(terminal) = self.tabs[self.active_tab].pane_tree.try_focused_terminal() {
            terminal.focus(window, cx);
        } else {
            self.focus_active_editor_or_workspace(window, cx);
            return;
        }
        self.sync_active_terminal_focus_states(cx);
    }

    pub(super) fn focus_active_editor_or_workspace(&self, window: &mut Window, cx: &mut App) {
        if !self.has_active_tab() {
            return;
        }

        let pane_tree = &self.tabs[self.active_tab].pane_tree;
        let editor_view = pane_tree.editor_view_for_pane(pane_tree.focused_pane_id());
        match active_pane_focus_target(false, editor_view.is_some()) {
            ActivePaneFocusTarget::Editor => {
                if let Some(editor_view) = editor_view {
                    let focus_handle = editor_view.read(cx).focus_handle(cx).clone();
                    focus_handle.focus(window, cx);
                }
            }
            ActivePaneFocusTarget::Workspace => {
                self.workspace_focus.clone().focus(window, cx);
            }
            ActivePaneFocusTarget::Terminal => {}
        }
        self.sync_active_terminal_focus_states(cx);
    }

    pub(super) fn focus_surface_in_active_tab(
        &mut self,
        surface_id: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.has_active_tab() {
            return;
        }
        let was_zoomed = self.tabs[self.active_tab]
            .pane_tree
            .zoomed_pane_id()
            .is_some();
        #[cfg(not(target_os = "macos"))]
        let _ = was_zoomed;

        let changed = self.tabs[self.active_tab]
            .pane_tree
            .focus_surface(surface_id);
        if !changed {
            #[cfg(not(target_os = "macos"))]
            {
                return;
            }
            #[cfg(target_os = "macos")]
            if !self.tabs[self.active_tab]
                .pane_tree
                .surface_infos(None)
                .into_iter()
                .any(|surface| surface.surface_id == surface_id)
            {
                return;
            }
        }
        #[cfg(target_os = "macos")]
        self.mark_active_tab_terminal_native_layout_pending(cx);
        #[cfg(target_os = "macos")]
        self.notify_active_tab_terminal_views(cx);
        if let Some(terminal) = self.tabs[self.active_tab]
            .pane_tree
            .try_focused_terminal()
            .cloned()
        {
            terminal.ensure_surface(window, cx);
            #[cfg(target_os = "macos")]
            self.sync_active_tab_native_view_visibility_now_or_after_layout(was_zoomed, window, cx);
            #[cfg(not(target_os = "macos"))]
            self.sync_active_tab_native_view_visibility(cx);
            terminal.focus(window, cx);
            self.sync_file_tree_from_active_focus(cx);
        } else {
            #[cfg(target_os = "macos")]
            self.sync_active_tab_native_view_visibility_now_or_after_layout(was_zoomed, window, cx);
            #[cfg(not(target_os = "macos"))]
            self.sync_active_tab_native_view_visibility(cx);
        }
        if changed {
            self.sync_sidebar(cx);
            self.save_session(cx);
        }
        self.sync_active_terminal_focus_states(cx);
        cx.notify();
    }

    pub(super) fn sync_file_tree_from_active_focus(&mut self, cx: &mut Context<Self>) {
        if !self.has_active_tab() {
            return;
        }

        let pane_tree = &self.tabs[self.active_tab].pane_tree;
        let focused_pane_id = pane_tree.focused_pane_id();

        let current_root = self.file_tree_view.read(cx).root().map(Path::to_path_buf);
        // 快照标签（git diff）的 path 只是标签标题，磁盘上没有这个文件。
        // 拿它推导根会把文件树带到不存在的目录，Git 面板随后也失效。
        // 这种情况保持现有根不变，只是不再高亮某个文件。
        if pane_tree
            .editor_view_for_pane(focused_pane_id)
            .is_some_and(|view| view.read(cx).active_tab_is_snapshot())
        {
            self.file_tree_view
                .update(cx, |tree, cx| tree.set_active_path(None, cx));
            return;
        }

        let root = if let Some(path) = pane_tree.editor_active_path_for_pane(focused_pane_id, cx) {
            file_tree_root_for_focus(
                FileTreeFocusSource::Editor {
                    file_path: Some(path.as_path()),
                },
                current_root.as_deref(),
            )
            .map(|root| (root, Some(path)))
        } else {
            pane_tree
                .try_focused_terminal()
                .and_then(|terminal| terminal.current_dir(cx))
                .and_then(|cwd| {
                    file_tree_root_for_focus(
                        FileTreeFocusSource::Terminal {
                            cwd: Some(cwd.as_str()),
                        },
                        current_root.as_deref(),
                    )
                    .map(|root| (root, None))
                })
        };

        if let Some((root, active_path)) = root {
            let search_root = root.clone();
            let git_root = root.clone();
            self.file_tree_view.update(cx, |tree, cx| {
                tree.set_root(root, cx);
                tree.set_active_path(active_path, cx);
            });
            self.search_view.update(cx, |search, cx| {
                search.set_root(search_root, cx);
            });
            self.git_view.update(cx, |git, cx| {
                git.set_root(git_root, cx);
            });
        }
    }

    /// Focus an editor pane by pane_id (called from the editor pane click handler).
    pub(super) fn focus_pane_in_active_tab(
        &mut self,
        pane_id: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.has_active_tab() {
            return;
        }
        log::debug!(
            "[editor-focus] focus_pane_in_active_tab: pane={} old_focused={}",
            pane_id,
            self.tabs[self.active_tab].pane_tree.focused_pane_id()
        );
        self.tabs[self.active_tab].pane_tree.focus_pane(pane_id);
        self.clear_terminal_focus_states_for_active_tab(cx);
        self.sync_active_tab_native_view_visibility(cx);
        if self.vertical_tabs_enabled() {
            self.sync_sidebar(cx);
        }
        let editor_view = self.tabs[self.active_tab]
            .pane_tree
            .editor_view_for_pane(pane_id);
        self.sync_file_tree_from_active_focus(cx);
        // Do not call sync_active_terminal_focus_states() here: editor panes are not terminal
        // focus targets. Re-syncing terminal focus after clicking an editor can restore focus
        // to the previously focused terminal pane, which makes Cmd+W hit the terminal again.
        if let Some(editor_view) = editor_view {
            let focus_handle = editor_view.read(cx).focus_handle(cx).clone();
            focus_handle.focus(window, cx);
        } else {
            self.workspace_focus.clone().focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn restore_terminal_focus_after_modal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.has_active_tab() {
            return;
        }
        self.focus_terminal(window, cx);
        cx.on_next_frame(window, |workspace, window, cx| {
            if workspace.has_active_tab() && !workspace.settings_panel.read(cx).is_overlay_visible()
            {
                workspace.focus_terminal(window, cx);
                cx.notify();
            }
        });
    }

    /// Show or hide ghostty NSViews for z-order management.
    /// When showing, only the active tab's views are made visible.
    /// When hiding, all views are hidden (for modal overlays).
    pub(super) fn set_ghostty_views_visible(&self, visible: bool, cx: &App) {
        if visible {
            self.sync_active_tab_native_view_visibility(cx);
        } else {
            // Hide all tabs' views for modal z-order
            for tab in &self.tabs {
                for terminal in tab.pane_tree.all_surface_terminals() {
                    terminal.set_native_view_visible(false, cx);
                }
            }
        }
    }

    // ── Ghostty event handlers ──────────────────────────────

    pub(crate) fn on_terminal_focus_changed(
        &mut self,
        entity: &Entity<GhosttyView>,
        _event: &GhosttyFocusChanged,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entity_id = entity.entity_id();
        let pane_tree = &mut self.tabs[self.active_tab].pane_tree;
        if let Some(pane_id) = pane_tree.pane_id_for_entity(entity_id) {
            pane_tree.focus(pane_id);
            entity.focus_handle(cx).focus(window, cx);
            self.sync_active_terminal_focus_states(cx);
            if self.vertical_tabs_enabled() {
                self.sync_sidebar(cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn on_terminal_process_exited(
        &mut self,
        entity: &Entity<GhosttyView>,
        _event: &GhosttyProcessExited,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Find which tab contains the dead terminal surface (may not be the active tab).
        let entity_id = entity.entity_id();
        let tab_idx = self
            .tabs
            .iter()
            .position(|tab| tab.pane_tree.pane_id_for_entity(entity_id).is_some());
        let Some(tab_idx) = tab_idx else { return };

        let surface = self.tabs[tab_idx]
            .pane_tree
            .surface_infos(None)
            .into_iter()
            .find(|surface| surface.terminal.entity_id() == entity_id);
        let Some(surface) = surface else { return };
        self.record_runtime_event_for_terminal(
            tab_idx,
            &surface.terminal,
            con_terminal::PaneRuntimeEvent::ProcessExited,
        );

        let pane_surface_count = self.tabs[tab_idx]
            .pane_tree
            .surface_infos(Some(surface.pane_id))
            .len();
        if pane_surface_count > 1 {
            let should_focus_replacement =
                tab_idx == self.active_tab && surface.is_active && surface.is_focused_pane;
            if let Some(outcome) = self.tabs[tab_idx]
                .pane_tree
                .close_surface(surface.surface_id, false)
            {
                let terminal = outcome.terminal;
                terminal.set_native_view_visible(false, cx);
                terminal.shutdown_surface(Some(window), cx);
            }
            if tab_idx == self.active_tab {
                self.sync_active_tab_native_view_visibility(cx);
                if should_focus_replacement
                    && let Some(replacement) =
                        self.tabs[tab_idx].pane_tree.try_focused_terminal().cloned()
                {
                    replacement.ensure_surface(window, cx);
                    replacement.focus(window, cx);
                }
                self.sync_active_terminal_focus_states(cx);
            }
            if self.vertical_tabs_enabled() {
                self.sync_sidebar(cx);
            }
            self.save_session(cx);
            cx.notify();
            return;
        }

        if self.tabs[tab_idx].pane_tree.pane_count() > 1 {
            let _ = self.close_pane_in_tab(tab_idx, surface.pane_id, window, cx);
            // After closing the terminal pane, the tab may still have editor
            // panes alive. Only close the tab/window if no panes remain.
        } else if self.tabs.len() > 1 {
            // Last pane in this tab — close the tab.
            self.close_tab_by_index(tab_idx, window, cx);
        } else {
            // Last pane in this window — close this workspace only.
            // App-level quit would tear down sibling windows too.
            if self.is_quick_terminal {
                self.destroy_quick_terminal_window(window, cx);
                return;
            }
            self.close_window_from_last_tab(window, cx);
        }
        cx.notify();
    }

    pub(crate) fn on_terminal_title_changed(
        &mut self,
        entity: &Entity<GhosttyView>,
        event: &GhosttyTitleChanged,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Application-owned animation frames are not naming context changes.
        if event.content_changed {
            self.sync_sidebar(cx);
            cx.notify();
        }
        // Record title evidence immediately, but only a semantic status change
        // invalidates chrome. Individual application spinner frames do not.
        self.observe_terminal_title(entity, cx);
    }

    pub(crate) fn on_terminal_bell(
        &mut self,
        _entity: &Entity<GhosttyView>,
        _event: &GhosttyBell,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        window.play_system_bell();
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    pub(crate) fn on_terminal_desktop_notification(
        &mut self,
        _entity: &Entity<GhosttyView>,
        event: &GhosttyDesktopNotification,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = if event.0.title.is_empty() {
            "Con".to_owned()
        } else {
            event.0.title.clone()
        };
        let body = event.0.body.clone();

        #[cfg(target_os = "linux")]
        {
            let notification_id = format!("co.nowledge.con.terminal.{}", uuid::Uuid::new_v4());
            cx.spawn(async move |_, _| {
                use ashpd::desktop::notification::{Notification, NotificationProxy};

                let proxy = match NotificationProxy::new().await {
                    Ok(proxy) => proxy,
                    Err(err) => {
                        log::warn!("Failed to connect to desktop notification portal: {err}");
                        return;
                    }
                };
                let notification =
                    Notification::new(&title).body((!body.is_empty()).then_some(body.as_str()));
                if let Err(err) = proxy.add_notification(&notification_id, notification).await {
                    log::warn!("Failed to show desktop notification: {err}");
                }
            })
            .detach();
        }

        #[cfg(target_os = "windows")]
        {
            use gpui_component::{WindowExt as _, notification::Notification};

            _window.push_notification(Notification::new().title(title).message(body), cx);
            if !_window.is_window_active() {
                flash_windows_taskbar(_window);
            }
        }
    }

    pub(crate) fn on_terminal_progress_changed(
        &mut self,
        entity: &Entity<GhosttyView>,
        _event: &GhosttyProgressChanged,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entity_id = entity.entity_id();
        if !self
            .tabs
            .iter()
            .any(|tab| tab.pane_tree.focused_terminal_entity_id() == Some(entity_id))
        {
            return;
        }
        if self.vertical_tabs_enabled() {
            self.sync_sidebar(cx);
        }
        cx.notify();
    }

    pub(crate) fn on_terminal_cwd_changed(
        &mut self,
        _entity: &Entity<GhosttyView>,
        _event: &GhosttyCwdChanged,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Shell integration reports cwd independently from title/output.
        // Persist immediately so restart continuity survives a later crash or
        // force-quit instead of depending on an unrelated tab/layout save.
        self.sync_sidebar(cx);
        self.save_session(cx);
        // Keep file tree root in sync with the active focused pane.
        self.sync_file_tree_from_active_focus(cx);
        let _active_focused_terminal = self
            .tabs
            .get(self.active_tab)
            .and_then(|tab| tab.pane_tree.focused_terminal_entity_id());
        cx.notify();
    }

    pub(crate) fn on_terminal_split_requested(
        &mut self,
        entity: &Entity<GhosttyView>,
        event: &GhosttySplitRequested,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entity_id = entity.entity_id();
        let Some(tab_idx) = self
            .tabs
            .iter()
            .position(|tab| tab.pane_tree.pane_id_for_entity(entity_id).is_some())
        else {
            return;
        };
        let Some(origin_pane_id) = self.tabs[tab_idx].pane_tree.pane_id_for_entity(entity_id)
        else {
            return;
        };

        let (direction, placement) = match event.0 {
            con_ghostty::GhosttySplitDirection::Right => {
                (SplitDirection::Horizontal, SplitPlacement::After)
            }
            con_ghostty::GhosttySplitDirection::Down => {
                (SplitDirection::Vertical, SplitPlacement::After)
            }
            con_ghostty::GhosttySplitDirection::Left => {
                (SplitDirection::Horizontal, SplitPlacement::Before)
            }
            con_ghostty::GhosttySplitDirection::Up => {
                (SplitDirection::Vertical, SplitPlacement::Before)
            }
        };

        let origin_terminal = self.tabs[tab_idx]
            .pane_tree
            .surface_infos(Some(origin_pane_id))
            .into_iter()
            .find_map(|surface| {
                (surface.terminal.entity_id() == entity_id).then_some(surface.terminal)
            });
        let cwd = origin_terminal
            .as_ref()
            .and_then(|terminal| terminal.current_dir(cx));

        let terminal = self.create_terminal(cwd.as_deref(), window, cx);
        let was_zoomed = self.tabs[tab_idx].pane_tree.zoomed_pane_id().is_some();
        self.tabs[tab_idx].pane_tree.split_pane_with_placement(
            origin_pane_id,
            direction,
            placement,
            terminal.clone(),
        );
        let tab_was_active = tab_idx == self.active_tab;
        #[cfg(target_os = "macos")]
        self.mark_tab_terminal_native_layout_pending(tab_idx, cx);
        self.notify_tab_terminal_views(tab_idx, cx);
        if tab_was_active {
            terminal.focus(window, cx);
            self.sync_active_terminal_focus_states(cx);
            self.sync_active_tab_native_view_visibility_now_or_after_layout(was_zoomed, window, cx);
        } else {
            terminal.set_focus_state(false, cx);
            self.sync_tab_native_view_visibility(tab_idx, false, cx);
        }
        Self::schedule_terminal_bootstrap_reassert(
            &terminal,
            tab_was_active,
            self.window_handle,
            self.workspace_handle.clone(),
            cx,
        );
        self.record_runtime_event_for_terminal(
            tab_idx,
            &terminal,
            con_terminal::PaneRuntimeEvent::PaneCreated {
                startup_command: None,
            },
        );
        self.save_session(cx);
        cx.notify();
    }

    /// Duplicate the tab at `index`, preserving pane layout and each pane's CWD.
    pub(super) fn duplicate_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.tabs.len() {
            return;
        }

        // Serialize the current pane tree layout (with cwd for every pane).
        let Some(layout) = self.tabs[index].pane_tree.to_persisted_state(cx, false) else {
            return;
        };
        let focused_pane_id = Some(self.tabs[index].pane_tree.focused_pane_id());

        // Rebuild the pane tree from the serialized layout, spawning a fresh
        // terminal in each pane's cwd.
        let ghostty_app = self.ghostty_app.clone();
        let font_size = self.font_size;
        let handoff_menu_entry_enabled = self.config.experimental.handoff;
        let mut make_terminal =
            |cwd: Option<&str>, _screen_text: Option<&[String]>, _force: bool| {
                make_ghostty_terminal(
                    &ghostty_app,
                    cwd,
                    None,
                    None,
                    None,
                    font_size,
                    handoff_menu_entry_enabled,
                    window,
                    cx,
                )
            };
        let pane_tree = PaneTree::from_state(&layout, focused_pane_id, &mut make_terminal);

        let summary_id = self.next_tab_summary_id;
        self.next_tab_summary_id += 1;
        let new_tab = Tab {
            pane_tree,
            server: None,
            title: format!("Terminal {}", summary_id + 1),
            user_label: self.tabs[index].user_label.clone(),
            agent_cli: None,
            color: self.tabs[index].color,
            summary_id,
            needs_attention: false,
            runtime_trackers: RefCell::new(HashMap::new()),
            runtime_cache: RefCell::new(HashMap::new()),
            shell_history: HashMap::new(),
        };
        let insert_at = index + 1;
        self.reveal_vertical_tab_rail_for_new_tab_if_needed(cx);
        self.tabs.insert(insert_at, new_tab);
        if self.active_tab >= insert_at {
            self.active_tab += 1;
        }
        self.sync_tab_strip_motion();
        self.activate_tab(insert_at, window, cx);
        cx.notify();
    }

    pub(super) fn duplicate_tab_by_id(
        &mut self,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.tab_index_for_summary_id(tab_id) {
            self.duplicate_tab(index, window, cx);
        }
    }

    /// Create a new editor tab with the given file path.
    pub(super) fn new_editor_tab(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor_view = self.create_editor_view_subscribed(window, cx);
        let pane_tree = PaneTree::new_editor(editor_view.clone());

        let summary_id = self.next_tab_summary_id;
        self.next_tab_summary_id += 1;
        let new_tab = Tab {
            pane_tree,
            server: None,
            title: editor_tab_title(None),
            user_label: None,
            agent_cli: None,
            color: None,
            summary_id,
            needs_attention: false,
            runtime_trackers: RefCell::new(HashMap::new()),
            runtime_cache: RefCell::new(HashMap::new()),
            shell_history: HashMap::new(),
        };

        let active_tab = self.active_tab;
        let insert_at = active_tab + 1;
        self.reveal_vertical_tab_rail_for_new_tab_if_needed(cx);
        self.tabs.insert(insert_at, new_tab);
        if self.active_tab >= insert_at {
            self.active_tab += 1;
        }
        self.sync_tab_strip_motion();
        self.activate_tab(insert_at, window, cx);

        let editor_focus = editor_view.read(cx).focus_handle(cx).clone();
        editor_view.update(cx, |editor: &mut EditorView, cx| {
            editor.open_file(path, cx);
        });
        editor_focus.focus(window, cx);
        self.sync_file_tree_from_active_focus(cx);
        self.save_session(cx);
        cx.notify();
    }

    /// Close all tabs except the one at `index`.
    pub(super) fn close_other_tabs(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.tabs.len() || self.tabs.len() <= 1 {
            return;
        }
        // Close from the end to avoid index shifting.
        for i in (0..self.tabs.len()).rev() {
            if i != index {
                self.close_tab_by_index(i, window, cx);
            }
        }
        cx.notify();
    }

    pub(super) fn close_other_tabs_by_id(
        &mut self,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.tab_index_for_summary_id(tab_id) {
            self.close_other_tabs(index, window, cx);
        }
    }

    /// Close all tabs to the right of `index` (exclusive).
    pub(super) fn close_tabs_to_right(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.tabs.len().saturating_sub(1) {
            return;
        }
        // Close from the end to avoid index shifting.
        let last = self.tabs.len() - 1;
        for i in (index + 1..=last).rev() {
            self.close_tab_by_index(i, window, cx);
        }
        cx.notify();
    }

    pub(super) fn close_tabs_to_right_by_id(
        &mut self,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.tab_index_for_summary_id(tab_id) {
            self.close_tabs_to_right(index, window, cx);
        }
    }

    /// Set (or clear) the accent color for a tab by index.
    pub(super) fn set_tab_color(
        &mut self,
        index: usize,
        color: Option<con_core::session::TabAccentColor>,
        cx: &mut Context<Self>,
    ) {
        if index >= self.tabs.len() {
            return;
        }
        self.tabs[index].color = color;
        self.save_session(cx);
        self.sync_sidebar(cx);
        cx.notify();
    }

    pub(super) fn set_tab_color_by_id(
        &mut self,
        tab_id: u64,
        color: Option<con_core::session::TabAccentColor>,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.tab_index_for_summary_id(tab_id) {
            self.set_tab_color(index, color, cx);
        }
    }
}

#[cfg(target_os = "windows")]
fn flash_windows_taskbar(window: &Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        FLASHW_TIMERNOFG, FLASHW_TRAY, FLASHWINFO, FlashWindowEx,
    };

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let info = FLASHWINFO {
        cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
        hwnd: HWND(handle.hwnd.get() as *mut std::ffi::c_void),
        dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG,
        uCount: 0,
        dwTimeout: 0,
    };
    unsafe {
        let _ = FlashWindowEx(&info);
    }
}
