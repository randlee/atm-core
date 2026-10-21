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
    use super::super::hook::tests::{caller, fixture, transport};
    use super::*;
    use atm_core::test_support::{TEST_SENDER, TEST_TEAM};

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
}

#[derive(Debug, Parser)]
#[command(about = "Start, end, or list cron schedules")]
pub struct CronCommand {
    #[command(subcommand)]
    pub command: CronSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum CronSubcommand {
    #[command(about = "Start a recurring module schedule with a mandatory stop condition")]
    Start(CronStartArgs),
    #[command(about = "End active schedules in a phase")]
    End(CronEndArgs),
    #[command(about = "List active and inactive schedules")]
    List(CronListArgs),
}

#[derive(Debug, Args)]
#[command(group = clap::ArgGroup::new("stop").required(true).multiple(false).args(["until", "until_hook"]))]
pub struct CronStartArgs {
    #[arg(help = "Schedule name within the phase")]
    pub name: String,
    #[arg(
        long = "cron",
        help = "Six-field UTC cron expression, validated by the daemon"
    )]
    pub cron_expr: String,
    #[arg(long, help = "Python module declaring its timeout")]
    pub module: PathBuf,
    #[arg(long, help = "Phase owning the schedule")]
    pub phase: String,
    #[arg(long, value_parser = parse_rfc3339_utc, help = "Stop at this RFC 3339 instant")]
    pub until: Option<DateTime<Utc>>,
    #[arg(
        long = "until-hook",
        help = "Ask the module stop hook before each tick"
    )]
    pub until_hook: bool,
    #[arg(long = "arg", action = clap::ArgAction::Append, allow_hyphen_values = true, help = "Module argument; repeat for each argument, including flags")]
    pub args: Vec<String>,
    #[arg(long, help = "Working directory; defaults to the module directory")]
    pub cwd: Option<PathBuf>,
    #[arg(long, value_parser = parse_custom_object, help = "Custom JSON object passed to the module")]
    pub custom: Option<Map<String, Value>>,
}

#[derive(Debug, Args)]
pub struct CronEndArgs {
    #[arg(long, help = "Phase whose active schedules will be ended")]
    pub phase: String,
    #[arg(help = "End only this schedule; otherwise end every schedule in the phase")]
    pub name: Option<String>,
}

#[derive(Debug, Args)]
pub struct CronListArgs {
    #[arg(long, help = "Limit schedules to this phase")]
    pub phase: Option<String>,
    #[arg(long, help = "Emit full schedules as a bare JSON array")]
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
