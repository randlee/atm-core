#![allow(
    dead_code,
    reason = "AC.2 preserves these crate-private Claude compatibility helpers until later internal consumers are removed."
)]

//! Hidden daemon-side ingress/export helper layer used by concrete boundary
//! adapters.

use crate::boundary::{
    ConfigLoadRequest, ConfigLoadResponse, ProjectionAppendMode,
    ProjectionExportAppendMessageSetRequest, ProjectionExportAppendMessageSetResponse,
    ProjectionExportRecordRequest, ProjectionExportRecordResponse,
    ProjectionExportReexportMessageRequest, ProjectionExportReexportMessageResponse,
    SourceFileRecord, SourceIngressDiagnosticsRequest, SourceIngressDiagnosticsResponse,
    SourceIngressIdentityFingerprintRequest, SourceIngressIdentityFingerprintResponse,
    SourceIngressImportRequest, SourceIngressImportResponse,
};
use crate::config;
use crate::error::AtmError;
use crate::mailbox;
use crate::mailbox::source::SourceFile;
use std::collections::HashSet;

fn to_boundary_source_file(source: SourceFile) -> SourceFileRecord {
    SourceFileRecord {
        path: source.path,
        messages: source.messages,
    }
}

fn from_boundary_source_file(source: SourceFileRecord) -> SourceFile {
    SourceFile {
        path: source.path,
        messages: source.messages,
    }
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

pub(crate) fn import_inbox_source(
    request: SourceIngressImportRequest,
) -> Result<SourceIngressImportResponse, AtmError> {
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
    Ok(SourceIngressImportResponse {
        source_files: source_files
            .into_iter()
            .map(to_boundary_source_file)
            .collect(),
    })
}

pub(crate) fn compute_identity_fingerprint(
    request: SourceIngressIdentityFingerprintRequest,
) -> SourceIngressIdentityFingerprintResponse {
    let fingerprint = request
        .message
        .message_id
        .map(|message_id| crate::boundary::MessageFingerprint::from(message_id.to_string()))
        .or_else(|| {
            Some(crate::boundary::MessageFingerprint::from(format!(
                "{}:{}",
                request.message.from,
                request.message.timestamp.into_inner().to_rfc3339()
            )))
        });
    SourceIngressIdentityFingerprintResponse { fingerprint }
}

pub(crate) fn report_inbox_diagnostics(
    request: SourceIngressDiagnosticsRequest,
) -> SourceIngressDiagnosticsResponse {
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

    SourceIngressDiagnosticsResponse {
        duplicate_message_ids,
        messages_without_ids,
    }
}

pub(crate) fn export_source_files(
    request: ProjectionExportRecordRequest,
) -> Result<ProjectionExportRecordResponse, AtmError> {
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
    Ok(ProjectionExportRecordResponse { committed_paths })
}

pub(crate) fn reexport_messages(
    request: ProjectionExportReexportMessageRequest,
) -> Result<ProjectionExportReexportMessageResponse, AtmError> {
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
    Ok(ProjectionExportReexportMessageResponse { wrote_messages })
}

pub(crate) fn append_message_set(
    request: ProjectionExportAppendMessageSetRequest,
) -> Result<ProjectionExportAppendMessageSetResponse, AtmError> {
    let wrote_messages = request.messages.len();
    match request.mode {
        ProjectionAppendMode::RecoveredLogicalMessageSet => {
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
    Ok(ProjectionExportAppendMessageSetResponse { wrote_messages })
}
