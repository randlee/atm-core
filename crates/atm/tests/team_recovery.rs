#![cfg(unix)]

mod support;

use std::fs;
use std::process::Command;
use std::thread;
use std::time::Duration;

use atm_core::schema::MessageEnvelope;
use atm_core::types::{AgentName, TeamName};
use chrono::Utc;
use serde_json::{Value, json};
use support::{ROLE_TEAM_LEAD, TEST_SENDER, TEST_TEAM};

#[test]
fn test_teams_lists_discovered_teams_deterministically() {
    let fixture = Fixture::new();
    fixture.write_team_config_value("zeta", json!({"members":[{"name":ROLE_TEAM_LEAD}]}));
    fixture.write_team_config_value(
        TEST_TEAM,
        json!({"members":[{"name":ROLE_TEAM_LEAD},{"name":TEST_SENDER}]}),
    );

    let output = fixture.run(&["teams", "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["action"], "list");
    assert_eq!(parsed["team"], TEST_TEAM);
    let teams = parsed["teams"].as_array().expect("teams array");
    assert_eq!(teams.len(), 2);
    assert_eq!(teams[0]["name"], TEST_TEAM);
    assert_eq!(teams[0]["member_count"], 2);
    assert_eq!(teams[1]["name"], "zeta");
}

#[test]
fn test_members_lists_current_roster_deterministically() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        TEST_TEAM,
        json!({
            "members": [
                {"name":TEST_SENDER,"agentType":"general-purpose","model":"sonnet","cwd":"/repo"},
                {"name":ROLE_TEAM_LEAD,"agentType":"lead","model":"opus","cwd":"/repo","tmuxPaneId":"%1"},
                {"name":"qa","agentType":"qa","model":"haiku","cwd":"/repo"}
            ]
        }),
    );

    let output = fixture.run(&["members", "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let parsed = fixture.stdout_json(&output);
    let members = parsed["members"].as_array().expect("members array");
    assert_eq!(members[0]["name"], ROLE_TEAM_LEAD);
    assert_eq!(members[1]["name"], TEST_SENDER);
    assert_eq!(members[2]["name"], "qa");
    assert_eq!(members[0]["tmux_pane_id"], "%1");
}

#[test]
fn test_add_member_rejects_duplicates_and_creates_inbox_state() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(TEST_TEAM, json!({"members":[{"name":ROLE_TEAM_LEAD}]}));

    let added = fixture.run(&[
        "teams",
        "add-member",
        TEST_TEAM,
        TEST_SENDER,
        "--agent-type",
        "general-purpose",
        "--model",
        "sonnet",
        "--json",
    ]);
    assert!(added.status.success(), "stderr: {}", fixture.stderr(&added));
    let parsed = fixture.stdout_json(&added);
    assert_eq!(parsed["action"], "add-member");
    assert_eq!(parsed["member"], TEST_SENDER);
    assert_eq!(parsed["created_inbox"], true);
    assert!(fixture.inbox_path(TEST_TEAM, TEST_SENDER).is_file());

    let config = fixture.read_team_config_value(TEST_TEAM);
    assert_eq!(config["members"].as_array().expect("members").len(), 2);

    let duplicate = fixture.run(&["teams", "add-member", TEST_TEAM, TEST_SENDER]);
    assert!(!duplicate.status.success());
    assert!(
        fixture.stderr(&duplicate).contains("already exists"),
        "stderr: {}",
        fixture.stderr(&duplicate)
    );

    let config = fixture.read_team_config_value(TEST_TEAM);
    assert_eq!(config["members"].as_array().expect("members").len(), 2);
}

#[test]
fn test_add_member_normalizes_tmux_member_shape() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(TEST_TEAM, json!({"members":[{"name":ROLE_TEAM_LEAD}]}));

    let added = fixture.run(&[
        "teams",
        "add-member",
        TEST_TEAM,
        TEST_SENDER,
        "--agent-type",
        "general-purpose",
        "--model",
        "sonnet",
        "--pane-id",
        "12",
        "--json",
    ]);
    assert!(added.status.success(), "stderr: {}", fixture.stderr(&added));

    let config = fixture.read_team_config_value(TEST_TEAM);
    let member = config["members"]
        .as_array()
        .expect("members")
        .iter()
        .find(|member| member["name"] == TEST_SENDER)
        .expect("sender member");
    assert_eq!(member["tmuxPaneId"], "%12");
    assert_eq!(member["backendType"], "tmux");
    assert_eq!(member["isActive"], true);
}

