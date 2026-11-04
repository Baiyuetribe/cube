//! Pane 协议类型：UI 与后台请求之间的传输契约。
//!
//! 这些类型原先定义在 `con-agent::tools` 中，但它们是终端本身的领域概念，
//! 与 AI Agent 无关。`con-core` 的控制面接口（panes.*）和 `con-app` 的
//! workspace 都直接依赖它们。

use crossbeam_channel::Sender;
use serde::{Deserialize, Serialize};

use crate::pane_control::{
    PaneAddressSpace, PaneControlCapability, PaneControlChannel, PaneProtocolAttachment,
    PaneVisibleTarget, TmuxControlState,
};
use crate::pane_state::{
    PaneConfidence, PaneEvidence, PaneEvidenceSource, PaneFrontState, PaneMode,
    PaneObservationHint, PaneObservationSupport, PaneRuntimeScope, RemoteWorkspaceAnchor,
};
use crate::shell_probe::ShellProbeResult;
use crate::tmux::{TmuxCapture, TmuxExecLocation, TmuxExecResult, TmuxSnapshot};

/// 新建 pane 时的位置。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaneCreateLocation {
    Right,
    Down,
}

impl PaneCreateLocation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Down => "down",
        }
    }
}

/// 定位一个 pane：按索引、按稳定 id，或两者皆空表示聚焦 pane。
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct PaneSelector {
    pub pane_index: Option<usize>,
    pub pane_id: Option<usize>,
}

impl PaneSelector {
    pub const fn new(pane_index: Option<usize>, pane_id: Option<usize>) -> Self {
        Self {
            pane_index,
            pane_id,
        }
    }

    pub fn describe(self) -> String {
        match (self.pane_index, self.pane_id) {
            (Some(index), Some(id)) => format!("pane {index} (id {id})"),
            (Some(index), None) => format!("pane {index}"),
            (None, Some(id)) => format!("pane id {id}"),
            (None, None) => "focused pane".to_string(),
        }
    }
}

/// 单个终端 pane 的元数据。
#[derive(Debug, Clone, Serialize)]
pub struct PaneInfo {
    pub index: usize,
    /// Stable pane id within the current tab lifetime.
    pub pane_id: usize,
    pub title: String,
    pub cwd: Option<String>,
    /// Foreground job-control process group reported by the local PTY.
    pub foreground_process_group_id: Option<u64>,
    /// Local PTY slave name for this pane.
    pub tty_name: Option<String>,
    pub is_focused: bool,
    pub rows: usize,
    pub cols: usize,
    /// Whether the embedded terminal surface has actually been initialized.
    pub surface_ready: bool,
    /// Whether the PTY child process is still running.
    pub is_alive: bool,
    /// Proven hostname when the backend can actually supply one.
    pub hostname: Option<String>,
    /// Confidence for the effective hostname, when detected.
    pub hostname_confidence: Option<PaneConfidence>,
    /// Evidence source for the effective hostname, when detected.
    pub hostname_source: Option<PaneEvidenceSource>,
    /// Best current remote SSH workspace anchor for this pane.
    pub remote_workspace: Option<RemoteWorkspaceAnchor>,
    /// Current verified front-state for the pane.
    pub front_state: PaneFrontState,
    /// Current pane mode: shell, tmux-like multiplexer, or another TUI.
    pub mode: PaneMode,
    /// Whether shell metadata like cwd and last_command is likely fresh for the visible app.
    pub shell_metadata_fresh: bool,
    /// Whether the most recent typed shell-context snapshot still matches the current shell frame.
    pub shell_context_fresh: bool,
    /// What the backend can authoritatively observe for this pane today.
    pub observation_support: PaneObservationSupport,
    /// The only address space valid for pane_index today.
    pub address_space: PaneAddressSpace,
    /// The best-known visible target inside this con pane.
    pub visible_target: PaneVisibleTarget,
    /// Nested runtime/control targets from outer shell toward the front-most visible app.
    pub target_stack: Vec<PaneVisibleTarget>,
    /// tmux adapter state when a tmux layer is present in this pane.
    pub tmux_control: Option<TmuxControlState>,
    /// Explicit protocol attachments currently available on this pane.
    pub control_attachments: Vec<PaneProtocolAttachment>,
    /// Control channels con can use on this pane.
    pub control_channels: Vec<PaneControlChannel>,
    /// Capabilities currently available on this pane.
    pub control_capabilities: Vec<PaneControlCapability>,
    /// Addressing and control notes to respect.
    pub control_notes: Vec<String>,
    /// The top-most active runtime scope, when detected.
    pub active_scope: Option<PaneRuntimeScope>,
    /// Detected external agent CLI, when visible.
    pub agent_cli: Option<String>,
    /// Evidence behind the current runtime summary.
    pub evidence: Vec<PaneEvidence>,
    /// Structured runtime scopes inferred from pane-local evidence.
    pub runtime_stack: Vec<PaneRuntimeScope>,
    /// Last verified shell-frame stack captured for this pane.
    pub last_verified_runtime_stack: Vec<PaneRuntimeScope>,
    /// Warnings about stale or advisory runtime metadata.
    pub runtime_warnings: Vec<String>,
    /// Last typed shell-context snapshot captured for this pane.
    pub shell_context: Option<crate::pane_state::PaneShellContext>,
    /// Recent con-originated actions for this pane.
    pub recent_actions: Vec<crate::pane_state::PaneActionRecord>,
    /// Weak observation hints derived from the current visible screen snapshot.
    pub screen_hints: Vec<PaneObservationHint>,
    /// tmux session hint when detected from the pane itself.
    pub tmux_session: Option<String>,
    /// Whether shell integration (OSC 133) is active.
    pub has_shell_integration: bool,
    /// Most recent command text when the backend can prove it.
    pub last_command: Option<String>,
    /// Exit code of the last command.
    pub last_exit_code: Option<i32>,
    /// A command is currently executing (between OSC 133 C and D).
    /// Only reliable when has_shell_integration is true.
    pub is_busy: bool,
}

