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
