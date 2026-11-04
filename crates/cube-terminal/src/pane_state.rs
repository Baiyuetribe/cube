use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::pane_control::PaneControlState;
use crate::shell_probe::{ShellProbeResult, ShellProbeTmuxContext};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PaneMode {
    Shell,
    Multiplexer,
    Tui,
    #[default]
    Unknown,
}

impl PaneMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Multiplexer => "multiplexer",
            Self::Tui => "tui",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PaneFrontState {
    ShellPrompt,
    InteractiveSurface,
    #[default]
    Unknown,
}

impl PaneFrontState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ShellPrompt => "shell_prompt",
            Self::InteractiveSurface => "interactive_surface",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaneScopeKind {
    Shell,
    RemoteShell,
    Multiplexer,
    InteractiveApp,
    AgentCli,
}

impl PaneScopeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::RemoteShell => "remote_shell",
            Self::Multiplexer => "multiplexer",
            Self::InteractiveApp => "interactive_app",
            Self::AgentCli => "agent_cli",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaneEvidenceSource {
    ShellIntegration,
    SurfaceState,
    Osc7,
    CommandLine,
    ShellProbe,
    ActionHistory,
    Title,
    ScreenStructure,
}

impl PaneEvidenceSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ShellIntegration => "shell_integration",
            Self::SurfaceState => "surface_state",
            Self::Osc7 => "osc7",
            Self::CommandLine => "command_line",
            Self::ShellProbe => "shell_probe",
            Self::ActionHistory => "action_history",
            Self::Title => "title",
            Self::ScreenStructure => "screen_structure",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaneConfidence {
    Strong,
    Advisory,
}

impl PaneConfidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Strong => "strong",
            Self::Advisory => "advisory",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaneRuntimeScope {
    pub kind: PaneScopeKind,
    pub label: Option<String>,
    pub host: Option<String>,
    pub confidence: PaneConfidence,
    pub evidence_source: PaneEvidenceSource,
}

