use super::*;

pub(super) type HorizontalTabRevealState =
    std::rc::Rc<std::cell::Cell<Option<(u64, usize, usize, Pixels)>>>;

/// Drag state for pane title bar drag-to-tab promotion.
#[derive(Clone)]
pub(super) struct PaneTitleDragState {
    pub(super) title: SharedString,
    pub(super) current_pos: Point<Pixels>,
    /// True while a pane title drag is active.
    pub(super) active: bool,
    pub(super) target: Option<PaneDropTarget>,
}

pub(super) fn pane_title_drag_to_tab_active(drag: Option<&PaneTitleDragState>) -> bool {
    drag.is_some_and(|drag| drag.active)
}

/// A pane uses the tab destination already visible in this window. In vertical
/// mode the sidebar owns promotion; creating a second strip changes native
/// terminal geometry in the middle of the drag.
pub(super) fn pane_drag_needs_horizontal_tabs(vertical_tabs: bool, active: bool) -> bool {
    active && !vertical_tabs
}

/// Workspace-owned drag preview state for horizontal tab-strip drags.
/// GPUI's built-in active-drag preview does not reliably honor relative
/// `.top/.left` constraints, so horizontal tab drags hide that preview and
/// render this overlay from the workspace root instead.
#[derive(Clone)]
pub(super) struct TabDragPreviewState {
    pub(super) title: SharedString,
    pub(super) icon: &'static str,
    pub(super) source_top: Pixels,
    pub(super) cursor_offset_x: Pixels,
    pub(super) size: Size<Pixels>,
    pub(super) source_visual_index: usize,
}

pub(super) fn tab_drag_overlay_origin(
    mouse: Point<Pixels>,
    preview: &TabDragPreviewState,
    min_left: Pixels,
    max_left: Pixels,
) -> Point<Pixels> {
    point(
        (mouse.x - preview.cursor_offset_x).clamp(min_left, max_left),
        preview.source_top,
    )
}

