//! Hidden daemon-side ingress/export helper layer used by concrete boundary
//! adapters.

use std::collections::HashSet;
use std::path::Path;

use crate::boundary::{
    ClaudeCompatibilityDeliveryMode, ConfigLoadRequest, ConfigLoadResponse,
    InboxExportAppendMessageSetRequest, InboxExportAppendMessageSetResponse,
    InboxExportRecordRequest, InboxExportRecordResponse, InboxExportReexportMessageRequest,
    InboxExportReexportMessageResponse, InboxIngressDiagnosticsRequest,
    InboxIngressDiagnosticsResponse, InboxIngressIdentityFingerprintRequest,
    InboxIngressIdentityFingerprintResponse, InboxIngressImportRequest, InboxIngressImportResponse,
    InboxSourceFileRecord, ReplaySource, RosterStore, RosterStoreLoadRosterRequest,
    RosterStoreReplaceRosterRequest,
};
use crate::config;
use crate::error::AtmError;
use crate::home;
use crate::mailbox;
use crate::mailbox::source::SourceFile;
use crate::types::TeamName;

fn to_boundary_source_file(source: SourceFile) -> InboxSourceFileRecord {
    InboxSourceFileRecord {
        path: source.path,
        messages: source.messages,
    }
}

fn from_boundary_source_file(source: InboxSourceFileRecord) -> SourceFile {
    SourceFile {
        path: source.path,
        messages: source.messages,
    }
}

fn replay_source_static(label: &'static str) -> ReplaySource {
    ReplaySource::new(label).unwrap_or_else(|_| unreachable!("static replay source must validate"))
}

pub(crate) fn load_workspace_config(
    request: ConfigLoadRequest,
) -> Result<ConfigLoadResponse, AtmError> {
    Ok(ConfigLoadResponse {
        config: config::load_config(&request.current_dir).map_err(|error| {
            AtmError::config(format!(
                "daemon ConfigIngress could not load workspace config from {}",
                request.current_dir.display()
            ))
            .with_recovery(
                "Fix the workspace ATM configuration or current-directory selection before retrying daemon config ingress.",
            )
            .with_source(error)
        })?,
    })
}

pub(crate) fn hydrate_roster_from_team_config_once_at_startup_if_empty(
    home_dir: &Path,
    team: &TeamName,
    roster_store: &dyn RosterStore,
) -> Result<bool, AtmError> {
    let existing = roster_store
        .load_roster(RosterStoreLoadRosterRequest { team: team.clone() })
        .map_err(|error| {
            AtmError::daemon_unavailable(format!(
                "daemon startup could not query canonical ATM roster state for team {} before config hydration",
                team
            ))
            .with_recovery(
                "Repair the ATM roster store before retrying startup-only team roster hydration.",
            )
            .with_source(error)
        })?;
    if !existing.members.is_empty() {
        return Ok(false);
    }

    let team_dir = home::team_dir_from_home(home_dir, team).map_err(|error| {
        AtmError::daemon_unavailable(format!(
            "daemon startup could not resolve team {} from {} for one-shot config hydration",
            team,
            home_dir.display()
        ))
        .with_recovery(
            "Verify the ATM home directory and team roster layout before retrying startup-only team roster hydration.",
        )
        .with_source(error)
    })?;
    let team_config = match config::load_team_config(&team_dir) {
        Ok(team_config) => team_config,
        Err(error) if error.is_missing_document() => return Ok(false),
        Err(error) => {
            return Err(AtmError::daemon_unavailable(format!(
                "daemon startup could not load team config from {} for one-shot roster hydration",
                team_dir.display()
            ))
            .with_recovery(
                "Fix the team ATM configuration before retrying startup-only roster hydration.",
            )
            .with_source(error));
        }
    };
    if team_config.members.is_empty() {
        return Ok(false);
    }

    let members = team_config
        .members
        .into_iter()
        .map(|member| {
            crate::boundary::RosterMemberRecord::from_claude_code_member(team.clone(), member)
        })
        .collect::<Vec<_>>();
    roster_store
        .replace_roster(RosterStoreReplaceRosterRequest {
            team: team.clone(),
            members,
            source: Some(replay_source_static("daemon-startup-config-hydration")),
        })
        .map_err(|error| {
            AtmError::daemon_unavailable(format!(
                "daemon startup could not hydrate canonical ATM roster state from {}",
                team_dir.display()
            ))
            .with_recovery(
                "Repair the ATM roster store or team config before retrying startup-only roster hydration.",
            )
            .with_source(error)
        })?;
    Ok(true)
}

