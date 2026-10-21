use std::path::PathBuf;

use async_trait::async_trait;

use super::{CronName, CronSchedule, PhaseId, StopCondition, sealed};
use crate::error::AtmError;
use crate::types::{AgentName, TeamName};

#[async_trait]
pub trait CronScheduler: sealed::Sealed + Send + Sync {
    async fn start(&self, spec: CronStartSpec) -> Result<CronSchedule, AtmError>;
    async fn end(
        &self,
        team: &TeamName,
        phase: &PhaseId,
        name: Option<&CronName>,
    ) -> Result<Vec<CronSchedule>, AtmError>;
    async fn list(
        &self,
        team: Option<&TeamName>,
        phase: Option<&PhaseId>,
    ) -> Result<Vec<CronSchedule>, AtmError>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct CronStartSpec {
    pub team: TeamName,
    pub phase: PhaseId,
    pub name: CronName,
    pub cron_expr: String,
    pub module_path: PathBuf,
    pub cwd: PathBuf,
    pub args: Vec<String>,
    pub timeout_ms: u32,
    pub custom: serde_json::Map<String, serde_json::Value>,
    pub stop: StopCondition,
    pub created_by: AgentName,
}

#[cfg(any(test, feature = "test-utils"))]
#[derive(Default)]
pub struct RecordingCronScheduler {
    pub rows: std::sync::Mutex<Vec<CronSchedule>>,
    pub calls: std::sync::Mutex<Vec<&'static str>>,
    pub start_pending: std::sync::atomic::AtomicBool,
}

#[cfg(any(test, feature = "test-utils"))]
impl sealed::Sealed for RecordingCronScheduler {}

#[cfg(any(test, feature = "test-utils"))]
#[async_trait]
impl CronScheduler for RecordingCronScheduler {
    async fn start(&self, spec: CronStartSpec) -> Result<CronSchedule, AtmError> {
        self.calls
            .lock()
            .expect("cron calls mutex poisoned")
            .push("start");
        if self.start_pending.load(std::sync::atomic::Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        let mut rows = self.rows.lock().expect("cron rows mutex poisoned");
        let existing = rows.iter().position(|row| {
            row.team == spec.team && row.phase == spec.phase && row.name == spec.name.as_str()
        });
        if existing.is_some_and(|index| rows[index].active) {
            return Err(AtmError::validation_with_recovery(
                "cron schedule is already active",
                "run atm cron end before restarting the schedule",
            ));
        }
        let id = existing.map(|index| rows[index].id).unwrap_or_else(|| {
            super::CronScheduleId::new(rows.iter().map(|row| row.id.get()).max().unwrap_or(0) + 1)
        });
        let generation = existing.map_or(super::CronGeneration::FIRST, |index| {
            rows[index]
                .generation
                .next()
                .expect("cron generation overflow")
        });
        let row = CronSchedule {
            id,
            generation,
            team: spec.team,
            phase: spec.phase,
            name: spec.name.into(),
            cron_expr: spec.cron_expr,
            module_path: spec.module_path,
            cwd: spec.cwd,
            args: spec.args,
            timeout_ms: spec.timeout_ms,
            jitter_s: 0,
            custom: spec.custom,
            stop: spec.stop,
            active: true,
            created_by: spec.created_by,
            created_at: "2026-10-18T00:00:00Z"
                .parse()
                .expect("fixed test timestamp is valid"),
            last_run_at: None,
            last_outcome: None,
            ended_at: None,
        };
        if let Some(index) = existing {
            rows[index] = row.clone();
        } else {
            rows.push(row.clone());
        }
        Ok(row)
    }

    async fn end(
        &self,
        team: &TeamName,
        phase: &PhaseId,
        name: Option<&CronName>,
    ) -> Result<Vec<CronSchedule>, AtmError> {
        self.calls
            .lock()
            .expect("cron calls mutex poisoned")
            .push("end");
        let mut rows = self.rows.lock().expect("cron rows mutex poisoned");
        let mut ended = Vec::new();
        for row in rows.iter_mut().filter(|row| {
            &row.team == team
                && &row.phase == phase
                && name.is_none_or(|name| row.name == name.as_str())
                && row.active
        }) {
            row.active = false;
            row.ended_at = Some(
                "2026-10-18T00:00:00Z"
                    .parse()
                    .expect("fixed test timestamp is valid"),
            );
            ended.push(row.clone());
        }
        Ok(ended)
    }

    async fn list(
        &self,
        team: Option<&TeamName>,
        phase: Option<&PhaseId>,
    ) -> Result<Vec<CronSchedule>, AtmError> {
        self.calls
            .lock()
            .expect("cron calls mutex poisoned")
            .push("list");
        Ok(self
            .rows
            .lock()
            .expect("cron rows mutex poisoned")
            .iter()
            .filter(|row| {
                team.is_none_or(|team| &row.team == team)
                    && phase.is_none_or(|phase| &row.phase == phase)
            })
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    use super::*;

    fn ready<F: Future>(future: F) -> F::Output {
        let mut future = Box::pin(future);
        match future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("recording scheduler unexpectedly blocked"),
        }
    }

    fn spec(team: &str, phase: &str, name: &str) -> CronStartSpec {
        CronStartSpec {
            team: team.parse().unwrap(),
            phase: PhaseId::parse(phase).unwrap(),
            name: CronName::parse(name).unwrap(),
            cron_expr: "0 * * * * *".into(),
            module_path: "/module.py".into(),
            cwd: "/work".into(),
            args: vec!["one".into()],
            timeout_ms: 1000,
            custom: serde_json::Map::from_iter([(
                "goal".into(),
                serde_json::json!([1, {"x": null}]),
            )]),
            stop: StopCondition::Hook,
            created_by: "solar".parse().unwrap(),
        }
    }

    #[test]
    fn scheduler_records_and_filters_without_clock_or_io() {
        let scheduler = RecordingCronScheduler::default();
        let first_spec = spec("test-team", "bk", "first");
        let first = ready(scheduler.start(first_spec.clone())).unwrap();
        assert_eq!(first.id.get(), 1);
        assert_eq!(first.generation.get(), 1);
        assert_eq!(first.custom, first_spec.custom);
        assert_eq!(first.stop, first_spec.stop);
        assert!(ready(scheduler.start(first_spec.clone())).is_err());
        let other = ready(scheduler.start(spec("other", "bk", "second"))).unwrap();
        assert_eq!(other.id.get(), 2);
        assert_eq!(
            ready(scheduler.list(Some(&first_spec.team), None)).unwrap(),
            vec![first.clone()]
        );
        assert!(
            ready(scheduler.list(None, Some(&PhaseId::parse("different").unwrap())))
                .unwrap()
                .is_empty()
        );
        let ended = ready(scheduler.end(
            &first_spec.team,
            &first_spec.phase,
            Some(&CronName::parse("first").unwrap()),
        ))
        .unwrap();
        assert_eq!(ended.len(), 1);
        assert!(!ended[0].active);
        assert!(ended[0].ended_at.is_some());
        assert!(
            ready(scheduler.end(&first_spec.team, &first_spec.phase, None))
                .unwrap()
                .is_empty()
        );
        let restarted = ready(scheduler.start(first_spec)).unwrap();
        assert_eq!(restarted.id, first.id);
        assert_eq!(restarted.generation.get(), 2);
        assert!(restarted.active);
        assert_eq!(restarted.ended_at, None);
        let all = ready(scheduler.list(None, None)).unwrap();
        assert_eq!(all, vec![restarted, other]);
        assert_eq!(
            *scheduler.calls.lock().unwrap(),
            vec![
                "start", "start", "start", "list", "list", "end", "end", "start", "list"
            ]
        );
    }
}
