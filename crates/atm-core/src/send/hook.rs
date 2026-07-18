use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Map, Value, json};
#[cfg(test)]
use tracing::Level;
use tracing::{debug, error, info, warn};

use super::{
    POST_SEND_HOOK_TIMEOUT, ResolvedRecipient, WarningEntry, nudge_template,
    qualified_sender_identity,
};
use crate::boundary::{
    BuiltInPostSendDispatch, GraftNudgeTarget, HookExecutionSummary, LocalTmuxNudgeTarget,
    PostSendBuiltInTarget, PostSendEmissionOutcome, PostSendEmissionPath, PostSendHookEmitter,
    PostSendHookEvent, built_in_nudge_template_kind_from_post_send_event,
};
use crate::config::types::HookRecipient;
use crate::config::{self, AtmConfig};
use crate::error::AtmError;
use crate::error_codes::AtmErrorCode;
use crate::protocol::{NotificationEvent, NotificationKind};
use crate::schema::compatible_home_dir;
use crate::service_runtime::{RetainedServiceRuntime, append_notification_log};
use crate::types::{AgentName, TeamName};

mod built_in_dispatch;
mod external;
mod payload;

const POST_SEND_HOOK_MAX_STDOUT_BYTES: usize = 8 * 1024;
const POST_SEND_HOOK_STDOUT_JOIN_TIMEOUT: Duration = Duration::from_millis(500);
use external::run_post_send_hooks_for_cli;
#[cfg(test)]
use external::{
    HookCancellationToken, PostSendHookResultLevel, finish_abandoned_post_send_hook_stdout_capture,
    hook_result_log_level, parse_post_send_hook_result,
};
use payload::{
    hook_matches_recipient, notification_event, post_send_event_from_message, post_send_warning,
    sender_config_root,
};

pub(crate) fn emit_post_send_effects<R>(
    runtime: &R,
    warnings: &mut Vec<WarningEntry>,
    config: Option<&AtmConfig>,
    post_send_emitter: Option<&dyn PostSendHookEmitter>,
    recipient: &ResolvedRecipient,
    delivery_snapshot: &crate::delivery_policy::DeliveryRecipientSnapshot,
    messages: &[crate::delivery_plan::LogicalMessage],
) where
    R: RetainedServiceRuntime + ?Sized,
{
    for message in messages {
        let event = post_send_event_from_message(
            recipient,
            message,
            delivery_snapshot.recipient_pane_id.as_ref(),
        );
        let outcome = emit_post_send_outcome(
            runtime,
            warnings,
            config,
            post_send_emitter,
            delivery_snapshot,
            &event,
        );
        if matches!(outcome, PostSendEmissionOutcome::Delivered { .. })
            && let Err(error) = append_notification_log(&notification_event(&event))
        {
            warnings.push(WarningEntry::with_code(
                error.code,
                format!(
                    "warning: notification delivery failed for {}@{}: {error}",
                    recipient.agent, recipient.team
                ),
                error.primary_recovery().map(str::to_owned),
            ));
        }
    }
}

