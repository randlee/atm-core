//! Hidden daemon-side ingress/export helper layer used by concrete boundary
//! adapters.

use std::collections::HashSet;

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

pub(crate) fn load_workspace_config(
    request: ConfigLoadRequest,
) -> Result<ConfigLoadResponse, AtmError> {
    Ok(ConfigLoadResponse {
        config: config::load_config(&request.current_dir).map_err(|error| {
            AtmError::daemon_unavailable(format!(
                "daemon ConfigIngress could not load workspace config from {}",
                request.current_dir.display()
            ))
            .with_recovery(
                "Fix the workspace ATM configuration or current-directory selection before retrying daemon startup or same-host bootstrap.",
            )
            .with_source(error)
        })?,
    })
}

pub(crate) fn load_team_config(
    request: ConfigTeamLoadRequest,
) -> Result<ConfigTeamLoadResponse, AtmError> {
    let team_dir = home::team_dir_from_home(&request.home_dir, &request.team).map_err(|error| {
        AtmError::daemon_unavailable(format!(
            "daemon ConfigIngress could not resolve team {} from {}",
            request.team,
            request.home_dir.display()
        ))
        .with_recovery(
            "Verify the ATM home directory and team roster layout before retrying daemon team configuration hydration.",
        )
        .with_source(error)
    })?;
    let team_config = config::load_team_config(&team_dir).map_err(|error| {
        AtmError::daemon_unavailable(format!(
            "daemon ConfigIngress could not load team config from {}",
            team_dir.display()
        ))
        .with_recovery(
            "Fix the team ATM configuration and retry daemon startup or team runtime hydration.",
        )
        .with_source(error)
    })?;
    Ok(ConfigTeamLoadResponse {
        team_dir,
        team_config,
    })
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
