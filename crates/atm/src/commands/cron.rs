use std::path::PathBuf;

use anyhow::Result;
use atm_core::boundary::{PhaseId, StopCondition};
use atm_core::caller_context::{CallerContext, CallerContextOverrides, resolve_cli_caller_context};
use atm_core::hook::{CronEndRequest, CronListQuery, CronStartRequest};
use atm_core::protocol::{HttpApiVersion, RequestEnvelope, ResponseEnvelope};
use chrono::{DateTime, Utc};
use clap::{Args, Parser, Subcommand};
use serde_json::{Map, Value};

use super::hook_module::{
    canonical_paths, parse_custom_object, parse_rfc3339_utc, read_cron_timeout_ms,
};
use super::send::preflight_daemon_api;
use crate::composition::{
    AtmHomePath, CliComposition, InvocationDir, resolve_command_runtime_context,
};
use crate::observability::CliObservability;

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::super::hook::tests::{caller, fixture, transport};
    use super::*;
    use atm_core::list::ListQuery;
    use atm_core::read::ReadQuery;
    use atm_core::test_support::{TEST_RECIPIENT, TEST_SENDER, TEST_TEAM};
    use atm_core::types::ReadSelection;
    use atm_daemon_bootstrap::test_support::ControlledCronComposition;

    fn start(module: &str) -> CronCommand {
        CronCommand::try_parse_from([
            "cron",
            "start",
            "goal",
            "--cron",
            "0 */5 * * * *",
            "--phase",
            TEST_TEAM,
            "--module",
            fixture(module).to_str().unwrap(),
            "--until-hook",
            "--arg",
            "status",
            "--arg",
            "--json",
        ])
        .unwrap()
    }

    fn row() -> atm_core::boundary::CronSchedule {
        serde_json::from_str(&format!(
            r#"{{"id":1,"generation":1,"team":"{TEST_TEAM}","phase":"{TEST_TEAM}","name":"goal","cron_expr":"0 */5 * * * *","module_path":"hook.py","cwd":"/repo","args":["status","--json"],"timeout_ms":2500,"jitter_s":7,"custom":{{"goal":"ship"}},"stop":{{"kind":"until","at":"2026-11-01T02:00:00+02:00"}},"active":true,"created_by":"{TEST_SENDER}","created_at":"2026-10-19T00:00:00Z","last_run_at":null,"last_outcome":null,"ended_at":null}}"#
        ))
        .unwrap()
    }

    async fn execute(
        command: CronCommand,
        response: ResponseEnvelope,
    ) -> (Result<String>, Vec<RequestEnvelope>) {
        let (transport, recorded) = transport(response);
        let observability = CliObservability::fallback();
        let composition = CliComposition::from_fake_transport(transport, &observability);
        let result = match command.request(caller()) {
            Ok(request) => command.execute_request(&composition, request).await,
            Err(error) => Err(error),
        };
        let requests = recorded.lock().unwrap().clone();
        (result, requests)
    }

    async fn execute_composed(
        composition: &ControlledCronComposition,
        command: &CronCommand,
    ) -> ResponseEnvelope {
        composition
            .execute(command.request(caller()).expect("CLI request"))
            .await
            .expect("composed daemon request")
    }

    async fn assert_empty_mailbox(composition: &ControlledCronComposition, target: &str) {
        let ResponseEnvelope::List(outcome) = composition
            .execute(RequestEnvelope::List(
                ListQuery::new(
                    composition.root_path().into(),
                    composition.root_path().into(),
                    TEST_SENDER.parse().expect("test sender"),
                    Some(target),
                    TEST_TEAM.parse().expect("test team"),
                    ReadSelection::All,
                    false,
                    None,
                    None,
                    None,
                    None,
                    None,
                )
                .expect("list request"),
            ))
            .await
            .expect("list mailbox")
        else {
            panic!("list response")
        };
        assert_eq!(outcome.count, 0, "rejected cron output is not persisted");
    }

    fn deferred_cron_start(
        name: &str,
        module: &std::path::Path,
        cwd: &std::path::Path,
    ) -> CronCommand {
        CronCommand::try_parse_from([
            "cron",
            "start",
            name,
            "--cron",
            "0 * * * * *",
            "--phase",
            TEST_TEAM,
            "--module",
            module.to_str().expect("fixture module utf-8"),
            "--cwd",
            cwd.to_str().expect("fixture root utf-8"),
            "--until",
            "2026-10-24T03:00:00Z",
        ])
        .expect("documented deferred fixture parses")
    }

    fn manifest_fixture_ids(root: &std::path::Path) -> BTreeSet<String> {
        let manifest: Value = serde_json::from_slice(
            &std::fs::read(root.join("docs/user-documents/examples/cron/manifest.json"))
                .expect("read cron fixture manifest"),
        )
        .expect("parse cron fixture manifest");
        manifest["commands"]
            .as_array()
            .expect("commands array")
            .iter()
            .map(|fixture| fixture["id"].as_str().expect("fixture id").to_owned())
            .collect()
    }

    #[tokio::test]
    async fn start_sends_args_timeout_and_stop() {
        let (result, requests) =
            execute(start("valid_hook.py"), ResponseEnvelope::CronStarted(row())).await;
        assert!(result.unwrap().contains("started goal"));
        let [RequestEnvelope::CronStart(request)] = requests.as_slice() else {
            panic!("{requests:?}")
        };
        assert_eq!(request.args, ["status", "--json"]);
        assert_eq!(request.cron_expr, "0 */5 * * * *");
        assert_eq!(request.timeout_ms, 2500);
        assert_eq!(request.stop, StopCondition::Hook);
        assert_eq!(
            request.module_path,
            fixture("valid_hook.py").canonicalize().unwrap()
        );
        assert_eq!(request.cwd, request.module_path.parent().unwrap());
        assert_eq!(request.custom, serde_json::json!({}));
    }

    #[tokio::test]
    async fn list_prints_stop_until_in_utc() {
        let command = CronCommand::try_parse_from(["cron", "list"]).unwrap();
        let output = execute(command, ResponseEnvelope::CronList(vec![row()]))
            .await
            .0
            .unwrap();
        assert!(output.contains("until 2026-11-01T00:00:00Z"));
        assert!(output.contains("JITTER_S") && output.contains("1 keys"));
    }

    #[tokio::test]
    async fn cron_list_empty_prints_none_and_json_brackets() {
        for (args, expected) in [
            (vec!["cron", "list"], "none\n"),
            (vec!["cron", "list", "--json"], "[]\n"),
        ] {
            assert_eq!(
                execute(
                    CronCommand::try_parse_from(args).unwrap(),
                    ResponseEnvelope::CronList(vec![])
                )
                .await
                .0
                .unwrap(),
                expected
            );
        }
    }

    #[tokio::test]
    async fn cron_end_with_nothing_active_prints_none_and_exits_zero() {
        let command = CronCommand::try_parse_from(["cron", "end", "--phase", TEST_TEAM]).unwrap();
        let (result, requests) = execute(command, ResponseEnvelope::CronEnded(vec![])).await;
        assert_eq!(
            result.unwrap(),
            format!("no active schedules for phase {TEST_TEAM}\n")
        );
        let [RequestEnvelope::CronEnd(request)] = requests.as_slice() else {
            panic!("{requests:?}")
        };
        assert_eq!(request.phase.as_str(), TEST_TEAM);
        assert_eq!(request.name, None);
    }

    #[tokio::test]
    async fn start_refuses_missing_module_and_missing_cwd_before_any_request() {
        for missing_module in [true, false] {
            let mut command = start("valid_hook.py");
            let CronSubcommand::Start(args) = &mut command.command else {
                unreachable!()
            };
            if missing_module {
                args.module = fixture("missing.py");
            } else {
                args.cwd = Some(fixture("missing-directory"));
            }
            let (result, requests) = execute(command, ResponseEnvelope::CronStarted(row())).await;
            assert_eq!(
                result
                    .unwrap_err()
                    .downcast_ref::<atm_core::error::AtmError>()
                    .unwrap()
                    .code(),
                atm_core::error::AtmErrorCode::HookModuleInvalid
            );
            assert!(requests.is_empty());
        }
    }

    #[tokio::test]
    async fn cron_timeout_defaults_to_timeout_ms() {
        for (module, expected) in [("valid_hook.py", 2500), ("cron_timeout.py", 20000)] {
            let (result, requests) =
                execute(start(module), ResponseEnvelope::CronStarted(row())).await;
            result.unwrap();
            let [RequestEnvelope::CronStart(request)] = requests.as_slice() else {
                panic!("{requests:?}")
            };
            assert_eq!(request.timeout_ms, expected);
        }
    }

    #[tokio::test]
    async fn cron_start_over_cap_sends_and_prints_the_daemon_refusal() {
        let error = atm_core::error::AtmError::hook_module_invalid("cron cap rejected by daemon");
        let (result, requests) = execute(
            start("cron_over_cap.py"),
            ResponseEnvelope::Error(error.clone()),
        )
        .await;
        assert_eq!(result.unwrap_err().to_string(), error.to_string());
        let [RequestEnvelope::CronStart(request)] = requests.as_slice() else {
            panic!("{requests:?}")
        };
        assert_eq!(request.timeout_ms, 30001);
    }

    #[tokio::test]
    async fn custom_flag_reaches_the_request() {
        let hook = crate::commands::hook::HookCommand::try_parse_from([
            "hook",
            "register",
            "task.close",
            "--phase",
            TEST_TEAM,
            "--module",
            fixture("valid_hook.py").to_str().unwrap(),
            "--custom",
            r#"{"goal":"ship"}"#,
        ])
        .unwrap();
        let (result, requests) = super::super::hook::tests::execute(
            hook,
            ResponseEnvelope::HookRegistered(super::super::hook::tests::row()),
        )
        .await;
        result.unwrap();
        let [RequestEnvelope::HookRegister(request)] = requests.as_slice() else {
            panic!("{requests:?}")
        };
        assert_eq!(request.custom, serde_json::json!({"goal":"ship"}));
        let command = CronCommand::try_parse_from([
            "cron",
            "start",
            "goal",
            "--phase",
            TEST_TEAM,
            "--module",
            fixture("valid_hook.py").to_str().unwrap(),
            "--cron",
            "0 * * * * *",
            "--until-hook",
            "--custom",
            r#"{"goal":"ship"}"#,
        ])
        .unwrap();
        let (result, requests) = execute(command, ResponseEnvelope::CronStarted(row())).await;
        result.unwrap();
        let [RequestEnvelope::CronStart(request)] = requests.as_slice() else {
            panic!("{requests:?}")
        };
        assert_eq!(request.custom, serde_json::json!({"goal":"ship"}));
        let command = CronCommand::try_parse_from(["cron", "list", "--json"]).unwrap();
        let output = execute(command, ResponseEnvelope::CronList(vec![row()]))
            .await
            .0
            .unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value[0]["custom"], request.custom);
        assert_eq!(value[0]["jitter_s"], 7);
    }

    #[test]
    fn custom_non_object_refused() {
        for raw in ["[1]", "x"] {
            let error = CronCommand::try_parse_from([
                "cron",
                "start",
                "goal",
                "--phase",
                TEST_TEAM,
                "--module",
                "hook.py",
                "--cron",
                "0 * * * * *",
                "--until-hook",
                "--custom",
                raw,
            ])
            .unwrap_err();
            assert!(error.to_string().contains("--custom"));
            let error = crate::commands::hook::HookCommand::try_parse_from([
                "hook",
                "register",
                "task.close",
                "--phase",
                TEST_TEAM,
                "--module",
                "hook.py",
                "--custom",
                raw,
            ])
            .unwrap_err();
            assert!(error.to_string().contains("--custom"));
        }
    }

    #[test]
    fn start_refuses_missing_stop_condition_before_any_request() {
        assert!(
            CronCommand::try_parse_from([
                "cron",
                "start",
                "goal",
                "--phase",
                TEST_TEAM,
                "--module",
                "hook.py",
                "--cron",
                "0 * * * * *"
            ])
            .is_err()
        );
    }

    #[test]
    fn start_refuses_both_stop_conditions_before_any_request() {
        assert!(
            CronCommand::try_parse_from([
                "cron",
                "start",
                "goal",
                "--phase",
                TEST_TEAM,
                "--module",
                "hook.py",
                "--cron",
                "0 * * * * *",
                "--until-hook",
                "--until",
                "2026-11-01T00:00:00Z"
            ])
            .is_err()
        );
    }

    #[test]
    fn until_parses_rfc3339_and_normalizes_to_utc() {
        let parse = |until| {
            CronCommand::try_parse_from([
                "cron",
                "start",
                "goal",
                "--phase",
                TEST_TEAM,
                "--module",
                "hook.py",
                "--cron",
                "0 * * * * *",
                "--until",
                until,
            ])
        };
        let command = parse("2026-11-01T02:00:00+02:00").unwrap();
        let CronSubcommand::Start(args) = command.command else {
            unreachable!()
        };
        assert_eq!(
            args.until
                .unwrap()
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "2026-11-01T00:00:00Z"
        );
        for value in ["2026-11-01", "tomorrow"] {
            assert!(parse(value).unwrap_err().to_string().contains("--until"));
        }
    }

    #[test]
    fn documented_cron_commands_match_the_live_clap_parser() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root");
        let manifest: Value = serde_json::from_slice(
            &std::fs::read(repo_root.join("docs/user-documents/examples/cron/manifest.json"))
                .expect("read cron fixture manifest"),
        )
        .expect("parse cron fixture manifest");
        for fixture in manifest["commands"].as_array().expect("commands array") {
            if fixture["deferred_execution"].as_bool() == Some(true) {
                continue;
            }
            let Some(argv) = fixture["argv"].as_array() else {
                continue;
            };
            let id = fixture["id"].as_str().expect("fixture id");
            let args = argv
                .iter()
                .map(|value| value.as_str().expect("argument").to_owned())
                .collect::<Vec<_>>();
            let expected_valid = fixture["valid"].as_bool().expect("valid flag");
            assert_eq!(
                CronCommand::try_parse_from(args).is_ok(),
                expected_valid,
                "live Clap parser disagrees with command fixture {id}"
            );
        }

        let until = manifest["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|fixture| fixture["id"] == "start-until-utc")
            .expect("RFC3339 fixture");
        let args = until["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        let CronSubcommand::Start(start) = CronCommand::try_parse_from(args).unwrap().command
        else {
            panic!("start fixture did not parse as start")
        };
        assert_eq!(
            start
                .until
                .unwrap()
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "2030-11-01T00:00:00Z"
        );
        assert_eq!(start.cron_expr, "0 0 2 * * * 2030");
    }

    #[tokio::test]
    async fn documented_cron_argv_fixtures_use_the_composed_daemon() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root");
        let manifest: Value = serde_json::from_slice(
            &std::fs::read(root.join("docs/user-documents/examples/cron/manifest.json"))
                .expect("read manifest"),
        )
        .expect("parse manifest");
        let composition = ControlledCronComposition::new().expect("compose daemon");
        let observability = CliObservability::fallback();

        for documented in manifest["commands"].as_array().expect("commands") {
            if documented["deferred_execution"].as_bool() == Some(true) {
                continue;
            }
            let Some(argv) = documented["argv"].as_array() else {
                continue;
            };
            let id = documented["id"].as_str().expect("fixture id");
            let mut argv = argv
                .iter()
                .map(|value| value.as_str().expect("argument").to_owned())
                .collect::<Vec<_>>();
            if let Some(module) = argv.iter().position(|value| value == "--module") {
                argv[module + 1] = fixture("valid_hook.py").display().to_string();
            }
            let parsed = CronCommand::try_parse_from(argv);
            if documented["valid"] == false {
                assert!(
                    parsed.is_err(),
                    "fixture {id} must be refused by CLI parsing"
                );
                continue;
            }
            let command = parsed.unwrap_or_else(|error| panic!("fixture {id}: {error}"));
            let request = command
                .request(caller())
                .unwrap_or_else(|error| panic!("fixture {id} request: {error}"));
            let response = composition
                .execute(request)
                .await
                .unwrap_or_else(|error| panic!("fixture {id} daemon: {error}"));
            let output = command
                .execute_request(
                    &CliComposition::from_fake_transport(transport(response).0, &observability),
                    command.request(caller()).expect("repeat request"),
                )
                .await
                .unwrap_or_else(|error| panic!("fixture {id} output: {error}"));
            assert!(!output.is_empty(), "fixture {id} rendered output");
        }

        let list = CronCommand::try_parse_from(["cron", "list", "--phase", "bk", "--json"])
            .expect("documented list argv");
        let RequestEnvelope::CronList(list_request) = list.request(caller()).expect("list request")
        else {
            unreachable!("cron list must build a list request")
        };
        let ResponseEnvelope::CronList(rows) = composition
            .execute(RequestEnvelope::CronList(list_request))
            .await
            .expect("list durable cron rows")
        else {
            panic!("cron list response")
        };
        assert_eq!(rows.len(), 3, "three documented starts persist once each");
        assert!(
            rows.iter().all(|row| !row.active),
            "end-phase stops remaining rows"
        );

        let composition = composition
            .reopen()
            .await
            .expect("reopen SQLite composition");
        let ResponseEnvelope::CronList(rows) = composition
            .execute(RequestEnvelope::CronList(CronListQuery {
                caller_identity: TEST_SENDER.parse().expect("test sender"),
                caller_team: TEST_TEAM.parse().expect("test team"),
                phase: Some(PhaseId::parse("bk").expect("fixture phase")),
            }))
            .await
            .expect("list recovered cron rows")
        else {
            panic!("reopened cron list response")
        };
        assert_eq!(rows.len(), 3, "restart retains documented schedules");
        assert!(rows.iter().all(|row| !row.active));
    }

    #[tokio::test]
    async fn documented_five_field_cron_fixture_is_refused_by_the_composed_daemon() {
        let composition = ControlledCronComposition::new().expect("compose daemon");
        let command = CronCommand::try_parse_from([
            "cron",
            "start",
            "five-fields",
            "--cron",
            "0 * * * *",
            "--phase",
            TEST_TEAM,
            "--module",
            fixture("valid_hook.py")
                .to_str()
                .expect("fixture module utf-8"),
            "--until-hook",
        ])
        .expect("five-field argv reaches the production daemon validator");
        let error = composition
            .execute(command.request(caller()).expect("five-field cron request"))
            .await
            .expect_err("five-field expression must be refused by the composed daemon");
        assert_eq!(error.code(), atm_core::error::AtmErrorCode::CronExprInvalid);
    }

    #[tokio::test]
    async fn every_documented_bk11_fixture_executes_through_the_composed_daemon() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root");
        let manifest: Value = serde_json::from_slice(
            &std::fs::read(root.join("docs/user-documents/examples/cron/manifest.json"))
                .expect("read manifest"),
        )
        .expect("parse manifest");
        let commands = manifest["commands"].as_array().expect("commands array");
        let mut executed = BTreeSet::new();

        let composition = ControlledCronComposition::new().expect("compose daemon");
        for documented in commands {
            if documented["deferred_execution"].as_bool() == Some(true) {
                continue;
            }
            let Some(argv) = documented["argv"].as_array() else {
                continue;
            };
            let id = documented["id"].as_str().expect("fixture id");
            let mut argv = argv
                .iter()
                .map(|value| value.as_str().expect("argument").to_owned())
                .collect::<Vec<_>>();
            if let Some(module) = argv.iter().position(|value| value == "--module") {
                argv[module + 1] = fixture("valid_hook.py").display().to_string();
            }
            let parsed = CronCommand::try_parse_from(argv);
            if documented["valid"] == false {
                assert!(
                    parsed.is_err(),
                    "fixture {id} must be refused by CLI parsing"
                );
            } else {
                let command = parsed.unwrap_or_else(|error| panic!("fixture {id}: {error}"));
                composition
                    .execute(command.request(caller()).expect("CLI request"))
                    .await
                    .unwrap_or_else(|error| panic!("fixture {id} daemon: {error}"));
            }
            executed.insert(id.to_owned());
        }

        let five_fields = CronCommand::try_parse_from([
            "cron",
            "start",
            "five-fields",
            "--cron",
            "0 * * * *",
            "--phase",
            TEST_TEAM,
            "--module",
            fixture("valid_hook.py")
                .to_str()
                .expect("fixture module utf-8"),
            "--until-hook",
        ])
        .expect("five-field argv parses");
        let error = composition
            .execute(five_fields.request(caller()).expect("five-field request"))
            .await
            .expect_err("five-field expression must be refused by the composed daemon");
        assert_eq!(error.code(), atm_core::error::AtmErrorCode::CronExprInvalid);
        executed.insert("reject-five-fields".to_owned());

        let module = root.join("crates/atm-daemon-bootstrap/tests/hook_fixture/fixture_hook.py");
        for (id, reply) in [
            (
                "reject-foreign-from",
                r#"{"result":"handled","atm-action":[{"command":"send","from":"other@atm-dev","request":{"to":"team-lead","message":"status"}}]}"#,
            ),
            (
                "reject-malformed-output",
                r#"{"result":"pass-through","cron-output":[{"kind":"message","body":""}]}"#,
            ),
        ] {
            let composition = ControlledCronComposition::new().expect("compose daemon");
            std::fs::write(composition.root_path().join("fixture_reply.json"), reply)
                .expect("write refusal reply");
            let command = deferred_cron_start(id, &module, composition.root_path());
            let ResponseEnvelope::CronStarted(schedule) =
                execute_composed(&composition, &command).await
            else {
                panic!("fixture {id} must start")
            };
            composition
                .start_cron_worker()
                .await
                .expect("start controlled worker");
            composition
                .advance_clock_and_wait_for_run(&schedule, chrono::Duration::minutes(1))
                .await
                .expect("fixture cron run completes");
            let composition = composition
                .reopen()
                .await
                .expect("reopen after deferred refusal");
            let ResponseEnvelope::CronList(rows) = composition
                .execute(RequestEnvelope::CronList(CronListQuery {
                    caller_identity: TEST_SENDER.parse().expect("test sender"),
                    caller_team: TEST_TEAM.parse().expect("test team"),
                    phase: Some(PhaseId::parse(TEST_TEAM).expect("test phase")),
                }))
                .await
                .expect("list refusal outcome")
            else {
                panic!("cron list response")
            };
            assert!(
                rows[0]
                    .last_outcome
                    .as_deref()
                    .is_some_and(|outcome| outcome.starts_with("failure:")),
                "fixture {id} must record its refusal: {:?}",
                rows[0].last_outcome
            );
            executed.insert(id.to_owned());
        }

        for documented in commands.iter().filter(|fixture| {
            fixture["id"]
                .as_str()
                .is_some_and(|id| id.starts_with("hook-"))
        }) {
            let id = documented["id"].as_str().expect("hook fixture id");
            let mut argv = documented["argv"]
                .as_array()
                .expect("hook fixture argv")
                .iter()
                .map(|value| value.as_str().expect("argument").to_owned())
                .collect::<Vec<_>>();
            let module = argv.iter().position(|value| value == "--module");
            if let Some(module) = module {
                argv[module + 1] = fixture("valid_hook.py").display().to_string();
            }
            let command = crate::commands::hook::HookCommand::try_parse_from(argv)
                .unwrap_or_else(|error| panic!("fixture {id}: {error}"));
            let composition = ControlledCronComposition::new().expect("compose daemon");
            let response = composition
                .execute(command.request(caller()).expect("hook CLI request"))
                .await
                .unwrap_or_else(|error| panic!("fixture {id} daemon: {error}"));
            assert!(
                matches!(
                    response,
                    ResponseEnvelope::HookRegistered(_) | ResponseEnvelope::HookDisabled(_)
                ),
                "fixture {id} must reach the composed hook daemon"
            );
            executed.insert(id.to_owned());
        }
        assert_eq!(
            executed,
            manifest_fixture_ids(&root),
            "every manifest fixture must execute before coverage is reported"
        );
    }

    #[tokio::test]
    async fn deferred_rejection_replies_are_refused_by_the_composed_cron_worker() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root");
        let module = root.join("crates/atm-daemon-bootstrap/tests/hook_fixture/fixture_hook.py");
        let composition = ControlledCronComposition::new().expect("compose daemon");

        std::fs::write(
            composition.root_path().join("fixture_reply.json"),
            r#"{"result":"handled","atm-action":[{"command":"send","from":"other@atm-dev","request":{"to":"team-lead","message":"status"}}]}"#,
        )
        .expect("write foreign-sender reply");
        let foreign = deferred_cron_start("reject-foreign-from", &module, composition.root_path());
        let ResponseEnvelope::CronStarted(foreign_schedule) =
            execute_composed(&composition, &foreign).await
        else {
            panic!("foreign refusal must start")
        };
        composition
            .start_cron_worker()
            .await
            .expect("start controlled cron worker");
        composition
            .advance_clock_and_wait_for_run(&foreign_schedule, chrono::Duration::minutes(1))
            .await
            .expect("foreign refusal run completes");
        let composition = composition
            .reopen()
            .await
            .expect("reopen after foreign reply");

        let RequestEnvelope::CronList(list) =
            CronCommand::try_parse_from(["cron", "list", "--phase", TEST_TEAM])
                .expect("cron list parses")
                .request(caller())
                .expect("cron list request")
        else {
            unreachable!("cron list command creates a list request")
        };
        let ResponseEnvelope::CronList(rows) = composition
            .execute(RequestEnvelope::CronList(list))
            .await
            .expect("list foreign-sender run")
        else {
            panic!("cron list response")
        };
        assert_eq!(rows.len(), 1);
        assert!(
            rows[0]
                .last_outcome
                .as_deref()
                .is_some_and(|outcome| outcome.starts_with("failure:")),
            "foreign sender is refused: {:?}",
            rows[0].last_outcome
        );

        assert_empty_mailbox(&composition, TEST_SENDER).await;
        assert_empty_mailbox(&composition, TEST_RECIPIENT).await;

        let end = CronCommand::try_parse_from(["cron", "end", "--phase", TEST_TEAM])
            .expect("cron end parses");
        assert!(matches!(
            execute_composed(&composition, &end).await,
            ResponseEnvelope::CronEnded(_)
        ));

        std::fs::write(
            composition.root_path().join("fixture_reply.json"),
            r#"{"result":"pass-through","cron-output":[{"kind":"message","body":""}]}"#,
        )
        .expect("write malformed-output reply");
        let malformed =
            deferred_cron_start("reject-malformed-output", &module, composition.root_path());
        let ResponseEnvelope::CronStarted(malformed_schedule) =
            execute_composed(&composition, &malformed).await
        else {
            panic!("malformed refusal must start")
        };
        composition
            .start_cron_worker()
            .await
            .expect("restart controlled cron worker");
        composition
            .advance_clock_and_wait_for_run(&malformed_schedule, chrono::Duration::minutes(1))
            .await
            .expect("malformed refusal run completes");
        let composition = composition
            .reopen()
            .await
            .expect("reopen after malformed reply");

        let ResponseEnvelope::CronList(rows) = composition
            .execute(RequestEnvelope::CronList(CronListQuery {
                caller_identity: TEST_SENDER.parse().expect("test sender"),
                caller_team: TEST_TEAM.parse().expect("test team"),
                phase: Some(PhaseId::parse(TEST_TEAM).expect("test phase")),
            }))
            .await
            .expect("list malformed-output run")
        else {
            panic!("cron list response")
        };
        assert_eq!(rows.len(), 2);
        assert!(
            rows[1]
                .last_outcome
                .as_deref()
                .is_some_and(|outcome| outcome.starts_with("failure:")),
            "malformed output is refused: {:?}",
            rows[1].last_outcome
        );
        assert_empty_mailbox(&composition, TEST_SENDER).await;
        assert_empty_mailbox(&composition, TEST_RECIPIENT).await;
    }

    #[tokio::test]
    async fn composed_cron_delivers_custom_mail_and_lifecycle_nudge() {
        let composition = ControlledCronComposition::new().expect("compose daemon");
        let module = fixture("cron_composed_actions.py");
        let command = CronCommand::try_parse_from([
            "cron",
            "start",
            "custom-mail",
            "--cron",
            "0 * * * * *",
            "--phase",
            TEST_TEAM,
            "--module",
            module.to_str().expect("fixture module utf-8"),
            "--arg",
            "custom-mail",
            "--until",
            "2026-10-24T01:00:00Z",
        ])
        .expect("custom-mail cron argv parses");
        let malformed_command = CronCommand::try_parse_from([
            "cron",
            "start",
            "lifecycle-error",
            "--cron",
            "0 * * * * *",
            "--phase",
            TEST_TEAM,
            "--module",
            module.to_str().expect("fixture module utf-8"),
            "--arg",
            "unknown",
            "--arg",
            "lifecycle-error",
            "--until",
            "2026-10-24T01:00:00Z",
        ])
        .expect("malformed lifecycle cron argv parses");
        let mut lifecycle_caller = caller();
        lifecycle_caller.caller_identity = TEST_RECIPIENT.parse().expect("test recipient");
        let ResponseEnvelope::CronStarted(custom_schedule) = composition
            .execute(
                command
                    .request(lifecycle_caller.clone())
                    .expect("custom-mail request"),
            )
            .await
            .expect("custom-mail start")
        else {
            panic!("custom mail must start")
        };
        let ResponseEnvelope::CronStarted(lifecycle_schedule) = composition
            .execute(
                malformed_command
                    .request(lifecycle_caller)
                    .expect("malformed lifecycle request"),
            )
            .await
            .expect("malformed lifecycle start")
        else {
            panic!("lifecycle error must start")
        };
        composition
            .start_cron_worker()
            .await
            .expect("start controlled cron worker");
        composition
            .advance_clock_and_wait_for_runs(
                &[&custom_schedule, &lifecycle_schedule],
                chrono::Duration::minutes(1),
            )
            .await
            .expect("custom mail and lifecycle runs complete");
        composition.shutdown_cron_worker().await;
        assert!(
            composition.has_herdr_prompt(
                TEST_RECIPIENT,
                "<atm cron name=\"lifecycle-error\" event=\"error\" error=\"cron hook runner failed\">",
            ),
            "production lifecycle transport emits the rendered error notice to the configured Herdr recipient: {:?}",
            composition.herdr_calls()
        );
        let composition = composition
            .reopen()
            .await
            .expect("reopen after custom mail");

        let ResponseEnvelope::List(outcome) = composition
            .execute(RequestEnvelope::List(
                ListQuery::new(
                    composition.root_path().into(),
                    composition.root_path().into(),
                    TEST_RECIPIENT.parse().expect("test recipient"),
                    Some(TEST_RECIPIENT),
                    TEST_TEAM.parse().expect("test team"),
                    ReadSelection::All,
                    false,
                    None,
                    None,
                    None,
                    None,
                    None,
                )
                .expect("custom-mail list request"),
            ))
            .await
            .expect("list custom mail")
        else {
            panic!("list response")
        };
        assert_eq!(outcome.count, 1);
        assert_eq!(
            outcome.rows[0].from,
            "atm-daemon"
                .parse::<atm_core::types::AgentName>()
                .expect("daemon sender")
        );
        assert_eq!(outcome.rows[0].summary, "fixture custom mail");
        let message_id = outcome.rows[0]
            .message_id
            .as_ref()
            .expect("persisted custom mail has an id")
            .to_string();
        let ResponseEnvelope::Receive(read) = composition
            .execute(RequestEnvelope::Receive(
                ReadQuery::new(
                    composition.root_path().into(),
                    composition.root_path().into(),
                    TEST_RECIPIENT.parse().expect("test recipient"),
                    None,
                    TEST_TEAM.parse().expect("test team"),
                    ReadSelection::All,
                    false,
                    false,
                    Some(&message_id),
                    None,
                    None,
                    None,
                    None,
                    None,
                )
                .expect("custom-mail read request"),
            ))
            .await
            .expect("read persisted custom mail")
        else {
            panic!("read response")
        };
        assert_eq!(
            read.message.expect("persisted custom mail").envelope.text,
            "fixture custom mail"
        );
    }
}

