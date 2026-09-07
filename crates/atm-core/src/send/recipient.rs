use crate::address::AgentAddress;
use crate::boundary::RosterEntry;
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
    if same_identity && target.host().is_none() && !provenance.is_authenticated_peer() {
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

/// Resolves a roster-scoped alias after workspace aliases have already been
/// considered. Canonical roster names always win, which keeps malformed
/// historical metadata from shadowing a real member.
pub(crate) fn resolve_roster_alias(
    candidate: &AgentName,
    team: &TeamName,
    roster: &[RosterEntry],
) -> AgentName {
    if roster
        .iter()
        .any(|member| member.team_name == *team && member.agent_name == *candidate)
    {
        return candidate.clone();
    }

    roster
        .iter()
        .find(|member| {
            member.team_name == *team
                && member
                    .metadata_json
                    .get("alias")
                    .and_then(serde_json::Value::as_str)
                    == Some(candidate.as_str())
        })
        .map_or_else(|| candidate.clone(), |member| member.agent_name.clone())
}

#[cfg(test)]
mod tests {
    use super::{
        ResolvedRecipient, resolve_recipient, resolve_roster_alias, validate_non_self_recipient,
    };
    use crate::address::AgentAddress;
    use crate::boundary::{RosterEntry, RosterHarness, RosterMemberKind};
    use crate::error_codes::AtmErrorCode;
    use crate::provenance::{WriteIngress, WriteProvenance, validate_write_provenance};
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::types::{AgentName, TeamName};

    fn member(team: &TeamName, name: &str, alias: Option<&str>) -> RosterEntry {
        let mut metadata_json = serde_json::Map::new();
        if let Some(alias) = alias {
            metadata_json.insert("alias".to_string(), serde_json::json!(alias));
        }
        RosterEntry {
            team_name: team.clone(),
            agent_name: AgentName::from_validated(name),
            member_kind: RosterMemberKind::Permanent,
            harness: RosterHarness::ClaudeCode,
            agent_type: crate::schema::AgentType::from("worker".to_string()),
            model: crate::types::ModelName::new("gpt-5").expect("model"),
            recipient_pane_id: None,
            metadata_json,
        }
    }

    #[test]
    fn roster_alias_resolves_to_canonical_member() {
        let team = TeamName::from_validated("test-team");
        let roster = vec![member(&team, ROLE_TEAM_LEAD, Some("team-lead_atm-dev"))];

        assert_eq!(
            resolve_roster_alias(
                &AgentName::from_validated("team-lead_atm-dev"),
                &team,
                &roster,
            ),
            AgentName::from_validated(ROLE_TEAM_LEAD)
        );
    }

    #[test]
    fn roster_alias_resolves_for_implicit_and_explicit_team_targets() {
        let team = TeamName::from_validated("test-team");
        let roster = vec![member(&team, ROLE_TEAM_LEAD, Some("team-lead_atm-dev"))];

        for raw_target in ["team-lead_atm-dev", "team-lead_atm-dev@test-team"] {
            let target = raw_target.parse::<AgentAddress>().expect("target");
            let resolved = resolve_recipient(&target, &team, None).expect("parse recipient");
            assert_eq!(resolved.team, team);
            assert_eq!(
                resolve_roster_alias(&resolved.agent, &resolved.team, &roster),
                AgentName::from_validated(ROLE_TEAM_LEAD),
                "{raw_target} must resolve through roster alias"
            );
        }
    }

    #[test]
    fn unknown_roster_alias_preserves_the_canonical_parse_result() {
        let team = TeamName::from_validated("test-team");
        let target = "unknown-alias@test-team"
            .parse::<AgentAddress>()
            .expect("target");
        let resolved = resolve_recipient(&target, &team, None).expect("parse recipient");

        assert_eq!(
            resolve_roster_alias(&resolved.agent, &resolved.team, &[]),
            AgentName::from_validated("unknown-alias")
        );
    }

    #[test]
    fn canonical_name_wins_over_historical_alias_collision() {
        let team = TeamName::from_validated("test-team");
        let roster = vec![
            member(&team, ROLE_TEAM_LEAD, Some("worker")),
            member(&team, "worker", None),
        ];

        assert_eq!(
            resolve_roster_alias(&AgentName::from_validated("worker"), &team, &roster),
            AgentName::from_validated("worker")
        );
    }

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
