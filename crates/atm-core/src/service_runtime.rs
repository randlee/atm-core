use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::boundary::{InboxExportReexportMessageRequest, MessageKey};
use crate::config::{self, AtmConfig};
use crate::delivery_policy::DeliveryRecipientSnapshot;
use crate::error::AtmError;
use crate::read::seen_state;
use crate::schema::{MessageEnvelope, TeamConfig};
use crate::send::{PostSendHookContext, maybe_run_post_send_hook};
use crate::types::{AgentName, IsoTimestamp, TeamName};
use crate::workflow::{self, WorkflowStateFile};

const WORKFLOW_LOCK_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RetainedMailboxTimeoutPolicy {
    pub(crate) workflow_lock_timeout: Duration,
}

pub(crate) trait RetainedServiceRuntime {
    fn load_config(&self, current_dir: &Path) -> Result<Option<AtmConfig>, AtmError>;
    fn load_team_config(&self, team_dir: &Path) -> Result<TeamConfig, AtmError>;
    fn team_dir(&self, home_dir: &Path, team: &TeamName) -> Result<PathBuf, AtmError>;
    fn inbox_path(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<PathBuf, AtmError>;
    fn load_seen_watermark(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<IsoTimestamp>, AtmError>;
    fn save_seen_watermark(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        timestamp: IsoTimestamp,
    ) -> Result<(), AtmError>;
    fn mailbox_timeout_policy(&self) -> RetainedMailboxTimeoutPolicy;
    fn maybe_run_post_send_hook(
        &self,
        warnings: &mut Vec<crate::send::WarningEntry>,
        config: Option<&AtmConfig>,
        context: PostSendHookContext<'_>,
    );
    #[expect(
        dead_code,
        reason = "Phase Y.6 leaves the full projection refresh owner in place for explicit rebuild and repair flows even after normal runtime send/ack move to append-only output."
    )]
    fn refresh_compat_inbox_projection(
        &self,
        home_dir: &Path,
        recipient: &DeliveryRecipientSnapshot,
    ) -> Result<(), AtmError>;
    fn append_compat_inbox_message(
        &self,
        inbox_path: &Path,
        recipient: &DeliveryRecipientSnapshot,
        message: &MessageEnvelope,
    ) -> Result<(), AtmError>;
    fn load_roster_member(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<crate::boundary::RosterMemberRecord>, AtmError>;

    fn commit_workflow_state<T, I, F>(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        extra_write_paths: I,
        timeout: Duration,
        body: F,
    ) -> Result<T, AtmError>
    where
        I: IntoIterator<Item = PathBuf>,
        F: FnOnce(&mut WorkflowStateFile) -> Result<(T, bool), AtmError>;
}

#[derive(Clone)]
pub struct LocalServiceRuntime {
    pub(crate) mail_store: std::sync::Arc<dyn crate::boundary::MailStore + Send + Sync>,
    pub(crate) task_store: std::sync::Arc<dyn crate::boundary::TaskStore + Send + Sync>,
    pub(crate) roster_store: std::sync::Arc<dyn crate::boundary::RosterStore + Send + Sync>,
}

impl LocalServiceRuntime {
    pub fn new(
        mail_store: std::sync::Arc<dyn crate::boundary::MailStore + Send + Sync>,
        task_store: std::sync::Arc<dyn crate::boundary::TaskStore + Send + Sync>,
        roster_store: std::sync::Arc<dyn crate::boundary::RosterStore + Send + Sync>,
    ) -> Self {
        Self {
            mail_store,
            task_store,
            roster_store,
        }
    }
}

impl fmt::Debug for LocalServiceRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalServiceRuntime")
            .field("mail_store", &std::sync::Arc::as_ptr(&self.mail_store))
            .field("task_store", &std::sync::Arc::as_ptr(&self.task_store))
            .field("roster_store", &std::sync::Arc::as_ptr(&self.roster_store))
            .finish()
    }
}

impl RetainedServiceRuntime for LocalServiceRuntime {
    fn load_config(&self, current_dir: &Path) -> Result<Option<AtmConfig>, AtmError> {
        config::load_config(current_dir)
    }

    fn load_team_config(&self, team_dir: &Path) -> Result<TeamConfig, AtmError> {
        config::load_team_config(team_dir)
    }

    fn team_dir(&self, home_dir: &Path, team: &TeamName) -> Result<PathBuf, AtmError> {
        crate::home::team_dir_from_home(home_dir, team)
    }

