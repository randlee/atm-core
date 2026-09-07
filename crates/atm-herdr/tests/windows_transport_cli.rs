#![cfg(all(windows, feature = "test-utils"))]

use std::time::Duration;

use atm_core::RequestDeadline;
use atm_core::types::AgentName;
use atm_herdr::testing::production_invoker_with_test_binary_and_environment;
use atm_herdr::{HerdrAgentStatus, HerdrError, HerdrProcessAdapter};

fn invoker(mode: &str) -> atm_herdr::HerdrProcessInvoker {
    production_invoker_with_test_binary_and_environment(
        env!("CARGO_BIN_EXE_fake-herdr").into(),
        vec![("FAKE_HERDR_MODE".to_owned(), mode.to_owned())],
    )
}

async fn get(
    invoker: &atm_herdr::HerdrProcessInvoker,
    deadline: RequestDeadline,
) -> Result<atm_herdr::HerdrGetOutcome, HerdrError> {
    let agent: AgentName = "windows-fixture".parse().expect("agent name");
    invoker
        .get(&agent, None, deadline, atm_herdr::BreakerPolicy::Shared)
        .await
}

#[tokio::test]
async fn windows_cli_process_accepts_crlf_json_output() {
    let result = get(
        &invoker("stdout-json-line"),
        RequestDeadline::after(Duration::from_secs(1)),
    )
    .await
    .expect("CRLF-delimited UTF-8 JSON response");
    assert_eq!(result.snapshot.status, HerdrAgentStatus::Working);
}

#[tokio::test]
async fn windows_cli_process_deadline_uses_bounded_kill_then_reap() {
    let result = get(
        &invoker("sleep-past-deadline"),
        RequestDeadline::after(Duration::from_millis(50)),
    )
    .await;
    assert_eq!(result, Err(HerdrError::TimedOut));
}
