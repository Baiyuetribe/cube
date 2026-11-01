use super::*;

impl ConWorkspace {
    pub(super) fn flush_pending_window_control_requests(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pending = std::mem::take(&mut self.pending_window_control_requests);
        if pending.is_empty() {
            return;
        }

        for request in pending {
            match request {
                PendingWindowControlRequest::TabsNew { response_tx } => {
                    self.new_tab(&NewTab, window, cx);
                    let result = Ok(json!({
                        "active_tab_index": self.active_tab + 1,
                        "tab_count": self.tabs.len(),
                        "focused_pane_id": self.tabs[self.active_tab].pane_tree.focused_pane_id(),
                    }));
                    Self::send_control_result(response_tx, result);
                }
                PendingWindowControlRequest::TabsClose {
                    tab_idx,
                    response_tx,
                } => {
                    if self.tabs.len() <= 1 {
                        Self::send_control_result(
                            response_tx,
                            Err(ControlError::invalid_params(
                                "refusing to close the last tab over the control plane",
                            )),
                        );
                        continue;
                    }
                    if tab_idx >= self.tabs.len() {
                        Self::send_control_result(
                            response_tx,
                            Err(ControlError::invalid_params(format!(
                                "Tab index {} is out of range. Valid tabs are 1..={}.",
                                tab_idx + 1,
                                self.tabs.len()
                            ))),
                        );
                        continue;
                    }
                    self.close_tab_by_index(tab_idx, window, cx);
                    let result = Ok(json!({
                        "active_tab_index": self.active_tab + 1,
                        "tab_count": self.tabs.len(),
                        "closed_tab_index": tab_idx + 1,
                    }));
                    Self::send_control_result(response_tx, result);
                }
            }
        }
    }

    pub(super) fn flush_pending_create_pane_requests(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pending = std::mem::take(&mut self.pending_create_pane_requests);
        if pending.is_empty() {
            return;
        }

        let mut created_any = false;
        for req in pending {
            if req.tab_idx >= self.tabs.len() {
                let _ = req
                    .response_tx
                    .send(con_terminal::PaneResponse::Error(format!(
                        "Tab {} is no longer available.",
                        req.tab_idx + 1
                    )));
                continue;
            }

            let terminal = self.create_terminal(req.cwd.as_deref(), window, cx);
            let direction = match req.location {
                con_terminal::PaneCreateLocation::Right => SplitDirection::Horizontal,
                con_terminal::PaneCreateLocation::Down => SplitDirection::Vertical,
            };
            let was_zoomed = self.tabs[req.tab_idx].pane_tree.zoomed_pane_id().is_some();
            self.tabs[req.tab_idx]
                .pane_tree
                .split(direction, terminal.clone());
            let should_focus = req.tab_idx == self.active_tab;
            if should_focus {
                #[cfg(target_os = "macos")]
                self.mark_tab_terminal_native_layout_pending(req.tab_idx, cx);
                self.notify_tab_terminal_views(req.tab_idx, cx);
                terminal.focus(window, cx);
                self.sync_active_terminal_focus_states(cx);
                self.sync_active_tab_native_view_visibility_now_or_after_layout(
                    was_zoomed, window, cx,
                );
            } else {
                terminal.set_focus_state(false, cx);
            }
            Self::schedule_terminal_bootstrap_reassert(
                &terminal,
                should_focus,
                self.window_handle,
                self.workspace_handle.clone(),
                cx,
            );

            if let Some(cmd) = &req.command {
                let cmd_with_newline = format!("{}\n", cmd);
                terminal.write(cmd_with_newline.as_bytes(), cx);
            }
            self.record_runtime_event_for_terminal(
                req.tab_idx,
                &terminal,
                con_terminal::PaneRuntimeEvent::PaneCreated {
                    startup_command: req.command.clone(),
                },
            );

            let pane_tree = &self.tabs[req.tab_idx].pane_tree;
            let pane_index = pane_tree
                .all_terminals()
                .iter()
                .enumerate()
                .find(|(_, pane)| pane.entity_id() == terminal.entity_id())
                .map(|(idx, _)| idx + 1)
                .unwrap_or_else(|| pane_tree.pane_count());
            let pane_id = pane_tree
                .pane_id_for_terminal(&terminal)
                .unwrap_or(pane_index);
            let _ = req
                .response_tx
                .send(con_terminal::PaneResponse::PaneCreated {
                    pane_index,
                    pane_id,
                    surface_ready: terminal.surface_ready(cx),
                    is_alive: terminal.is_alive(cx),
                    has_shell_integration: terminal.has_shell_integration(cx),
                    observation_support: terminal.observation_support(cx),
                    foreground_process_group_id: terminal.foreground_process_group_id(cx),
                    tty_name: terminal.tty_name(cx),
                });
            created_any = true;
        }

        if created_any {
            self.save_session(cx);
        }
        cx.notify();
        window.refresh();
    }

