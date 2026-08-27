use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

use base64::Engine as _;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::AtmError;
use crate::template_workflow::TemplateVariableName;
use crate::validation::{validate_agent_at_team, validate_path_segment};

pub const LOCAL_CAPABILITY_BYTES: usize = 32;

/// Per-bind secret used to authenticate same-host local HTTP and graft calls.
///
/// The storage contract owns this value because durable graft registrations
/// must be implementable without a dependency on `atm-core`.
#[derive(Clone, PartialEq, Eq)]
pub struct LocalCapability([u8; LOCAL_CAPABILITY_BYTES]);

impl fmt::Debug for LocalCapability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LocalCapability([REDACTED])")
    }
}

impl LocalCapability {
    pub fn generate() -> Result<Self, AtmError> {
        let mut bytes = [0; LOCAL_CAPABILITY_BYTES];
        getrandom::fill(&mut bytes).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to generate local HTTP capability: {source}"
            ))
        })?;
        Ok(Self(bytes))
    }

    pub fn to_base64url(&self) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(self.0)
    }

    pub fn parse_base64url(value: &str) -> Result<Self, AtmError> {
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|source| {
                AtmError::local_http_capability_invalid("local HTTP capability is not base64url")
                    .with_cause(source)
            })?;
        let bytes: [u8; LOCAL_CAPABILITY_BYTES] = bytes.try_into().map_err(|_| {
            AtmError::local_http_capability_invalid(
                "local HTTP capability must decode to exactly 32 bytes",
            )
        })?;
        Ok(Self(bytes))
    }

    pub fn matches_header(&self, value: &str) -> bool {
        match Self::parse_base64url(value) {
            Ok(candidate) => {
                let mut difference = 0_u8;
                for (left, right) in self.0.iter().zip(candidate.0.iter()) {
                    difference |= left ^ right;
                }
                difference == 0
            }
            Err(_) => false,
        }
    }
}

impl Serialize for LocalCapability {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_base64url())
    }
}

impl<'de> Deserialize<'de> for LocalCapability {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse_base64url(&value).map_err(serde::de::Error::custom)
    }
}

/// Validated ULID identifying the owner generation of a local lease.
#[derive(Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct OwnerGeneration(String);

impl OwnerGeneration {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        value.parse::<ulid::Ulid>().map_err(|_| {
            AtmError::validation("graft receiver owner generation must be a valid ULID")
        })?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for OwnerGeneration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("OwnerGeneration")
            .field(&self.0)
            .finish()
    }
}

impl fmt::Display for OwnerGeneration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for OwnerGeneration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Lowercase SHA-256 identity for an immutable raw template file.
///
/// This value is the storage-facing representation of the upstream
/// `sc-composer` raw-byte template digest. It deliberately does not perform
/// hashing itself: callers must obtain the digest through the approved
/// template-composer adapter.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TemplateSha(String);

impl TemplateSha {
    /// Construct a validated lowercase hexadecimal SHA-256 identity.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when `value` is not exactly 64 lowercase
    /// hexadecimal characters.
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        let is_lower_hex = value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'));
        if !is_lower_hex {
            return Err(AtmError::validation(
                "template SHA must be exactly 64 lowercase hexadecimal characters",
            ));
        }
        Ok(Self(value))
    }

    /// Borrow the lowercase hexadecimal digest.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume this identity into its lowercase hexadecimal digest.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl FromStr for TemplateSha {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl AsRef<str> for TemplateSha {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for TemplateSha {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Parsed, storage-ready frontmatter for a template file.
///
/// The values are produced by the approved template-composer adapter and will
/// be persisted as schema JSON by the Phase AN catalog capability.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TemplateFrontmatter {
    /// Required variable names declared by the template.
    pub required_variables: Vec<TemplateVariableName>,
    /// Default values declared by the template.
    pub defaults: serde_json::Map<String, serde_json::Value>,
    /// Descriptive frontmatter metadata, including the template type key.
    pub metadata: serde_json::Map<String, serde_json::Value>,
    /// Canonical literal tags parsed from `metadata.tags` at catalog admission.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub template_tags: Vec<crate::template_workflow::TemplateTag>,
    /// Canonical workflow declaration parsed from `metadata.workflow`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<crate::template_workflow::TemplateWorkflowDeclaration>,
}

impl TemplateFrontmatter {
    /// Captures supported metadata as canonical immutable catalog data.
    ///
    /// The raw `metadata` map remains available for general template authoring
    /// data, while workflow-facing consumers use these validated fields.
    pub fn with_normalized_workflow_metadata(mut self) -> Result<Self, AtmError> {
        let declaration =
            crate::template_workflow::TemplateTagDeclaration::from_frontmatter(&self)?;
        self.template_tags = declaration.tags;
        self.workflow = declaration.workflow;
        Ok(self)
    }

