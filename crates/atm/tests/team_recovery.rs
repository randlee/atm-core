use std::fs;
use std::process::Command;

use atm_core::schema::MessageEnvelope;
use chrono::Utc;
use serde_json::{Value, json};

#[test]
fn test_teams_lists_discovered_teams_deterministically() {
    let fixture = Fixture::new();
    fixture.write_team_config_value("zeta", json!({"members":[{"name":"team-lead"}]}));
    fixture.write_team_config_value(
        "atm-dev",
        json!({"members":[{"name":"team-lead"},{"name":"arch-ctm"}]}),
    );

    let output = fixture.run(&["teams", "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["action"], "list");
    assert_eq!(parsed["team"], "atm-dev");
    let teams = parsed["teams"].as_array().expect("teams array");
    assert_eq!(teams.len(), 2);
    assert_eq!(teams[0]["name"], "atm-dev");
    assert_eq!(teams[0]["member_count"], 2);
    assert_eq!(teams[1]["name"], "zeta");
}

#[test]
fn test_members_lists_current_roster_deterministically() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        "atm-dev",
        json!({
            "members": [
                {"name":"arch-ctm","agentType":"general-purpose","model":"sonnet","cwd":"/repo"},
                {"name":"team-lead","agentType":"lead","model":"opus","cwd":"/repo","tmuxPaneId":"%1"},
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
    assert_eq!(members[0]["name"], "team-lead");
    assert_eq!(members[1]["name"], "arch-ctm");
    assert_eq!(members[2]["name"], "qa");
    assert_eq!(members[0]["tmux_pane_id"], "%1");
}

#[test]
fn test_add_member_rejects_duplicates_and_creates_inbox_state() {
    let fixture = Fixture::new();
    fixture.write_team_config_value("atm-dev", json!({"members":[{"name":"team-lead"}]}));

    let added = fixture.run(&[
        "teams",
        "add-member",
        "atm-dev",
        "arch-ctm",
        "--agent-type",
        "general-purpose",
        "--model",
        "sonnet",
        "--json",
    ]);
    assert!(added.status.success(), "stderr: {}", fixture.stderr(&added));
    let parsed = fixture.stdout_json(&added);
    assert_eq!(parsed["action"], "add-member");
    assert_eq!(parsed["member"], "arch-ctm");
    assert_eq!(parsed["created_inbox"], true);
    assert!(fixture.inbox_path("atm-dev", "arch-ctm").is_file());

    let config = fixture.read_team_config_value("atm-dev");
    assert_eq!(config["members"].as_array().expect("members").len(), 2);

    let duplicate = fixture.run(&["teams", "add-member", "atm-dev", "arch-ctm"]);
    assert!(!duplicate.status.success());
    assert!(
        fixture.stderr(&duplicate).contains("already exists"),
        "stderr: {}",
        fixture.stderr(&duplicate)
    );

    let config = fixture.read_team_config_value("atm-dev");
    assert_eq!(config["members"].as_array().expect("members").len(), 2);
}

#[test]
fn test_add_member_rolls_back_inbox_when_config_write_fails() {
    let fixture = Fixture::new();
    fixture.write_team_config_value("atm-dev", json!({"members":[{"name":"team-lead"}]}));

    let output = fixture.run_with_env(
        &[
            "teams",
            "add-member",
            "atm-dev",
            "arch-ctm",
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
    assert!(!fixture.inbox_path("atm-dev", "arch-ctm").exists());

    let config = fixture.read_team_config_value("atm-dev");
    let members = config["members"].as_array().expect("members");
    assert_eq!(members.len(), 1);
    assert_eq!(members[0]["name"], "team-lead");
}

#[test]
fn test_backup_captures_config_inboxes_and_tasks() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        "atm-dev",
        json!({"leadSessionId":"lead-1","members":[{"name":"team-lead"},{"name":"arch-ctm"}]}),
    );
    fixture.write_inbox("atm-dev", "arch-ctm", "backup me");
    fixture.write_task("atm-dev", 7, json!({"id":"7","status":"open"}));
    fixture.write_highwatermark("atm-dev", "7\n");

    let output = fixture.run(&["teams", "backup", "atm-dev", "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let parsed = fixture.stdout_json(&output);
    let backup_path = parsed["backup_path"].as_str().expect("backup path");
    let backup_dir = std::path::Path::new(backup_path);
    assert!(backup_dir.join("config.json").is_file());
    assert!(backup_dir.join("inboxes").join("arch-ctm.json").is_file());
    assert!(backup_dir.join("tasks").join("7.json").is_file());
    assert!(backup_dir.join("tasks").join(".highwatermark").is_file());
}

#[test]
fn test_backup_excludes_mailbox_lock_sentinels() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        "atm-dev",
        json!({"leadSessionId":"lead-1","members":[{"name":"team-lead"},{"name":"arch-ctm"}]}),
    );
    fixture.write_inbox("atm-dev", "arch-ctm", "backup me");
    fixture.write_text(
        fixture
            .team_dir("atm-dev")
            .join("inboxes")
            .join("arch-ctm.json.lock"),
        &u32::MAX.to_string(),
    );

    let output = fixture.run(&["teams", "backup", "atm-dev", "--json"]);
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
            .join("arch-ctm.json.lock")
            .exists()
    );
}

#[test]
fn test_restore_dry_run_reports_members_inboxes_and_tasks() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        "atm-dev",
        json!({"leadSessionId":"lead-current","members":[{"name":"team-lead"}]}),
    );

    let backup_dir = fixture.make_backup_dir("atm-dev", "20260407T010203000000000Z");
    fixture.write_json(
        backup_dir.join("config.json"),
        &json!({
            "leadSessionId":"lead-backup",
            "members":[
                {"name":"team-lead"},
                {"name":"arch-ctm","agentType":"general-purpose","model":"sonnet","cwd":"/repo"}
            ]
        }),
    );
    fixture.write_inbox_at(
        backup_dir.join("inboxes").join("arch-ctm.json"),
        "team-lead",
        "restored",
    );
    fixture.write_json(
        backup_dir.join("tasks").join("80.json"),
        &json!({"id":"80","status":"open"}),
    );

    let output = fixture.run(&[
        "teams",
        "restore",
        "atm-dev",
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
    assert_eq!(parsed["would_restore_members"][0], "arch-ctm");
    assert_eq!(parsed["would_restore_inboxes"][0], "arch-ctm.json");
    assert_eq!(parsed["would_restore_tasks"], 1);
}

#[test]
fn test_restore_preserves_team_lead_and_recomputes_highwatermark() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        "atm-dev",
        json!({
            "leadSessionId":"lead-current",
            "members":[
                {"name":"team-lead","model":"current-lead","agentType":"lead","cwd":"/repo"},
                {"name":"existing","model":"existing","agentType":"worker","cwd":"/repo"}
            ]
        }),
    );
    fixture.write_inbox("atm-dev", "team-lead", "keep me");
    fixture.write_task("atm-dev", 75, json!({"id":"75","status":"stale"}));
    fixture.write_highwatermark("atm-dev", "75\n");

    let backup_dir = fixture.make_backup_dir("atm-dev", "20260407T020304000000000Z");
    fixture.write_json(
        backup_dir.join("config.json"),
        &json!({
            "leadSessionId":"lead-backup",
            "members":[
                {"name":"team-lead","model":"backup-lead","agentType":"lead","cwd":"/backup"},
                {
                    "name":"arch-ctm",
                    "agentId":"arch-ctm@atm-dev",
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
        backup_dir.join("inboxes").join("team-lead.json"),
        "arch-ctm",
        "do not restore",
    );
    fixture.write_inbox_at(
        backup_dir.join("inboxes").join("arch-ctm.json"),
        "team-lead",
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
        "atm-dev",
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

    let config = fixture.read_team_config_value("atm-dev");
    assert_eq!(config["leadSessionId"], "lead-current");
    assert_eq!(config["members"][0]["name"], "team-lead");
    assert_eq!(config["members"][0]["model"], "current-lead");

    let restored = config["members"]
        .as_array()
        .expect("members")
        .iter()
        .find(|member| member["name"] == "arch-ctm")
        .expect("restored member");
    assert_eq!(restored["tmuxPaneId"], "");
    assert!(restored.get("sessionId").is_none());
    assert!(restored.get("activity").is_none());

    let team_lead_inbox =
        fs::read_to_string(fixture.inbox_path("atm-dev", "team-lead")).expect("team-lead inbox");
    assert!(team_lead_inbox.contains("keep me"));
    let restored_inbox =
        fs::read_to_string(fixture.inbox_path("atm-dev", "arch-ctm")).expect("restored inbox");
    assert!(restored_inbox.contains("restore worker inbox"));
    assert_eq!(fixture.read_highwatermark("atm-dev"), "82");
}

#[test]
fn test_restore_sweeps_stale_mailbox_lock_sentinels() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        "atm-dev",
        json!({"leadSessionId":"lead-current","members":[{"name":"team-lead"}]}),
    );
    fixture.write_text(
        fixture
            .team_dir("atm-dev")
            .join("inboxes")
            .join("arch-ctm.json.lock"),
        &u32::MAX.to_string(),
    );

    let backup_dir = fixture.make_backup_dir("atm-dev", "20260407T020304500000000Z");
    fixture.write_json(
        backup_dir.join("config.json"),
        &json!({
            "leadSessionId":"lead-backup",
            "members":[
                {"name":"team-lead"},
                {"name":"arch-ctm","agentType":"general-purpose","model":"sonnet","cwd":"/repo"}
            ]
        }),
    );
    fixture.write_inbox_at(
        backup_dir.join("inboxes").join("arch-ctm.json"),
        "team-lead",
        "restore worker inbox",
    );

    let output = fixture.run(&[
        "teams",
        "restore",
        "atm-dev",
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
            .team_dir("atm-dev")
            .join("inboxes")
            .join("arch-ctm.json.lock")
            .exists()
    );
}

#[test]
fn test_backup_restore_roundtrip_leaves_zero_mailbox_locks() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        "atm-dev",
        json!({"leadSessionId":"lead-1","members":[{"name":"team-lead"},{"name":"arch-ctm"}]}),
    );
    fixture.write_inbox("atm-dev", "arch-ctm", "backup me");
    fixture.write_text(
        fixture
            .team_dir("atm-dev")
            .join("inboxes")
            .join("arch-ctm.json.lock"),
        &u32::MAX.to_string(),
    );

    let backup_output = fixture.run(&["teams", "backup", "atm-dev", "--json"]);
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
        "atm-dev",
        "--from",
        backup_path.as_str(),
        "--json",
    ]);
    assert!(
        restore_output.status.success(),
        "stderr: {}",
        fixture.stderr(&restore_output)
    );

    let lock_files = fs::read_dir(fixture.team_dir("atm-dev").join("inboxes"))
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
        "atm-dev",
        json!({
            "leadSessionId":"lead-current",
            "members":[
                {"name":"team-lead","agentType":"lead","cwd":"/repo"},
                {"name":"existing","agentType":"worker","cwd":"/repo"}
            ]
        }),
    );
    fixture.write_inbox("atm-dev", "existing", "keep existing inbox");

    let backup_dir = fixture.make_backup_dir("atm-dev", "20260407T030405000000000Z");
    fixture.write_json(
        backup_dir.join("config.json"),
        &json!({
            "leadSessionId":"lead-backup",
            "members":[
                {"name":"team-lead","agentType":"lead","cwd":"/backup"},
                {"name":"existing","agentType":"worker","cwd":"/backup"},
                {"name":"arch-ctm","agentType":"general-purpose","cwd":"/repo"}
            ]
        }),
    );
    fixture.write_inbox_at(
        backup_dir.join("inboxes").join("existing.json"),
        "team-lead",
        "do not overwrite existing inbox",
    );
    fixture.write_inbox_at(
        backup_dir.join("inboxes").join("arch-ctm.json"),
        "team-lead",
        "restore new member inbox",
    );

    let output = fixture.run(&[
        "teams",
        "restore",
        "atm-dev",
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
        fs::read_to_string(fixture.inbox_path("atm-dev", "existing")).expect("existing inbox");
    assert!(existing_inbox.contains("keep existing inbox"));
    assert!(!existing_inbox.contains("do not overwrite existing inbox"));

    let restored_inbox =
        fs::read_to_string(fixture.inbox_path("atm-dev", "arch-ctm")).expect("restored inbox");
    assert!(restored_inbox.contains("restore new member inbox"));
}

#[test]
fn test_restore_cleans_preexisting_staging_before_restore() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        "atm-dev",
        json!({"leadSessionId":"lead-current","members":[{"name":"team-lead"}]}),
    );
    fixture.write_text(
        fixture
            .team_dir("atm-dev")
            .join(".restore-staging")
            .join("stale.txt"),
        "stale marker",
    );
    fixture.write_inbox_at(
        fixture
            .team_dir("atm-dev")
            .join(".restore-staging")
            .join("inboxes")
            .join("stale.json"),
        "team-lead",
        "stale inbox content",
    );

    let backup_dir = fixture.make_backup_dir("atm-dev", "20260407T040505000000000Z");
    fixture.write_json(
        backup_dir.join("config.json"),
        &json!({
            "leadSessionId":"lead-backup",
            "members":[
                {"name":"team-lead"},
                {"name":"arch-ctm","agentType":"general-purpose","model":"sonnet","cwd":"/repo"}
            ]
        }),
    );
    fixture.write_inbox_at(
        backup_dir.join("inboxes").join("arch-ctm.json"),
        "team-lead",
        "fresh restored inbox",
    );

    let output = fixture.run(&[
        "teams",
        "restore",
        "atm-dev",
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
            .team_dir("atm-dev")
            .join(".restore-staging")
            .exists()
    );
    assert!(!fixture.inbox_path("atm-dev", "stale").exists());
    let restored_inbox =
        fs::read_to_string(fixture.inbox_path("atm-dev", "arch-ctm")).expect("restored inbox");
    assert!(restored_inbox.contains("fresh restored inbox"));
    assert!(!restored_inbox.contains("stale inbox content"));
}

#[test]
fn test_restore_inbox_staging_failure_preserves_config_and_live_state() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        "atm-dev",
        json!({
            "leadSessionId":"lead-current",
            "members":[{"name":"team-lead"}]
        }),
    );
    fixture.write_task("atm-dev", 7, json!({"id":"7","status":"open"}));
    fixture.write_highwatermark("atm-dev", "7\n");

    let backup_dir = fixture.make_backup_dir("atm-dev", "20260407T040506500000000Z");
    fixture.write_json(
        backup_dir.join("config.json"),
        &json!({
            "leadSessionId":"lead-backup",
            "members":[
                {"name":"team-lead"},
                {"name":"arch-ctm","agentType":"general-purpose","model":"sonnet","cwd":"/repo"}
            ]
        }),
    );
    fixture.write_inbox_at(
        backup_dir.join("inboxes").join("arch-ctm.json"),
        "team-lead",
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
            "atm-dev",
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

    let config = fixture.read_team_config_value("atm-dev");
    let members = config["members"].as_array().expect("members");
    assert_eq!(members.len(), 1);
    assert_eq!(members[0]["name"], "team-lead");
    assert_eq!(config["leadSessionId"], "lead-current");
    assert!(!fixture.inbox_path("atm-dev", "arch-ctm").exists());
    assert!(fixture.tasks_dir("atm-dev").join("7.json").is_file());
    assert!(!fixture.tasks_dir("atm-dev").join("80.json").exists());
    assert_eq!(fixture.read_highwatermark("atm-dev"), "7");
    assert!(
        fixture
            .team_dir("atm-dev")
            .join(".restore-in-progress")
            .is_file()
    );
}

