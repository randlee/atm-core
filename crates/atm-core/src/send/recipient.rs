use crate::address::AgentAddress;
use crate::config;
use crate::error::AtmError;
use crate::provenance::ValidatedWriteProvenance;
use crate::types::{AgentName, TeamName};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedRecipient {
    pub(crate) agent: AgentName,
    pub(crate) team: TeamName,
}

pub(crate) fn validate_non_self_recipient(
    sender: &AgentName,
    sender_team: &TeamName,
    recipient: &ResolvedRecipient,
    target: &AgentAddress,
    provenance: ValidatedWriteProvenance,
) -> Result<(), AtmError> {
    let same_identity = sender
        .as_str()
        .eq_ignore_ascii_case(recipient.agent.as_str())
        && sender_team
            .as_str()
            .eq_ignore_ascii_case(recipient.team.as_str());
    if same_identity && target.host().is_none() && !provenance.is_peer_receipt() {
        return Err(AtmError::self_addressed_send_invalid(format!(
            "self-addressed messages are invalid ATM input: '{sender}@{sender_team}' may not send to itself"
        )));
    }
    Ok(())
}

pub(crate) fn resolve_recipient(
    target_address: &AgentAddress,
    caller_team: &TeamName,
    config: Option<&config::AtmConfig>,
) -> Result<ResolvedRecipient, AtmError> {
    // `AgentAddress` has already validated the explicit team segment. Never
    // parse it again and silently substitute the caller team on failure.
    let team = target_address
        .team()
        .cloned()
        .unwrap_or_else(|| caller_team.clone());

    Ok(ResolvedRecipient {
        agent: config::aliases::resolve_agent_name(target_address.agent(), config)?,
        team,
    })
}

#[cfg(test)]
mod tests {
    use super::{ResolvedRecipient, validate_non_self_recipient};
    use crate::address::AgentAddress;
    use crate::error_codes::AtmErrorCode;
    use crate::provenance::{WriteIngress, WriteProvenance, validate_write_provenance};
    use crate::types::{AgentName, TeamName};

    #[test]
    fn rejects_case_variant_self_target() {
        let provenance = validate_write_provenance(
            WriteIngress::Canonical,
            WriteProvenance {
                target_host: None,
                authenticated_source_host: None,
                origin_message_id: false,
                origin_timestamp: false,
            },
        )
        .expect("local provenance");
        let error = validate_non_self_recipient(
            &AgentName::from_validated("Sender-A"),
            &TeamName::from_validated("Test-Team"),
            &ResolvedRecipient {
                agent: AgentName::from_validated("sender-a"),
                team: TeamName::from_validated("test-team"),
            },
            &"sender-a@test-team"
                .parse::<AgentAddress>()
                .expect("target"),
            provenance,
        )
        .expect_err("case-variant self target must be rejected");

        assert_eq!(error.code(), AtmErrorCode::SelfAddressedSendInvalid);
    }

    #[test]
    fn allows_host_qualified_self_target() {
        let target = "sender-a@test-team.127.0.0.1"
            .parse::<AgentAddress>()
            .expect("host-qualified target");
        let provenance = validate_write_provenance(
            WriteIngress::Canonical,
            WriteProvenance {
                target_host: target.host(),
                authenticated_source_host: None,
                origin_message_id: false,
                origin_timestamp: false,
            },
        )
        .expect("host-qualified origin provenance");
        validate_non_self_recipient(
            &AgentName::from_validated("sender-a"),
            &TeamName::from_validated("test-team"),
            &ResolvedRecipient {
                agent: AgentName::from_validated("sender-a"),
                team: TeamName::from_validated("test-team"),
            },
            &target,
            provenance,
        )
        .expect("host-qualified self target must use the ordinary peer route");
    }

    #[test]
    fn allows_authenticated_peer_after_target_normalization() {
        let target = "sender-a@test-team"
            .parse::<AgentAddress>()
            .expect("normalized target");
        let peer_host = "peer.example.test".parse().expect("peer host");
        let provenance = validate_write_provenance(
            WriteIngress::Canonical,
            WriteProvenance {
                target_host: target.host(),
                authenticated_source_host: Some(&peer_host),
                origin_message_id: true,
                origin_timestamp: true,
            },
        )
        .expect("authenticated peer provenance");
        validate_non_self_recipient(
            &AgentName::from_validated("sender-a"),
            &TeamName::from_validated("test-team"),
            &ResolvedRecipient {
                agent: AgentName::from_validated("sender-a"),
                team: TeamName::from_validated("test-team"),
            },
            &target,
            provenance,
        )
        .expect("authenticated peer receipt must not become a local self-send");
    }
}
