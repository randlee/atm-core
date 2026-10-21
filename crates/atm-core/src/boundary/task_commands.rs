use async_trait::async_trait;

use super::{HookName, TaskHook, TaskOp, sealed};
use crate::api::{
    ApiRequest, ApiResponse, AuthenticatedIngress, MessageCollectionRequest, RequestDeadline,
};
use crate::error::AtmError;
use crate::list::{ListQuery, TaskLedgerQuery};
use crate::protocol::{RequestId, TaskMoveRequest};
use crate::send::WriteRequest;
use crate::types::{AgentName, HostName, TeamName};

#[derive(Clone, Debug)]
pub struct TaskCallCtx {
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
    pub host: Option<HostName>,
    pub ingress: AuthenticatedIngress,
    pub deadline: RequestDeadline,
    pub request_id: RequestId,
}

#[derive(Clone, Debug)]
pub struct TaskAssign(pub Box<WriteRequest>);

#[derive(Clone, Debug)]
pub struct TaskStart(pub Box<WriteRequest>);

#[derive(Clone, Debug)]
pub struct TaskClose(pub Box<WriteRequest>);

#[derive(Clone, Debug)]
pub struct TaskList(pub ListQuery);

#[derive(Clone, Debug)]
pub struct TaskEvents(pub ListQuery);

