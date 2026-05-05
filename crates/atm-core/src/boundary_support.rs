//! Hidden helper layer used by concrete boundary adapters.
#![allow(dead_code)]

use std::collections::HashSet;

use tracing::info;

use crate::boundary::{
    ConfigLoadRequest, ConfigLoadResponse, ConfigTeamLoadRequest, ConfigTeamLoadResponse,
    InboxExportRecordRequest, InboxExportRecordResponse, InboxExportReexportMessageRequest,
    InboxExportReexportMessageResponse, InboxIngressDiagnosticsRequest,
    InboxIngressDiagnosticsResponse, InboxIngressIdentityFingerprintRequest,
    InboxIngressIdentityFingerprintResponse, InboxIngressImportRequest, InboxIngressImportResponse,
    InboxSourceFileRecord,
};
use crate::config;
use crate::error::AtmError;
use crate::home;
use crate::mailbox;
use crate::mailbox::source::SourceFile;
use crate::protocol::{
    NotificationEvent, RuntimeStatusSnapshot, WatchEventBatch, WatchSubscriptionRequest,
};

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

pub fn load_workspace_config(request: ConfigLoadRequest) -> Result<ConfigLoadResponse, AtmError> {
    Ok(ConfigLoadResponse {
        config: config::load_config(&request.current_dir)?,
    })
}

pub fn load_team_config(
    request: ConfigTeamLoadRequest,
) -> Result<ConfigTeamLoadResponse, AtmError> {
    let team_dir = home::team_dir_from_home(&request.home_dir, request.team.as_str())?;
    let team_config = config::load_team_config(&team_dir)?;
    Ok(ConfigTeamLoadResponse {
        team_dir,
        team_config,
    })
}

pub fn import_inbox_source(
    request: InboxIngressImportRequest,
) -> Result<InboxIngressImportResponse, AtmError> {
    let source_files = mailbox::store::observe_source_files(
        &request.home_dir,
        request.team.as_str(),
        request.agent.as_str(),
    )?;
    Ok(InboxIngressImportResponse {
        source_files: source_files
            .into_iter()
            .map(to_boundary_source_file)
            .collect(),
    })
}

pub fn compute_identity_fingerprint(
    request: InboxIngressIdentityFingerprintRequest,
) -> Result<InboxIngressIdentityFingerprintResponse, AtmError> {
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
    Ok(InboxIngressIdentityFingerprintResponse { fingerprint })
}

pub fn report_inbox_diagnostics(
    request: InboxIngressDiagnosticsRequest,
) -> Result<InboxIngressDiagnosticsResponse, AtmError> {
    let mut seen = HashSet::new();
    let mut duplicate_legacy_message_ids = 0usize;
    let mut messages_without_ids = 0usize;

    for source in request.source_files {
        for message in source.messages {
            if let Some(message_id) = message.message_id {
                if !seen.insert(message_id) {
                    duplicate_legacy_message_ids += 1;
                }
            } else {
                messages_without_ids += 1;
            }
        }
    }

    Ok(InboxIngressDiagnosticsResponse {
        duplicate_legacy_message_ids,
        messages_without_ids,
    })
}

pub fn export_source_files(
    request: InboxExportRecordRequest,
) -> Result<InboxExportRecordResponse, AtmError> {
    let committed_paths = request.source_files.len();
    let source_files = request
        .source_files
        .into_iter()
        .map(from_boundary_source_file)
        .collect::<Vec<_>>();
    mailbox::store::commit_source_files(&source_files)?;
    Ok(InboxExportRecordResponse { committed_paths })
}

pub fn reexport_messages(
    request: InboxExportReexportMessageRequest,
) -> Result<InboxExportReexportMessageResponse, AtmError> {
    let wrote_messages = request.messages.len();
    mailbox::store::commit_mailbox_state(&request.path, &request.messages)?;
    Ok(InboxExportReexportMessageResponse { wrote_messages })
}

pub fn deliver_notification(event: NotificationEvent) -> Result<(), AtmError> {
    info!(kind = %event.kind, detail = %event.detail, "daemon notification delivered");
    Ok(())
}

pub fn snapshot_status() -> Result<RuntimeStatusSnapshot, AtmError> {
    Ok(RuntimeStatusSnapshot {
        status: "ready".to_string(),
        detail: Some("daemon runtime adapters are active".to_string()),
    })
}

pub fn poll_watch(request: WatchSubscriptionRequest) -> Result<WatchEventBatch, AtmError> {
    let paths = crate::mailbox::source::discover_source_paths(
        &request.home_dir,
        request.team.as_str(),
        request.agent.as_str(),
    )?;
    Ok(WatchEventBatch { paths })
}
