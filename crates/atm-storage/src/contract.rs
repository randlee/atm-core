use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fmt;
use std::net::SocketAddr;
use std::num::NonZeroU16;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use crate::error::AtmError;
use crate::schema::{AtmMessageId, InboxMessage, MessageEnvelope};
use crate::types::{
    AgentName, HostName, IsoTimestamp, LocalCapability, MemberKey, ModelName, OwnerGeneration,
    PaneId, TaskId, TeamName,
};

#[doc(hidden)]
pub mod sealed {
    pub trait Sealed {}
}

fn require_non_blank(value: String, subject: &str) -> Result<String, AtmError> {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct TaskState(String);

impl TaskState {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        require_non_blank(value.into(), "task state").map(Self)
    }
}

impl AsRef<str> for TaskState {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Deref for TaskState {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for TaskState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl FromStr for TaskState {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl PartialEq<&str> for TaskState {
    fn eq(&self, other: &&str) -> bool {
        self.as_ref() == *other
    }
}

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BuiltInNudgeTemplateKind {
    Delivery,
    DeliveryAck,
    DeliveryTask,
    DeliveryTaskAck,
    Acknowledge,
    AcknowledgeTask,
}

impl BuiltInNudgeTemplateKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Delivery => "delivery",
            Self::DeliveryAck => "delivery_ack",
            Self::DeliveryTask => "delivery_task",
            Self::DeliveryTaskAck => "delivery_task_ack",
            Self::Acknowledge => "acknowledge",
            Self::AcknowledgeTask => "acknowledge_task",
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
        match value {
            "delivery" => Ok(Self::Delivery),
            "delivery_ack" => Ok(Self::DeliveryAck),
            "delivery_task" => Ok(Self::DeliveryTask),
            "delivery_task_ack" => Ok(Self::DeliveryTaskAck),
            "acknowledge" => Ok(Self::Acknowledge),
            "acknowledge_task" => Ok(Self::AcknowledgeTask),
            other => Err(AtmError::validation(format!(
                "unsupported built-in nudge template kind `{other}`"
            ))),
        }
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

/// Explicit resource-management outcomes from a bounded reader lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadLaneError {
    UnauthorizedScope,
    Saturated { reason: &'static str },
    DeadlineExpired { stage: &'static str },
    Unavailable { message: String },
}

impl fmt::Display for ReadLaneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnauthorizedScope => {
                formatter.write_str("mailbox scope does not authorize this read")
            }
            Self::Saturated { reason } => {
                write!(formatter, "mailbox reader lane is saturated: {reason}")
            }
            Self::DeadlineExpired { stage } => {
                write!(formatter, "mailbox reader deadline expired while {stage}")
            }
            Self::Unavailable { message } => {
                write!(formatter, "mailbox reader lane is unavailable: {message}")
            }
        }
    }
}

impl std::error::Error for ReadLaneError {}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RosterMemberKind {
    Permanent,
    Ephemeral,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RosterHarness {
    ClaudeCode,
    CodexCli,
    GeminiCli,
    Opencode,
    Hermes,
    PythonGraft,
}

impl RosterHarness {
    /// Return the stable CLI/JSON spelling for this roster harness.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::CodexCli => "codex-cli",
            Self::GeminiCli => "gemini-cli",
            Self::Opencode => "opencode",
            Self::Hermes => "hermes",
            Self::PythonGraft => "python-graft",
        }
    }
}

impl fmt::Display for RosterHarness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentType {
    GeneralPurpose,
    Plan,
    Lead,
    Qa,
    Worker,
    Unknown(String),
}

impl Default for AgentType {
    fn default() -> Self {
        Self::Unknown(String::new())
    }
}

impl From<String> for AgentType {
    fn from(value: String) -> Self {
        match value.as_str() {
            "general-purpose" => Self::GeneralPurpose,
            "plan" => Self::Plan,
            "lead" => Self::Lead,
            "qa" => Self::Qa,
            "worker" => Self::Worker,
            _ => Self::Unknown(value),
        }
    }
}

impl AgentType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::GeneralPurpose => "general-purpose",
            Self::Plan => "plan",
            Self::Lead => "lead",
            Self::Qa => "qa",
            Self::Worker => "worker",
            Self::Unknown(value) => value,
        }
    }
}

impl From<AgentType> for String {
    fn from(value: AgentType) -> Self {
        value.as_str().to_string()
    }
}

