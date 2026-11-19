//! Doctor collection for hook and cron registrations (design §9.10).
//!
//! Rows group by `(module_path, cwd, team, phase)`, so one module serving two
//! hooks and a schedule is one doctor call. The section never fails doctor.
//! A failed call only sets its entry's `error`. A failed hook-store read adds
//! one error finding and leaves the section empty; the router refreshes the
//! summary when it appends that finding.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use atm_core::api::RequestDeadline;
use atm_core::boundary::{
    CronSchedule, CronScheduleStore, HookRegistration, HookRegistrationStore,
};
use atm_core::doctor::hooks::{HookDoctorEntry, HookDoctorError, HookDoctorReply};
use atm_core::doctor::{DoctorFinding, DoctorSeverity};
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

/// The team whose hook registrations one doctor call reports, or `None` for
/// every team. The roster team the report is about comes first, then `--team`,
/// then the caller's `ATM_TEAM`. A call that names no team at all reports every
/// team, as `docs/atm-error-codes.md` (`atm doctor` team scope) states.
pub(crate) fn hook_team(
    all_teams: bool,
    roster_team: Option<TeamName>,
    team_override: Option<TeamName>,
    caller_team: Option<TeamName>,
) -> Option<TeamName> {
    if all_teams {
        return None;
    }
    roster_team.or(team_override).or(caller_team)
}

/// What one doctor call reports for hooks.
#[derive(Debug, Default)]
pub(crate) struct HookCollection {
    pub(crate) entries: Vec<HookDoctorEntry>,
    pub(crate) findings: Vec<DoctorFinding>,
}

/// `team` is the one team in scope, or `None` for every team.
pub(crate) async fn collect(
    router: &StorageAndNudgeRouter,
    team: Option<&TeamName>,
    deadline: RequestDeadline,
) -> HookCollection {
    match groups(router, team, deadline).await {
        Ok(groups) => HookCollection {
            entries: run_groups(router, groups, deadline).await,
            findings: Vec::new(),
        },
        Err(error) => HookCollection {
            entries: Vec::new(),
            findings: vec![store_read_finding(&error)],
        },
    }
}

/// The error finding for a failed hook-store read. The section stays empty,
/// and this finding is the only signal that rows exist but were not reported.
fn store_read_finding(error: &AtmError) -> DoctorFinding {
    DoctorFinding {
        severity: DoctorSeverity::Error,
        code: AtmErrorCode::StoreQueryFailed,
        message: format!("hook registrations could not be read: {}", error.message()),
        remediation: Some(
            "fix the store read failure and rerun atm doctor; hook rows are omitted until it succeeds"
                .to_owned(),
        ),
    }
}

async fn groups(
    router: &StorageAndNudgeRouter,
    team: Option<&TeamName>,
    deadline: RequestDeadline,
) -> Result<Vec<Group>, AtmError> {
    let hooks = router.service_runtime.hook_registration_store()?;
    // A runtime without a cron store has no schedules; hook rows still report.
    let cron = router.service_runtime.cron_schedule_store().ok();
    // The store reads are one control-path bridge job, so they serialize with
    // registry writes and never run on the request task.
    let (registrations, cron_rows) = router
        .control_path_sync_bridge
        .run(deadline, move || read_rows(hooks.as_ref(), cron.as_deref()))
        .await?;
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
    Ok(rows
        .into_values()
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
        .collect())
}