#[test]
fn test_add_member_rejects_non_canonical_tmux_target_syntax() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(TEST_TEAM, json!({"members":[{"name":ROLE_TEAM_LEAD}]}));

    let output = fixture.run(&[
        "teams",
        "add-member",
        TEST_TEAM,
        TEST_SENDER,
        "--agent-type",
        "general-purpose",
        "--model",
        "sonnet",
        "--pane-id",
        "session:1.2",
    ]);
    assert!(!output.status.success());
    assert!(
        fixture.stderr(&output).contains("tmux pane id"),
        "stderr: {}",
        fixture.stderr(&output)
    );
}

#[test]
fn test_add_member_rolls_back_inbox_when_config_write_fails() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(TEST_TEAM, json!({"members":[{"name":ROLE_TEAM_LEAD}]}));

    let output = fixture.run_with_env(
        &[
            "teams",
            "add-member",
            TEST_TEAM,
            TEST_SENDER,
            "--agent-type",
            "general-purpose",
            "--model",
            "sonnet",
            "--json",
        ],
        &[("ATM_TEST_FAIL_TEAM_CONFIG_WRITE", "1")],
    );
    assert!(
        !output.status.success(),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        fixture
            .stderr(&output)
            .contains("forced team config write failure"),
        "stderr: {}",
        fixture.stderr(&output)
    );
    assert!(!fixture.inbox_path(TEST_TEAM, TEST_SENDER).exists());

    let config = fixture.read_team_config_value(TEST_TEAM);
    let members = config["members"].as_array().expect("members");
    assert_eq!(members.len(), 1);
    assert_eq!(members[0]["name"], ROLE_TEAM_LEAD);
}

#[test]
fn test_backup_captures_config_inboxes_and_tasks() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        TEST_TEAM,
        json!({"leadSessionId":"lead-1","members":[{"name":ROLE_TEAM_LEAD},{"name":TEST_SENDER}]}),
    );
    fixture.write_inbox(TEST_TEAM, TEST_SENDER, "backup me");
    fixture.write_task(TEST_TEAM, 7, json!({"id":"7","status":"open"}));
    fixture.write_highwatermark(TEST_TEAM, "7\n");

    let output = fixture.run(&["teams", "backup", TEST_TEAM, "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let parsed = fixture.stdout_json(&output);
    let backup_path = parsed["backup_path"].as_str().expect("backup path");
    let backup_dir = std::path::Path::new(backup_path);
    assert!(backup_dir.join("config.json").is_file());
    assert!(
        backup_dir
            .join("inboxes")
            .join(format!("{TEST_SENDER}.json"))
            .is_file()
    );
    assert!(backup_dir.join("tasks").join("7.json").is_file());
    assert!(backup_dir.join("tasks").join(".highwatermark").is_file());
}

#[test]
fn test_backup_excludes_mailbox_lock_sentinels() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        TEST_TEAM,
        json!({"leadSessionId":"lead-1","members":[{"name":ROLE_TEAM_LEAD},{"name":TEST_SENDER}]}),
    );
    fixture.write_inbox(TEST_TEAM, TEST_SENDER, "backup me");
    fixture.write_text(
        fixture
            .team_dir(TEST_TEAM)
            .join("inboxes")
            .join(format!("{TEST_SENDER}.json.lock")),
        &u32::MAX.to_string(),
    );

    let output = fixture.run(&["teams", "backup", TEST_TEAM, "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let parsed = fixture.stdout_json(&output);
    let backup_path = parsed["backup_path"].as_str().expect("backup path");
    let backup_dir = std::path::Path::new(backup_path);
    assert!(
        !backup_dir
            .join("inboxes")
            .join(format!("{TEST_SENDER}.json.lock"))
            .exists()
    );
}

#[test]
fn test_restore_dry_run_reports_members_inboxes_and_tasks() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        TEST_TEAM,
        json!({"leadSessionId":"lead-current","members":[{"name":ROLE_TEAM_LEAD}]}),
    );

    let backup_dir = fixture.make_backup_dir(TEST_TEAM, "20260407T010203000000000Z");
    fixture.write_json(
        backup_dir.join("config.json"),
        &json!({
            "leadSessionId":"lead-backup",
            "members":[
                {"name":ROLE_TEAM_LEAD},
                {"name":TEST_SENDER,"agentType":"general-purpose","model":"sonnet","cwd":"/repo"}
            ]
        }),
    );
    fixture.write_inbox_at(
        backup_dir
            .join("inboxes")
            .join(format!("{TEST_SENDER}.json")),
        ROLE_TEAM_LEAD,
        "restored",
    );
    fixture.write_json(
        backup_dir.join("tasks").join("80.json"),
        &json!({"id":"80","status":"open"}),
    );

    let output = fixture.run(&[
        "teams",
        "restore",
        TEST_TEAM,
        "--from",
        backup_dir.to_str().expect("utf8"),
        "--dry-run",
        "--json",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["dry_run"], true);
    assert_eq!(parsed["would_restore_members"][0], TEST_SENDER);
    assert_eq!(
        parsed["would_restore_inboxes"][0],
        format!("{TEST_SENDER}.json")
    );
    assert_eq!(parsed["would_restore_tasks"], 1);
}