impl Serialize for AgentType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl fmt::Display for AgentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AgentType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self::from(String::deserialize(deserializer)?))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterMember {
    pub team_name: TeamName,
    pub agent_name: AgentName,
    pub member_kind: RosterMemberKind,
    pub harness: RosterHarness,
    #[serde(default)]
    pub agent_type: AgentType,
    #[serde(default)]
    pub model: ModelName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipient_pane_id: Option<PaneId>,
    #[serde(default)]
    pub metadata_json: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterSnapshot {
    pub team_name: TeamName,
    pub members: Vec<RosterMember>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refreshed_at: Option<IsoTimestamp>,
}

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
    /// Materializes a mailbox projection through the backend-owned async lane.
    ///
    /// Threaded-message validation needs this snapshot before it can submit
    /// its immutable successor. The Tokio daemon must therefore not fall back
    /// to [`MessageStore::list_messages`], which can synchronously open a
    /// database reader on a request worker.
    async fn list_messages_async(&self, _query: MessageQuery) -> Result<Vec<Message>, AtmError> {
        Err(AtmError::daemon_unavailable(
            "message store does not implement async mailbox projection admission",
        ))
    }

    /// Makes one immutable message durable, or returns the record that already
    /// owns its key, without blocking the Tokio request executor.
    async fn save_message_if_absent_async(
        &self,
        message: Message,
    ) -> Result<Option<Message>, AtmError> {
        self.save_message_if_absent(&message)
    }

    /// Atomically admits a mailbox record and its template decomposition on
    /// the backend-owned async writer lane.
    async fn admit_template_message_async(
        &self,
        _admission: crate::TemplateMessageAdmission,
    ) -> Result<Option<Message>, AtmError> {
        Err(AtmError::daemon_unavailable(
            "message store does not implement async template-message admission",
        ))
    }

    /// Resolves a pending acknowledgement source, persists its reply, and
    /// transitions that source as one async durable admission.
    async fn acknowledge_message_atomically_async(
        &self,
        source: AcknowledgementSource,
        builder: Arc<dyn AcknowledgementReplyBuilder>,
    ) -> Result<AcknowledgementCommit, AtmError> {
        self.acknowledge_message_atomically(&source, builder)
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

    async fn load_message(
        &self,
        scope: MailboxScope,
        key: MessageKey,
        deadline: ReadDeadline,
    ) -> Result<Option<Message>, ReadLaneError>;
}

pub trait RosterStore: sealed::Sealed + Send + Sync {
    fn load_roster(&self, team: &TeamName) -> Result<RosterSnapshot, AtmError>;
    fn save_roster(&self, roster: &RosterSnapshot) -> Result<(), AtmError>;
    fn list_teams(&self) -> Result<Vec<TeamName>, AtmError>;
}

/// Registration payload for one loopback graft receiver lease.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftReceiverRegistration {
    pub team: TeamName,
    pub agent: AgentName,
    pub endpoint: SocketAddr,
    pub capability: LocalCapability,
    pub owner_generation: OwnerGeneration,
}

/// Durable graft receiver endpoint and its liveness observations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftReceiverLease {
    pub endpoint: SocketAddr,
    pub capability: LocalCapability,
    pub owner_generation: OwnerGeneration,
    pub registered_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unreachable_since: Option<DateTime<Utc>>,
}

/// Errors returned by the durable graft receiver endpoint store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraftEndpointStoreError {
    /// Reserved for a future caller that cannot prove same-host exclusivity
    /// (ADR-056): same-host callers instead prove exclusivity through the
    /// receiver's flock and are never rejected by this variant today.
    AlreadyActive,
    /// The supplied owner generation does not own the stored lease.
    NotOwner,
    /// The backend could not complete the requested operation.
    ///
    /// Preserves the originating [`AtmError`]'s code and cause chain (RBP-F001)
    /// instead of flattening it into an opaque string, so callers can
    /// distinguish e.g. a caller-input constraint violation from a true
    /// backend outage rather than collapsing every failure into one generic
    /// presentation.
    Storage {
        code: crate::error_codes::AtmErrorCode,
        message: String,
        cause: Option<String>,
    },
}

impl GraftEndpointStoreError {
    /// Wraps a structured backend [`AtmError`] as a [`Self::Storage`]
    /// variant, preserving its code and cause instead of flattening it into
    /// an opaque string.
    #[must_use]
    pub fn storage(error: &AtmError) -> Self {
        Self::Storage {
            code: error.code(),
            message: error.message().to_string(),
            cause: error.cause().map(ToOwned::to_owned),
        }
    }
}

