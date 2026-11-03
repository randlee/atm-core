use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use crate::error::AtmError;
use crate::schema::{AtmMessageId, InboxMessage, MessageEnvelope};
use crate::task_state::TaskCloseOutcome;
use crate::types::{AgentName, IsoTimestamp, MemberKey, TaskId, TeamName};

#[doc(hidden)]
pub mod sealed {
    pub trait Sealed {}
}

pub(crate) fn require_non_blank(value: String, subject: &str) -> Result<String, AtmError> {
    if value.trim().is_empty() {
        return Err(AtmError::validation(format!("{subject} must not be blank")));
    }
    Ok(value)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct MessageKey(String);

impl MessageKey {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        require_non_blank(value.into(), "message key").map(Self)
    }

    pub fn into_inner(self) -> String {
        self.0
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_atm_message_id(&self) -> Result<AtmMessageId, AtmError> {
        let raw = self.as_str().strip_prefix("atm:").unwrap_or(self.as_str());
        raw.parse::<AtmMessageId>()
            .map_err(|error| AtmError::validation(format!("message key parse failed: {error}")))
    }

    fn from_atm_message_id(value: AtmMessageId) -> Self {
        let mut key = String::with_capacity(4 + value.to_string().len());
        key.push_str("atm:");
        key.push_str(&value.to_string());
        Self(key)
    }
}

impl From<AtmMessageId> for MessageKey {
    fn from(value: AtmMessageId) -> Self {
        Self::from_atm_message_id(value)
    }
}

impl FromStr for MessageKey {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl fmt::Display for MessageKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AsRef<str> for MessageKey {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

pub use crate::peer_contract::*;
pub use crate::read_lane_error::ReadLaneError;
pub use crate::task_state::{TaskEventRow, TaskRow};
pub use crate::task_store::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct AckTransition(String);

impl AckTransition {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        require_non_blank(value.into(), "ack transition").map(Self)
    }
}

impl AsRef<str> for AckTransition {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Deref for AckTransition {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for AckTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl FromStr for AckTransition {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BuiltInNudgeTemplateKind {
    Delivery,
    DeliveryAck,
    Queue,
    QueueAck,
    Acknowledge,
    TaskQueued,
    TaskReady,
    TaskReminder,
    TaskStarted,
    TaskComplete,
    TaskClosed,
    /// Cron lifecycle nudge override family; separate from post-send events.
    Cron,
}

impl BuiltInNudgeTemplateKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Delivery => "delivery",
            Self::DeliveryAck => "delivery_ack",
            Self::Queue => "queue",
            Self::QueueAck => "queue_ack",
            Self::Acknowledge => "acknowledge",
            Self::TaskQueued => "task_queued",
            Self::TaskReady => "task_ready",
            Self::TaskReminder => "task_reminder",
            Self::TaskStarted => "task_started",
            Self::TaskComplete => "task_complete",
            Self::TaskClosed => "task_closed",
            Self::Cron => "cron",
        }
    }
}

impl fmt::Display for BuiltInNudgeTemplateKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for BuiltInNudgeTemplateKind {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        crate::validation::parse_built_in_template_kind(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TeamNudgeTemplateOverrideMode {
    Override { template_body: String },
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TeamNudgeTemplateOverrideRow {
    pub team_name: TeamName,
    pub kind: BuiltInNudgeTemplateKind,
    pub mode: TeamNudgeTemplateOverrideMode,
    pub updated_at: IsoTimestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StaleNudgeTemplateOverrideKind {
    pub kind: String,
    pub updated_at: IsoTimestamp,
}

impl TeamNudgeTemplateOverrideRow {
    pub fn template_body(&self) -> Option<&str> {
        match &self.mode {
            TeamNudgeTemplateOverrideMode::Override { template_body } => Some(template_body),
            TeamNudgeTemplateOverrideMode::Disabled => None,
        }
    }

    pub fn is_disabled(&self) -> bool {
        matches!(self.mode, TeamNudgeTemplateOverrideMode::Disabled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AckRequirementState {
    NotRequired,
    RequiredPending,
    RequiredAcknowledged,
}

pub fn derive_ack_requirement(message: &InboxMessage) -> AckRequirementState {
    if !message.requires_ack {
        AckRequirementState::NotRequired
    } else if message.acknowledged_at.is_some() {
        AckRequirementState::RequiredAcknowledged
    } else {
        AckRequirementState::RequiredPending
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub team: TeamName,
    pub agent: AgentName,
    pub message_key: MessageKey,
    pub envelope: MessageEnvelope,
}

/// Result of admitting one immutable message and applying any governed task
/// operation carried by that newly inserted local message.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MessageAdmissionOutcome {
    pub existing: Option<Message>,
    pub already_closed: Option<TaskCloseOutcome>,
    /// Assignee snapshot for an applied start or close operation.
    pub task_assignee: Option<AgentName>,
    /// Landed queue position for an assignment admitted by this write.
    pub queued_position: Option<u32>,
    /// Canonical reassignment notice inserted atomically for the old assignee.
    pub reassign_notice: Option<Message>,
    /// A governed task operation rejected after its report was retained as
    /// ordinary mail. Callers must complete ordinary post-write handling
    /// before surfacing this error to the sender.
    pub task_rejection: Option<AtmError>,
    /// Task-ledger rows appended atomically with this successful admission.
    /// The enclosing `CommittedTaskWrite::task_events` is then empty; it holds
    /// rows only for a rejected operation.
    pub task_events: Vec<TaskEventRow>,
}

impl MessageAdmissionOutcome {
    #[must_use]
    pub fn passive(existing: Option<Message>) -> Self {
        Self {
            existing,
            ..Self::default()
        }
    }
}

/// Aggregate display counts for one mailbox without materializing its messages.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MailboxBucketCounts {
    pub unread: usize,
    pub pending_ack: usize,
    pub history: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailMessageState {
    pub team: TeamName,
    pub agent: AgentName,
    pub actor: AgentName,
    pub message_key: MessageKey,
    pub read: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_ack_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledged_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<IsoTimestamp>,
}

/// Opaque hash or content-addressable identifier that marks the last
/// successfully ingested message boundary for a replay source.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageFingerprint(String);

impl MessageFingerprint {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MessageFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<String> for MessageFingerprint {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl AsRef<str> for MessageFingerprint {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageQuery {
    pub team: TeamName,
    pub agent: AgentName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender: Option<AgentName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// A storage-owned, criteria-based mailbox page.
///
/// Unlike [`MessageQuery`], this carries the complete typed predicate to the
/// reader backend so it can apply visibility, successor collapse, ordering,
/// and a page limit before it materializes message envelopes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailboxListQuery {
    pub filters: crate::search::SearchFilters,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// The mailbox a read operation is authorized to inspect.
///
/// This deliberately travels with every asynchronous mailbox request instead
/// of relying on callers to keep the team and agent embedded in a query in
/// sync.  Backends must reject a mismatch before they schedule database work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailboxScope {
    pub team: TeamName,
    pub agent: AgentName,
}

impl MailboxScope {
    #[must_use]
    pub const fn new(team: TeamName, agent: AgentName) -> Self {
        Self { team, agent }
    }

    #[must_use]
    pub fn permits(&self, query: &MessageQuery) -> bool {
        self.team == query.team && self.agent == query.agent
    }
}

/// A storage-owned deadline for the bounded mailbox reader lane.
///
/// `atm-storage` intentionally does not depend on `atm-core`; the Tokio
/// runtime translates its request deadline at its boundary before calling the
/// reader capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadDeadline {
    remaining: Duration,
}

impl ReadDeadline {
    pub fn new(remaining: Duration) -> Result<Self, AtmError> {
        if remaining.is_zero() {
            return Err(AtmError::validation(
                "mailbox read deadline must be non-zero",
            ));
        }
        Ok(Self { remaining })
    }

    #[must_use]
    pub const fn remaining(self) -> Duration {
        self.remaining
    }
}

mod roster;
pub use roster::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageReceivedEvent {
    pub team: TeamName,
    pub agent: AgentName,
    pub message_key: MessageKey,
    pub timestamp: IsoTimestamp,
}

/// Identifies the pending source record that an acknowledgement admission
/// must resolve inside the writer transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcknowledgementSource {
    pub team: TeamName,
    pub agent: AgentName,
    pub message_id: AtmMessageId,
}

/// Builds the immutable acknowledgement reply after the writer has loaded the
/// pending source record, but before that transaction commits.  The callback
/// deliberately receives no storage handle: it may derive the reply from the
/// source but cannot perform a second application-layer source read.
pub trait AcknowledgementReplyBuilder: Send + Sync {
    fn build_reply(&self, source: &Message) -> Result<Message, AtmError>;
}

/// The records made durable by one acknowledgement admission transaction.
#[derive(Debug, Clone, PartialEq)]
pub struct AcknowledgementCommit {
    pub reply: Message,
    pub source: Message,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterChangedEvent {
    pub team: TeamName,
    pub member_count: usize,
    pub timestamp: IsoTimestamp,
}

pub trait MessageStore: sealed::Sealed + Send + Sync {
    fn save_message(&self, message: &Message) -> Result<(), AtmError>;
    /// Makes one immutable message durable, or returns the record that already
    /// owns its key. Production stores should override this so the normal
    /// insert path does not perform a separate reader round trip before the
    /// writer transaction.
    fn save_message_if_absent(&self, message: &Message) -> Result<Option<Message>, AtmError> {
        if let Some(existing) = self.load_message(&message.message_key)? {
            return Ok(Some(existing));
        }
        self.save_message(message)?;
        Ok(None)
    }
    /// Provenance-aware admission that also returns the governed task-close
    /// result produced by a newly inserted local message.
    fn admit_message_with_provenance(
        &self,
        message: &Message,
        provenance: MessageWriteOrigin,
    ) -> Result<crate::CommittedTaskWrite<MessageAdmissionOutcome>, AtmError>;
    /// Commits related immutable mailbox records as one durable unit.
    ///
    /// AI.31 uses this for an acknowledgement reply plus the acknowledged
    /// source record; adapters must not expose a partially committed pair.
    fn save_messages_atomically(&self, messages: &[Message]) -> Result<(), AtmError>;
    /// Resolves a pending source, builds its immutable reply, and transitions
    /// the source in one writer transaction.  The default preserves backward
    /// compatibility for narrow test doubles; production stores must override
    /// it rather than compose a read plus `save_messages_atomically`.
    fn acknowledge_message_atomically(
        &self,
        _source: &AcknowledgementSource,
        _builder: Arc<dyn AcknowledgementReplyBuilder>,
    ) -> Result<AcknowledgementCommit, AtmError> {
        Err(AtmError::daemon_unavailable(
            "message store does not implement atomic acknowledgement admission",
        ))
    }
    fn load_message(&self, key: &MessageKey) -> Result<Option<Message>, AtmError>;
    fn list_messages(&self, query: &MessageQuery) -> Result<Vec<Message>, AtmError>;
    /// Returns mailbox display counts when the backend can aggregate them
    /// without materializing every immutable message record.
    fn mailbox_bucket_counts(
        &self,
        _team: &TeamName,
        _agent: &AgentName,
    ) -> Result<Option<MailboxBucketCounts>, AtmError> {
        Ok(None)
    }
    fn delete_message(&self, key: &MessageKey) -> Result<(), AtmError>;
    /// Deletes a message while preserving its scheduler generation fence in
    /// the durable writer transaction. Compatibility stores accept only Local.
    fn delete_message_with_origin(
        &self,
        key: &MessageKey,
        origin: MessageWriteOrigin,
    ) -> Result<(), AtmError> {
        if origin == MessageWriteOrigin::Local {
            self.delete_message(key)
        } else {
            Err(AtmError::validation(
                "message store does not implement fenced message deletion",
            ))
        }
    }
}

/// Async durable-admission boundary used by the Tokio HTTP runtime.
///
/// The future represents bounded admission to the backend's one ordered write
/// lane and the durable result from that lane. Implementations may keep the
/// actual database connection synchronous; callers must never create a
/// blocking task merely to submit or await a message write.
///
/// `MessageStore` remains the compatibility surface for non-Tokio callers.
/// New daemon write paths use this trait so an implementation can provide
/// async backpressure without exposing its transaction queue or database.
#[async_trait::async_trait]
pub trait AsyncMessageStore: MessageStore {
    /// Applies the narrow state transition requested by a successful mailbox
    /// read. Selection remains reader-lane work; this is the sole ordered
    /// writer-lane follow-up and is deliberately asynchronous.
    async fn apply_read_display_state_async(
        &self,
        _scope: MailboxScope,
        _message_ids: Vec<MessageKey>,
        _seen_watermark: Option<IsoTimestamp>,
    ) -> Result<(), AtmError> {
        Err(AtmError::daemon_unavailable(
            "message store does not implement async mailbox read-state transition",
        ))
    }

    /// Applies display state with an origin checked by the durable writer.
    /// Narrow compatibility stores continue to support Local reads only.
    async fn apply_read_display_state_with_origin_async(
        &self,
        scope: MailboxScope,
        message_ids: Vec<MessageKey>,
        seen_watermark: Option<IsoTimestamp>,
        origin: MessageWriteOrigin,
    ) -> Result<(), AtmError> {
        if origin == MessageWriteOrigin::Local {
            self.apply_read_display_state_async(scope, message_ids, seen_watermark)
                .await
        } else {
            Err(AtmError::validation(
                "message store does not implement fenced mailbox read-state transition",
            ))
        }
    }

    /// Makes one immutable message durable, or returns the record that already
    /// owns its key, without blocking the Tokio request executor.
    async fn save_message_if_absent_async(
        &self,
        message: Message,
    ) -> Result<Option<Message>, AtmError> {
        self.save_message_if_absent(&message)
    }

    /// Async companion to [`MessageStore::admit_message_with_provenance`].
    async fn admit_message_with_provenance_async(
        &self,
        message: Message,
        provenance: MessageWriteOrigin,
    ) -> Result<crate::CommittedTaskWrite<MessageAdmissionOutcome>, AtmError>;

    /// Atomically admits a mailbox record and its template decomposition on
    /// the backend-owned async writer lane.
    async fn admit_template_message_async(
        &self,
        _admission: crate::TemplateMessageAdmission,
    ) -> Result<crate::CommittedTaskWrite<MessageAdmissionOutcome>, AtmError> {
        Err(AtmError::daemon_unavailable(
            "message store does not implement async template-message admission",
        ))
    }

    /// Resolves a pending acknowledgement source, persists its reply, and
    /// transitions that source as one async durable admission. A refused
    /// acknowledgement still returns its committed rejection-audit rows.
    async fn acknowledge_message_atomically_async(
        &self,
        source: AcknowledgementSource,
        builder: Arc<dyn AcknowledgementReplyBuilder>,
    ) -> Result<crate::CommittedTaskWrite<AcknowledgementCommit>, AtmError> {
        self.acknowledge_message_atomically(&source, builder)
            .map(|commit| crate::CommittedTaskWrite {
                operation: Ok(commit),
                task_events: Vec::new(),
            })
    }

    /// Cron-aware acknowledgement admission. The default remains compatible
    /// for ordinary local ACKs and refuses a cron fence unless a backend can
    /// verify it in the same transaction as source transition and reply write.
    async fn acknowledge_message_atomically_with_origin_async(
        &self,
        source: AcknowledgementSource,
        builder: Arc<dyn AcknowledgementReplyBuilder>,
        origin: MessageWriteOrigin,
    ) -> Result<crate::CommittedTaskWrite<AcknowledgementCommit>, AtmError> {
        if origin == MessageWriteOrigin::Local {
            self.acknowledge_message_atomically_async(source, builder)
                .await
        } else {
            Err(AtmError::validation(
                "message store does not implement fenced acknowledgement admission",
            ))
        }
    }
}

/// Tokio-safe, read-only mailbox capability.
///
/// This is intentionally separate from [`AsyncMessageStore`]: the latter is
/// the ordered durable writer lane.  Implementations of this trait must not
/// acquire that writer lane or a write-capable database connection.
#[async_trait::async_trait]
pub trait AsyncMailboxReader: sealed::Sealed + Send + Sync {
    async fn list_messages(
        &self,
        scope: MailboxScope,
        query: MessageQuery,
        deadline: ReadDeadline,
    ) -> Result<Vec<Message>, ReadLaneError>;

    /// Lists a bounded page after applying the storage-owned typed criteria.
    ///
    /// This is deliberately separate from the narrow legacy-compatible
    /// [`MessageQuery`] shape: callers that need mailbox display selection
    /// must not load an entire mailbox merely to select one page.
    async fn list_matching_messages(
        &self,
        scope: MailboxScope,
        query: MailboxListQuery,
        deadline: ReadDeadline,
    ) -> Result<Vec<Message>, ReadLaneError>;

    /// Runs a bounded exploratory mailbox list through the tool-class slice
    /// of the shared reader pool.
    ///
    /// Implementations that do not distinguish reader-pool classes retain
    /// the ordinary read-only behavior. SQLite overrides this so operator and
    /// CLI enumeration cannot consume every reader worker.
    async fn list_messages_for_tool(
        &self,
        scope: MailboxScope,
        query: MessageQuery,
        deadline: ReadDeadline,
    ) -> Result<Vec<Message>, ReadLaneError> {
        self.list_messages(scope, query, deadline).await
    }

    /// Counts messages through the storage-owned SQL criteria path.  This
    /// deliberately returns aggregates rather than materialized messages so a
    /// mailbox list can report buckets without exhausting its read deadline.
    async fn count_messages(
        &self,
        scope: MailboxScope,
        filters: crate::search::SearchFilters,
        group_by: Option<crate::search::SearchCountGroupBy>,
        deadline: ReadDeadline,
    ) -> Result<Vec<crate::search::SearchCount>, ReadLaneError>;

    async fn load_message(
        &self,
        scope: MailboxScope,
        key: MessageKey,
        deadline: ReadDeadline,
    ) -> Result<Option<Message>, ReadLaneError>;

    /// Bounded read-only roster projection for validating an addressed mailbox.
    async fn mailbox_member_exists(
        &self,
        scope: MailboxScope,
        deadline: ReadDeadline,
    ) -> Result<bool, ReadLaneError>;

    /// Bounded durable seen state; never reads a caller-owned watermark file.
    async fn load_seen_watermark(
        &self,
        scope: MailboxScope,
        deadline: ReadDeadline,
    ) -> Result<Option<IsoTimestamp>, ReadLaneError>;
}

pub trait RosterStore: sealed::Sealed + Send + Sync {
    fn load_roster(&self, team: &TeamName) -> Result<RosterSnapshot, AtmError>;
    fn save_roster(&self, roster: &RosterSnapshot) -> Result<(), AtmError>;
    fn list_teams(&self) -> Result<Vec<TeamName>, AtmError>;

    /// Lists every effective roster name across the durable database.
    ///
    /// Backends with a native projection should override this with one query.
    /// The fallback preserves the contract for narrow test doubles.
    fn unique_names(&self) -> Result<Vec<RosterUniqueName>, AtmError> {
        let mut names = Vec::new();
        for team in self.list_teams()? {
            names.extend(
                self.load_roster(&team)?
                    .members
                    .iter()
                    .map(RosterUniqueName::from_member),
            );
        }
        Ok(names)
    }
}

/// Runtime-owned live state for one known roster member.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMemberState {
    #[default]
    Unknown,
    IdentityConflict,
    Offline,
    Idle,
    Active,
    Blocked,
}

/// Provenance of one accepted runtime observation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeObservationSource {
    Heartbeat,
    HerdrPoll,
}

/// Whether the source supplied a usable state in its most recent accepted
/// mutation. An unavailable observation retains the last known state while
/// making that staleness explicit to scheduling callers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeObservationAvailability {
    #[default]
    Unobserved,
    Fresh,
    Unavailable,
}

/// Monotonic accepted-state-observation sequence for one roster member.
/// Heartbeats and successful poll observations advance it, including
/// same-state observations. Failed/incomplete polls update availability
/// metadata without creating a new revision; an Idle revision is therefore a
/// stable attention-opportunity seam for Phase AZ scheduling.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RosterStateRevision(u64);

impl RosterStateRevision {
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[must_use]
    pub fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

/// Heartbeat-owned process/session metadata. Herdr mutations omit this value
/// and therefore cannot erase identity learned from an authenticated hook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterRuntimeIdentity {
    pub pid: u32,
    pub session_id: Option<crate::types::SessionId>,
}

/// The single ephemeral lifecycle record attached to a master-roster member.
/// Source and timestamps describe accepted mutations; they do not grant one
/// source precedence over another.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RosterRuntimeObservation {
    pub revision: RosterStateRevision,
    pub state: RuntimeMemberState,
    pub availability: RuntimeObservationAvailability,
    pub last_observation_attempt_by: Option<RuntimeObservationSource>,
    pub last_observation_attempt_at: Option<IsoTimestamp>,
    pub last_observed_by: Option<RuntimeObservationSource>,
    pub last_observed_at: Option<IsoTimestamp>,
    pub pid: Option<u32>,
    pub session_id: Option<crate::types::SessionId>,
    pub last_active_at: Option<IsoTimestamp>,
    pub state_changed_by: Option<RuntimeObservationSource>,
    pub state_changed_at: Option<IsoTimestamp>,
    pub session_changed_by: Option<RuntimeObservationSource>,
    pub session_changed_at: Option<IsoTimestamp>,
}

/// One accepted-order mutation of a roster member's canonical runtime state.
/// `state: None` records source unavailability while retaining the last known
/// state. `identity: None` preserves existing heartbeat identity metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterRuntimeObservationUpdate {
    agent: AgentName,
    state: Option<RuntimeMemberState>,
    availability: RuntimeObservationAvailability,
    source: RuntimeObservationSource,
    observed_at: IsoTimestamp,
    identity: Option<RosterRuntimeIdentity>,
}

impl RosterRuntimeObservationUpdate {
    /// Constructs one successful state observation. Successful observations
    /// are always fresh and advance the member revision when accepted.
    #[must_use]
    pub fn observed(
        agent: AgentName,
        state: RuntimeMemberState,
        source: RuntimeObservationSource,
        observed_at: IsoTimestamp,
        identity: Option<RosterRuntimeIdentity>,
    ) -> Self {
        Self {
            agent,
            state: Some(state),
            availability: RuntimeObservationAvailability::Fresh,
            source,
            observed_at,
            identity,
        }
    }

    /// Constructs one unavailable-source observation. It preserves the last
    /// successful state and identity and does not advance the member revision.
    #[must_use]
    pub fn unavailable(
        agent: AgentName,
        source: RuntimeObservationSource,
        observed_at: IsoTimestamp,
    ) -> Self {
        Self {
            agent,
            state: None,
            availability: RuntimeObservationAvailability::Unavailable,
            source,
            observed_at,
            identity: None,
        }
    }

    #[must_use]
    pub fn agent(&self) -> &AgentName {
        &self.agent
    }

    #[must_use]
    pub fn state(&self) -> Option<RuntimeMemberState> {
        self.state
    }

    #[must_use]
    pub fn availability(&self) -> RuntimeObservationAvailability {
        self.availability
    }

    #[must_use]
    pub fn source(&self) -> RuntimeObservationSource {
        self.source
    }

    #[must_use]
    pub fn observed_at(&self) -> IsoTimestamp {
        self.observed_at
    }

    #[must_use]
    pub fn identity(&self) -> Option<&RosterRuntimeIdentity> {
        self.identity.as_ref()
    }
}

/// Result of applying one canonical state mutation. Missing roster members
/// are omitted from a batch result, so consumers cannot schedule from an
/// observation that raced with removal from the master roster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterRuntimeMutationOutcome {
    pub agent: AgentName,
    pub previous_state: RuntimeMemberState,
    pub current: RosterRuntimeObservation,
    pub state_changed: bool,
    pub pid_changed: bool,
}

/// Ephemeral per-member roster state that never round-trips through the
/// durable roster store. Nothing here is ever read from or written to a
/// durable backend.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RosterMemberEphemeralState {
    /// The canonical lifecycle state used by runtime projections and
    /// scheduling decisions.
    pub runtime: RosterRuntimeObservation,
    /// Set/cleared by Herdr queue-wake bookkeeping when a member's steer
    /// target is pending a wake attempt.
    pub herdr_wake_pending: bool,
}

/// Cross-crate seam for the runtime-owned, write-through in-memory roster
/// mirror. Every roster consumer reads through this handle after startup
/// hydration; a durable roster write updates the backing mirror in the same
/// operation through the paired [`RosterStore`] write-through implementation.
///
/// The concrete write-through implementation (the [`RosterStore`] impl and
/// the state backing both this trait and that impl) is authorized only at
/// the `atm-storage-rusqlite` composition boundary (see
/// `boundaries/atm-storage-rusqlite/roster-store-sqlite.toml`). This trait
/// exists so `atm-core` and other consumer crates can hold and call the
/// mirror without depending on that concrete backend crate.
pub trait RosterRuntimeMirror: sealed::Sealed + Send + Sync {
    /// Reads one team's roster from RAM. Never issues a durable read.
    fn load_team_roster(&self, team: &TeamName) -> Vec<RosterMember>;
    /// Reads one roster member from RAM. Never issues a durable read.
    fn load_roster_member(&self, team: &TeamName, agent: &AgentName) -> Option<RosterMember>;
    /// Enumerates every team currently held in RAM. Never issues a durable read.
    fn list_teams(&self) -> Vec<TeamName>;
    /// Reads one member's ephemeral (non-durable) roster state from RAM.
    fn ephemeral_state(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Option<RosterMemberEphemeralState>;
    /// Runs one terminal-admission operation while holding the member team's
    /// roster read guard. Runtime observations and roster replacements take
    /// the paired write guard, so they cannot change an admitted state or
    /// epoch before the operation finishes its durable marker write.
    ///
    /// The operation must not re-enter this mirror for the same team.
    fn with_member_runtime_admission(
        &self,
        team: &TeamName,
        agent: &AgentName,
        operation: &mut dyn FnMut(Option<RosterMemberEphemeralState>) -> Result<(), AtmError>,
    ) -> Result<(), AtmError>;
    /// Applies one accepted-order batch of runtime observations under the
    /// team's roster lock. Missing members produce no outcome.
    fn apply_runtime_observations(
        &self,
        team: &TeamName,
        updates: &[RosterRuntimeObservationUpdate],
    ) -> Vec<RosterRuntimeMutationOutcome>;
    /// Reads one team's canonical runtime observations from RAM in roster
    /// order. Never issues a durable read.
    fn load_runtime_observations(
        &self,
        team: &TeamName,
    ) -> Vec<(AgentName, RosterRuntimeObservation)>;
    /// Sets one member's Herdr wake-pending ephemeral flag in RAM only.
    /// Returns `false` without effect when the member is not present in the
    /// current roster snapshot.
    fn set_herdr_wake_pending(&self, team: &TeamName, agent: &AgentName, pending: bool) -> bool;
    /// Re-hydrates the RAM roster mirror from the durable roster store.
    ///
    /// This is *not* the write-through synchronization mechanism -- every
    /// durable roster write already updates RAM in the same operation. This
    /// exists only for an authenticated control-plane reload boundary that
    /// wants to re-derive RAM from durable state (e.g. after an out-of-band
    /// durable migration).
    ///
    /// # Errors
    /// Returns an error if the durable roster store cannot be read; RAM is
    /// left unchanged on failure.
    fn reload_from_durable(&self) -> Result<(), AtmError>;
}

pub trait StorageNotifier: sealed::Sealed + Send + Sync {
    fn message_received(&self, event: &MessageReceivedEvent) -> Result<(), AtmError>;
    fn roster_changed(&self, event: &RosterChangedEvent) -> Result<(), AtmError>;
}

pub trait NudgeTemplateOverrideStore: sealed::Sealed + Send + Sync {
    fn list_stale_template_override_kinds(
        &self,
        team: &TeamName,
    ) -> Result<Vec<StaleNudgeTemplateOverrideKind>, AtmError>;

    fn list_template_overrides(
        &self,
        team: &TeamName,
    ) -> Result<Vec<TeamNudgeTemplateOverrideRow>, AtmError>;

    fn load_template_override(
        &self,
        team: &TeamName,
        kind: BuiltInNudgeTemplateKind,
    ) -> Result<Option<TeamNudgeTemplateOverrideRow>, AtmError>;

    fn save_template_override(
        &self,
        team: &TeamName,
        kind: BuiltInNudgeTemplateKind,
        template_body: &str,
    ) -> Result<TeamNudgeTemplateOverrideRow, AtmError>;

    fn disable_template_override(
        &self,
        team: &TeamName,
        kind: BuiltInNudgeTemplateKind,
    ) -> Result<TeamNudgeTemplateOverrideRow, AtmError>;

    fn clear_template_override(&self, team: &TeamName, kind: &str) -> Result<bool, AtmError>;
}

/// Maximum automatic delivery attempts for one deferred (queue-kind) nudge.
///
/// At or above this count the marker stays set but becomes auto-retry
/// ineligible and the row is reported stuck (ADR-054 (f)).
pub const MAX_NUDGE_ATTEMPTS: u32 = 5;

/// One claimed deferred nudge: the message and the failed-attempt count that
/// preceded this claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NudgeClaim {
    pub msg: AtmMessageId,
    /// Value of `nudge_attempts` when the claim was taken. `requeue_pending`
    /// stores `attempt + 1`; `release_pending` leaves it unchanged.
    pub attempt: u32,
}

/// Durable at-most-once delivery state for deferred (`atm queue`) nudges.
///
/// The store owns one marker column pair on `mail_message_states`; no caller
/// above the backend crate writes SQL. All methods are synchronous.
pub trait PendingNudgeStore: sealed::Sealed + Send + Sync {
    /// Marks one just-admitted message as awaiting a deferred nudge.
    ///
    /// Conditional on the row still being unread and not deleted. Returns
    /// whether the marker was set.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] if the underlying storage operation fails.
    fn mark_pending(
        &self,
        member: &MemberKey,
        msg: &AtmMessageId,
        at: IsoTimestamp,
    ) -> Result<bool, AtmError>;

    /// Atomically selects and claims the oldest eligible pending message.
    ///
    /// Eligibility requires the row to be unread, marker set, not deleted,
    /// and `nudge_attempts < MAX_NUDGE_ATTEMPTS`. Selection order is FIFO via
    /// `message_key` (ULID order). `None` means nothing was eligible, or the
    /// claim lost a race to another caller. This is THE at-most-once
    /// mechanism: one conditional `UPDATE … RETURNING`.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] if the underlying storage operation fails.
    fn claim_next_pending(&self, member: &MemberKey) -> Result<Option<NudgeClaim>, AtmError>;

    /// Restores the marker after a failed dispatch.
    ///
    /// Sets `nudge_attempts = claim.attempt + 1`.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] if the underlying storage operation fails.
    fn requeue_pending(&self, member: &MemberKey, claim: &NudgeClaim) -> Result<(), AtmError>;

    /// Restores a claim refused for a lifecycle reason (AQ2.7 `agent_blocked`).
    ///
    /// Leaves `nudge_attempts` unchanged. Conditional and idempotent on claim
    /// identity.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] if the underlying storage operation fails.
    fn release_pending(&self, member: &MemberKey, claim: &NudgeClaim) -> Result<(), AtmError>;

    /// Re-arms one just-handed-off message at its next due time while it is
    /// still open. A message read between claim and handoff is not re-armed.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] if the underlying storage operation fails.
    fn rearm_pending_after_handoff(
        &self,
        member: &MemberKey,
        msg: &AtmMessageId,
        next_due: IsoTimestamp,
    ) -> Result<(), AtmError>;

    /// Reserves a terminal queue item before its one-shot handoff by clearing
    /// its marker while leaving the mailbox item visible and unread. The
    /// caller must not hand off when this returns `false`.
    fn complete_pending_before_terminal_handoff(
        &self,
        member: &MemberKey,
        claim: &NudgeClaim,
    ) -> Result<bool, AtmError>;

    /// Restores a terminal queue item's marker after the handoff reports a
    /// failure whose contract guarantees that no input was accepted.
    fn restore_pending_after_failed_terminal_handoff(
        &self,
        member: &MemberKey,
        claim: &NudgeClaim,
    ) -> Result<bool, AtmError>;

    /// Enumerates members holding at least one eligible pending marker.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] if the underlying storage operation fails.
    fn list_pending_members(&self) -> Result<Vec<MemberKey>, AtmError>;
}

/// One configurable pending-store double shared across workspace tests.
#[doc(hidden)]
#[cfg(any(test, feature = "test-utils"))]
pub struct DummyPendingNudgeStore(pub(crate) crate::testing::PendingStoreState);

#[cfg(any(test, feature = "test-utils"))]
impl sealed::Sealed for DummyPendingNudgeStore {}

#[cfg(any(test, feature = "test-utils"))]
impl PendingNudgeStore for DummyPendingNudgeStore {
    fn mark_pending(
        &self,
        member: &MemberKey,
        msg: &AtmMessageId,
        at: IsoTimestamp,
    ) -> Result<bool, AtmError> {
        self.mark(member, msg, at)
    }

    fn claim_next_pending(&self, member: &MemberKey) -> Result<Option<NudgeClaim>, AtmError> {
        self.claim(member)
    }

    fn requeue_pending(&self, member: &MemberKey, claim: &NudgeClaim) -> Result<(), AtmError> {
        self.requeue(member, claim)
    }

    fn release_pending(&self, member: &MemberKey, claim: &NudgeClaim) -> Result<(), AtmError> {
        self.release(member, claim)
    }

    fn rearm_pending_after_handoff(
        &self,
        member: &MemberKey,
        msg: &AtmMessageId,
        next_due: IsoTimestamp,
    ) -> Result<(), AtmError> {
        if let Some(failure) = self.rearm_failure() {
            return Err(failure);
        }
        self.inner().map_or(Ok(()), |inner| {
            inner.rearm_pending_after_handoff(member, msg, next_due)
        })
    }

    fn complete_pending_before_terminal_handoff(
        &self,
        member: &MemberKey,
        claim: &NudgeClaim,
    ) -> Result<bool, AtmError> {
        self.inner().map_or(Ok(true), |inner| {
            inner.complete_pending_before_terminal_handoff(member, claim)
        })
    }

    fn restore_pending_after_failed_terminal_handoff(
        &self,
        member: &MemberKey,
        claim: &NudgeClaim,
    ) -> Result<bool, AtmError> {
        self.inner().map_or(Ok(true), |inner| {
            inner.restore_pending_after_failed_terminal_handoff(member, claim)
        })
    }

    fn list_pending_members(&self) -> Result<Vec<MemberKey>, AtmError> {
        self.list()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AckRequirementState, AtmMessageId, BuiltInNudgeTemplateKind, CertificateFingerprint,
        GraftReceiverEndpointStore, GraftReceiverRegistration, Message, MessageKey, MessageQuery,
        MessageReceivedEvent, MessageStore, NudgeClaim, NudgeTemplateOverrideStore,
        PendingNudgeStore, PrivateKeyRef, RosterChangedEvent, RosterHarness, RosterMember,
        RosterMemberKind, RosterSnapshot, RosterStore, RosterUniqueName,
        StaleNudgeTemplateOverrideKind, StorageNotifier, TeamNudgeTemplateOverrideMode,
        TeamNudgeTemplateOverrideRow, derive_ack_requirement, roster_write_delta, sealed,
    };
    use crate::ROLE_WORKER;
    use crate::error::AtmError;
    use crate::schema::MessageEnvelope;
    use crate::types::{
        AgentName, IsoTimestamp, LocalCapability, MemberKey, ModelName, OwnerGeneration, TeamName,
    };
    use chrono::Utc;
    use serde_json::Map;
    use std::net::SocketAddr;

    #[test]
    fn roster_write_delta_only_contains_changed_or_new_effective_names() {
        let team_a: TeamName = "team-a".parse().expect("team");
        let member: AgentName = "bob".parse().expect("agent");
        let unchanged = RosterUniqueName {
            team_name: team_a.clone(),
            agent_name: member.clone(),
            unique_name: "bob".to_owned(),
        };
        let changed = RosterUniqueName {
            unique_name: "bobby".to_owned(),
            ..unchanged.clone()
        };
        let added = RosterUniqueName {
            team_name: "team-b".parse().expect("team"),
            agent_name: "sam".parse().expect("agent"),
            unique_name: "sam".to_owned(),
        };

        assert_eq!(
            roster_write_delta(&[unchanged], &[changed.clone(), added.clone()]),
            vec![changed, added]
        );
    }

    #[derive(Default)]
    struct DummyStore;

    #[derive(Default)]
    struct DummyNudgeTemplateOverrideStore;

    // RBQA-F002/F003: the no-op `GraftReceiverEndpointStore` test double
    // lives once in `crate::testing::NoopGraftReceiverEndpointStore`,
    // shared with `atm-core`'s admission tests, instead of being duplicated
    // here.
    use crate::testing::NoopGraftReceiverEndpointStore as DummyGraftReceiverEndpointStore;

    impl sealed::Sealed for DummyStore {}

    impl MessageStore for DummyStore {
        fn save_message(&self, _message: &Message) -> Result<(), AtmError> {
            Ok(())
        }

        fn admit_message_with_provenance(
            &self,
            _message: &Message,
            _provenance: crate::MessageWriteOrigin,
        ) -> Result<crate::CommittedTaskWrite<crate::MessageAdmissionOutcome>, AtmError> {
            unreachable!("contract test double does not admit messages")
        }

        fn save_messages_atomically(&self, _messages: &[Message]) -> Result<(), AtmError> {
            Ok(())
        }

        fn load_message(&self, _key: &MessageKey) -> Result<Option<Message>, AtmError> {
            Ok(None)
        }

        fn list_messages(&self, _query: &MessageQuery) -> Result<Vec<Message>, AtmError> {
            Ok(Vec::new())
        }

        fn delete_message(&self, _key: &MessageKey) -> Result<(), AtmError> {
            Ok(())
        }
    }

    impl RosterStore for DummyStore {
        fn load_roster(&self, team: &TeamName) -> Result<RosterSnapshot, AtmError> {
            Ok(RosterSnapshot {
                team_name: team.clone(),
                members: Vec::new(),
                refreshed_at: None,
            })
        }

        fn save_roster(&self, _roster: &RosterSnapshot) -> Result<(), AtmError> {
            Ok(())
        }

        fn list_teams(&self) -> Result<Vec<TeamName>, AtmError> {
            Ok(Vec::new())
        }
    }

    impl StorageNotifier for DummyStore {
        fn message_received(&self, _event: &MessageReceivedEvent) -> Result<(), AtmError> {
            Ok(())
        }

        fn roster_changed(&self, _event: &RosterChangedEvent) -> Result<(), AtmError> {
            Ok(())
        }
    }

    impl sealed::Sealed for DummyNudgeTemplateOverrideStore {}

    impl NudgeTemplateOverrideStore for DummyNudgeTemplateOverrideStore {
        fn list_stale_template_override_kinds(
            &self,
            _team: &TeamName,
        ) -> Result<Vec<StaleNudgeTemplateOverrideKind>, AtmError> {
            Ok(Vec::new())
        }

        fn list_template_overrides(
            &self,
            _team: &TeamName,
        ) -> Result<Vec<TeamNudgeTemplateOverrideRow>, AtmError> {
            Ok(Vec::new())
        }

        fn load_template_override(
            &self,
            _team: &TeamName,
            _kind: BuiltInNudgeTemplateKind,
        ) -> Result<Option<TeamNudgeTemplateOverrideRow>, AtmError> {
            Ok(None)
        }

        fn save_template_override(
            &self,
            team: &TeamName,
            kind: BuiltInNudgeTemplateKind,
            template_body: &str,
        ) -> Result<TeamNudgeTemplateOverrideRow, AtmError> {
            Ok(TeamNudgeTemplateOverrideRow {
                team_name: team.clone(),
                kind,
                mode: TeamNudgeTemplateOverrideMode::Override {
                    template_body: template_body.to_string(),
                },
                updated_at: IsoTimestamp::from_datetime(Utc::now()),
            })
        }

        fn disable_template_override(
            &self,
            team: &TeamName,
            kind: BuiltInNudgeTemplateKind,
        ) -> Result<TeamNudgeTemplateOverrideRow, AtmError> {
            Ok(TeamNudgeTemplateOverrideRow {
                team_name: team.clone(),
                kind,
                mode: TeamNudgeTemplateOverrideMode::Disabled,
                updated_at: IsoTimestamp::from_datetime(Utc::now()),
            })
        }

        fn clear_template_override(&self, _team: &TeamName, _kind: &str) -> Result<bool, AtmError> {
            Ok(true)
        }
    }

    #[test]
    fn storage_traits_are_object_safe() {
        let store = DummyStore;
        let message_store: &dyn MessageStore = &store;
        let roster_store: &dyn RosterStore = &store;
        let notifier: &dyn StorageNotifier = &store;
        let pending_nudge_store: &dyn PendingNudgeStore =
            &crate::testing::DummyPendingNudgeStore::default();
        let override_store: &dyn NudgeTemplateOverrideStore = &DummyNudgeTemplateOverrideStore;
        let graft_receiver_endpoint_store: &dyn GraftReceiverEndpointStore =
            &DummyGraftReceiverEndpointStore;

        let team: TeamName = "test-team".parse().expect("team");
        let agent: AgentName = ROLE_WORKER.parse().expect("agent");
        let key = MessageKey::new("atm:test-1").expect("key");
        let member = MemberKey::new(team.clone(), agent.clone());

        let message = Message {
            team: team.clone(),
            agent: agent.clone(),
            message_key: key.clone(),
            envelope: MessageEnvelope {
                from: agent.clone(),
                source_chat_id: None,
                text: "hello".to_string(),
                timestamp: IsoTimestamp::from_datetime(Utc::now()),
                read: false,
                source_team: Some(team.clone()),
                destination_chat_id: None,
                summary: None,
                message_id: None,
                requires_ack: false,
                pending_ack_at: None,
                acknowledged_at: None,
                acknowledges_message_id: None,
                parent_message_id: None,
                thread_mode: None,
                expires_at: None,
                task_id: None,
                placement: None,
                task_op: None,
                task_complete: None,
                extra: Map::new(),
            },
        };
        let roster = RosterSnapshot {
            team_name: team.clone(),
            members: vec![RosterMember {
                team_name: team.clone(),
                agent_name: agent.clone(),
                member_kind: RosterMemberKind::Permanent,
                harness: RosterHarness::ClaudeCode,
                agent_type: super::AgentType::Worker,
                model: ModelName::default(),
                recipient_pane_id: None,
                metadata_json: Map::new(),
            }],
            refreshed_at: None,
        };

        message_store.save_message(&message).expect("save message");
        assert!(message_store.load_message(&key).expect("load").is_none());
        assert!(
            message_store
                .list_messages(&MessageQuery {
                    team: team.clone(),
                    agent: agent.clone(),
                    sender: None,
                    task_id: None,
                    limit: Some(5),
                })
                .expect("list")
                .is_empty()
        );
        message_store.delete_message(&key).expect("delete");

        roster_store.save_roster(&roster).expect("save roster");
        assert_eq!(
            roster_store
                .load_roster(&team)
                .expect("load roster")
                .team_name,
            team
        );
        assert!(roster_store.list_teams().expect("list teams").is_empty());

        notifier
            .message_received(&MessageReceivedEvent {
                team: team.clone(),
                agent: agent.clone(),
                message_key: key,
                timestamp: IsoTimestamp::from_datetime(Utc::now()),
            })
            .expect("message notification");
        notifier
            .roster_changed(&RosterChangedEvent {
                team,
                member_count: 1,
                timestamp: IsoTimestamp::from_datetime(Utc::now()),
            })
            .expect("roster notification");

        let override_row = override_store
            .save_template_override(
                &"test-team".parse().expect("team"),
                BuiltInNudgeTemplateKind::DeliveryAck,
                "<atm/>",
            )
            .expect("save override");
        assert_eq!(override_row.kind, BuiltInNudgeTemplateKind::DeliveryAck);

        let msg = AtmMessageId::new();
        assert!(
            pending_nudge_store
                .mark_pending(&member, &msg, IsoTimestamp::from_datetime(Utc::now()))
                .expect("mark pending")
        );
        assert!(
            pending_nudge_store
                .claim_next_pending(&member)
                .expect("claim next pending")
                .is_none()
        );
        let claim = NudgeClaim { msg, attempt: 0 };
        pending_nudge_store
            .requeue_pending(&member, &claim)
            .expect("requeue pending");
        pending_nudge_store
            .release_pending(&member, &claim)
            .expect("release pending");
        pending_nudge_store
            .rearm_pending_after_handoff(&member, &msg, IsoTimestamp::now())
            .expect("rearm pending after handoff");
        assert!(
            pending_nudge_store
                .list_pending_members()
                .expect("list pending members")
                .is_empty()
        );

        let generation =
            OwnerGeneration::new("01J00000000000000000000001").expect("owner generation");
        let registration = GraftReceiverRegistration {
            team: "test-team".parse().expect("team"),
            agent: ROLE_WORKER.parse().expect("agent"),
            endpoint: "127.0.0.1:43101".parse::<SocketAddr>().expect("endpoint"),
            capability: LocalCapability::generate().expect("capability"),
            owner_generation: generation.clone(),
        };
        graft_receiver_endpoint_store
            .register(&registration, Utc::now())
            .expect("register");
        assert!(
            graft_receiver_endpoint_store
                .lookup(&registration.team, &registration.agent)
                .expect("lookup")
                .is_none(),
            "the dummy store never persists a lease"
        );
        graft_receiver_endpoint_store
            .refresh(
                &registration.team,
                &registration.agent,
                &generation,
                Utc::now(),
            )
            .expect("refresh");
        graft_receiver_endpoint_store
            .mark_unreachable(
                &registration.team,
                &registration.agent,
                &generation,
                Utc::now(),
            )
            .expect("mark unreachable");
        graft_receiver_endpoint_store
            .unregister(&registration.team, &registration.agent, &generation)
            .expect("unregister");
    }

    #[test]
    fn twelve_kinds_round_trip_as_str_from_str() {
        let kinds = [
            BuiltInNudgeTemplateKind::Delivery,
            BuiltInNudgeTemplateKind::DeliveryAck,
            BuiltInNudgeTemplateKind::Queue,
            BuiltInNudgeTemplateKind::QueueAck,
            BuiltInNudgeTemplateKind::Acknowledge,
            BuiltInNudgeTemplateKind::TaskQueued,
            BuiltInNudgeTemplateKind::TaskReady,
            BuiltInNudgeTemplateKind::TaskReminder,
            BuiltInNudgeTemplateKind::TaskStarted,
            BuiltInNudgeTemplateKind::TaskComplete,
            BuiltInNudgeTemplateKind::TaskClosed,
            BuiltInNudgeTemplateKind::Cron,
        ];
        for kind in kinds {
            assert_eq!(kind.as_str().parse::<BuiltInNudgeTemplateKind>(), Ok(kind));
        }
    }

    #[test]
    fn retired_kinds_parse_to_hint_naming_the_six() {
        for retired in [
            "task",
            "acknowledge_task",
            "delivery_task",
            "delivery_task_ack",
        ] {
            let error = retired
                .parse::<BuiltInNudgeTemplateKind>()
                .expect_err("retired kind");
            for replacement in [
                "task_queued",
                "task_ready",
                "task_reminder",
                "task_started",
                "task_complete",
                "task_closed",
            ] {
                assert!(error.message().contains(replacement));
            }
        }
    }

    #[test]
    fn unknown_template_kind_is_rejected_by_rust_validation() {
        let error = "future_unregistered_kind"
            .parse::<BuiltInNudgeTemplateKind>()
            .expect_err("unknown kind");
        assert!(
            error
                .message()
                .contains("unsupported built-in nudge template kind")
        );
    }

    #[test]
    fn derive_ack_requirement_ignores_task_id_and_uses_only_requires_ack_and_acknowledged_at() {
        let base = MessageEnvelope {
            from: "sender".parse().expect("agent"),
            source_chat_id: None,
            text: "hello".to_string(),
            timestamp: IsoTimestamp::from_datetime(Utc::now()),
            read: false,
            source_team: Some("test-team".parse().expect("team")),
            destination_chat_id: None,
            summary: None,
            message_id: None,
            requires_ack: false,
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: Some("AD.99".parse().expect("task")),
            placement: None,
            task_op: None,
            task_complete: None,
            extra: Map::new(),
        };

        assert_eq!(
            derive_ack_requirement(&base),
            AckRequirementState::NotRequired
        );

        let mut pending = base.clone();
        pending.requires_ack = true;
        assert_eq!(
            derive_ack_requirement(&pending),
            AckRequirementState::RequiredPending
        );

        pending.acknowledged_at = Some(IsoTimestamp::from_datetime(Utc::now()));
        assert_eq!(
            derive_ack_requirement(&pending),
            AckRequirementState::RequiredAcknowledged
        );
    }

    #[test]
    fn security_reference_newtypes_reject_blank_deserialization() {
        assert!(serde_json::from_str::<CertificateFingerprint>("\" \"").is_err());
        assert!(serde_json::from_str::<PrivateKeyRef>("\" \"").is_err());
    }
}