fn emit_post_send_outcome<R>(
    runtime: &R,
    warnings: &mut Vec<WarningEntry>,
    config: Option<&AtmConfig>,
    post_send_emitter: Option<&dyn PostSendHookEmitter>,
    delivery_snapshot: &crate::delivery_policy::DeliveryRecipientSnapshot,
    event: &PostSendHookEvent,
) -> PostSendEmissionOutcome
where
    R: RetainedServiceRuntime + ?Sized,
{
    let hook_summary = config
        .map(|loaded| run_post_send_hooks_for_cli(warnings, loaded, event))
        .unwrap_or_else(|| HookExecutionSummary::new(0, 0, 0).expect("zero summary"));
    if hook_summary.succeeded_rules() > 0 {
        return PostSendEmissionOutcome::Delivered {
            path: PostSendEmissionPath::ExternalHook,
            hook_summary,
        };
    }
    if hook_summary.matched_rules() > 0 {
        let warning = warnings.last().cloned().unwrap_or_else(|| {
            WarningEntry::with_code(
                AtmErrorCode::WarningHookExecutionFailed,
                format!(
                    "warning: post-send hook execution failed for {}@{} message {}.",
                    event.recipient, event.recipient_team, event.message_id
                ),
                Some(
                    "Inspect the matching post-send hook command output and retry once the hook exits successfully."
                        .to_string(),
                ),
            )
        });
        return PostSendEmissionOutcome::Failed {
            hook_summary,
            warning,
        };
    }
    let Some(dispatch) =
        built_in_dispatch::build_built_in_dispatch(runtime, delivery_snapshot, event)
    else {
        return PostSendEmissionOutcome::NoCapability { hook_summary };
    };
    let Some(post_send_emitter) = post_send_emitter else {
        return PostSendEmissionOutcome::NoCapability { hook_summary };
    };
    match post_send_emitter.emit_post_send(&dispatch) {
        Ok(path) => PostSendEmissionOutcome::Delivered { path, hook_summary },
        Err(error) => {
            let warning = post_send_warning("post-send emission failed", event, &error);
            warnings.push(warning.clone());
            PostSendEmissionOutcome::Failed {
                hook_summary,
                warning,
            }
        }
    }
}