#[derive(Debug, Parser)]
#[command(
    about = "Manage recurring hook schedules",
    long_about = "Manage persistent UTC schedules that run a hook module. Schedules belong to a team and phase, survive daemon restarts, and require exactly one stop condition. Use atm help cron for the reply, output, lifecycle, and recovery guide."
)]
pub struct CronCommand {
    #[command(subcommand)]
    pub command: CronSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum CronSubcommand {
    #[command(
        about = "Start a recurring hook schedule",
        long_about = "Start a persistent schedule for a hook module. The expression uses six fields with seconds first, or seven fields with an optional year; the supported aliases are @yearly, @monthly, @weekly, @daily, and @hourly. Five-field expressions are refused. Give exactly one future --until timestamp or --until-hook. The module timeout comes from CRON_TIMEOUT_MS, falling back to TIMEOUT_MS, and is capped at 30 seconds. The daemon stores a fixed jitter offset with the schedule."
    )]
    Start(CronStartArgs),
    #[command(
        about = "End active schedules in a phase",
        long_about = "End active schedules for the selected phase. Add a schedule name to end only that schedule; omit it to end every active schedule in the phase. Ended rows remain available in atm cron list. Ending a schedule is a manual stop and does not delete its history."
    )]
    End(CronEndArgs),
    #[command(
        about = "List active and ended schedules",
        long_about = "List schedules for the caller's team, including active and ended rows. Filter by phase when needed. Human output summarizes custom-object keys; --json emits a bare array with the stored schedule fields, including creator, generation, jitter, last run, and outcome."
    )]
    List(CronListArgs),
}

