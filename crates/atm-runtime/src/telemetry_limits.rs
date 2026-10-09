//! Worker limits for the task telemetry runtime.

use std::ops::RangeInclusive;
use std::time::Duration;

pub(crate) const DEFAULT_CAPACITY: usize = 256;
pub(crate) const DEFAULT_DRAIN: Duration = Duration::from_secs(2);
pub(crate) const MIN_DURATION: Duration = Duration::from_millis(1);
pub(crate) const MAX_DURATION: Duration = Duration::from_secs(30);
const QUEUE_CAPACITY: RangeInclusive<usize> = 1..=4096;

/// Accepts a queue of 1..=4096 records and a drain timeout of 1 ms..=30 s.
pub(crate) fn within_limits(queue_capacity: usize, drain_timeout: Duration) -> bool {
    QUEUE_CAPACITY.contains(&queue_capacity)
        && (MIN_DURATION..=MAX_DURATION).contains(&drain_timeout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_telemetry::TaskTelemetryConfig;

    #[test]
    fn task_config_validates_against_the_limit_rule() {
        let cases = [
            (0, DEFAULT_DRAIN, false),
            (1, MIN_DURATION, true),
            (4096, MAX_DURATION, true),
            (4097, DEFAULT_DRAIN, false),
            (DEFAULT_CAPACITY, Duration::ZERO, false),
            (DEFAULT_CAPACITY, MAX_DURATION + MIN_DURATION, false),
        ];
        for (queue_capacity, drain_timeout, expected) in cases {
            assert_eq!(within_limits(queue_capacity, drain_timeout), expected);
            let task = TaskTelemetryConfig {
                queue_capacity,
                drain_timeout,
            };
            assert_eq!(task.validate().is_ok(), expected, "{task:?}");
        }
    }
}
