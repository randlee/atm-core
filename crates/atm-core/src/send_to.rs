//! Send-To CLI-surface support: picker-recipient resolution, same-host/
//! remote classification, attachment staging, and the `--from-json`
//! `PickerOutput` schema (ADR-055 decisions (d)-(g)).
//!
//! This module deliberately does **not** spawn a transfer-script child
//! process: that is real process I/O the `atm` CLI binary owns (it already
//! depends on `tokio::process`, `atm-core` does not). This module owns every
//! pure, synchronously-testable step around it: resolving a picker
//! recipient's registered host, classifying same-host vs remote, staging a
//! same-host attachment under `send_to_staging_dir()`, validating a transfer
//! script's untrusted stdout, and building the decision-(d) message-text
//! attachment note.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use ulid::Ulid;

use crate::address::AgentAddress;
use crate::atm_temp::{AtmTemp, send_to_staging_dir};
#[allow(
    deprecated,
    reason = "send_to resolves picker recipients through the retained atm-core roster boundary, matching every other roster-reading module in this crate"
)]
use crate::boundary::{RosterEntry, RosterStore};
use crate::error::AtmError;
use crate::types::{AgentName, HostName, TeamName};

/// Roster metadata key naming a member's registered host (ADR-055 decision
/// (e)). Stored under [`RosterEntry::metadata_json`], validated as a
/// [`HostName`] on read; never inferred from heartbeat, DNS, or socket state.
pub const ROSTER_HOST_METADATA_KEY: &str = "host";

/// The `PickerOutput` schema version this sprint understands (ADR-055
/// decision (g), PRD §4.2/§5a). An unrecognized version is rejected before
/// any staging or transfer -- the version gate is the picker compatibility
/// contract.
pub const PICKER_OUTPUT_SCHEMA_VERSION: u64 = 1;

/// Failures resolving a `--from-json` recipient into a routable address
/// (ADR-055 decisions (e)/(f)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressResolutionError {
    /// `member_id` is not `agent@team` shaped, or its parts do not parse as
    /// a valid [`AgentName`]/[`TeamName`].
    InvalidMemberId { member_id: String, reason: String },
    /// `member_id` does not name a roster member of the given team.
    UnknownMember { member_id: String },
    /// The member exists but has no registered host (`host: null`); remote
    /// routing for `--from-json` cannot proceed without one.
    HostUnregistered { member_id: String },
    /// The roster's stored `host` metadata value failed to parse as a
    /// [`HostName`].
    InvalidRegisteredHost { member_id: String, reason: String },
    /// A recipient is host-qualified but the sender's `.atm.toml` has no
    /// `local_host` (ADR-055 decision (f)): same-host vs. remote cannot be
    /// decided without guessing.
    LocalHostUnset { host: HostName },
    /// The roster could not be queried at all (storage-layer failure).
    RosterUnavailable { reason: String },
}

impl fmt::Display for AddressResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMemberId { member_id, reason } => {
                write!(f, "invalid recipient '{member_id}': {reason}")
            }
            Self::UnknownMember { member_id } => {
                write!(f, "'{member_id}' is not a known roster member")
            }
            Self::HostUnregistered { member_id } => {
                write!(
                    f,
                    "'{member_id}' has no registered host; remote Send-To routing requires one"
                )
            }
            Self::InvalidRegisteredHost { member_id, reason } => {
                write!(f, "'{member_id}' has an invalid registered host: {reason}")
            }
            Self::LocalHostUnset { host } => {
                write!(
                    f,
                    "recipient is qualified for host '{host}' but this machine's \
                     `.atm.toml` has no `local_host` set"
                )
            }
            Self::RosterUnavailable { reason } => {
                write!(f, "roster could not be queried: {reason}")
            }
        }
    }
}

impl std::error::Error for AddressResolutionError {}

