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
        end: Box<WorkflowFact>,
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
            if request.end.matches(&fact.snapshot)
                && let Some(start) = starts.pop_front()
            {
                let elapsed = fact.message_at.into_inner() - start.message_at.into_inner();
                let milliseconds: u64 = elapsed.num_milliseconds().try_into().map_err(|_| {
                    AtmError::workflow_query_invalid("workflow end precedes its paired start")
                })?;
                let duration = Duration::from_millis(milliseconds);
                observations.push(LifecycleObservation::Completed {
                    start,
                    end: Box::new(fact.clone()),
                    duration,
                });
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
        StoredWorkflowMetadata, TeamName, WorkflowIteration, WorkflowSnapshot,
    };
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct EvidenceCorpus {
        expected_aggregate_counts:
            std::collections::BTreeMap<String, std::collections::BTreeMap<String, u64>>,
        families: Vec<EvidenceFamily>,
    }

    #[derive(Debug, Deserialize)]
    struct EvidenceFamily {
        name: String,
        template_tags: Vec<String>,
        instance_tags: Vec<String>,
        template_type: String,
        content_format: String,
        scope_kind: String,
        scope_id: String,
        events: Vec<EvidenceEvent>,
        expected: EvidenceExpected,
    }

    #[derive(Debug, Deserialize)]
    struct EvidenceEvent {
        id: String,
        at: String,
        state: String,
        stage: String,
        transition: String,
        iteration: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct EvidenceExpected {
        completed_durations_millis: Vec<u64>,
        incomplete_ids: Vec<String>,
        iteration_counts: std::collections::BTreeMap<String, u64>,
        applied_template_tags: Vec<String>,
        effective_tags: Vec<String>,
    }

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

    #[test]
    fn retained_unrelated_vocabulary_fixtures_prove_generic_lifecycle_facts() {
        let corpus: EvidenceCorpus = serde_json::from_str(include_str!(
            "../../../docs/plans/phase-an/fixtures/workflow-metadata-evidence.json"
        ))
        .expect("retained AN.12 fixture corpus");
        assert_eq!(corpus.families.len(), 2, "two unrelated vocabularies");
        let mut aggregate_counts =
            std::collections::BTreeMap::<String, std::collections::BTreeMap<String, u64>>::new();

        for family in corpus.families {
            let team: TeamName = "evidence-team".parse().expect("team");
            let agent: AgentName = "evidence-agent".parse().expect("agent");
            let scope_kind = WorkflowScopeKind::new(family.scope_kind.clone()).expect("scope kind");
            let scope_id = WorkflowScopeId::new(family.scope_id.clone()).expect("scope id");
            let mut template_tags = family
                .template_tags
                .iter()
                .map(|tag| atm_storage::TemplateTag::new(tag.clone()).expect("template tag"))
                .collect::<Vec<_>>();
            template_tags.sort();
            let instance_tags = family
                .instance_tags
                .iter()
                .map(|tag| atm_storage::InstanceTag::new(tag.clone()).expect("instance tag"))
                .collect::<Vec<_>>();
            let records = family
                .events
                .iter()
                .map(|event| StoredSearchMatch {
                    key: SearchResultKey {
                        team: team.clone(),
                        agent: agent.clone(),
                        message_key: event.id.parse::<MessageKey>().expect("message key"),
                    },
                    message_id: Some(event.id.clone()),
                    message_at: event.at.parse().expect("timestamp"),
                    from: StoredSearchAddress {
                        team: team.clone(),
                        agent: agent.clone(),
                        chat_id: None,
                    },
                    to: StoredSearchAddress {
                        team: team.clone(),
                        agent: agent.clone(),
                        chat_id: None,
                    },
                    template_sha: None,
                    template_type: None,
                    category: None,
                    match_fields: Vec::new(),
                    snippet: None,
                    workflow: Some(StoredWorkflowMetadata {
                        snapshot: WorkflowSnapshot {
                            scope_kind: scope_kind.clone(),
                            scope_id: scope_id.clone(),
                            state: WorkflowState::new(event.state.clone()).expect("state"),
                            stage: WorkflowStage::new(event.stage.clone()).expect("stage"),
                            transition: WorkflowTransition::new(event.transition.clone())
                                .expect("transition"),
                            iteration: event.iteration.as_ref().map(|value| {
                                WorkflowIteration::new(value.clone()).expect("iteration")
                            }),
                        },
                        tag_provenance: MessageTagProvenance {
                            instance_tags: instance_tags.clone(),
                            applied_template_tags: template_tags.clone(),
                            derived_tags: vec![
                                atm_storage::DerivedTag::new(format!(
                                    "template-type:{}",
                                    family.template_type
                                ))
                                .expect("derived tag"),
                                atm_storage::DerivedTag::new(format!(
                                    "content-format:{}",
                                    family.content_format
                                ))
                                .expect("derived tag"),
                                atm_storage::DerivedTag::new(format!(
                                    "workflow-scope-kind:{}",
                                    family.scope_kind
                                ))
                                .expect("derived tag"),
                                atm_storage::DerivedTag::new(format!(
                                    "workflow-state:{}",
                                    event.state
                                ))
                                .expect("derived tag"),
                                atm_storage::DerivedTag::new(format!(
                                    "workflow-stage:{}",
                                    event.stage
                                ))
                                .expect("derived tag"),
                                atm_storage::DerivedTag::new(format!(
                                    "workflow-transition:{}",
                                    event.transition
                                ))
                                .expect("derived tag"),
                            ],
                            effective_tags: atm_storage::DecomposedMessageAdmission::expected_tag_provenance_for(
                                &instance_tags,
                                &template_tags,
                                Some(&family.template_type),
                                Some(&family.content_format),
                                &WorkflowSnapshot {
                                    scope_kind: scope_kind.clone(),
                                    scope_id: scope_id.clone(),
                                    state: WorkflowState::new(event.state.clone()).expect("state"),
                                    stage: WorkflowStage::new(event.stage.clone()).expect("stage"),
                                    transition: WorkflowTransition::new(event.transition.clone()).expect("transition"),
                                    iteration: event.iteration.as_ref().map(|value| WorkflowIteration::new(value.clone()).expect("iteration")),
                                },
                            )
                            .expect("canonical provenance")
                            .effective_tags,
                            ..MessageTagProvenance::default()
                        },
                    }),
                })
                .collect::<Vec<_>>();
            for record in &records {
                let snapshot = &record.workflow.as_ref().expect("workflow").snapshot;
                for (dimension, value) in [
                    ("scope_kind", snapshot.scope_kind.as_str()),
                    ("state", snapshot.state.as_str()),
                    ("stage", snapshot.stage.as_str()),
                    ("transition", snapshot.transition.as_str()),
                ] {
                    *aggregate_counts
                        .entry(dimension.to_owned())
                        .or_default()
                        .entry(value.to_owned())
                        .or_insert(0) += 1;
                }
            }
            let start = records
                .first()
                .expect("fixture start")
                .workflow
                .as_ref()
                .expect("workflow")
                .snapshot
                .clone();
            let end = records
                .iter()
                .find(|record| {
                    record
                        .workflow
                        .as_ref()
                        .expect("workflow")
                        .snapshot
                        .state
                        .as_str()
                        != start.state.as_str()
                })
                .expect("fixture end")
                .workflow
                .as_ref()
                .expect("workflow")
                .snapshot
                .clone();
            let observations = project_lifecycles(
                &WorkflowProjectionRequest {
                    scope_kind,
                    scope_id: Some(scope_id),
                    start: WorkflowSelector {
                        state: Some(start.state),
                        ..Default::default()
                    },
                    end: WorkflowSelector {
                        state: Some(end.state),
                        ..Default::default()
                    },
                    time_range: None,
                },
                records.clone(),
            )
            .expect("generic lifecycle projection");
            let completed = observations
                .iter()
                .filter_map(|observation| match observation {
                    LifecycleObservation::Completed { duration, .. } => {
                        Some(duration.as_millis() as u64)
                    }
                    LifecycleObservation::Incomplete { .. } => None,
                })
                .collect::<Vec<_>>();
            let incomplete = observations
                .iter()
                .filter_map(|observation| match observation {
                    LifecycleObservation::Completed { .. } => None,
                    LifecycleObservation::Incomplete { start } => start.message_id.clone(),
                })
                .collect::<Vec<_>>();
            assert_eq!(
                completed, family.expected.completed_durations_millis,
                "{} durations",
                family.name
            );
            assert_eq!(
                incomplete, family.expected.incomplete_ids,
                "{} incomplete",
                family.name
            );
            let iterations = records
                .iter()
                .filter_map(|record| {
                    record
                        .workflow
                        .as_ref()
                        .expect("workflow")
                        .snapshot
                        .iteration
                        .as_ref()
                        .map(|iteration| iteration.as_str().to_owned())
                })
                .fold(
                    std::collections::BTreeMap::new(),
                    |mut counts, iteration| {
                        *counts.entry(iteration).or_insert(0) += 1;
                        counts
                    },
                );
            assert_eq!(
                iterations, family.expected.iteration_counts,
                "{} iteration counts",
                family.name
            );
            assert_eq!(
                records[0]
                    .workflow
                    .as_ref()
                    .expect("workflow")
                    .tag_provenance
                    .applied_template_tags
                    .iter()
                    .map(|tag| tag.as_str())
                    .collect::<Vec<_>>(),
                family
                    .expected
                    .applied_template_tags
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                "{} applied template tags",
                family.name
            );
            assert_eq!(
                records[0]
                    .workflow
                    .as_ref()
                    .expect("workflow")
                    .tag_provenance
                    .effective_tags
                    .iter()
                    .map(|tag| tag.as_str())
                    .collect::<Vec<_>>(),
                family
                    .expected
                    .effective_tags
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                "{} effective tags",
                family.name
            );
        }
        assert_eq!(
            aggregate_counts, corpus.expected_aggregate_counts,
            "only the four bounded aggregate dimensions are hand-computed"
        );
    }
}