#[test]
fn test_restore_config_failure_leaves_restore_marker_and_rerun_completes() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        "atm-dev",
        json!({"leadSessionId":"lead-current","members":[{"name":"team-lead"}]}),
    );

    let backup_dir = fixture.make_backup_dir("atm-dev", "20260407T040506000000000Z");
    fixture.write_json(
        backup_dir.join("config.json"),
        &json!({
            "leadSessionId":"lead-backup",
            "members":[
                {"name":"team-lead"},
                {"name":"arch-ctm","agentType":"general-purpose","model":"sonnet","cwd":"/repo"}
            ]
        }),
    );
    fixture.write_inbox_at(
        backup_dir.join("inboxes").join("arch-ctm.json"),
        "team-lead",
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
            "atm-dev",
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
            .team_dir("atm-dev")
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
        "atm-dev",
        "--from",
        backup_dir.to_str().expect("utf8"),
        "--json",
    ]);
    assert!(retry.status.success(), "stderr: {}", fixture.stderr(&retry));
    assert!(
        !fixture
            .team_dir("atm-dev")
            .join(".restore-in-progress")
            .exists()
    );
    let config = fixture.read_team_config_value("atm-dev");
    assert!(
        config["members"]
            .as_array()
            .expect("members")
            .iter()
            .any(|member| member["name"] == "arch-ctm")
    );
    assert!(fixture.inbox_path("atm-dev", "arch-ctm").is_file());
}

