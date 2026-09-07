//! Pure roster-derived Herdr configuration decision for doctor projection.

use crate::delivery_channel::LocalMessageReceivedBackend;
use crate::team_admin::MembersList;

/// Returns whether the supplied roster has at least one Herdr-routed member.
///
/// The caller retains the same snapshot for endpoint grouping and member
/// observation, avoiding a second roster read that could produce a different
/// configuration decision.
#[must_use]
pub fn herdr_is_configured(roster: &MembersList) -> bool {
    roster.members.iter().any(|member| {
        matches!(
            member.local_message_received_backend(),
            Some(LocalMessageReceivedBackend::Herdr { .. })
        )
    })
}

#[cfg(test)]
mod tests {
    use crate::RosterHarness;
    use crate::delivery_channel::{HerdrSession, LocalMessageReceivedBackend};
    use crate::schema::HomeDirPath;
    use crate::team_admin::{MemberSummary, MembersList};
    use crate::test_support::TEST_TEAM;
    use crate::types::{AgentName, ModelName, TeamName};

    use super::herdr_is_configured;

    fn member(name: &str, local_backend: Option<LocalMessageReceivedBackend>) -> MemberSummary {
        MemberSummary {
            name: AgentName::from_validated(name.to_owned()),
            agent_id: String::new(),
            agent_type: String::new(),
            harness: RosterHarness::ClaudeCode,
            model: ModelName::default(),
            joined_at: None,
            tmux_pane_id: None,
            backend: None,
            herdr_session: None,
            herdr_agent: None,
            local_backend,
            home_dir: HomeDirPath::default(),
            live_cwd: None,
            host: None,
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn configured_is_derived_only_from_the_snapshot_backend_projection() {
        let team = TeamName::from_validated(TEST_TEAM);
        let absent = MembersList {
            team: team.clone(),
            members: vec![member("tmux-agent", None)],
        };
        let session = HerdrSession::new("review").expect("valid test session");
        let configured = MembersList {
            team,
            members: vec![
                member("tmux-agent", None),
                member(
                    "herdr-agent",
                    Some(LocalMessageReceivedBackend::Herdr {
                        session: Some(session),
                        agent: None,
                    }),
                ),
            ],
        };

        assert!(!herdr_is_configured(&absent));
        assert!(herdr_is_configured(&configured));
    }
}