/// The active hook rows and schedules, read as one store snapshot.
fn read_rows(
    hooks: &dyn HookRegistrationStore,
    cron: Option<&dyn CronScheduleStore>,
) -> Result<(Vec<HookRegistration>, Vec<CronSchedule>), AtmError> {
    let registrations = hooks.active_all()?;
    let cron_rows = match cron {
        Some(schedules) => schedules.active_all()?,
        None => Vec::new(),
    };
    Ok((registrations, cron_rows))
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

    use atm_core::boundary::{CronScheduleStore, NewCronSchedule, StopCondition};
    use atm_core::boundary::{
        DummyHookRunner, HookRegistration, HookRegistrationStore, HookRunner,
    };
    use atm_core::doctor::DoctorSeverity;
    use atm_core::doctor::hooks::HookDoctorReply;
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_runtime_test_support::{InMemoryCronScheduleStore, InMemoryHookRegistrationStore};
    use serde_json::{Map, Value, json};
    use std::sync::atomic::Ordering;

    use super::{collect, hook_team};
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

    fn other_team() -> atm_core::types::TeamName {
        "other-team".parse().expect("team")
    }

    #[test]
    fn scope_resolves_roster_then_override_then_caller() {
        assert_eq!(hook_team(true, Some(team()), None, None), None);
        assert_eq!(
            hook_team(false, Some(team()), Some(other_team()), None),
            Some(team())
        );
        assert_eq!(
            hook_team(false, None, Some(other_team()), Some(team())),
            Some(other_team())
        );
        assert_eq!(hook_team(false, None, None, Some(team())), Some(team()));
        // A call that names no team reports every team, as the error-code doc states.
        assert_eq!(hook_team(false, None, None, None), None);
    }

    #[tokio::test]
    async fn team_scope_excludes_other_teams_rows() {
        let setup = setup(true);
        let mut foreign = row("task.assign", "bj", 1);
        foreign.team = other_team();
        setup.hooks.register(foreign).expect("register");

        let collection = collect(&setup.router, Some(&team()), deadline()).await;

        assert!(collection.entries.is_empty());
        assert!(collection.findings.is_empty());
        assert!(setup.runner.calls.lock().expect("calls").is_empty());
    }

    #[tokio::test]
    async fn schedule_row_joins_its_hook_group_with_team_phase_and_custom_asserted() {
        let mut setup = setup(true);
        let cron = Arc::new(InMemoryCronScheduleStore::new());
        setup.router.service_runtime = setup
            .router
            .service_runtime
            .clone()
            .with_cron_schedule_store(cron.clone());
        // The schedule shares the bj module with a hook row, so they form one group.
        setup
            .hooks
            .register(row("task.assign", "bj", 1))
            .expect("register");
        let mut custom = Map::new();
        custom.insert("n".to_owned(), json!(7));
        cron.start(NewCronSchedule {
            team: team(),
            phase: phase("bj"),
            name: "nightly".to_owned(),
            cron_expr: "0 3 * * *".to_owned(),
            module_path: "/modules/bj.wasm".into(),
            cwd: "/work".into(),
            args: Vec::new(),
            timeout_ms: 1_000,
            jitter_s: 0,
            custom,
            stop: StopCondition::Hook,
            created_by: agent(atm_core::roles::ROLE_TEAM_LEAD),
            created_at: at(2),
        })
        .expect("schedule");
        setup
            .runner
            .doctor_replies
            .lock()
            .expect("replies")
            .push_back(ok_reply());

        let entries = collect(&setup.router, Some(&team()), deadline())
            .await
            .entries;

        assert_eq!(
            entries.len(),
            1,
            "the hook row and the schedule share one group"
        );
        let entry = &entries[0];
        assert_eq!(entry.team, team());
        assert_eq!(entry.phase.as_str(), "bj");
        assert_eq!(entry.hooks, vec![hook("task.assign")]);
        assert_eq!(entry.schedules, vec!["nightly".to_owned()]);
        let calls = setup.runner.calls.lock().expect("calls");
        assert_eq!(calls.len(), 1, "one doctor call for the group");
        // The hook row sorts first, so its custom (n = 1) is the group's target custom.
        assert_eq!(calls[0].0.custom.get("n"), Some(&Value::from(1)));
    }

    #[tokio::test]
    async fn all_teams_scope_reports_every_team() {
        let setup = setup(true);
        let mut foreign = row("task.assign", "bj", 1);
        foreign.team = other_team();
        setup.hooks.register(foreign).expect("register");
        setup
            .runner
            .doctor_replies
            .lock()
            .expect("replies")
            .push_back(ok_reply());

        let collection = collect(&setup.router, None, deadline()).await;

        assert_eq!(collection.entries.len(), 1);
        assert_eq!(collection.entries[0].team, other_team());
    }

    #[tokio::test]
    async fn failed_store_read_reports_one_error_finding_and_no_entries() {
        let setup = setup(true);
        setup
            .hooks
            .register(row("task.assign", "bj", 1))
            .expect("register");
        // Deterministic failure: the store double fails its next read, with no timing.
        setup.hooks.fail_active_all.store(true, Ordering::SeqCst);

        let collection = collect(&setup.router, Some(&team()), deadline()).await;

        assert!(
            collection.entries.is_empty(),
            "the hook section stays empty"
        );
        assert_eq!(collection.findings.len(), 1);
        let finding = &collection.findings[0];
        assert_eq!(finding.severity, DoctorSeverity::Error);
        assert_eq!(finding.code, AtmErrorCode::StoreQueryFailed);
        assert!(
            finding
                .message
                .contains("hook registrations could not be read")
        );
        assert!(
            finding
                .message
                .contains("injected hook registration active_all failure")
        );
        assert!(setup.runner.calls.lock().expect("calls").is_empty());
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

        let entries = collect(&setup.router, Some(&team()), deadline())
            .await
            .entries;

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
    async fn failed_call_sets_only_the_entry_error() {
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

        let entries = collect(&setup.router, Some(&team()), deadline())
            .await
            .entries;

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
        let entries = collect(&setup.router, Some(&team()), deadline())
            .await
            .entries;
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

        let entries = collect(&setup.router, Some(&team()), deadline())
            .await
            .entries;

        assert_eq!(entries.len(), 1);
        let error = entries[0].error.as_ref().expect("error set on the entry");
        assert_eq!(error.code, AtmErrorCode::DaemonUnavailable);
        assert_eq!(error.message, "hook runner was not installed");
    }
}