#[derive(Debug, Args)]
#[command(group = clap::ArgGroup::new("stop").required(true).multiple(false).args(["until", "until_hook"]))]
pub struct CronStartArgs {
    #[arg(
        help = "Schedule name within the phase",
        long_help = "Unique schedule name within the selected team and phase. An active schedule with the same name is refused; restarting an ended name reuses its stored row."
    )]
    pub name: String,
    #[arg(
        long = "cron",
        help = "Six- or seven-field UTC cron expression",
        long_help = "Cron 0.17 expression in UTC: six fields with seconds first, or seven fields with an optional year. The supported aliases are @yearly, @monthly, @weekly, @daily, and @hourly. Five fields, expressions with no future occurrence, and a stop time that is not in the future are rejected before a schedule is stored."
    )]
    pub cron_expr: String,
    #[arg(
        long,
        help = "Hook module to run",
        long_help = "Path to the Python hook module. The CLI resolves the module and working directory before sending the start request. The module declares CRON_TIMEOUT_MS, or TIMEOUT_MS when the cron-specific value is absent; the daemon enforces the 30,000 ms maximum."
    )]
    pub module: PathBuf,
    #[arg(
        long,
        help = "Phase that owns the schedule",
        long_help = "Phase associated with this schedule. Any roster member of the caller's team may start, end, or list schedules."
    )]
    pub phase: String,
    #[arg(
        long,
        value_parser = parse_rfc3339_utc,
        help = "Stop at a future RFC 3339 instant",
        long_help = "End the schedule at this future RFC 3339 instant. The stored timestamp is normalized to UTC. This option conflicts with --until-hook; provide exactly one stop condition."
    )]
    pub until: Option<DateTime<Utc>>,
    #[arg(
        long = "until-hook",
        help = "Ask the module stop hook before each tick",
        long_help = "Use the module's cron.stop reply as the stop condition before each scheduled tick. Reply {\"stop\":false} to continue or {\"stop\":true,\"reason\":\"...\"} to end. This option conflicts with --until; provide exactly one stop condition."
    )]
    pub until_hook: bool,
    #[arg(
        long = "arg",
        action = clap::ArgAction::Append,
        allow_hyphen_values = true,
        help = "Module argument; repeat for each argument",
        long_help = "One argument passed in the cron.tick request's args array. Repeat this option for additional values. Values beginning with a hyphen, such as --json, are accepted and are not interpreted as ATM options."
    )]
    pub args: Vec<String>,
    #[arg(
        long,
        help = "Working directory; defaults to the module directory",
        long_help = "Working directory used to resolve and run the module. Relative paths are resolved from the current invocation directory. If omitted, the canonical module directory is used."
    )]
    pub cwd: Option<PathBuf>,
    #[arg(
        long,
        value_parser = parse_custom_object,
        help = "Custom JSON object passed to the module",
        long_help = "JSON object passed unchanged to the module in the cron.tick and cron.stop request envelopes. Arrays, scalars, and malformed JSON are rejected. The serialized object is limited by the daemon's hook custom-data limit."
    )]
    pub custom: Option<Map<String, Value>>,
}

