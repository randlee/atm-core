use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fmt;
use std::num::NonZeroU16;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use crate::error::AtmError;
use crate::schema::{AtmMessageId, InboxMessage, MessageEnvelope};
use crate::types::{AgentName, HostName, IsoTimestamp, ModelName, PaneId, TaskId, TeamName};

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
    fn delete_message(&self, key: &MessageKey) -> Result<(), AtmError>;
}

pub trait RosterStore: sealed::Sealed + Send + Sync {
    fn load_roster(&self, team: &TeamName) -> Result<RosterSnapshot, AtmError>;
    fn save_roster(&self, roster: &RosterSnapshot) -> Result<(), AtmError>;
    fn list_teams(&self) -> Result<Vec<TeamName>, AtmError>;
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

/// Per-peer, operator-controlled bound for one reconciliation scan.
///
/// A zero age disables reconciliation.  This is configuration only: it does
/// not represent a cursor, retry budget, receipt, or delivery state.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerSyncPolicy {
    #[serde(with = "duration_seconds")]
    pub max_message_age: Duration,
    pub max_batch_messages: NonZeroU16,
}

/// Reconciliation is deliberately bounded; a policy may never widen one pass
/// beyond this value.
pub const MAX_PEER_SYNC_BATCH_MESSAGES: u16 = 100;
/// Recovery selects recent immutable writes only; wider windows risk timestamp
/// arithmetic outside the representable range.
pub const MAX_PEER_SYNC_MESSAGE_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

impl PeerSyncPolicy {
    pub fn validate(self) -> Result<Self, AtmError> {
        if self.max_batch_messages.get() > MAX_PEER_SYNC_BATCH_MESSAGES {
            return Err(AtmError::peer_config_validation(
                "peer sync max_batch_messages exceeds the hard limit of 100",
            ));
        }
        if self.max_message_age > MAX_PEER_SYNC_MESSAGE_AGE {
            return Err(AtmError::peer_config_validation(
                "peer sync max_message_age exceeds the hard limit of 30 days",
            ));
        }
        Ok(self)
    }
}

impl Default for PeerSyncPolicy {
    fn default() -> Self {
        Self {
            max_message_age: Duration::ZERO,
            max_batch_messages: NonZeroU16::new(MAX_PEER_SYNC_BATCH_MESSAGES)
                .expect("hard limit is non-zero"),
        }
    }
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
    fn peer_sync_policy(&self, _host: &HostName) -> Result<PeerSyncPolicy, AtmError> {
        Ok(PeerSyncPolicy::default())
    }
    fn save_peer_sync_policy(
        &self,
        _host: &HostName,
        _policy: PeerSyncPolicy,
    ) -> Result<(), AtmError> {
        Err(AtmError::validation(
            "selected storage backend does not support durable peer sync policy",
        ))
    }
}

/// Immutable canonical peer write selected for a bounded reconciliation pass.
/// The JSON is the origin writer's serialized request, retained with the
/// canonical message rather than in an outbox or delivery-state table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredPeerWrite {
    /// Ordering key retained with the immutable canonical write. This is a
    /// transient scan cursor only; it is never delivery state.
    pub created_at: IsoTimestamp,
    pub message_id: AtmMessageId,
    pub request_json: String,
}

/// Read-only selection of local, immutable peer-directed messages.
pub trait OutboundMessageQuery: sealed::Sealed + Send + Sync {
    fn page_for_peer(
        &self,
        peer: &HostName,
        not_before: IsoTimestamp,
        after: Option<(IsoTimestamp, AtmMessageId)>,
        limit: NonZeroU16,
        budget: std::time::Duration,
    ) -> Result<Vec<StoredPeerWrite>, AtmError>;

    /// Load one immutable peer-directed write by its canonical identity.
    ///
    /// The peer-drain coordinator uses this only after its bounded
    /// reconciliation page does not contain a newly persisted job. It is a
    /// direct eligibility lookup, not a cursor or delivery-state mutation.
    fn find_for_peer(
        &self,
        peer: &HostName,
        message_id: AtmMessageId,
        budget: std::time::Duration,
    ) -> Result<Option<StoredPeerWrite>, AtmError>;
}

mod duration_seconds {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(value.as_secs())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Duration::from_secs(u64::deserialize(deserializer)?))
    }
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

#[cfg(test)]
mod tests {
    use super::{
        AckRequirementState, BuiltInNudgeTemplateKind, CertificateFingerprint,
        MAX_PEER_SYNC_MESSAGE_AGE, Message, MessageKey, MessageQuery, MessageReceivedEvent,
        MessageStore, NudgeTemplateOverrideStore, PeerSyncPolicy, PrivateKeyRef,
        RosterChangedEvent, RosterHarness, RosterMember, RosterMemberKind, RosterSnapshot,
        RosterStore, StorageNotifier, TeamNudgeTemplateOverrideMode, TeamNudgeTemplateOverrideRow,
        derive_ack_requirement, sealed,
    };
    use crate::ROLE_WORKER;
    use crate::error::AtmError;
    use crate::schema::MessageEnvelope;
    use crate::types::{AgentName, IsoTimestamp, ModelName, TeamName};
    use chrono::Utc;
    use serde_json::Map;
    use std::num::NonZeroU16;
    use std::time::Duration;

    #[derive(Default)]
    struct DummyStore;

    #[derive(Default)]
    struct DummyNudgeTemplateOverrideStore;

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

    #[test]
    fn storage_traits_are_object_safe() {
        let store = DummyStore;
        let message_store: &dyn MessageStore = &store;
        let roster_store: &dyn RosterStore = &store;
        let notifier: &dyn StorageNotifier = &store;
        let override_store: &dyn NudgeTemplateOverrideStore = &DummyNudgeTemplateOverrideStore;

        let team: TeamName = "test-team".parse().expect("team");
        let agent: AgentName = ROLE_WORKER.parse().expect("agent");
        let key = MessageKey::new("atm:test-1").expect("key");

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

    #[test]
    fn peer_sync_policy_rejects_windows_beyond_the_bounded_recovery_limit() {
        let policy = PeerSyncPolicy {
            max_message_age: MAX_PEER_SYNC_MESSAGE_AGE + Duration::from_secs(1),
            max_batch_messages: NonZeroU16::new(1).expect("non-zero"),
        };
        assert!(policy.validate().is_err());
    }
}
