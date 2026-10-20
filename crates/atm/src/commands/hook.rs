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
    use atm_core::protocol::{CLI_SCHEMA_VERSION, CompatibilityVerdict, ReleaseVersion};
    use atm_core::transport::testing::FakeClientTransport;
    use std::sync::{Arc, Mutex};

    pub(crate) fn caller() -> CallerContext {
        CallerContext {
            caller_identity: "solar".parse().unwrap(),
            caller_chat_id: None,
            caller_team: "bk".parse().unwrap(),
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

    pub(crate) fn row() -> atm_storage::HookRegistration {
        serde_json::from_value(serde_json::json!({
            "team":"bk", "phase":"bk", "hook":"task.close", "module_path":"hook.py", "cwd":"/repo",
            "timeout_ms":2500,"custom":{"goal":"ship"},"active":false,"registered_by":"solar",
            "registered_at":"2026-10-19T00:00:00Z","disabled_at":"2026-10-19T01:00:00Z"
        }))
        .unwrap()
    }

    pub(super) fn register(module: &str, name: &str) -> HookCommand {
        HookCommand {
            command: HookSubcommand::Register(HookRegisterArgs {
                hook: name.into(),
                module: fixture(module),
                phase: "bk".into(),
                cwd: None,
                custom: None,
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
                "registered task.close for bk/bk (timeout 2500 ms)\n"
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
            HookCommand::try_parse_from(["hook", "disable", "--phase", "bk", "task.close"])
                .unwrap();
        let (result, requests) =
            execute(command, ResponseEnvelope::HookDisabled(vec![row(), row()])).await;
        assert_eq!(result.unwrap().lines().count(), 2);
        let [RequestEnvelope::HookDisable(request)] = requests.as_slice() else {
            panic!("{requests:?}")
        };
        assert_eq!(request.phase.as_str(), "bk");
        assert_eq!(request.hook.as_deref(), Some("task.close"));
    }

    #[tokio::test]
    async fn disable_with_no_active_hooks_prints_none_and_exits_zero() {
        let command = HookCommand::try_parse_from(["hook", "disable", "--phase", "bk"]).unwrap();
        assert_eq!(
            execute(command, ResponseEnvelope::HookDisabled(vec![]))
                .await
                .0
                .unwrap(),
            "no active hooks for phase bk\n"
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
}

#[derive(Debug, Args)]
pub struct HookDisableArgs {
    #[arg(long, help = "Phase whose active registrations will be disabled")]
    pub phase: String,
    #[arg(help = "Disable only this hook; otherwise disable every hook in the phase")]
    pub hook: Option<String>,
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
        if let HookSubcommand::List(args) = &self.command {
            preflight_daemon_api(&composition, HttpApiVersion::parse("1.13.0")?, "hook").await?;
            return match composition.execute_request(request).await? {
                ResponseEnvelope::HookList(rows) => {
                    crate::output::print_hook_list(&rows, args.json)
                }
                other => Err(atm_daemon_client::unexpected_response("hook list", other).into()),
            };
        }
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
        preflight_daemon_api(composition, HttpApiVersion::parse("1.13.0")?, "hook").await?;
        match composition.execute_request(request).await? {
            ResponseEnvelope::HookRegistered(row) => Ok(format!(
                "registered {} for {}/{} (timeout {} ms)\n",
                row.hook, row.team, row.phase, row.timeout_ms
            )),
            ResponseEnvelope::HookDisabled(rows) => {
                if rows.is_empty() {
                    let HookSubcommand::Disable(args) = &self.command else {
                        unreachable!()
                    };
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
            ResponseEnvelope::HookList(rows) => {
                let HookSubcommand::List(args) = &self.command else {
                    unreachable!()
                };
                crate::output::render_hook_list(&rows, args.json)
            }
            other => Err(atm_daemon_client::unexpected_response("hook", other).into()),
        }
    }
}
