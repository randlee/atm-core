use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;
use std::num::NonZeroU16;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;

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
    /// Atomically admits a full group of new immutable records. Unlike the
    /// upsert-oriented batch writer, any existing member rejects the complete
    /// operation so a concurrent competing admission cannot be swallowed.
    fn admit_messages_atomically(&self, _messages: &[Message]) -> Result<(), AtmError> {
        Err(AtmError::daemon_unavailable(
            "message store does not implement atomic immutable batch admission",
        ))
    }
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
    /// Atomically retires durable peer-delivery markers after the configured
    /// peer accepts one exact ordered outbound batch. The immutable messages
    /// remain unchanged; no submitted subset may be retired on mismatch.
    fn confirm_peer_delivery_batch(
        &self,
        _canonical_host: &HostName,
        _message_ids: &[AtmMessageId],
    ) -> Result<(), AtmError> {
        Err(AtmError::daemon_unavailable(
            "message store does not implement atomic peer delivery confirmation",
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

/// Validate the stable hostname used as a configured peer authority.
///
/// Literal IPs are accepted only as explicit `PeerAliasKey::Ip` inputs. They
/// cannot become canonical storage or connection identities because an address
/// can change independently of the configured peer hostname.
pub fn validate_canonical_peer_host(host: &HostName) -> Result<(), AtmError> {
    if host.as_str().parse::<IpAddr>().is_ok() {
        return Err(AtmError::peer_config_validation(format!(
            "canonical peer host `{host}` must not be an IP literal"
        )));
    }
    Ok(())
}

/// One normalized key accepted for a trusted-peer endpoint.
///
/// IP literals are intentionally a separate variant: callers parse an IP
/// before attempting a hostname so a literal cannot become a second spelling
/// of a host alias.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PeerAliasKey {
    Host(HostName),
    Ip(IpAddr),
}

impl PeerAliasKey {
    pub fn parse(value: &str) -> Result<Self, AtmError> {
        if let Ok(ip) = value.parse::<IpAddr>() {
            return Ok(Self::Ip(ip));
        }
        value.parse::<HostName>().map(Self::Host).map_err(|error| {
            AtmError::peer_config_validation(format!(
                "peer alias `{value}` is neither a valid IP literal nor hostname: {error}"
            ))
        })
    }

    #[must_use]
    pub fn alias_kind(&self) -> &'static str {
        match self {
            Self::Host(_) => "host",
            Self::Ip(_) => "ip",
        }
    }

    #[must_use]
    pub fn alias_value(&self) -> String {
        match self {
            Self::Host(host) => host.as_str().to_owned(),
            Self::Ip(ip) => ip.to_string(),
        }
    }
}

impl FromStr for PeerAliasKey {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl fmt::Display for PeerAliasKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host(host) => f.write_str(host.as_str()),
            Self::Ip(ip) => ip.fmt(f),
        }
    }
}

/// Immutable canonical destination selected from a peer alias.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct PeerEndpoint {
    pub canonical_host: HostName,
    pub port: NonZeroU16,
}

/// Durable opt-in for the peer resend layer.
///
/// Disabling this setting leaves peer delivery on the direct one-attempt path;
/// immutable `peerOutbound` records remain the sole durable backlog.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerResendCacheSetting {
    pub enabled: bool,
}

/// One reloadable, immutable peer-alias snapshot used by local admission.
#[derive(Debug, Clone, Default)]
pub struct PeerDirectory {
    by_alias: HashMap<PeerAliasKey, PeerEndpoint>,
}

impl PeerDirectory {
    pub fn from_configuration(
        peers: impl IntoIterator<Item = TrustedPeer>,
        aliases: impl IntoIterator<Item = (PeerAliasKey, HostName)>,
    ) -> Result<Self, AtmError> {
        let mut endpoints = HashMap::new();
        let mut by_alias = HashMap::new();
        for peer in peers {
            validate_canonical_peer_host(&peer.host)?;
            if !peer.enabled {
                continue;
            }
            let endpoint = PeerEndpoint {
                canonical_host: peer.host.clone(),
                port: peer.https_port,
            };
            endpoints.insert(peer.host.clone(), endpoint.clone());
            by_alias.insert(PeerAliasKey::Host(peer.host), endpoint);
        }

        for (alias, canonical_host) in aliases {
            if matches!(&alias, PeerAliasKey::Host(host) if host.as_str().parse::<IpAddr>().is_ok())
            {
                return Err(AtmError::peer_config_validation(format!(
                    "host peer alias `{alias}` must not be an IP literal"
                )));
            }
            validate_canonical_peer_host(&canonical_host)?;
            let endpoint = endpoints.get(&canonical_host).cloned().ok_or_else(|| {
                AtmError::peer_config_validation(format!(
                    "peer alias `{alias}` references unknown or disabled canonical peer `{canonical_host}`"
                ))
            })?;
            if by_alias.insert(alias.clone(), endpoint).is_some() {
                return Err(AtmError::peer_config_validation(format!(
                    "peer alias `{alias}` duplicates an existing normalized peer alias"
                )));
            }
        }
        Ok(Self { by_alias })
    }

