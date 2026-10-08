//! Bounded projection of durable ATM facts onto native OpenTelemetry signals.
//!
//! Events never wait for missing sequences. A close observed before its start
//! emits a partial span with only the known timestamp; later facts cannot
//! rewrite already-exported history. Sequence fences keep late old closes from
//! ending a newer assignment. Dedup is best effort after its 8192-entry window
//! or assignment-state eviction; no replay/backfill or outage storage exists.

use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use atm_core::{
    TaskTelemetryError, TaskTelemetryKind as Kind, TaskTelemetryRecord, TaskTelemetrySink,
    WorkflowTelemetryError, WorkflowTelemetryRecord, WorkflowTelemetrySink,
};
use opentelemetry::metrics::{Counter, Histogram, Meter};
use opentelemetry::trace::{Event, Span, SpanId, TraceId, Tracer};
use opentelemetry::{Context, KeyValue};
use opentelemetry_sdk::trace::{IdGenerator, RandomIdGenerator, SdkTracer};
use sha2::{Digest, Sha256};

const ACTIVE_LIMIT: usize = 4096;
const EVENT_LIMIT: usize = 64;
const BYTE_LIMIT: usize = 64 * 1024;
const RECORD_LIMIT: usize = 16 * 1024;
const DEDUP_LIMIT: usize = 8192;
const HISTOGRAM_BUCKETS: &[f64] = &[
    1., 10., 100., 1_000., 10_000., 60_000., 300_000., 3_600_000.,
];

/// Native SDK identity policy for durable ATM projections. Context values are
/// native OTel ids and are scoped synchronously around span construction.
/// SDK 0.33's SpanBuilder has no id override: build_with_context synchronously
/// calls both generator methods for a root span. The attach guard below restores
/// ambient context before returning (and is never held across an await).
#[derive(Debug)]
pub(crate) struct DurableIds;

impl IdGenerator for DurableIds {
    fn new_trace_id(&self) -> TraceId {
        Context::map_current(|context| context.get::<TraceId>().copied())
            .unwrap_or_else(|| RandomIdGenerator::default().new_trace_id())
    }
    fn new_span_id(&self) -> SpanId {
        Context::map_current(|context| context.get::<SpanId>().copied())
            .unwrap_or_else(|| RandomIdGenerator::default().new_span_id())
    }
}

struct Assignment {
    generation: Option<u64>,
    assigned: Option<SystemTime>,
    started: Option<(Option<u64>, SystemTime)>,
    first: SystemTime,
    last: SystemTime,
    highest_seq: Option<u64>,
    closed_through: Option<u64>,
    closed: bool,
    events: Vec<(Option<u64>, Event)>,
    bytes: usize,
    attributes: Vec<KeyValue>,
    touched: u64,
}

impl Assignment {
    fn new(at: SystemTime, attributes: Vec<KeyValue>, touched: u64) -> Self {
        Self {
            generation: None,
            assigned: None,
            started: None,
            first: at,
            last: at,
            highest_seq: None,
            closed_through: None,
            closed: false,
            events: Vec::new(),
            bytes: 0,
            attributes,
            touched,
        }
    }
    fn split_future(&mut self, next: &mut Self, generation: Option<u64>, at: SystemTime) {
        // The new assignment row itself can arrive after its start.
        // Move only sequence-proven future events into that generation.
        let (future, previous) = std::mem::take(&mut self.events)
            .into_iter()
            .partition(|(seq, _)| seq.is_some() && *seq >= generation);
        next.events = future;
        self.events = previous;
        if self
            .started
            .is_some_and(|(seq, _)| seq.is_some() && seq >= generation)
        {
            next.started = self.started.take();
        }
        next.bytes = self.bytes; // conservative bound after splitting
        next.last = next
            .events
            .iter()
            .map(|(_, event)| event.timestamp)
            .max()
            .unwrap_or(at)
            .max(at);
        next.highest_seq = next.events.iter().filter_map(|(seq, _)| *seq).max();
        self.last = self
            .events
            .iter()
            .map(|(_, event)| event.timestamp)
            .max()
            .unwrap_or(self.first);
    }
}

#[derive(Default)]
struct State {
    assignments: HashMap<[u8; 32], Assignment>,
    seen: HashSet<[u8; 32]>,
    order: VecDeque<[u8; 32]>,
    clock: u64,
}

