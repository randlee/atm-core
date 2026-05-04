#![allow(dead_code)]

macro_rules! define_test_identities {
    ($team:literal, $sender:literal, $recipient:literal, $qa:literal, $lead:literal, $daemon:literal, $origin:literal) => {
        pub const TEST_TEAM: &str = $team;
        pub const TEST_SENDER: &str = $sender;
        pub const TEST_RECIPIENT: &str = $recipient;
        pub const TEST_QA: &str = $qa;
        pub const TEST_QA_AGENT: &str = TEST_QA;
        pub use crate::roles::ROLE_TEAM_LEAD;
        pub const TEST_LEAD: &str = $lead;
        pub const TEST_DAEMON: &str = $daemon;
        pub const TEST_ORIGIN: &str = $origin;
        pub const TEST_SENDER_ADDRESS: &str = concat!($sender, "@", $team);
        pub const TEST_RECIPIENT_ADDRESS: &str = concat!($recipient, "@", $team);
        pub const TEST_LEAD_ADDRESS: &str = concat!($lead, "@", $team);
    };
}

define_test_identities!(
    "test-team",
    "sender-a",
    "recipient",
    "qa-a",
    "test-lead",
    "daemon",
    "host-a"
);
