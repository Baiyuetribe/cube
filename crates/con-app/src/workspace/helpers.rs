use super::*;

pub(super) enum FileTreeFocusSource<'a> {
    Terminal { cwd: Option<&'a str> },
    Editor { file_path: Option<&'a Path> },
}

pub(super) fn file_tree_root_for_focus(
    source: FileTreeFocusSource<'_>,
    current_root: Option<&Path>,
) -> Option<PathBuf> {
    match source {
        FileTreeFocusSource::Terminal { cwd } => cwd.map(PathBuf::from),
        FileTreeFocusSource::Editor { file_path } => {
            let file_path = file_path?;
            if let Some(root) = current_root
                && file_path.starts_with(root)
            {
                return Some(root.to_path_buf());
            }
            file_path.parent().map(Path::to_path_buf)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub(super) enum WorkspaceCloseIntent {
    CloseEditorFile,
    ClosePane,
    CloseTab,
    CloseWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EditorFileCloseOutcome {
    KeepEditorPane,
    CloseEditorPane,
    ReplaceEditorPaneWithTerminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EditorLineBoundary {
    Start,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ActivePaneFocusTarget {
    Terminal,
    Editor,
    Workspace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ActivationFocusTarget {
    InputBar,
    ActiveEditor,
    Terminal,
}

pub(super) fn active_pane_focus_target(
    has_focused_terminal: bool,
    has_focused_editor: bool,
) -> ActivePaneFocusTarget {
    if has_focused_terminal {
        ActivePaneFocusTarget::Terminal
    } else if has_focused_editor {
        ActivePaneFocusTarget::Editor
    } else {
        ActivePaneFocusTarget::Workspace
    }
}

pub(super) fn activation_focus_target_for_reassertion(
    input_bar_focused: bool,
    editor_has_keyboard_focus: bool,
    active_pane_is_editor: bool,
) -> ActivationFocusTarget {
    if input_bar_focused {
        ActivationFocusTarget::InputBar
    } else if editor_has_keyboard_focus || active_pane_is_editor {
        ActivationFocusTarget::ActiveEditor
    } else {
        ActivationFocusTarget::Terminal
    }
}

pub(super) fn resize_drag_should_continue(pressed_button: Option<MouseButton>) -> bool {
    pressed_button == Some(MouseButton::Left)
}

pub(super) fn workspace_close_intent(
    pane_count: usize,
    editor_file_tabs: Option<usize>,
    tab_count: usize,
) -> WorkspaceCloseIntent {
    if matches!(editor_file_tabs, Some(count) if count > 0) {
        return WorkspaceCloseIntent::CloseEditorFile;
    }
    if pane_count > 1 {
        return WorkspaceCloseIntent::ClosePane;
    }
    if tab_count > 1 {
        return WorkspaceCloseIntent::CloseTab;
    }
    WorkspaceCloseIntent::CloseWindow
}

pub(super) fn workspace_close_intent_for_close_tab(
    pane_count: usize,
    keyboard_focused_editor_file_tabs: Option<usize>,
    focused_pane_editor_file_tabs: Option<usize>,
    tab_count: usize,
) -> WorkspaceCloseIntent {
    workspace_close_intent(
        pane_count,
        keyboard_focused_editor_file_tabs.or(focused_pane_editor_file_tabs),
        tab_count,
    )
}

pub(super) fn editor_file_close_outcome(
    pane_count: usize,
    editor_pane_empty: bool,
) -> EditorFileCloseOutcome {
    if !editor_pane_empty {
        return EditorFileCloseOutcome::KeepEditorPane;
    }
    if pane_count > 1 {
        return EditorFileCloseOutcome::CloseEditorPane;
    }
    EditorFileCloseOutcome::ReplaceEditorPaneWithTerminal
}

pub(super) fn editor_line_boundary_for_key(
    key: &str,
    control: bool,
    platform: bool,
    alt: bool,
    shift: bool,
) -> Option<EditorLineBoundary> {
    if !control || platform || alt || shift {
        return None;
    }

    match key {
        "a" => Some(EditorLineBoundary::Start),
        "e" => Some(EditorLineBoundary::End),
        _ => None,
    }
}

pub(super) fn focused_editor_pane_key_fallback_allowed(input_surface_focused: bool) -> bool {
    !input_surface_focused
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn should_show_activity_bar(
    left_sidebar_available: bool,
    _activity_slot: ActivitySlot,
) -> bool {
    left_sidebar_available
}

pub(super) fn point_in_bounds(
    p: &gpui::Point<gpui::Pixels>,
    b: &gpui::Bounds<gpui::Pixels>,
) -> bool {
    p.x >= b.origin.x
        && p.x < b.origin.x + b.size.width
        && p.y >= b.origin.y
        && p.y < b.origin.y + b.size.height
}

pub(super) fn pane_split_drop_target_from_position(
    source_pane_id: usize,
    cursor: gpui::Point<gpui::Pixels>,
    panes: &[(usize, gpui::Bounds<gpui::Pixels>)],
) -> Option<PaneSplitDropTarget> {
    panes.iter().find_map(|(target_pane_id, bounds)| {
        if *target_pane_id == source_pane_id || !point_in_bounds(&cursor, bounds) {
            return None;
        }

        let width = bounds.size.width.as_f32().max(1.0);
        let height = bounds.size.height.as_f32().max(1.0);
        let local_x = (cursor.x - bounds.origin.x).as_f32();
        let local_y = (cursor.y - bounds.origin.y).as_f32();
        let left = local_x / width;
        let right = 1.0 - left;
        let top = local_y / height;
        let bottom = 1.0 - top;
        let candidates = [
            (top, SplitDirection::Vertical, SplitPlacement::Before),
            (bottom, SplitDirection::Vertical, SplitPlacement::After),
            (left, SplitDirection::Horizontal, SplitPlacement::Before),
            (right, SplitDirection::Horizontal, SplitPlacement::After),
        ];
        let (_, direction, placement) = candidates
            .into_iter()
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))?;

        Some(PaneSplitDropTarget {
            target_pane_id: *target_pane_id,
            direction,
            placement,
            bounds: *bounds,
        })
    })
}

pub(super) fn split_preview_local_rect(
    bounds: Bounds<Pixels>,
    content_bounds: Bounds<Pixels>,
) -> (f32, f32, f32, f32) {
    let left = (bounds.origin.x - content_bounds.origin.x).as_f32();
    let top = (bounds.origin.y - content_bounds.origin.y).as_f32();
    (
        left,
        top,
        bounds.size.width.as_f32(),
        bounds.size.height.as_f32(),
    )
}

pub(super) fn split_preview_regions(
    bounds: Bounds<Pixels>,
    direction: SplitDirection,
    placement: SplitPlacement,
) -> SplitPreviewRegions {
    let seam_thickness = px(SPLIT_PREVIEW_SEAM_THICKNESS);
    match (direction, placement) {
        (SplitDirection::Vertical, SplitPlacement::Before) => {
            let incoming_height = bounds.size.height * 0.5;
            let seam_top = bounds.origin.y + incoming_height - seam_thickness * 0.5;
            SplitPreviewRegions {
                incoming: Bounds::new(bounds.origin, size(bounds.size.width, incoming_height)),
                existing: Bounds::new(
                    point(bounds.origin.x, bounds.origin.y + incoming_height),
                    size(bounds.size.width, bounds.size.height - incoming_height),
                ),
                seam: Bounds::new(
                    point(bounds.origin.x, seam_top),
                    size(bounds.size.width, seam_thickness),
                ),
            }
        }
        (SplitDirection::Vertical, SplitPlacement::After) => {
            let existing_height = bounds.size.height * 0.5;
            let seam_top = bounds.origin.y + existing_height - seam_thickness * 0.5;
            SplitPreviewRegions {
                existing: Bounds::new(bounds.origin, size(bounds.size.width, existing_height)),
                incoming: Bounds::new(
                    point(bounds.origin.x, bounds.origin.y + existing_height),
                    size(bounds.size.width, bounds.size.height - existing_height),
                ),
                seam: Bounds::new(
                    point(bounds.origin.x, seam_top),
                    size(bounds.size.width, seam_thickness),
                ),
            }
        }
        (SplitDirection::Horizontal, SplitPlacement::Before) => {
            let incoming_width = bounds.size.width * 0.5;
            let seam_left = bounds.origin.x + incoming_width - seam_thickness * 0.5;
            SplitPreviewRegions {
                incoming: Bounds::new(bounds.origin, size(incoming_width, bounds.size.height)),
                existing: Bounds::new(
                    point(bounds.origin.x + incoming_width, bounds.origin.y),
                    size(bounds.size.width - incoming_width, bounds.size.height),
                ),
                seam: Bounds::new(
                    point(seam_left, bounds.origin.y),
                    size(seam_thickness, bounds.size.height),
                ),
            }
        }
        (SplitDirection::Horizontal, SplitPlacement::After) => {
            let existing_width = bounds.size.width * 0.5;
            let seam_left = bounds.origin.x + existing_width - seam_thickness * 0.5;
            SplitPreviewRegions {
                existing: Bounds::new(bounds.origin, size(existing_width, bounds.size.height)),
                incoming: Bounds::new(
                    point(bounds.origin.x + existing_width, bounds.origin.y),
                    size(bounds.size.width - existing_width, bounds.size.height),
                ),
                seam: Bounds::new(
                    point(seam_left, bounds.origin.y),
                    size(seam_thickness, bounds.size.height),
                ),
            }
        }
    }
}

/// Convert a probe in the live preview back to the original tab order.
/// Ignore the dragged tab's placeholder: it is the result of a reorder, not a
/// neighbour to cross. Otherwise repainting can immediately reverse a swap.
pub(super) fn horizontal_tab_slot_from_bounds(
    cursor: gpui::Point<gpui::Pixels>,
    tab_bounds: &[gpui::Bounds<gpui::Pixels>],
    tab_count: usize,
    source_index: usize,
    source_visual_index: usize,
) -> Option<usize> {
    if tab_count == 0
        || tab_bounds.len() != tab_count
        || source_index >= tab_count
        || source_visual_index >= tab_count
    {
        return None;
    }
    let insertion = tab_bounds
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != source_visual_index)
        .position(|(_, bounds)| cursor.x < bounds.center().x)
        .unwrap_or(tab_count - 1);
    Some(if insertion > source_index {
        insertion + 1
    } else {
        insertion
    })
}

pub(super) fn tab_reorder_destination(source: usize, slot: usize, count: usize) -> usize {
    let destination = if source < slot { slot - 1 } else { slot };
    destination.min(count.saturating_sub(1))
}

pub(super) fn tab_rename_commit_label(
    value: &str,
    changed_by_user: bool,
) -> Option<Option<String>> {
    changed_by_user.then(|| normalize_tab_user_label(value))
}

pub(super) fn normalize_tab_user_label(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

#[cfg(test)]
pub(super) fn remap_tab_rename_state_after_close(
    rename: Option<TabRenameStateSnapshot>,
    cancelled_id: Option<u64>,
    closed_index: usize,
) -> (Option<TabRenameStateSnapshot>, Option<u64>) {
    let rename = rename.and_then(|state| match state.tab_index.cmp(&closed_index) {
        std::cmp::Ordering::Less => Some(state),
        std::cmp::Ordering::Equal => None,
        std::cmp::Ordering::Greater => Some(TabRenameStateSnapshot {
            tab_id: state.tab_id,
            tab_index: state.tab_index - 1,
        }),
    });
    (rename, cancelled_id)
}

#[cfg(test)]
pub(super) fn remap_tab_rename_state_after_reorder(
    rename: Option<TabRenameStateSnapshot>,
    cancelled_id: Option<u64>,
    old_order: &[u64],
    new_order: &[u64],
) -> (Option<TabRenameStateSnapshot>, Option<u64>) {
    let rename = rename.and_then(|state| {
        let summary_id = *old_order.get(state.tab_index)?;
        let tab_index = new_order.iter().position(|id| *id == summary_id)?;
        Some(TabRenameStateSnapshot {
            tab_id: state.tab_id,
            tab_index,
        })
    });
    (rename, cancelled_id)
}

pub(super) fn clear_pane_tab_promotion_drag_state(
    pane_title_drag: &mut Option<PaneTitleDragState>,
    tab_strip_drop_slot: &mut Option<usize>,
    tab_drag_target: &mut Option<TabDragTarget>,
) {
    *pane_title_drag = None;
    *tab_strip_drop_slot = None;
    *tab_drag_target = None;
}

pub(super) fn rebase_active_tab_for_insert(active_tab: usize, insert_index: usize) -> usize {
    if insert_index <= active_tab {
        active_tab.saturating_add(1)
    } else {
        active_tab
    }
}

pub(super) fn should_reveal_vertical_tab_rail_for_new_tab(
    vertical_tabs_enabled: bool,
    left_panel_open: bool,
) -> bool {
    vertical_tabs_enabled && !left_panel_open
}

#[cfg(test)]
pub(super) struct NewTabSyncPolicy {
    pub(super) activates_new_tab: bool,
    pub(super) syncs_sidebar: bool,
    pub(super) notifies_ui: bool,
    pub(super) syncs_native_visibility: bool,
    pub(super) reuses_shared_tab_activation_flow: bool,
}