impl fmt::Display for GraftEndpointStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyActive => formatter.write_str("graft receiver lease is already active"),
            Self::NotOwner => {
                formatter.write_str("graft receiver lease is owned by another generation")
            }
            Self::Storage {
                code,
                message,
                cause,
            } => match cause {
                Some(cause) => write!(
                    formatter,
                    "graft receiver endpoint storage failed ({code}): {message}: {cause}"
                ),
                None => write!(
                    formatter,
                    "graft receiver endpoint storage failed ({code}): {message}"
                ),
            },
        }
    }
}

/// Durable registry for same-host graft receiver endpoints.
pub trait GraftReceiverEndpointStore: sealed::Sealed + Send + Sync {
    fn register(
        &self,
        registration: &GraftReceiverRegistration,
        now: DateTime<Utc>,
    ) -> Result<(), GraftEndpointStoreError>;

    fn refresh(
        &self,
        team: &TeamName,
        agent: &AgentName,
        owner_generation: &OwnerGeneration,
        now: DateTime<Utc>,
    ) -> Result<(), GraftEndpointStoreError>;

    fn unregister(
        &self,
        team: &TeamName,
        agent: &AgentName,
        owner_generation: &OwnerGeneration,
    ) -> Result<(), GraftEndpointStoreError>;

    fn lookup(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<GraftReceiverLease>, GraftEndpointStoreError>;

    fn mark_unreachable(
        &self,
        team: &TeamName,
        agent: &AgentName,
        owner_generation: &OwnerGeneration,
        now: DateTime<Utc>,
    ) -> Result<(), GraftEndpointStoreError>;
}

/// A non-empty, opaque certificate fingerprint. It cannot be confused with a
/// private-key reference at storage and transport boundaries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(try_from = "String", into = "String")]
pub struct CertificateFingerprint(String);

impl CertificateFingerprint {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CertificateFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for CertificateFingerprint {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        require_non_blank(value.to_owned(), "certificate fingerprint").map(Self)
    }
}

impl TryFrom<String> for CertificateFingerprint {
    type Error = AtmError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<CertificateFingerprint> for String {
    fn from(value: CertificateFingerprint) -> Self {
        value.0
    }
}

/// A non-empty opaque reference to locally held private-key material.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(try_from = "String", into = "String")]
pub struct PrivateKeyRef(String);

impl PrivateKeyRef {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PrivateKeyRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for PrivateKeyRef {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        require_non_blank(value.to_owned(), "certificate key reference").map(Self)
    }
}

impl TryFrom<String> for PrivateKeyRef {
    type Error = AtmError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<PrivateKeyRef> for String {
    fn from(value: PrivateKeyRef) -> Self {
        value.0
    }
}

/// One durable HTTPS listener configuration. This is control-plane state only;
/// it contains no delivery, retry, or mailbox data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HttpsInterface {
    pub bind_addr: std::net::SocketAddr,
    pub advertise_host: HostName,
    pub enabled: bool,
}

/// Public identity of the local TLS certificate. The private key is referenced
/// indirectly so doctor and callers cannot read secret material from storage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalCertificate {
    pub fingerprint: CertificateFingerprint,
    pub private_key_ref: PrivateKeyRef,
}

/// One exact, pinned peer allowed to use the cross-host HTTPS listener.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustedPeer {
    pub host: HostName,
    pub fingerprint: CertificateFingerprint,
    pub enabled: bool,
    pub https_port: NonZeroU16,
}

/// Backend-neutral durable cross-host configuration.
///
/// This boundary deliberately excludes transport state, retries, receipts,
/// and mailbox state. HTTPS adapters consume this contract but never SQLite
/// implementation types.
pub trait PeerConfigStore: sealed::Sealed + Send + Sync {
    fn list_interfaces(&self) -> Result<Vec<HttpsInterface>, AtmError>;
    fn save_interface(&self, interface: &HttpsInterface) -> Result<(), AtmError>;
    fn remove_interface(&self, bind_addr: std::net::SocketAddr) -> Result<bool, AtmError>;
    fn local_certificate(&self) -> Result<Option<LocalCertificate>, AtmError>;
    fn save_local_certificate(&self, certificate: &LocalCertificate) -> Result<(), AtmError>;
    fn list_trusted_peers(&self) -> Result<Vec<TrustedPeer>, AtmError>;
    fn trusted_peer(&self, host: &HostName) -> Result<Option<TrustedPeer>, AtmError>;
    fn save_trusted_peer(&self, peer: &TrustedPeer) -> Result<(), AtmError>;
    fn remove_trusted_peer(&self, host: &HostName) -> Result<bool, AtmError>;
}

