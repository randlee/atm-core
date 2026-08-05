use std::path::Path;

use crate::boundary::PostSendHookEmitter;
use crate::delivery_execution::{
    DeliveryTransitionContext, emit_delivery_plan_transitions, execute_delivery_plan,
};
use crate::delivery_policy::DeliveryPolicyCoordinator;
use crate::error::AtmError;
use crate::observability::ObservabilityPort;
use crate::schema::AtmMessageId;
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::RetainedMailboxRuntime;
use crate::types::{AgentName, TeamName};

use super::outcome::{SendExecutionContext, build_send_delivery_plan};
use super::{DeliveryPersistenceResult, ResolvedRecipient, hook};

/// Executes local post-write effects from a committed immutable record.
///
/// The daemon calls this only from its post-commit worker. Admission never
/// waits for hook, tmux, or graft I/O.
pub fn emit_persisted_local_post_write(
    runtime: &LocalServiceRuntime,
    observability: &dyn ObservabilityPort,
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
    message_id: AtmMessageId,
    post_send_emitter: &dyn PostSendHookEmitter,
) -> Result<(), AtmError> {
    let key = crate::boundary::MessageKey::from(message_id);
    let Some(record) = runtime.load_message_record(home_dir, team, agent, &key)? else {
        return Ok(());
    };
    let recipient = ResolvedRecipient {
        agent: agent.clone(),
        team: team.clone(),
    };
    let delivery_snapshot =
        DeliveryPolicyCoordinator::new().resolve_recipient_snapshot(runtime, team, agent)?;
    let context = SendExecutionContext {
        #[cfg(test)]
        post_send_config: None,
        recipient: recipient.clone(),
        canonical_sender: record.envelope.from.clone(),
        inbox_path: runtime.inbox_path(home_dir, team, agent)?,
        delivery_snapshot,
        delivery_family: DeliveryPolicyCoordinator::resolve_send_family(
            record.envelope.parent_message_id,
            record.envelope.thread_mode,
        ),
        warnings: Vec::new(),
    };
    let persistence = DeliveryPersistenceResult::persisted(record.envelope.clone());
    let plan = build_send_delivery_plan(
        &context,
        record.envelope.requires_ack,
        record.envelope.acknowledges_message_id.is_some(),
        &persistence,
    )?;
    let execution = execute_delivery_plan(runtime, None, &plan)?;
    emit_delivery_plan_transitions(
        observability,
        DeliveryTransitionContext {
            family: context.delivery_family,
            team,
            agent,
            sender: &record.envelope.from,
            message_id,
            task_id: record.envelope.task_id.clone(),
        },
        &plan,
        &execution,
    )?;
    let mut warnings = Vec::new();
    hook::emit_post_send_effects(
        runtime,
        &mut warnings,
        None,
        Some(post_send_emitter),
        &recipient,
        &context.delivery_snapshot,
        &plan.messages,
    );
    for warning in warnings {
        tracing::warn!(
            code = ?warning.code,
            message_id = %message_id,
            "post-commit local post-write effect completed with warning: {}",
            warning.message
        );
    }
    Ok(())
}