impl State {
    fn duplicate(&mut self, id: [u8; 32]) -> bool {
        if !self.seen.insert(id) {
            return true;
        }
        self.order.push_back(id);
        if self.order.len() > DEDUP_LIMIT
            && let Some(old) = self.order.pop_front()
        {
            self.seen.remove(&old);
        }
        false
    }
}

/// Existing ATM sink contracts implemented with native SDK instrumentation.
pub(crate) struct TaskExporter {
    tracer: SdkTracer,
    events: Counter<u64>,
    time_to_start: Histogram<f64>,
    time_to_close: Histogram<f64>,
    // MUTEX: task and workflow workers share bounded projection state. Keep
    // only projection and nonblocking SDK recording/admission here; exporter
    // network I/O and SDK lifecycle/shutdown work must remain outside.
    state: Mutex<State>,
}

impl TaskExporter {
    pub(crate) fn new(tracer: SdkTracer, meter: Meter) -> Self {
        Self {
            tracer,
            events: meter.u64_counter("atm.task.events").build(),
            time_to_start: meter
                .f64_histogram("atm.task.time_to_start_ms")
                .with_unit("ms")
                .with_boundaries(HISTOGRAM_BUCKETS.to_vec())
                .build(),
            time_to_close: meter
                .f64_histogram("atm.task.time_to_close_ms")
                .with_unit("ms")
                .with_boundaries(HISTOGRAM_BUCKETS.to_vec())
                .build(),
            state: Mutex::new(State::default()),
        }
    }

    fn task(&self, record: TaskTelemetryRecord) -> Result<(), TaskTelemetryError> {
        let encoded = serde_json::to_vec(&record).map_err(|_| TaskTelemetryError::Rejected)?;
        if encoded.len() > RECORD_LIMIT {
            return Err(TaskTelemetryError::Rejected);
        }
        let key = identity(&[
            b"task",
            record.team.as_str().as_bytes(),
            record.task_id.as_str().as_bytes(),
        ]);
        let id = if let Some(seq) = record.seq {
            identity(&[b"event", &key, &seq.to_be_bytes()])
        } else {
            identity(&[b"handoff", &key, &encoded])
        };
        let at: SystemTime = record.at.into_inner().into();
        let attributes = task_attributes(&record);
        let mut state = self
            .state
            .lock()
            .map_err(|_| TaskTelemetryError::Unavailable)?;
        if state.duplicate(id) {
            return Ok(());
        }
        // Only a finite kind enum is a metric dimension (15 series maximum).
        let labels = [KeyValue::new("kind", record.kind.as_str())];
        self.events.add(1, &labels);
        let trace_id = trace_id_from_digest(key);
        let span_id = span_id_from_digest(id);
        self.span(
            "atm.task.event",
            trace_id,
            span_id,
            at,
            at,
            attributes.clone(),
            Vec::new(),
        );
        state.clock = state.clock.wrapping_add(1);
        if !state.assignments.contains_key(&key)
            && state.assignments.len() >= ACTIVE_LIMIT
            && let Some(old) = state
                .assignments
                .iter()
                .min_by_key(|(_, assignment)| assignment.touched)
                .map(|(key, _)| *key)
            && let Some(assignment) = state.assignments.remove(&old)
        {
            self.finish_assignment(old, assignment, true);
        }
        self.project_task(&mut state, key, &record, attributes, encoded.len());
        Ok(())
    }

    fn project_task(
        &self,
        state: &mut State,
        key: [u8; 32],
        record: &TaskTelemetryRecord,
        attributes: Vec<KeyValue>,
        bytes: usize,
    ) {
        let at: SystemTime = record.at.into_inner().into();
        let touched = state.clock;
        let begins = matches!(
            record.kind,
            Kind::Assigned | Kind::Reassigned | Kind::Reopened
        );
        let replacement = state.assignments.get(&key).is_some_and(|assignment| {
            begins
                && record.seq.is_some_and(|seq| {
                    assignment
                        .generation
                        .is_some_and(|generation| seq > generation)
                        && assignment.closed_through.is_none_or(|closed| seq > closed)
                })
        });
        let mut next = Assignment::new(at, attributes.clone(), touched);
        if replacement && let Some(mut assignment) = state.assignments.remove(&key) {
            assignment.split_future(&mut next, record.seq, at);
            self.finish_assignment(key, assignment, true);
        }
        let assignment = state.assignments.entry(key).or_insert(next);
        self.record_assignment(key, assignment, record, attributes, bytes, touched);
    }