pub trait StorageNotifier: sealed::Sealed + Send + Sync {
    fn message_received(&self, event: &MessageReceivedEvent) -> Result<(), AtmError>;
    fn roster_changed(&self, event: &RosterChangedEvent) -> Result<(), AtmError>;
}

pub trait NudgeTemplateOverrideStore: sealed::Sealed + Send + Sync {
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

    fn clear_template_override(
        &self,
        team: &TeamName,
        kind: BuiltInNudgeTemplateKind,
    ) -> Result<bool, AtmError>;
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

    /// Clears the marker for one message on the read path.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] if the underlying storage operation fails.
    fn clear_pending_on_read(&self, member: &MemberKey, msg: &AtmMessageId)
    -> Result<(), AtmError>;

    /// Clears the marker for exactly one just-handed-off message.
    ///
    /// Unconditional and idempotent.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] if the underlying storage operation fails.
    fn clear_pending_on_handoff(
        &self,
        member: &MemberKey,
        msg: &AtmMessageId,
    ) -> Result<(), AtmError>;

    /// Enumerates members holding at least one eligible pending marker.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] if the underlying storage operation fails.
    fn list_pending_members(&self) -> Result<Vec<MemberKey>, AtmError>;
}

#[cfg(test)]
mod tests {
    use super::{
        AckRequirementState, AtmMessageId, BuiltInNudgeTemplateKind, CertificateFingerprint,
        GraftReceiverEndpointStore, GraftReceiverRegistration, Message, MessageKey, MessageQuery,
        MessageReceivedEvent, MessageStore, NudgeClaim, NudgeTemplateOverrideStore,
        PendingNudgeStore, PrivateKeyRef, RosterChangedEvent, RosterHarness, RosterMember,
        RosterMemberKind, RosterSnapshot, RosterStore, StorageNotifier,
        TeamNudgeTemplateOverrideMode, TeamNudgeTemplateOverrideRow, derive_ack_requirement,
        sealed,
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

        fn clear_template_override(
            &self,
            _team: &TeamName,
            _kind: BuiltInNudgeTemplateKind,
        ) -> Result<bool, AtmError> {
            Ok(true)
        }
    }

    #[derive(Default)]
    struct DummyPendingNudgeStore;

    impl sealed::Sealed for DummyPendingNudgeStore {}

    impl PendingNudgeStore for DummyPendingNudgeStore {
        fn mark_pending(
            &self,
            _member: &MemberKey,
            _msg: &AtmMessageId,
            _at: IsoTimestamp,
        ) -> Result<bool, AtmError> {
            Ok(true)
        }

        fn claim_next_pending(&self, _member: &MemberKey) -> Result<Option<NudgeClaim>, AtmError> {
            Ok(None)
        }

        fn requeue_pending(
            &self,
            _member: &MemberKey,
            _claim: &NudgeClaim,
        ) -> Result<(), AtmError> {
            Ok(())
        }

        fn release_pending(
            &self,
            _member: &MemberKey,
            _claim: &NudgeClaim,
        ) -> Result<(), AtmError> {
            Ok(())
        }

        fn clear_pending_on_read(
            &self,
            _member: &MemberKey,
            _msg: &AtmMessageId,
        ) -> Result<(), AtmError> {
            Ok(())
        }

        fn clear_pending_on_handoff(
            &self,
            _member: &MemberKey,
            _msg: &AtmMessageId,
        ) -> Result<(), AtmError> {
            Ok(())
        }

        fn list_pending_members(&self) -> Result<Vec<MemberKey>, AtmError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn storage_traits_are_object_safe() {
        let store = DummyStore;
        let message_store: &dyn MessageStore = &store;
        let roster_store: &dyn RosterStore = &store;
        let notifier: &dyn StorageNotifier = &store;
        let pending_nudge_store: &dyn PendingNudgeStore = &DummyPendingNudgeStore;
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
            .clear_pending_on_read(&member, &msg)
            .expect("clear pending on read");
        pending_nudge_store
            .clear_pending_on_handoff(&member, &msg)
            .expect("clear pending on handoff");
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