    pub(super) fn reconcile_runtime_trackers_for_tab(&self, tab_idx: usize) {
        let pane_ids: HashSet<usize> = self.tabs[tab_idx]
            .pane_tree
            .pane_terminals()
            .into_iter()
            .map(|(pane_id, _)| pane_id)
            .collect();
        self.tabs[tab_idx]
            .runtime_trackers
            .borrow_mut()
            .retain(|pane_id, _| pane_ids.contains(pane_id));
        self.tabs[tab_idx]
            .runtime_cache
            .borrow_mut()
            .retain(|pane_id, _| pane_ids.contains(pane_id));
    }

    pub(super) fn observe_terminal_runtime_for_tab(
        &self,
        tab_idx: usize,
        terminal: &TerminalPane,
        recent_output_lines: usize,
        cx: &App,
    ) -> (
        con_terminal::PaneObservationFrame,
        con_terminal::PaneRuntimeState,
    ) {
        let pane_tree = &self.tabs[tab_idx].pane_tree;
        let pane_id = pane_tree
            .pane_id_for_terminal(terminal)
            .unwrap_or(usize::MAX);
        let observation = terminal.observation_frame(recent_output_lines, cx);
        let runtime = {
            let mut trackers = self.tabs[tab_idx].runtime_trackers.borrow_mut();
            let tracker = trackers.entry(pane_id).or_default();
            tracker.observe(observation.clone())
        };
        self.tabs[tab_idx]
            .runtime_cache
            .borrow_mut()
            .insert(pane_id, runtime.clone());
        (observation, runtime)
    }

    pub(super) fn cached_runtime_for_tab(
        &self,
        tab_idx: usize,
        terminal: &TerminalPane,
    ) -> Option<con_terminal::PaneRuntimeState> {
        let pane_id = self.tabs[tab_idx]
            .pane_tree
            .pane_id_for_terminal(terminal)?;
        self.tabs[tab_idx]
            .runtime_cache
            .borrow()
            .get(&pane_id)
            .cloned()
    }

    pub(super) fn record_runtime_event_for_terminal(
        &self,
        tab_idx: usize,
        terminal: &TerminalPane,
        event: con_terminal::PaneRuntimeEvent,
    ) {
        let pane_tree = &self.tabs[tab_idx].pane_tree;
        let pane_id = pane_tree
            .pane_id_for_terminal(terminal)
            .unwrap_or(usize::MAX);
        let mut trackers = self.tabs[tab_idx].runtime_trackers.borrow_mut();
        let tracker = trackers.entry(pane_id).or_default();
        tracker.record_action(event);
    }