impl AddressResolutionError {
    /// Actionable recovery text matching ADR-055's `AddressResolutionError`
    /// table.
    #[must_use]
    pub fn recovery(&self) -> String {
        match self {
            Self::InvalidMemberId { .. } => {
                "Use the `agent@team` shape from `atm teams --json --members`.".to_string()
            }
            Self::UnknownMember { .. } => "Run `atm members` to list known recipients.".to_string(),
            Self::HostUnregistered { .. } => {
                "Run `atm teams update-member --host <h>`, or send with an explicit \
                 `agent@team.host` recipient instead of `--from-json`."
                    .to_string()
            }
            Self::InvalidRegisteredHost { .. } => {
                "Run `atm teams update-member --host <h>` with a valid hostname.".to_string()
            }
            Self::LocalHostUnset { .. } => {
                "Set `local_host` in `.atm.toml`, or omit `--host`/route the recipient \
                 without a host qualifier to send it as local."
                    .to_string()
            }
            Self::RosterUnavailable { .. } => {
                "Retry once the local roster store is reachable.".to_string()
            }
        }
    }
}

impl From<AddressResolutionError> for AtmError {
    fn from(error: AddressResolutionError) -> Self {
        let recovery = error.recovery();
        AtmError::validation_with_recovery(error.to_string(), recovery)
    }
}

/// Extracts `member`'s registered host from its roster metadata (ADR-055
/// decision (e)).
///
/// Returns `Ok(None)` when the member has no `host` metadata key at all
/// (never inferred). A present-but-unparsable value is a hard error: an
/// operator who set a malformed host should see it, not have it silently
/// treated as unset.
///
/// # Errors
///
/// Returns [`AddressResolutionError::InvalidRegisteredHost`] when the stored
/// value is present but is not a valid [`HostName`] string.
pub fn member_host(
    member_id: &str,
    member: &RosterEntry,
) -> Result<Option<HostName>, AddressResolutionError> {
    let Some(raw) = member.metadata_json.get(ROSTER_HOST_METADATA_KEY) else {
        return Ok(None);
    };
    let Some(raw) = raw.as_str() else {
        return Err(AddressResolutionError::InvalidRegisteredHost {
            member_id: member_id.to_string(),
            reason: "the roster `host` value must be a string".to_string(),
        });
    };
    raw.parse::<HostName>().map(Some).map_err(|error| {
        AddressResolutionError::InvalidRegisteredHost {
            member_id: member_id.to_string(),
            reason: error.detail().to_string(),
        }
    })
}

/// Resolves one `--from-json` recipient (`agent@team`) into a routable
/// [`AgentAddress`] using the durable roster's registered host binding
/// (ADR-055 decision (e)). This is the single canonical picker-recipient
/// resolution algorithm.
///
/// # Errors
///
/// Returns [`AddressResolutionError`] when `member_id` is malformed, names an
/// unknown roster member, or the member's registered host is null (`--
/// from-json` cannot route a file transfer without one) or unparsable.
#[allow(
    deprecated,
    reason = "resolves against the retained atm-core RosterStore boundary, matching this module's other roster reads"
)]
pub fn resolve_picker_recipient(
    member_id: &str,
    roster: &dyn RosterStore,
) -> Result<AgentAddress, AddressResolutionError> {
    let (agent, team) = parse_member_id(member_id)?;
    let entry = roster
        .query_membership(&team, &agent)
        .map_err(|error| AddressResolutionError::RosterUnavailable {
            reason: error.detail().to_string(),
        })?
        .ok_or_else(|| AddressResolutionError::UnknownMember {
            member_id: member_id.to_string(),
        })?;
    let host = member_host(member_id, &entry)?.ok_or_else(|| {
        AddressResolutionError::HostUnregistered {
            member_id: member_id.to_string(),
        }
    })?;
    AgentAddress::new(agent, None, Some(team), Some(host)).map_err(|error| {
        AddressResolutionError::InvalidMemberId {
            member_id: member_id.to_string(),
            reason: error.detail().to_string(),
        }
    })
}

