use atm_core::api::{ApiResponse, RequestDeadline};
use atm_core::error::AtmError;
use atm_core::list::ListQuery;
use atm_core::protocol::ResponseEnvelope;
use atm_core::read::{PeekQuery, ReadQuery};

use super::StorageAndNudgeRouter;

impl StorageAndNudgeRouter {
    pub(super) async fn list_messages_impl(
        &self,
        mut query: ListQuery,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        atm_core::list::canonicalize_roster_aliases(
            &mut query,
            |team, member, allow_database_wide_alias| {
                self.service_runtime.resolve_roster_member_at_ingress(
                    team,
                    member,
                    allow_database_wide_alias,
                )
            },
        );
        if query.task_ledger.is_some() {
            if deadline.expired() {
                return Err(AtmError::daemon_unavailable(
                    "request deadline expired before task-ledger inspection",
                ));
            }
            let read_deadline = atm_runtime::read_deadline(deadline)?;
            return atm_core::list::list_task_ledger_with_runtime_async(
                query,
                &self.service_runtime,
                read_deadline,
            )
            .await
            .map(ResponseEnvelope::List)
            .map(ApiResponse::new);
        }
        let runtime = self.async_mailbox_runtime.as_ref().ok_or_else(|| {
            AtmError::daemon_unavailable(
                "async mailbox runtime was not installed at daemon startup",
            )
        })?;
        let command = atm_core::list::prepare_async_list(&query)?;
        runtime
            .list_command(command, deadline)
            .await
            .map(ResponseEnvelope::List)
            .map(ApiResponse::new)
    }

    pub(super) async fn peek_messages_impl(
        &self,
        mut query: PeekQuery,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        let runtime = self.async_mailbox_runtime.as_ref().ok_or_else(|| {
            AtmError::daemon_unavailable(
                "async mailbox runtime was not installed at daemon startup",
            )
        })?;
        atm_core::read::canonicalize_peek_roster_aliases(
            &mut query,
            |team, member, allow_database_wide_alias| {
                self.service_runtime.resolve_roster_member_at_ingress(
                    team,
                    member,
                    allow_database_wide_alias,
                )
            },
        );
        let command = atm_core::read::async_projection::prepare_async_peek(&query)?;
        runtime
            .peek_command(command, deadline)
            .await
            .map(Box::new)
            .map(ResponseEnvelope::Peek)
            .map(ApiResponse::new)
    }

    pub(super) async fn receive_messages_impl(
        &self,
        query: ReadQuery,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        self.receive_messages_with_origin(
            query,
            deadline,
            atm_core::send::MessageWriteOrigin::Local,
        )
        .await
    }

    pub(super) async fn receive_messages_with_origin(
        &self,
        mut query: ReadQuery,
        deadline: RequestDeadline,
        origin: atm_core::send::MessageWriteOrigin,
    ) -> Result<ApiResponse, AtmError> {
        let runtime = self.async_mailbox_runtime.as_ref().ok_or_else(|| {
            AtmError::daemon_unavailable(
                "async mailbox runtime was not installed at daemon startup",
            )
        })?;
        atm_core::read::canonicalize_roster_aliases(
            &mut query,
            |team, member, allow_database_wide_alias| {
                self.service_runtime.resolve_roster_member_at_ingress(
                    team,
                    member,
                    allow_database_wide_alias,
                )
            },
        );
        let command =
            atm_core::read::async_projection::prepare_async_read(&query)?.with_write_origin(origin);
        runtime
            .read_command(command, deadline)
            .await
            .map(Box::new)
            .map(ResponseEnvelope::Receive)
            .map(ApiResponse::new)
    }
}