    pub(super) fn resolve_pane_target_for_tab(
        &self,
        tab_idx: usize,
        selector: con_terminal::PaneSelector,
    ) -> Result<ResolvedPaneTarget, String> {
        let pane_tree = &self.tabs[tab_idx].pane_tree;
        let all_terminals = pane_tree.all_terminals();
        let focused_terminal = pane_tree.try_visible_focus_terminal();
        let focused_pane_id = focused_terminal
            .map(|(pane_id, _)| pane_id)
            .unwrap_or_else(|| pane_tree.focused_pane_id());
        let focused_pane = focused_terminal.map(|(_, terminal)| terminal.clone());
        let focused_pane_index = all_terminals
            .iter()
            .enumerate()
            .find(|(_, terminal)| pane_tree.pane_id_for_terminal(terminal) == Some(focused_pane_id))
            .map(|(idx, _)| idx + 1)
            .unwrap_or(1);

        let by_id = match selector.pane_id {
            Some(pane_id) => {
                let pane = all_terminals
                    .iter()
                    .enumerate()
                    .find_map(|(idx, terminal)| {
                        (pane_tree.pane_id_for_terminal(terminal) == Some(pane_id)).then(|| {
                            ResolvedPaneTarget {
                                pane: (*terminal).clone(),
                                pane_index: idx + 1,
                                pane_id,
                            }
                        })
                    })
                    .ok_or_else(|| {
                        format!(
                            "Pane id {} is no longer available in this tab. That pane was likely closed. Re-run list_panes to choose a new target.",
                            pane_id
                        )
                    })?;
                Some(pane)
            }
            None => None,
        };

        let by_index = match selector.pane_index {
            Some(index) => {
                if index == 0 || index > all_terminals.len() {
                    if let Some(from_id) = by_id.clone() {
                        log::warn!(
                            "Pane selector index {} is stale; continuing with pane_id {}",
                            index,
                            from_id.pane_id
                        );
                        None
                    } else {
                        return Err(format!(
                            "Pane index {} is no longer valid in the current layout. The pane layout changed or that pane was closed. Re-run list_panes and prefer pane_id for follow-up targeting.",
                            index,
                        ));
                    }
                } else {
                    let pane = all_terminals[index - 1].clone();
                    let pane_id = match pane_tree.pane_id_for_terminal(&pane) {
                        Some(pane_id) => pane_id,
                        None if by_id.is_some() => {
                            let from_id = by_id.as_ref().expect("checked is_some");
                            log::warn!(
                                "Pane selector index {} no longer resolves to a live pane; continuing with pane_id {}",
                                index,
                                from_id.pane_id
                            );
                            return Ok(from_id.clone());
                        }
                        None => {
                            return Err(format!(
                                "Pane index {} no longer resolves to a live pane. Re-run list_panes and prefer pane_id for follow-up targeting.",
                                index
                            ));
                        }
                    };
                    Some(ResolvedPaneTarget {
                        pane,
                        pane_index: index,
                        pane_id,
                    })
                }
            }
            None => None,
        };

        match (by_index, by_id) {
            (Some(from_index), Some(from_id)) => {
                if from_index.pane_id != from_id.pane_id {
                    log::warn!(
                        "Pane selector mismatch: pane_index {} resolved to pane_id {}, but caller also supplied pane_id {}; continuing with pane_id",
                        from_index.pane_index,
                        from_index.pane_id,
                        from_id.pane_id
                    );
                    return Ok(from_id);
                }
                Ok(from_id)
            }
            (Some(target), None) => Ok(target),
            (None, Some(target)) => Ok(target),
            (None, None) => {
                let pane = focused_pane.ok_or_else(|| {
                    "No terminal pane is focused in this tab. Use list_panes to select a target.".to_string()
                })?;
                Ok(ResolvedPaneTarget {
                    pane,
                    pane_index: focused_pane_index,
                    pane_id: focused_pane_id,
                })
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn spawn_shell_anchor_command<F>(
        &self,
        _tab_idx: usize,
        pane: TerminalPane,
        pane_index: usize,
        command: String,
        timeout_secs: u64,
        parse_response: F,
        response_tx: crossbeam_channel::Sender<con_terminal::PaneResponse>,
        cx: &mut Context<Self>,
    ) where
        F: Fn(Vec<String>) -> Result<con_terminal::PaneResponse, String> + Send + 'static,
    {
        let _ = pane.take_command_finished(cx);
        pane.write(format!("{command}\n").as_bytes(), cx);

        cx.spawn(async move |this, cx| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(250))
                    .await;

                let lines = this
                    .update(cx, |_, cx| pane.recent_lines(400, cx))
                    .unwrap_or_default();

                if let Ok(response) = parse_response(lines.clone()) {
                    let _ = this.update(cx, |_, cx| {
                        pane.recover_shell_prompt_state(cx);
                    });
                    let _ = response_tx.send(response);
                    return;
                }

                if std::time::Instant::now() >= deadline {
                    let excerpt = this
                        .update(cx, |_, cx| pane.recent_lines(120, cx).join("\n"))
                        .unwrap_or_default();
                    let _ = response_tx.send(con_terminal::PaneResponse::Error(format!(
                        "Shell-anchor command timed out in pane {} after {}s.\nRecent output:\n{}",
                        pane_index, timeout_secs, excerpt
                    )));
                    return;
                }

                let finished = this
                    .update(cx, |_, cx| {
                        pane.take_command_finished(cx).is_some() || !pane.is_busy(cx)
                    })
                    .unwrap_or(false);
                if !finished {
                    continue;
                }

                let excerpt = this
                    .update(cx, |_, cx| pane.recent_lines(120, cx).join("\n"))
                    .unwrap_or_default();
                let parse_err = parse_response(lines).err().unwrap_or_else(|| {
                    "shell-anchor markers were not observed in pane output".to_string()
                });
                let _ = response_tx.send(con_terminal::PaneResponse::Error(format!(
                    "Shell-anchor command in pane {} could not be parsed: {}\nRecent output:\n{}",
                    pane_index, parse_err, excerpt
                )));
                return;
            }
        })
        .detach();
    }

