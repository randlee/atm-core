//! Hidden daemon-side ingress/export helper layer used by concrete boundary
//! adapters.

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
    NotificationEvent, ReconcileRequest, ReconcileResult, WatchEventBatch, WatchSubscriptionRequest,
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
    let team_dir = home::team_dir_from_home(&request.home_dir, &request.team)?;
    let team_config = config::load_team_config(&team_dir)?;
    Ok(ConfigTeamLoadResponse {
        team_dir,
        team_config,
    })
}

pub fn import_inbox_source(
    request: InboxIngressImportRequest,
) -> Result<InboxIngressImportResponse, AtmError> {
    let source_files = mailbox::store::load_compat_source_projections(
        &request.home_dir,
        &request.team,
        &request.agent,
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

    Ok(InboxIngressDiagnosticsResponse {
        duplicate_message_ids,
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
    mailbox::store::write_compat_source_projections(&source_files)?;
    Ok(InboxExportRecordResponse { committed_paths })
}

pub fn reexport_messages(
    request: InboxExportReexportMessageRequest,
) -> Result<InboxExportReexportMessageResponse, AtmError> {
    let wrote_messages = request.messages.len();
    mailbox::store::write_compat_mailbox_projection(&request.path, &request.messages)?;
    Ok(InboxExportReexportMessageResponse { wrote_messages })
}

pub fn deliver_notification(event: NotificationEvent) -> Result<(), AtmError> {
    // The current daemon notification adapter is intentionally a retained-log
    // no-op until the R.17 notifier runtime replaces it with a real sink.
    info!(kind = %event.kind, detail = %event.detail, "daemon notification delivered");
    Ok(())
}

pub fn poll_watch(request: WatchSubscriptionRequest) -> Result<WatchEventBatch, AtmError> {
    let paths = crate::mailbox::source::discover_source_paths(
        &request.home_dir,
        &request.team,
        &request.agent,
    )?;
    Ok(WatchEventBatch { paths })
}

pub fn reconcile(request: ReconcileRequest) -> Result<ReconcileResult, AtmError> {
    let batch = poll_watch(WatchSubscriptionRequest {
        home_dir: request.home_dir.clone(),
        team: request.team.clone(),
        agent: request.agent.clone(),
    })?;
    let import = import_inbox_source(InboxIngressImportRequest {
        home_dir: request.home_dir,
        team: request.team,
        agent: request.agent,
    })?;
    Ok(ReconcileResult {
        observed_paths: batch.paths.len(),
        imported_sources: import.source_files.len(),
    })
}
