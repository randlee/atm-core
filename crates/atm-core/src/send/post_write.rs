use std::path::Path;

use crate::boundary::BuiltInPostSendDispatch;
use crate::delivery_plan::DeliveryPlan;
use crate::delivery_policy::DeliveryPolicyCoordinator;
use crate::error::AtmError;
use crate::schema::AtmMessageId;
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::RetainedMailboxRuntime;
use crate::types::{AgentName, TeamName};

use super::{
    DeliveryPersistenceResult, ResolvedRecipient, SendExecutionContext, build_send_delivery_plan,
    hook,
};

struct CommittedPostWrite {
    context: SendExecutionContext,
    plan: DeliveryPlan,
}

fn load_committed_post_write(
    runtime: &LocalServiceRuntime,
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
    message_id: AtmMessageId,
) -> Result<Option<CommittedPostWrite>, AtmError> {
    let key = crate::boundary::MessageKey::from(message_id);
    let Some(record) = runtime.load_message_record(home_dir, team, agent, &key)? else {
        return Ok(None);
    };
    let recipient = ResolvedRecipient {
        agent: agent.clone(),
        team: team.clone(),
    };
    let delivery_snapshot =
        DeliveryPolicyCoordinator::new().resolve_recipient_snapshot(runtime, team, agent)?;
    let context = SendExecutionContext {
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
    Ok(Some(CommittedPostWrite { context, plan }))
}

/// Builds the injected receiver-hook dispatches for one committed message.
///
/// This is the replacement-runtime planning seam: it reads only durable core
/// state and returns no task, thread, or hook side effect. The Tokio runtime
/// owns awaiting the injected asynchronous emitter and applies the inherited
/// request deadline there.
pub fn build_received_message_hook_dispatches_after_commit(
    runtime: &LocalServiceRuntime,
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
    message_id: AtmMessageId,
) -> Result<Vec<BuiltInPostSendDispatch>, AtmError> {
    let Some(committed) = load_committed_post_write(runtime, home_dir, team, agent, message_id)?
    else {
        return Ok(Vec::new());
    };
    let mut dispatches = Vec::new();
    for message in &committed.plan.messages {
        let event = hook::post_send_event_from_message(
            &committed.context.recipient,
            message,
            committed
                .context
                .delivery_snapshot
                .recipient_pane_id
                .as_ref(),
        )?;
        if let Some(dispatch) =
            hook::build_built_in_dispatch(runtime, &committed.context.delivery_snapshot, &event)
        {
            dispatches.push(dispatch);
        }
    }
    Ok(dispatches)
}
