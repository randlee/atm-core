//! Pure task-nudge policy for one accepted roster observation.

use atm_core::boundary::{
    TASK_CONSECUTIVE_REFUSAL_THRESHOLD, TASK_REMINDER_INTERVAL_MS, TASK_STALLED_REMINDER_THRESHOLD,
    TaskRow, TaskState,
};
use atm_core::protocol::RuntimeMemberState;
use atm_core::types::IsoTimestamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskDisposition {
    Nudge,
    ResetReminders,
    EscalateStalled,
    EscalateEpisode(EpisodeKind),
    Hold(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EpisodeKind {
    Blocked,
    Offline,
}

impl EpisodeKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Blocked => "blocked",
            Self::Offline => "offline",
        }
    }

    pub(crate) const fn runtime_state(self) -> RuntimeMemberState {
        match self {
            Self::Blocked => RuntimeMemberState::Blocked,
            Self::Offline => RuntimeMemberState::Offline,
        }
    }
}

/// Decides the one nudge/escalation action for an accepted roster
/// observation.
///
/// `active_since` is the timestamp the member entered its current runtime
/// state (`RosterRuntimeObservation::state_changed_at`). It only matters
/// while `state` is [`RuntimeMemberState::Active`]: the reminder budget
/// resets only once the assignee has been continuously active for at least
/// one reminder interval, so a quick reply between idle turns does not
/// silently clear an escalated task.
pub(crate) fn dispose(
    mail_pending: bool,
    state: RuntimeMemberState,
    head: Option<&TaskRow>,
    now: IsoTimestamp,
    active_since: Option<IsoTimestamp>,
    new_episode: bool,
    consecutive_refusals: u32,
) -> TaskDisposition {
    use RuntimeMemberState as S;
    use TaskDisposition as D;

    match (mail_pending, state, head) {
        // The ledger's Started transition is the authoritative work signal.
        // Codex panes can report Idle throughout a running task, so roster
        // Idle must not override an Active task row. Explicit Blocked/Offline
        // observations still take the episode-escalation path below.
        (_, S::Idle, Some(task)) if task.state == TaskState::Active => D::Hold("task active"),
        (_, S::Active, Some(task))
            if task.reminder_count > 0 && sustained_active(active_since, now) =>
        {
            D::ResetReminders
        }
        (_, S::Active, _) => D::Hold("active"),
        (_, S::Blocked, Some(_)) => {
            if new_episode {
                D::EscalateEpisode(EpisodeKind::Blocked)
            } else {
                D::Hold("episode reported")
            }
        }
        (_, S::Blocked, None) => D::Hold("no open task"),
        (_, S::Offline, Some(_)) => {
            if new_episode {
                D::EscalateEpisode(EpisodeKind::Offline)
            } else {
                D::Hold("episode reported")
            }
        }
        (_, S::Offline, None) => D::Hold("no open task"),
        (_, S::Unknown, _) => D::Hold("unobserved"),
        (_, S::IdentityConflict, _) => D::Hold("identity conflict"),
        (_, S::Idle, Some(_)) if consecutive_refusals >= TASK_CONSECUTIVE_REFUSAL_THRESHOLD => {
            D::Hold("refusals escalated")
        }
        // An unstarted assignment receives one bounded reminder budget and
        // one lead notification. A new assignment/reopen resets both counters.
        (_, S::Idle, Some(task)) if escalation_due(task) => D::EscalateStalled,
        (_, S::Idle, Some(task)) if task.lead_notified_count > 0 => D::Hold("stalled reported"),
        (_, S::Idle, Some(task)) if task.reminder_count >= TASK_STALLED_REMINDER_THRESHOLD => {
            D::Hold("stalled reported")
        }
        (true, S::Idle, _) => D::Hold("mail pending"),
        (false, S::Idle, None) => D::Hold("no open task"),
        (false, S::Idle, Some(task)) if !reminder_due(task, now) => D::Hold("rate limited"),
        (false, S::Idle, Some(_)) => D::Nudge,
    }
}

/// Whether `active_since` shows the member has been continuously active for
/// at least one full reminder interval as of `now`. An unknown activation
/// time (`None`) is never treated as sustained.
fn sustained_active(active_since: Option<IsoTimestamp>, now: IsoTimestamp) -> bool {
    active_since.is_some_and(|since| {
        (now.into_inner() - since.into_inner()).num_milliseconds() >= TASK_REMINDER_INTERVAL_MS
    })
}

/// Whether `task` has reached its next `TASK_STALLED_REMINDER_THRESHOLD`
/// reminder budget without a lead escalation yet. A Started task cannot
/// trigger a stale Assigned-state escalation. This predicate is shared by
/// [`dispose`]'s `EscalateStalled` trigger and by the queue-wake pump's
/// `queue_drain_allowed`, which holds queue draining only in the tick where a
/// stall escalation is about to fire.
pub(crate) fn escalation_due(task: &TaskRow) -> bool {
    task.state == TaskState::Assigned
        && task.reminder_count >= TASK_STALLED_REMINDER_THRESHOLD
        && task.lead_notified_count == 0
}

