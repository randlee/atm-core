use super::NotificationRuntime;
use crate::worker_support::{
    reap_retained_join_helpers, reap_retained_join_helpers_until_empty_for_test,
    retained_join_helper_count_for_test,
};
use atm_core::protocol::{NotificationEvent, NotificationKind};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use tempfile::TempDir;

#[test]
#[serial_test::serial(env)]
fn notification_runtime_deliver_uses_bounded_command_channel() {
    reap_retained_join_helpers();
    let tempdir = TempDir::new().expect("tempdir");
    let output_path = tempdir.path().join("notifications.jsonl");
    let entered_gate = Arc::new((Mutex::new(false), Condvar::new()));
    let release_gate = Arc::new((Mutex::new(false), Condvar::new()));
    let runtime = NotificationRuntime::new_for_test_with_path_factory_and_deadline(
        Arc::new({
            let entered_gate = Arc::clone(&entered_gate);
            let release_gate = Arc::clone(&release_gate);
            move || {
                let (entered_lock, entered_wake) = &*entered_gate;
                let mut entered = entered_lock.lock().expect("entered gate lock");
                *entered = true;
                entered_wake.notify_all();
                drop(entered);

                let (release_lock, release_wake) = &*release_gate;
                let mut released = release_lock.lock().expect("release gate lock");
                while !*released {
                    released = release_wake.wait(released).expect("release gate wait");
                }

                Ok(output_path.clone())
            }
        }),
        1,
        Duration::from_secs(1),
    );
    runtime.start().expect("start");
    runtime
        .deliver(notification_event("first"))
        .expect("first deliver");

    {
        let (entered_lock, entered_wake) = &*entered_gate;
        let entered = entered_lock.lock().expect("entered gate lock");
        let (_entered_guard, wait_result) = entered_wake
            .wait_timeout_while(entered, Duration::from_secs(1), |entered| !*entered)
            .expect("entered gate wait");
        assert!(
            !wait_result.timed_out(),
            "worker never entered path factory"
        );
    }

    runtime
        .deliver(notification_event("second"))
        .expect("second deliver fills bounded channel");
    let error = runtime
        .deliver(notification_event("third"))
        .expect_err("third deliver should backpressure");
    assert!(error.message.contains("queue is full"));

    {
        let (release_lock, release_wake) = &*release_gate;
        let mut released = release_lock.lock().expect("release gate lock");
        *released = true;
        release_wake.notify_all();
    }
    runtime
        .shutdown()
        .unwrap_or_else(|error| panic!("shutdown failed: {error}"));
}

#[test]
#[serial_test::serial(env)]
fn notification_runtime_persistence_failure_publishes_degraded_status() {
    reap_retained_join_helpers();
    let tempdir = TempDir::new().expect("tempdir");
    let blocking_path = tempdir.path().join("blocking-file");
    std::fs::write(&blocking_path, "not-a-dir").expect("blocking file");
    let output_path = blocking_path.join("notifications.jsonl");
    let runtime = NotificationRuntime::new_for_test_with_path(output_path, 8);
    runtime.start().expect("start");
    runtime
        .deliver(notification_event("message delivered"))
        .expect("first deliver queues");
    runtime
        .shutdown()
        .unwrap_or_else(|error| panic!("shutdown failed: {error}"));

    let error = runtime
        .deliver(notification_event("message delivered"))
        .expect_err("degraded");
    assert!(
        error
            .message
            .contains("notification runtime is unavailable")
    );
}

#[test]
#[serial_test::serial(env)]
fn notification_runtime_shutdown_stays_bounded_after_worker_backpressure() {
    reap_retained_join_helpers();
    let tempdir = TempDir::new().expect("tempdir");
    let output_path = tempdir.path().join("notifications.jsonl");
    let blocked_output_path = output_path.clone();
    let entered_gate = Arc::new((Mutex::new(false), Condvar::new()));
    let release_gate = Arc::new((Mutex::new(false), Condvar::new()));
    let (worker_done_tx, worker_done_rx) = std::sync::mpsc::sync_channel(1);
    let runtime = NotificationRuntime::new_for_test_with_path_factory_and_deadline(
        Arc::new({
            let entered_gate = Arc::clone(&entered_gate);
            let release_gate = Arc::clone(&release_gate);
            let worker_done_tx = worker_done_tx.clone();
            move || {
                let (entered_lock, entered_wake) = &*entered_gate;
                let mut entered = entered_lock.lock().expect("entered gate lock");
                *entered = true;
                entered_wake.notify_all();
                drop(entered);

                let (release_lock, release_wake) = &*release_gate;
                let mut released = release_lock.lock().expect("release gate lock");
                while !*released {
                    released = release_wake.wait(released).expect("release gate wait");
                }
                drop(released);
                worker_done_tx.send(()).expect("worker done");

                Ok(blocked_output_path.clone())
            }
        }),
        8,
        Duration::from_millis(25),
    );
    runtime.start().expect("start");
    runtime
        .deliver(notification_event("message delivered"))
        .expect("deliver");

    {
        let (entered_lock, entered_wake) = &*entered_gate;
        let entered = entered_lock.lock().expect("entered gate lock");
        let (_entered_guard, wait_result) = entered_wake
            .wait_timeout_while(entered, Duration::from_secs(1), |entered| !*entered)
            .expect("entered gate wait");
        assert!(
            !wait_result.timed_out(),
            "worker never entered path factory"
        );
    }

    let error = runtime.shutdown().expect_err("shutdown should time out");
    assert_eq!(retained_join_helper_count_for_test(), 1);

    let recovery_runtime = NotificationRuntime::new_for_test_with_path(output_path.clone(), 8);
    recovery_runtime.start().expect("recovery start");
    recovery_runtime.shutdown().expect("recovery shutdown");

    {
        let (release_lock, release_wake) = &*release_gate;
        let mut released = release_lock.lock().expect("release gate lock");
        *released = true;
        release_wake.notify_all();
    }
    assert!(
        error
            .message
            .contains("notification runtime shutdown exceeded")
    );
    worker_done_rx.recv().expect("worker done recv");
    reap_retained_join_helpers_until_empty_for_test();
}

fn notification_event(detail: &str) -> NotificationEvent {
    NotificationEvent {
        kind: NotificationKind::Delivery,
        detail: detail.to_string(),
        team: None,
        agent: None,
    }
}
