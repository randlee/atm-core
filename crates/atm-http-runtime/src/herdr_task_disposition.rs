//! Pure task-nudge policy for one accepted roster observation.

use atm_core::boundary::{
    TASK_CONSECUTIVE_REFUSAL_THRESHOLD, TASK_REMINDER_INTERVAL_MS, TASK_STALLED_REMINDER_THRESHOLD,
    TaskRow,
};
use atm_core::protocol::RuntimeMemberState;
use atm_core::types::IsoTimestamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskDisposition {
    Nudge,
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
}

pub(crate) fn dispose(
    mail_pending: bool,
    state: RuntimeMemberState,
    head: Option<&TaskRow>,
    now: IsoTimestamp,
    new_episode: bool,
    consecutive_refusals: u32,
) -> TaskDisposition {
    use RuntimeMemberState as S;
    use TaskDisposition as D;

    match (mail_pending, state, head) {
        (_, S::Active, _) => D::Hold("active"),
        (_, S::Blocked, _) => {
            if new_episode {
                D::EscalateEpisode(EpisodeKind::Blocked)
            } else {
                D::Hold("episode reported")
            }
        }
        (_, S::Offline, _) => {
            if new_episode {
                D::EscalateEpisode(EpisodeKind::Offline)
            } else {
                D::Hold("episode reported")
            }
        }
        (_, S::Unknown, _) => D::Hold("unobserved"),
        (_, S::IdentityConflict, _) => D::Hold("identity conflict"),
        (true, S::Idle, _) => D::Hold("mail pending"),
        (false, S::Idle, None) => D::Hold("no open task"),
        (false, S::Idle, Some(_)) if consecutive_refusals >= TASK_CONSECUTIVE_REFUSAL_THRESHOLD => {
            D::Hold("refusals escalated")
        }
        (false, S::Idle, Some(task)) if task.lead_notified_count > 0 => D::Hold("stalled"),
        (false, S::Idle, Some(task)) if task.reminder_count >= TASK_STALLED_REMINDER_THRESHOLD => {
            D::EscalateStalled
        }
        (false, S::Idle, Some(task)) if !reminder_due(task, now) => D::Hold("rate limited"),
        (false, S::Idle, Some(_)) => D::Nudge,
    }
}

fn reminder_due(task: &TaskRow, now: IsoTimestamp) -> bool {
    task.last_reminded_at.is_none_or(|last| {
        (now.into_inner() - last.into_inner()).num_milliseconds() >= TASK_REMINDER_INTERVAL_MS
    })
}

#[cfg(test)]
mod tests {
    use super::{EpisodeKind, TaskDisposition, dispose};
    use atm_core::boundary::{TaskRow, TaskState};
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
                false,
                0
            ),
            TaskDisposition::Hold("mail pending")
        );
    }

    #[test]
    fn blocked_without_task_still_escalates_once() {
        assert_eq!(
            dispose(false, RuntimeMemberState::Blocked, None, now(), true, 0),
            TaskDisposition::EscalateEpisode(EpisodeKind::Blocked)
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
                false,
                0
            ),
            TaskDisposition::Hold("active")
        );
    }

    #[test]
    fn threshold_is_terminal() {
        let mut task = row();
        task.reminder_count = 10;
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Idle,
                Some(&task),
                now(),
                false,
                0
            ),
            TaskDisposition::EscalateStalled
        );
        task.lead_notified_count = 1;
        task.reminder_count = 25;
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Idle,
                Some(&task),
                now(),
                false,
                0
            ),
            TaskDisposition::Hold("stalled")
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
        escalated.reminder_count = 25;
        escalated.lead_notified_count = 1;
        let mut active = row();
        active.state = TaskState::Active;

        assert_eq!(
            dispose(false, RuntimeMemberState::Active, None, now(), true, 0),
            TaskDisposition::Hold("active")
        );
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Blocked,
                Some(&fresh),
                now(),
                true,
                0
            ),
            TaskDisposition::EscalateEpisode(EpisodeKind::Blocked)
        );
        assert_eq!(
            dispose(false, RuntimeMemberState::Blocked, None, now(), false, 0),
            TaskDisposition::Hold("episode reported")
        );
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Offline,
                Some(&fresh),
                now(),
                true,
                0
            ),
            TaskDisposition::EscalateEpisode(EpisodeKind::Offline)
        );
        assert_eq!(
            dispose(false, RuntimeMemberState::Offline, None, now(), false, 0),
            TaskDisposition::Hold("episode reported")
        );
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Unknown,
                Some(&fresh),
                now(),
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
                true,
                0
            ),
            TaskDisposition::Hold("identity conflict")
        );
        assert_eq!(
            dispose(true, RuntimeMemberState::Idle, Some(&fresh), now(), true, 0),
            TaskDisposition::Hold("mail pending")
        );
        assert_eq!(
            dispose(false, RuntimeMemberState::Idle, None, now(), true, 0),
            TaskDisposition::Hold("no open task")
        );
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Idle,
                Some(&fresh),
                now(),
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
                true,
                0
            ),
            TaskDisposition::Hold("stalled")
        );
        assert_eq!(
            dispose(
                false,
                RuntimeMemberState::Idle,
                Some(&threshold),
                now(),
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
                true,
                0
            ),
            TaskDisposition::Nudge
        );
    }
}
