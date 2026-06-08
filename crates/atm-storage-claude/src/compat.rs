use std::collections::HashSet;
use std::path::PathBuf;

use atm_storage::{AgentName, AtmError, MessageEnvelope, TeamName};
use serde::{Deserialize, Serialize};

use crate::mailbox;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceFileRecord {
    pub path: PathBuf,
    pub messages: Vec<MessageEnvelope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceIngressImportRequest {
    pub home_dir: PathBuf,
    pub team: TeamName,
    pub agent: AgentName,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceIngressImportResponse {
    pub source_files: Vec<SourceFileRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceIngressIdentityFingerprintRequest {
    pub message: MessageEnvelope,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceIngressIdentityFingerprintResponse {
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceIngressDiagnosticsRequest {
    pub source_files: Vec<SourceFileRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceIngressDiagnosticsResponse {
    pub duplicate_message_ids: usize,
    pub messages_without_ids: usize,
}

pub trait SourceIngress {
    fn import_inbox_source(
        &self,
        request: SourceIngressImportRequest,
    ) -> Result<SourceIngressImportResponse, AtmError>;

    fn compute_identity_fingerprint(
        &self,
        request: SourceIngressIdentityFingerprintRequest,
    ) -> SourceIngressIdentityFingerprintResponse;

    fn report_diagnostics(
        &self,
        request: SourceIngressDiagnosticsRequest,
    ) -> SourceIngressDiagnosticsResponse;
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectionAppendMode {
    RecoveredLogicalMessageSet,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectionExportRecordRequest {
    pub source_files: Vec<SourceFileRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionExportRecordResponse {
    pub committed_paths: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectionExportReexportMessageRequest {
    pub path: PathBuf,
    pub messages: Vec<MessageEnvelope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionExportReexportMessageResponse {
    pub wrote_messages: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectionExportAppendMessageSetRequest {
    pub path: PathBuf,
    pub messages: Vec<MessageEnvelope>,
    pub mode: ProjectionAppendMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionExportAppendMessageSetResponse {
    pub wrote_messages: usize,
}

pub trait ProjectionExport {
    fn export_record(
        &self,
        request: ProjectionExportRecordRequest,
    ) -> Result<ProjectionExportRecordResponse, AtmError>;

    fn reexport_message(
        &self,
        request: ProjectionExportReexportMessageRequest,
    ) -> Result<ProjectionExportReexportMessageResponse, AtmError>;

    fn append_message_set(
        &self,
        request: ProjectionExportAppendMessageSetRequest,
    ) -> Result<ProjectionExportAppendMessageSetResponse, AtmError>;
}

pub fn import_inbox_source(
    request: SourceIngressImportRequest,
) -> Result<SourceIngressImportResponse, AtmError> {
    Ok(SourceIngressImportResponse {
        source_files: mailbox::import_source_projections(
            &request.home_dir,
            &request.team,
            &request.agent,
        )?,
    })
}

pub fn compute_identity_fingerprint(
    request: SourceIngressIdentityFingerprintRequest,
) -> SourceIngressIdentityFingerprintResponse {
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
    SourceIngressIdentityFingerprintResponse { fingerprint }
}

pub fn report_inbox_diagnostics(
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

pub fn export_source_files(
    request: ProjectionExportRecordRequest,
) -> Result<ProjectionExportRecordResponse, AtmError> {
    let committed_paths = request.source_files.len();
    mailbox::export_source_projections(&request.source_files)?;
    Ok(ProjectionExportRecordResponse { committed_paths })
}

pub fn reexport_messages(
    request: ProjectionExportReexportMessageRequest,
) -> Result<ProjectionExportReexportMessageResponse, AtmError> {
    let wrote_messages = request.messages.len();
    mailbox::reexport_messages(&request.path, &request.messages)?;
    Ok(ProjectionExportReexportMessageResponse { wrote_messages })
}

pub fn append_message_set(
    request: ProjectionExportAppendMessageSetRequest,
) -> Result<ProjectionExportAppendMessageSetResponse, AtmError> {
    let wrote_messages = request.messages.len();
    mailbox::append_message_set(&request.path, request.mode, &request.messages)?;
    Ok(ProjectionExportAppendMessageSetResponse { wrote_messages })
}
