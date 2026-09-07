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
