//! Non-write request dispatch. A task-shaped read or move goes to the installed
//! task commands when a filter is set; every other request keeps its route here,
//! including the hook and cron control routes.

use atm_core::api::{ApiRequest, ApiResponse, AuthenticatedIngress, RequestDeadline};
use atm_core::boundary::TaskCommand;
use atm_core::error::AtmError;
use atm_core::protocol::{RequestId, ResponseEnvelope};

use super::{StorageAndNudgeRouter, compatibility_verdict};

impl StorageAndNudgeRouter {
    pub(super) async fn dispatch_non_write(
        &self,
        request: ApiRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
        request_id: RequestId,
    ) -> Result<ApiResponse, AtmError> {
        if deadline.expired() {
            return Err(AtmError::daemon_unavailable(
                "request deadline expired before replacement route dispatch",
            ));
        }
        let request = match self.hooks.task_commands.clone() {
            None => request,
            Some(commands) => match TaskCommand::classify(request) {
                Ok(command) => {
                    return self
                        .call_task_command(&*commands, command, ingress, deadline, request_id)
                        .await;
                }
                Err(request) => request,
            },
        };
        match request {
            ApiRequest::Write(_) => unreachable!("writes use the canonical write path"),
            ApiRequest::Messages(request) => match *request {
                atm_core::api::MessageCollectionRequest::List(query) => {
                    self.list_messages(query, deadline).await
                }
                atm_core::api::MessageCollectionRequest::Peek(query) => {
                    self.peek_messages(query, deadline).await
                }
                atm_core::api::MessageCollectionRequest::Receive(query) => {
                    self.receive_messages(query, deadline).await
                }
            },
            ApiRequest::Clear(query) => self.clear_messages(query, deadline).await,
            ApiRequest::Doctor(query) => self.doctor(query, deadline).await,
            ApiRequest::Search(request) => self.search(*request, ingress, deadline).await,
            ApiRequest::CompatibilityPreflight(preflight) => Ok(ApiResponse::new(
                ResponseEnvelope::CompatibilityVerdict(compatibility_verdict(preflight)),
            )),
            ApiRequest::Heartbeat(request) => self.heartbeat(request, ingress, deadline).await,
            ApiRequest::QueueGetNext(request) => {
                self.queue_get_next(request, ingress, deadline).await
            }
            ApiRequest::TaskMove(request) => self.task_move(request, ingress, deadline).await,
            ApiRequest::GraftReceiverRegister(request) => {
                self.graft_receiver_register(request, ingress, deadline)
                    .await
            }
            ApiRequest::GraftReceiverRefresh(request) => {
                self.graft_receiver_refresh(request, ingress, deadline)
                    .await
            }
            ApiRequest::GraftReceiverUnregister(request) => {
                self.graft_receiver_unregister(request, ingress, deadline)
                    .await
            }
            ApiRequest::GraftReceiverLookup { team, agent } => {
                self.graft_receiver_lookup(team, agent, ingress, deadline)
                    .await
            }
            ApiRequest::ReloadRuntimeView => self.reload_runtime_view(ingress),
            ApiRequest::HookRegister(request) => {
                self.hook_register(request, ingress, deadline).await
            }
            ApiRequest::HookDisable(request) => self.hook_disable(request, ingress, deadline).await,
            ApiRequest::HookList(request) => self.hook_list(request, ingress, deadline).await,
            ApiRequest::CronStart(request) => self.cron_start(request, ingress, deadline).await,
            ApiRequest::CronEnd(request) => self.cron_end(request, ingress, deadline).await,
            ApiRequest::CronList(request) => self.cron_list(request, ingress, deadline).await,
        }
    }

    pub(super) fn reload_runtime_view_impl(
        &self,
        ingress: AuthenticatedIngress,
    ) -> Result<ApiResponse, AtmError> {
        if ingress != AuthenticatedIngress::Local {
            return Err(AtmError::validation(
                "runtime reload is available only through authenticated local HTTP adapters",
            ));
        }
        // Write-through already keeps the RAM roster mirror synchronized
        // with every durable mutation; this re-hydration is not the
        // synchronization mechanism, it only re-derives RAM from durable
        // state for the rare case of an out-of-band durable change.
        self.service_runtime.reload_roster_from_durable_store()?;
        Ok(ApiResponse::new(ResponseEnvelope::RuntimeViewReloaded))
    }
}
