//! Production adapters for persisted cron delivery intents.

use std::sync::Arc;

use atm_core::boundary::{
    BuiltInNudgeTemplateKind, CronLifecycleNudge, CronLifecycleNudgeEmitter, CronLifecycleNudger,
    CronModuleMailDelivery, CronModuleMailIntent, CronNoticeIntent, CronRunPermit, DeliveryReceipt,
    ResolvedBuiltInNudgeTemplate, sealed,
};
use atm_core::error::AtmError;
use atm_core::send::{default_template, render_cron_lifecycle_nudge};

use crate::StorageAndNudgeRouter;

/// Routes committed lifecycle notices through the dedicated direct-prompt port.
#[derive(Clone)]
pub struct CronLifecycleNudgerAdapter {
    router: StorageAndNudgeRouter,
    emitter: Arc<dyn CronLifecycleNudgeEmitter>,
}

impl CronLifecycleNudgerAdapter {
    #[must_use]
    pub fn new(router: StorageAndNudgeRouter, emitter: Arc<dyn CronLifecycleNudgeEmitter>) -> Self {
        Self { router, emitter }
    }
}

#[async_trait::async_trait]
impl CronLifecycleNudger for CronLifecycleNudgerAdapter {
    async fn deliver(&self, intent: &CronNoticeIntent) -> Result<DeliveryReceipt, AtmError> {
        if let Err(error) = self
            .router
            .check_roster_member(intent.team(), intent.recipient())
        {
            if error.code() == atm_core::error::AtmErrorCode::AgentNotFound {
                return Ok(DeliveryReceipt::RecipientUnavailable);
            }
            return Err(error);
        }
        let override_row = self
            .router
            .service_runtime
            .load_nudge_template_override(intent.team(), BuiltInNudgeTemplateKind::Cron)?;
        let body = match override_row {
            Some(row) if row.is_disabled() => None,
            Some(row) => row.template_body().map(ToOwned::to_owned),
            None => Some(default_template(BuiltInNudgeTemplateKind::Cron).to_owned()),
        };
        let template = ResolvedBuiltInNudgeTemplate {
            kind: BuiltInNudgeTemplateKind::Cron,
            body,
        };
        let Some(rendered_nudge) =
            render_cron_lifecycle_nudge(&template, intent.name(), intent.event())?
        else {
            return Ok(DeliveryReceipt::Suppressed);
        };
        let Some(target) = self
            .router
            .service_runtime
            .resolve_cron_lifecycle_nudge_target(
                intent.team(),
                intent.recipient(),
                rendered_nudge.clone(),
            )?
        else {
            return Ok(DeliveryReceipt::RecipientUnavailable);
        };
        self.emitter
            .emit(
                &CronLifecycleNudge {
                    key: intent.key(),
                    team: intent.team().clone(),
                    recipient: intent.recipient().clone(),
                    rendered_nudge,
                },
                &target,
            )
            .await
    }
}

/// Delivers module output as ordinary canonical mail under the run permit.
#[derive(Clone)]
pub struct CronModuleMailDeliveryAdapter {
    router: StorageAndNudgeRouter,
}

impl CronModuleMailDeliveryAdapter {
    #[must_use]
    pub fn new(router: StorageAndNudgeRouter) -> Self {
        Self { router }
    }
}

#[async_trait::async_trait]
impl CronModuleMailDelivery for CronModuleMailDeliveryAdapter {
    async fn deliver(
        &self,
        intent: &CronModuleMailIntent,
        permit: &CronRunPermit,
    ) -> Result<DeliveryReceipt, AtmError> {
        self.router.deliver_cron_module_mail(intent, permit).await
    }
}

impl sealed::Sealed for CronLifecycleNudgerAdapter {}
impl sealed::Sealed for CronModuleMailDeliveryAdapter {}