pub(crate) fn import_inbox_source(
    request: InboxIngressImportRequest,
) -> Result<InboxIngressImportResponse, AtmError> {
    let source_files =
        mailbox::import_source_projections(&request.home_dir, &request.team, &request.agent)
            .map_err(|error| {
                AtmError::daemon_unavailable(format!(
                    "daemon inbox ingress could not import source projections for {}@{} from {}",
                    request.agent,
                    request.team,
                    request.home_dir.display()
                ))
                .with_recovery(
                    "Fix the team inbox source files or ATM home selection before retrying daemon inbox ingestion.",
                )
                .with_source(error)
            })?;
    Ok(InboxIngressImportResponse {
        source_files: source_files
            .into_iter()
            .map(to_boundary_source_file)
            .collect(),
    })
}

pub(crate) fn compute_identity_fingerprint(
    request: InboxIngressIdentityFingerprintRequest,
) -> InboxIngressIdentityFingerprintResponse {
    let fingerprint = request
        .message
        .message_id
        .map(|message_id| message_id.to_string())
        .or_else(|| {
            Some(format!(
                "{}:{}",
                request.message.from,
                request.message.timestamp.into_inner().to_rfc3339()
            ))
        });
    InboxIngressIdentityFingerprintResponse { fingerprint }
}

pub(crate) fn report_inbox_diagnostics(
    request: InboxIngressDiagnosticsRequest,
) -> InboxIngressDiagnosticsResponse {
    let mut seen = HashSet::new();
    let mut duplicate_message_ids = 0usize;
    let mut messages_without_ids = 0usize;

    for source in request.source_files {
        for message in source.messages {
            if let Some(message_id) = message.message_id {
                if !seen.insert(message_id) {
                    duplicate_message_ids += 1;
                }
            } else {
                messages_without_ids += 1;
            }
        }
    }

    InboxIngressDiagnosticsResponse {
        duplicate_message_ids,
        messages_without_ids,
    }
}

pub(crate) fn export_source_files(
    request: InboxExportRecordRequest,
) -> Result<InboxExportRecordResponse, AtmError> {
    let committed_paths = request.source_files.len();
    let source_files = request
        .source_files
        .into_iter()
        .map(from_boundary_source_file)
        .collect::<Vec<_>>();
    mailbox::export_compat_source_projections(&source_files).map_err(|error| {
        AtmError::daemon_unavailable(format!(
            "daemon inbox export could not commit {} source projection path(s)",
            committed_paths
        ))
        .with_recovery(
            "Fix the destination inbox projection files or ATM home permissions before retrying daemon inbox export.",
        )
        .with_source(error)
    })?;
    Ok(InboxExportRecordResponse { committed_paths })
}

pub(crate) fn reexport_messages(
    request: InboxExportReexportMessageRequest,
) -> Result<InboxExportReexportMessageResponse, AtmError> {
    // This seam is rebuild-only after Yb Y.10. Runtime send/ack delivery must
    // not route through full mailbox rewrite.
    let wrote_messages = request.messages.len();
    mailbox::export_compat_mailbox_projection(&request.path, &request.messages).map_err(
        |error| {
            AtmError::daemon_unavailable(format!(
                "daemon inbox export could not rewrite mailbox projection {}",
                request.path.display()
            ))
            .with_recovery(
                "Fix the destination mailbox projection path or file permissions before retrying daemon message re-export.",
            )
            .with_source(error)
        },
    )?;
    Ok(InboxExportReexportMessageResponse { wrote_messages })
}