    /// Ensures pre-parsed metadata still matches the immutable raw metadata.
    pub fn validate_workflow_metadata(&self) -> Result<(), AtmError> {
        let declaration = crate::template_workflow::TemplateTagDeclaration::from_frontmatter(self)?;
        if self.template_tags != declaration.tags || self.workflow != declaration.workflow {
            return Err(AtmError::new(
                crate::AtmErrorCode::TemplateWorkflowInvalid,
                "template workflow metadata must be normalized before catalog admission",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IsoTimestamp(DateTime<Utc>);

impl IsoTimestamp {
    pub fn now() -> Self {
        Self(Utc::now())
    }

    pub fn from_datetime(datetime: DateTime<Utc>) -> Self {
        Self(datetime)
    }

    pub fn into_inner(self) -> DateTime<Utc> {
        self.0
    }
}

impl FromStr for IsoTimestamp {
    type Err = chrono::ParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse::<DateTime<Utc>>().map(Self::from_datetime)
    }
}

impl From<DateTime<Utc>> for IsoTimestamp {
    fn from(datetime: DateTime<Utc>) -> Self {
        Self(datetime)
    }
}

impl fmt::Display for IsoTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.to_rfc3339())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct AgentName(String);

impl AgentName {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }

    #[doc(hidden)]
    pub fn from_validated(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl FromStr for AgentName {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        validate_path_segment(trimmed, "agent")?;
        Ok(Self(trimmed.to_string()))
    }
}

impl<'de> Deserialize<'de> for AgentName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl From<AgentName> for String {
    fn from(value: AgentName) -> Self {
        value.0
    }
}

impl AsRef<str> for AgentName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for AgentName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for AgentName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq<&str> for AgentName {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

/// Validated, durable identifier for one independently-addressable agent
/// context. It deliberately uses the same safe segment policy as agent and
/// team names; `:` remains an address delimiter owned by `AgentIdentity`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ChatId(String);

impl ChatId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl FromStr for ChatId {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        validate_path_segment(trimmed, "chat id")?;
        Ok(Self(trimmed.to_string()))
    }
}

impl<'de> Deserialize<'de> for ChatId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for ChatId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Validated DNS-name or IPv4-style host component in an ATM address.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct HostName(String);

impl HostName {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Reports whether this host is suitable as a durable peer identity.
    ///
    /// `HostName` remains permissive because ATM addresses and transport
    /// provenance must continue to represent historical IP and mDNS values.
    /// Peer configuration uses this explicit semantic check before accepting a
    /// host that will later be used as the source qualifier in a nudge.
    pub fn is_durable_hostname(&self) -> bool {
        let ipv4_shaped = self.0.split('.').count() == 4
            && self
                .0
                .split('.')
                .all(|label| !label.is_empty() && label.bytes().all(|byte| byte.is_ascii_digit()));
        // A stable local-network mDNS name (for example, `rand-m5.local`) is
        // a durable authority even though the address it resolves to may
        // change with Wi-Fi, Ethernet, VPN, or DHCP. Literal IPv4 values are
        // routing observations, not durable peer identities.
        !ipv4_shaped
    }
}

impl FromStr for HostName {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(AtmError::address_parse("host must not be empty"));
        }
        for label in trimmed.split('.') {
            validate_path_segment(label, "host label")?;
        }
        Ok(Self(trimmed.to_string()))
    }
}

impl<'de> Deserialize<'de> for HostName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for HostName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The sole parser and renderer for `agent[:chat-id]`. Transport and adapter
/// layers consume this value rather than splitting `:` themselves.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub agent: AgentName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<ChatId>,
}

impl AgentIdentity {
    pub fn new(agent: AgentName, chat_id: Option<ChatId>) -> Self {
        Self { agent, chat_id }
    }
}

impl FromStr for AgentIdentity {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(AtmError::address_parse("agent identity must not be empty"));
        }
        let (agent, chat_id) = match trimmed.split_once(':') {
            Some((agent, chat_id)) => {
                if chat_id.contains(':') {
                    return Err(AtmError::address_parse(
                        "agent identity may contain only one ':'",
                    ));
                }
                (agent.parse()?, Some(chat_id.parse()?))
            }
            None => (trimmed.parse()?, None),
        };
        Ok(Self { agent, chat_id })
    }
}

