#![cfg(feature = "test-utils")]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use atm_core::types::AgentName;
use atm_core::{HerdrSession, RequestDeadline};
use atm_herdr::testing::production_invoker_with_test_binary_and_environment;
use atm_herdr::{HerdrError, HerdrProcessAdapter};

#[derive(Debug)]
struct FixtureManifest {
    mode: String,
    delta: Option<String>,
    response: Option<String>,
}

impl FixtureManifest {
    fn parse(bytes: &[u8]) -> Self {
        let value: serde_json::Value = serde_json::from_slice(bytes).expect("manifest JSON");
        Self {
            mode: value
                .get("mode")
                .and_then(serde_json::Value::as_str)
                .expect("manifest mode")
                .to_owned(),
            delta: value
                .get("delta")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            response: value
                .get("response")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
        }
    }
}

fn invoker(mode: String, delta: Option<&str>) -> atm_herdr::HerdrProcessInvoker {
    let mut environment = vec![("FAKE_HERDR_MODE".to_owned(), mode)];
    if let Some(delta) = delta {
        environment.push(("FAKE_HERDR_DELTA".to_owned(), delta.to_owned()));
    }
    production_invoker_with_test_binary_and_environment(
        env!("CARGO_BIN_EXE_fake-herdr").into(),
        environment,
    )
}

fn fixture_manifests() -> Vec<(PathBuf, FixtureManifest)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/herdr-versions");
    let mut fixtures = fs::read_dir(root)
        .expect("fixture directories")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter_map(|directory| {
            let manifest = directory.join("manifest.json");
            manifest.exists().then(|| {
                let bytes = fs::read(manifest).expect("manifest");
                let manifest = FixtureManifest::parse(&bytes);
                (directory, manifest)
            })
        })
        .collect::<Vec<_>>();
    fixtures.sort_by(|(left, _), (right, _)| left.cmp(right));
    fixtures
}

fn manifest_mode(directory: &Path, manifest: &FixtureManifest) -> String {
    manifest.response.as_ref().map_or_else(
        || manifest.mode.clone(),
        |_| format!("{}:{}", manifest.mode, directory.display()),
    )
}

async fn get(
    invoker: &atm_herdr::HerdrProcessInvoker,
    session: Option<&HerdrSession>,
) -> Result<atm_herdr::HerdrGetOutcome, HerdrError> {
    let agent: AgentName = "fixture-agent".parse().expect("agent name");
    invoker
        .get(
            &agent,
            session,
            RequestDeadline::after(Duration::from_secs(1)),
            atm_herdr::BreakerPolicy::Shared,
        )
        .await
}

#[tokio::test]
async fn production_transport_round_trips_argv_and_two_sessions() {
    let invoker = invoker("echo-argv-and-herdr-session".to_owned(), None);
    for session in ["fixture-one", "fixture-two"] {
        let session = HerdrSession::new(session).expect("session");
        let result = get(&invoker, Some(&session))
            .await
            .expect("fake child response decodes through CliIo");
        let expected_name = format!("fixture-agent:{session}");
        assert_eq!(
            result.snapshot.name.as_deref(),
            Some(expected_name.as_str())
        );
    }
}

#[tokio::test]
async fn every_manifest_drives_its_declared_mode_and_delta() {
    for (directory, manifest) in fixture_manifests() {
        let invoker = invoker(
            manifest_mode(&directory, &manifest),
            manifest.delta.as_deref(),
        );
        let result = get(&invoker, None).await;
        if manifest.delta.is_some() {
            assert_eq!(result, Err(HerdrError::AgentPromptStalled));
        } else {
            assert_eq!(
                result
                    .expect("manifest fixture response")
                    .snapshot
                    .name
                    .as_deref(),
                Some("fake"),
            );
        }
    }
}

#[tokio::test]
async fn production_transport_tolerates_unknown_fields_and_crlf() {
    let result = get(&invoker("stdout-json-line".to_owned(), None), None)
        .await
        .expect("CRLF JSON with unknown fields");
    assert_eq!(result.snapshot.status, atm_herdr::HerdrAgentStatus::Working);
}

#[tokio::test]
async fn replay_runs_twice_through_one_production_invoker() {
    for (directory, manifest) in fixture_manifests() {
        if manifest.response.is_none() {
            continue;
        }
        let invoker = invoker(
            manifest_mode(&directory, &manifest),
            manifest.delta.as_deref(),
        );
        let first = get(&invoker, None).await.expect("first replay");
        let second = get(&invoker, None).await.expect("second replay");
        assert_eq!(first, second);
    }
}