    pub(super) fn pane_blocks_shell_anchor_control(pane: &TerminalPane, cx: &App) -> bool {
        if !pane.is_busy(cx) {
            return false;
        }

        let observation = pane.observation_frame(40, cx);
        let prompt_like = observation
            .screen_hints
            .iter()
            .any(|hint| hint.kind == con_terminal::PaneObservationHintKind::PromptLikeInput);

        if prompt_like {
            pane.recover_shell_prompt_state(cx);
            return false;
        }

        true
    }

    pub(super) fn effective_remote_host_for_tab(
        &self,
        tab_idx: usize,
        terminal: &TerminalPane,
        cx: &App,
    ) -> Option<String> {
        self.cached_runtime_for_tab(tab_idx, terminal)
            .map(|runtime| runtime.remote_host)
            .unwrap_or_else(|| {
                self.observe_terminal_runtime_for_tab(tab_idx, terminal, 12, cx)
                    .1
                    .remote_host
            })
    }

    pub(super) fn resolve_control_tab_index(
        &self,
        tab_index: Option<usize>,
    ) -> Result<usize, ControlError> {
        match tab_index {
            Some(index) if index == 0 || index > self.tabs.len() => {
                Err(ControlError::invalid_params(format!(
                    "Tab index {} is out of range. Valid tabs are 1..={}.",
                    index,
                    self.tabs.len()
                )))
            }
            Some(index) => Ok(index - 1),
            None => Ok(self.active_tab),
        }
    }