pub(crate) fn append_message_set(
    request: InboxExportAppendMessageSetRequest,
) -> Result<InboxExportAppendMessageSetResponse, AtmError> {
    let wrote_messages = request.messages.len();
    match request.mode {
        ClaudeCompatibilityDeliveryMode::RecoveredLogicalMessageSet => {
            let export_policy = mailbox::store::export_policy_for_path(&request.path).map_err(
                |error| {
                    AtmError::daemon_unavailable(format!(
                        "daemon inbox export could not resolve recovered export policy for {}",
                        request.path.display()
                    ))
                    .with_recovery(
                        "Fix the ATM config beside the destination mailbox projection before retrying recovered Claude compatibility delivery.",
                    )
                    .with_source(error)
                },
            )?;
            mailbox::store::append_compat_mailbox_message_set(
                &request.path,
                export_policy,
                &request.messages,
            )
            .map_err(|error| {
                AtmError::daemon_unavailable(format!(
                    "daemon inbox export could not materialize recovered logical message set for {}",
                    request.path.display()
                ))
                .with_recovery(
                    "Fix the destination mailbox projection path or file permissions before retrying recovered Claude compatibility delivery.",
                )
                .with_source(error)
            })?;
        }
    }
    Ok(InboxExportAppendMessageSetResponse { wrote_messages })
}

#[cfg(test)]
mod tests {
    use super::hydrate_roster_from_team_config_once_at_startup_if_empty;
    use crate::boundary::sealed::Sealed;
    use crate::boundary::{
        self, RosterStore, RosterStoreHealthSnapshot, RosterStoreHealthSnapshotRequest,
        RosterStoreHealthSnapshotResponse, RosterStoreListTeamsRequest,
        RosterStoreListTeamsResponse, RosterStoreLoadRosterRequest, RosterStoreLoadRosterResponse,
        RosterStoreQueryMembershipRequest, RosterStoreQueryMembershipResponse,
        RosterStoreReplaceRosterRequest, RosterStoreReplaceRosterResponse,
    };
    use crate::schema::AgentMember;
    use crate::types::{AgentName, TeamName};
    use std::collections::BTreeSet;
    use std::sync::Mutex;
    use tempfile::TempDir;

    #[derive(Debug, Default)]
    struct TestRosterStore {
        state: Mutex<TestRosterState>,
    }

    #[derive(Debug, Default)]
    struct TestRosterState {
        members: Vec<boundary::RosterMemberRecord>,
        replace_calls: usize,
    }

    impl TestRosterStore {
        fn members(&self) -> Vec<boundary::RosterMemberRecord> {
            self.state.lock().expect("roster state").members.clone()
        }

        fn replace_calls(&self) -> usize {
            self.state.lock().expect("roster state").replace_calls
        }
    }

    impl Sealed for TestRosterStore {}

    impl RosterStore for TestRosterStore {
        fn replace_roster(
            &self,
            request: RosterStoreReplaceRosterRequest,
        ) -> Result<RosterStoreReplaceRosterResponse, crate::error::AtmError> {
            let mut state = self.state.lock().expect("roster state");
            let previous_member_count = state.members.len() as u64;
            state.members = request.members;
            state.replace_calls += 1;
            Ok(RosterStoreReplaceRosterResponse {
                team: request.team,
                previous_member_count,
                current_member_count: state.members.len() as u64,
                replaced: true,
            })
        }

        fn load_roster(
            &self,
            request: RosterStoreLoadRosterRequest,
        ) -> Result<RosterStoreLoadRosterResponse, crate::error::AtmError> {
            let state = self.state.lock().expect("roster state");
            Ok(RosterStoreLoadRosterResponse {
                team: request.team,
                members: state.members.clone(),
            })
        }