#[test]
fn test_restore_preserves_team_lead_and_recomputes_highwatermark() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        TEST_TEAM,
        json!({
            "leadSessionId":"lead-current",
            "members":[
                {"name":ROLE_TEAM_LEAD,"model":"current-lead","agentType":"lead","cwd":"/repo"},
                {"name":"existing","model":"existing","agentType":"worker","cwd":"/repo"}
            ]
        }),
    );
    fixture.write_inbox(TEST_TEAM, ROLE_TEAM_LEAD, "keep me");
    fixture.write_task(TEST_TEAM, 75, json!({"id":"75","status":"stale"}));
    fixture.write_highwatermark(TEST_TEAM, "75\n");

    let backup_dir = fixture.make_backup_dir(TEST_TEAM, "20260407T020304000000000Z");
    fixture.write_json(
        backup_dir.join("config.json"),
        &json!({
            "leadSessionId":"lead-backup",
            "members":[
                {"name":ROLE_TEAM_LEAD,"model":"backup-lead","agentType":"lead","cwd":"/backup"},
                {
                    "name":TEST_SENDER,
                    "agentId": format!("{TEST_SENDER}@{TEST_TEAM}"),
                    "agentType":"general-purpose",
                    "model":"sonnet",
                    "cwd":"/repo",
                    "tmuxPaneId":"%9",
                    "sessionId":"session-123",
                    "activity":"idle"
                }
            ]
        }),
    );
    fixture.write_inbox_at(
        backup_dir
            .join("inboxes")
            .join(format!("{ROLE_TEAM_LEAD}.json")),
        TEST_SENDER,
        "do not restore",
    );
    fixture.write_inbox_at(
        backup_dir
            .join("inboxes")
            .join(format!("{TEST_SENDER}.json")),
        ROLE_TEAM_LEAD,
        "restore worker inbox",
    );
    fixture.write_json(
        backup_dir.join("tasks").join("80.json"),
        &json!({"id":"80","status":"open"}),
    );
    fixture.write_json(
        backup_dir.join("tasks").join("82.json"),
        &json!({"id":"82","status":"done"}),
    );
    fixture.write_text(backup_dir.join("tasks").join(".highwatermark"), "1\n");

    let output = fixture.run(&[
        "teams",
        "restore",
        TEST_TEAM,
        "--from",
        backup_dir.to_str().expect("utf8"),
        "--json",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["members_restored"], 1);
    assert_eq!(parsed["inboxes_restored"], 1);
    assert_eq!(parsed["tasks_restored"], 2);

    let config = fixture.read_team_config_value(TEST_TEAM);
    assert_eq!(config["leadSessionId"], "lead-current");
    assert_eq!(config["members"][0]["name"], ROLE_TEAM_LEAD);
    assert_eq!(config["members"][0]["model"], "current-lead");

    let restored = config["members"]
        .as_array()
        .expect("members")
        .iter()
        .find(|member| member["name"] == TEST_SENDER)
        .expect("restored member");
    assert_eq!(restored["tmuxPaneId"], "");
    assert!(restored.get("sessionId").is_none());
    assert!(restored.get("activity").is_none());

    let team_lead_inbox =
        fs::read_to_string(fixture.inbox_path(TEST_TEAM, ROLE_TEAM_LEAD)).expect("lead inbox");
    assert!(team_lead_inbox.contains("keep me"));
    let restored_inbox =
        fs::read_to_string(fixture.inbox_path(TEST_TEAM, TEST_SENDER)).expect("restored inbox");
    assert!(restored_inbox.contains("restore worker inbox"));
    assert_eq!(fixture.read_highwatermark(TEST_TEAM), "82");
}