pub(crate) fn load_post_send_config_for_sender<R>(
    runtime: &R,
    sender_team: &TeamName,
    sender: &AgentName,
) -> Result<Option<AtmConfig>, AtmError>
where
    R: crate::service_runtime::RetainedServiceRuntime + ?Sized,
{
    let Some(member) = runtime.load_roster_member(sender_team, sender)? else {
        return Ok(None);
    };
    let Some(config_root) = sender_config_root(&member.metadata_json) else {
        return Ok(None);
    };
    runtime.load_config(&config_root)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use serde_json::{Map, json};
    use tempfile::tempdir;
    use tracing::Level;

    use super::{
        HookCancellationToken, POST_SEND_HOOK_MAX_STDOUT_BYTES, PostSendHookResultLevel,
        emit_post_send_effects, finish_abandoned_post_send_hook_stdout_capture,
        hook_matches_recipient, hook_result_log_level, load_post_send_config_for_sender,
        parse_post_send_hook_result, sender_config_root,
    };
    use crate::boundary::{
        self, BuiltInNudgeTemplateKind, BuiltInPostSendDispatch, GraftNudgeTarget,
        PostSendBuiltInTarget, PostSendEmissionPath, PostSendHookEmitter, RosterEntry,
        RosterHarness, RosterMemberKind, TeamNudgeTemplateOverrideMode,
        TeamNudgeTemplateOverrideRow,
    };
    use crate::config::AtmConfig;
    use crate::config::types::{HookRecipient, PostSendHookRule};
    use crate::delivery_plan::LogicalMessage;
    use crate::delivery_policy::{DeliveryHarnessPath, DeliveryRecipientSnapshot};
    use crate::error::AtmError;
    use crate::error_codes::AtmErrorCode;
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::schema::AckIntentFields;
    #[allow(
        deprecated,
        reason = "Phase AD obsolete: derived compatibility field only. Hook tests intentionally exercise the retained legacy cwd compatibility seam."
    )]
    use crate::schema::agent_member::LEGACY_CWD_METADATA_KEY;
    use crate::schema::{AtmMessageId, HOME_DIR_METADATA_KEY, InboxMessage};
    use crate::send::ResolvedRecipient;
    use crate::service_runtime::RetainedServiceRuntime;
    use crate::test_support::{EnvGuard, TEST_SENDER};
    use crate::types::{AgentName, IsoTimestamp, PaneId, TeamName};

    struct ConfigLookupRuntime {
        roster_entry: Option<RosterEntry>,
        config_lookup_root: PathBuf,
        config: Option<AtmConfig>,
    }

    impl crate::boundary::sealed::Sealed for ConfigLookupRuntime {}

    impl RetainedServiceRuntime for ConfigLookupRuntime {
        fn load_config(&self, current_dir: &Path) -> Result<Option<AtmConfig>, AtmError> {
            Ok((current_dir == self.config_lookup_root)
                .then_some(self.config.clone())
                .flatten())
        }

        fn inbox_path(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<PathBuf, AtmError> {
            unreachable!("config lookup test does not resolve inbox paths")
        }

        fn load_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<Option<IsoTimestamp>, AtmError> {
            Ok(None)
        }

        fn save_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _timestamp: IsoTimestamp,
        ) -> Result<(), AtmError> {
            Ok(())
        }

        fn deliver_non_claude_payloads(
            &self,
            _recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
            _messages: &[InboxMessage],
        ) -> Result<(), AtmError> {
            unreachable!("config lookup test does not deliver outbound payloads")
        }

        fn load_roster_member(
            &self,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<Option<RosterEntry>, AtmError> {
            Ok(self.roster_entry.clone())
        }

        fn load_team_roster(&self, _team: &TeamName) -> Result<Vec<RosterEntry>, AtmError> {
            Ok(Vec::new())
        }
    }

    struct HookEmissionRuntime {
        override_row: Option<TeamNudgeTemplateOverrideRow>,
    }

    impl HookEmissionRuntime {
        fn new(override_row: Option<TeamNudgeTemplateOverrideRow>) -> Self {
            Self { override_row }
        }
    }

    impl crate::boundary::sealed::Sealed for HookEmissionRuntime {}

    impl RetainedServiceRuntime for HookEmissionRuntime {
        fn load_config(&self, _current_dir: &Path) -> Result<Option<AtmConfig>, AtmError> {
            Ok(None)
        }

        fn load_nudge_template_override(
            &self,
            _team: &TeamName,
            _kind: BuiltInNudgeTemplateKind,
        ) -> Result<Option<TeamNudgeTemplateOverrideRow>, AtmError> {
            Ok(self.override_row.clone())
        }

        fn inbox_path(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<PathBuf, AtmError> {
            unreachable!("hook emission test does not resolve inbox paths")
        }

        fn load_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<Option<IsoTimestamp>, AtmError> {
            Ok(None)
        }

        fn save_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _timestamp: IsoTimestamp,
        ) -> Result<(), AtmError> {
            Ok(())
        }

        fn deliver_non_claude_payloads(
            &self,
            _recipient: &DeliveryRecipientSnapshot,
            _messages: &[InboxMessage],
        ) -> Result<(), AtmError> {
            unreachable!("hook emission test does not deliver non-claude payloads")
        }

        fn load_roster_member(
            &self,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<Option<RosterEntry>, AtmError> {
            Ok(None)
        }

        fn load_team_roster(&self, _team: &TeamName) -> Result<Vec<RosterEntry>, AtmError> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct RecordingEmitter {
        emitted: Mutex<Vec<BuiltInPostSendDispatch>>,
    }

    impl RecordingEmitter {
        fn emitted(&self) -> Vec<BuiltInPostSendDispatch> {
            self.emitted.lock().expect("emitter lock").clone()
        }
    }

    impl boundary::sealed::Sealed for RecordingEmitter {}

    impl PostSendHookEmitter for RecordingEmitter {
        fn emit_post_send(
            &self,
            dispatch: &BuiltInPostSendDispatch,
        ) -> Result<PostSendEmissionPath, AtmError> {
            self.emitted
                .lock()
                .expect("emitter lock")
                .push(dispatch.clone());
            Ok(match dispatch.target {
                PostSendBuiltInTarget::LocalTmux(_) => PostSendEmissionPath::LocalTmux,
                PostSendBuiltInTarget::Graft(_) => PostSendEmissionPath::GraftPort,
            })
        }
    }

    #[test]
    fn hook_matches_recipient_exact_and_wildcard_values() {
        assert!(hook_matches_recipient(
            &HookRecipient::Named(TEST_SENDER.parse().expect("recipient")),
            &TEST_SENDER.parse().expect("candidate")
        ));
        assert!(hook_matches_recipient(
            &HookRecipient::Wildcard,
            &TEST_SENDER.parse().expect("candidate")
        ));
        assert!(!hook_matches_recipient(
            &HookRecipient::Named(ROLE_TEAM_LEAD.parse().expect("recipient")),
            &TEST_SENDER.parse().expect("candidate")
        ));
    }

    #[test]
    fn parse_post_send_hook_result_accepts_valid_json_object() {
        let parsed = parse_post_send_hook_result(
            Path::new("hook"),
            br#"{"level":"debug","message":"nudged","fields":{"pane_id":"%42"}}"#,
        )
        .expect("valid hook result");

        assert_eq!(parsed.message, "nudged");
        assert_eq!(parsed.fields["pane_id"], json!("%42"));
    }

    #[test]
    fn parse_post_send_hook_result_ignores_invalid_schema() {
        let parsed =
            parse_post_send_hook_result(Path::new("hook"), br#"{"level":"trace","message":"x"}"#);

        assert!(parsed.is_none());
    }

    #[test]
    fn parse_post_send_hook_result_ignores_oversized_stdout() {
        let oversized = vec![b'a'; POST_SEND_HOOK_MAX_STDOUT_BYTES + 1];
        let parsed = parse_post_send_hook_result(Path::new("hook"), &oversized);

        assert!(parsed.is_none());
    }

    #[test]
    fn error_hook_results_map_to_error_level() {
        assert_eq!(
            hook_result_log_level(PostSendHookResultLevel::Error),
            Level::ERROR
        );
    }

    #[test]
    fn hook_cancellation_token_tracks_cancelled_state() {
        let token = HookCancellationToken::default();
        assert!(!token.is_cancelled());

        token.cancel();

        assert!(token.is_cancelled());
    }

    #[test]
    fn bounded_stdout_teardown_returns_promptly_for_completed_reader() {
        let handle = std::thread::spawn(|| Ok::<Vec<u8>, std::io::Error>(Vec::new()));
        finish_abandoned_post_send_hook_stdout_capture(Some(handle), Path::new("hook"));
    }

    #[test]
    #[allow(
        deprecated,
        reason = "Phase AD obsolete: test fixture intentionally exercises the retained legacy cwd compatibility fallback."
    )]
    fn sender_config_root_prefers_home_dir_and_falls_back_to_cwd() {
        let home_dir_metadata =
            Map::from_iter([(HOME_DIR_METADATA_KEY.to_string(), json!("/repo/home"))]);
        assert_eq!(
            sender_config_root(&home_dir_metadata),
            Some(PathBuf::from("/repo/home"))
        );

        let cwd_only_metadata =
            Map::from_iter([(LEGACY_CWD_METADATA_KEY.to_string(), json!("/repo/cwd"))]);
        assert_eq!(
            sender_config_root(&cwd_only_metadata),
            Some(PathBuf::from("/repo/cwd"))
        );
    }

    #[test]
    #[allow(
        deprecated,
        reason = "Phase AD obsolete: test fixture intentionally seeds legacy cwd metadata to verify the bounded compatibility read."
    )]
    fn load_post_send_config_uses_sender_roster_metadata_not_caller_cwd() {
        let config_root = PathBuf::from("/repo/home");
        let runtime = ConfigLookupRuntime {
            roster_entry: Some(RosterEntry {
                team_name: TeamName::from_validated("test-team"),
                agent_name: AgentName::from_validated(TEST_SENDER),
                member_kind: RosterMemberKind::Permanent,
                harness: RosterHarness::ClaudeCode,
                agent_type: crate::schema::AgentType::default(),
                model: crate::types::ModelName::default(),
                recipient_pane_id: None,
                metadata_json: Map::from_iter([(
                    LEGACY_CWD_METADATA_KEY.to_string(),
                    json!(config_root.display().to_string()),
                )]),
            }),
            config_lookup_root: config_root.clone(),
            config: Some(AtmConfig {
                config_root: config_root.clone(),
                ..Default::default()
            }),
        };

        let loaded = load_post_send_config_for_sender(
            &runtime,
            &TeamName::from_validated("test-team"),
            &AgentName::from_validated(TEST_SENDER),
        )
        .expect("config lookup");

        assert_eq!(
            loaded.as_ref().map(|config| &config.config_root),
            Some(&config_root)
        );
    }

    fn logical_message(text: &str) -> LogicalMessage {
        let ack_intent = AckIntentFields::not_required();
        LogicalMessage::new(
            InboxMessage {
                from: AgentName::from_validated(TEST_SENDER),
                text: text.to_string(),
                timestamp: IsoTimestamp::now(),
                read: false,
                source_team: Some(TeamName::from_validated("test-team")),
                summary: Some(text.to_string()),
                message_id: Some(AtmMessageId::new()),
                requires_ack: ack_intent.requires_ack,
                pending_ack_at: ack_intent.pending_ack_at,
                acknowledged_at: ack_intent.acknowledged_at,
                acknowledges_message_id: None,
                parent_message_id: None,
                thread_mode: None,
                expires_at: None,
                task_id: None,
                extra: Map::new(),
            },
            false,
            false,
        )
        .expect("logical message")
    }

    fn install_test_home(home_dir: &Path) -> EnvGuard {
        EnvGuard::set_many([
            ("HOME", home_dir.to_str()),
            ("USERPROFILE", None),
            ("ATM_LOG_DIR", None),
        ])
    }

    fn read_notification_events(_home_dir: &Path) -> Vec<crate::protocol::NotificationEvent> {
        let notification_path = crate::home::host_runtime_dir()
            .expect("host runtime dir")
            .join("notifications.jsonl");
        match fs::read_to_string(notification_path) {
            Ok(contents) => contents
                .lines()
                .map(|line| serde_json::from_str(line).expect("notification event"))
                .collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => panic!("failed to read notification log: {error}"),
        }
    }

    #[test]
    #[serial_test::serial(env)]
    fn built_in_fallback_dispatches_local_tmux_through_emitter() {
        let tempdir = tempdir().expect("tempdir");
        let _env = install_test_home(tempdir.path());
        let runtime = HookEmissionRuntime::new(None);
        let emitter = RecordingEmitter::default();
        let recipient = ResolvedRecipient {
            agent: AgentName::from_validated("recipient"),
            team: TeamName::from_validated("test-team"),
        };
        let snapshot = DeliveryRecipientSnapshot {
            agent: recipient.agent.clone(),
            team: recipient.team.clone(),
            remote_host: None,
            harness: DeliveryHarnessPath::ClaudeCode,
            recipient_pane_id: Some(PaneId::from_cli("%9").expect("pane")),
            local_tmux_post_send: true,
            graft_post_send: false,
            roster_backed: true,
        };
        let mut warnings = Vec::new();

        emit_post_send_effects(
            &runtime,
            &mut warnings,
            None,
            Some(&emitter),
            &recipient,
            &snapshot,
            &[logical_message("hello")],
        );

        assert!(warnings.is_empty());
        let emitted = emitter.emitted();
        assert_eq!(emitted.len(), 1);
        let dispatch = &emitted[0];
        match &dispatch.target {
            PostSendBuiltInTarget::LocalTmux(target) => {
                assert_eq!(target.pane_id, PaneId::from_cli("%9").expect("pane"));
                assert!(target.rendered_nudge.contains("read atm --team test-team"));
                assert!(
                    target
                        .rendered_nudge
                        .contains(&dispatch.event.message_id.to_string())
                );
                assert!(target.rendered_nudge.contains("hello"));
            }
            other => panic!("expected local tmux dispatch, got {other:?}"),
        }

        let notifications = read_notification_events(tempdir.path());
        assert!(!notifications.is_empty());
    }

    #[test]
    #[serial_test::serial(env)]
    fn external_post_send_hook_takes_precedence_over_built_in_nudge() {
        let tempdir = tempdir().expect("tempdir");
        let hook_capture = tempdir.path().join("hook-capture.txt");
        #[cfg(windows)]
        let hook_path = tempdir.path().join("hook.cmd");
        #[cfg(not(windows))]
        let hook_path = tempdir.path().join("hook");
        #[cfg(windows)]
        fs::write(
            &hook_path,
            "@echo off\r\nsetlocal EnableDelayedExpansion\r\n> \"%ATM_TEST_HOOK_CAPTURE%\" echo !ATM_POST_SEND!\r\nexit /b 0\r\n",
        )
        .expect("write hook shim");
        #[cfg(not(windows))]
        fs::write(
            &hook_path,
            "#!/bin/sh\nprintf '%s\\n' \"$ATM_POST_SEND\" > \"$ATM_TEST_HOOK_CAPTURE\"\nexit 0\n",
        )
        .expect("write hook shim");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&hook_path).expect("metadata").permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&hook_path, perms).expect("chmod");
        }

        let hook_capture_value = hook_capture.display().to_string();
        let _env = EnvGuard::set_many([
            ("ATM_TEST_HOOK_CAPTURE", Some(hook_capture_value.as_str())),
            ("ATM_HOME", tempdir.path().to_str()),
            ("ATM_CONFIG_HOME", tempdir.path().to_str()),
            ("HOME", tempdir.path().to_str()),
        ]);

        let config = AtmConfig {
            config_root: tempdir.path().to_path_buf(),
            post_send_hooks: vec![PostSendHookRule {
                recipient: HookRecipient::Named("recipient".parse().expect("recipient")),
                command: vec![hook_path.display().to_string()],
            }],
            ..Default::default()
        };
        let recipient = ResolvedRecipient {
            agent: AgentName::from_validated("recipient"),
            team: TeamName::from_validated("test-team"),
        };
        let snapshot = DeliveryRecipientSnapshot {
            agent: recipient.agent.clone(),
            team: recipient.team.clone(),
            remote_host: None,
            harness: DeliveryHarnessPath::ClaudeCode,
            recipient_pane_id: Some(PaneId::from_cli("%9").expect("pane")),
            local_tmux_post_send: true,
            graft_post_send: false,
            roster_backed: true,
        };
        let mut warnings = Vec::new();
        let runtime = HookEmissionRuntime::new(None);
        let emitter = RecordingEmitter::default();

        emit_post_send_effects(
            &runtime,
            &mut warnings,
            Some(&config),
            Some(&emitter),
            &recipient,
            &snapshot,
            &[logical_message("hello")],
        );

        let captured = fs::read_to_string(&hook_capture).expect("hook capture");
        assert!(captured.contains("\"description\":\"hello\""));
        assert!(emitter.emitted().is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    #[serial_test::serial(env)]
    fn mixed_success_hook_accounting_preserves_delivery_and_warning() {
        let tempdir = tempdir().expect("tempdir");
        let hook_capture = tempdir.path().join("hook-capture.txt");
        #[cfg(windows)]
        let hook_ok = tempdir.path().join("hook-ok.cmd");
        #[cfg(not(windows))]
        let hook_ok = tempdir.path().join("hook-ok");
        #[cfg(windows)]
        let hook_fail = tempdir.path().join("hook-fail.cmd");
        #[cfg(not(windows))]
        let hook_fail = tempdir.path().join("hook-fail");
        #[cfg(windows)]
        fs::write(
            &hook_ok,
            "@echo off\r\nsetlocal EnableDelayedExpansion\r\n> \"%ATM_TEST_HOOK_CAPTURE%\" echo !ATM_POST_SEND!\r\nexit /b 0\r\n",
        )
        .expect("write ok hook");
        #[cfg(not(windows))]
        fs::write(
            &hook_ok,
            "#!/bin/sh\nprintf '%s\\n' \"$ATM_POST_SEND\" > \"$ATM_TEST_HOOK_CAPTURE\"\nexit 0\n",
        )
        .expect("write ok hook");
        #[cfg(windows)]
        fs::write(&hook_fail, "@echo off\r\nexit /b 7\r\n").expect("write failing hook");
        #[cfg(not(windows))]
        fs::write(&hook_fail, "#!/bin/sh\nexit 7\n").expect("write failing hook");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in [&hook_ok, &hook_fail] {
                let mut perms = fs::metadata(path).expect("metadata").permissions();
                perms.set_mode(0o755);
                fs::set_permissions(path, perms).expect("chmod");
            }
        }

        let hook_capture_value = hook_capture.display().to_string();
        let _env = EnvGuard::set_many([
            ("ATM_TEST_HOOK_CAPTURE", Some(hook_capture_value.as_str())),
            ("ATM_HOME", tempdir.path().to_str()),
            ("ATM_CONFIG_HOME", tempdir.path().to_str()),
            ("HOME", tempdir.path().to_str()),
        ]);

        let config = AtmConfig {
            config_root: tempdir.path().to_path_buf(),
            post_send_hooks: vec![
                PostSendHookRule {
                    recipient: HookRecipient::Named("recipient".parse().expect("recipient")),
                    command: vec![hook_ok.display().to_string()],
                },
                PostSendHookRule {
                    recipient: HookRecipient::Named("recipient".parse().expect("recipient")),
                    command: vec![hook_fail.display().to_string()],
                },
            ],
            ..Default::default()
        };
        let recipient = ResolvedRecipient {
            agent: AgentName::from_validated("recipient"),
            team: TeamName::from_validated("test-team"),
        };
        let snapshot = DeliveryRecipientSnapshot {
            agent: recipient.agent.clone(),
            team: recipient.team.clone(),
            remote_host: None,
            harness: DeliveryHarnessPath::ClaudeCode,
            recipient_pane_id: Some(PaneId::from_cli("%9").expect("pane")),
            local_tmux_post_send: true,
            graft_post_send: false,
            roster_backed: true,
        };
        let runtime = HookEmissionRuntime::new(None);
        let emitter = RecordingEmitter::default();
        let mut warnings = Vec::new();

        emit_post_send_effects(
            &runtime,
            &mut warnings,
            Some(&config),
            Some(&emitter),
            &recipient,
            &snapshot,
            &[logical_message("hello")],
        );

        assert_eq!(emitter.emitted().len(), 0);
        assert_eq!(warnings.len(), 1);
        assert_eq!(
            warnings[0].code,
            Some(AtmErrorCode::WarningHookExecutionFailed)
        );
        let notifications = read_notification_events(tempdir.path());
        assert!(!notifications.is_empty());
    }

    #[test]
    #[serial_test::serial(env)]
    fn graft_fallback_dispatches_through_emitter_without_tmux_fields() {
        let tempdir = tempdir().expect("tempdir");
        let _env = install_test_home(tempdir.path());
        let runtime = HookEmissionRuntime::new(Some(TeamNudgeTemplateOverrideRow {
            team_name: TeamName::from_validated("test-team"),
            kind: BuiltInNudgeTemplateKind::Delivery,
            mode: TeamNudgeTemplateOverrideMode::Override {
                template_body: "<ignored/>".to_string(),
            },
            updated_at: IsoTimestamp::now(),
        }));
        let emitter = RecordingEmitter::default();
        let recipient = ResolvedRecipient {
            agent: AgentName::from_validated("recipient"),
            team: TeamName::from_validated("test-team"),
        };
        let snapshot = DeliveryRecipientSnapshot {
            agent: recipient.agent.clone(),
            team: recipient.team.clone(),
            remote_host: None,
            harness: DeliveryHarnessPath::NonClaude,
            recipient_pane_id: None,
            local_tmux_post_send: false,
            graft_post_send: true,
            roster_backed: true,
        };
        let mut warnings = Vec::new();

        emit_post_send_effects(
            &runtime,
            &mut warnings,
            None,
            Some(&emitter),
            &recipient,
            &snapshot,
            &[logical_message("hello")],
        );

        assert!(warnings.is_empty());
        let emitted = emitter.emitted();
        assert_eq!(emitted.len(), 1);
        match &emitted[0].target {
            PostSendBuiltInTarget::Graft(GraftNudgeTarget {
                recipient,
                recipient_team,
            }) => {
                assert_eq!(recipient, &AgentName::from_validated("recipient"));
                assert_eq!(recipient_team, &TeamName::from_validated("test-team"));
            }
            other => panic!("expected graft dispatch, got {other:?}"),
        }
    }
}