    fn record_assignment(
        &self,
        key: [u8; 32],
        assignment: &mut Assignment,
        record: &TaskTelemetryRecord,
        attributes: Vec<KeyValue>,
        bytes: usize,
        touched: u64,
    ) {
        let at: SystemTime = record.at.into_inner().into();
        let begins = matches!(
            record.kind,
            Kind::Assigned | Kind::Reassigned | Kind::Reopened
        );
        assignment.touched = touched;
        // Neither old assignments nor old terminal rows may mutate the current
        // generation. The event itself is still observable above.
        if record.seq.is_some_and(|seq| {
            assignment
                .generation
                .is_some_and(|generation| seq < generation)
                || assignment
                    .closed_through
                    .is_some_and(|closed| seq <= closed)
        }) {
            return;
        }
        if assignment.closed {
            if !begins || at <= assignment.last {
                return;
            }
            *assignment = Assignment::new(at, attributes.clone(), touched);
        }
        if begins {
            assignment.generation = record.seq;
            assignment.assigned = Some(at);
            assignment.attributes = attributes.clone();
        }
        if record.kind == Kind::Started {
            assignment.started = Some((record.seq, at));
        }
        assignment.first = assignment.first.min(at);
        assignment.last = assignment.last.max(at);
        assignment.highest_seq = assignment.highest_seq.max(record.seq);
        if assignment.events.len() < EVENT_LIMIT && assignment.bytes + bytes <= BYTE_LIMIT {
            assignment.bytes += bytes;
            assignment.events.push((
                record.seq,
                Event::new(record.kind.as_str(), at, attributes, 0),
            ));
        }
        if ends_assignment(record.kind) {
            self.close_assignment(key, assignment, record);
        }
    }

    fn close_assignment(
        &self,
        key: [u8; 32],
        assignment: &mut Assignment,
        record: &TaskTelemetryRecord,
    ) {
        let at: SystemTime = record.at.into_inner().into();
        // A delayed close before an already observed later row cannot end
        // that row's assignment, even if its assignment row was lost.
        if record.seq < assignment.highest_seq {
            return;
        }
        if let Some(assigned) = assignment.assigned {
            // Defer duration metrics until closure so a late assignment
            // row can associate an already-observed start correctly.
            if let Some((_, started)) = assignment.started
                && let Ok(duration) = started.duration_since(assigned)
            {
                self.time_to_start
                    .record(duration.as_secs_f64() * 1000., &[]);
            }
            if let Ok(duration) = at.duration_since(assigned) {
                self.time_to_close
                    .record(duration.as_secs_f64() * 1000., &[]);
            }
        }
        let partial = assignment.assigned.is_none();
        let span_digest = assignment_identity(key, assignment);
        let trace_id = trace_id_from_digest(key);
        let span_id = span_id_from_digest(span_digest);
        let mut attributes = assignment.attributes.clone();
        attributes.push(KeyValue::new("atm.partial", partial));
        self.span(
            "atm.task",
            trace_id,
            span_id,
            assignment.assigned.unwrap_or(assignment.first),
            at,
            attributes,
            std::mem::take(&mut assignment.events)
                .into_iter()
                .map(|(_, event)| event)
                .collect(),
        );
        assignment.closed_through = record.seq.or(assignment.highest_seq);
        assignment.closed = true;
        assignment.bytes = 0;
    }