fn parse_member_id(member_id: &str) -> Result<(AgentName, TeamName), AddressResolutionError> {
    let invalid = |reason: &str| AddressResolutionError::InvalidMemberId {
        member_id: member_id.to_string(),
        reason: reason.to_string(),
    };
    let (agent_raw, team_raw) = member_id
        .split_once('@')
        .ok_or_else(|| invalid("expected the `agent@team` shape"))?;
    if team_raw.is_empty() || team_raw.contains('@') {
        return Err(invalid("expected exactly one `@`, with a non-empty team"));
    }
    let agent = agent_raw
        .parse::<AgentName>()
        .map_err(|error| invalid(error.detail()))?;
    let team = team_raw
        .parse::<TeamName>()
        .map_err(|error| invalid(error.detail()))?;
    Ok((agent, team))
}

/// Where a Send-To recipient's attachments must land (ADR-055 decision (f)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecipientLocality {
    /// Stage under this host's own `send_to_staging_dir()`.
    SameHost,
    /// Invoke the named host's transfer script.
    Remote(HostName),
}

/// Classifies a recipient as same-host or remote (ADR-055 decision (f)).
///
/// `recipient_host` is `None` when the resolved recipient has no host
/// qualifier (existing precedent: no host qualifier means "local to the
/// receiving/authenticated boundary", [`AgentAddress::without_host`]'s doc
/// comment) -- that is always same-host, with no `local_host` requirement.
/// A *present* `recipient_host` requires `local_host` to be set so same-host
/// vs. remote can be decided without guessing.
///
/// # Errors
///
/// Returns [`AddressResolutionError::LocalHostUnset`] when `recipient_host`
/// is `Some` but `local_host` is `None`.
pub fn classify_recipient_locality(
    recipient_host: Option<&HostName>,
    local_host: Option<&HostName>,
) -> Result<RecipientLocality, AddressResolutionError> {
    let Some(recipient_host) = recipient_host else {
        return Ok(RecipientLocality::SameHost);
    };
    let Some(local_host) = local_host else {
        return Err(AddressResolutionError::LocalHostUnset {
            host: recipient_host.clone(),
        });
    };
    if recipient_host
        .as_str()
        .eq_ignore_ascii_case(local_host.as_str())
    {
        Ok(RecipientLocality::SameHost)
    } else {
        Ok(RecipientLocality::Remote(recipient_host.clone()))
    }
}

/// One recipient entry in a `--from-json` `PickerOutput` document.
///
/// A bare string, deserialized as the picker `agent@team` recipient id
/// (ADR-055's `resolve_picker_recipient` input shape).
pub type PickerRecipientId = String;

/// The `--from-json` stdin document contract (ADR-055 decisions (g), PRD
/// §4.2/§5a).
///
/// Unknown top-level keys are rejected (`deny_unknown_fields`): the picker
/// compatibility contract cannot silently accept a shape it does not fully
/// understand.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PickerOutput {
    pub schema_version: u64,
    pub recipients: Vec<PickerRecipientId>,
    #[serde(default)]
    pub note: Option<String>,
}

/// Failures validating a `--from-json` stdin document before any staging or
/// transfer begins (R5/R13: a malformed request stages and sends nothing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerOutputError {
    /// The stdin payload was not valid JSON, or did not match the
    /// `PickerOutput` shape (including an unrecognized top-level key or
    /// trailing/non-terminated input).
    Malformed(String),
    /// `schema_version` is not [`PICKER_OUTPUT_SCHEMA_VERSION`].
    UnrecognizedSchemaVersion(u64),
    /// `recipients` was empty.
    EmptyRecipients,
    /// The same recipient id appeared more than once.
    DuplicateRecipient(String),
}

impl fmt::Display for PickerOutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(reason) => write!(f, "--from-json input is malformed: {reason}"),
            Self::UnrecognizedSchemaVersion(version) => write!(
                f,
                "--from-json schema_version {version} is not supported (expected \
                 {PICKER_OUTPUT_SCHEMA_VERSION})"
            ),
            Self::EmptyRecipients => f.write_str("--from-json recipients must not be empty"),
            Self::DuplicateRecipient(id) => {
                write!(f, "--from-json recipients contains a duplicate: '{id}'")
            }
        }
    }
}