#[test]
fn test_restore_success_clears_restore_marker() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        "atm-dev",
        json!({"leadSessionId":"lead-current","members":[{"name":"team-lead"}]}),
    );

    let backup_dir = fixture.make_backup_dir("atm-dev", "20260407T050607000000000Z");
    fixture.write_json(
        backup_dir.join("config.json"),
        &json!({
            "leadSessionId":"lead-backup",
            "members":[
                {"name":"team-lead"},
                {"name":"arch-ctm","agentType":"general-purpose","model":"sonnet","cwd":"/repo"}
            ]
        }),
    );
    fixture.write_inbox_at(
        backup_dir.join("inboxes").join("arch-ctm.json"),
        "team-lead",
        "restore worker inbox",
    );

    let output = fixture.run(&[
        "teams",
        "restore",
        "atm-dev",
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
            .team_dir("atm-dev")
            .join(".restore-in-progress")
            .exists()
    );
}

#[test]
fn test_restore_marker_removal_failure_is_warning_only() {
    let fixture = Fixture::new();
    fixture.write_team_config_value(
        "atm-dev",
        json!({"leadSessionId":"lead-current","members":[{"name":"team-lead"}]}),
    );

    let backup_dir = fixture.make_backup_dir("atm-dev", "20260407T050608000000000Z");
    fixture.write_json(
        backup_dir.join("config.json"),
        &json!({
            "leadSessionId":"lead-backup",
            "members":[
                {"name":"team-lead"},
                {"name":"arch-ctm","agentType":"general-purpose","model":"sonnet","cwd":"/repo"}
            ]
        }),
    );
    fixture.write_inbox_at(
        backup_dir.join("inboxes").join("arch-ctm.json"),
        "team-lead",
        "restore worker inbox",
    );

    let output = fixture.run_with_env(
        &[
            "teams",
            "restore",
            "atm-dev",
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

    let config = fixture.read_team_config_value("atm-dev");
    assert!(
        config["members"]
            .as_array()
            .expect("members")
            .iter()
            .any(|member| member["name"] == "arch-ctm")
    );
    assert!(fixture.inbox_path("atm-dev", "arch-ctm").is_file());
    assert!(
        fixture
            .team_dir("atm-dev")
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
            "default_team = \"atm-dev\"\n",
        )
        .expect("config");
        Self { tempdir }
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        self.run_with_env(args, &[])
    }

    fn run_with_env(&self, args: &[&str], extra_env: &[(&str, &str)]) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_atm"));
        command
            .args(args)
            .env("ATM_HOME", self.tempdir.path())
            .env("ATM_CONFIG_HOME", self.tempdir.path())
            .current_dir(self.tempdir.path());
        for (key, value) in extra_env {
            command.env(key, value);
        }
        command.output().expect("run atm")
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
        self.write_inbox_at(self.inbox_path(team, member), "team-lead", text);
    }

    fn write_inbox_at(&self, path: std::path::PathBuf, from: &str, text: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("inbox dir");
        }
        let envelope = MessageEnvelope {
            from: from.to_string(),
            text: text.to_string(),
            timestamp: atm_core::types::IsoTimestamp::from_datetime(Utc::now()),
            read: false,
            source_team: Some("atm-dev".to_string()),
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
