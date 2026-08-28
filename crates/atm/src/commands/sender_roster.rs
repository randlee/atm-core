use atm_core::send::WarningEntry;
use atm_core::types::{AgentName, TeamName};
use atm_daemon_bootstrap::with_default_peer_address_stores;
use atm_storage::AtmError;

/// Return a local, non-blocking advisory when a claimed sender has no inbox.
///
/// The claimed identity remains a trusted-local assertion. A roster lookup is
/// therefore informational only: a failed lookup and an absent row must never
/// block a successfully completed CLI write.
pub(crate) fn unrostered_sender_warning(
    sender: &AgentName,
    team: &TeamName,
) -> Option<WarningEntry> {
    unrostered_sender_warning_with_lookup(sender, team, || {
        with_default_peer_address_stores(|roster_store, _peer_store| {
            roster_store.load_roster(team).map(|snapshot| {
                snapshot
                    .members
                    .into_iter()
                    .map(|member| member.agent_name)
                    .collect()
            })
        })
    })
}

fn unrostered_sender_warning_with_lookup(
    sender: &AgentName,
    team: &TeamName,
    lookup: impl FnOnce() -> Result<Vec<AgentName>, AtmError>,
) -> Option<WarningEntry> {
    let roster_members = lookup().ok()?;
    sender_roster_warning(sender, team, roster_members.iter())
}

fn sender_roster_warning<'a>(
    sender: &AgentName,
    team: &TeamName,
    roster_members: impl IntoIterator<Item = &'a AgentName>,
) -> Option<WarningEntry> {
    let rostered = roster_members.into_iter().any(|member| member == sender);
    (!rostered).then(|| {
        WarningEntry::new(
            format!(
                "declared sender {sender}@{team} is not on the ATM roster; this identity has no inbox and cannot receive replies or assignments."
            ),
            Some(format!(
                "Add it with `atm teams add-member {team} {sender}` if this identity needs an inbox."
            )),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{sender_roster_warning, unrostered_sender_warning_with_lookup};
    use atm_core::error::AtmError;
    use atm_core::test_support::{TEST_SENDER, TEST_TEAM};

    #[test]
    fn rostered_declared_sender_does_not_add_an_advisory() {
        let sender = TEST_SENDER.parse().expect("sender");
        let team = TEST_TEAM.parse().expect("team");

        assert!(sender_roster_warning(&sender, &team, [&sender]).is_none());
    }

    #[test]
    fn unrostered_declared_sender_gets_non_blocking_inbox_advisory() {
        let sender = "unregistered-tool".parse().expect("sender");
        let rostered_member = TEST_SENDER.parse().expect("rostered member");
        let team = TEST_TEAM.parse().expect("team");

        let warning = sender_roster_warning(&sender, &team, [&rostered_member])
            .expect("unrostered sender warning");

        assert!(warning.message.contains("unregistered-tool@test-team"));
        assert!(warning.message.contains("has no inbox"));
        assert!(
            warning
                .message
                .contains("cannot receive replies or assignments")
        );
        assert_eq!(
            warning.recovery.as_deref(),
            Some(
                "Add it with `atm teams add-member test-team unregistered-tool` if this identity needs an inbox."
            )
        );
    }

    #[test]
    fn failed_roster_lookup_is_silent_and_cannot_block_a_cli_write() {
        let sender = "unregistered-tool".parse().expect("sender");
        let team = TEST_TEAM.parse().expect("team");

        let warning = unrostered_sender_warning_with_lookup(&sender, &team, || {
            Err(AtmError::daemon_unavailable("test roster lookup failure"))
        });

        assert!(warning.is_none());
    }
}