impl std::error::Error for PickerOutputError {}

impl From<PickerOutputError> for AtmError {
    fn from(error: PickerOutputError) -> Self {
        AtmError::validation_with_recovery(
            error.to_string(),
            "Provide one PickerOutput object: \
             {\"schema_version\":1,\"recipients\":[\"agent@team\",...],\"note\":\"...\"}.",
        )
    }
}

/// Parses and validates a `--from-json` stdin document (ADR-055 decision
/// (g), PRD §4.2/§5a).
///
/// Rejects unknown keys, empty/duplicate recipients, malformed/trailing
/// input, and an unrecognized `schema_version` -- all before any staging or
/// transfer begins (R5/R13).
///
/// # Errors
///
/// Returns [`PickerOutputError`] for every rejection case listed above.
pub fn parse_picker_output(raw: &str) -> Result<PickerOutput, PickerOutputError> {
    let output: PickerOutput = serde_json::from_str(raw)
        .map_err(|error| PickerOutputError::Malformed(error.to_string()))?;
    if output.schema_version != PICKER_OUTPUT_SCHEMA_VERSION {
        return Err(PickerOutputError::UnrecognizedSchemaVersion(
            output.schema_version,
        ));
    }
    if output.recipients.is_empty() {
        return Err(PickerOutputError::EmptyRecipients);
    }
    let mut seen = std::collections::HashSet::with_capacity(output.recipients.len());
    for recipient in &output.recipients {
        if !seen.insert(recipient.as_str()) {
            return Err(PickerOutputError::DuplicateRecipient(recipient.clone()));
        }
    }
    Ok(output)
}

/// Stages same-host attachments under `send_to_staging_dir()` (ADR-055
/// decision (a)/deliverable 3), returning the landed directory.
///
/// Every source file's readability is validated **before** any staging
/// occurs (missing/unreadable source -> hard error before any staging): this
/// function never partially stages a batch.
///
/// # Errors
///
/// Returns [`AtmError`] when a source file is missing, is not a regular
/// file, or the staging directory cannot be created, or a copy fails.
pub fn stage_same_host_attachments(
    atm_temp: &AtmTemp,
    transfer_id: &Ulid,
    files: &[PathBuf],
) -> Result<PathBuf, AtmError> {
    for file in files {
        let metadata = std::fs::metadata(file).map_err(|error| {
            AtmError::validation_with_recovery(
                format!("attachment '{}' could not be read: {error}", file.display()),
                "Check the path and permissions, then retry.",
            )
        })?;
        if !metadata.is_file() {
            return Err(AtmError::validation_with_recovery(
                format!("attachment '{}' is not a regular file", file.display()),
                "Attach a regular file, not a directory or special path.",
            ));
        }
    }
    let landed_dir = send_to_staging_dir(atm_temp, transfer_id);
    std::fs::create_dir_all(&landed_dir).map_err(|error| {
        AtmError::validation_with_recovery(
            format!(
                "could not create the staging directory {}: {error}",
                landed_dir.display()
            ),
            "Check `$ATM_TEMP` permissions and available disk space.",
        )
    })?;
    for file in files {
        let file_name = file.file_name().ok_or_else(|| {
            AtmError::validation_with_recovery(
                format!("attachment '{}' has no file name", file.display()),
                "Attach a file, not a path ending in `..` or `/`.",
            )
        })?;
        std::fs::copy(file, landed_dir.join(file_name)).map_err(|error| {
            AtmError::validation_with_recovery(
                format!("could not stage attachment '{}': {error}", file.display()),
                "Check the source file and staging-directory permissions.",
            )
        })?;
    }
    Ok(landed_dir)
}