        fn query_membership(
            &self,
            request: RosterStoreQueryMembershipRequest,
        ) -> Result<RosterStoreQueryMembershipResponse, crate::error::AtmError> {
            let state = self.state.lock().expect("roster state");
            let member = state
                .members
                .iter()
                .find(|record| record.agent_name == request.member)
                .cloned();
            Ok(RosterStoreQueryMembershipResponse {
                team: request.team,
                is_member: member.is_some(),
                member,
            })
        }

        fn list_teams(
            &self,
            _request: RosterStoreListTeamsRequest,
        ) -> Result<RosterStoreListTeamsResponse, crate::error::AtmError> {
            let state = self.state.lock().expect("roster state");
            let teams = state
                .members
                .iter()
                .map(|record| record.team_name.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            Ok(RosterStoreListTeamsResponse { teams })
        }

        fn health_snapshot(
            &self,
            request: RosterStoreHealthSnapshotRequest,
        ) -> Result<RosterStoreHealthSnapshotResponse, crate::error::AtmError> {
            let state = self.state.lock().expect("roster state");
            Ok(RosterStoreHealthSnapshotResponse {
                snapshot: RosterStoreHealthSnapshot {
                    team: request.team,
                    member_count: state.members.len() as u64,
                    stale: false,
                    refreshed_at: None,
                },
            })
        }
    }

    fn write_team_config(home_dir: &std::path::Path, team: &TeamName, members: &[AgentName]) {
        let team_dir = crate::home::team_dir_from_home(home_dir, team).expect("team dir");
        std::fs::create_dir_all(&team_dir).expect("team dir");
        let config = crate::schema::TeamConfig {
            members: members
                .iter()
                .cloned()
                .map(AgentMember::with_name)
                .collect(),
            extra: serde_json::Map::new(),
        };
        std::fs::write(
            team_dir.join("config.json"),
            serde_json::to_vec(&config).expect("config"),
        )
        .expect("write config");
    }

    #[test]
    fn startup_hydration_seeds_empty_roster_from_team_config() {
        let tempdir = TempDir::new().expect("tempdir");
        let team: TeamName = "startup-team".parse().expect("team");
        let sender: AgentName = "sender".parse().expect("agent");
        let lead: AgentName = "team-lead".parse().expect("agent");
        write_team_config(tempdir.path(), &team, &[sender.clone(), lead.clone()]);
        let roster_store = TestRosterStore::default();

        let hydrated = hydrate_roster_from_team_config_once_at_startup_if_empty(
            tempdir.path(),
            &team,
            &roster_store,
        )
        .expect("hydrate roster");

        assert!(hydrated);
        assert_eq!(roster_store.replace_calls(), 1);
        let members = roster_store.members();
        assert_eq!(members.len(), 2);
        assert!(members.iter().any(|record| record.agent_name == sender));
        assert!(members.iter().any(|record| record.agent_name == lead));
    }

    #[test]
    fn startup_hydration_noops_when_roster_is_non_empty() {
        let tempdir = TempDir::new().expect("tempdir");
        let team: TeamName = "startup-team".parse().expect("team");
        let sender: AgentName = "sender".parse().expect("agent");
        write_team_config(tempdir.path(), &team, std::slice::from_ref(&sender));
        let roster_store = TestRosterStore::default();
        roster_store
            .replace_roster(RosterStoreReplaceRosterRequest {
                team: team.clone(),
                members: vec![boundary::RosterMemberRecord {
                    team_name: team.clone(),
                    agent_name: sender.clone(),
                    member_kind: boundary::RosterMemberKind::Permanent,
                    harness: boundary::RosterHarness::ClaudeCode,
                    agent_type: String::new(),
                    model: String::new(),
                    recipient_pane_id: None,
                    metadata_json: serde_json::Map::new(),
                }],
                source: None,
            })
            .expect("seed roster");

        let hydrated = hydrate_roster_from_team_config_once_at_startup_if_empty(
            tempdir.path(),
            &team,
            &roster_store,
        )
        .expect("hydrate roster");

        assert!(!hydrated);
        assert_eq!(roster_store.replace_calls(), 1);
        let members = roster_store.members();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].agent_name, sender);
    }
}
