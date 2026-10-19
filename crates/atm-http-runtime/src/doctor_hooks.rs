//! Doctor collection for hook and cron registrations (design §9.10).
//!
//! Rows group by `(module_path, cwd, team, phase)`, so one module serving two
//! hooks and a schedule is one doctor call. The section never fails doctor and
//! never changes its summary: a failed call only sets its entry's `error`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use atm_core::api::RequestDeadline;
use atm_core::boundary::{CronSchedule, HookRegistration};
use atm_core::doctor::hooks::{HookDoctorEntry, HookDoctorError, HookDoctorReply};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::hook::HookTarget;
use atm_core::types::TeamName;

use crate::storage_and_nudge_router::StorageAndNudgeRouter;

type GroupKey = (PathBuf, PathBuf, String, String);

/// One doctor call: the rows sharing a module, cwd, team and phase.
struct Group {
    target: HookTarget,
    entry: HookDoctorEntry,
}

pub(crate) async fn collect(
    router: &StorageAndNudgeRouter,
    team: Option<&TeamName>,
    deadline: RequestDeadline,
) -> Vec<HookDoctorEntry> {
    let Some(groups) = groups(router, team) else {
        return Vec::new();
    };
    run_groups(router, groups, deadline).await
}

fn groups(router: &StorageAndNudgeRouter, team: Option<&TeamName>) -> Option<Vec<Group>> {
    let hooks = router.service_runtime.hook_registration_store().ok()?;
    let registrations = hooks.active_all().ok()?;
    // A runtime without a cron store has no schedules; hook rows still report.
    let cron_rows = match router.service_runtime.cron_schedule_store() {
        Ok(schedules) => schedules.active_all().ok()?,
        Err(_) => Vec::new(),
    };
    let in_scope = |row_team: &TeamName| team.is_none_or(|team| team == row_team);

    let mut rows: BTreeMap<GroupKey, (Vec<HookRegistration>, Vec<CronSchedule>)> = BTreeMap::new();
    for row in registrations.into_iter().filter(|row| in_scope(&row.team)) {
        rows.entry(group_key(&row.module_path, &row.cwd, &row.team, &row.phase))
            .or_default()
            .0
            .push(row);
    }
    for row in cron_rows.into_iter().filter(|row| in_scope(&row.team)) {
        rows.entry(group_key(&row.module_path, &row.cwd, &row.team, &row.phase))
            .or_default()
            .1
            .push(row);
    }
    Some(
        rows.into_values()
            .map(|(mut hooks, mut schedules)| {
                hooks.sort_by(|a, b| a.hook.as_str().cmp(b.hook.as_str()));
                schedules.sort_by(|a, b| a.name.cmp(&b.name));
                let target = match (hooks.first(), schedules.first()) {
                    (Some(row), _) => HookTarget::from(row),
                    (None, Some(row)) => HookTarget::from(row),
                    (None, None) => unreachable!("a group holds at least one row"),
                };
                let entry = HookDoctorEntry {
                    module_path: target.module_path.clone(),
                    cwd: target.cwd.clone(),
                    team: target.team.clone(),
                    phase: target.phase.clone(),
                    hooks: hooks.iter().map(|row| row.hook.clone()).collect(),
                    schedules: schedules.iter().map(|row| row.name.clone()).collect(),
                    timeout_ms: target.timeout_ms,
                    success: None,
                    message: None,
                    findings: Vec::new(),
                    error: None,
                };
                Group { target, entry }
            })
            .collect(),
    )
}

fn group_key(
    module: &std::path::Path,
    cwd: &std::path::Path,
    team: &TeamName,
    phase: &impl std::fmt::Display,
) -> GroupKey {
    (
        module.to_path_buf(),
        cwd.to_path_buf(),
        team.as_str().to_owned(),
        phase.to_string(),
    )
}

async fn run_groups(
    router: &StorageAndNudgeRouter,
    groups: Vec<Group>,
    deadline: RequestDeadline,
) -> Vec<HookDoctorEntry> {
    let Some(runner) = router.hooks.runner.clone() else {
        return groups
            .into_iter()
            .map(|group| {
                with_error(
                    group.entry,
                    AtmErrorCode::DaemonUnavailable,
                    "hook runner was not installed",
                )
            })
            .collect();
    };
    let doctor_deadline: RequestDeadline = deadline.minus(Duration::from_millis(250));
    let mut calls = tokio::task::JoinSet::new();
    for (index, group) in groups.iter().enumerate() {
        let runner = Arc::clone(&runner);
        let target = group.target.clone();
        calls.spawn(async move { (index, runner.doctor(&target, doctor_deadline).await) });
    }
    let mut results: Vec<Option<Result<HookDoctorReply, AtmError>>> =
        (0..groups.len()).map(|_| None).collect();
    while let Some(joined) = calls.join_next().await {
        if let Ok((index, result)) = joined {
            results[index] = Some(result);
        }
    }
    groups
        .into_iter()
        .zip(results)
        .map(|(group, result)| match result {
            Some(Ok(reply)) => HookDoctorEntry {
                success: Some(reply.success),
                message: Some(reply.message),
                findings: reply.findings,
                ..group.entry
            },
            Some(Err(error)) => with_error(group.entry, error.code(), error.message()),
            None => with_error(
                group.entry,
                AtmErrorCode::DaemonUnavailable,
                "hook doctor call did not complete",
            ),
        })
        .collect()
}