fn reminder_due(task: &TaskRow, now: IsoTimestamp) -> bool {
    task.last_reminded_at.is_none_or(|last| {
        (now.into_inner() - last.into_inner()).num_milliseconds() >= TASK_REMINDER_INTERVAL_MS
    })
}

#[cfg(test)]
mod tests {
    use super::{EpisodeKind, TaskDisposition, dispose};
    use atm_core::boundary::{TASK_STALLED_REMINDER_THRESHOLD, TaskRow, TaskState};
    use atm_core::protocol::RuntimeMemberState;
    use atm_core::schema::AtmMessageId;
    use atm_core::types::{AgentName, IsoTimestamp, TaskId, TeamName};

    fn now() -> IsoTimestamp {
        "2026-09-11T00:00:00Z".parse().expect("timestamp")
    }

    fn row() -> TaskRow {
        TaskRow {
            team: "team".parse::<TeamName>().expect("team"),
            task_id: "task".parse::<TaskId>().expect("task"),
            assignee: "agent".parse::<AgentName>().expect("assignee"),
            assigner: "assigner".parse::<AgentName>().expect("assigner"),
            state: TaskState::Assigned,
            position: None,
            assignment_message_id: AtmMessageId::new(),
            description: "task".to_owned(),
            assigned_at: now(),
            updated_at: now(),
            last_reminded_at: None,
            reminder_count: 0,
            lead_notified_count: 0,
        }
    }

    #[test]
    fn idle_member_with_open_mail_holds_mail_pending() {
        assert_eq!(
            dispose(
                true,
                RuntimeMemberState::Idle,
                Some(&row()),
                now(),
                None,
                false,
                0
            ),
            TaskDisposition::Hold("mail pending")
        );
    }

