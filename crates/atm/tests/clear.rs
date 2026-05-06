#![cfg(all(unix, feature = "daemon-integration-tests"))]

mod support;

use std::ops::Deref;

use crate::support::CliFixture;
use atm_core::types::IsoTimestamp;
use chrono::{Duration, Utc};
use support::{TEST_LEAD, TEST_ORIGIN, TEST_SENDER, TEST_TEAM};

#[test]
fn test_clear_default_removes_only_read_and_acknowledged() {
    let fixture = Fixture::new(&[TEST_SENDER]);
    let base = Utc::now() - Duration::days(10);
    fixture.write_inbox(
        TEST_SENDER,
        &[
            fixture.message(TEST_LEAD, "unread", false, None, None, base),
            fixture.message(
                TEST_LEAD,
                "pending",
                true,
                Some(base + Duration::days(1)),
                None,
                base + Duration::days(1),
            ),
            fixture.message(
                TEST_LEAD,
                "read",
                true,
                None,
                None,
                base + Duration::days(2),
            ),
            fixture.message(
                TEST_LEAD,
                "acknowledged",
                true,
                None,
                Some(base + Duration::days(3)),
                base + Duration::days(3),
            ),
        ],
    );

    let output = fixture.run(&["clear", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["action"], "clear");
    assert_eq!(parsed["removed_total"], 2);
    assert_eq!(parsed["remaining_total"], 2);
    assert_eq!(parsed["removed_by_class"]["read"], 1);
    assert_eq!(parsed["removed_by_class"]["acknowledged"], 1);
    assert!(parsed["removed_by_class"]["unread"].is_null());
    assert!(parsed["removed_by_class"]["pending_ack"].is_null());

    let inbox = fixture.inbox_contents(TEST_SENDER);
    assert_eq!(inbox.len(), 2);
    assert_eq!(inbox[0].text, "unread");
    assert_eq!(inbox[1].text, "pending");
}

#[test]
fn test_clear_dry_run_does_not_mutate() {
    let fixture = Fixture::new(&[TEST_SENDER]);
    fixture.write_inbox(
        TEST_SENDER,
        &[fixture.message(
            TEST_LEAD,
            "read",
            true,
            None,
            None,
            Utc::now() - Duration::days(3),
        )],
    );

    let output = fixture.run(&["clear", "--dry-run", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["removed_total"], 1);

    let inbox = fixture.inbox_contents(TEST_SENDER);
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].text, "read");
}