    pub(super) fn tab_index_for_summary_id(&self, tab_id: u64) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.summary_id == tab_id)
    }

    pub(super) fn pane_selector_from_target(
        target: con_core::PaneTarget,
    ) -> con_terminal::PaneSelector {
        con_terminal::PaneSelector::new(target.pane_index, target.pane_id)
    }

    pub(super) fn send_control_result(
        response_tx: tokio::sync::oneshot::Sender<ControlResult>,
        result: ControlResult,
    ) {
        let _ = response_tx.send(result);
    }

    pub(super) fn pane_response_to_control_result(
        tab_idx: usize,
        response: con_terminal::PaneResponse,
    ) -> ControlResult {
        match response {
            con_terminal::PaneResponse::PaneList(panes) => Ok(json!({
                "tab_index": tab_idx + 1,
                "panes": panes,
            })),
            con_terminal::PaneResponse::Content(content) => Ok(json!({ "content": content })),
            con_terminal::PaneResponse::KeysSent => Ok(json!({ "status": "sent" })),
            con_terminal::PaneResponse::TmuxInfo(tmux) => Ok(json!({
                "tab_index": tab_idx + 1,
                "tmux": tmux,
            })),
            con_terminal::PaneResponse::TmuxList(snapshot) => Ok(json!({
                "tab_index": tab_idx + 1,
                "snapshot": snapshot,
            })),
            con_terminal::PaneResponse::TmuxCapture(capture) => Ok(json!({
                "tab_index": tab_idx + 1,
                "capture": capture,
            })),
            con_terminal::PaneResponse::TmuxExec(exec) => Ok(json!({
                "tab_index": tab_idx + 1,
                "exec": exec,
            })),
            con_terminal::PaneResponse::ShellProbe(shell) => Ok(json!({
                "tab_index": tab_idx + 1,
                "shell": shell,
            })),
            con_terminal::PaneResponse::SearchResults(matches) => Ok(json!({
                "matches": matches,
            })),
            con_terminal::PaneResponse::BusyStatus {
                surface_ready,
                is_alive,
                is_busy,
                has_shell_integration,
            } => Ok(json!({
                "surface_ready": surface_ready,
                "is_alive": is_alive,
                "is_busy": is_busy,
                "has_shell_integration": has_shell_integration,
            })),
            con_terminal::PaneResponse::WaitComplete { status, output } => Ok(json!({
                "status": status,
                "output": output,
            })),
            con_terminal::PaneResponse::PaneCreated {
                pane_index,
                pane_id,
                surface_ready,
                is_alive,
                has_shell_integration,
                observation_support,
                foreground_process_group_id,
                tty_name,
            } => Ok(json!({
                "tab_index": tab_idx + 1,
                "pane_index": pane_index,
                "pane_id": pane_id,
                "surface_ready": surface_ready,
                "is_alive": is_alive,
                "has_shell_integration": has_shell_integration,
                "observation_support": observation_support,
                "foreground_process_group_id": foreground_process_group_id,
                "tty_name": tty_name,
            })),
            con_terminal::PaneResponse::Error(err) => Err(ControlError::invalid_params(err)),
        }
    }

    pub(super) fn terminal_exec_response_to_control_result(
        response: TerminalExecResponse,
    ) -> ControlResult {
        if let Some(error) = response.completion_error_with_output() {
            return Err(ControlError::internal(error));
        }
        Ok(json!({
            "output": response.output,
            "exit_code": response.exit_code,
        }))
    }

    pub(super) fn spawn_control_pane_query(
        &mut self,
        tab_idx: usize,
        query: con_terminal::PaneQuery,
        response_tx: tokio::sync::oneshot::Sender<ControlResult>,
        cx: &mut Context<Self>,
    ) {
        let (pane_response_tx, pane_response_rx) = crossbeam_channel::bounded(1);
        self.handle_pane_request_for_tab(
            tab_idx,
            con_terminal::PaneRequest {
                query,
                response_tx: pane_response_tx,
            },
            cx,
        );

        self.runtime.spawn(async move {
            let result = match tokio::task::spawn_blocking(move || {
                pane_response_rx.recv_timeout(std::time::Duration::from_secs(240))
            })
            .await
            {
                Ok(Ok(response)) => Self::pane_response_to_control_result(tab_idx, response),
                Ok(Err(_)) => Err(ControlError::internal(
                    "Timed out waiting for the pane operation to finish",
                )),
                Err(err) => Err(ControlError::internal(format!(
                    "Pane operation join failed: {err}"
                ))),
            };
            Self::send_control_result(response_tx, result);
        });
    }
}
