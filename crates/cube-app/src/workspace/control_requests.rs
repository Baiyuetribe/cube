use super::*;

impl CubeWorkspace {
    pub(super) fn spawn_control_terminal_exec(
        &mut self,
        tab_idx: usize,
        command: String,
        target: cube_core::PaneTarget,
        response_tx: tokio::sync::oneshot::Sender<ControlResult>,
        cx: &mut Context<Self>,
    ) {
        let (exec_response_tx, exec_response_rx) = crossbeam_channel::bounded(1);
        self.handle_terminal_exec_request_for_tab(
            tab_idx,
            TerminalExecRequest {
                command,
                working_dir: None,
                target: Self::pane_selector_from_target(target),
                response_tx: exec_response_tx,
            },
            cx,
        );

        self.runtime.spawn(async move {
            let result = match tokio::task::spawn_blocking(move || {
                exec_response_rx.recv_timeout(std::time::Duration::from_secs(240))
            })
            .await
            {
                Ok(Ok(response)) => Self::terminal_exec_response_to_control_result(response),
                Ok(Err(_)) => Err(ControlError::internal(
                    "Timed out waiting for the visible shell command to finish",
                )),
                Err(err) => Err(ControlError::internal(format!(
                    "Visible shell join failed: {err}"
                ))),
            };
            Self::send_control_result(response_tx, result);
        });
    }

