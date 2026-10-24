use std::path::PathBuf;

use anyhow::Result;
use atm_core::boundary::PhaseId;
use atm_core::caller_context::{CallerContext, CallerContextOverrides, resolve_cli_caller_context};
use atm_core::hook::{HookDisableRequest, HookListQuery, HookRegisterRequest};
use atm_core::protocol::{HttpApiVersion, RequestEnvelope, ResponseEnvelope};
use clap::{Args, Parser, Subcommand};
use serde_json::{Map, Value};

use super::hook_module::{canonical_paths, parse_custom_object, read_timeout_ms};
use super::send::preflight_daemon_api;
use crate::composition::{
    AtmHomePath, CliComposition, InvocationDir, resolve_command_runtime_context,
};
use crate::observability::CliObservability;

#[derive(Debug, Parser)]
#[command(about = "Register, disable, or list task hooks")]
pub struct HookCommand {
    #[command(subcommand)]
    pub command: HookSubcommand,
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use atm_core::api::ApiResponse;
    use atm_core::observability::NullObservability;
    use atm_core::protocol::{CLI_SCHEMA_VERSION, CompatibilityVerdict, ReleaseVersion};
    use atm_core::test_support::{TEST_SENDER, TEST_TEAM};
    use atm_core::transport::testing::{FakeClientTransport, LoopbackClientTransport};
    use atm_daemon_bootstrap::test_support::ControlledCronComposition;
    use std::sync::{Arc, Mutex};

    pub(crate) fn caller() -> CallerContext {
        CallerContext {
            caller_identity: TEST_SENDER.parse().unwrap(),
            caller_chat_id: None,
            caller_team: TEST_TEAM.parse().unwrap(),
            activity_observation: None,
        }
    }