fn with_error(entry: HookDoctorEntry, code: AtmErrorCode, message: &str) -> HookDoctorEntry {
    HookDoctorEntry {
        error: Some(HookDoctorError {
            code,
            message: message.to_owned(),
        }),
        ..entry
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use atm_core::boundary::{
        DummyHookRunner, HookRegistration, HookRegistrationStore, HookRunner,
    };
    use atm_core::doctor::hooks::HookDoctorReply;
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_runtime_test_support::InMemoryHookRegistrationStore;
    use serde_json::{Map, Value, json};

    use super::collect;
    use crate::hook_filter::ActiveHookSnapshot;
    use crate::hook_filter::test_support::{agent, at, deadline, hook, phase, team};
    use crate::storage_and_nudge_router::StorageAndNudgeRouter;
    use crate::storage_and_nudge_router::tests::fixture;

    struct Setup {
        _fixture: crate::storage_and_nudge_router::tests::Fixture,
        router: StorageAndNudgeRouter,
        hooks: Arc<InMemoryHookRegistrationStore>,
        runner: Arc<DummyHookRunner>,
    }

    fn setup(with_runner: bool) -> Setup {
        let fixture = fixture(true, None, None);
        let hooks = Arc::new(InMemoryHookRegistrationStore::default());
        let runner = Arc::new(DummyHookRunner::default());
        let mut router = fixture.router.clone();
        router.service_runtime = router
            .service_runtime
            .clone()
            .with_hook_registration_store(hooks.clone());
        if with_runner {
            let snapshot = ActiveHookSnapshot::load(hooks.clone()).expect("snapshot");
            router = crate::hook_filter::test_support::install_filter(
                router,
                Arc::new(snapshot),
                runner.clone() as Arc<dyn HookRunner>,
            );
        }
        Setup {
            _fixture: fixture,
            router,
            hooks,
            runner,
        }
    }

    /// A registration in its own module per phase, with the given custom map.
    fn row(hook_name: &str, phase_name: &str, n: u64) -> HookRegistration {
        let mut custom = Map::new();
        custom.insert("n".to_owned(), json!(n));
        HookRegistration {
            team: team(),
            phase: phase(phase_name),
            hook: hook(hook_name),
            module_path: format!("/modules/{phase_name}.wasm").into(),
            cwd: "/work".into(),
            timeout_ms: 1_000,
            custom,
            active: true,
            registered_by: agent(atm_core::roles::ROLE_TEAM_LEAD),
            registered_at: at(1),
            disabled_at: None,
        }
    }

    fn ok_reply() -> Result<HookDoctorReply, AtmError> {
        HookDoctorReply::parse(br#"{"success":true,"message":"ok"}"#)
    }

    #[tokio::test]
    async fn one_entry_per_module_phase_with_first_row_custom() {
        let setup = setup(true);
        // Two hooks share the bj module, so they form one group; bk is its own group.
        setup
            .hooks
            .register(row("task.start", "bj", 2))
            .expect("register");
        setup
            .hooks
            .register(row("task.assign", "bj", 1))
            .expect("register");
        setup
            .hooks
            .register(row("task.assign", "bk", 3))
            .expect("register");
        // Replies are popped in call order, which the concurrent calls do not fix,
        // so both replies are identical and the assertions read the set of results.
        for _ in 0..2 {
            setup
                .runner
                .doctor_replies
                .lock()
                .expect("replies")
                .push_back(ok_reply());
        }

        let entries = collect(&setup.router, Some(&team()), deadline()).await;

        assert_eq!(entries.len(), 2, "one entry per module and phase");
        let bj = entries
            .iter()
            .find(|entry| entry.phase.as_str() == "bj")
            .expect("bj entry");
        assert_eq!(bj.hooks.len(), 2);
        assert_eq!(bj.success, Some(true));
        assert!(bj.error.is_none());
        let calls = setup.runner.calls.lock().expect("calls");
        assert_eq!(calls.len(), 2, "one doctor call per group");
        let bj_target = calls
            .iter()
            .map(|(target, _, _)| target)
            .find(|target| target.phase.as_str() == "bj")
            .expect("bj target");
        // The first row by hook name is task.assign, whose custom is n = 1.
        assert_eq!(bj_target.custom.get("n"), Some(&Value::from(1)));
    }

    #[tokio::test]
    async fn failed_call_sets_error_and_summary_is_unchanged() {
        let setup = setup(true);
        setup
            .hooks
            .register(row("task.assign", "bj", 1))
            .expect("register");
        setup
            .runner
            .doctor_replies
            .lock()
            .expect("replies")
            .push_back(Err(AtmError::hook_malformed("doctor reply broken")));

        let entries = collect(&setup.router, Some(&team()), deadline()).await;

        assert_eq!(entries.len(), 1);
        let error = entries[0].error.as_ref().expect("error set on the entry");
        assert_eq!(error.code, AtmErrorCode::HookMalformed);
        assert!(
            entries[0].success.is_none(),
            "a failed call reports no success"
        );
        assert!(entries[0].message.is_none());
    }

    #[tokio::test]
    async fn no_registration_gives_empty_hooks() {
        let setup = setup(true);
        let entries = collect(&setup.router, Some(&team()), deadline()).await;
        assert!(entries.is_empty());
        assert!(setup.runner.calls.lock().expect("calls").is_empty());
    }

    #[tokio::test]
    async fn missing_runner_reports_each_group_unavailable() {
        let setup = setup(false);
        setup
            .hooks
            .register(row("task.assign", "bj", 1))
            .expect("register");

        let entries = collect(&setup.router, Some(&team()), deadline()).await;

        assert_eq!(entries.len(), 1);
        let error = entries[0].error.as_ref().expect("error set on the entry");
        assert_eq!(error.code, AtmErrorCode::DaemonUnavailable);
        assert_eq!(error.message, "hook runner was not installed");
    }
}