impl PaneRuntimeScope {
    pub fn summary(&self) -> String {
        match (self.kind, self.label.as_deref(), self.host.as_deref()) {
            (PaneScopeKind::RemoteShell, _, Some(host)) => format!("remote_shell({host})"),
            (_, Some(label), _) => format!("{}({label})", self.kind.as_str()),
            _ => self.kind.as_str().to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaneActionKind {
    PaneCreated,
    VisibleShellExec,
    RawInput,
    ShellProbe,
    ProcessExited,
}

impl PaneActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PaneCreated => "pane_created",
            Self::VisibleShellExec => "visible_shell_exec",
            Self::RawInput => "raw_input",
            Self::ShellProbe => "shell_probe",
            Self::ProcessExited => "process_exited",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaneActionRecord {
    pub sequence: u64,
    pub kind: PaneActionKind,
    pub summary: String,
    pub command: Option<String>,
    pub source: PaneEvidenceSource,
    pub confidence: PaneConfidence,
    pub input_generation: Option<u64>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneShellContext {
    pub captured_input_generation: u64,
    pub host: Option<String>,
    pub pwd: Option<String>,
    pub term: Option<String>,
    pub term_program: Option<String>,
    pub ssh_connection: Option<String>,
    pub ssh_tty: Option<String>,
    pub tmux_env: Option<String>,
    pub nvim_listen_address: Option<String>,
    pub tmux: Option<ShellProbeTmuxContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteWorkspaceAnchor {
    pub host: String,
    pub source: PaneEvidenceSource,
    pub confidence: PaneConfidence,
    pub note: String,
}

impl PaneShellContext {
    fn from_probe(result: &ShellProbeResult, captured_input_generation: u64) -> Self {
        Self {
            captured_input_generation,
            host: result.host.clone(),
            pwd: result.pwd.clone(),
            term: result.term.clone(),
            term_program: result.term_program.clone(),
            ssh_connection: result.ssh_connection.clone(),
            ssh_tty: result.ssh_tty.clone(),
            tmux_env: result.tmux_env.clone(),
            nvim_listen_address: result.nvim_listen_address.clone(),
            tmux: result.tmux.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PaneObservationSupport {
    /// Whether the backend can provide authoritative foreground command text.
    pub foreground_command: bool,
    /// Whether the backend can provide a foreground process-group ID.
    #[serde(default)]
    pub foreground_process_group_id: bool,
    /// Whether the backend can provide a PTY slave name.
    #[serde(default)]
    pub tty_name: bool,
    /// Whether the backend can provide authoritative alternate-screen state.
    pub alternate_screen: bool,
    /// Whether the backend can provide authoritative remote-host identity.
    pub remote_host_identity: bool,
}

impl PaneObservationSupport {
    pub fn backend_limit_note(&self) -> Option<String> {
        let mut missing = Vec::new();
        if !self.foreground_command {
            missing.push("foreground command text");
        }
        if !self.foreground_process_group_id {
            missing.push("foreground process-group ID");
        }
        if !self.tty_name {
            missing.push("PTY slave name");
        }
        if !self.alternate_screen {
            missing.push("alternate-screen state");
        }
        if !self.remote_host_identity {
            missing.push("remote-host identity");
        }

        if missing.is_empty() {
            return None;
        }

        Some(format!(
            "The terminal backend does not currently export authoritative {} for this pane. Unproven foreground runtimes must stay unknown.",
            missing.join(", ")
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PaneObservationFrame {
    pub title: Option<String>,
    pub cwd: Option<String>,
    #[serde(default)]
    pub foreground_process_group_id: Option<u64>,
    #[serde(default)]
    pub tty_name: Option<String>,
    pub recent_output: Vec<String>,
    pub screen_hints: Vec<PaneObservationHint>,
    pub last_command: Option<String>,
    pub last_exit_code: Option<i32>,
    pub last_command_duration_secs: Option<f64>,
    pub support: PaneObservationSupport,
    pub has_shell_integration: bool,
    pub is_alt_screen: bool,
    pub is_busy: bool,
    pub input_generation: u64,
    pub last_command_finished_input_generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PaneRuntimeState {
    pub front_state: PaneFrontState,
    pub mode: PaneMode,
    pub shell_metadata_fresh: bool,
    pub screen_prompt_like: bool,
    pub screen_tmux_like: bool,
    pub screen_ssh_disconnected: bool,
    pub remote_host: Option<String>,
    pub remote_host_confidence: Option<PaneConfidence>,
    pub remote_host_source: Option<PaneEvidenceSource>,
    pub agent_cli: Option<String>,
    pub tmux_session: Option<String>,
    pub last_verified_scope_stack: Vec<PaneRuntimeScope>,
    pub last_verified_tmux_session: Option<String>,
    pub shell_context: Option<PaneShellContext>,
    pub shell_context_fresh: bool,
    pub active_scope: Option<PaneRuntimeScope>,
    pub evidence: Vec<PaneEvidence>,
    pub scope_stack: Vec<PaneRuntimeScope>,
    pub recent_actions: Vec<PaneActionRecord>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaneObservationHintKind {
    PromptLikeInput,
    HtopLikeScreen,
    LoginBannerVisible,
    SshConnectionClosed,
    TmuxLikeScreen,
}

impl PaneObservationHintKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PromptLikeInput => "prompt_like_input",
            Self::HtopLikeScreen => "htop_like_screen",
            Self::LoginBannerVisible => "login_banner_visible",
            Self::SshConnectionClosed => "ssh_connection_closed",
            Self::TmuxLikeScreen => "tmux_like_screen",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaneObservationHint {
    pub kind: PaneObservationHintKind,
    pub confidence: PaneConfidence,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TabWorkspaceKind {
    LocalShell,
    RemoteShell,
    TmuxWorkspace,
    Unknown,
}

impl TabWorkspaceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalShell => "local_shell",
            Self::RemoteShell => "remote_shell",
            Self::TmuxWorkspace => "tmux_workspace",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TabWorkspaceState {
    Ready,
    NeedsInspection,
    Disconnected,
    Interactive,
    Unknown,
}

impl TabWorkspaceState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::NeedsInspection => "needs_inspection",
            Self::Disconnected => "disconnected",
            Self::Interactive => "interactive",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TabWorkspaceSummary {
    pub pane_index: usize,
    pub pane_id: usize,
    pub host: Option<String>,
    pub tmux_session: Option<String>,
    pub cwd: Option<String>,
    pub agent_cli: Option<String>,
    pub kind: TabWorkspaceKind,
    pub state: TabWorkspaceState,
    pub note: String,
}

impl PaneRuntimeState {
    pub fn from_observation(observation: &PaneObservationFrame) -> Self {
        PaneRuntimeTracker::default().observe(observation.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneEvidence {
    pub subject: String,
    pub value: Option<String>,
    pub source: PaneEvidenceSource,
    pub confidence: PaneConfidence,
    pub generation: u64,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
struct TrackedShellContext {
    result: ShellProbeResult,
    captured_input_generation: u64,
}

#[derive(Debug, Clone)]
pub enum PaneRuntimeEvent {
    PaneCreated {
        startup_command: Option<String>,
    },
    VisibleShellExec {
        command: String,
        input_generation: u64,
    },
    RawInput {
        keys: String,
        input_generation: u64,
    },
    ShellProbe {
        // Boxed: this variant is ~392 B larger than the others, and boxing
        // keeps PaneRuntimeEvent cheap to clone as it flows through the
        // tracker.
        result: Box<ShellProbeResult>,
        captured_input_generation: u64,
    },
    ProcessExited,
}

#[derive(Debug, Clone, Default)]
pub struct PaneRuntimeTracker {
    generation: u64,
    event_sequence: u64,
    shell_context: Option<TrackedShellContext>,
    recent_actions: VecDeque<PaneActionRecord>,
}

impl PaneRuntimeTracker {
    pub fn record_action(&mut self, event: PaneRuntimeEvent) {
        self.event_sequence += 1;
        let sequence = self.event_sequence;

        match event {
            PaneRuntimeEvent::PaneCreated { startup_command } => {
                let summary = startup_command
                    .as_ref()
                    .map(|command| {
                        format!("Cube created this pane with startup command `{command}`")
                    })
                    .unwrap_or_else(|| "Cube created this pane".to_string());
                self.push_recent_action(PaneActionRecord {
                    sequence,
                    kind: PaneActionKind::PaneCreated,
                    summary,
                    command: startup_command.clone(),
                    source: PaneEvidenceSource::ActionHistory,
                    confidence: PaneConfidence::Advisory,
                    input_generation: None,
                    note: startup_command.as_deref().and_then(command_intent_note),
                });
            }
            PaneRuntimeEvent::VisibleShellExec {
                command,
                input_generation,
            } => {
                self.push_recent_action(PaneActionRecord {
                    sequence,
                    kind: PaneActionKind::VisibleShellExec,
                    summary: format!("Cube executed `{command}` in the visible shell"),
                    command: Some(command.clone()),
                    source: PaneEvidenceSource::ActionHistory,
                    confidence: PaneConfidence::Advisory,
                    input_generation: Some(input_generation),
                    note: command_intent_note(&command),
                });
            }
            PaneRuntimeEvent::RawInput {
                keys,
                input_generation,
            } => {
                self.push_recent_action(PaneActionRecord {
                    sequence,
                    kind: PaneActionKind::RawInput,
                    summary: format!("Cube sent raw input `{}`", summarize_raw_input(&keys)),
                    command: None,
                    source: PaneEvidenceSource::ActionHistory,
                    confidence: PaneConfidence::Advisory,
                    input_generation: Some(input_generation),
                    note: Some(
                        "Raw input describes what Cube sent to the pane, not what the foreground app proved in response."
                            .to_string(),
                    ),
                });
            }
            PaneRuntimeEvent::ShellProbe {
                result,
                captured_input_generation,
            } => {
                let summary = summarize_shell_probe(&result);
                self.shell_context = Some(TrackedShellContext {
                    result: *result,
                    captured_input_generation,
                });
                self.push_recent_action(PaneActionRecord {
                    sequence,
                    kind: PaneActionKind::ShellProbe,
                    summary,
                    command: None,
                    source: PaneEvidenceSource::ShellProbe,
                    confidence: PaneConfidence::Strong,
                    input_generation: Some(captured_input_generation),
                    note: Some(
                        "This probe describes the shell frame that was visible when Cube ran the probe."
                            .to_string(),
                    ),
                });
            }
            PaneRuntimeEvent::ProcessExited => {
                self.shell_context = None;
                self.push_recent_action(PaneActionRecord {
                    sequence,
                    kind: PaneActionKind::ProcessExited,
                    summary: "the pane process exited".to_string(),
                    command: None,
                    source: PaneEvidenceSource::ActionHistory,
                    confidence: PaneConfidence::Strong,
                    input_generation: None,
                    note: None,
                });
            }
        }
    }

    pub fn observe(&mut self, observation: PaneObservationFrame) -> PaneRuntimeState {
        self.generation += 1;
        let generation = self.generation;

        let shell_metadata_fresh = shell_metadata_is_fresh(
            observation.has_shell_integration,
            observation.input_generation,
            observation.last_command_finished_input_generation,
        );

        let shell_context = self.shell_context.as_ref().map(|context| {
            PaneShellContext::from_probe(&context.result, context.captured_input_generation)
        });
        let shell_context_fresh = self.shell_context.as_ref().is_some_and(|context| {
            shell_metadata_fresh
                && observation.input_generation == context.captured_input_generation
        });
        let screen_prompt_like = observation
            .screen_hints
            .iter()
            .any(|hint| hint.kind == PaneObservationHintKind::PromptLikeInput);
        let screen_tmux_like = observation
            .screen_hints
            .iter()
            .any(|hint| hint.kind == PaneObservationHintKind::TmuxLikeScreen);
        let screen_ssh_disconnected = observation
            .screen_hints
            .iter()
            .any(|hint| hint.kind == PaneObservationHintKind::SshConnectionClosed);

        let (
            remote_host,
            remote_host_confidence,
            remote_host_source,
            remote_scope,
            remote_host_evidence,
        ) = remote_host_from_shell_context(
            self.shell_context.as_ref(),
            shell_context_fresh,
            generation,
        );
        let tmux_scope = tmux_scope_from_shell_context(
            self.shell_context.as_ref(),
            shell_context_fresh,
            remote_host.as_deref(),
        );
        let action_tmux_session = self
            .recent_actions
            .iter()
            .rev()
            .find_map(tmux_session_from_action_record);
        let interactive_scope = if observation.support.alternate_screen && observation.is_alt_screen
        {
            Some(PaneRuntimeScope {
                kind: PaneScopeKind::InteractiveApp,
                label: None,
                host: remote_host.clone(),
                confidence: PaneConfidence::Strong,
                evidence_source: PaneEvidenceSource::SurfaceState,
            })
        } else {
            None
        };

        let mut last_verified_scope_stack =
            shell_context_scope_stack(self.shell_context.as_ref(), shell_context_fresh);
        let current_scope_stack = if shell_metadata_fresh {
            let mut stack = Vec::new();
            if let Some(scope) = remote_scope {
                stack.push(scope);
            }
            if let Some(scope) = tmux_scope.clone() {
                stack.push(scope);
            }
            stack.push(PaneRuntimeScope {
                kind: PaneScopeKind::Shell,
                label: None,
                host: remote_host.clone(),
                confidence: PaneConfidence::Strong,
                evidence_source: PaneEvidenceSource::ShellIntegration,
            });
            stack
        } else if let Some(scope) = interactive_scope.clone() {
            vec![scope]
        } else {
            Vec::new()
        };
        if last_verified_scope_stack.is_empty() && shell_metadata_fresh {
            last_verified_scope_stack = current_scope_stack.clone();
        }

        let front_scope = current_scope_stack.last().cloned();
        let front_state = if shell_metadata_fresh {
            PaneFrontState::ShellPrompt
        } else if observation.support.alternate_screen && observation.is_alt_screen {
            PaneFrontState::InteractiveSurface
        } else {
            PaneFrontState::Unknown
        };

        let mode = match front_state {
            PaneFrontState::ShellPrompt => PaneMode::Shell,
            PaneFrontState::InteractiveSurface => PaneMode::Tui,
            PaneFrontState::Unknown => PaneMode::Unknown,
        };

        let mut evidence = Vec::new();

        if shell_metadata_fresh {
            evidence.push(PaneEvidence {
                subject: "shell_prompt".to_string(),
                value: Some("confirmed".to_string()),
                source: PaneEvidenceSource::ShellIntegration,
                confidence: PaneConfidence::Strong,
                generation: self.generation,
                note: Some(
                    "Ghostty shell integration observed a clean shell prompt after the most recent input.".to_string(),
                ),
            });
        }

        if observation.support.foreground_process_group_id
            && let Some(process_group_id) = observation.foreground_process_group_id
        {
            evidence.push(PaneEvidence {
                subject: "foreground_process_group_id".to_string(),
                value: Some(process_group_id.to_string()),
                source: PaneEvidenceSource::SurfaceState,
                confidence: PaneConfidence::Strong,
                generation,
                note: Some(
                    "The local PTY reports this foreground job-control group. It is not an exact process identity."
                        .to_string(),
                ),
            });
        }

        if observation.support.tty_name
            && let Some(tty_name) = observation.tty_name.as_ref()
        {
            evidence.push(PaneEvidence {
                subject: "tty_name".to_string(),
                value: Some(tty_name.clone()),
                source: PaneEvidenceSource::SurfaceState,
                confidence: PaneConfidence::Strong,
                generation,
                note: None,
            });
        }

        if let Some(evidence_item) = remote_host_evidence {
            evidence.push(evidence_item);
        }

        if let Some(multiplexer) = tmux_scope.as_ref() {
            evidence.push(PaneEvidence {
                subject: "multiplexer".to_string(),
                value: multiplexer.label.clone(),
                source: multiplexer.evidence_source,
                confidence: multiplexer.confidence,
                generation,
                note: Some(
                    "A typed shell probe confirmed that the current shell prompt is nested inside tmux."
                        .to_string(),
                ),
            });
        }

        if let Some(scope) = interactive_scope.as_ref() {
            evidence.push(PaneEvidence {
                subject: "interactive_surface".to_string(),
                value: None,
                source: PaneEvidenceSource::SurfaceState,
                confidence: scope.confidence,
                generation,
                note: Some(
                    "Ghostty reports alternate-screen mode for the visible surface.".to_string(),
                ),
            });
        }

        if let Some(context) = shell_context.as_ref() {
            evidence.push(PaneEvidence {
                subject: "shell_probe".to_string(),
                value: context.host.clone().or_else(|| context.pwd.clone()),
                source: PaneEvidenceSource::ShellProbe,
                confidence: if shell_context_fresh {
                    PaneConfidence::Strong
                } else {
                    PaneConfidence::Advisory
                },
                generation,
                note: Some(if shell_context_fresh {
                    "A typed shell probe matches the current visible shell frame.".to_string()
                } else {
                    "The last typed shell probe describes an earlier shell frame.".to_string()
                }),
            });
        }

        if shell_metadata_fresh
            && shell_context
                .as_ref()
                .and_then(|context| context.tmux.as_ref())
                .is_none()
            && let Some(session) = action_tmux_session.as_ref()
        {
            evidence.push(PaneEvidence {
                    subject: "tmux_shell_anchor".to_string(),
                    value: Some(session.clone()),
                    source: PaneEvidenceSource::ActionHistory,
                    confidence: PaneConfidence::Advisory,
                    generation,
                    note: Some(
                        "A recent Cube-executed tmux command targeted this session from the current fresh shell prompt. Cube can use that shell as a tmux control anchor while the prompt remains fresh."
                            .to_string(),
                    ),
                });
        }

        let active_scope = front_scope;
        let tmux_session = current_scope_stack
            .iter()
            .find(|scope| scope.kind == PaneScopeKind::Multiplexer)
            .and_then(|scope| scope.label.clone());
        let last_verified_tmux_session = last_verified_scope_stack
            .iter()
            .find(|scope| scope.kind == PaneScopeKind::Multiplexer)
            .and_then(|scope| scope.label.clone())
            .or(action_tmux_session);
        let agent_cli =
            classify_screen_agent_cli(observation.title.as_deref(), &observation.recent_output)
                .map(ToString::to_string);

        let mut warnings = Vec::new();
        if !shell_metadata_fresh {
            warnings.push(
                "Visible shell prompt is not confirmed. Treat cwd and last_command as historical shell metadata, not foreground-app truth.".to_string(),
            );
        }
        if shell_context.is_some() && !shell_context_fresh {
            warnings.push(
                "The last shell probe is historical. It can explain the last verified shell frame, but it does not prove the current foreground target.".to_string(),
            );
        }
        if let Some(note) = observation.support.backend_limit_note() {
            warnings.push(note);
        }
        if current_scope_stack.is_empty() && !last_verified_scope_stack.is_empty() {
            warnings.push(format!(
                "Last verified shell frame: {}.",
                format_runtime_stack(&last_verified_scope_stack)
            ));
        }

        PaneRuntimeState {
            front_state,
            mode,
            shell_metadata_fresh,
            screen_prompt_like,
            screen_tmux_like,
            screen_ssh_disconnected,
            remote_host,
            remote_host_confidence,
            remote_host_source,
            agent_cli,
            tmux_session,
            last_verified_scope_stack,
            last_verified_tmux_session,
            shell_context,
            shell_context_fresh,
            active_scope,
            evidence,
            scope_stack: current_scope_stack,
            recent_actions: self.recent_actions.iter().cloned().collect(),
            warnings,
        }
    }

    fn push_recent_action(&mut self, action: PaneActionRecord) {
        const MAX_RECENT_ACTIONS: usize = 8;
        self.recent_actions.push_back(action);
        while self.recent_actions.len() > MAX_RECENT_ACTIONS {
            self.recent_actions.pop_front();
        }
    }
}

fn summarize_raw_input(keys: &str) -> String {
    let escaped: String = keys.chars().flat_map(char::escape_default).collect();
    let mut preview = escaped;
    if preview.len() > 48 {
        preview.truncate(48);
        preview.push_str("...");
    }
    preview
}

fn summarize_shell_probe(result: &ShellProbeResult) -> String {
    let mut parts = Vec::new();
    if let Some(host) = &result.host {
        parts.push(format!("host `{host}`"));
    }
    if let Some(tmux) = &result.tmux {
        if let Some(session) = &tmux.session_name {
            parts.push(format!("tmux session `{session}`"));
        } else {
            parts.push("tmux context".to_string());
        }
        if let Some(pane) = &tmux.pane_id {
            parts.push(format!("pane `{pane}`"));
        }
    }
    if let Some(path) = &result.nvim_listen_address {
        parts.push(format!("nvim socket `{path}`"));
    }

    if parts.is_empty() {
        "Cube probed the visible shell context".to_string()
    } else {
        format!(
            "Cube probed the visible shell context and captured {}",
            parts.join(", ")
        )
    }
}

fn command_intent_note(command: &str) -> Option<String> {
    if looks_like_tmux_command(command) {
        return Some(
            "This command targets tmux/tmate. Treat it as causal history about how Cube entered a multiplexer, not as proof that tmux is still front-most now."
                .to_string(),
        );
    }

    if is_agent_cli_command(command) {
        return Some(
            "This command launched a supported agent CLI. Treat it as causal history until newer pane evidence proves what is currently visible."
                .to_string(),
        );
    }

    if command_basename(command).as_deref() == Some("ssh") {
        return Some(
            "This command opened an SSH connection. It is useful history, but remote identity is only proven after a typed shell probe or backend export."
                .to_string(),
        );
    }

    None
}

/// Contributions of the tracked shell context to pane evidence: remote host,
/// its confidence and source, the runtime scope, and the evidence record.
type ShellContextEvidence = (
    Option<String>,
    Option<PaneConfidence>,
    Option<PaneEvidenceSource>,
    Option<PaneRuntimeScope>,
    Option<PaneEvidence>,
);

fn remote_host_from_shell_context(
    shell_context: Option<&TrackedShellContext>,
    shell_context_fresh: bool,
    generation: u64,
) -> ShellContextEvidence {
    let Some(context) = shell_context else {
        return (None, None, None, None, None);
    };
    let Some(host) = context.result.host.clone() else {
        return (None, None, None, None, None);
    };
    if context.result.ssh_connection.is_none() {
        return (None, None, None, None, None);
    }

    let confidence = if shell_context_fresh {
        PaneConfidence::Strong
    } else {
        PaneConfidence::Advisory
    };
    let source = PaneEvidenceSource::ShellProbe;

    (
        if shell_context_fresh {
            Some(host.clone())
        } else {
            None
        },
        if shell_context_fresh {
            Some(confidence)
        } else {
            None
        },
        if shell_context_fresh {
            Some(source)
        } else {
            None
        },
        Some(PaneRuntimeScope {
            kind: PaneScopeKind::RemoteShell,
            label: Some(host.clone()),
            host: Some(host.clone()),
            confidence,
            evidence_source: source,
        }),
        Some(PaneEvidence {
            subject: "remote_host".to_string(),
            value: Some(host),
            source,
            confidence,
            generation,
            note: Some(if shell_context_fresh {
                "The last shell probe confirmed that the visible shell is running on a remote host."
                    .to_string()
            } else {
                "A historical shell probe previously confirmed remote shell context for this pane."
                    .to_string()
            }),
        }),
    )
}

fn shell_context_scope_stack(
    shell_context: Option<&TrackedShellContext>,
    shell_context_fresh: bool,
) -> Vec<PaneRuntimeScope> {
    let Some(context) = shell_context else {
        return Vec::new();
    };

    let confidence = if shell_context_fresh {
        PaneConfidence::Strong
    } else {
        PaneConfidence::Advisory
    };
    let host = context
        .result
        .host
        .clone()
        .filter(|_| context.result.ssh_connection.is_some());

    let mut scopes = Vec::new();
    if let Some(host) = host.clone() {
        scopes.push(PaneRuntimeScope {
            kind: PaneScopeKind::RemoteShell,
            label: Some(host.clone()),
            host: Some(host),
            confidence,
            evidence_source: PaneEvidenceSource::ShellProbe,
        });
    }
    if let Some(tmux) = &context.result.tmux {
        scopes.push(PaneRuntimeScope {
            kind: PaneScopeKind::Multiplexer,
            label: tmux
                .session_name
                .clone()
                .or_else(|| Some("tmux".to_string())),
            host: host.clone(),
            confidence,
            evidence_source: PaneEvidenceSource::ShellProbe,
        });
    }
    scopes.push(PaneRuntimeScope {
        kind: PaneScopeKind::Shell,
        label: None,
        host,
        confidence,
        evidence_source: PaneEvidenceSource::ShellProbe,
    });
    scopes
}

fn tmux_scope_from_shell_context(
    shell_context: Option<&TrackedShellContext>,
    shell_context_fresh: bool,
    remote_host: Option<&str>,
) -> Option<PaneRuntimeScope> {
    let context = shell_context?;
    let tmux = context.result.tmux.as_ref()?;
    if !shell_context_fresh {
        return None;
    }

    Some(PaneRuntimeScope {
        kind: PaneScopeKind::Multiplexer,
        label: tmux
            .session_name
            .clone()
            .or_else(|| Some("tmux".to_string())),
        host: remote_host.map(str::to_string),
        confidence: PaneConfidence::Strong,
        evidence_source: PaneEvidenceSource::ShellProbe,
    })
}

fn looks_like_tmux_command(command: &str) -> bool {
    command_basename(command)
        .as_deref()
        .is_some_and(|name| matches!(name, "tmux" | "tmate"))
}

fn is_env_assignment(token: &str) -> bool {
    token.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

fn command_basename(command: &str) -> Option<String> {
    let tokens = command.split_whitespace();
    for token in tokens {
        let token = token.trim_matches(&['"', '\''][..]);
        if token.is_empty() {
            continue;
        }
        if is_env_assignment(token) || matches!(token, "env" | "sudo" | "command" | "nohup") {
            continue;
        }
        let basename = token
            .rsplit('/')
            .next()
            .unwrap_or(token)
            .to_ascii_lowercase();
        if !basename.is_empty() {
            return Some(basename);
        }
    }
    None
}

fn parse_tmux_target(command: &str) -> Option<String> {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    for window in tokens.windows(2) {
        if matches!(window[0], "-t" | "-s") {
            return Some(window[1].trim_matches(&['"', '\''][..]).to_string());
        }
        if let Some(rest) = window[0].strip_prefix("-t")
            && !rest.is_empty()
        {
            return Some(rest.trim_matches(&['"', '\''][..]).to_string());
        }
        if let Some(rest) = window[0].strip_prefix("-s")
            && !rest.is_empty()
        {
            return Some(rest.trim_matches(&['"', '\''][..]).to_string());
        }
    }
    None
}

pub fn detect_tmux_session(last_command: Option<&str>) -> Option<String> {
    last_command
        .filter(|command| looks_like_tmux_command(command))
        .and_then(parse_tmux_target)
}

pub(crate) fn tmux_session_from_action_record(action: &PaneActionRecord) -> Option<String> {
    if !matches!(
        action.kind,
        PaneActionKind::PaneCreated | PaneActionKind::VisibleShellExec
    ) {
        return None;
    }
    detect_tmux_session(action.command.as_deref())
}

fn is_agent_cli_command(command: &str) -> bool {
    command_basename(command).as_deref().is_some_and(|name| {
        matches!(
            name,
            "claude" | "claude-code" | "codex" | "opencode" | "open-code"
        )
    })
}

pub fn infer_pane_mode(
    _last_command: Option<&str>,
    has_shell_integration: bool,
    is_alt_screen: bool,
    input_generation: u64,
    last_command_finished_input_generation: u64,
) -> PaneMode {
    if is_alt_screen {
        return PaneMode::Tui;
    }

    if shell_metadata_is_fresh(
        has_shell_integration,
        input_generation,
        last_command_finished_input_generation,
    ) {
        return PaneMode::Shell;
    }

    PaneMode::Unknown
}

pub fn shell_metadata_is_fresh(
    has_shell_integration: bool,
    input_generation: u64,
    last_command_finished_input_generation: u64,
) -> bool {
    has_shell_integration && input_generation == last_command_finished_input_generation
}

pub fn direct_terminal_exec_is_safe(runtime: &PaneRuntimeState) -> bool {
    PaneControlState::from_runtime(runtime).allows_visible_shell_exec()
}
// 从 con-agent::context 迁移：终端 pane 运行时状态与观察类型

fn format_runtime_stack(scopes: &[PaneRuntimeScope]) -> String {
    if scopes.is_empty() {
        "unknown".to_string()
    } else {
        scopes
            .iter()
            .map(PaneRuntimeScope::summary)
            .collect::<Vec<_>>()
            .join(" > ")
    }
}

// ── 屏幕观察与工作区推导 ──────────────────────────────────────────

// --- fn_classify ---
pub fn classify_screen_agent_cli(_title: Option<&str>, lines: &[String]) -> Option<&'static str> {
    let joined = lines.join("\n").to_ascii_lowercase();

    if joined.contains("openai codex") {
        return Some("codex");
    }

    if joined.contains("claude code") {
        return Some("claude");
    }

    let opencode_markers = [
        joined.contains("ask anything"),
        joined.contains("tab agents"),
        joined.contains("ctrl+p commands"),
    ];
    if opencode_markers
        .into_iter()
        .filter(|present| *present)
        .count()
        >= 2
    {
        return Some("opencode");
    }

    None
}

// --- fn_derive_screen_hints ---
pub fn derive_screen_hints(title: Option<&str>, lines: &[String]) -> Vec<PaneObservationHint> {
    let mut hints = Vec::new();

    let non_empty: Vec<&str> = lines
        .iter()
        .map(|line| line.trim_end())
        .filter(|line| !line.trim().is_empty())
        .collect();

    if let Some(line) = non_empty
        .iter()
        .rev()
        .take(3)
        .find(|line| is_prompt_like_line(line))
    {
        hints.push(PaneObservationHint {
            kind: PaneObservationHintKind::PromptLikeInput,
            confidence: PaneConfidence::Advisory,
            detail: format!(
                "A prompt-like input line is visible near the bottom of the current screen: `{}`.",
                line.trim()
            ),
        });
    }

    let htop_markers = [
        "Load average:",
        "Tasks:",
        "PID USER",
        "TIME+  Command",
        "Swp[",
        "Mem[",
    ];
    let marker_count = htop_markers
        .iter()
        .filter(|marker| lines.iter().any(|line| line.contains(**marker)))
        .count();
    if marker_count >= 2 {
        hints.push(PaneObservationHint {
            kind: PaneObservationHintKind::HtopLikeScreen,
            confidence: PaneConfidence::Advisory,
            detail: "The current visible screen resembles htop output.".to_string(),
        });
    }

    if lines.iter().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("Last login:") || trimmed == "System information as of"
    }) {
        hints.push(PaneObservationHint {
            kind: PaneObservationHintKind::LoginBannerVisible,
            confidence: PaneConfidence::Advisory,
            detail: "The current visible screen includes a login banner or shell welcome text."
                .to_string(),
        });
    }

    if lines.iter().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("Connection to ") && trimmed.contains(" closed")
    }) {
        hints.push(PaneObservationHint {
            kind: PaneObservationHintKind::SshConnectionClosed,
            confidence: PaneConfidence::Advisory,
            detail: "The current visible screen shows that an SSH connection was closed."
                .to_string(),
        });
    }

    if title.is_some_and(title_looks_tmux_like) {
        hints.push(PaneObservationHint {
            kind: PaneObservationHintKind::TmuxLikeScreen,
            confidence: PaneConfidence::Advisory,
            detail: "The pane title resembles a tmux session or tmux status title. Treat this as an observation, not native tmux proof.".to_string(),
        });
    }

    hints
}

/// Classifies whether recent visible pane lines appear to be an interactive agent CLI.
///
/// Pass the current pane title, when available, and the current visible screen or other recent
/// pane lines. Returns `"codex"`, `"claude"`, or `"opencode"` when the marker heuristics match a
/// known agent CLI; returns `None` for ordinary shell output or unknown TUIs. Detection is based on
/// visible text markers, so stale scrollback, localized UI text, or future CLI UI changes can cause
/// false positives or false negatives.
// --- fn_remote_anchor ---
pub fn remote_workspace_anchor(
    runtime: &PaneRuntimeState,
    observation: &PaneObservationFrame,
) -> Option<RemoteWorkspaceAnchor> {
    if let (Some(host), Some(confidence), Some(source)) = (
        runtime.remote_host.clone(),
        runtime.remote_host_confidence,
        runtime.remote_host_source,
    ) {
        return Some(RemoteWorkspaceAnchor {
            host,
            source,
            confidence,
            note: "Remote host is directly anchored by pane-local runtime evidence.".to_string(),
        });
    }

    let host = ssh_target_from_recent_actions(&runtime.recent_actions)?;
    let prompt_like = observation
        .screen_hints
        .iter()
        .any(|hint| hint.kind == PaneObservationHintKind::PromptLikeInput);
    let has_tmux = runtime.tmux_session.is_some()
        || has_tmux_scope(&runtime.scope_stack)
        || has_tmux_scope(&runtime.last_verified_scope_stack);
    let has_interactive_front = matches!(runtime.front_state, PaneFrontState::InteractiveSurface)
        || runtime.mode == PaneMode::Tui
        || runtime.agent_cli.is_some();

    if observation.is_busy || has_tmux || has_interactive_front || !prompt_like {
        return None;
    }

    Some(RemoteWorkspaceAnchor {
        host,
        source: PaneEvidenceSource::ActionHistory,
        confidence: PaneConfidence::Advisory,
        note: "Cube created or used this pane for SSH recently, and the current screen still looks prompt-like without contradictory tmux/TUI evidence.".to_string(),
    })
}

fn title_looks_tmux_like(title: &str) -> bool {
    let lower = title.to_ascii_lowercase();
    if lower.contains("tmux") || lower.contains("tmate") {
        return true;
    }

    let has_window_box = title.chars().any(|ch| matches!(ch, '❐' | '❑' | '❏'));
    let has_window_state = title.chars().any(|ch| matches!(ch, '●' | '○' | '◉' | '*'));
    let has_numbered_window = title
        .split_whitespace()
        .any(|token| token.chars().next().is_some_and(|ch| ch.is_ascii_digit()));

    has_window_box && has_window_state && has_numbered_window
}

fn parse_ssh_target(command: &str) -> Option<String> {
    let mut tokens = command.split_whitespace().peekable();
    while let Some(token) = tokens.next() {
        let token = token.trim_matches(&['"', '\''][..]);
        if token.is_empty() {
            continue;
        }
        if token.contains('=') && !token.starts_with('-') {
            continue;
        }
        let basename = token.rsplit('/').next().unwrap_or(token);
        if basename != "ssh" {
            continue;
        }
        while let Some(arg) = tokens.next() {
            let arg = arg.trim_matches(&['"', '\''][..]);
            if arg.is_empty() {
                continue;
            }
            if arg == "--" {
                return tokens
                    .next()
                    .map(|target| target.trim_matches(&['"', '\''][..]).to_string());
            }
            if arg.starts_with('-') {
                let takes_value = matches!(
                    arg,
                    "-b" | "-c"
                        | "-D"
                        | "-E"
                        | "-e"
                        | "-F"
                        | "-I"
                        | "-i"
                        | "-J"
                        | "-L"
                        | "-l"
                        | "-m"
                        | "-O"
                        | "-o"
                        | "-p"
                        | "-Q"
                        | "-R"
                        | "-S"
                        | "-W"
                        | "-w"
                );
                if takes_value && !arg.contains('=') {
                    let _ = tokens.next();
                }
                continue;
            }
            return Some(arg.to_string());
        }
    }
    None
}

// --- fn_ssh_target_from_recent_actions ---
pub fn ssh_target_from_recent_actions(actions: &[PaneActionRecord]) -> Option<String> {
    actions
        .iter()
        .rev()
        .filter_map(|action| action.command.as_deref())
        .find_map(parse_ssh_target)
}

// --- fn_has_tmux_scope ---
fn has_tmux_scope(scopes: &[PaneRuntimeScope]) -> bool {
    scopes
        .iter()
        .any(|scope| scope.kind == PaneScopeKind::Multiplexer)
}

// --- fn_is_prompt_like_line ---
fn is_prompt_like_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.len() > 100 || trimmed.contains("Expected:") {
        return false;
    }
    if trimmed.contains("  ") {
        return false;
    }

    let starts_like_prompt = trimmed
        .chars()
        .next()
        .is_some_and(|c| matches!(c, '$' | '#' | '%' | '>' | ')' | '❯'));
    let ends_like_prompt = trimmed
        .chars()
        .last()
        .is_some_and(|c| matches!(c, '$' | '#' | '%' | '>'));

    starts_like_prompt || ends_like_prompt
}
