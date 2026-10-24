//! Generation-fenced cron schedule admission for dispatched actions.

use super::*;

impl StorageAndNudgeRouter {
    pub(crate) async fn dispatch_with_cron_permit(
        &self,
        request: ApiRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
        request_id: RequestId,
        permit_context: CronPermitContext<'_>,
    ) -> Result<ApiResponse, AtmError> {
        self.admit_cron_run_permit(
            permit_context.permit,
            permit_context.team,
            permit_context.phase,
            deadline,
        )
        .await?;
        match request {
            ApiRequest::Write(request) => {
                self.write_unhooked_with_cron_permit(
                    *request,
                    ingress,
                    deadline,
                    request_id,
                    Some(permit_context.permit),
                )
                .await
            }
            ApiRequest::TaskMove(request) => {
                self.task_move(request, ingress, deadline, Some(permit_context.permit))
                    .await
            }
            ApiRequest::Clear(query) => {
                let origin = atm_core::send::cron_run_message_write_origin(permit_context.permit);
                self.clear_messages(query, deadline, origin).await
            }
            ApiRequest::Messages(request) => match *request {
                atm_core::api::MessageCollectionRequest::Receive(query) => {
                    self.receive_messages_with_origin(
                        query,
                        deadline,
                        atm_core::send::cron_run_message_write_origin(permit_context.permit),
                    )
                    .await
                }
                request => {
                    self.dispatch_with_request_id(
                        ApiRequest::Messages(Box::new(request)),
                        ingress,
                        deadline,
                        request_id,
                    )
                    .await
                }
            },
            request => {
                self.dispatch_with_request_id(request, ingress, deadline, request_id)
                    .await
            }
        }
    }

    pub(crate) async fn deliver_cron_module_mail(
        &self,
        intent: &atm_core::boundary::CronModuleMailIntent,
        permit: &atm_core::boundary::CronRunPermit,
    ) -> Result<atm_core::boundary::DeliveryReceipt, AtmError> {
        use atm_core::boundary::{CronDeliveryKey, CronModuleOutput};

        let CronDeliveryKey::ModuleMail {
            schedule_id,
            generation,
            ..
        } = intent.key
        else {
            return Err(AtmError::validation(
                "cron module mail intent has a lifecycle key",
            ));
        };
        if schedule_id != permit.schedule_id() || generation != permit.generation() {
            return Err(AtmError::validation(
                "cron module mail intent does not match its run permit",
            ));
        }
        if let Err(error) = self.check_roster_member(&intent.team, &intent.recipient) {
            if error.code() == atm_core::error::AtmErrorCode::AgentNotFound {
                return Ok(atm_core::boundary::DeliveryReceipt::RecipientUnavailable);
            }
            return Err(error);
        }
        let body = match &intent.output {
            CronModuleOutput::Message { body } | CronModuleOutput::Error { body } => body.as_str(),
        };
        let to = format!("{}@{}", intent.recipient, intent.team);
        let request = atm_core::send::WriteRequest::new(
            self.daemon_home.clone(),
            self.daemon_home.clone(),
            atm_core::types::AgentName::from_validated(atm_core::boundary::DAEMON_ACTOR_NAME),
            &to,
            intent.team.clone(),
            SendMessageSource::Inline(body.to_owned()),
            None,
            false,
            None,
            false,
        )?;
        let deadline = RequestDeadline::after(std::time::Duration::from_secs(30));
        let prepared = self
            .prepare_and_commit_write(
                request,
                deadline,
                None,
                Some(permit),
                Some(intent.message_id),
            )
            .await?;
        let delivered = prepared.newly_persisted;
        let _ = self
            .finish_write_delivery(
                prepared,
                AuthenticatedIngress::Local,
                deadline,
                atm_core::protocol::next_request_id(),
            )
            .await?;
        Ok(if delivered {
            atm_core::boundary::DeliveryReceipt::Accepted {
                receipt_id: intent.message_id.to_string(),
            }
        } else {
            atm_core::boundary::DeliveryReceipt::Suppressed
        })
    }

    pub(crate) async fn admit_cron_run_permit(
        &self,
        permit: &atm_core::boundary::CronRunPermit,
        team: &atm_core::types::TeamName,
        phase: &atm_core::boundary::PhaseId,
        deadline: RequestDeadline,
    ) -> Result<(), AtmError> {
        if deadline.expired() {
            return Err(AtmError::daemon_unavailable(
                "cron action deadline expired before permit admission",
            ));
        }
        let store = self.service_runtime.cron_schedule_store()?;
        let schedule_id = permit.schedule_id();
        let generation = permit.generation();
        let team = team.clone();
        let phase = phase.clone();
        let remaining = deadline.remaining().ok_or_else(|| {
            AtmError::daemon_unavailable("cron action deadline expired before permit admission")
        })?;
        tokio::time::timeout(remaining, tokio::task::spawn_blocking(move || {
                let schedules = store.active_all()?;
                let admitted = schedules.iter().any(|schedule| {
                    schedule.active
                        && schedule.id == schedule_id
                        && schedule.generation == generation
                        && schedule.team == team
                        && schedule.phase == phase
                });
                if admitted {
                    Ok(())
                } else {
                    Err(AtmError::validation_with_recovery(
                        "cron action permit is stale or its schedule is no longer active",
                        "allow the current cron run to stop and let the scheduler start a fresh generation",
                    ))
                }
            }))
            .await
            .map_err(|_| AtmError::daemon_unavailable("cron action permit admission timed out"))?
            .map_err(|_| AtmError::daemon_unavailable("cron action permit admission task failed"))?
    }
}
