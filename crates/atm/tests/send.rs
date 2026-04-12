use std::fs;
use std::path::PathBuf;
use std::process::Command;

use atm_core::schema::{AgentMember, MessageEnvelope, TeamConfig};

#[test]
fn test_send_creates_inbox_file() {
    let fixture = Fixture::new("recipient");

    let output = fixture.run(&["send", "recipient@atm-dev", "hello from test"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    assert!(
        fixture
            .stdout(&output)
            .contains("Sent to recipient@atm-dev [message_id:"),
        "stdout: {}",
        fixture.stdout(&output)
    );

    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].text, "hello from test");
    assert_eq!(inbox[0].from, "arch-ctm");
    assert!(inbox[0].message_id.is_some());
}

#[test]
fn test_send_dry_run_no_file() {
    let fixture = Fixture::new("recipient");

    let output = fixture.run(&[
        "send",
        "recipient@atm-dev",
        "hello from test",
        "--dry-run",
        "--json",
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid dry-run json");
    assert_eq!(parsed["action"], "send");
    assert_eq!(parsed["team"], "atm-dev");
    assert_eq!(parsed["agent"], "recipient");
    assert_eq!(parsed["message"], "hello from test");
    assert_eq!(parsed["dry_run"], true);
    assert_eq!(parsed["requires_ack"], false);

    assert!(!fixture.inbox_path("recipient").exists());
}

#[test]
fn test_send_json_output() {
    let fixture = Fixture::new("recipient");

    let output = fixture.run(&["send", "recipient@atm-dev", "hello json", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid send json");
    assert_eq!(parsed["action"], "send");
    assert_eq!(parsed["team"], "atm-dev");
    assert_eq!(parsed["agent"], "recipient");
    assert_eq!(parsed["outcome"], "sent");
    assert_eq!(parsed["requires_ack"], false);
    assert!(parsed["message_id"].as_str().is_some());
}

#[test]
fn test_send_emits_retained_log_record() {
    let fixture = Fixture::new("recipient");

    let send = fixture.run(&["send", "recipient@atm-dev", "hello emit", "--json"]);
    assert!(send.status.success(), "stderr: {}", fixture.stderr(&send));

    let output = fixture.run(&["log", "filter", "--match", "command=send", "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid log json");
    let records = parsed["records"].as_array().expect("records array");
    assert!(
        records.iter().any(|record| {
            record["fields"]["command"] == "send"
                && record["fields"]["agent"] == "recipient"
                && record["fields"]["team"] == "atm-dev"
        }),
        "stdout: {}",
        fixture.stdout(&output)
    );
}

#[test]
fn test_send_requires_ack() {
    let fixture = Fixture::new("recipient");

    let output = fixture.run(&["send", "recipient@atm-dev", "please ack", "--requires-ack"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
    assert!(inbox[0].pending_ack_at.is_some());
}

#[test]
fn test_send_persists_task_id() {
    let fixture = Fixture::new("recipient");

    let output = fixture.run(&[
        "send",
        "recipient@atm-dev",
        "task assignment",
        "--task-id",
        "TASK-123",
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].task_id.as_deref(), Some("TASK-123"));
}

#[test]
fn test_send_supports_positional_message_with_file() {
    let fixture = Fixture::new("recipient");
    let attachment = fixture.tempdir.path().join("notes.txt");
    fs::write(&attachment, "attachment body").expect("attachment");

    let output = fixture.run(&[
        "send",
        "recipient@atm-dev",
        "context first",
        "--file",
        attachment.to_str().expect("attachment path"),
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
    assert!(
        inbox[0]
            .text
            .starts_with("context first\n\nFile reference:")
    );
}

#[test]
fn test_send_tolerates_invalid_team_members_when_recipient_is_valid() {
    let fixture = Fixture::new("recipient");
    fixture.write_raw_team_config(
        r#"{
            "members": [
                {"name":"recipient"},
                {"broken": true},
                17
            ]
        }"#,
    );

    let output = fixture.run(&["send", "recipient@atm-dev", "hello despite bad siblings"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].text, "hello despite bad siblings");
}

#[test]
fn test_send_accepts_string_member_compatibility_form() {
    let fixture = Fixture::new("recipient");
    fixture.write_raw_team_config(r#"{"members":["recipient"]}"#);

    let output = fixture.run(&["send", "recipient@atm-dev", "hello legacy"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].text, "hello legacy");
}

#[test]
fn test_send_reports_actionable_error_for_malformed_team_config() {
    let fixture = Fixture::new("recipient");
    fixture.write_raw_team_config(r#"{"members":[{"name":"recipient"}"#);

    let output = fixture.run(&["send", "recipient@atm-dev", "hello"]);

    assert!(!output.status.success());
    let stderr = fixture.stderr(&output);
    assert!(stderr.contains("failed to parse team config"));
    assert!(stderr.contains("config.json"));
    assert!(stderr.contains("Repair the JSON syntax in config.json"));
}

#[test]
fn test_send_missing_config_uses_existing_inbox_fallback_and_warns_sender() {
    let fixture = Fixture::new("recipient");
    fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config");
    fixture.write_inbox("recipient", &[]);
    fixture.write_inbox("team-lead", &[]);

    let output = fixture.run(&["send", "recipient@atm-dev", "hello fallback"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let stdout = fixture.stdout(&output);
    let stderr = fixture.stderr(&output);
    assert!(stdout.contains("Sent to recipient@atm-dev"));
    assert!(stderr.contains("warning: team config is missing"));

    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].text, "hello fallback");

    let notices = fixture.inbox_contents("team-lead");
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].from, "atm-identity-missing@atm-dev");
    assert!(
        notices[0]
            .text
            .contains("ATM warning: send used existing inbox fallback")
    );
}

#[test]
fn test_send_does_not_fall_back_to_obsolete_config_identity() {
    let fixture = Fixture::new("recipient");
    fixture.write_atm_config("[atm]\nidentity = \"config-agent\"\n");

    let output = fixture.run_without_identity(&["send", "recipient@atm-dev", "hello"]);

    assert!(!output.status.success());
    let stderr = fixture.stderr(&output);
    assert!(
        stderr.contains("identity is not configured"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("Set ATM_IDENTITY"), "stderr: {stderr}");
}

#[test]
fn test_send_missing_config_deduplicates_team_lead_notice() {
    let fixture = Fixture::new("recipient");
    fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config");
    fixture.write_inbox("recipient", &[]);
    fixture.write_inbox("team-lead", &[]);

    let first = fixture.run(&["send", "recipient@atm-dev", "first"]);
    assert!(first.status.success(), "stderr: {}", fixture.stderr(&first));

    let second = fixture.run(&["send", "recipient@atm-dev", "second"]);
    assert!(
        second.status.success(),
        "stderr: {}",
        fixture.stderr(&second)
    );

    let notices = fixture.inbox_contents("team-lead");
    assert_eq!(notices.len(), 1);
}

#[test]
fn test_send_missing_config_deduplicates_team_lead_notice_under_concurrency() {
    let fixture = Fixture::new("recipient");
    fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config");
    fixture.write_inbox("recipient", &[]);
    fixture.write_inbox("team-lead", &[]);

    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(|| fixture.run(&["send", "recipient@atm-dev", "first"]));
        let second = scope.spawn(|| fixture.run(&["send", "recipient@atm-dev", "second"]));
        (
            first.join().expect("first send"),
            second.join().expect("second send"),
        )
    });

    assert!(first.status.success(), "stderr: {}", fixture.stderr(&first));
    assert!(
        second.status.success(),
        "stderr: {}",
        fixture.stderr(&second)
    );
    let notices = fixture.inbox_contents("team-lead");
    assert_eq!(notices.len(), 1);
}

#[test]
fn test_send_missing_config_notice_resets_after_config_is_restored() {
    let fixture = Fixture::new("recipient");
    fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config");
    fixture.write_inbox("recipient", &[]);
    fixture.write_inbox("team-lead", &[]);

    let first = fixture.run(&["send", "recipient@atm-dev", "first"]);
    assert!(first.status.success(), "stderr: {}", fixture.stderr(&first));
    assert_eq!(fixture.inbox_contents("team-lead").len(), 1);

    fixture.write_team_config("recipient");
    let second = fixture.run(&["send", "recipient@atm-dev", "with config restored"]);
    assert!(
        second.status.success(),
        "stderr: {}",
        fixture.stderr(&second)
    );
    assert_eq!(fixture.inbox_contents("team-lead").len(), 1);

    fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config again");
    let third = fixture.run(&["send", "recipient@atm-dev", "broken again"]);
    assert!(third.status.success(), "stderr: {}", fixture.stderr(&third));
    assert_eq!(fixture.inbox_contents("team-lead").len(), 2);
}

#[test]
fn test_send_missing_config_fails_when_recipient_inbox_does_not_exist() {
    let fixture = Fixture::new("recipient");
    fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config");

    let output = fixture.run(&["send", "recipient@atm-dev", "hello"]);

    assert!(!output.status.success());
    let stderr = fixture.stderr(&output);
    assert!(stderr.contains("team config is missing"));
    assert!(stderr.contains("cannot safely proceed"));
    assert!(stderr.contains("Restore config.json"));
}

#[test]
fn test_send_missing_config_does_not_block_when_team_lead_inbox_is_absent() {
    let fixture = Fixture::new("recipient");
    fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config");
    fixture.write_inbox("recipient", &[]);

    let output = fixture.run(&["send", "recipient@atm-dev", "hello fallback"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
}

#[test]
fn test_send_resolves_recipient_alias_before_membership_validation() {
    let fixture = Fixture::new("team-lead");
    fixture.write_atm_config("[atm]\n[atm.aliases]\ntl = \"team-lead\"\n");

    let output = fixture.run(&["send", "tl@atm-dev", "hello alias"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let inbox = fixture.inbox_contents("team-lead");
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].text, "hello alias");
}

#[test]
fn test_send_cross_team_projects_alias_and_persists_canonical_from_identity() {
    let fixture = Fixture::new("recipient");
    fixture.write_team_config_for_team("other-team", "recipient");
    fixture.write_atm_config("[atm]\n[atm.aliases]\nlead = \"arch-ctm\"\n");

    let output = fixture.run_with_env(
        &["send", "recipient@other-team", "hello cross-team"],
        &[("ATM_TEAM", "atm-dev")],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let inbox = fixture.inbox_contents_in_team("other-team", "recipient");
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].from, "lead");
    assert_eq!(
        inbox[0].extra["metadata"]["atm"]["fromIdentity"],
        "arch-ctm@atm-dev"
    );
}

#[test]
fn test_send_runs_post_send_hook_with_expected_payload() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("capture");
    fixture.write_atm_config(&format!(
        "[atm]\npost_send_hook = ['{}', 'capture', '{}']\npost_send_hook_senders = ['arch-ctm']\n",
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run(&["send", "recipient@atm-dev", "hello hook"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let payload: serde_json::Value =
        serde_json::from_slice(&fs::read(payload_path).expect("hook payload")).expect("json");
    assert_eq!(payload["from"], "arch-ctm@atm-dev");
    assert_eq!(payload["to"], "recipient@atm-dev");
    assert_eq!(payload["requires_ack"], false);
    assert!(payload["message_id"].as_str().is_some());
    assert!(payload.get("task_id").is_none());
    assert_eq!(payload["hook_match"]["sender"], true);
    assert_eq!(payload["hook_match"]["recipient"], false);
}

#[test]
fn test_send_post_send_hook_failure_does_not_roll_back_send() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("fail");
    fixture.write_atm_config(&format!(
        "[atm]\npost_send_hook = ['{}', 'fail', '{}']\npost_send_hook_senders = ['arch-ctm']\n",
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run(&["send", "recipient@atm-dev", "hello failed hook", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid send json");
    let warnings = parsed["warnings"].as_array().expect("warnings array");
    assert!(
        warnings.iter().any(|warning| warning
            .as_str()
            .is_some_and(|warning| warning.contains("post-send hook exited unsuccessfully"))),
        "stdout: {}",
        fixture.stdout(&output)
    );
    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
}

#[test]
fn test_send_emits_post_send_hook_skip_warning_when_no_filter_matches() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("capture");
    fixture.write_atm_config(&format!(
        "[atm]\npost_send_hook = ['{}', 'capture', '{}']\npost_send_hook_senders = ['team-lead']\npost_send_hook_recipients = ['quality-mgr']\n",
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run(&["send", "recipient@atm-dev", "hello skipped hook"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    assert!(!payload_path.exists(), "hook payload unexpectedly created");
    assert_eq!(
        fixture.stderr(&output),
        "post-send hook skipped: sender arch-ctm not in post_send_hook_senders team-lead\nand recipient recipient not in post_send_hook_recipients quality-mgr\n"
    );
    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
}

#[test]
fn test_send_emits_post_send_hook_skip_warning_on_stderr_in_json_mode() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("capture");
    fixture.write_atm_config(&format!(
        "[atm]\npost_send_hook = ['{}', 'capture', '{}']\npost_send_hook_senders = ['team-lead']\npost_send_hook_recipients = ['quality-mgr']\n",
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run(&["send", "recipient@atm-dev", "hello skipped hook", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    assert!(!payload_path.exists(), "hook payload unexpectedly created");
    assert_eq!(
        fixture.stderr(&output),
        "post-send hook skipped: sender arch-ctm not in post_send_hook_senders team-lead\nand recipient recipient not in post_send_hook_recipients quality-mgr\n"
    );
}

#[test]
fn test_send_runs_post_send_hook_when_recipient_matches_filter() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("capture");
    fixture.write_atm_config(&format!(
        "[atm]\npost_send_hook = ['{}', 'capture', '{}']\npost_send_hook_recipients = ['recipient']\n",
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run(&["send", "recipient@atm-dev", "hello recipient hook"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let payload: serde_json::Value =
        serde_json::from_slice(&fs::read(payload_path).expect("hook payload")).expect("json");
    assert_eq!(payload["hook_match"]["sender"], false);
    assert_eq!(payload["hook_match"]["recipient"], true);
}

#[test]
fn test_send_runs_post_send_hook_once_when_sender_and_recipient_both_match() {
    let fixture = Fixture::new("recipient");
    let (hook_path, counter_path) = fixture.install_hook_fixture("count");
    fixture.write_atm_config(&format!(
        "[atm]\npost_send_hook = ['{}', 'count', '{}']\npost_send_hook_senders = ['arch-ctm']\npost_send_hook_recipients = ['recipient']\n",
        hook_path.display(),
        counter_path.display()
    ));

    let output = fixture.run(&["send", "recipient@atm-dev", "hello both filters"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    assert_eq!(
        fs::read_to_string(counter_path).expect("counter").trim(),
        "1"
    );
}

#[test]
fn test_send_runs_post_send_hook_for_multiline_message_when_sender_matches() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("capture");
    fixture.write_atm_config(&format!(
        "[atm]\npost_send_hook = ['{}', 'capture', '{}']\npost_send_hook_senders = ['arch-ctm']\n",
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run(&[
        "send",
        "recipient@atm-dev",
        "<atm-task id=\"task-1\">\n  <description>Review the Phase 2 plan.</description>\n</atm-task>",
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let payload: serde_json::Value =
        serde_json::from_slice(&fs::read(payload_path).expect("hook payload")).expect("json");
    assert_eq!(payload["from"], "arch-ctm@atm-dev");
    assert_eq!(payload["to"], "recipient@atm-dev");
    assert!(payload["message_id"].as_str().is_some());
}

#[test]
fn test_send_ignores_post_send_hook_configured_only_in_core_section() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("capture");
    fixture.write_atm_config(&format!(
        "[core]\ndefault_team = 'atm-dev'\nidentity = 'team-lead'\npost_send_hook = ['{}', 'capture', '{}']\npost_send_hook_senders = ['arch-ctm']\npost_send_hook_recipients = ['recipient']\n",
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run(&["send", "recipient@atm-dev", "hello core section"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    assert!(!payload_path.exists(), "hook payload unexpectedly created");
    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
}

#[test]
fn test_send_post_send_hook_receives_only_configured_positional_args() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("capture-meta");
    fixture.write_atm_config(&format!(
        "[atm]\npost_send_hook = ['{}', 'capture-meta', '{}']\npost_send_hook_senders = ['arch-ctm']\n",
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run(&["send", "recipient@atm-dev", "hello args"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let captured: serde_json::Value =
        serde_json::from_slice(&fs::read(payload_path).expect("hook meta")).expect("json");
    assert_eq!(captured["args"], serde_json::json!([]));
    assert_eq!(captured["payload"]["to"], "recipient@atm-dev");
    assert_eq!(captured["payload"]["hook_match"]["sender"], true);
    assert_eq!(captured["payload"]["hook_match"]["recipient"], false);
}

#[test]
fn test_send_runs_post_send_hook_when_sender_filter_is_wildcard() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("capture");
    fixture.write_atm_config(&format!(
        "[atm]\npost_send_hook = ['{}', 'capture', '{}']\npost_send_hook_senders = ['*']\n",
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run(&["send", "recipient@atm-dev", "hello wildcard sender"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let payload: serde_json::Value =
        serde_json::from_slice(&fs::read(payload_path).expect("hook payload")).expect("json");
    assert_eq!(payload["hook_match"]["sender"], true);
    assert_eq!(payload["hook_match"]["recipient"], false);
}

#[test]
fn test_send_runs_post_send_hook_when_recipient_filter_is_wildcard() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("capture");
    fixture.write_atm_config(&format!(
        "[atm]\npost_send_hook = ['{}', 'capture', '{}']\npost_send_hook_recipients = ['*']\n",
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run(&["send", "recipient@atm-dev", "hello wildcard recipient"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let payload: serde_json::Value =
        serde_json::from_slice(&fs::read(payload_path).expect("hook payload")).expect("json");
    assert_eq!(payload["hook_match"]["sender"], false);
    assert_eq!(payload["hook_match"]["recipient"], true);
}

#[test]
fn test_send_does_not_run_post_send_hook_when_filter_lists_are_empty() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("capture");
    fixture.write_atm_config(&format!(
        "[atm]\npost_send_hook = ['{}', 'capture', '{}']\n",
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run(&["send", "recipient@atm-dev", "hello empty filters"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    assert!(!payload_path.exists(), "hook payload unexpectedly created");
    assert_eq!(fixture.stderr(&output), "");
    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
}

#[test]
fn test_send_rejects_retired_post_send_hook_members_config() {
    let fixture = Fixture::new("recipient");
    fixture.write_atm_config(
        "[atm]\npost_send_hook = ['bin/hook']\npost_send_hook_members = ['team-lead']\n",
    );

    let output = fixture.run(&["send", "recipient@atm-dev", "hello retired"]);

    assert!(!output.status.success());
    let stderr = fixture.stderr(&output);
    assert!(stderr.contains("post_send_hook_members"));
    assert!(stderr.contains(".atm.toml"));
    assert!(stderr.contains("Use 'post_send_hook_senders' (match on sender identity) and/or 'post_send_hook_recipients' (match on recipient name) under [atm]. Use '*' to match all senders or all recipients."));
}

#[test]
fn test_send_ignores_invalid_hook_result_stdout() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("result-invalid");
    fixture.write_atm_config(&format!(
        "[atm]\npost_send_hook = ['{}', 'result-invalid', '{}']\npost_send_hook_senders = ['arch-ctm']\n",
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run(&["send", "recipient@atm-dev", "hello invalid hook result"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    assert_eq!(fixture.stderr(&output), "");
    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
}

#[test]
fn test_send_logs_structured_hook_result_stdout() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("result-debug");
    fixture.write_atm_config(&format!(
        "[atm]\npost_send_hook = ['{}', 'result-debug', '{}']\npost_send_hook_senders = ['arch-ctm']\n",
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run_with_env(
        &[
            "--stderr-logs",
            "send",
            "recipient@atm-dev",
            "hello hook result",
            "--json",
        ],
        &[("ATM_LOG", "debug")],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["agent"], "recipient");
    assert_eq!(parsed["team"], "atm-dev");
    let stderr = fixture.stderr(&output);
    assert!(
        stderr.contains("hook fixture captured payload"),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains("atm_post_send_hook_fixture"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("hook_result_fields"), "stderr: {stderr}");
}

#[test]
fn test_send_help_mentions_post_send_hook_config() {
    let output = Command::new(env!("CARGO_BIN_EXE_atm"))
        .args(["send", "--help"])
        .output()
        .expect("run atm send --help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    assert!(stdout.contains("post_send_hook"));
    assert!(stdout.contains("post_send_hook_senders"));
    assert!(stdout.contains("post_send_hook_recipients"));
    assert!(stdout.contains("ATM_LOG=debug"));
    assert!(stdout.contains(".atm.toml"));
}

struct Fixture {
    tempdir: tempfile::TempDir,
}

impl Fixture {
    fn new(recipient: &str) -> Self {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let fixture = Self { tempdir };
        fixture.write_team_config(recipient);
        fixture
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        self.run_with_env(args, &[])
    }

    fn run_without_identity(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_atm"))
            .args(args)
            .env("ATM_HOME", self.tempdir.path())
            .env("ATM_CONFIG_HOME", self.tempdir.path())
            .env_remove("ATM_IDENTITY")
            .env("ATM_TEAM", "atm-dev")
            .current_dir(self.tempdir.path())
            .output()
            .expect("run atm without identity")
    }

    fn run_with_env(&self, args: &[&str], extra_env: &[(&str, &str)]) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_atm"));
        command
            .args(args)
            .env("ATM_HOME", self.tempdir.path())
            .env("ATM_CONFIG_HOME", self.tempdir.path())
            .env("ATM_IDENTITY", "arch-ctm")
            .env("ATM_TEAM", "atm-dev")
            .current_dir(self.tempdir.path());
        for (key, value) in extra_env {
            command.env(key, value);
        }
        command.output().expect("run atm")
    }

    fn write_team_config(&self, recipient: &str) {
        self.write_team_config_for_team("atm-dev", recipient);
    }

    fn write_team_config_for_team(&self, team: &str, recipient: &str) {
        let team_dir = self.tempdir.path().join(".claude").join("teams").join(team);
        fs::create_dir_all(&team_dir).expect("team dir");
        let config = TeamConfig {
            members: vec![AgentMember {
                name: recipient.to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        fs::write(
            team_dir.join("config.json"),
            serde_json::to_vec(&config).expect("team config"),
        )
        .expect("write team config");
    }

    fn write_raw_team_config(&self, raw: &str) {
        let team_dir = self
            .tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join("atm-dev");
        fs::create_dir_all(&team_dir).expect("team dir");
        fs::write(team_dir.join("config.json"), raw).expect("write raw team config");
    }

    fn write_atm_config(&self, raw: &str) {
        fs::write(self.tempdir.path().join(".atm.toml"), raw).expect("write .atm.toml");
    }

    fn inbox_path(&self, recipient: &str) -> std::path::PathBuf {
        self.inbox_path_in_team("atm-dev", recipient)
    }

    fn inbox_path_in_team(&self, team: &str, recipient: &str) -> std::path::PathBuf {
        self.tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join(team)
            .join("inboxes")
            .join(format!("{recipient}.json"))
    }

    fn write_inbox(&self, recipient: &str, messages: &[MessageEnvelope]) {
        let inbox_path = self.inbox_path(recipient);
        if let Some(parent) = inbox_path.parent() {
            fs::create_dir_all(parent).expect("inbox dir");
        }
        let raw = if messages.is_empty() {
            String::new()
        } else {
            format!(
                "{}\n",
                messages
                    .iter()
                    .map(|message| serde_json::to_string(message).expect("json line"))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        };
        fs::write(inbox_path, raw).expect("write inbox");
    }

    fn inbox_contents(&self, recipient: &str) -> Vec<MessageEnvelope> {
        self.inbox_contents_in_team("atm-dev", recipient)
    }

    fn inbox_contents_in_team(&self, team: &str, recipient: &str) -> Vec<MessageEnvelope> {
        let inbox_path = self.inbox_path_in_team(team, recipient);
        let raw = fs::read_to_string(&inbox_path).expect("inbox contents");
        if raw.trim().is_empty() {
            return Vec::new();
        }
        raw.lines()
            .map(|line| serde_json::from_str(line).expect("json line"))
            .collect()
    }

    fn team_dir(&self) -> std::path::PathBuf {
        self.tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join("atm-dev")
    }

    fn install_hook_fixture(&self, mode: &str) -> (PathBuf, PathBuf) {
        let fixture_binary = PathBuf::from(env!("CARGO_BIN_EXE_atm_post_send_hook_fixture"));
        let hook_dir = self.tempdir.path().join("bin");
        fs::create_dir_all(&hook_dir).expect("hook dir");
        let hook_path = hook_dir.join(fixture_binary.file_name().expect("hook binary filename"));
        fs::copy(&fixture_binary, &hook_path).expect("copy hook fixture");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = fs::metadata(&hook_path)
                .expect("hook metadata")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&hook_path, permissions).expect("hook permissions");
        }
        let payload_path = self.tempdir.path().join(format!("{mode}-payload.json"));
        (
            PathBuf::from("bin").join(hook_path.file_name().expect("copied hook binary filename")),
            payload_path,
        )
    }

    fn stdout(&self, output: &std::process::Output) -> String {
        String::from_utf8(output.stdout.clone()).expect("stdout utf8")
    }

    fn stdout_json(&self, output: &std::process::Output) -> serde_json::Value {
        serde_json::from_slice(&output.stdout).expect("valid json")
    }

    fn stderr(&self, output: &std::process::Output) -> String {
        String::from_utf8(output.stderr.clone()).expect("stderr utf8")
    }
}
