//! Deterministic sender-admission coverage for the daemon's virtual sender boundary.

use atm_core::{
    boundary::{SenderClaim, resolve_sender_claim},
    caller_context::CallerContext,
    test_support::TEST_TEAM,
    types::{AgentName, TeamName},
};

fn caller() -> CallerContext {
    CallerContext {
        caller_identity: AgentName::from_validated("creator"),
        caller_chat_id: None,
        caller_team: TeamName::from_validated(TEST_TEAM),
        activity_observation: None,
    }
}

#[test]
fn raw_cron_tool_sender_is_refused_while_the_authenticated_creator_is_preserved() {
    let creator: atm_storage::AgentAddress = format!("creator@{TEST_TEAM}")
        .parse()
        .expect("creator address");
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
    .expect_err("textual cron sender cannot be claimed as an ordinary sender");
    assert!(error.detail().contains("daemon-owned scheduler principal"));
}