    fn inbox_path(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<PathBuf, AtmError> {
        crate::home::inbox_path_from_home(home_dir, team, agent)
    }

    fn load_seen_watermark(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<IsoTimestamp>, AtmError> {
        seen_state::load_seen_watermark(home_dir, team, agent)
    }

    fn save_seen_watermark(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        timestamp: IsoTimestamp,
    ) -> Result<(), AtmError> {
        seen_state::save_seen_watermark(home_dir, team, agent, timestamp)
    }

    fn mailbox_timeout_policy(&self) -> RetainedMailboxTimeoutPolicy {
        RetainedMailboxTimeoutPolicy {
            workflow_lock_timeout: WORKFLOW_LOCK_TIMEOUT,
        }
    }

    fn maybe_run_post_send_hook(
        &self,
        warnings: &mut Vec<crate::send::WarningEntry>,
        config: Option<&AtmConfig>,
        context: PostSendHookContext<'_>,
    ) {
        maybe_run_post_send_hook(warnings, config, context);
    }

    fn refresh_compat_inbox_projection(
        &self,
        home_dir: &Path,
        recipient: &DeliveryRecipientSnapshot,
    ) -> Result<(), AtmError> {
        if !recipient.allows_claude_jsonl_append() {
            return Ok(());
        }
        let inbox_path =
            crate::home::inbox_path_from_home(home_dir, &recipient.team, &recipient.agent)?;
        let messages =
            load_store_backed_mailbox_projection(self, &recipient.team, &recipient.agent)?;

        crate::direct_boundaries::reexport_messages(InboxExportReexportMessageRequest {
            path: inbox_path,
            messages,
        })
        .map(|_| ())
    }

    fn append_compat_inbox_message(
        &self,
        inbox_path: &Path,
        recipient: &DeliveryRecipientSnapshot,
        message: &MessageEnvelope,
    ) -> Result<(), AtmError> {
        if !recipient.allows_claude_jsonl_append() {
            return Ok(());
        }
        if compat_inbox_uses_legacy_array_format(inbox_path)? {
            let messages =
                load_store_backed_mailbox_projection(self, &recipient.team, &recipient.agent)?;
            return crate::direct_boundaries::reexport_messages(
                InboxExportReexportMessageRequest {
                    path: inbox_path.to_path_buf(),
                    messages,
                },
            )
            .map(|_| ());
        }
        crate::mailbox::store::append_compat_mailbox_message(inbox_path, message)
    }

    fn load_roster_member(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<crate::boundary::RosterMemberRecord>, AtmError> {
        self.roster_store
            .query_membership(crate::boundary::RosterStoreQueryMembershipRequest {
                team: team.clone(),
                member: agent.clone(),
            })
            .map(|response| response.member)
    }

    fn commit_workflow_state<T, I, F>(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        extra_write_paths: I,
        timeout: Duration,
        body: F,
    ) -> Result<T, AtmError>
    where
        I: IntoIterator<Item = PathBuf>,
        F: FnOnce(&mut WorkflowStateFile) -> Result<(T, bool), AtmError>,
    {
        workflow::commit_workflow_state(home_dir, team, agent, extra_write_paths, timeout, body)
    }
}

fn load_store_backed_mailbox_projection(
    runtime: &LocalServiceRuntime,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<MessageEnvelope>, AtmError> {
    let mut metadata_rows = runtime
        .mail_store
        .query_mailbox_metadata(crate::boundary::MailStoreQueryMailboxMetadataRequest {
            team: team.clone(),
            agent: agent.clone(),
            limit: None,
        })?
        .rows;
    metadata_rows.sort_by(|left, right| {
        left.message_at
            .cmp(&right.message_at)
            .then_with(|| left.message_key.as_ref().cmp(right.message_key.as_ref()))
    });

    metadata_rows
        .into_iter()
        .map(|row| load_projection_message(runtime, team, agent, &row.message_key))
        .collect()
}

fn load_projection_message(
    runtime: &LocalServiceRuntime,
    team: &TeamName,
    agent: &AgentName,
    message_key: &MessageKey,
) -> Result<MessageEnvelope, AtmError> {
    runtime
        .mail_store
        .load_message(crate::boundary::MailStoreLoadMessageRequest {
            team: team.clone(),
            agent: agent.clone(),
            message_key: message_key.clone(),
        })?
        .record
        .map(|record| record.envelope)
        .ok_or_else(|| {
            AtmError::validation(format!(
                "sqlite mailbox metadata row {} could not be reloaded for compatibility inbox export",
                message_key
            ))
            .with_recovery(
                "Repair or remove the malformed sqlite mailbox row before retrying the ATM command.",
            )
        })
}

fn compat_inbox_uses_legacy_array_format(path: &Path) -> Result<bool, AtmError> {
    if !path.exists() {
        return Ok(false);
    }

    let mut file = File::open(path).map_err(|error| {
        AtmError::mailbox_read(format!(
            "failed to inspect compatibility inbox {} before append: {error}",
            path.display()
        ))
        .with_recovery(
            "Retry after concurrent ATM activity completes, or verify the inbox file is readable before retrying the append-only compatibility write.",
        )
        .with_source(error)
    })?;
    let mut prefix = [0_u8; 256];
    let bytes_read = file.read(&mut prefix).map_err(|error| {
        AtmError::mailbox_read(format!(
            "failed to read compatibility inbox {} before append: {error}",
            path.display()
        ))
        .with_recovery(
            "Retry after concurrent ATM activity completes, or verify the inbox file is readable before retrying the append-only compatibility write.",
        )
        .with_source(error)
    })?;
    let visible = String::from_utf8_lossy(&prefix[..bytes_read]);
    Ok(visible.trim_start().starts_with('['))
}
