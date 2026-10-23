//! Deterministic sender-admission coverage for the daemon's virtual sender boundary.

use atm_core::{
    boundary::{SenderClaim, resolve_sender_claim},
    caller_context::CallerContext,
    types::{AgentName, TeamName},
};

fn caller() -> CallerContext {
    CallerContext {
        caller_identity: AgentName::from_validated("creator"),
        caller_chat_id: None,
        caller_team: TeamName::from_validated("atm-dev"),
        activity_observation: None,
    }
}

#[test]
fn raw_cron_tool_sender_is_refused_while_the_authenticated_creator_is_preserved() {
    let creator: atm_storage::AgentAddress = "creator@atm-dev".parse().expect("creator address");
    assert_eq!(
        resolve_sender_claim(SenderClaim::Ordinary {
            address: creator.clone(),
            caller: caller(),
        })
        .expect("authenticated creator"),
        creator,
    );

    let raw_virtual: atm_storage::AgentAddress =
        "cron@tool".parse().expect("virtual sender syntax");
    let error = resolve_sender_claim(SenderClaim::Ordinary {
        address: raw_virtual,
        caller: caller(),
    })
    .expect_err("textual cron sender cannot bypass daemon registry admission");
    assert!(error.detail().contains("registry admission"));
}
