//! Generic, local workflow lifecycle projection over immutable admission facts.
//!
//! This module deliberately knows no ATM workflow vocabulary. Callers choose
//! opaque snapshot values, while the pairing algorithm supplies only stable
//! ordering and one-to-one lifecycle semantics.

use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use atm_storage::{
    AtmError, IsoTimestamp, StoredSearchMatch, TimeRange, WorkflowScopeId, WorkflowScopeKind,
    WorkflowStage, WorkflowState, WorkflowTransition,
};
use serde::{Deserialize, Serialize};

/// A partial exact selector over a durable workflow snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowSelector {
    pub state: Option<WorkflowState>,
    pub stage: Option<WorkflowStage>,
    pub transition: Option<WorkflowTransition>,
}

impl WorkflowSelector {
    /// Rejects selectors that would turn lifecycle analytics into an
    /// unbounded match-all query.
    pub fn validate(&self, label: &str) -> Result<(), AtmError> {
        if self.state.is_none() && self.stage.is_none() && self.transition.is_none() {
            return Err(AtmError::workflow_query_invalid(format!(
                "workflow lifecycle {label} selector must include state, stage, or transition"
            )));
        }
        Ok(())
    }

    fn matches(&self, snapshot: &atm_storage::WorkflowSnapshot) -> bool {
        self.state
            .as_ref()
            .is_none_or(|value| value == &snapshot.state)
            && self
                .stage
                .as_ref()
                .is_none_or(|value| value == &snapshot.stage)
            && self
                .transition
                .as_ref()
                .is_none_or(|value| value == &snapshot.transition)
    }
}

/// Bounded request for deterministic lifecycle pairing within each scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowProjectionRequest {
    pub scope_kind: WorkflowScopeKind,
    pub scope_id: Option<WorkflowScopeId>,
    pub start: WorkflowSelector,
    pub end: WorkflowSelector,
    pub time_range: Option<TimeRange>,
}

impl WorkflowProjectionRequest {
    pub fn validate(&self) -> Result<(), AtmError> {
        self.start.validate("start")?;
        self.end.validate("end")?;
        if let Some(range) = &self.time_range {
            range
                .validate()
                .map_err(|error| AtmError::workflow_query_invalid(error.detail().to_owned()))?;
        }
        Ok(())
    }
}

/// A durable workflow fact with the complete immutable provenance available to
/// local callers. No payload or merged variables appear here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowFact {
    pub key: atm_storage::SearchResultKey,
    pub message_id: Option<String>,
    pub message_at: IsoTimestamp,
    pub snapshot: atm_storage::WorkflowSnapshot,
    pub tag_provenance: atm_storage::MessageTagProvenance,
}

/// One completed or still-open generic lifecycle observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifecycleObservation {
    Completed {
        start: WorkflowFact,
        end: WorkflowFact,
        duration: Duration,
    },
    Incomplete {
        start: WorkflowFact,
    },
}