/// 对 pane 的查询请求。
#[derive(Debug)]
pub enum PaneQuery {
    /// List all panes with metadata.
    List,
    /// Read recent output from a specific pane.
    ReadContent { target: PaneSelector, lines: usize },
    /// Send raw keystrokes to a specific pane (for TUI interaction, Ctrl-C, etc.).
    SendKeys { target: PaneSelector, keys: String },
    /// Search scrollback + visible screen for a text pattern.
    SearchText {
        target: PaneSelector,
        pattern: String,
        max_matches: usize,
    },
    /// Return tmux adapter state for a pane whose target stack contains tmux.
    InspectTmux { target: PaneSelector },
    /// Query tmux windows/panes through a same-session tmux control anchor.
    TmuxList { pane: PaneSelector },
    /// Capture pane content from a tmux pane target through a same-session tmux control anchor.
    TmuxCapture {
        pane: PaneSelector,
        target: Option<String>,
        lines: usize,
    },
    /// Send literal text or tmux key names to a tmux pane target through a same-session tmux control anchor.
    TmuxSendKeys {
        pane: PaneSelector,
        target: String,
        literal_text: Option<String>,
        key_names: Vec<String>,
        append_enter: bool,
    },
    /// Run a command through tmux itself by creating a new tmux target.
    TmuxRunCommand {
        pane: PaneSelector,
        target: Option<String>,
        location: TmuxExecLocation,
        command: String,
        window_name: Option<String>,
        cwd: Option<String>,
        detached: bool,
    },
    /// Run a read-only shell-scoped probe in a pane with a proven fresh shell prompt.
    ProbeShellContext { target: PaneSelector },
    /// Lightweight busy check for a single pane.
    CheckBusy { target: PaneSelector },
    /// Wait for a pane to become idle or match a pattern.
    WaitFor {
        target: PaneSelector,
        timeout_secs: Option<u64>,
        pattern: Option<String>,
    },
    /// Create a new terminal pane (tab), optionally running a command in it.
    CreatePane {
        command: Option<String>,
        location: PaneCreateLocation,
    },
}

/// workspace 对 pane 查询的响应。
#[derive(Debug, Clone)]
pub enum PaneResponse {
    PaneList(Vec<PaneInfo>),
    Content(String),
    KeysSent,
    TmuxInfo(TmuxControlState),
    TmuxList(TmuxSnapshot),
    TmuxCapture(TmuxCapture),
    TmuxExec(TmuxExecResult),
    ShellProbe(ShellProbeResult),
    /// Search results: Vec of (pane_index, line_number, line_text).
    SearchResults(Vec<(usize, usize, String)>),
    /// Lightweight busy-check response.
    BusyStatus {
        surface_ready: bool,
        is_alive: bool,
        is_busy: bool,
        has_shell_integration: bool,
    },
    /// Response from a wait_for operation.
    WaitComplete {
        status: String,
        output: String,
    },
    /// A new pane was created successfully.
    PaneCreated {
        pane_index: usize,
        pane_id: usize,
        surface_ready: bool,
        is_alive: bool,
        has_shell_integration: bool,
        observation_support: PaneObservationSupport,
        foreground_process_group_id: Option<u64>,
        tty_name: Option<String>,
    },
    Error(String),
}

/// 从 pane 消费者发往 workspace 的请求。
#[derive(Debug)]
pub struct PaneRequest {
    pub query: PaneQuery,
    pub response_tx: Sender<PaneResponse>,
}

/// 在可见终端 pane 中执行命令的请求。
#[derive(Debug)]
pub struct TerminalExecRequest {
    pub command: String,
    pub working_dir: Option<String>,
    pub target: PaneSelector,
    pub response_tx: Sender<TerminalExecResponse>,
}

/// 可见终端执行的响应。
#[derive(Debug, Clone)]
pub struct TerminalExecResponse {
    pub output: String,
    pub exit_code: Option<i32>,
    status: TerminalExecStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalExecStatus {
    Completed,
    Unconfirmed,
}

impl TerminalExecResponse {
    const UNCONFIRMED_MESSAGE: &'static str = "Shell command completion could not be confirmed; the command may still be running. Do not issue another command to this pane until its state is verified.";

    pub fn completed(output: String, exit_code: Option<i32>) -> Self {
        Self {
            output,
            exit_code,
            status: TerminalExecStatus::Completed,
        }
    }

    pub fn unconfirmed(output: String) -> Self {
        Self {
            output,
            exit_code: None,
            status: TerminalExecStatus::Unconfirmed,
        }
    }

    pub fn is_unconfirmed(&self) -> bool {
        self.status == TerminalExecStatus::Unconfirmed
    }

    pub fn completion_error(&self) -> Option<&'static str> {
        (self.status == TerminalExecStatus::Unconfirmed).then_some(Self::UNCONFIRMED_MESSAGE)
    }

    pub fn completion_error_with_output(&self) -> Option<String> {
        self.completion_error().map(|message| {
            if self.output.is_empty() {
                message.to_string()
            } else {
                format!("{message}\nRecent output:\n{}", self.output)
            }
        })
    }

    /// 以适合展示的形式返回输出，未确认时附加提示。
    pub fn display_output(&self) -> String {
        if self.is_unconfirmed() {
            if self.output.is_empty() {
                Self::UNCONFIRMED_MESSAGE.to_string()
            } else {
                format!("{}\n\n{}", self.output, Self::UNCONFIRMED_MESSAGE)
            }
        } else {
            self.output.clone()
        }
    }
}