    #[test]
    fn blocked_without_task_holds_episode() {
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Blocked,
                None,
                now(),
                None,
                true,
                0
            ),
            TaskDisposition::Hold("no open task")
        );
    }

    #[test]
    fn episode_kind_has_stable_name() {
        assert_eq!(EpisodeKind::Blocked.as_str(), "blocked");
        assert_eq!(EpisodeKind::Offline.as_str(), "offline");
    }

    #[test]
    fn active_member_with_stalled_task_holds() {
        let mut task = row();
        task.reminder_count = 10;
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Active,
                Some(&task),
                now(),
                None,
                false,
                0
            ),
            TaskDisposition::Hold("active")
        );
    }

    #[test]
    fn started_task_holds_idle_roster_but_blocked_episode_escalates() {
        let mut task = row();
        task.state = TaskState::Active;
        task.reminder_count = TASK_STALLED_REMINDER_THRESHOLD;
        task.lead_notified_count = 0;
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Idle,
                Some(&task),
                now(),
                None,
                false,
                0
            ),
            TaskDisposition::Hold("task active")
        );
        assert!(!super::escalation_due(&task));
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Blocked,
                Some(&task),
                now(),
                None,
                true,
                0
            ),
            TaskDisposition::EscalateEpisode(EpisodeKind::Blocked)
        );
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Blocked,
                Some(&task),
                now(),
                None,
                false,
                0
            ),
            TaskDisposition::Hold("episode reported")
        );
    }

    #[test]
    fn active_reset_requires_a_full_reminder_interval() {
        let mut task = row();
        task.reminder_count = 3;
        let thirty_seconds_ago: IsoTimestamp = "2026-09-10T23:59:30Z".parse().expect("timestamp");
        let sixty_seconds_ago: IsoTimestamp = "2026-09-10T23:59:00Z".parse().expect("timestamp");

        // A quick reply between idle turns is not sustained activity.
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Active,
                Some(&task),
                now(),
                Some(thirty_seconds_ago),
                false,
                0
            ),
            TaskDisposition::Hold("active")
        );

        // A full reminder interval of continuous activity resets the budget.
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Active,
                Some(&task),
                now(),
                Some(sixty_seconds_ago),
                false,
                0
            ),
            TaskDisposition::ResetReminders
        );

        // An unknown activation time is never treated as sustained.
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Active,
                Some(&task),
                now(),
                None,
                false,
                0
            ),
            TaskDisposition::Hold("active")
        );
    }

    #[test]
    fn assigned_stall_escalates_once_then_holds_until_a_state_transition() {
        let mut task = row();

        // First budget of reminders exhausted, no escalation sent yet.
        task.reminder_count = 10;
        task.lead_notified_count = 0;
        assert_eq!(
            dispose(
                true,
                RuntimeMemberState::Idle,
                Some(&task),
                now(),
                None,
                false,
                0
            ),
            TaskDisposition::EscalateStalled
        );

        // Once reported, neither a stale counter nor a new idle observation
        // reopens the same assignment's reminder budget.
        task.reminder_count = 15;
        task.lead_notified_count = 1;
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Idle,
                Some(&task),
                now(),
                None,
                false,
                0
            ),
            TaskDisposition::Hold("stalled reported")
        );

        // A second full budget inherited from older daemon versions does not
        // cause another lead notification.
        task.reminder_count = 20;
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Idle,
                Some(&task),
                now(),
                None,
                false,
                0
            ),
            TaskDisposition::Hold("stalled reported")
        );

        // Starting the task overrides even an unreported old threshold.
        task.lead_notified_count = 0;
        task.state = TaskState::Active;
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Idle,
                Some(&task),
                now(),
                None,
                false,
                0
            ),
            TaskDisposition::Hold("task active")
        );

        // The recorded lead notification is a latch even if an activity
        // reset has cleared the reminder budget.
        task.state = TaskState::Assigned;
        task.reminder_count = 0;
        task.lead_notified_count = 1;
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Idle,
                Some(&task),
                now(),
                None,
                false,
                0
            ),
            TaskDisposition::Hold("stalled reported")
        );

        // An assignment below threshold still respects the rate limit.
        task.state = TaskState::Assigned;
        task.reminder_count = 2;
        task.lead_notified_count = 0;
        task.last_reminded_at = Some(now());
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Idle,
                Some(&task),
                now(),
                None,
                false,
                0
            ),
            TaskDisposition::Hold("rate limited")
        );
    }

    #[test]
    fn rate_limit_boundary() {
        let mut task = row();
        task.last_reminded_at = Some("2026-09-10T23:59:00.001Z".parse().expect("timestamp"));
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Idle,
                Some(&task),
                now(),
                None,
                false,
                0
            ),
            TaskDisposition::Hold("rate limited")
        );
        task.last_reminded_at = Some("2026-09-10T23:59:00Z".parse().expect("timestamp"));
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Idle,
                Some(&task),
                now(),
                None,
                false,
                0
            ),
            TaskDisposition::Nudge
        );
        task.last_reminded_at = None;
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Idle,
                Some(&task),
                now(),
                None,
                false,
                0
            ),
            TaskDisposition::Nudge
        );
    }

    #[test]
    fn rate_limit_clock_reversal_is_not_due() {
        let mut task = row();
        task.last_reminded_at = Some("2026-09-11T00:00:05Z".parse().expect("timestamp"));
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Idle,
                Some(&task),
                now(),
                None,
                false,
                0
            ),
            TaskDisposition::Hold("rate limited")
        );
    }

    #[test]
    fn dispose_table_is_exhaustive() {
        let fresh = row();
        let mut rate_limited = row();
        rate_limited.last_reminded_at = Some("2026-09-10T23:59:30Z".parse().expect("timestamp"));
        let mut threshold = row();
        threshold.reminder_count = 10;
        let mut escalated = row();
        escalated.reminder_count = 15;
        escalated.lead_notified_count = 1;
        let mut active = row();
        active.state = TaskState::Active;

        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Active,
                None,
                now(),
                None,
                true,
                0
            ),
            TaskDisposition::Hold("active")
        );
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Blocked,
                Some(&fresh),
                now(),
                None,
                true,
                0
            ),
            TaskDisposition::EscalateEpisode(EpisodeKind::Blocked)
        );
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Blocked,
                None,
                now(),
                None,
                false,
                0
            ),
            TaskDisposition::Hold("no open task")
        );
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Offline,
                Some(&fresh),
                now(),
                None,
                true,
                0
            ),
            TaskDisposition::EscalateEpisode(EpisodeKind::Offline)
        );
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Offline,
                None,
                now(),
                None,
                false,
                0
            ),
            TaskDisposition::Hold("no open task")
        );
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Unknown,
                Some(&fresh),
                now(),
                None,
                true,
                0
            ),
            TaskDisposition::Hold("unobserved")
        );
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::IdentityConflict,
                Some(&fresh),
                now(),
                None,
                true,
                0
            ),
            TaskDisposition::Hold("identity conflict")
        );
        assert_eq!(
            dispose(
                true,
                RuntimeMemberState::Idle,
                Some(&fresh),
                now(),
                None,
                true,
                0
            ),
            TaskDisposition::Hold("mail pending")
        );
        assert_eq!(
            dispose(false, RuntimeMemberState::Idle, None, now(), None, true, 0),
            TaskDisposition::Hold("no open task")
        );
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Idle,
                Some(&fresh),
                now(),
                None,
                true,
                3
            ),
            TaskDisposition::Hold("refusals escalated")
        );
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Idle,
                Some(&escalated),
                now(),
                None,
                true,
                0
            ),
            TaskDisposition::Hold("stalled reported")
        );
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Idle,
                Some(&threshold),
                now(),
                None,
                true,
                0
            ),
            TaskDisposition::EscalateStalled
        );
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Idle,
                Some(&rate_limited),
                now(),
                None,
                true,
                0
            ),
            TaskDisposition::Hold("rate limited")
        );
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Idle,
                Some(&fresh),
                now(),
                None,
                true,
                0
            ),
            TaskDisposition::Nudge
        );
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Idle,
                Some(&active),
                now(),
                None,
                true,
                0
            ),
            TaskDisposition::Hold("task active")
        );
    }
}