/// Projects records into deterministic, one-to-one lifecycle observations.
///
/// Records are ordered inside each scope by durable timestamp and immutable
/// mailbox key. An end consumes the earliest preceding unpaired start; a fact
/// that matches both selectors is considered as an end first, so it cannot
/// pair with itself.
pub fn project_lifecycles(
    request: &WorkflowProjectionRequest,
    records: impl IntoIterator<Item = StoredSearchMatch>,
) -> Result<Vec<LifecycleObservation>, AtmError> {
    request.validate()?;
    let mut scopes = BTreeMap::<WorkflowScopeId, Vec<WorkflowFact>>::new();
    for record in records {
        let Some(workflow) = record.workflow else {
            continue;
        };
        let snapshot = workflow.snapshot;
        if snapshot.scope_kind != request.scope_kind
            || request
                .scope_id
                .as_ref()
                .is_some_and(|scope_id| scope_id != &snapshot.scope_id)
            || request.time_range.as_ref().is_some_and(|range| {
                range.since.is_some_and(|since| record.message_at < since)
                    || range.until.is_some_and(|until| record.message_at > until)
            })
        {
            continue;
        }
        scopes
            .entry(snapshot.scope_id.clone())
            .or_default()
            .push(WorkflowFact {
                key: record.key,
                message_id: record.message_id,
                message_at: record.message_at,
                snapshot,
                tag_provenance: workflow.tag_provenance,
            });
    }

    let mut observations = Vec::new();
    for facts in scopes.values_mut() {
        facts.sort_by(|left, right| {
            left.message_at
                .cmp(&right.message_at)
                .then_with(|| left.key.team.cmp(&right.key.team))
                .then_with(|| left.key.agent.cmp(&right.key.agent))
                .then_with(|| left.key.message_key.cmp(&right.key.message_key))
        });
        let mut starts = VecDeque::<WorkflowFact>::new();
        for fact in facts.iter().cloned() {
            if request.end.matches(&fact.snapshot) {
                if let Some(start) = starts.pop_front() {
                    let elapsed = fact.message_at.into_inner() - start.message_at.into_inner();
                    let milliseconds: u64 =
                        elapsed.num_milliseconds().try_into().map_err(|_| {
                            AtmError::workflow_query_invalid(
                                "workflow end precedes its paired start",
                            )
                        })?;
                    let duration = Duration::from_millis(milliseconds);
                    observations.push(LifecycleObservation::Completed {
                        start,
                        end: fact.clone(),
                        duration,
                    });
                }
            }
            if request.start.matches(&fact.snapshot) {
                starts.push_back(fact);
            }
        }
        observations.extend(
            starts
                .into_iter()
                .map(|start| LifecycleObservation::Incomplete { start }),
        );
    }
    Ok(observations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use atm_storage::{
        AgentName, MessageKey, MessageTagProvenance, SearchResultKey, StoredSearchAddress,
        StoredWorkflowMetadata, TeamName, WorkflowSnapshot,
    };

    fn record(key: &str, time: &str, state: &str, scope: &str) -> StoredSearchMatch {
        let team: TeamName = "workflow-test".parse().expect("team");
        let agent: AgentName = "agent".parse().expect("agent");
        StoredSearchMatch {
            key: SearchResultKey {
                team: team.clone(),
                agent: agent.clone(),
                message_key: key.parse::<MessageKey>().expect("key"),
            },
            message_id: Some(key.to_owned()),
            message_at: time.parse().expect("time"),
            from: StoredSearchAddress {
                agent: agent.clone(),
                team: team.clone(),
                chat_id: None,
            },
            to: StoredSearchAddress {
                agent,
                team,
                chat_id: None,
            },
            template_sha: None,
            template_type: None,
            category: None,
            match_fields: Vec::new(),
            snippet: None,
            workflow: Some(StoredWorkflowMetadata {
                snapshot: WorkflowSnapshot {
                    scope_kind: WorkflowScopeKind::new("release").expect("kind"),
                    scope_id: WorkflowScopeId::new(scope).expect("scope"),
                    state: WorkflowState::new(state).expect("state"),
                    stage: WorkflowStage::new("delivery").expect("stage"),
                    transition: WorkflowTransition::new("event").expect("transition"),
                    iteration: None,
                },
                tag_provenance: MessageTagProvenance::default(),
            }),
        }
    }
    fn request() -> WorkflowProjectionRequest {
        WorkflowProjectionRequest {
            scope_kind: WorkflowScopeKind::new("release").expect("kind"),
            scope_id: None,
            start: WorkflowSelector {
                state: Some(WorkflowState::new("opened").expect("state")),
                ..Default::default()
            },
            end: WorkflowSelector {
                state: Some(WorkflowState::new("closed").expect("state")),
                ..Default::default()
            },
            time_range: None,
        }
    }
    #[test]
    fn pairs_earliest_start_once_and_retains_incomplete_facts() {
        let observations = project_lifecycles(
            &request(),
            [
                record("1", "2026-08-01T00:00:00Z", "opened", "one"),
                record("2", "2026-08-01T00:00:00Z", "opened", "one"),
                record("3", "2026-08-01T00:01:00Z", "closed", "one"),
                record("4", "2026-08-01T00:02:00Z", "closed", "one"),
                record("5", "2026-08-01T00:03:00Z", "opened", "two"),
            ],
        )
        .expect("projection");
        assert!(
            matches!(&observations[0], LifecycleObservation::Completed { start, end, .. } if start.message_id.as_deref() == Some("1") && end.message_id.as_deref() == Some("3"))
        );
        assert!(
            matches!(&observations[1], LifecycleObservation::Completed { start, end, .. } if start.message_id.as_deref() == Some("2") && end.message_id.as_deref() == Some("4"))
        );
        assert!(
            matches!(&observations[2], LifecycleObservation::Incomplete { start } if start.message_id.as_deref() == Some("5"))
        );
    }
    #[test]
    fn rejects_empty_selector_and_impossible_time_range() {
        let mut invalid = request();
        invalid.start = WorkflowSelector::default();
        assert_eq!(
            project_lifecycles(&invalid, [])
                .expect_err("invalid")
                .code(),
            atm_storage::AtmErrorCode::WorkflowQueryInvalid
        );
        let mut impossible = request();
        impossible.time_range = Some(TimeRange {
            since: Some("2026-08-02T00:00:00Z".parse().expect("time")),
            until: Some("2026-08-01T00:00:00Z".parse().expect("time")),
        });
        assert_eq!(
            project_lifecycles(&impossible, [])
                .expect_err("invalid")
                .code(),
            atm_storage::AtmErrorCode::WorkflowQueryInvalid
        );
    }
}