#[test]
fn test_restore_sweeps_stale_mailbox_lock_sentinels() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        TEST_TEAM,
        json!({"leadSessionId":"lead-current","members":[{"name":ROLE_TEAM_LEAD}]}),
    );
    fixture.write_text(
        fixture
            .team_dir(TEST_TEAM)
            .join("inboxes")
            .join(format!("{TEST_SENDER}.json.lock")),
        &u32::MAX.to_string(),
    );

    let backup_dir = fixture.make_backup_dir(TEST_TEAM, "20260407T020304500000000Z");
    fixture.write_json(
        backup_dir.join("config.json"),
        &json!({
            "leadSessionId":"lead-backup",
            "members":[
                {"name":ROLE_TEAM_LEAD},
                {"name":TEST_SENDER,"agentType":"general-purpose","model":"sonnet","cwd":"/repo"}
            ]
        }),
    );
    fixture.write_inbox_at(
        backup_dir
            .join("inboxes")
            .join(format!("{TEST_SENDER}.json")),
        ROLE_TEAM_LEAD,
        "restore worker inbox",
    );

    let output = fixture.run(&[
        "teams",
        "restore",
        TEST_TEAM,
        "--from",
        backup_dir.to_str().expect("utf8"),
        "--json",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    assert!(
        !fixture
            .team_dir(TEST_TEAM)
            .join("inboxes")
            .join(format!("{TEST_SENDER}.json.lock"))
            .exists()
    );
}

#[test]
fn test_backup_restore_roundtrip_leaves_zero_mailbox_locks() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        TEST_TEAM,
        json!({"leadSessionId":"lead-1","members":[{"name":ROLE_TEAM_LEAD},{"name":TEST_SENDER}]}),
    );
    fixture.write_inbox(TEST_TEAM, TEST_SENDER, "backup me");
    fixture.write_text(
        fixture
            .team_dir(TEST_TEAM)
            .join("inboxes")
            .join(format!("{TEST_SENDER}.json.lock")),
        &u32::MAX.to_string(),
    );

    let backup_output = fixture.run(&["teams", "backup", TEST_TEAM, "--json"]);
    assert!(
        backup_output.status.success(),
        "stderr: {}",
        fixture.stderr(&backup_output)
    );
    let backup_path = fixture.stdout_json(&backup_output)["backup_path"]
        .as_str()
        .expect("backup path")
        .to_string();

    let restore_output = fixture.run(&[
        "teams",
        "restore",
        TEST_TEAM,
        "--from",
        backup_path.as_str(),
        "--json",
    ]);
    assert!(
        restore_output.status.success(),
        "stderr: {}",
        fixture.stderr(&restore_output)
    );

    let lock_files = fs::read_dir(fixture.team_dir(TEST_TEAM).join("inboxes"))
        .expect("inboxes dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("lock"))
        .count();
    assert_eq!(lock_files, 0);
}

