use super::*;

impl ConWorkspace {
    pub(super) fn on_input_escape(
        &mut self,
        _input_bar: &Entity<InputBar>,
        _event: &EscapeInput,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pane_scope_picker_open {
            self.pane_scope_picker_open = false;
            cx.notify();
        }
    }

    pub(super) fn on_skill_autocomplete_changed(
        &mut self,
        _input_bar: &Entity<InputBar>,
        _event: &SkillAutocompleteChanged,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.notify();
    }

    pub(super) fn on_toggle_pane_scope_picker_requested(
        &mut self,
        _input_bar: &Entity<InputBar>,
        _event: &TogglePaneScopePickerRequested,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_pane_scope_picker(&TogglePaneScopePicker, window, cx);
    }

    pub(super) fn on_input_edited(
        &mut self,
        _input_bar: &Entity<InputBar>,
        _event: &InputEdited,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refresh_input_suggestion(window, cx);
        cx.notify();
    }

    pub(super) fn on_input_scope_changed(
        &mut self,
        _input_bar: &Entity<InputBar>,
        _event: &InputScopeChanged,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_active_terminal_focus_states(cx);
        cx.notify();
    }

    pub(super) fn on_input_submit(
        &mut self,
        input_bar: &Entity<InputBar>,
        _event: &SubmitInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pane_scope_picker_open = false;
        let (content, mode) = input_bar.update(cx, |bar, cx| {
            let content = bar.take_content(window, cx);
            bar.clear_completion_ui();
            (content, bar.mode())
        });

        if content.trim().is_empty() {
            return;
        }

        self.record_input_history(&content);
        let recent_inputs = self.recent_input_history(80);
        input_bar.update(cx, |bar, cx| bar.set_recent_commands(recent_inputs, cx));

        match mode {
            InputMode::Shell | InputMode::Smart => {
                self.execute_shell(&content, window, cx);
            }
        }

        cx.notify();
    }
}
