use super::*;

impl ConWorkspace {
    pub(super) fn record_shell_command(
        &mut self,
        tab_idx: usize,
        pane_id: usize,
        command: &str,
        cwd: Option<String>,
    ) {
        let trimmed = command.trim();
        if trimmed.is_empty() || tab_idx >= self.tabs.len() {
            return;
        }

        let history = self.tabs[tab_idx].shell_history.entry(pane_id).or_default();
        if let Some(existing_idx) = history.iter().position(|entry| entry.command == trimmed) {
            history.remove(existing_idx);
        }
        history.push_back(CommandSuggestionEntry {
            command: trimmed.to_string(),
            cwd: cwd.clone(),
        });
        while history.len() > MAX_SHELL_HISTORY_PER_PANE {
            history.pop_front();
        }

        if let Some(existing_idx) = self
            .global_shell_history
            .iter()
            .position(|entry| entry.command == trimmed)
        {
            self.global_shell_history.remove(existing_idx);
        }
        self.global_shell_history.push_back(CommandSuggestionEntry {
            command: trimmed.to_string(),
            cwd,
        });
        while self.global_shell_history.len() > MAX_GLOBAL_SHELL_HISTORY {
            self.global_shell_history.pop_front();
        }
    }

    /// Hook kept for call-site symmetry after recording a new shell command.
    pub(super) fn after_shell_command_recorded(&self, _cx: &App) {}

    pub(super) fn record_input_history(&mut self, input: &str) {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return;
        }