#[test]
fn test_restore_does_not_overwrite_existing_member_inbox() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        TEST_TEAM,
        json!({
            "leadSessionId":"lead-current",
            "members":[
                {"name":ROLE_TEAM_LEAD,"agentType":"lead","cwd":"/repo"},
                {"name":"existing","agentType":"worker","cwd":"/repo"}
            ]
        }),
    );
    fixture.write_inbox(TEST_TEAM, "existing", "keep existing inbox");

    let backup_dir = fixture.make_backup_dir(TEST_TEAM, "20260407T030405000000000Z");
    fixture.write_json(
        backup_dir.join("config.json"),
        &json!({
            "leadSessionId":"lead-backup",
            "members":[
                {"name":ROLE_TEAM_LEAD,"agentType":"lead","cwd":"/backup"},
                {"name":"existing","agentType":"worker","cwd":"/backup"},
                {"name":TEST_SENDER,"agentType":"general-purpose","cwd":"/repo"}
            ]
        }),
    );
    fixture.write_inbox_at(
        backup_dir.join("inboxes").join("existing.json"),
        ROLE_TEAM_LEAD,
        "do not overwrite existing inbox",
    );
    fixture.write_inbox_at(
        backup_dir
            .join("inboxes")
            .join(format!("{TEST_SENDER}.json")),
        ROLE_TEAM_LEAD,
        "restore new member inbox",
    );

    let output = fixture.run(&[
        "teams",
        "restore",
        TEST_TEAM,
        "--from",
        backup_dir.to_str().expect("utf8"),
        "--json",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let existing_inbox =
        fs::read_to_string(fixture.inbox_path(TEST_TEAM, "existing")).expect("existing inbox");
    assert!(existing_inbox.contains("keep existing inbox"));
    assert!(!existing_inbox.contains("do not overwrite existing inbox"));

    let restored_inbox =
        fs::read_to_string(fixture.inbox_path(TEST_TEAM, TEST_SENDER)).expect("restored inbox");
    assert!(restored_inbox.contains("restore new member inbox"));
}

#[test]
fn test_restore_rejects_preexisting_staging_before_restore() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        TEST_TEAM,
        json!({"leadSessionId":"lead-current","members":[{"name":ROLE_TEAM_LEAD}]}),
    );
    fixture.write_text(
        fixture
            .team_dir(TEST_TEAM)
            .join(".restore-staging")
            .join("stale.txt"),
        "stale marker",
    );
    fixture.write_inbox_at(
        fixture
            .team_dir(TEST_TEAM)
            .join(".restore-staging")
            .join("inboxes")
            .join("stale.json"),
        ROLE_TEAM_LEAD,
        "stale inbox content",
    );

    let backup_dir = fixture.make_backup_dir(TEST_TEAM, "20260407T040505000000000Z");
    fixture.write_json(
        backup_dir.join("config.json"),
        &json!({
            "leadSessionId":"lead-backup",
            "members":[
                {"name":ROLE_TEAM_LEAD},
                {"name":TEST_SENDER,"agentType":"general-purpose","model":"sonnet","cwd":"/repo"}
            ]
        }),
    );
    fixture.write_inbox_at(
        backup_dir
            .join("inboxes")
            .join(format!("{TEST_SENDER}.json")),
        ROLE_TEAM_LEAD,
        "fresh restored inbox",
    );

    let output = fixture.run(&[
        "teams",
        "restore",
        TEST_TEAM,
        "--from",
        backup_dir.to_str().expect("utf8"),
        "--json",
    ]);
    assert!(
        !output.status.success(),
        "stdout: {}",
        fixture.stderr(&output)
    );
    assert!(
        fixture
            .stderr(&output)
            .contains("restore staging directory already exists"),
        "stderr: {}",
        fixture.stderr(&output)
    );
    assert!(
        fixture
            .team_dir(TEST_TEAM)
            .join(".restore-staging")
            .exists()
    );
    assert!(!fixture.inbox_path(TEST_TEAM, TEST_SENDER).exists());
    assert!(!fixture.inbox_path(TEST_TEAM, "stale").exists());
}

