//! Worker limits shared by the task and workflow telemetry runtimes.

use std::ops::RangeInclusive;
use std::time::Duration;

pub(crate) const DEFAULT_CAPACITY: usize = 256;
pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(1);
pub(crate) const DEFAULT_DRAIN: Duration = Duration::from_secs(2);
pub(crate) const MIN_DURATION: Duration = Duration::from_millis(1);
pub(crate) const MAX_DURATION: Duration = Duration::from_secs(30);
const QUEUE_CAPACITY: RangeInclusive<usize> = 1..=4096;

/// Accepts a queue of 1..=4096 records and timeouts of 1 ms..=30 s.
pub(crate) fn within_limits(
    queue_capacity: usize,
    emit_timeout: Duration,
    drain_timeout: Duration,
) -> bool {
    let durations = MIN_DURATION..=MAX_DURATION;
    QUEUE_CAPACITY.contains(&queue_capacity)
        && durations.contains(&emit_timeout)
        && durations.contains(&drain_timeout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_telemetry::TaskTelemetryConfig;
    use crate::workflow_telemetry::WorkflowTelemetryConfig;

    #[test]
    fn task_and_workflow_configs_share_one_limit_rule() {
        let cases = [
            (0, DEFAULT_TIMEOUT, DEFAULT_DRAIN),
            (1, MIN_DURATION, MAX_DURATION),
            (4096, MAX_DURATION, MIN_DURATION),
            (4097, DEFAULT_TIMEOUT, DEFAULT_DRAIN),
            (DEFAULT_CAPACITY, Duration::ZERO, DEFAULT_DRAIN),
            (
                DEFAULT_CAPACITY,
                DEFAULT_TIMEOUT,
                MAX_DURATION + MIN_DURATION,
            ),
        ];
        for (queue_capacity, emit_timeout, drain_timeout) in cases {
            let expected = within_limits(queue_capacity, emit_timeout, drain_timeout);
            let task = TaskTelemetryConfig {
                queue_capacity,
                emit_timeout,
                drain_timeout,
            };
            let workflow = WorkflowTelemetryConfig {
                queue_capacity,
                emit_timeout,
                drain_timeout,
            };
            assert_eq!(task.validate().is_ok(), expected, "{task:?}");
            assert_eq!(workflow.validate().is_ok(), expected, "{workflow:?}");
        }
        assert!(within_limits(1, MIN_DURATION, MAX_DURATION));
        assert!(!within_limits(4097, DEFAULT_TIMEOUT, DEFAULT_DRAIN));
        let task = TaskTelemetryConfig::default();
        let workflow = WorkflowTelemetryConfig::default();
        assert_eq!(
            (task.queue_capacity, task.emit_timeout, task.drain_timeout),
            (
                workflow.queue_capacity,
                workflow.emit_timeout,
                workflow.drain_timeout
            )
        );
    }
}
