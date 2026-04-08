use std::fs;
use std::sync::{Arc, Barrier, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use atm_core::ack::{AckRequest, ack_mail};
use atm_core::observability::NullObservability;
use atm_core::schema::{AgentMember, LegacyMessageId, MessageEnvelope, TeamConfig};
use atm_core::types::IsoTimestamp;
use chrono::Utc;
use tempfile::TempDir;
use uuid::Uuid;

#[test]
fn concurrent_ack_on_overlapping_inbox_sets_completes_without_deadlock() {
    let fixture = Fixture::new();
    let observability = Arc::new(NullObservability);
    let barrier = Arc::new(Barrier::new(3));
    let (tx, rx) = mpsc::channel();

    let arch_request = fixture.ack_request("arch-ctm", fixture.arch_message_id, "ack from arch");
    let qa_request = fixture.ack_request("qa", fixture.qa_message_id, "ack from qa");

    let started = Instant::now();
    for (label, request) in [("arch", arch_request), ("qa", qa_request)] {
        let barrier = Arc::clone(&barrier);
        let tx = tx.clone();
        let observability = Arc::clone(&observability);
        thread::spawn(move || {
            barrier.wait();
            tx.send((label, ack_mail(request, observability.as_ref())))
                .expect("send result");
        });
    }
    drop(tx);

    barrier.wait();
    let first = rx
        .recv_timeout(Duration::from_secs(4))
        .expect("first ack result");
    let second = rx
        .recv_timeout(Duration::from_secs(4))
        .expect("second ack result");

    assert!(
        first.1.is_ok(),
        "first ack failed for {}: {:?}",
        first.0,
        first.1
    );
    assert!(
        second.1.is_ok(),
        "second ack failed for {}: {:?}; arch inbox: {:?}; qa inbox: {:?}",
        second.0,
        second.1,
        fixture.inbox_contents("arch-ctm"),
        fixture.inbox_contents("qa")
    );
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "overlapping ack operations exceeded the deadlock budget"
    );

    let arch_inbox = fixture.inbox_contents("arch-ctm");
    let qa_inbox = fixture.inbox_contents("qa");
    assert!(
        arch_inbox
            .iter()
            .any(|message| message.text == "ack from qa")
    );
    assert!(
        qa_inbox
            .iter()
            .any(|message| message.text == "ack from arch")
    );
}

struct Fixture {
    tempdir: TempDir,
    arch_message_id: LegacyMessageId,
    qa_message_id: LegacyMessageId,
}

impl Fixture {
    fn new() -> Self {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let team_dir = tempdir.path().join(".claude").join("teams").join("atm-dev");
        fs::create_dir_all(team_dir.join("inboxes")).expect("inboxes");

        let config = TeamConfig {
            members: ["team-lead", "arch-ctm", "qa"]
                .into_iter()
                .map(|name| AgentMember {
                    name: name.to_string(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        fs::write(
            team_dir.join("config.json"),
            serde_json::to_vec(&config).expect("team config"),
        )
        .expect("write team config");

        let arch_message_id = LegacyMessageId::from(Uuid::new_v4());
        let qa_message_id = LegacyMessageId::from(Uuid::new_v4());

        write_inbox(
            &team_dir.join("inboxes").join("arch-ctm.json"),
            &[message("qa", &arch_message_id, "arch pending")],
        );
        write_inbox(
            &team_dir.join("inboxes").join("qa.json"),
            &[message("arch-ctm", &qa_message_id, "qa pending")],
        );

        Self {
            tempdir,
            arch_message_id,
            qa_message_id,
        }
    }

    fn ack_request(
        &self,
        actor: &str,
        message_id: LegacyMessageId,
        reply_body: &str,
    ) -> AckRequest {
        AckRequest {
            home_dir: self.tempdir.path().to_path_buf(),
            current_dir: self.tempdir.path().to_path_buf(),
            actor_override: Some(actor.to_string()),
            team_override: Some("atm-dev".to_string()),
            message_id,
            reply_body: reply_body.to_string(),
        }
    }

    fn inbox_contents(&self, agent: &str) -> Vec<MessageEnvelope> {
        let path = self
            .tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join("atm-dev")
            .join("inboxes")
            .join(format!("{agent}.json"));
        let raw = fs::read_to_string(path).expect("inbox contents");
        raw.lines()
            .map(|line| serde_json::from_str(line).expect("json line"))
            .collect()
    }
}

fn write_inbox(path: &std::path::Path, messages: &[MessageEnvelope]) {
    let raw = messages
        .iter()
        .map(|message| serde_json::to_string(message).expect("json line"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(path, format!("{raw}\n")).expect("write inbox");
}

fn message(from: &str, message_id: &LegacyMessageId, text: &str) -> MessageEnvelope {
    MessageEnvelope {
        from: from.to_string(),
        text: text.to_string(),
        timestamp: IsoTimestamp::from_datetime(Utc::now()),
        read: true,
        source_team: Some("atm-dev".to_string()),
        summary: None,
        message_id: Some(*message_id),
        pending_ack_at: Some(IsoTimestamp::from_datetime(Utc::now())),
        acknowledged_at: None,
        acknowledges_message_id: None,
        task_id: None,
        extra: serde_json::Map::new(),
    }
}