#[test]
fn test_restore_inbox_staging_failure_preserves_config_and_live_state() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        TEST_TEAM,
        json!({
            "leadSessionId":"lead-current",
            "members":[{"name":ROLE_TEAM_LEAD}]
        }),
    );
    fixture.write_task(TEST_TEAM, 7, json!({"id":"7","status":"open"}));
    fixture.write_highwatermark(TEST_TEAM, "7\n");

    let backup_dir = fixture.make_backup_dir(TEST_TEAM, "20260407T040506500000000Z");
    fixture.write_json(
        backup_dir.join("config.json"),
        &json!({
            "leadSessionId":"lead-backup",
            "members":[
                {"name":ROLE_TEAM_LEAD},
                {"name":TEST_SENDER,"agentType":"general-purpose","model":"sonnet","cwd":"/repo"}
            ]
        }),
    );
    fixture.write_inbox_at(
        backup_dir
            .join("inboxes")
            .join(format!("{TEST_SENDER}.json")),
        ROLE_TEAM_LEAD,
        "restore worker inbox",
    );
    fixture.write_json(
        backup_dir.join("tasks").join("80.json"),
        &json!({"id":"80","status":"open"}),
    );

    let output = fixture.run_with_env(
        &[
            "teams",
            "restore",
            TEST_TEAM,
            "--from",
            backup_dir.to_str().expect("utf8"),
            "--json",
        ],
        &[("ATM_TEST_FAIL_RESTORE_INBOX_STAGE", "1")],
    );
    assert!(
        !output.status.success(),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let config = fixture.read_team_config_value(TEST_TEAM);
    let members = config["members"].as_array().expect("members");
    assert_eq!(members.len(), 1);
    assert_eq!(members[0]["name"], ROLE_TEAM_LEAD);
    assert_eq!(config["leadSessionId"], "lead-current");
    assert!(!fixture.inbox_path(TEST_TEAM, TEST_SENDER).exists());
    assert!(fixture.tasks_dir(TEST_TEAM).join("7.json").is_file());
    assert!(!fixture.tasks_dir(TEST_TEAM).join("80.json").exists());
    assert_eq!(fixture.read_highwatermark(TEST_TEAM), "7");
    assert!(
        fixture
            .team_dir(TEST_TEAM)
            .join(".restore-in-progress")
            .is_file()
    );
}

#[test]
fn test_restore_config_failure_leaves_restore_marker_and_rerun_completes() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        TEST_TEAM,
        json!({"leadSessionId":"lead-current","members":[{"name":ROLE_TEAM_LEAD}]}),
    );

    let backup_dir = fixture.make_backup_dir(TEST_TEAM, "20260407T040506000000000Z");
    fixture.write_json(
        backup_dir.join("config.json"),
        &json!({
            "leadSessionId":"lead-backup",
            "members":[
                {"name":ROLE_TEAM_LEAD},
                {"name":TEST_SENDER,"agentType":"general-purpose","model":"sonnet","cwd":"/repo"}
            ]
        }),
    );
    fixture.write_inbox_at(
        backup_dir
            .join("inboxes")
            .join(format!("{TEST_SENDER}.json")),
        ROLE_TEAM_LEAD,
        "restore worker inbox",
    );
    fixture.write_json(
        backup_dir.join("tasks").join("80.json"),
        &json!({"id":"80","status":"open"}),
    );

    let output = fixture.run_with_env(
        &[
            "teams",
            "restore",
            TEST_TEAM,
            "--from",
            backup_dir.to_str().expect("utf8"),
            "--json",
        ],
        &[("ATM_TEST_FAIL_TEAM_CONFIG_WRITE", "1")],
    );
    assert!(
        !output.status.success(),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        fixture
            .team_dir(TEST_TEAM)
            .join(".restore-in-progress")
            .is_file()
    );

    let doctor = fixture.run(&["doctor", "--json"]);
    assert!(
        doctor.status.success(),
        "stderr: {}",
        fixture.stderr(&doctor)
    );
    let parsed = fixture.stdout_json(&doctor);
    let findings = parsed["findings"].as_array().expect("findings array");
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "ATM_WARNING_RESTORE_IN_PROGRESS" && finding["severity"] == "warning"
        }),
        "stdout: {}",
        String::from_utf8(doctor.stdout.clone()).expect("stdout utf8")
    );

    let retry = fixture.run(&[
        "teams",
        "restore",
        TEST_TEAM,
        "--from",
        backup_dir.to_str().expect("utf8"),
        "--json",
    ]);
    assert!(retry.status.success(), "stderr: {}", fixture.stderr(&retry));
    assert!(
        !fixture
            .team_dir(TEST_TEAM)
            .join(".restore-in-progress")
            .exists()
    );
    let config = fixture.read_team_config_value(TEST_TEAM);
    assert!(
        config["members"]
            .as_array()
            .expect("members")
            .iter()
            .any(|member| member["name"] == TEST_SENDER)
    );
    assert!(fixture.inbox_path(TEST_TEAM, TEST_SENDER).is_file());
}