/// Validates a transfer script's untrusted stdout as a landed-directory path
/// (ADR-055 decision (c)): exactly one line, absolute, no control
/// characters.
///
/// # Errors
///
/// Returns [`AtmError`] when stdout is not valid UTF-8, is empty, is more
/// than one line, contains a control character, or is not an absolute path.
pub fn validate_landed_dir_stdout(raw: &[u8]) -> Result<PathBuf, AtmError> {
    let invalid = |reason: &str| {
        AtmError::validation_with_recovery(
            format!("transfer script produced an invalid landed path: {reason}"),
            "Fix the transfer script to print exactly one absolute path on stdout.",
        )
    };
    let text = std::str::from_utf8(raw).map_err(|_source| invalid("stdout was not valid UTF-8"))?;
    let mut lines = text.lines();
    let Some(first) = lines.next() else {
        return Err(invalid("stdout was empty"));
    };
    if lines.next().is_some() {
        return Err(invalid("stdout must be exactly one line"));
    }
    if first.is_empty() {
        return Err(invalid("stdout must not be blank"));
    }
    if first.chars().any(char::is_control) {
        return Err(invalid("stdout must not contain control characters"));
    }
    let path = Path::new(first);
    if !path.is_absolute() {
        return Err(invalid("landed path must be absolute"));
    }
    Ok(path.to_path_buf())
}