        if let Some(existing_idx) = self
            .global_input_history
            .iter()
            .position(|entry| entry == trimmed)
        {
            self.global_input_history.remove(existing_idx);
        }
        self.global_input_history.push_back(trimmed.to_string());
        while self.global_input_history.len() > MAX_GLOBAL_INPUT_HISTORY {
            self.global_input_history.pop_front();
        }
    }

    pub(super) fn recent_input_history(&self, limit: usize) -> Vec<String> {
        self.global_input_history
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }

    pub(super) fn history_completion_for_prefix(
        &self,
        prefix: &str,
        cwd: Option<&str>,
        mode: InputMode,
        is_remote: bool,
    ) -> Option<String> {
        let mut fallback: Option<String> = None;

        for entry in self.global_shell_history.iter().rev() {
            if entry.command == prefix || !entry.command.starts_with(prefix) {
                continue;
            }

            if cwd.is_some() && entry.cwd.as_deref() == cwd {
                return Some(entry.command.clone());
            }

            if fallback.is_none() {
                fallback = Some(entry.command.clone());
            }
        }

        if fallback.is_some() {
            return fallback;
        }

        self.global_input_history
            .iter()
            .rev()
            .find(|entry| {
                entry.as_str() != prefix
                    && entry.starts_with(prefix)
                    && (mode == InputMode::Shell
                        || con_terminal::looks_like_command(entry, is_remote))
            })
            .cloned()
    }

    pub(super) fn local_path_completion_for_prefix(
        &self,
        tab_idx: usize,
        pane_id: usize,
        input: &str,
        cx: &App,
    ) -> Option<LocalPathCompletion> {
        let pane_tree = &self.tabs.get(tab_idx)?.pane_tree;
        let terminal = pane_tree
            .pane_terminals()
            .into_iter()
            .find_map(|(id, terminal)| (id == pane_id).then_some(terminal))?;

        if self
            .effective_remote_host_for_tab(tab_idx, &terminal, cx)
            .is_some()
        {
            return None;
        }

        let cwd = terminal.current_dir(cx)?;
        let token_start = input
            .char_indices()
            .rev()
            .find_map(|(idx, ch)| ch.is_whitespace().then_some(idx + ch.len_utf8()))
            .unwrap_or(0);
        let token = &input[token_start..];
        if token.is_empty() {
            return None;
        }

        let head = input[..token_start].trim_end();
        let first_word = head.split_whitespace().next().unwrap_or_default();
        let completes_path = first_word == "cd"
            || token.starts_with('~')
            || token.starts_with('.')
            || token.contains('/');
        if !completes_path {
            return None;
        }

        let directories_only = first_word == "cd";
        let home_dir = dirs::home_dir();
        let (search_dir, dir_prefix, search_prefix) = if let Some(stripped) =
            token.strip_prefix("~/")
        {
            let home = home_dir?;
            match stripped.rsplit_once('/') {
                Some((dir, prefix)) => (home.join(dir), format!("~/{dir}/"), prefix.to_string()),
                None => (home, "~/".to_string(), stripped.to_string()),
            }
        } else if token == "~" {
            let home = home_dir?;
            (home, String::new(), "~".to_string())
        } else if let Some((dir, prefix)) = token.rsplit_once('/') {
            let base = if dir.is_empty() {
                PathBuf::from("/")
            } else if Path::new(dir).is_absolute() {
                PathBuf::from(dir)
            } else {
                PathBuf::from(&cwd).join(dir)
            };
            (base, format!("{dir}/"), prefix.to_string())
        } else {
            (PathBuf::from(&cwd), String::new(), token.to_string())
        };

        let mut matches = std::fs::read_dir(&search_dir)
            .ok()?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let file_type = entry.file_type().ok()?;
                if directories_only && !file_type.is_dir() {
                    return None;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                name.starts_with(&search_prefix)
                    .then_some((name, file_type.is_dir()))
            })
            .collect::<Vec<_>>();
        if matches.is_empty() {
            return None;
        }
        matches.sort_by(|a, b| a.0.cmp(&b.0));

        let matched_name = if matches.len() == 1 {
            let (name, is_dir) = &matches[0];
            let mut single = name.clone();
            if *is_dir {
                single.push('/');
            }
            single
        } else {
            let prefix = longest_common_prefix(matches.iter().map(|(name, _)| name.as_str()));
            if prefix.chars().count() <= search_prefix.chars().count() {
                let candidates = matches
                    .into_iter()
                    .map(|(name, is_dir)| {
                        let mut candidate = if token == "~" {
                            name
                        } else {
                            format!("{dir_prefix}{name}")
                        };
                        if is_dir {
                            candidate.push('/');
                        }
                        format!("{}{}", &input[..token_start], candidate)
                    })
                    .collect::<Vec<_>>();
                return Some(LocalPathCompletion::Candidates(candidates));
            }
            prefix
        };

        let completed_token = if token == "~" {
            matched_name
        } else {
            format!("{dir_prefix}{matched_name}")
        };

        Some(LocalPathCompletion::Inline(format!(
            "{}{}",
            &input[..token_start],
            completed_token
        )))
    }

    pub(super) fn refresh_input_suggestion(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.has_active_tab() {
            log::debug!(target: "con::suggestions", "skip suggestion: no active tab");
            self.input_bar
                .update(cx, |bar, _cx| bar.clear_completion_ui());
            return;
        }

        let (text, mode, target_ids) = self.input_bar.update(cx, |bar, cx| {
            (bar.current_text(cx), bar.mode(), bar.target_pane_ids())
        });

        let trimmed = text.trim();
        if trimmed.is_empty()
            || text.contains('\n')
            || trimmed.starts_with('/')
            || target_ids.len() != 1
        {
            log::debug!(
                target: "con::suggestions",
                "skip suggestion: empty={} multiline={} slash={} targets={}",
                trimmed.is_empty(),
                text.contains('\n'),
                trimmed.starts_with('/'),
                target_ids.len()
            );
            self.input_bar
                .update(cx, |bar, _cx| bar.clear_completion_ui());
            return;
        }

        let pane_id = target_ids[0];
        let pane_tree = &self.tabs[self.active_tab].pane_tree;
        let pane = pane_tree
            .pane_terminals()
            .into_iter()
            .find_map(|(id, terminal)| (id == pane_id).then_some(terminal));
        let cwd = pane.as_ref().and_then(|pane| pane.current_dir(cx));
        let is_remote = pane.as_ref().is_some_and(|terminal| {
            self.effective_remote_host_for_tab(self.active_tab, terminal, cx)
                .is_some()
        });

        if let Some(path_match) =
            self.local_path_completion_for_prefix(self.active_tab, pane_id, &text, cx)
        {
            match path_match {
                LocalPathCompletion::Inline(path_match) => {
                    log::debug!(
                        target: "con::suggestions",
                        "use path suggestion prefix={:?} completion={:?}",
                        text,
                        path_match
                    );
                    self.input_bar.update(cx, |bar, _cx| {
                        bar.set_path_inline_suggestion(&text, &path_match);
                    });
                }
                LocalPathCompletion::Candidates(candidates) => {
                    log::debug!(
                        target: "con::suggestions",
                        "use path candidates prefix={:?} count={}",
                        text,
                        candidates.len()
                    );
                    self.input_bar.update(cx, |bar, _cx| {
                        bar.set_path_completion_candidates(&text, candidates);
                    });
                }
            }
            return;
        }

        if let Some(history_match) =
            self.history_completion_for_prefix(&text, cwd.as_deref(), mode, is_remote)
        {
            log::debug!(
                target: "con::suggestions",
                "use history suggestion prefix={:?} completion={:?}",
                text,
                history_match
            );
            self.input_bar.update(cx, |bar, _cx| {
                bar.set_history_inline_suggestion(&text, &history_match);
            });
            return;
        }

        self.input_bar
            .update(cx, |bar, _cx| bar.clear_completion_ui());
    }

    #[cfg(test)]
    pub(super) fn new_tab_sync_policy_for_tests() -> NewTabSyncPolicy {
        NewTabSyncPolicy {
            activates_new_tab: true,
            syncs_sidebar: true,
            notifies_ui: true,
            syncs_native_visibility: true,
            reuses_shared_tab_activation_flow: true,
        }
    }

    pub(super) fn should_defer_top_chrome_refresh_when_tab_strip_appears() -> bool {
        cfg!(not(target_os = "macos"))
    }

    #[cfg(test)]
    pub(super) fn should_defer_top_chrome_refresh_when_tab_strip_appears_for_tests() -> bool {
        Self::should_defer_top_chrome_refresh_when_tab_strip_appears()
    }
}
