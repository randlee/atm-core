//! The un-hooked task commands.
//!
//! [`NormalTaskCommands`] is the inner body that [`crate::hook_filter`] wraps.
//! It delegates to the router's un-hooked owners (`write_unhooked`, `task_move`,
//! `list_messages`), so no task body is copied here.

use async_trait::async_trait;
use atm_core::api::ApiResponse;
use atm_core::boundary::{
    TaskAssign, TaskCallCtx, TaskClose, TaskCommands, TaskEvents, TaskList, TaskStart, sealed,
};
use atm_core::error::AtmError;
use atm_core::protocol::TaskMoveRequest;

use crate::storage_and_nudge_router::StorageAndNudgeRouter;

pub struct NormalTaskCommands {
    router: StorageAndNudgeRouter,
}

impl NormalTaskCommands {
    pub fn new(base: StorageAndNudgeRouter) -> Self {
        Self { router: base }
    }
}

impl sealed::Sealed for NormalTaskCommands {}

#[async_trait]
impl TaskCommands for NormalTaskCommands {
    async fn assign(&self, req: TaskAssign, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError> {
        self.router
            .write_unhooked(*req.0, ctx.ingress.clone(), ctx.deadline, ctx.request_id)
            .await
    }
    async fn start(&self, req: TaskStart, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError> {
        self.router
            .write_unhooked(*req.0, ctx.ingress.clone(), ctx.deadline, ctx.request_id)
            .await
    }
    async fn close(&self, req: TaskClose, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError> {
        self.router
            .write_unhooked(*req.0, ctx.ingress.clone(), ctx.deadline, ctx.request_id)
            .await
    }
    async fn move_task(
        &self,
        req: TaskMoveRequest,
        ctx: &TaskCallCtx,
    ) -> Result<ApiResponse, AtmError> {
        self.router
            .task_move(req, ctx.ingress.clone(), ctx.deadline)
            .await
    }
    async fn list(&self, req: TaskList, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError> {
        self.router.list_messages(req.0, ctx.deadline).await
    }
    async fn events(&self, req: TaskEvents, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError> {
        self.router.list_messages(req.0, ctx.deadline).await
    }
}

#[cfg(test)]
mod tests {
    use atm_core::api::{AuthenticatedIngress, RequestDeadline};
    use atm_core::boundary::{TaskAssign, TaskCallCtx, TaskCommands, TaskList};
    use atm_core::protocol::ResponseEnvelope;

    use super::NormalTaskCommands;
    use crate::storage_and_nudge_router::tests::{fixture, task_write_request};

    fn sender_ctx() -> TaskCallCtx {
        TaskCallCtx {
            caller_identity: "sender".parse().expect("agent"),
            caller_team: "test-team".parse().expect("team"),
            host: None,
            ingress: AuthenticatedIngress::Local,
            deadline: RequestDeadline::after(std::time::Duration::from_secs(5)),
            request_id: atm_core::protocol::next_request_id(),
        }
    }

    /// Pass-through check: the installed normal commands answer with the
    /// un-hooked response shapes. Every existing router test that dispatches a
    /// task operation through the router runs both unfiltered and through
    /// `NormalTaskCommands`; service-runtime-direct and non-task tests are
    /// outside that suite.
    #[tokio::test]
    async fn existing_task_tests_pass_through_normal_task_commands() {
        let fixture = fixture(true, None, None);
        let commands = NormalTaskCommands::new(fixture.router.clone());

        let assigned = commands
            .assign(
                TaskAssign(Box::new(task_write_request(&fixture, "T-PASS-A"))),
                &sender_ctx(),
            )
            .await
            .expect("assign passes through");
        assert!(matches!(assigned.into_inner(), ResponseEnvelope::Send(_)));

        let listed = commands
            .list(
                TaskList(atm_core::list::ListQuery {
                    home_dir: fixture.home_dir.clone(),
                    current_dir: fixture.current_dir.clone(),
                    caller_identity: "sender".parse().expect("agent"),
                    caller_team: "test-team".parse().expect("team"),
                    target_address: None,
                    selection_mode: atm_core::types::ReadSelection::All,
                    seen_state_filter: false,
                    limit: None,
                    sender_filter: None,
                    timestamp_filter: None,
                    task_filter: None,
                    contains_filter: None,
                    task_ledger: Some(atm_core::list::TaskLedgerQuery::Tasks { member: None }),
                }),
                &sender_ctx(),
            )
            .await;
        assert!(listed.is_ok(), "task list passes through: {listed:?}");
    }
}