#[async_trait]
pub trait TaskCommands: sealed::Sealed + Send + Sync {
    async fn assign(&self, req: TaskAssign, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError>;
    async fn start(&self, req: TaskStart, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError>;
    async fn close(&self, req: TaskClose, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError>;
    async fn move_task(
        &self,
        req: TaskMoveRequest,
        ctx: &TaskCallCtx,
    ) -> Result<ApiResponse, AtmError>;
    async fn list(&self, req: TaskList, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError>;
    async fn events(&self, req: TaskEvents, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError>;
}

#[derive(Clone, Debug)]
pub enum TaskCommand {
    Assign(TaskAssign),
    Start(TaskStart),
    Close(TaskClose),
    Move(TaskMoveRequest),
    List(TaskList),
    Events(TaskEvents),
}

impl TaskCommand {
    #[allow(clippy::result_large_err)]
    pub fn classify(request: ApiRequest) -> Result<Self, ApiRequest> {
        match request {
            ApiRequest::Write(request) => {
                if request.acknowledges_message_id.is_some() {
                    return Err(ApiRequest::Write(request));
                }
                match request.task_op_normalized() {
                    Ok((_, Some(TaskOp::Start))) => Ok(Self::Start(TaskStart(request))),
                    Ok((_, Some(TaskOp::Close { .. }))) => Ok(Self::Close(TaskClose(request))),
                    Ok((Some(_), None)) => Ok(Self::Assign(TaskAssign(request))),
                    _ => Err(ApiRequest::Write(request)),
                }
            }
            ApiRequest::TaskMove(request) => Ok(Self::Move(request)),
            ApiRequest::Messages(request) => match *request {
                MessageCollectionRequest::List(query) => match query.task_ledger {
                    Some(TaskLedgerQuery::Tasks { .. }) => Ok(Self::List(TaskList(query))),
                    Some(TaskLedgerQuery::Events { .. }) => Ok(Self::Events(TaskEvents(query))),
                    _ => Err(ApiRequest::Messages(Box::new(
                        MessageCollectionRequest::List(query),
                    ))),
                },
                other => Err(ApiRequest::Messages(Box::new(other))),
            },
            other => Err(other),
        }
    }

    pub fn hook_name(&self) -> HookName {
        (match self {
            Self::Assign(_) => TaskHook::Assign,
            Self::Start(_) => TaskHook::Start,
            Self::Close(_) => TaskHook::Close,
            Self::Move(_) => TaskHook::Move,
            Self::List(_) => TaskHook::List,
            Self::Events(_) => TaskHook::Events,
        })
        .hook_name()
    }

    /// The caller identity and team carried by this classified task command.
    pub fn caller(&self) -> (&AgentName, &TeamName) {
        match self {
            Self::Assign(TaskAssign(request))
            | Self::Start(TaskStart(request))
            | Self::Close(TaskClose(request)) => (&request.caller_identity, &request.caller_team),
            Self::Move(request) => (&request.caller_identity, &request.caller_team),
            Self::List(TaskList(query)) | Self::Events(TaskEvents(query)) => {
                (&query.caller_identity, &query.caller_team)
            }
        }
    }

    /// Whether this task command is a dry-run write.
    pub fn dry_run(&self) -> bool {
        match self {
            Self::Assign(TaskAssign(request))
            | Self::Start(TaskStart(request))
            | Self::Close(TaskClose(request)) => request.dry_run,
            Self::Move(_) | Self::List(_) | Self::Events(_) => false,
        }
    }

    pub fn request_body(&self) -> serde_json::Value {
        match self {
            Self::Assign(TaskAssign(request))
            | Self::Start(TaskStart(request))
            | Self::Close(TaskClose(request)) => serde_json::to_value(request),
            Self::Move(request) => serde_json::to_value(request),
            Self::List(TaskList(query)) | Self::Events(TaskEvents(query)) => {
                serde_json::to_value(query)
            }
        }
        .expect("task request types serialize as JSON objects")
    }

    pub async fn call(
        self,
        commands: &dyn TaskCommands,
        ctx: &TaskCallCtx,
    ) -> Result<ApiResponse, AtmError> {
        match self {
            Self::Assign(request) => commands.assign(request, ctx).await,
            Self::Start(request) => commands.start(request, ctx).await,
            Self::Close(request) => commands.close(request, ctx).await,
            Self::Move(request) => commands.move_task(request, ctx).await,
            Self::List(request) => commands.list(request, ctx).await,
            Self::Events(request) => commands.events(request, ctx).await,
        }
    }
}

#[cfg(any(test, feature = "test-utils"))]
#[derive(Default)]
pub struct RecordingTaskCommands {
    pub calls: std::sync::Mutex<Vec<(HookName, serde_json::Value)>>,
    pub response: std::sync::Mutex<Option<ApiResponse>>,
}

#[cfg(any(test, feature = "test-utils"))]
impl sealed::Sealed for RecordingTaskCommands {}

#[cfg(any(test, feature = "test-utils"))]
impl RecordingTaskCommands {
    fn record(&self, command: TaskCommand) -> Result<ApiResponse, AtmError> {
        self.calls
            .lock()
            .expect("recording calls mutex poisoned")
            .push((command.hook_name(), command.request_body()));
        self.response
            .lock()
            .expect("recording response mutex poisoned")
            .clone()
            .ok_or_else(|| {
                AtmError::validation_with_recovery(
                    "RecordingTaskCommands has no response",
                    "set the test double's response before calling it",
                )
            })
    }
}

#[cfg(any(test, feature = "test-utils"))]
#[async_trait]
impl TaskCommands for RecordingTaskCommands {
    async fn assign(&self, req: TaskAssign, _ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError> {
        self.record(TaskCommand::Assign(req))
    }
    async fn start(&self, req: TaskStart, _ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError> {
        self.record(TaskCommand::Start(req))
    }
    async fn close(&self, req: TaskClose, _ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError> {
        self.record(TaskCommand::Close(req))
    }
    async fn move_task(
        &self,
        req: TaskMoveRequest,
        _ctx: &TaskCallCtx,
    ) -> Result<ApiResponse, AtmError> {
        self.record(TaskCommand::Move(req))
    }
    async fn list(&self, req: TaskList, _ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError> {
        self.record(TaskCommand::List(req))
    }
    async fn events(&self, req: TaskEvents, _ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError> {
        self.record(TaskCommand::Events(req))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boundary::TaskCloseOutcome;
    use crate::send::SendMessageSource;
    use crate::types::ReadSelection;

    const TEST_SENDER: &str = "test-sender";

    fn write() -> WriteRequest {
        WriteRequest::new(
            "/home".into(),
            "/work".into(),
            TEST_SENDER.parse().unwrap(),
            "dev@test-team",
            "test-team".parse().unwrap(),
            SendMessageSource::Inline("body".into()),
            None,
            false,
            Some("bk-2".parse().unwrap()),
            false,
        )
        .unwrap()
    }

    fn query(view: Option<TaskLedgerQuery>) -> ListQuery {
        ListQuery {
            home_dir: "/home".into(),
            current_dir: "/work".into(),
            caller_identity: TEST_SENDER.parse().unwrap(),
            caller_team: "test-team".parse().unwrap(),
            target_address: None,
            selection_mode: ReadSelection::All,
            seen_state_filter: false,
            limit: None,
            sender_filter: None,
            timestamp_filter: None,
            task_filter: None,
            contains_filter: None,
            task_ledger: view,
        }
    }

    #[test]
    fn classify_maps_each_task_request() {
        let assign = write();
        let mut start = assign.clone();
        start.task_op = Some(TaskOp::Start);
        let mut close = assign.clone();
        close.task_op = Some(TaskOp::Close {
            outcome: TaskCloseOutcome::Completed,
            reason: Some("done".into()),
        });
        let mut legacy = assign.clone();
        legacy.task_complete = legacy.task_id.clone();
        let move_request = TaskMoveRequest {
            caller_identity: TEST_SENDER.parse().unwrap(),
            caller_team: "test-team".parse().unwrap(),
            task_id: "bk-2".parse().unwrap(),
            target: atm_storage::MoveTarget::End,
        };
        let cases = [
            (ApiRequest::Write(Box::new(assign)), "task.assign"),
            (ApiRequest::Write(Box::new(start)), "task.start"),
            (ApiRequest::Write(Box::new(close)), "task.close"),
            (ApiRequest::Write(Box::new(legacy)), "task.close"),
            (ApiRequest::TaskMove(move_request), "task.move"),
            (
                ApiRequest::Messages(Box::new(MessageCollectionRequest::List(query(Some(
                    TaskLedgerQuery::Tasks { member: None },
                ))))),
                "task.list",
            ),
            (
                ApiRequest::Messages(Box::new(MessageCollectionRequest::List(query(Some(
                    TaskLedgerQuery::Events {
                        task_id: "bk-2".parse().unwrap(),
                        member: None,
                    },
                ))))),
                "task.events",
            ),
        ];
        for (request, name) in cases {
            let expected = match &request {
                ApiRequest::Write(value) => serde_json::to_value(value).unwrap(),
                ApiRequest::TaskMove(value) => serde_json::to_value(value).unwrap(),
                ApiRequest::Messages(value) => match value.as_ref() {
                    MessageCollectionRequest::List(value) => serde_json::to_value(value).unwrap(),
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            };
            let command = TaskCommand::classify(request).unwrap();
            assert_eq!(command.hook_name().as_str(), name);
            assert_eq!(command.request_body(), expected);
        }
        for view in [
            None,
            Some(TaskLedgerQuery::History {
                member: None,
                limit: 5,
            }),
        ] {
            assert!(
                TaskCommand::classify(ApiRequest::Messages(Box::new(
                    MessageCollectionRequest::List(query(view))
                )))
                .is_err()
            );
        }
        let mut ordinary = write();
        ordinary.task_id = None;
        assert!(TaskCommand::classify(ApiRequest::Write(Box::new(ordinary))).is_err());
        let mut acknowledged = write();
        acknowledged.acknowledges_message_id = Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".parse().unwrap());
        acknowledged.task_op = Some(TaskOp::Start);
        assert!(TaskCommand::classify(ApiRequest::Write(Box::new(acknowledged))).is_err());
        assert!(matches!(
            TaskCommand::classify(ApiRequest::ReloadRuntimeView),
            Err(ApiRequest::ReloadRuntimeView)
        ));
    }

    #[test]
    fn caller_and_dry_run_cover_every_task_command_variant() {
        let assign = write();
        let mut start = write();
        start.task_op = Some(TaskOp::Start);
        start.caller_identity = "starter".parse().unwrap();
        start.caller_team = "start-team".parse().unwrap();
        start.dry_run = true;
        let mut close = write();
        close.task_op = Some(TaskOp::Close {
            outcome: TaskCloseOutcome::Completed,
            reason: Some("done".into()),
        });
        close.caller_identity = "closer".parse().unwrap();
        close.caller_team = "close-team".parse().unwrap();
        close.dry_run = true;

        let cases = [
            (
                TaskCommand::Assign(TaskAssign(Box::new(assign))),
                TEST_SENDER,
                "test-team",
                false,
            ),
            (
                TaskCommand::Start(TaskStart(Box::new(start))),
                "starter",
                "start-team",
                true,
            ),
            (
                TaskCommand::Close(TaskClose(Box::new(close))),
                "closer",
                "close-team",
                true,
            ),
            (
                TaskCommand::Move(TaskMoveRequest {
                    caller_identity: "mover".parse().unwrap(),
                    caller_team: "move-team".parse().unwrap(),
                    task_id: "bk-2".parse().unwrap(),
                    target: atm_storage::MoveTarget::End,
                }),
                "mover",
                "move-team",
                false,
            ),
            (
                TaskCommand::List(TaskList(query(Some(TaskLedgerQuery::Tasks {
                    member: None,
                })))),
                TEST_SENDER,
                "test-team",
                false,
            ),
            (
                TaskCommand::Events(TaskEvents(query(Some(TaskLedgerQuery::Events {
                    task_id: "bk-2".parse().unwrap(),
                    member: None,
                })))),
                TEST_SENDER,
                "test-team",
                false,
            ),
        ];

        for (command, identity, team, dry_run) in cases {
            let (caller_identity, caller_team) = command.caller();
            assert_eq!(caller_identity.as_str(), identity);
            assert_eq!(caller_team.as_str(), team);
            assert_eq!(command.dry_run(), dry_run);
        }
    }

    #[test]
    fn normalization_error_surfaces_the_existing_validation_error() {
        let mut request = write();
        request.task_complete = Some("different".parse().unwrap());
        let expected = request.task_op_normalized().unwrap_err().to_string();
        let Err(ApiRequest::Write(returned)) =
            TaskCommand::classify(ApiRequest::Write(Box::new(request)))
        else {
            panic!("invalid request was intercepted")
        };
        assert_eq!(
            returned.task_op_normalized().unwrap_err().to_string(),
            expected
        );
        assert!(expected.contains("different tasks"));
    }

    #[test]
    fn call_uses_recording_boundary_and_preserves_body() {
        let commands = RecordingTaskCommands::default();
        *commands.response.lock().unwrap() = Some(ApiResponse::new(
            crate::protocol::ResponseEnvelope::RuntimeViewReloaded,
        ));
        let commands_to_call = [
            TaskCommand::Assign(TaskAssign(Box::new(write()))),
            TaskCommand::Start(TaskStart(Box::new(write()))),
            TaskCommand::Close(TaskClose(Box::new(write()))),
            TaskCommand::Move(TaskMoveRequest {
                caller_identity: TEST_SENDER.parse().unwrap(),
                caller_team: "test-team".parse().unwrap(),
                task_id: "bk-2".parse().unwrap(),
                target: atm_storage::MoveTarget::Head,
            }),
            TaskCommand::List(TaskList(query(Some(TaskLedgerQuery::Tasks {
                member: None,
            })))),
            TaskCommand::Events(TaskEvents(query(Some(TaskLedgerQuery::Events {
                task_id: "bk-2".parse().unwrap(),
                member: None,
            })))),
        ];
        let expected: Vec<_> = commands_to_call
            .iter()
            .map(|command| (command.hook_name(), command.request_body()))
            .collect();
        let ctx = TaskCallCtx {
            caller_identity: TEST_SENDER.parse().unwrap(),
            caller_team: "test-team".parse().unwrap(),
            host: None,
            ingress: AuthenticatedIngress::Local,
            deadline: RequestDeadline::after(std::time::Duration::ZERO),
            request_id: RequestId::new(1).unwrap(),
        };
        for command in commands_to_call {
            let mut future = Box::pin(command.call(&commands, &ctx));
            let mut context = std::task::Context::from_waker(std::task::Waker::noop());
            assert!(matches!(
                std::future::Future::poll(future.as_mut(), &mut context),
                std::task::Poll::Ready(Ok(_))
            ));
        }
        let calls = commands.calls.lock().unwrap();
        assert_eq!(*calls, expected);
    }
}