    pub fn normalize(&self, alias: &PeerAliasKey) -> Result<PeerEndpoint, AtmError> {
        self.by_alias.get(alias).cloned().ok_or_else(|| {
            AtmError::peer_config_validation(format!(
                "no enabled trusted peer is configured for alias `{alias}`"
            ))
        })
    }

    #[must_use]
    pub fn endpoint_for_canonical_host(&self, host: &HostName) -> Option<PeerEndpoint> {
        self.by_alias
            .get(&PeerAliasKey::Host(host.clone()))
            .cloned()
    }
}

/// Backend-neutral durable configured-peer HTTP configuration.
///
/// This boundary deliberately excludes transport state, retries, receipts,
/// and mailbox state. Peer HTTP adapters consume this contract but never SQLite
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
    fn peer_directory(&self) -> Result<PeerDirectory, AtmError>;
    fn list_peer_aliases(&self) -> Result<Vec<(PeerAliasKey, HostName)>, AtmError>;
    fn save_peer_alias(
        &self,
        alias: PeerAliasKey,
        canonical_host: HostName,
    ) -> Result<(), AtmError>;
    fn remove_peer_alias(&self, alias: &PeerAliasKey) -> Result<bool, AtmError>;
    fn peer_resend_cache_setting(&self) -> Result<PeerResendCacheSetting, AtmError>;
    fn save_peer_resend_cache_setting(
        &self,
        setting: PeerResendCacheSetting,
    ) -> Result<(), AtmError>;
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
    fn pending_peer_hosts(&self, budget: std::time::Duration) -> Result<Vec<HostName>, AtmError>;
    fn page_for_peer(
        &self,
        peer: &HostName,
        after: Option<(IsoTimestamp, AtmMessageId)>,
        limit: NonZeroU16,
        budget: std::time::Duration,
    ) -> Result<Vec<StoredPeerWrite>, AtmError>;
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
        AckRequirementState, BuiltInNudgeTemplateKind, CertificateFingerprint, Message, MessageKey,
        MessageQuery, MessageReceivedEvent, MessageStore, NudgeTemplateOverrideStore, PeerAliasKey,
        PeerDirectory, PrivateKeyRef, RosterChangedEvent, RosterHarness, RosterMember,
        RosterMemberKind, RosterSnapshot, RosterStore, StorageNotifier,
        TeamNudgeTemplateOverrideMode, TeamNudgeTemplateOverrideRow, TrustedPeer,
        derive_ack_requirement, sealed,
    };
    use crate::ROLE_WORKER;
    use crate::error::AtmError;
    use crate::schema::MessageEnvelope;
    use crate::types::{AgentName, IsoTimestamp, ModelName, TeamName};
    use chrono::Utc;
    use serde_json::Map;

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

    fn enabled_peer(host: &str, port: u16) -> TrustedPeer {
        TrustedPeer {
            host: host.parse().expect("host"),
            fingerprint: "sha256:test".parse().expect("fingerprint"),
            enabled: true,
            https_port: std::num::NonZeroU16::new(port).expect("non-zero port"),
        }
    }

    #[test]
    fn peer_directory_normalizes_host_and_ip_aliases_in_constant_time_snapshot() {
        let directory = PeerDirectory::from_configuration(
            [enabled_peer("rand-m5.local", 43101)],
            [
                (
                    "m5".parse::<PeerAliasKey>().expect("host alias"),
                    "rand-m5.local".parse().expect("canonical host"),
                ),
                (
                    "192.168.128.82".parse::<PeerAliasKey>().expect("IP alias"),
                    "rand-m5.local".parse().expect("canonical host"),
                ),
            ],
        )
        .expect("directory");

        for alias in ["rand-m5.local", "m5", "192.168.128.82"] {
            let endpoint = directory
                .normalize(&alias.parse().expect("alias"))
                .expect("configured alias");
            assert_eq!(endpoint.canonical_host.as_str(), "rand-m5.local");
            assert_eq!(endpoint.port.get(), 43101);
        }
        assert!(
            directory
                .normalize(&"unknown.local".parse().expect("alias"))
                .is_err()
        );
    }

    #[test]
    fn peer_directory_rejects_aliases_for_unknown_or_disabled_peers() {
        let disabled = TrustedPeer {
            enabled: false,
            ..enabled_peer("offline.local", 43101)
        };
        let error = PeerDirectory::from_configuration(
            [disabled],
            [(
                "192.168.128.83".parse::<PeerAliasKey>().expect("IP alias"),
                "offline.local".parse().expect("canonical host"),
            )],
        )
        .expect_err("disabled canonical peer must fail closed");
        assert!(error.message().contains("unknown or disabled"));
    }

    #[test]
    fn peer_directory_rejects_an_ip_literal_canonical_host() {
        let error = PeerDirectory::from_configuration([enabled_peer("127.0.0.1", 43101)], [])
            .expect_err("canonical peer hosts are stable DNS names, not addresses");
        assert!(error.message().contains("must not be an IP literal"));
    }
}