#[derive(Debug, Args)]
pub struct CronEndArgs {
    #[arg(
        long,
        help = "Phase whose active schedules will be ended",
        long_help = "Select the phase whose active schedules are ended. The selected caller must belong to the schedule's team."
    )]
    pub phase: String,
    #[arg(
        help = "End only this schedule; otherwise end every schedule in the phase",
        long_help = "Optional schedule name. When supplied, only that active schedule is ended; when omitted, every active schedule in the phase is ended."
    )]
    pub name: Option<String>,
}

#[derive(Debug, Args)]
pub struct CronListArgs {
    #[arg(
        long,
        help = "Limit schedules to this phase",
        long_help = "Show only schedules associated with this phase. Without this option, all schedules for the caller's team are listed."
    )]
    pub phase: Option<String>,
    #[arg(
        long,
        help = "Emit full schedules as a bare JSON array",
        long_help = "Write the stored schedule rows as one JSON array. Each row includes the creator, generation, active state, stop condition, custom object, fixed jitter_s, last_run_at, last_outcome, and ended_at fields. An empty result is []."
    )]
    pub json: bool,
}

impl CronCommand {
    pub async fn run(self, observability: &CliObservability) -> Result<()> {
        let caller = resolve_cli_caller_context(CallerContextOverrides::default())?;
        let request = self.request(caller)?;
        let (home, current) = resolve_command_runtime_context("cron")?;
        let composition = CliComposition::bootstrap(
            "cron",
            observability,
            InvocationDir::new(&current),
            AtmHomePath::new(&home),
        )?;
        preflight_daemon_api(&composition, HttpApiVersion::parse("1.13.0")?, "cron").await?;
        print!("{}", self.execute_request(&composition, request).await?);
        Ok(())
    }