#[test]
fn test_restore_success_clears_restore_marker() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        TEST_TEAM,
        json!({"leadSessionId":"lead-current","members":[{"name":ROLE_TEAM_LEAD}]}),
    );

    let backup_dir = fixture.make_backup_dir(TEST_TEAM, "20260407T050607000000000Z");
    fixture.write_json(
        backup_dir.join("config.json"),
        &json!({
            "leadSessionId":"lead-backup",
            "members":[
                {"name":ROLE_TEAM_LEAD},
                {"name":TEST_SENDER,"agentType":"general-purpose","model":"sonnet","cwd":"/repo"}
            ]
        }),
    );
    fixture.write_inbox_at(
        backup_dir
            .join("inboxes")
            .join(format!("{TEST_SENDER}.json")),
        ROLE_TEAM_LEAD,
        "restore worker inbox",
    );

    let output = fixture.run(&[
        "teams",
        "restore",
        TEST_TEAM,
        "--from",
        backup_dir.to_str().expect("utf8"),
        "--json",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    assert!(
        !fixture
            .team_dir(TEST_TEAM)
            .join(".restore-in-progress")
            .exists()
    );
}

#[test]
fn test_restore_marker_removal_failure_is_warning_only() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        TEST_TEAM,
        json!({"leadSessionId":"lead-current","members":[{"name":ROLE_TEAM_LEAD}]}),
    );

    let backup_dir = fixture.make_backup_dir(TEST_TEAM, "20260407T050608000000000Z");
    fixture.write_json(
        backup_dir.join("config.json"),
        &json!({
            "leadSessionId":"lead-backup",
            "members":[
                {"name":ROLE_TEAM_LEAD},
                {"name":TEST_SENDER,"agentType":"general-purpose","model":"sonnet","cwd":"/repo"}
            ]
        }),
    );
    fixture.write_inbox_at(
        backup_dir
            .join("inboxes")
            .join(format!("{TEST_SENDER}.json")),
        ROLE_TEAM_LEAD,
        "restore worker inbox",
    );

    let output = fixture.run_with_env(
        &[
            "teams",
            "restore",
            TEST_TEAM,
            "--from",
            backup_dir.to_str().expect("utf8"),
            "--json",
        ],
        &[("ATM_TEST_FAIL_RESTORE_MARKER_REMOVE", "1")],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let config = fixture.read_team_config_value(TEST_TEAM);
    assert!(
        config["members"]
            .as_array()
            .expect("members")
            .iter()
            .any(|member| member["name"] == TEST_SENDER)
    );
    assert!(fixture.inbox_path(TEST_TEAM, TEST_SENDER).is_file());
    assert!(
        fixture
            .team_dir(TEST_TEAM)
            .join(".restore-in-progress")
            .is_file()
    );

    let doctor = fixture.run(&["doctor", "--json"]);
    assert!(
        doctor.status.success(),
        "stderr: {}",
        fixture.stderr(&doctor)
    );
    let parsed = fixture.stdout_json(&doctor);
    let findings = parsed["findings"].as_array().expect("findings array");
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "ATM_WARNING_RESTORE_IN_PROGRESS" && finding["severity"] == "warning"
        }),
        "stdout: {}",
        String::from_utf8(doctor.stdout.clone()).expect("stdout utf8")
    );
}

