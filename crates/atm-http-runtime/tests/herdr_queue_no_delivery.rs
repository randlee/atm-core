// Behavioral coverage for the BA.3 no-delivery-channel Hold contract.

use super::*;

use atm_core::boundary::{MailboxScope, MessageQuery, ReadDeadline, TaskStore};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

struct WarningCounter(Arc<AtomicUsize>);

impl<S> Layer<S> for WarningCounter
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if *event.metadata().level() == Level::WARN {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[tokio::test]
async fn member_without_dispatchable_backend_holds_and_logs_once_per_tick() {
    let (_root, runtime, fake, pump, store, keys, _now) =
        build_task_only_pump_without_delivery_channel();
    let key = &keys[0];
    let task_id: TaskId = "AX5-TASK-00".parse().expect("task id");
    let warnings = Arc::new(AtomicUsize::new(0));
    let _subscriber = tracing::subscriber::set_default(
        tracing_subscriber::registry().with(WarningCounter(Arc::clone(&warnings))),
    );

    for tick in 0..2 {
        if tick > 0 {
            queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
        }
        pump.tick_once().await;
        assert_eq!(
            warnings.load(Ordering::SeqCst),
            tick + 1,
            "one no-delivery Hold warning per tick"
        );
    }

    assert!(fake.calls().iter().all(|call| {
        !matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. })
    }));
    let row = store
        .load_task(key.team(), &task_id)
        .expect("load task")
        .expect("task row");
    assert_eq!(row.reminder_count, 0, "a held task has no reminder audit");
    assert!(store
        .list_task_events(key.team(), &task_id, Some(key.agent()))
        .expect("task events")
        .is_empty());

    let reader = runtime.async_mailbox_reader().expect("mailbox reader");
    let messages = reader
        .list_messages(
            MailboxScope::new(key.team().clone(), "sender".parse().expect("sender")),
            MessageQuery {
                team: key.team().clone(),
                agent: "sender".parse().expect("sender"),
                sender: None,
                task_id: None,
                limit: None,
            },
            ReadDeadline::new(Duration::from_secs(1)).expect("read deadline"),
        )
        .await
        .expect("mailbox messages");
    assert!(messages.is_empty(), "a held task sends no receipt or mail");
}