impl fmt::Display for AgentIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.agent.as_str())?;
        if let Some(chat_id) = &self.chat_id {
            write!(f, ":{chat_id}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct AgentId(String);

impl AgentId {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        let trimmed = value.trim();
        validate_agent_at_team(trimmed, "agent id")?;
        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl FromStr for AgentId {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for AgentId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl From<AgentId> for String {
    fn from(value: AgentId) -> Self {
        value.0
    }
}

impl AsRef<str> for AgentId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for AgentId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct TeamName(String);

impl TeamName {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }

    #[doc(hidden)]
    pub fn from_validated(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl FromStr for TeamName {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        validate_path_segment(trimmed, "team")?;
        Ok(Self(trimmed.to_string()))
    }
}

impl<'de> Deserialize<'de> for TeamName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl From<TeamName> for String {
    fn from(value: TeamName) -> Self {
        value.0
    }
}

impl AsRef<str> for TeamName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for TeamName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for TeamName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AgentId, AgentIdentity, AgentName, ChatId, HostName, IsoTimestamp, LocalCapability,
        OwnerGeneration, TemplateSha,
    };
    use std::str::FromStr;

    #[test]
    fn local_capability_debug_never_prints_the_token() {
        let capability = LocalCapability::generate().expect("capability");
        let rendered = format!("{capability:?}");
        assert_eq!(rendered, "LocalCapability([REDACTED])");
        assert!(
            !rendered.contains(&capability.to_base64url()),
            "debug output must never leak the encoded capability token"
        );
    }

    #[test]
    fn owner_generation_accepts_valid_ulid_and_rejects_garbage() {
        let generation =
            OwnerGeneration::new("01J00000000000000000000001").expect("valid ULID generation");
        assert_eq!(generation.as_str(), "01J00000000000000000000001");
        assert_eq!(generation.to_string(), "01J00000000000000000000001");

        for invalid in [
            "",
            "generation-1",
            "not-a-ulid",
            "01J0000000000000000000000",
        ] {
            assert!(
                OwnerGeneration::new(invalid).is_err(),
                "{invalid} must not parse as a valid owner generation"
            );
        }
    }

    #[test]
    fn agent_identity_owns_chat_id_parsing_and_rendering() {
        let base = AgentIdentity::from_str("omega-prime").expect("base identity");
        assert_eq!(base.agent, AgentName::from_validated("omega-prime"));
        assert_eq!(base.chat_id, None);

        let qualified = AgentIdentity::from_str("omega-prime:1234").expect("chat identity");
        assert_eq!(
            qualified.chat_id,
            Some(ChatId::from_str("1234").expect("chat id"))
        );
        assert_eq!(qualified.to_string(), "omega-prime:1234");

        for invalid in [
            "",
            ":1234",
            "omega-prime:",
            "omega-prime:one:two",
            "omega-prime:bad/name",
        ] {
            assert!(
                AgentIdentity::from_str(invalid).is_err(),
                "{invalid} must fail"
            );
        }
    }

    #[test]
    fn agent_id_new_matches_agent_name_validation_for_valid_value() {
        let agent_id = AgentId::new("worker-1").expect("agent id");
        let agent_name: AgentName = "worker-1".parse().expect("agent name");

        assert_eq!(agent_id.as_str(), agent_name.as_str());
    }

    #[test]
    fn agent_id_new_rejects_invalid_path_segments() {
        for invalid in [
            "",
            ".hidden",
            "two..dots",
            "bad/name",
            "bad name",
            "role@bad.team",
        ] {
            assert!(
                AgentId::new(invalid).is_err(),
                "expected `{invalid}` to fail"
            );
        }
    }

    #[test]
    fn agent_id_deserialize_applies_validation() {
        let parsed: AgentId = serde_json::from_str("\"worker-2\"").expect("deserialize");
        assert_eq!(parsed.as_str(), "worker-2");
        let compound: AgentId =
            serde_json::from_str("\"worker-2@test-team\"").expect("compound deserialize");
        assert_eq!(compound.as_str(), "worker-2@test-team");

        let error = serde_json::from_str::<AgentId>("\"bad/name\"").expect_err("invalid id");
        assert!(
            error
                .to_string()
                .contains("must use only ASCII letters, digits, '-' or '_'")
        );
    }

    #[test]
    fn iso_timestamp_from_str_round_trips_rfc3339_input() {
        let timestamp: IsoTimestamp = "2026-07-11T01:20:17Z".parse().expect("timestamp");

        assert_eq!(timestamp.to_string(), "2026-07-11T01:20:17+00:00");
    }

    #[test]
    fn host_name_distinguishes_durable_peer_names_from_attachment_names() {
        for attachment_name in ["192.168.128.29", "192.168.128.029", "999.1.1.1"] {
            let host: HostName = attachment_name.parse().expect("generic host syntax");
            assert!(
                !host.is_durable_hostname(),
                "{attachment_name} must not be treated as a durable peer identity"
            );
        }

        for durable_name in ["peer.example", "peer.localhost", "peer.local", "PEER.LOCAL"] {
            let host: HostName = durable_name.parse().expect("host");
            assert!(
                host.is_durable_hostname(),
                "{durable_name} should be accepted as a durable peer identity"
            );
        }
    }

    #[test]
    fn template_sha_requires_exact_lowercase_sha256_hex() {
        let sha =
            TemplateSha::new("d3d06622826ac021d6e65098cc412034df9cdddd7248b46283029e43ca636b72")
                .expect("valid SHA-256 fixture");
        assert_eq!(sha.as_str(), sha.to_string());

        for invalid in [
            "",
            "D3D06622826AC021D6E65098CC412034DF9CDDDD7248B46283029E43CA636B72",
            "d3d06622826ac021d6e65098cc412034df9cdddd7248b46283029e43ca636b7",
            "d3d06622826ac021d6e65098cc412034df9cdddd7248b46283029e43ca636b7zz",
        ] {
            assert!(TemplateSha::new(invalid).is_err(), "{invalid} must fail");
        }
    }
}

impl PartialEq<&str> for TeamName {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct TaskId(String);

impl TaskId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl FromStr for TaskId {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(AtmError::validation("task id must not be blank"));
        }
        Ok(Self(trimmed.to_string()))
    }
}

impl<'de> Deserialize<'de> for TaskId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl From<TaskId> for String {
    fn from(value: TaskId) -> Self {
        value.0
    }
}

impl AsRef<str> for TaskId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for TaskId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

const MAX_MODEL_NAME_BYTES: usize = 256;
const MAX_PANE_ID_BYTES: usize = 256;

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ModelName(String);

impl ModelName {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.len() > MAX_MODEL_NAME_BYTES {
            return Err(AtmError::validation(format!(
                "model must be at most {MAX_MODEL_NAME_BYTES} bytes"
            )));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl<'de> Deserialize<'de> for ModelName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl From<ModelName> for String {
    fn from(value: ModelName) -> Self {
        value.0
    }
}

impl AsRef<str> for ModelName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for ModelName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for ModelName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct PaneId(String);

impl PaneId {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(AtmError::validation("pane id must not be blank"));
        }
        if trimmed.len() > MAX_PANE_ID_BYTES {
            return Err(AtmError::validation(format!(
                "pane id must be at most {MAX_PANE_ID_BYTES} bytes"
            )));
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn from_cli(value: &str) -> Result<Self, AtmError> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(AtmError::validation("pane id must not be blank"));
        }
        let normalized = if trimmed.starts_with('%') || trimmed.contains(':') {
            trimmed.to_string()
        } else {
            format!("%{trimmed}")
        };
        Self::new(normalized)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl<'de> Deserialize<'de> for PaneId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl From<PaneId> for String {
    fn from(value: PaneId) -> Self {
        value.0
    }
}

impl AsRef<str> for PaneId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for PaneId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for PaneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The canonical durable-mailbox member key for nudge and queue surfaces.
///
/// One team-scoped agent identity. This is the key every pending-nudge,
/// drain, sweep, and pump surface uses; features must not define their own
/// per-surface member key. Distinct from the private
/// `atm_http_runtime::runtime_health::MemberKey`, whose migration onto this
/// type is a non-blocking follow-up.
///
/// # Examples
///
/// ```
/// use atm_storage::types::{AgentName, MemberKey, TeamName};
///
/// let team: TeamName = "atm-dev".parse().expect("team");
/// let agent: AgentName = "worker".parse().expect("agent");
/// let member = MemberKey::new(team, agent);
///
/// assert_eq!(member.to_string(), "worker@atm-dev");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MemberKey {
    pub team: TeamName,
    pub agent: AgentName,
}

impl MemberKey {
    #[must_use]
    pub fn new(team: TeamName, agent: AgentName) -> Self {
        Self { team, agent }
    }

    #[must_use]
    pub fn team(&self) -> &TeamName {
        &self.team
    }

    #[must_use]
    pub fn agent(&self) -> &AgentName {
        &self.agent
    }
}

impl fmt::Display for MemberKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.agent.as_str(), self.team.as_str())
    }
}
