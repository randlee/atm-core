//! Query construction and read-only execution for the Python graft session.

use super::*;
use atm_core::read::PeekQuery;

/// Read-family operations share one outer Python-to-async bridge.  Native
/// tools select `Peek` so they cannot mutate mailbox state, while the legacy
/// Python read API retains its explicit mutating operation.
enum ReadOperation {
    Read(ReadQuery),
    Peek(PeekQuery),
}

impl PyGraftSession {
    pub(super) fn build_read_query(&self, seen_state_update: bool) -> PyResult<ReadQuery> {
        let (home_dir, current_dir) = Self::command_paths()?;
        let team = self.caller_team()?;
        let query = ReadQuery::new(
            home_dir,
            current_dir,
            self.caller.agent().clone(),
            None,
            team.clone(),
            ReadSelection::All,
            false,
            seen_state_update,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .map_err(atm_error)?;
        Ok(query
            .with_caller_chat_id(self.caller.chat_id().cloned())
            .with_activity_observation(activity_observation_for_resolved_caller(
                self.caller.agent(),
                &team,
            )))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_tool_read_query(
        &self,
        selection: &str,
        message_id: Option<&str>,
        task: Option<&str>,
        contains: Option<&str>,
        since: Option<&str>,
        from_agent: Option<&str>,
    ) -> PyResult<PeekQuery> {
        let (home_dir, current_dir) = Self::command_paths()?;
        let team = self.caller_team()?;
        let timestamp = since
            .map(str::parse::<IsoTimestamp>)
            .transpose()
            .map_err(|error| {
                atm_error(AtmError::validation(format!(
                    "invalid since timestamp: {error}"
                )))
            })?;
        PeekQuery::new(
            home_dir,
            current_dir,
            self.caller.agent().clone(),
            None,
            team,
            Self::read_selection(selection)?,
            false,
            message_id,
            from_agent,
            timestamp,
            task,
            contains,
            None,
        )
        .map_err(atm_error)
        .map(|query| query.with_caller_chat_id(self.caller.chat_id().cloned()))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_list_query(
        &self,
        selection: &str,
        limit: Option<usize>,
        task: Option<&str>,
        contains: Option<&str>,
        since: Option<&str>,
        from_agent: Option<&str>,
    ) -> PyResult<ListQuery> {
        let (home_dir, current_dir) = Self::command_paths()?;
        let team = self.caller_team()?;
        let timestamp = since
            .map(str::parse::<IsoTimestamp>)
            .transpose()
            .map_err(|error| {
                atm_error(AtmError::validation(format!(
                    "invalid since timestamp: {error}"
                )))
            })?;
        ListQuery::new(
            home_dir,
            current_dir,
            self.caller.agent().clone(),
            None,
            team,
            Self::read_selection(selection)?,
            false,
            limit,
            from_agent,
            timestamp,
            task,
            contains,
        )
        .map_err(atm_error)
    }

    pub(super) fn read_raw(&self, query: ReadQuery) -> PyResult<ReadOutcome> {
        self.read_operation_raw(ReadOperation::Read(query))
    }

    pub(super) fn peek_raw(&self, query: PeekQuery) -> PyResult<ReadOutcome> {
        self.read_operation_raw(ReadOperation::Peek(query))
    }

    fn read_operation_raw(&self, operation: ReadOperation) -> PyResult<ReadOutcome> {
        let client = self.client()?;
        python_extension_runtime()?
            .lock()
            .map_err(|_| poisoned_lock_error("ATM Python extension runtime"))?
            .block_on(async move {
                match operation {
                    ReadOperation::Read(query) => client.read_message(query).await,
                    ReadOperation::Peek(query) => client.peek_message(query).await,
                }
            })
            .map_err(atm_error)
    }

    pub(super) fn read_outcome(&self, query: ReadQuery) -> PyResult<AtmReadResult> {
        self.read_raw(query).and_then(AtmReadResult::from_outcome)
    }

    pub(super) fn peek_outcome(&self, query: PeekQuery) -> PyResult<AtmReadResult> {
        self.peek_raw(query).and_then(AtmReadResult::from_outcome)
    }

    pub(super) fn list_outcome(&self, query: ListQuery) -> PyResult<AtmListResult> {
        let client = self.client()?;
        python_extension_runtime()?
            .lock()
            .map_err(|_| poisoned_lock_error("ATM Python extension runtime"))?
            .block_on(client.list_messages(query))
            .map(AtmListResult::from)
            .map_err(atm_error)
    }
}