    fn finish_assignment(&self, key: [u8; 32], assignment: Assignment, partial: bool) {
        if assignment.events.is_empty() {
            return;
        }
        let span_digest = assignment_identity(key, &assignment);
        let trace_id = trace_id_from_digest(key);
        let span_id = span_id_from_digest(span_digest);
        let mut attributes = assignment.attributes;
        attributes.push(KeyValue::new("atm.partial", partial));
        self.span(
            "atm.task",
            trace_id,
            span_id,
            assignment.assigned.unwrap_or(assignment.first),
            assignment.last,
            attributes,
            assignment
                .events
                .into_iter()
                .map(|(_, event)| event)
                .collect(),
        );
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "native span fields are projected once without introducing a parallel signal model"
    )]
    fn span(
        &self,
        name: &'static str,
        trace_id: TraceId,
        span_id: SpanId,
        start: SystemTime,
        end: SystemTime,
        attributes: Vec<KeyValue>,
        mut events: Vec<Event>,
    ) {
        events.sort_by_key(|event| event.timestamp);
        let builder = self
            .tracer
            .span_builder(name)
            .with_start_time(start)
            .with_attributes(attributes)
            .with_events(events);
        let context = Context::new().with_value(trace_id).with_value(span_id);
        let mut span = {
            let _guard = context.clone().attach();
            self.tracer.build_with_context(builder, &context)
        };
        span.end_with_timestamp(end.max(start));
    }

    fn workflow(&self, record: WorkflowTelemetryRecord) -> Result<(), TaskTelemetryError> {
        let encoded = serde_json::to_vec(&record).map_err(|_| TaskTelemetryError::Rejected)?;
        if encoded.len() > RECORD_LIMIT {
            return Err(TaskTelemetryError::Rejected);
        }
        let key = identity(&[
            b"workflow",
            record.scope_kind.as_str().as_bytes(),
            record.scope_id.as_str().as_bytes(),
        ]);
        let id = identity(&[b"workflow-span", &encoded]);
        let mut state = self
            .state
            .lock()
            .map_err(|_| TaskTelemetryError::Unavailable)?;
        if state.duplicate(id) {
            return Ok(());
        }
        drop(state);
        let start: SystemTime = record.start_timestamp.into_inner().into();
        let end = record
            .end_timestamp
            .map(|at| at.into_inner().into())
            .unwrap_or(start);
        let trace_id = trace_id_from_digest(key);
        let span_id = span_id_from_digest(id);
        self.span(
            "atm.workflow",
            trace_id,
            span_id,
            start,
            end,
            vec![
                KeyValue::new("atm.workflow.scope_kind", record.scope_kind.to_string()),
                KeyValue::new("atm.workflow.scope_id", record.scope_id.as_str().to_owned()),
                KeyValue::new("atm.workflow.state", record.state.to_string()),
                KeyValue::new("atm.workflow.stage", record.stage.to_string()),
                KeyValue::new("atm.workflow.transition", record.transition.to_string()),
                KeyValue::new(
                    "atm.workflow.start_message_id",
                    record.start_message_id.to_string(),
                ),
                KeyValue::new(
                    "atm.workflow.end_message_id",
                    record
                        .end_message_id
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_default(),
                ),
                KeyValue::new(
                    "atm.workflow.iteration",
                    record
                        .iteration
                        .map(|iteration| {
                            serde_json::to_string(&iteration).expect("typed iteration")
                        })
                        .unwrap_or_default(),
                ),
                KeyValue::new(
                    "atm.workflow.observation",
                    match record.observation {
                        atm_core::WorkflowTelemetryObservation::Completed => "completed",
                        atm_core::WorkflowTelemetryObservation::Incomplete => "incomplete",
                    },
                ),
                KeyValue::new("atm.partial", record.end_timestamp.is_none()),
            ],
            Vec::new(),
        );
        Ok(())
    }
}

impl Drop for TaskExporter {
    fn drop(&mut self) {
        let assignments = self
            .state
            .lock()
            .map(|mut state| std::mem::take(&mut state.assignments))
            .unwrap_or_default();
        for (key, assignment) in assignments {
            self.finish_assignment(key, assignment, true);
        }
    }
}

impl atm_core::boundary::sealed::Sealed for TaskExporter {}
impl TaskTelemetrySink for TaskExporter {
    fn emit(
        &self,
        record: TaskTelemetryRecord,
    ) -> Pin<Box<dyn Future<Output = Result<(), TaskTelemetryError>> + Send + '_>> {
        Box::pin(async move { self.task(record) })
    }
}
impl WorkflowTelemetrySink for TaskExporter {
    fn emit(
        &self,
        record: WorkflowTelemetryRecord,
    ) -> Pin<Box<dyn Future<Output = Result<(), WorkflowTelemetryError>> + Send + '_>> {
        Box::pin(async move {
            self.workflow(record).map_err(|error| match error {
                TaskTelemetryError::Unavailable => WorkflowTelemetryError::Unavailable,
                TaskTelemetryError::Rejected => WorkflowTelemetryError::Rejected,
                TaskTelemetryError::TimedOut => WorkflowTelemetryError::TimedOut,
            })
        })
    }
}

