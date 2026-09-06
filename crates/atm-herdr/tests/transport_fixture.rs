use std::fs;
use std::path::Path;
use std::time::Duration;

use atm_core::types::AgentName;
use atm_core::{HerdrSession, RequestDeadline};
use atm_herdr::HerdrProcessAdapter;
use atm_herdr::testing::production_invoker_with_test_binary;

fn invoker() -> atm_herdr::HerdrProcessInvoker {
    production_invoker_with_test_binary(env!("CARGO_BIN_EXE_fake-herdr").into())
}

#[tokio::test]
async fn production_transport_decodes_fake_child_output_for_two_sessions() {
    let agent: AgentName = "fixture-agent".parse().expect("agent name");
    let invoker = invoker();
    for session in ["fixture-one", "fixture-two"] {
        let session = HerdrSession::new(session).expect("session");
        let result = invoker
            .get(
                &agent,
                Some(&session),
                RequestDeadline::after(Duration::from_secs(1)),
                atm_herdr::BreakerPolicy::Shared,
            )
            .await
            .expect("fake child response decodes through CliIo");
        assert_eq!(result.snapshot.name.as_deref(), Some("fake"));
    }
}

#[tokio::test]
async fn every_manifest_drives_the_production_invoker() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/herdr-versions");
    let agent: AgentName = "fixture-agent".parse().expect("agent name");
    for entry in fs::read_dir(root).expect("fixture directories") {
        let manifest = entry.expect("directory entry").path().join("manifest.json");
        if !manifest.exists() {
            continue;
        }
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).expect("manifest")).expect("manifest JSON");
        assert!(value.get("mode").is_some() || value.get("delta").is_some());
        invoker()
            .get(
                &agent,
                None,
                RequestDeadline::after(Duration::from_secs(1)),
                atm_herdr::BreakerPolicy::Shared,
            )
            .await
            .expect("manifest fixture routes through production transport");
    }
}