pub(super) fn tab_drag_overlay_probe_position(
    mouse: Point<Pixels>,
    preview: &TabDragPreviewState,
    preview_size: Size<Pixels>,
    min_left: Pixels,
    max_left: Pixels,
) -> Point<Pixels> {
    let origin = tab_drag_overlay_origin(mouse, preview, min_left, max_left);
    point(
        origin.x + preview_size.width / 2.0,
        origin.y + preview_size.height / 2.0,
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum PaneDropTarget {
    NewTab { slot: usize },
    Split(PaneSplitDropTarget),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum TabDragTarget {
    Reorder { slot: usize },
}

pub(super) fn centered_drag_preview_origin(
    cursor: gpui::Point<gpui::Pixels>,
    preview_size: gpui::Size<gpui::Pixels>,
) -> gpui::Point<gpui::Pixels> {
    point(
        cursor.x - preview_size.width / 2.0,
        cursor.y - preview_size.height / 2.0,
    )
}

pub(super) fn clamp_preview_origin_to_tab_bar(
    origin: gpui::Point<gpui::Pixels>,
    preview_size: gpui::Size<gpui::Pixels>,
    tab_bar_top: gpui::Pixels,
    tab_bar_height: gpui::Pixels,
) -> gpui::Point<gpui::Pixels> {
    let max_top = tab_bar_top + (tab_bar_height - preview_size.height).max(px(0.0));
    point(origin.x, origin.y.clamp(tab_bar_top, max_top))
}

pub(super) fn tab_drag_preview_origin(
    cursor: gpui::Point<gpui::Pixels>,
    preview_size: gpui::Size<gpui::Pixels>,
    tab_bar_top: gpui::Pixels,
    tab_bar_height: gpui::Pixels,
) -> gpui::Point<gpui::Pixels> {
    clamp_preview_origin_to_tab_bar(
        centered_drag_preview_origin(cursor, preview_size),
        preview_size,
        tab_bar_top,
        tab_bar_height,
    )
}

pub(super) fn pane_drag_floating_preview_origin(
    cursor: gpui::Point<gpui::Pixels>,
    preview_size: gpui::Size<gpui::Pixels>,
) -> gpui::Point<gpui::Pixels> {
    point(
        cursor.x - preview_size.width / 2.0,
        cursor.y - preview_size.height / 2.0,
    )
}

pub(super) fn tab_like_drag_preview_size() -> gpui::Size<gpui::Pixels> {
    Size {
        width: px(TAB_DRAG_PREVIEW_WIDTH),
        height: px(TAB_DRAG_PREVIEW_HEIGHT),
    }
}

pub(super) fn is_tab_strip_preview_active(
    gpui_tab_drag_active: bool,
    pane_title_drag_to_tab_active: bool,
) -> bool {
    gpui_tab_drag_active || pane_title_drag_to_tab_active
}

pub(super) fn pane_title_drag_tab_slot(
    cursor: gpui::Point<gpui::Pixels>,
    tab_bounds: &[gpui::Bounds<gpui::Pixels>],
    tab_count: usize,
) -> usize {
    if tab_bounds.is_empty() {
        return 0;
    }
    for (i, bounds) in tab_bounds.iter().enumerate() {
        let mid_x = bounds.origin.x + bounds.size.width / 2.0;
        if cursor.x < mid_x {
            return i;
        }
    }
    tab_count
}

pub(super) fn is_dragged_tab_source(
    active_dragged_tab_session_id: Option<u64>,
    tab_session_id: u64,
) -> bool {
    active_dragged_tab_session_id == Some(tab_session_id)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SplitPreviewRegions {
    pub(super) incoming: Bounds<Pixels>,
    pub(super) existing: Bounds<Pixels>,
    pub(super) seam: Bounds<Pixels>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PaneSplitDropTarget {
    pub(super) target_pane_id: usize,
    pub(super) direction: SplitDirection,
    pub(super) placement: SplitPlacement,
    pub(super) bounds: Bounds<Pixels>,
}

/// Inline rename editor for the horizontal tab strip.
pub(super) struct TabRenameEditor {
    pub(super) tab_id: u64,
    pub(super) tab_index: usize,
    pub(super) generation: u64,
    pub(super) input: Entity<InputState>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TabRenameStateSnapshot {
    pub(super) tab_id: u64,
    pub(super) tab_index: usize,
}

pub(super) struct Tab {
    pub(super) pane_tree: PaneTree,
    pub(super) title: String,
    /// 这个标签是通过远程连接打开的服务器。分屏时新 pane 会用同一个
    /// 连接再开一个独立会话，而不是落到本地 shell。
    pub(super) server: Option<con_core::connections::SshConnection>,
    /// User-supplied label that overrides every auto-derived name in
    /// the panel/strip. Set via inline rename (double-click in
    /// vertical panel) or context menu. `None` means "use smart
    /// auto-derived name".
    pub(super) user_label: Option<String>,
    /// Optional accent color set via the tab context menu.
    pub(super) color: Option<con_core::session::TabAccentColor>,
    /// Cached classification of the focused terminal as an interactive agent
    /// CLI. Detection combines process, title, and bounded screen signals.
    pub(super) agent_cli: Option<&'static str>,
    pub(super) needs_attention: bool,
    /// Stable id for this tab across the window's lifetime.
    /// Sidebar rows use it so reorders, closes, and re-opens don't collide.
    pub(super) summary_id: u64,
    pub(super) runtime_trackers: RefCell<HashMap<usize, con_terminal::PaneRuntimeTracker>>,
    pub(super) runtime_cache: RefCell<HashMap<usize, con_terminal::PaneRuntimeState>>,
    pub(super) shell_history: HashMap<usize, VecDeque<CommandSuggestionEntry>>,
}

#[derive(Clone)]
pub(super) struct CommandSuggestionEntry {
    pub(super) command: String,
    pub(super) cwd: Option<String>,
}

pub(super) struct SettingsWindowView {
    pub(super) panel: Entity<SettingsPanel>,
}

impl Render for SettingsWindowView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.panel.clone())
    }
}

#[cfg(target_os = "macos")]
#[repr(C)]
pub(super) struct NSOperatingSystemVersion {
    pub(super) major_version: isize,
    pub(super) minor_version: isize,
    pub(super) patch_version: isize,
}

#[derive(Clone)]
pub(super) struct ResolvedPaneTarget {
    pub(super) pane: TerminalPane,
    pub(super) pane_index: usize,
    pub(super) pane_id: usize,
}

/// A deferred create-pane request waiting for a window-aware context.
pub(super) struct PendingCreatePane {
    pub(super) command: Option<String>,
    pub(super) cwd: Option<String>,
    pub(super) tab_idx: usize,
    pub(super) location: con_terminal::PaneCreateLocation,
    pub(super) response_tx: crossbeam_channel::Sender<con_terminal::PaneResponse>,
}

pub(super) enum PendingWindowControlRequest {
    TabsNew {
        response_tx: oneshot::Sender<ControlResult>,
    },
    TabsClose {
        tab_idx: usize,
        response_tx: oneshot::Sender<ControlResult>,
    },
}

pub(super) enum SessionSaveRequest {
    Save(Session, GlobalHistoryState),
    Flush(Session, GlobalHistoryState, crossbeam_channel::Sender<()>),
}