    pub(super) fn handle_control_request(
        &mut self,
        request: ControlRequestEnvelope,
        cx: &mut Context<Self>,
    ) {
        let ControlRequestEnvelope {
            command,
            response_tx,
        } = request;

        match command {
            ControlCommand::SystemIdentify => {
                let result = serde_json::to_value(SystemIdentifyResult {
                    app: "con".to_string(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    socket_path: self
                        .control_socket
                        .as_ref()
                        .map(|handle| handle.path().display().to_string())
                        .unwrap_or_else(|| cube_core::control_socket_path().display().to_string()),
                    active_tab_index: self.active_tab + 1,
                    tab_count: self.tabs.len(),
                    methods: cube_core::control_methods(),
                })
                .map_err(|err| ControlError::internal(err.to_string()));
                Self::send_control_result(response_tx, result);
            }
            ControlCommand::SystemCapabilities => {
                Self::send_control_result(
                    response_tx,
                    Ok(json!({ "methods": cube_core::control_methods() })),
                );
            }
            ControlCommand::TabsList => {
                let tabs = self
                    .tabs
                    .iter()
                    .enumerate()
                    .map(|(idx, tab)| TabInfo {
                        index: idx + 1,
                        title: tab
                            .pane_tree
                            .try_focused_terminal()
                            .and_then(|t| t.title(cx))
                            .unwrap_or_else(|| tab.title.clone()),
                        is_active: idx == self.active_tab,
                        pane_count: tab.pane_tree.pane_count(),
                        focused_pane_id: tab.pane_tree.focused_pane_id(),
                        needs_attention: tab.needs_attention,
                    })
                    .collect::<Vec<_>>();
                Self::send_control_result(
                    response_tx,
                    Ok(json!({
                        "active_tab_index": self.active_tab + 1,
                        "tabs": tabs,
                    })),
                );
            }
            ControlCommand::TabsNew => {
                self.pending_window_control_requests
                    .push(PendingWindowControlRequest::TabsNew { response_tx });
                self.schedule_pending_create_pane_flush(cx);
            }
            ControlCommand::TabsClose { tab_index } => {
                match self.resolve_control_tab_index(tab_index) {
                    Ok(tab_idx) => {
                        self.pending_window_control_requests.push(
                            PendingWindowControlRequest::TabsClose {
                                tab_idx,
                                response_tx,
                            },
                        );
                        self.schedule_pending_create_pane_flush(cx);
                    }
                    Err(err) => Self::send_control_result(response_tx, Err(err)),
                }
            }
            ControlCommand::PanesList { tab_index } => {
                match self.resolve_control_tab_index(tab_index) {
                    Ok(tab_idx) => {
                        self.spawn_control_pane_query(
                            tab_idx,
                            cube_terminal::PaneQuery::List,
                            response_tx,
                            cx,
                        );
                    }
                    Err(err) => Self::send_control_result(response_tx, Err(err)),
                }
            }
            ControlCommand::PanesRead {
                tab_index,
                target,
                lines,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => self.spawn_control_pane_query(
                    tab_idx,
                    cube_terminal::PaneQuery::ReadContent {
                        target: Self::pane_selector_from_target(target),
                        lines,
                    },
                    response_tx,
                    cx,
                ),
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
            ControlCommand::PanesExec {
                tab_index,
                target,
                command,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => {
                    self.spawn_control_terminal_exec(tab_idx, command, target, response_tx, cx)
                }
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
            ControlCommand::PanesSendKeys {
                tab_index,
                target,
                keys,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => self.spawn_control_pane_query(
                    tab_idx,
                    cube_terminal::PaneQuery::SendKeys {
                        target: Self::pane_selector_from_target(target),
                        keys,
                    },
                    response_tx,
                    cx,
                ),
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
            ControlCommand::PanesCreate {
                tab_index,
                location,
                command,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => self.spawn_control_pane_query(
                    tab_idx,
                    cube_terminal::PaneQuery::CreatePane { command, location },
                    response_tx,
                    cx,
                ),
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
            ControlCommand::PanesWait {
                tab_index,
                target,
                timeout_secs,
                pattern,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => self.spawn_control_pane_query(
                    tab_idx,
                    cube_terminal::PaneQuery::WaitFor {
                        target: Self::pane_selector_from_target(target),
                        timeout_secs,
                        pattern,
                    },
                    response_tx,
                    cx,
                ),
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
            ControlCommand::PanesProbeShell { tab_index, target } => {
                match self.resolve_control_tab_index(tab_index) {
                    Ok(tab_idx) => self.spawn_control_pane_query(
                        tab_idx,
                        cube_terminal::PaneQuery::ProbeShellContext {
                            target: Self::pane_selector_from_target(target),
                        },
                        response_tx,
                        cx,
                    ),
                    Err(err) => Self::send_control_result(response_tx, Err(err)),
                }
            }
            ControlCommand::TreeGet { tab_index } => {
                match self.resolve_control_tab_index(tab_index) {
                    Ok(tab_idx) => {
                        let tabs = self
                        .tabs
                        .iter()
                        .enumerate()
                        .filter(|(idx, _)| tab_index.is_none() || *idx == tab_idx)
                        .map(|(idx, tab)| {
                            let panes = tab
                                .pane_tree
                                .pane_terminals()
                                .into_iter()
                                .enumerate()
                                .map(|(pane_index, (pane_id, terminal))| {
                                    json!({
                                        "pane_index": pane_index + 1,
                                        "pane_id": pane_id,
                                        "pane_ref": format!("pane:{}", pane_index + 1),
                                        "title": terminal.title(cx).unwrap_or_else(|| format!("Pane {}", pane_index + 1)),
                                        "is_focused": pane_id == tab.pane_tree.focused_pane_id(),
                                    })
                                })
                                .collect::<Vec<_>>();
                            json!({
                                "tab_index": idx + 1,
                                "is_active": idx == self.active_tab,
                                "title": tab.title,
                                "focused_pane_id": tab.pane_tree.focused_pane_id(),
                                "panes": panes,
                            })
                        })
                        .collect::<Vec<_>>();
                        Self::send_control_result(
                            response_tx,
                            Ok(json!({
                                "active_tab_index": self.active_tab + 1,
                                "tabs": tabs,
                            })),
                        );
                    }
                    Err(err) => Self::send_control_result(response_tx, Err(err)),
                }
            }
            ControlCommand::TmuxInspect { tab_index, target } => {
                match self.resolve_control_tab_index(tab_index) {
                    Ok(tab_idx) => self.spawn_control_pane_query(
                        tab_idx,
                        cube_terminal::PaneQuery::InspectTmux {
                            target: Self::pane_selector_from_target(target),
                        },
                        response_tx,
                        cx,
                    ),
                    Err(err) => Self::send_control_result(response_tx, Err(err)),
                }
            }
            ControlCommand::TmuxList { tab_index, target } => {
                match self.resolve_control_tab_index(tab_index) {
                    Ok(tab_idx) => self.spawn_control_pane_query(
                        tab_idx,
                        cube_terminal::PaneQuery::TmuxList {
                            pane: Self::pane_selector_from_target(target),
                        },
                        response_tx,
                        cx,
                    ),
                    Err(err) => Self::send_control_result(response_tx, Err(err)),
                }
            }
            ControlCommand::TmuxCapture {
                tab_index,
                pane,
                target,
                lines,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => self.spawn_control_pane_query(
                    tab_idx,
                    cube_terminal::PaneQuery::TmuxCapture {
                        pane: Self::pane_selector_from_target(pane),
                        target,
                        lines,
                    },
                    response_tx,
                    cx,
                ),
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
            ControlCommand::TmuxSendKeys {
                tab_index,
                pane,
                target,
                literal_text,
                key_names,
                append_enter,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => self.spawn_control_pane_query(
                    tab_idx,
                    cube_terminal::PaneQuery::TmuxSendKeys {
                        pane: Self::pane_selector_from_target(pane),
                        target,
                        literal_text,
                        key_names,
                        append_enter,
                    },
                    response_tx,
                    cx,
                ),
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
            ControlCommand::TmuxRun {
                tab_index,
                pane,
                target,
                location,
                command,
                window_name,
                cwd,
                detached,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => self.spawn_control_pane_query(
                    tab_idx,
                    cube_terminal::PaneQuery::TmuxRunCommand {
                        pane: Self::pane_selector_from_target(pane),
                        target,
                        location,
                        command,
                        window_name,
                        cwd,
                        detached,
                    },
                    response_tx,
                    cx,
                ),
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
        }
    }
}
