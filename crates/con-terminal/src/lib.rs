pub mod input_classify;
pub mod pane_control;
pub mod pane_protocol;
pub mod pane_state;
pub mod program_status;
pub mod shell_probe;
mod theme;
pub mod tmux;

pub use input_classify::looks_like_command;
pub use pane_control::{
    PaneAddressSpace, PaneAttachmentAuthority, PaneAttachmentKind, PaneAttachmentTransport,
    PaneControlCapability, PaneControlChannel, PaneControlState, PaneProtocolAttachment,
    PaneVisibleTarget, PaneVisibleTargetKind, TmuxControlMode, TmuxControlState,
    format_control_attachments, format_target_stack,
};
pub use pane_protocol::{
    PaneCreateLocation, PaneInfo, PaneQuery, PaneRequest, PaneResponse, PaneSelector,
    TerminalExecRequest, TerminalExecResponse,
};
pub use pane_state::{
    PaneActionKind, PaneActionRecord, PaneConfidence, PaneEvidence, PaneEvidenceSource,
    PaneFrontState, PaneMode, PaneObservationFrame, PaneObservationHint, PaneObservationHintKind,
    PaneObservationSupport, PaneRuntimeEvent, PaneRuntimeScope, PaneRuntimeState,
    PaneRuntimeTracker, PaneScopeKind, PaneShellContext, RemoteWorkspaceAnchor, TabWorkspaceKind,
    TabWorkspaceState, TabWorkspaceSummary, classify_screen_agent_cli, derive_screen_hints,
    detect_tmux_session, direct_terminal_exec_is_safe, infer_pane_mode, remote_workspace_anchor,
    shell_metadata_is_fresh,
};
pub use shell_probe::{
    ShellProbeResult, ShellProbeTmuxContext, build_shell_probe_command, parse_shell_probe_lines,
    shell_probe_output_complete,
};
pub use theme::{Color, TerminalTheme};
pub use tmux::{
    TmuxCapture, TmuxExecLocation, TmuxExecResult, TmuxPaneInfo, TmuxSnapshot,
    build_tmux_capture_command, build_tmux_exec_command, build_tmux_list_command,
    build_tmux_send_keys_command, parse_tmux_capture_lines, parse_tmux_exec_lines,
    parse_tmux_list_lines,
};