/// Builds the decision-(d) message-text attachment note: landed paths ride
/// in ordinary message text, never a structured envelope field.
///
/// `landed_dir` is the local staging directory (same-host) or the transfer
/// script's validated stdout (remote); `source_files` are the original
/// attachment paths the caller passed to `--attach`, in order.
#[must_use]
pub fn format_attachment_note(landed_dir: &Path, source_files: &[PathBuf]) -> String {
    let mut text = String::from("Attached files (on this host):");
    for file in source_files {
        let file_name = file
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| file.display().to_string());
        text.push_str("\n- ");
        text.push_str(&landed_dir.join(file_name).display().to_string());
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atm_temp::{EnvSource, resolve_atm_temp};
    use crate::boundary::{RosterHarness, RosterMemberKind};
    use serde_json::json;

    struct FixedEnvSource(std::collections::HashMap<&'static str, String>);

    impl EnvSource for FixedEnvSource {
        fn var(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
    }

    /// Builds a real, security-checked [`AtmTemp`] rooted at an already
    /// `0700`-permissioned throwaway directory, exercising the same
    /// `resolve_atm_temp` construction path production code uses (`AtmTemp`
    /// has no other constructor). `tempfile::tempdir()`'s mode depends on the
    /// process umask, not always `0700`, so this secures it explicitly first
    /// -- the same pattern `atm_temp.rs`'s own tests use.
    fn atm_temp_for_tests(root: &Path) -> AtmTemp {
        secure_dir_for_tests(root);
        let mut vars = std::collections::HashMap::new();
        vars.insert("ATM_TEMP", root.to_str().expect("utf8 path").to_string());
        resolve_atm_temp(&FixedEnvSource(vars)).expect("throwaway root resolves")
    }

    #[cfg(unix)]
    fn secure_dir_for_tests(dir: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .expect("chmod test root");
    }

    #[cfg(not(unix))]
    fn secure_dir_for_tests(_dir: &Path) {}

    fn agent_name(value: &str) -> AgentName {
        value.parse().expect("valid agent name")
    }

    fn team_name(value: &str) -> TeamName {
        value.parse().expect("valid team name")
    }

    fn host(value: &str) -> HostName {
        value.parse().expect("valid host")
    }

    fn roster_entry(agent: &str, team: &str, host: Option<&str>) -> RosterEntry {
        let mut metadata_json = serde_json::Map::new();
        if let Some(host) = host {
            metadata_json.insert(ROSTER_HOST_METADATA_KEY.to_string(), json!(host));
        }
        RosterEntry {
            team_name: team_name(team),
            agent_name: agent_name(agent),
            member_kind: RosterMemberKind::Permanent,
            harness: RosterHarness::ClaudeCode,
            agent_type: Default::default(),
            model: Default::default(),
            recipient_pane_id: None,
            metadata_json,
        }
    }

    struct FakeRosterStore {
        entries: Vec<RosterEntry>,
    }

    #[allow(
        deprecated,
        reason = "test double for the retained RosterStore boundary"
    )]
    impl crate::boundary::sealed::Sealed for FakeRosterStore {}

    #[allow(
        deprecated,
        reason = "test double for the retained RosterStore boundary"
    )]
    impl RosterStore for FakeRosterStore {
        fn replace_roster(
            &self,
            _team: &TeamName,
            _members: &[RosterEntry],
        ) -> Result<(), AtmError> {
            unimplemented!("not exercised by these tests")
        }

        fn load_roster(&self, team: &TeamName) -> Result<Vec<RosterEntry>, AtmError> {
            Ok(self
                .entries
                .iter()
                .filter(|entry| &entry.team_name == team)
                .cloned()
                .collect())
        }

        fn query_membership(
            &self,
            team: &TeamName,
            member: &AgentName,
        ) -> Result<Option<RosterEntry>, AtmError> {
            Ok(self
                .entries
                .iter()
                .find(|entry| &entry.team_name == team && &entry.agent_name == member)
                .cloned())
        }

        fn list_teams(&self) -> Result<Vec<TeamName>, AtmError> {
            Ok(self
                .entries
                .iter()
                .map(|entry| entry.team_name.clone())
                .collect())
        }

        fn health_snapshot(
            &self,
            _team: &TeamName,
        ) -> Result<crate::boundary::RosterStoreHealthSnapshot, AtmError> {
            unimplemented!("not exercised by these tests")
        }
    }

    // -- member_host --

    #[test]
    fn member_host_is_none_when_metadata_key_absent() {
        let entry = roster_entry("arch-ctm", "test-team", None);
        assert_eq!(member_host("arch-ctm@test-team", &entry), Ok(None));
    }

    #[test]
    fn member_host_parses_a_valid_registered_host() {
        let entry = roster_entry("arch-ctm", "test-team", Some("rand-m5.local"));
        assert_eq!(
            member_host("arch-ctm@test-team", &entry),
            Ok(Some(host("rand-m5.local")))
        );
    }

    #[test]
    fn member_host_rejects_an_unparsable_registered_host() {
        let entry = roster_entry("arch-ctm", "test-team", Some("has a space"));
        let error = member_host("arch-ctm@test-team", &entry).expect_err("invalid host string");
        assert!(matches!(
            error,
            AddressResolutionError::InvalidRegisteredHost { .. }
        ));
    }

    // -- resolve_picker_recipient --

    #[test]
    fn resolve_picker_recipient_resolves_a_registered_host() {
        let roster = FakeRosterStore {
            entries: vec![roster_entry("arch-ctm", "test-team", Some("rand-m5.local"))],
        };
        let resolved = resolve_picker_recipient("arch-ctm@test-team", &roster).expect("resolves");
        assert_eq!(resolved.agent(), &agent_name("arch-ctm"));
        assert_eq!(resolved.team(), Some(&team_name("test-team")));
        assert_eq!(resolved.host(), Some(&host("rand-m5.local")));
    }

    #[test]
    fn resolve_picker_recipient_fails_closed_on_unknown_member() {
        let roster = FakeRosterStore { entries: vec![] };
        let error = resolve_picker_recipient("arch-ctm@test-team", &roster)
            .expect_err("unknown member must fail closed");
        assert!(matches!(
            error,
            AddressResolutionError::UnknownMember { .. }
        ));
    }

    #[test]
    fn resolve_picker_recipient_fails_closed_on_null_host() {
        let roster = FakeRosterStore {
            entries: vec![roster_entry("arch-ctm", "test-team", None)],
        };
        let error = resolve_picker_recipient("arch-ctm@test-team", &roster)
            .expect_err("null host must fail closed for --from-json");
        assert!(matches!(
            error,
            AddressResolutionError::HostUnregistered { .. }
        ));
    }

    #[test]
    fn resolve_picker_recipient_rejects_a_malformed_member_id() {
        let roster = FakeRosterStore { entries: vec![] };
        let error = resolve_picker_recipient("not-a-member-id", &roster).expect_err("malformed id");
        assert!(matches!(
            error,
            AddressResolutionError::InvalidMemberId { .. }
        ));
    }

    // -- classify_recipient_locality --

    #[test]
    fn no_host_qualifier_is_always_same_host() {
        assert_eq!(
            classify_recipient_locality(None, None),
            Ok(RecipientLocality::SameHost)
        );
        assert_eq!(
            classify_recipient_locality(None, Some(&host("rand-m5.local"))),
            Ok(RecipientLocality::SameHost)
        );
    }

    #[test]
    fn host_matching_local_host_is_same_host() {
        assert_eq!(
            classify_recipient_locality(Some(&host("rand-m5.local")), Some(&host("rand-m5.local"))),
            Ok(RecipientLocality::SameHost)
        );
    }

    #[test]
    fn host_differing_from_local_host_is_remote() {
        assert_eq!(
            classify_recipient_locality(Some(&host("fastpc4.local")), Some(&host("rand-m5.local"))),
            Ok(RecipientLocality::Remote(host("fastpc4.local")))
        );
    }

    #[test]
    fn host_qualified_recipient_with_unset_local_host_fails_closed() {
        let error = classify_recipient_locality(Some(&host("fastpc4.local")), None)
            .expect_err("must fail closed, never guess");
        assert_eq!(
            error,
            AddressResolutionError::LocalHostUnset {
                host: host("fastpc4.local")
            }
        );
    }

    // -- parse_picker_output --

    #[test]
    fn parse_picker_output_accepts_a_valid_document() {
        let output = parse_picker_output(
            r#"{"schema_version":1,"recipients":["a@team","b@team"],"note":"hi"}"#,
        )
        .expect("valid document");
        assert_eq!(output.recipients, vec!["a@team", "b@team"]);
        assert_eq!(output.note.as_deref(), Some("hi"));
    }

    #[test]
    fn parse_picker_output_note_is_optional() {
        let output =
            parse_picker_output(r#"{"schema_version":1,"recipients":["a@team"]}"#).expect("valid");
        assert_eq!(output.note, None);
    }

    #[test]
    fn parse_picker_output_rejects_unrecognized_schema_version() {
        let error = parse_picker_output(r#"{"schema_version":2,"recipients":["a@team"]}"#)
            .expect_err("unrecognized version");
        assert_eq!(error, PickerOutputError::UnrecognizedSchemaVersion(2));
    }

    #[test]
    fn parse_picker_output_rejects_empty_recipients() {
        let error = parse_picker_output(r#"{"schema_version":1,"recipients":[]}"#)
            .expect_err("empty recipients");
        assert_eq!(error, PickerOutputError::EmptyRecipients);
    }

    #[test]
    fn parse_picker_output_rejects_duplicate_recipients() {
        let error = parse_picker_output(r#"{"schema_version":1,"recipients":["a@team","a@team"]}"#)
            .expect_err("duplicate recipients");
        assert_eq!(
            error,
            PickerOutputError::DuplicateRecipient("a@team".to_string())
        );
    }

    #[test]
    fn parse_picker_output_rejects_unknown_keys() {
        let error =
            parse_picker_output(r#"{"schema_version":1,"recipients":["a@team"],"extra":1}"#)
                .expect_err("unknown key");
        assert!(matches!(error, PickerOutputError::Malformed(_)));
    }

    #[test]
    fn parse_picker_output_rejects_trailing_input() {
        let error =
            parse_picker_output(r#"{"schema_version":1,"recipients":["a@team"]}{"trailing":true}"#)
                .expect_err("trailing input");
        assert!(matches!(error, PickerOutputError::Malformed(_)));
    }

    #[test]
    fn parse_picker_output_rejects_malformed_json() {
        let error = parse_picker_output("not json").expect_err("malformed input");
        assert!(matches!(error, PickerOutputError::Malformed(_)));
    }

    // -- stage_same_host_attachments --

    #[test]
    fn stage_same_host_attachments_copies_files_into_the_shared_staging_convention() {
        let root = tempfile::tempdir().expect("tempdir");
        let atm_temp = atm_temp_for_tests(root.path());
        let source_dir = tempfile::tempdir().expect("source tempdir");
        let source_file = source_dir.path().join("report.pdf");
        std::fs::write(&source_file, b"pdf-bytes").expect("write source file");
        let transfer_id = Ulid::new();

        let landed_dir = stage_same_host_attachments(
            &atm_temp,
            &transfer_id,
            std::slice::from_ref(&source_file),
        )
        .expect("stages");

        assert_eq!(landed_dir, send_to_staging_dir(&atm_temp, &transfer_id));
        let landed_file = landed_dir.join("report.pdf");
        assert_eq!(
            std::fs::read(&landed_file).expect("landed file exists"),
            b"pdf-bytes"
        );
    }

    #[test]
    fn stage_same_host_attachments_hard_errors_before_any_staging_on_missing_source() {
        let root = tempfile::tempdir().expect("tempdir");
        let atm_temp = atm_temp_for_tests(root.path());
        let source_dir = tempfile::tempdir().expect("source tempdir");
        let present = source_dir.path().join("present.txt");
        std::fs::write(&present, b"ok").expect("write present file");
        let missing = source_dir.path().join("missing.txt");
        let transfer_id = Ulid::new();

        let error = stage_same_host_attachments(&atm_temp, &transfer_id, &[present, missing])
            .expect_err("missing source must hard-error before any staging");
        assert!(error.detail().contains("could not be read"));

        let landed_dir = send_to_staging_dir(&atm_temp, &transfer_id);
        assert!(
            !landed_dir.exists(),
            "no staging directory must be created when validation fails"
        );
    }

    // -- validate_landed_dir_stdout --

    #[test]
    fn validate_landed_dir_stdout_accepts_a_single_absolute_line() {
        let landed = validate_landed_dir_stdout(b"/remote/atm-temp/send-to/abc\n")
            .expect("valid single-line absolute path");
        assert_eq!(landed, PathBuf::from("/remote/atm-temp/send-to/abc"));
    }

    #[test]
    fn validate_landed_dir_stdout_rejects_multiple_lines() {
        let error = validate_landed_dir_stdout(b"/one\n/two\n").expect_err("multi-line");
        assert!(error.detail().contains("exactly one line"));
    }

    #[test]
    fn validate_landed_dir_stdout_rejects_relative_paths() {
        let error = validate_landed_dir_stdout(b"relative/path\n").expect_err("relative path");
        assert!(error.detail().contains("absolute"));
    }

    #[test]
    fn validate_landed_dir_stdout_rejects_control_characters() {
        let error = validate_landed_dir_stdout(b"/tmp/has\ttab\n").expect_err("control character");
        assert!(error.detail().contains("control"));
    }

    #[test]
    fn validate_landed_dir_stdout_rejects_empty_output() {
        let error = validate_landed_dir_stdout(b"").expect_err("empty stdout");
        assert!(error.detail().contains("empty"));
    }

    // -- format_attachment_note --

    #[test]
    fn format_attachment_note_lists_every_landed_file() {
        let note = format_attachment_note(
            Path::new("/atm-temp/send-to/abc"),
            &[
                PathBuf::from("/local/report.pdf"),
                PathBuf::from("/local/notes.txt"),
            ],
        );
        assert!(note.starts_with("Attached files (on this host):"));
        assert!(note.contains("/atm-temp/send-to/abc/report.pdf"));
        assert!(note.contains("/atm-temp/send-to/abc/notes.txt"));
    }
}