    pub(crate) fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/hook_module")
            .join(name)
    }

    pub(crate) fn transport(
        response: ResponseEnvelope,
    ) -> (Arc<FakeClientTransport>, Arc<Mutex<Vec<RequestEnvelope>>>) {
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let requests = recorded.clone();
        let transport = Arc::new(FakeClientTransport::new(move |request| {
            if matches!(request, RequestEnvelope::CompatibilityPreflight(_)) {
                return Ok(ResponseEnvelope::CompatibilityVerdict(
                    CompatibilityVerdict::Compatible {
                        daemon_release: ReleaseVersion::current(),
                        daemon_schema_version: CLI_SCHEMA_VERSION,
                        daemon_http_api_version: HttpApiVersion::parse("1.13.0").unwrap(),
                    },
                ));
            }
            requests.lock().unwrap().push(request);
            Ok(response.clone())
        }));
        (transport, recorded)
    }

    pub(crate) fn row() -> atm_core::boundary::HookRegistration {
        serde_json::from_str(&format!(
            r#"{{"team":"{TEST_TEAM}","phase":"{TEST_TEAM}","hook":"task.close","module_path":"hook.py","cwd":"/repo","timeout_ms":2500,"custom":{{"goal":"ship"}},"active":false,"registered_by":"{TEST_SENDER}","registered_at":"2026-10-19T00:00:00Z","disabled_at":"2026-10-19T01:00:00Z"}}"#
        ))
        .unwrap()
    }

    pub(super) fn register(module: &str, name: &str) -> HookCommand {
        HookCommand {
            command: HookSubcommand::Register(HookRegisterArgs {
                hook: name.into(),
                module: fixture(module),
                phase: TEST_TEAM.into(),
                cwd: None,
                custom: None,
                json: false,
            }),
        }
    }

    pub(crate) async fn execute(
        command: HookCommand,
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

    fn documented_argv(id: &str) -> Vec<String> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root");
        let manifest: Value = serde_json::from_slice(
            &std::fs::read(root.join("docs/user-documents/examples/cron/manifest.json"))
                .expect("read cron fixture manifest"),
        )
        .expect("parse cron fixture manifest");
        let documented = manifest["commands"]
            .as_array()
            .expect("commands array")
            .iter()
            .find(|fixture| fixture["id"] == id)
            .unwrap_or_else(|| panic!("missing hook fixture {id}"));
        let mut argv = documented["argv"]
            .as_array()
            .expect("fixture argv")
            .iter()
            .map(|value| value.as_str().expect("argument").to_owned())
            .collect::<Vec<_>>();
        if let Some(module) = argv.iter().position(|value| value == "--module") {
            argv[module + 1] = fixture("valid_hook.py").display().to_string();
        }
        argv
    }

    fn controlled_composition<'a>(
        daemon: Arc<ControlledCronComposition>,
        observability: &'a CliObservability,
    ) -> CliComposition<'a> {
        let transport = Arc::new(LoopbackClientTransport::with_async_handler(
            Arc::new(NullObservability),
            move |request| {
                let daemon = Arc::clone(&daemon);
                Box::pin(async move {
                    daemon
                        .execute(request.into_inner())
                        .await
                        .map(ApiResponse::new)
                })
            },
        ));
        CliComposition::from_loopback_transport(transport, observability)
    }

    async fn execute_controlled(
        command: HookCommand,
        daemon: Arc<ControlledCronComposition>,
        observability: &CliObservability,
    ) -> String {
        let request = command.request(caller()).expect("build CLI request");
        command
            .execute_request(&controlled_composition(daemon, observability), request)
            .await
            .expect("execute composed CLI command")
    }

    fn parse_json(output: &str) -> Value {
        assert_eq!(output.matches('\n').count(), 1, "one JSON line");
        assert_eq!(output.lines().count(), 1, "one JSON value");
        serde_json::from_str(output).expect("JSON output")
    }

    #[tokio::test]
    async fn hook_register_json_contract_uses_the_composed_daemon() {
        let daemon = Arc::new(ControlledCronComposition::new().expect("compose daemon"));
        let observability = CliObservability::fallback();
        let output = execute_controlled(
            HookCommand::try_parse_from(documented_argv("hook-register-json-contract"))
                .expect("parse documented register fixture"),
            Arc::clone(&daemon),
            &observability,
        )
        .await;
        let registration = parse_json(&output);
        assert_eq!(registration["action"], "hook.register");
        let row = registration["registration"].clone();
        assert_eq!(row["team"], TEST_TEAM);
        assert_eq!(row["phase"], "bk");
        assert_eq!(row["hook"], "task.close");
        let module_path = fixture("valid_hook.py").canonicalize().unwrap();
        assert_eq!(row["module_path"], serde_json::json!(module_path));
        assert_eq!(row["cwd"], serde_json::json!(module_path.parent().unwrap()));
        assert_eq!(row["timeout_ms"], 2500);
        assert_eq!(row["custom"], serde_json::json!({}));
        assert_eq!(row["active"], true);
        assert_eq!(row["registered_by"], TEST_SENDER);
        assert!(!row["registered_at"].is_null());
        assert!(row["disabled_at"].is_null());

        let listed = execute_controlled(
            HookCommand::try_parse_from(["hook", "list", "--phase", "bk", "--json"])
                .expect("parse list command"),
            daemon,
            &observability,
        )
        .await;
        assert_eq!(parse_json(&listed), serde_json::json!([row]));
    }

    #[tokio::test]
    async fn hook_disable_one_json_contract_uses_the_composed_daemon() {
        let daemon = Arc::new(ControlledCronComposition::new().expect("compose daemon"));
        let observability = CliObservability::fallback();
        execute_controlled(
            HookCommand::try_parse_from(documented_argv("hook-register-json-contract"))
                .expect("parse documented register fixture"),
            Arc::clone(&daemon),
            &observability,
        )
        .await;

        let disabled = parse_json(
            &execute_controlled(
                HookCommand::try_parse_from(documented_argv("hook-disable-one-json-contract"))
                    .expect("parse documented single-disable fixture"),
                Arc::clone(&daemon),
                &observability,
            )
            .await,
        );
        assert_eq!(disabled["action"], "hook.disable");
        assert_eq!(disabled["count"], 1);
        let rows = disabled["disabled"].as_array().expect("disabled rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["hook"], "task.close");
        assert_eq!(rows[0]["phase"], "bk");
        assert_eq!(rows[0]["active"], false);
        assert!(!rows[0]["disabled_at"].is_null());

        let listed = execute_controlled(
            HookCommand::try_parse_from(["hook", "list", "--phase", "bk", "--json"])
                .expect("parse list command"),
            daemon,
            &observability,
        )
        .await;
        assert_eq!(parse_json(&listed), disabled["disabled"]);
    }

    #[tokio::test]
    async fn hook_disable_phase_json_contract_uses_the_composed_daemon() {
        let daemon = Arc::new(ControlledCronComposition::new().expect("compose daemon"));
        let observability = CliObservability::fallback();
        for hook in ["task.close", "task.start"] {
            let mut argv = documented_argv("hook-register-json-contract");
            argv[2] = hook.into();
            execute_controlled(
                HookCommand::try_parse_from(argv).expect("parse register command"),
                Arc::clone(&daemon),
                &observability,
            )
            .await;
        }

        let disabled = parse_json(
            &execute_controlled(
                HookCommand::try_parse_from(documented_argv("hook-disable-phase-json-contract"))
                    .expect("parse documented phase-disable fixture"),
                Arc::clone(&daemon),
                &observability,
            )
            .await,
        );
        assert_eq!(disabled["action"], "hook.disable");
        assert_eq!(disabled["count"], 2);
        let rows = disabled["disabled"].as_array().expect("disabled rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows.iter()
                .map(|row| row["hook"].as_str().expect("hook"))
                .collect::<Vec<_>>(),
            ["task.close", "task.start"]
        );
        assert!(rows.iter().all(|row| {
            row["phase"] == "bk" && row["active"] == false && !row["disabled_at"].is_null()
        }));

        let listed = execute_controlled(
            HookCommand::try_parse_from(["hook", "list", "--phase", "bk", "--json"])
                .expect("parse list command"),
            daemon,
            &observability,
        )
        .await;
        assert_eq!(parse_json(&listed), disabled["disabled"]);
    }

    #[tokio::test]
    async fn hook_disable_no_match_json_contract_uses_the_composed_daemon() {
        let daemon = Arc::new(ControlledCronComposition::new().expect("compose daemon"));
        let observability = CliObservability::fallback();
        let disabled = parse_json(
            &execute_controlled(
                HookCommand::try_parse_from(documented_argv("hook-disable-no-match-json-contract"))
                    .expect("parse documented no-match fixture"),
                Arc::clone(&daemon),
                &observability,
            )
            .await,
        );
        assert_eq!(
            disabled,
            serde_json::json!({"action": "hook.disable", "disabled": [], "count": 0})
        );

        let listed = execute_controlled(
            HookCommand::try_parse_from(["hook", "list", "--phase", "missing", "--json"])
                .expect("parse list command"),
            daemon,
            &observability,
        )
        .await;
        assert_eq!(parse_json(&listed), serde_json::json!([]));
    }

    #[tokio::test]
    async fn register_reads_timeout_ms_from_module() {
        for module in ["valid_hook.py", "cron_timeout.py"] {
            let (result, requests) = execute(
                register(module, "task.close"),
                ResponseEnvelope::HookRegistered(row()),
            )
            .await;
            assert_eq!(
                result.unwrap(),
                format!("registered task.close for {TEST_TEAM}/{TEST_TEAM} (timeout 2500 ms)\n")
            );
            let [RequestEnvelope::HookRegister(request)] = requests.as_slice() else {
                panic!("{requests:?}")
            };
            assert_eq!(request.timeout_ms, 2500);
            assert_eq!(request.module_path, fixture(module).canonicalize().unwrap());
            assert_eq!(request.cwd, request.module_path.parent().unwrap());
            assert_eq!(request.custom, serde_json::json!({}));
        }
    }

    #[tokio::test]
    async fn register_json_returns_the_persisted_registration() {
        let command = HookCommand::try_parse_from([
            "hook",
            "register",
            "task.close",
            "--module",
            fixture("valid_hook.py").to_str().unwrap(),
            "--phase",
            TEST_TEAM,
            "--json",
        ])
        .unwrap();
        let expected_row = row();
        let (result, requests) = execute(
            command,
            ResponseEnvelope::HookRegistered(expected_row.clone()),
        )
        .await;
        let output = result.unwrap();
        let parsed: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["action"], "hook.register");
        assert_eq!(
            parsed["registration"],
            serde_json::to_value(expected_row).unwrap()
        );
        assert_eq!(output.matches('\n').count(), 1);
        assert_eq!(output.lines().count(), 1);
        assert!(matches!(
            requests.as_slice(),
            [RequestEnvelope::HookRegister(_)]
        ));
    }

    #[tokio::test]
    async fn register_refuses_bad_timeout_ms_before_any_request() {
        for module in [
            "no_timeout.py",
            "zero_timeout.py",
            "indented.py",
            "two_lines.py",
        ] {
            let (result, requests) = execute(
                register(module, "task.close"),
                ResponseEnvelope::HookRegistered(row()),
            )
            .await;
            let error = result.unwrap_err();
            assert_eq!(
                error
                    .downcast_ref::<atm_core::error::AtmError>()
                    .unwrap()
                    .code(),
                atm_core::error::AtmErrorCode::HookModuleInvalid
            );
            assert!(requests.is_empty());
        }
    }

    #[tokio::test]
    async fn register_rejects_a_mismatched_hook_list_response() {
        let (result, requests) = execute(
            register("valid_hook.py", "task.close"),
            ResponseEnvelope::HookList(vec![]),
        )
        .await;

        assert!(result.is_err());
        assert!(matches!(
            requests.as_slice(),
            [RequestEnvelope::HookRegister(_)]
        ));
    }

    #[tokio::test]
    async fn register_over_cap_sends_and_prints_the_daemon_refusal() {
        let error = atm_core::error::AtmError::hook_module_invalid("daemon cap refusal");
        let (result, requests) = execute(
            register("over_cap.py", "task.close"),
            ResponseEnvelope::Error(error.clone()),
        )
        .await;
        assert_eq!(result.unwrap_err().to_string(), error.to_string());
        let [RequestEnvelope::HookRegister(request)] = requests.as_slice() else {
            panic!("{requests:?}")
        };
        assert_eq!(request.timeout_ms, 3000);
    }

    #[tokio::test]
    async fn register_refuses_missing_module_and_missing_cwd_before_any_request() {
        for missing_module in [true, false] {
            let mut command = register("valid_hook.py", "task.close");
            let HookSubcommand::Register(args) = &mut command.command else {
                unreachable!()
            };
            if missing_module {
                args.module = fixture("missing.py");
            } else {
                args.cwd = Some(fixture("missing-dir"));
            }
            let (result, requests) =
                execute(command, ResponseEnvelope::HookRegistered(row())).await;
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
    async fn register_sends_any_hook_name_and_prints_the_daemon_message() {
        for name in ["cron.tick", "task.foo"] {
            let error = atm_core::error::AtmError::hook_module_invalid("daemon hook refusal");
            let (result, requests) = execute(
                register("valid_hook.py", name),
                ResponseEnvelope::Error(error.clone()),
            )
            .await;
            assert_eq!(result.unwrap_err().to_string(), error.to_string());
            let [RequestEnvelope::HookRegister(request)] = requests.as_slice() else {
                panic!("{requests:?}")
            };
            assert_eq!(request.hook, name);
        }
    }

    #[tokio::test]
    async fn disable_sends_phase_and_hook_and_prints_one_line_per_row() {
        let command =
            HookCommand::try_parse_from(["hook", "disable", "--phase", TEST_TEAM, "task.close"])
                .unwrap();
        let (result, requests) =
            execute(command, ResponseEnvelope::HookDisabled(vec![row(), row()])).await;
        assert_eq!(
            result.unwrap(),
            format!(
                "disabled task.close for {TEST_TEAM}/{TEST_TEAM}\ndisabled task.close for {TEST_TEAM}/{TEST_TEAM}\n"
            )
        );
        let [RequestEnvelope::HookDisable(request)] = requests.as_slice() else {
            panic!("{requests:?}")
        };
        assert_eq!(request.phase.as_str(), TEST_TEAM);
        assert_eq!(request.hook.as_deref(), Some("task.close"));
    }

    #[tokio::test]
    async fn disable_json_returns_exact_rows_and_count_for_single_and_phase_requests() {
        let rows = vec![row(), row()];
        for command in [
            HookCommand::try_parse_from([
                "hook",
                "disable",
                "--json",
                "--phase",
                TEST_TEAM,
                "task.close",
            ])
            .unwrap(),
            HookCommand::try_parse_from(["hook", "disable", "--phase", TEST_TEAM, "--json"])
                .unwrap(),
        ] {
            let (result, requests) =
                execute(command, ResponseEnvelope::HookDisabled(rows.clone())).await;
            let output = result.unwrap();
            let parsed: Value = serde_json::from_str(&output).unwrap();
            assert_eq!(parsed["action"], "hook.disable");
            assert_eq!(parsed["disabled"], serde_json::to_value(&rows).unwrap());
            assert_eq!(parsed["count"], rows.len());
            assert_eq!(output.matches('\n').count(), 1);
            assert_eq!(output.lines().count(), 1);
            assert!(matches!(
                requests.as_slice(),
                [RequestEnvelope::HookDisable(_)]
            ));
        }
    }

    #[tokio::test]
    async fn disable_json_with_no_matches_returns_empty_rows_and_zero_count() {
        let command =
            HookCommand::try_parse_from(["hook", "disable", "--phase", TEST_TEAM, "--json"])
                .unwrap();
        let (result, _) = execute(command, ResponseEnvelope::HookDisabled(vec![])).await;
        let output = result.unwrap();
        let parsed: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(
            parsed,
            serde_json::json!({"action":"hook.disable", "disabled":[], "count":0})
        );
        assert_eq!(output.matches('\n').count(), 1);
        assert_eq!(output.lines().count(), 1);
    }

    #[tokio::test]
    async fn json_mutations_preserve_typed_daemon_errors() {
        let error = atm_core::error::AtmError::hook_module_invalid("daemon refusal");
        let mut register = register("valid_hook.py", "task.close");
        let HookSubcommand::Register(args) = &mut register.command else {
            unreachable!()
        };
        args.json = true;
        let (result, _) = execute(register, ResponseEnvelope::Error(error.clone())).await;
        let returned = result.unwrap_err();
        assert_eq!(returned.to_string(), error.to_string());
        assert_eq!(
            returned
                .downcast_ref::<atm_core::error::AtmError>()
                .unwrap()
                .code(),
            atm_core::error::AtmErrorCode::HookModuleInvalid
        );

        let command =
            HookCommand::try_parse_from(["hook", "disable", "--phase", TEST_TEAM, "--json"])
                .unwrap();
        let (result, _) = execute(command, ResponseEnvelope::Error(error.clone())).await;
        let returned = result.unwrap_err();
        assert_eq!(returned.to_string(), error.to_string());
        assert_eq!(
            returned
                .downcast_ref::<atm_core::error::AtmError>()
                .unwrap()
                .code(),
            atm_core::error::AtmErrorCode::HookModuleInvalid
        );
    }

    #[tokio::test]
    async fn register_json_preserves_typed_local_errors_without_sending_a_request() {
        let mut command = register("no_timeout.py", "task.close");
        let HookSubcommand::Register(args) = &mut command.command else {
            unreachable!()
        };
        args.json = true;
        let (result, requests) = execute(command, ResponseEnvelope::HookRegistered(row())).await;
        let error = result.unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<atm_core::error::AtmError>()
                .unwrap()
                .code(),
            atm_core::error::AtmErrorCode::HookModuleInvalid
        );
        assert!(requests.is_empty());
    }

    #[tokio::test]
    async fn disable_with_no_active_hooks_prints_none_and_exits_zero() {
        let command =
            HookCommand::try_parse_from(["hook", "disable", "--phase", TEST_TEAM]).unwrap();
        assert_eq!(
            execute(command, ResponseEnvelope::HookDisabled(vec![]))
                .await
                .0
                .unwrap(),
            format!("no active hooks for phase {TEST_TEAM}\n")
        );
    }

    #[tokio::test]
    async fn list_prints_inactive_rows_and_json_is_a_bare_array() {
        let command = HookCommand::try_parse_from(["hook", "list"]).unwrap();
        let output = execute(command, ResponseEnvelope::HookList(vec![row()]))
            .await
            .0
            .unwrap();
        assert!(
            output.contains("ACTIVE")
                && output.contains("false")
                && output.contains("DISABLED_AT")
                && output.contains("1 keys")
        );
        for rows in [vec![], vec![row()]] {
            let command = HookCommand::try_parse_from(["hook", "list", "--json"]).unwrap();
            let output = execute(command, ResponseEnvelope::HookList(rows.clone()))
                .await
                .0
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&output).unwrap(),
                serde_json::to_value(rows).unwrap()
            );
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum HookSubcommand {
    #[command(about = "Register a module for one task hook and phase")]
    Register(HookRegisterArgs),
    #[command(about = "Disable active registrations in a phase")]
    Disable(HookDisableArgs),
    #[command(about = "List active and inactive hook registrations")]
    List(HookListArgs),
}

#[derive(Debug, Args)]
pub struct HookRegisterArgs {
    #[arg(help = "Task hook name, validated by the daemon")]
    pub hook: String,
    #[arg(long, help = "Python module declaring TIMEOUT_MS")]
    pub module: PathBuf,
    #[arg(long, help = "Phase owning the registration")]
    pub phase: String,
    #[arg(long, help = "Working directory; defaults to the module directory")]
    pub cwd: Option<PathBuf>,
    #[arg(long, value_parser = parse_custom_object, help = "Custom JSON object passed to the module")]
    pub custom: Option<Map<String, Value>>,
    #[arg(long, help = "Emit the persisted registration as JSON")]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct HookDisableArgs {
    #[arg(long, help = "Phase whose active registrations will be disabled")]
    pub phase: String,
    #[arg(help = "Disable only this hook; otherwise disable every hook in the phase")]
    pub hook: Option<String>,
    #[arg(long, help = "Emit the disabled registrations as JSON")]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct HookListArgs {
    #[arg(long, help = "Limit registrations to this phase")]
    pub phase: Option<String>,
    #[arg(long, help = "Emit the full registrations as a bare JSON array")]
    pub json: bool,
}

impl HookCommand {
    pub async fn run(self, observability: &CliObservability) -> Result<()> {
        let caller = resolve_cli_caller_context(CallerContextOverrides::default())?;
        let request = self.request(caller)?;
        let (home, current) = resolve_command_runtime_context("hook")?;
        let composition = CliComposition::bootstrap(
            "hook",
            observability,
            InvocationDir::new(&current),
            AtmHomePath::new(&home),
        )?;
        preflight_daemon_api(&composition, HttpApiVersion::parse("1.13.0")?, "hook").await?;
        print!("{}", self.execute_request(&composition, request).await?);
        Ok(())
    }

    pub(super) fn request(&self, caller: CallerContext) -> Result<RequestEnvelope> {
        let caller_identity = caller.caller_identity;
        let caller_team = caller.caller_team;
        Ok(match &self.command {
            HookSubcommand::Register(args) => {
                let (module_path, cwd) = canonical_paths(&args.module, args.cwd.as_deref())?;
                let timeout_ms = read_timeout_ms(&module_path)?;
                RequestEnvelope::HookRegister(HookRegisterRequest {
                    caller_identity,
                    caller_team,
                    phase: PhaseId::parse(&args.phase)?,
                    hook: args.hook.clone(),
                    module_path,
                    cwd,
                    timeout_ms,
                    custom: Value::Object(args.custom.clone().unwrap_or_default()),
                })
            }
            HookSubcommand::Disable(args) => RequestEnvelope::HookDisable(HookDisableRequest {
                caller_identity,
                caller_team,
                phase: PhaseId::parse(&args.phase)?,
                hook: args.hook.clone(),
            }),
            HookSubcommand::List(args) => RequestEnvelope::HookList(HookListQuery {
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
            (HookSubcommand::Register(args), ResponseEnvelope::HookRegistered(row)) => {
                if args.json {
                    Ok(format!(
                        "{}\n",
                        serde_json::to_string(&serde_json::json!({
                            "action": "hook.register",
                            "registration": row,
                        }))?
                    ))
                } else {
                    Ok(format!(
                        "registered {} for {}/{} (timeout {} ms)\n",
                        row.hook, row.team, row.phase, row.timeout_ms
                    ))
                }
            }
            (HookSubcommand::Disable(args), ResponseEnvelope::HookDisabled(rows)) => {
                if args.json {
                    Ok(format!(
                        "{}\n",
                        serde_json::to_string(&serde_json::json!({
                            "action": "hook.disable",
                            "disabled": rows,
                            "count": rows.len(),
                        }))?
                    ))
                } else if rows.is_empty() {
                    Ok(format!("no active hooks for phase {}\n", args.phase))
                } else {
                    Ok(rows
                        .iter()
                        .map(|row| {
                            format!("disabled {} for {}/{}\n", row.hook, row.team, row.phase)
                        })
                        .collect())
                }
            }
            (HookSubcommand::List(args), ResponseEnvelope::HookList(rows)) => {
                crate::output::render_hook_list(&rows, args.json)
            }
            (_, other) => Err(atm_daemon_client::unexpected_response("hook", other).into()),
        }
    }
}