    pub(super) fn request(&self, caller: CallerContext) -> Result<RequestEnvelope> {
        let caller_identity = caller.caller_identity;
        let caller_team = caller.caller_team;
        Ok(match &self.command {
            CronSubcommand::Start(args) => {
                let (module_path, cwd) = canonical_paths(&args.module, args.cwd.as_deref())?;
                let timeout_ms = read_cron_timeout_ms(&module_path)?;
                let stop = match (args.until, args.until_hook) {
                    (Some(at), false) => StopCondition::Until { at: at.into() },
                    (None, true) => StopCondition::Hook,
                    _ => {
                        return Err(atm_core::error::AtmError::validation(
                            "exactly one stop condition is required",
                        )
                        .into());
                    }
                };
                RequestEnvelope::CronStart(CronStartRequest {
                    caller_identity,
                    caller_team,
                    phase: PhaseId::parse(&args.phase)?,
                    name: args.name.clone(),
                    cron_expr: args.cron_expr.clone(),
                    module_path,
                    cwd,
                    args: args.args.clone(),
                    timeout_ms,
                    stop,
                    custom: Value::Object(args.custom.clone().unwrap_or_default()),
                })
            }
            CronSubcommand::End(args) => RequestEnvelope::CronEnd(CronEndRequest {
                caller_identity,
                caller_team,
                phase: PhaseId::parse(&args.phase)?,
                name: args.name.clone(),
            }),
            CronSubcommand::List(args) => RequestEnvelope::CronList(CronListQuery {
                caller_identity,
                caller_team,
                phase: args.phase.as_deref().map(PhaseId::parse).transpose()?,
            }),
        })
    }

    pub(super) async fn execute_request(
        &self,
        composition: &CliComposition<'_>,
        request: RequestEnvelope,
    ) -> Result<String> {
        let response = composition.execute_request(request).await?;
        match (&self.command, response) {
            (CronSubcommand::Start(_), ResponseEnvelope::CronStarted(row)) => Ok(format!(
                "started {} for {}/{} (timeout {} ms)\n",
                row.name, row.team, row.phase, row.timeout_ms
            )),
            (CronSubcommand::End(args), ResponseEnvelope::CronEnded(rows)) => {
                if rows.is_empty() {
                    Ok(format!("no active schedules for phase {}\n", args.phase))
                } else {
                    Ok(rows
                        .iter()
                        .map(|row| format!("ended {} for {}/{}\n", row.name, row.team, row.phase))
                        .collect())
                }
            }
            (CronSubcommand::List(args), ResponseEnvelope::CronList(rows)) => {
                crate::output::render_cron_list(&rows, args.json)
            }
            (_, other) => Err(atm_daemon_client::unexpected_response("cron", other).into()),
        }
    }
}
