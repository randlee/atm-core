use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{AgentName, MessageEnvelope, TeamName};

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
