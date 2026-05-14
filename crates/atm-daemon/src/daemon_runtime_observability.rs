use std::borrow::Cow;
use std::sync::Arc;

use atm_core::error::AtmError;
use atm_core::observability::ObservabilityPort;
use atm_core::schema::AtmMessageId;
use atm_core::types::{AgentName, TaskId, TeamName};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DaemonSubsystem {
    Bootstrap,
    Composition,
    LocalIpcTransport,
    AdvisoryRuntime,
    NotificationRuntime,
    PeerTransport,
    WatchRuntime,
    ReconcileRuntime,
    RuntimeHealth,
    HostOwnership,
    LifecycleControl,
    RuntimeStatusCache,
    ObservabilitySink,
}

impl DaemonSubsystem {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Bootstrap => "bootstrap",
            Self::Composition => "composition",
            Self::LocalIpcTransport => "local_ipc_transport",
            Self::AdvisoryRuntime => "advisory_runtime",
            Self::NotificationRuntime => "notification_runtime",
            Self::PeerTransport => "peer_transport",
            Self::WatchRuntime => "watch_runtime",
            Self::ReconcileRuntime => "reconcile_runtime",
            Self::RuntimeHealth => "runtime_health",
            Self::HostOwnership => "host_ownership",
            Self::LifecycleControl => "lifecycle_control",
            Self::RuntimeStatusCache => "runtime_status_cache",
            Self::ObservabilitySink => "observability_sink",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamScope {
    Team(TeamName),
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonEvent {
    pub subsystem: DaemonSubsystem,
    pub action: &'static str,
    pub outcome: &'static str,
    pub team: TeamScope,
    pub agent: Option<AgentName>,
    pub sender: Option<AgentName>,
    pub recipient: Option<AgentName>,
    pub message_id: Option<AtmMessageId>,
    pub task_id: Option<TaskId>,
    pub detail: Cow<'static, str>,
}

impl DaemonEvent {
    pub(crate) fn new(
        subsystem: DaemonSubsystem,
        action: &'static str,
        outcome: &'static str,
        detail: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            subsystem,
            action,
            outcome,
            team: TeamScope::None,
            agent: None,
            sender: None,
            recipient: None,
            message_id: None,
            task_id: None,
            detail: detail.into(),
        }
    }

    pub(crate) fn with_team(mut self, team: TeamName) -> Self {
        self.team = TeamScope::Team(team);
        self
    }

    pub(crate) fn with_agent(mut self, agent: AgentName) -> Self {
        self.agent = Some(agent);
        self
    }

    pub(crate) fn with_sender(mut self, sender: AgentName) -> Self {
        self.sender = Some(sender);
        self
    }

    pub(crate) fn with_recipient(mut self, recipient: AgentName) -> Self {
        self.recipient = Some(recipient);
        self
    }

    pub(crate) fn with_message_id(mut self, message_id: AtmMessageId) -> Self {
        self.message_id = Some(message_id);
        self
    }

    pub(crate) fn with_task_id(mut self, task_id: TaskId) -> Self {
        self.task_id = Some(task_id);
        self
    }
}

/// Daemon-runtime observability operations that stay daemon-specific above the
/// shared ATM observability boundary.
pub trait DaemonRuntimeObservability:
    atm_core::boundary::sealed::Sealed + ObservabilityPort + Send + Sync
{
    /// Emit one daemon subsystem event into the retained sink.
    fn emit_daemon_event(&self, event: DaemonEvent) -> Result<(), AtmError>;

    /// Attempt one best-effort synchronous flush during daemon shutdown.
    fn best_effort_flush_blocking(&self) -> Result<(), AtmError>;
}

#[derive(Clone)]
pub(crate) struct SubsystemObservability {
    subsystem: DaemonSubsystem,
    inner: Option<Arc<dyn DaemonRuntimeObservability>>,
}

impl std::fmt::Debug for SubsystemObservability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubsystemObservability")
            .field("subsystem", &self.subsystem)
            .field("enabled", &self.inner.is_some())
            .finish()
    }
}

impl SubsystemObservability {
    pub(crate) fn new(
        subsystem: DaemonSubsystem,
        inner: Arc<dyn DaemonRuntimeObservability>,
    ) -> Self {
        Self {
            subsystem,
            inner: Some(inner),
        }
    }

    pub(crate) fn disabled(subsystem: DaemonSubsystem) -> Self {
        Self {
            subsystem,
            inner: None,
        }
    }

    pub(crate) fn subsystem(&self) -> DaemonSubsystem {
        self.subsystem.clone()
    }

    pub(crate) fn event(
        &self,
        action: &'static str,
        outcome: &'static str,
        detail: impl Into<Cow<'static, str>>,
    ) -> DaemonEvent {
        // Shared subsystem emit helpers deliberately start with no team scope because the thin
        // sink cannot infer mailbox ownership; callers that know team context attach it
        // explicitly on the returned event instead of relying on logger-held mutable state.
        DaemonEvent::new(self.subsystem(), action, outcome, detail)
    }

    pub(crate) fn emit_event(&self, event: DaemonEvent) -> Result<(), AtmError> {
        if let Some(inner) = &self.inner {
            inner.emit_daemon_event(event)?;
        }
        Ok(())
    }

    pub(crate) fn emit_event_or_warn(&self, event: DaemonEvent) {
        if let Err(error) = self.emit_event(event.clone()) {
            tracing::warn!(
                subsystem = event.subsystem.as_str(),
                action = event.action,
                outcome = event.outcome,
                error_code = %error.code,
                error_message = %error.message,
                "daemon subsystem observability emit failed"
            );
        }
    }

    pub(crate) fn emit(
        &self,
        action: &'static str,
        outcome: &'static str,
        detail: impl Into<Cow<'static, str>>,
    ) -> Result<(), AtmError> {
        self.emit_event(self.event(action, outcome, detail))
    }

    pub(crate) fn emit_or_warn(
        &self,
        action: &'static str,
        outcome: &'static str,
        detail: impl Into<Cow<'static, str>>,
    ) {
        self.emit_event_or_warn(self.event(action, outcome, detail));
    }
}