struct Fixture {
    tempdir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let tempdir = tempfile::tempdir().expect("tempdir");
        fs::write(
            tempdir.path().join(".atm.toml"),
            format!("default_team = \"{}\"\n", TEST_TEAM),
        )
        .expect("config");
        Self { tempdir }
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        for attempt in 0..3 {
            let mut first = Command::new(env!("CARGO_BIN_EXE_atm"));
            let output =
                support::configure_atm_command(&mut first, self.tempdir.path(), Some(TEST_SENDER))
                    .args(args)
                    .current_dir(self.tempdir.path())
                    .output()
                    .unwrap_or_else(|error| {
                        panic!("atm {:?} failed on attempt {attempt}: {error}", args)
                    });
            if output.status.success()
                || !support::is_daemon_start_transient(&output)
                || attempt == 2
            {
                return output;
            }

            thread::sleep(Duration::from_millis(50));
        }
        unreachable!("team_recovery retries should always return")
    }

    fn run_with_env(&self, args: &[&str], extra_env: &[(&str, &str)]) -> std::process::Output {
        let mut first = Command::new(env!("CARGO_BIN_EXE_atm"));
        support::configure_atm_command(&mut first, self.tempdir.path(), Some(TEST_SENDER))
            .args(args)
            .current_dir(self.tempdir.path());
        for (key, value) in extra_env {
            first.env(key, value);
        }
        let output = first.output().expect("run atm");
        if !support::is_daemon_start_transient(&output) {
            return output;
        }

        let mut retry = Command::new(env!("CARGO_BIN_EXE_atm"));
        support::configure_atm_command(&mut retry, self.tempdir.path(), Some(TEST_SENDER))
            .args(args)
            .current_dir(self.tempdir.path());
        for (key, value) in extra_env {
            retry.env(key, value);
        }
        retry.output().expect("retry atm")
    }
    fn write_team_config_value(&self, team: &str, value: Value) {
        self.write_json(self.team_dir(team).join("config.json"), &value);
    }

    fn read_team_config_value(&self, team: &str) -> Value {
        serde_json::from_slice(
            &fs::read(self.team_dir(team).join("config.json")).expect("config json"),
        )
        .expect("team config json")
    }

    fn write_inbox(&self, team: &str, member: &str, text: &str) {
        self.write_inbox_at(self.inbox_path(team, member), ROLE_TEAM_LEAD, text);
    }

    fn write_inbox_at(&self, path: std::path::PathBuf, from: &str, text: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("inbox dir");
        }
        let envelope = MessageEnvelope {
            from: from.parse::<AgentName>().expect("agent"),
            text: text.to_string(),
            timestamp: atm_core::types::IsoTimestamp::from_datetime(Utc::now()),
            read: false,
            source_team: Some(TEST_TEAM.parse::<TeamName>().expect("team")),
            summary: None,
            message_id: None,
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            task_id: None,
            extra: serde_json::Map::new(),
        };
        let raw = serde_json::to_string(&envelope).expect("envelope");
        fs::write(path, format!("{raw}\n")).expect("write inbox");
    }

    fn write_task(&self, team: &str, id: usize, value: Value) {
        self.write_json(self.tasks_dir(team).join(format!("{id}.json")), &value);
    }

    fn write_highwatermark(&self, team: &str, value: &str) {
        self.write_text(self.tasks_dir(team).join(".highwatermark"), value);
    }

    fn read_highwatermark(&self, team: &str) -> String {
        fs::read_to_string(self.tasks_dir(team).join(".highwatermark"))
            .expect("highwatermark")
            .trim()
            .to_string()
    }

    fn make_backup_dir(&self, team: &str, stamp: &str) -> std::path::PathBuf {
        let path = self
            .tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join(".backups")
            .join(team)
            .join(stamp);
        fs::create_dir_all(path.join("inboxes")).expect("backup inbox dir");
        fs::create_dir_all(path.join("tasks")).expect("backup task dir");
        path
    }

    fn write_json(&self, path: std::path::PathBuf, value: &Value) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("json dir");
        }
        fs::write(path, serde_json::to_vec_pretty(value).expect("json")).expect("write json");
    }

    fn write_text(&self, path: std::path::PathBuf, value: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("text dir");
        }
        fs::write(path, value).expect("write text");
    }

    fn stdout_json(&self, output: &std::process::Output) -> Value {
        serde_json::from_slice(&output.stdout).expect("valid json")
    }

    fn stderr(&self, output: &std::process::Output) -> String {
        String::from_utf8(output.stderr.clone()).expect("stderr utf8")
    }

    fn team_dir(&self, team: &str) -> std::path::PathBuf {
        self.tempdir.path().join(".claude").join("teams").join(team)
    }

    fn inbox_path(&self, team: &str, member: &str) -> std::path::PathBuf {
        self.team_dir(team)
            .join("inboxes")
            .join(format!("{member}.json"))
    }

    fn tasks_dir(&self, team: &str) -> std::path::PathBuf {
        self.tempdir.path().join(".claude").join("tasks").join(team)
    }
}
