use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config::{self, AtmConfig};
use crate::error::AtmError;
use crate::mailbox;
use crate::mailbox::source::SourceFile;
use crate::read::seen_state;
use crate::schema::{MessageEnvelope, TeamConfig};
use crate::send::{PostSendHookContext, maybe_run_post_send_hook};
use crate::types::IsoTimestamp;
use crate::workflow::{self, WorkflowStateFile};

pub(crate) trait RetainedServiceRuntime {
    fn load_config(&self, current_dir: &Path) -> Result<Option<AtmConfig>, AtmError>;
    fn load_team_config(&self, team_dir: &Path) -> Result<TeamConfig, AtmError>;
    fn team_dir(&self, home_dir: &Path, team: &str) -> Result<PathBuf, AtmError>;
    fn inbox_path(&self, home_dir: &Path, team: &str, agent: &str) -> Result<PathBuf, AtmError>;
    fn workflow_state_path(
        &self,
        home_dir: &Path,
        team: &str,
        agent: &str,
    ) -> Result<PathBuf, AtmError>;
    fn load_workflow_state(
        &self,
        home_dir: &Path,
        team: &str,
        agent: &str,
    ) -> Result<WorkflowStateFile, AtmError>;
    fn save_workflow_state(
        &self,
        home_dir: &Path,
        team: &str,
        agent: &str,
        state: &WorkflowStateFile,
    ) -> Result<(), AtmError>;
    fn observe_source_files(
        &self,
        home_dir: &Path,
        team: &str,
        agent: &str,
    ) -> Result<Vec<SourceFile>, AtmError>;
    fn commit_source_files(&self, source_files: &[SourceFile]) -> Result<(), AtmError>;
    fn read_messages(&self, path: &Path) -> Result<Vec<MessageEnvelope>, AtmError>;
    fn commit_mailbox_state(
        &self,
        path: &Path,
        messages: &[MessageEnvelope],
    ) -> Result<(), AtmError>;
    fn load_seen_watermark(
        &self,
        home_dir: &Path,
        team: &str,
        agent: &str,
    ) -> Result<Option<IsoTimestamp>, AtmError>;
    fn save_seen_watermark(
        &self,
        home_dir: &Path,
        team: &str,
        agent: &str,
        timestamp: IsoTimestamp,
    ) -> Result<(), AtmError>;
    fn default_lock_timeout(&self) -> Duration;
    fn maybe_run_post_send_hook(
        &self,
        warnings: &mut Vec<String>,
        config: Option<&AtmConfig>,
        context: PostSendHookContext<'_>,
    );

    fn commit_workflow_state<T, I, F>(
        &self,
        home_dir: &Path,
        team: &str,
        agent: &str,
        extra_write_paths: I,
        timeout: Duration,
        body: F,
    ) -> Result<T, AtmError>
    where
        I: IntoIterator<Item = PathBuf>,
        F: FnOnce(&mut WorkflowStateFile) -> Result<(T, bool), AtmError>;

    fn with_locked_source_files<T, I, F>(
        &self,
        home_dir: &Path,
        team: &str,
        agent: &str,
        extra_write_paths: I,
        timeout: Duration,
        body: F,
    ) -> Result<T, AtmError>
    where
        I: IntoIterator<Item = PathBuf>,
        F: FnOnce(&[PathBuf], &mut Vec<SourceFile>) -> Result<T, AtmError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LocalServiceRuntime;

impl RetainedServiceRuntime for LocalServiceRuntime {
    fn load_config(&self, current_dir: &Path) -> Result<Option<AtmConfig>, AtmError> {
        config::load_config(current_dir)
    }

    fn load_team_config(&self, team_dir: &Path) -> Result<TeamConfig, AtmError> {
        config::load_team_config(team_dir)
    }

    fn team_dir(&self, home_dir: &Path, team: &str) -> Result<PathBuf, AtmError> {
        crate::home::team_dir_from_home(home_dir, team)
    }

    fn inbox_path(&self, home_dir: &Path, team: &str, agent: &str) -> Result<PathBuf, AtmError> {
        crate::home::inbox_path_from_home(home_dir, team, agent)
    }

    fn workflow_state_path(
        &self,
        home_dir: &Path,
        team: &str,
        agent: &str,
    ) -> Result<PathBuf, AtmError> {
        crate::home::workflow_state_path_from_home(home_dir, team, agent)
    }

    fn load_workflow_state(
        &self,
        home_dir: &Path,
        team: &str,
        agent: &str,
    ) -> Result<WorkflowStateFile, AtmError> {
        workflow::load_workflow_state(home_dir, team, agent)
    }

    fn save_workflow_state(
        &self,
        home_dir: &Path,
        team: &str,
        agent: &str,
        state: &WorkflowStateFile,
    ) -> Result<(), AtmError> {
        workflow::save_workflow_state(home_dir, team, agent, state)
    }

    fn observe_source_files(
        &self,
        home_dir: &Path,
        team: &str,
        agent: &str,
    ) -> Result<Vec<SourceFile>, AtmError> {
        mailbox::store::observe_source_files(home_dir, team, agent)
    }

    fn commit_source_files(&self, source_files: &[SourceFile]) -> Result<(), AtmError> {
        mailbox::store::commit_source_files(source_files)
    }

    fn read_messages(&self, path: &Path) -> Result<Vec<MessageEnvelope>, AtmError> {
        mailbox::read_messages(path)
    }

    fn commit_mailbox_state(
        &self,
        path: &Path,
        messages: &[MessageEnvelope],
    ) -> Result<(), AtmError> {
        mailbox::store::commit_mailbox_state(path, messages)
    }

    fn load_seen_watermark(
        &self,
        home_dir: &Path,
        team: &str,
        agent: &str,
    ) -> Result<Option<IsoTimestamp>, AtmError> {
        seen_state::load_seen_watermark(home_dir, team, agent)
    }

    fn save_seen_watermark(
        &self,
        home_dir: &Path,
        team: &str,
        agent: &str,
        timestamp: IsoTimestamp,
    ) -> Result<(), AtmError> {
        seen_state::save_seen_watermark(home_dir, team, agent, timestamp)
    }

    fn default_lock_timeout(&self) -> Duration {
        mailbox::lock::default_lock_timeout()
    }

    fn maybe_run_post_send_hook(
        &self,
        warnings: &mut Vec<String>,
        config: Option<&AtmConfig>,
        context: PostSendHookContext<'_>,
    ) {
        maybe_run_post_send_hook(warnings, config, context);
    }

    fn commit_workflow_state<T, I, F>(
        &self,
        home_dir: &Path,
        team: &str,
        agent: &str,
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

    fn with_locked_source_files<T, I, F>(
        &self,
        home_dir: &Path,
        team: &str,
        agent: &str,
        extra_write_paths: I,
        timeout: Duration,
        body: F,
    ) -> Result<T, AtmError>
    where
        I: IntoIterator<Item = PathBuf>,
        F: FnOnce(&[PathBuf], &mut Vec<SourceFile>) -> Result<T, AtmError>,
    {
        mailbox::store::with_locked_source_files(
            home_dir,
            team,
            agent,
            extra_write_paths,
            timeout,
            body,
        )
    }
}