fn identity(parts: &[&[u8]]) -> [u8; 32] {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    hash.finalize().into()
}

fn trace_id_from_digest(digest: [u8; 32]) -> TraceId {
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    TraceId::from_bytes(bytes)
}

fn span_id_from_digest(digest: [u8; 32]) -> SpanId {
    let mut bytes = [0; 8];
    bytes.copy_from_slice(&digest[..8]);
    SpanId::from_bytes(bytes)
}

fn assignment_identity(key: [u8; 32], assignment: &Assignment) -> [u8; 32] {
    let nanos = assignment
        .assigned
        .unwrap_or(assignment.first)
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_nanos();
    identity(&[
        b"assignment",
        &key,
        &assignment.generation.unwrap_or(0).to_be_bytes(),
        &nanos.to_be_bytes(),
    ])
}

fn task_attributes(record: &TaskTelemetryRecord) -> Vec<KeyValue> {
    let mut attributes = vec![
        KeyValue::new("atm.task.kind", record.kind.as_str()),
        KeyValue::new("atm.team", record.team.to_string()),
        KeyValue::new("atm.task.id", record.task_id.to_string()),
        KeyValue::new("atm.task.assignee", record.assignee.to_string()),
    ];
    // Serialize only the contract's explicitly allowlisted, typed facts.
    if let Ok(serde_json::Value::Object(facts)) = serde_json::to_value(record) {
        for name in [
            "actor",
            "seq",
            "from_state",
            "to_state",
            "close_outcome",
            "message_id",
            "reminder_outcome",
            "marker",
            "handoff",
        ] {
            if let Some(value) = facts.get(name).filter(|value| !value.is_null()) {
                let value = match value {
                    serde_json::Value::String(text) => text.clone(),
                    value => value.to_string(),
                };
                attributes.push(KeyValue::new(format!("atm.task.{name}"), value));
            }
        }
    }
    attributes
}

fn ends_assignment(kind: Kind) -> bool {
    match kind {
        Kind::Completed | Kind::Refused | Kind::Cancelled => true,
        Kind::Assigned
        | Kind::Acked
        | Kind::Started
        | Kind::Reassigned
        | Kind::Reopened
        | Kind::Rejected
        | Kind::Reminded
        | Kind::LeadNotified
        | Kind::Moved
        | Kind::Migrated
        | Kind::RemindersReset
        | Kind::PromptHandoff => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::{metrics::MeterProvider, trace::TracerProvider};

    #[test]
    fn assignment_event_byte_and_dedup_state_remain_bounded() {
        let tracer = opentelemetry_sdk::trace::SdkTracerProvider::builder()
            .with_id_generator(DurableIds)
            .build();
        let meter = opentelemetry_sdk::metrics::SdkMeterProvider::builder().build();
        let exporter = TaskExporter::new(tracer.tracer("bounds"), meter.meter("bounds"));
        for seq in 1..=128 {
            exporter
                .task(crate::otel_tests::record(
                    "large-history",
                    Kind::Reminded,
                    seq,
                    1,
                ))
                .unwrap();
        }
        {
            let state = exporter.state.lock().unwrap();
            let assignment = state.assignments.values().next().unwrap();
            assert_eq!(assignment.events.len(), EVENT_LIMIT);
            assert!(assignment.bytes <= BYTE_LIMIT);
        }
        for n in 0..=DEDUP_LIMIT {
            exporter
                .task(crate::otel_tests::record(
                    &format!("task-{n}"),
                    Kind::Assigned,
                    1,
                    1,
                ))
                .unwrap();
        }
        {
            let state = exporter.state.lock().unwrap();
            assert_eq!(state.assignments.len(), ACTIVE_LIMIT);
            assert_eq!(state.seen.len(), DEDUP_LIMIT);
            assert_eq!(state.order.len(), DEDUP_LIMIT);
        }
    }

    #[test]
    fn dedup_guarantee_expires_only_after_retained_window() {
        let mut state = State::default();
        let first = identity(&[b"first"]);
        assert!(!state.duplicate(first));
        assert!(state.duplicate(first));
        for n in 0..DEDUP_LIMIT {
            assert!(!state.duplicate(identity(&[&n.to_be_bytes()])));
        }
        assert!(!state.duplicate(first));
    }
}