#[test]
fn test_clear_emits_retained_log_record() {
    let fixture = Fixture::new(&[TEST_SENDER]);
    fixture.write_inbox(
        TEST_SENDER,
        &[fixture.message(
            TEST_LEAD,
            "read",
            true,
            None,
            None,
            Utc::now() - Duration::days(3),
        )],
    );

    let clear = fixture.run(&["clear", "--json"]);
    assert!(clear.status.success(), "stderr: {}", fixture.stderr(&clear));

    let output = fixture.run(&["log", "filter", "--match", "command=clear", "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    let records = parsed["records"].as_array().expect("records array");
    assert!(
        records.iter().any(|record| {
            record["fields"]["command"] == "clear"
                && record["fields"]["agent"] == TEST_SENDER
                && record["fields"]["team"] == TEST_TEAM
        }),
        "stdout: {}",
        String::from_utf8(output.stdout.clone()).expect("stdout utf8")
    );
}

#[test]
fn test_clear_never_removes_pending_ack() {
    let fixture = Fixture::new(&[TEST_SENDER]);
    fixture.write_inbox(
        TEST_SENDER,
        &[fixture.message(
            TEST_LEAD,
            "pending",
            true,
            Some(Utc::now() - Duration::days(2)),
            None,
            Utc::now() - Duration::days(2),
        )],
    );

    let output = fixture.run(&["clear", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["removed_total"], 0);
    assert_eq!(fixture.inbox_contents(TEST_SENDER).len(), 1);
    assert!(
        fixture.inbox_contents(TEST_SENDER)[0]
            .pending_ack_at
            .is_some()
    );
}

#[test]
fn test_clear_uses_workflow_sidecar_and_removes_cleared_entry() {
    let fixture = Fixture::new(&[TEST_SENDER]);
    let message = fixture.message(
        TEST_LEAD,
        "sidecar-managed read",
        false,
        None,
        None,
        Utc::now() - Duration::days(2),
    );
    let message_id = message.message_id.expect("message id");
    fixture.write_inbox(TEST_SENDER, &[message]);
    fixture.write_workflow_state(
        TEST_SENDER,
        serde_json::json!({
            "messages": {
                format!("legacy:{message_id}"): {
                    "read": true
                }
            }
        }),
    );

    let output = fixture.run(&["clear", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    assert!(fixture.inbox_contents(TEST_SENDER).is_empty());
    let workflow = fixture.workflow_state_contents(TEST_SENDER);
    assert!(workflow["messages"][format!("legacy:{message_id}")].is_null());
}

#[test]
fn test_clear_idle_only_removes_only_idle_notifications() {
    let fixture = Fixture::new(&[TEST_SENDER]);
    fixture.write_inbox(
        TEST_SENDER,
        &[
            fixture.message(
                TEST_LEAD,
                &idle_notification_text(TEST_LEAD),
                true,
                None,
                None,
                Utc::now() - Duration::days(4),
            ),
            fixture.message(
                TEST_LEAD,
                "normal read",
                true,
                None,
                None,
                Utc::now() - Duration::days(4),
            ),
        ],
    );

    let output = fixture.run(&["clear", "--idle-only", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["removed_total"], 1);
    assert_eq!(parsed["removed_by_class"]["read"], 1);

    let inbox = fixture.inbox_contents(TEST_SENDER);
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].text, "normal read");
}

#[test]
fn test_clear_preserves_unknown_fields_on_retained_messages() {
    let fixture = Fixture::new(&[TEST_SENDER]);
    let mut retained = fixture.message(
        TEST_LEAD,
        "pending",
        true,
        Some(Utc::now() - Duration::days(2)),
        None,
        Utc::now() - Duration::days(2),
    );
    retained
        .extra
        .insert("futureField".into(), serde_json::json!({"nested": true}));

    fixture.write_inbox(
        TEST_SENDER,
        &[
            fixture.message(
                TEST_LEAD,
                "clearable",
                true,
                None,
                None,
                Utc::now() - Duration::days(3),
            ),
            retained,
        ],
    );

    let output = fixture.run(&["clear", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let inbox = fixture.inbox_contents(TEST_SENDER);
    assert_eq!(inbox.len(), 1);
    assert_eq!(
        inbox[0].extra["futureField"],
        serde_json::json!({"nested": true})
    );
}

#[test]
fn test_clear_older_than_filters_candidates() {
    let fixture = Fixture::new(&[TEST_SENDER]);
    fixture.write_inbox(
        TEST_SENDER,
        &[
            fixture.message(
                TEST_LEAD,
                "older",
                true,
                None,
                None,
                Utc::now() - Duration::days(10),
            ),
            fixture.message(
                TEST_LEAD,
                "newer",
                true,
                None,
                None,
                Utc::now() - Duration::hours(6),
            ),
        ],
    );

    let output = fixture.run(&["clear", "--older-than", "7d", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["removed_total"], 1);

    let inbox = fixture.inbox_contents(TEST_SENDER);
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].text, "newer");
}

#[test]
fn test_clear_explicit_target() {
    let fixture = Fixture::new(&[TEST_SENDER, "agent-b"]);
    fixture.write_inbox(
        TEST_SENDER,
        &[fixture.message(
            TEST_LEAD,
            "keep mine",
            true,
            None,
            None,
            Utc::now() - Duration::days(10),
        )],
    );
    fixture.write_inbox(
        "agent-b",
        &[fixture.message(
            TEST_LEAD,
            "clear agent b",
            true,
            None,
            None,
            Utc::now() - Duration::days(10),
        )],
    );

    let output = fixture.run(&["clear", "agent-b", "--as", TEST_SENDER, "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["agent"], "agent-b");
    assert_eq!(parsed["removed_total"], 1);
    assert_eq!(fixture.inbox_contents("agent-b").len(), 0);
    assert_eq!(fixture.inbox_contents(TEST_SENDER).len(), 1);
}

#[test]
fn test_clear_removes_from_origin_inbox_file() {
    let fixture = Fixture::new(&[TEST_SENDER]);
    fixture.write_origin_inbox(
        TEST_SENDER,
        TEST_ORIGIN,
        &[fixture.message(
            TEST_LEAD,
            "origin read",
            true,
            None,
            None,
            Utc::now() - Duration::days(8),
        )],
    );

    let output = fixture.run(&["clear", "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    assert_eq!(
        fixture
            .origin_inbox_contents(TEST_SENDER, TEST_ORIGIN)
            .len(),
        0
    );
}

struct Fixture(CliFixture);

impl Fixture {
    fn new(members: &[&str]) -> Self {
        Self(CliFixture::new_with_members(members))
    }
}

impl Deref for Fixture {
    type Target = CliFixture;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

fn idle_notification_text(from: &str) -> String {
    serde_json::json!({
        "type": "idle_notification",
        "from": from,
        "timestamp": IsoTimestamp::now().into_inner().to_rfc3339(),
        "idleReason": "available"
    })
    .to_string()
}
