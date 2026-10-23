//! SQLite implementation of the durable cron transition contract.

use std::str::FromStr;
use std::sync::Arc;

use atm_storage::contract::sealed;
use atm_storage::{
    AgentName, AtmError, AtmErrorCode, AtmMessageId, CronCommitResult, CronCommittedNoticeRecord,
    CronDeliveryIntent, CronDeliveryKey, CronDiagnosticDetail, CronFailure, CronFailureKind,
    CronGeneration, CronLifecycleEvent, CronModuleMailIntent, CronModuleOutput, CronName,
    CronNoticeIntent, CronNoticeKey, CronRunTransition, CronScheduleId, CronSuccess,
    CronTransitionStore, CronTransitionWrite, DeliveryReceipt, DeliveryRetryCause,
    PendingCronDelivery, TeamName,
};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::shared_db::SharedDb;
use crate::store_support::{internal, stamp, write_error};

const EVENTS: &str = "cron_transition_event";
const INTENTS: &str = "cron_delivery_intent";

struct SqliteCronTransitionStore {
    db: Arc<SharedDb>,
}

pub(crate) fn cron_transition_store(
    db: Arc<SharedDb>,
) -> Arc<dyn CronTransitionStore + Send + Sync> {
    Arc::new(SqliteCronTransitionStore { db })
}

impl sealed::Sealed for SqliteCronTransitionStore {}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum StoredIntent {
    Lifecycle {
        team: String,
        recipient: String,
        name: String,
        event: StoredEvent,
    },
    ModuleMail {
        message_id: String,
        team: String,
        recipient: String,
        name: String,
        output: CronModuleOutput,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum StoredEvent {
    Error {
        failure_kind: CronFailureKind,
        code: Option<String>,
        detail: String,
    },
    Stopped {
        message: String,
    },
}

struct SqliteCommittedCronNoticeRecord {
    key: CronNoticeKey,
    team: TeamName,
    recipient: AgentName,
    name: CronName,
    event: CronLifecycleEvent,
}
impl sealed::Sealed for SqliteCommittedCronNoticeRecord {}
impl CronCommittedNoticeRecord for SqliteCommittedCronNoticeRecord {
    fn key(&self) -> CronNoticeKey {
        self.key
    }
    fn team(&self) -> &TeamName {
        &self.team
    }
    fn recipient(&self) -> &AgentName {
        &self.recipient
    }
    fn name(&self) -> &CronName {
        &self.name
    }
    fn event(&self) -> &CronLifecycleEvent {
        &self.event
    }
}

fn stored_id(id: CronScheduleId) -> Result<i64, AtmError> {
    i64::try_from(id.get())
        .map_err(|_| AtmError::validation("cron schedule id exceeds SQLite range"))
}
fn stored_seq(sequence: u64) -> Result<i64, AtmError> {
    i64::try_from(sequence)
        .map_err(|_| AtmError::validation("cron transition sequence exceeds SQLite range"))
}
fn failure_columns(transition: &CronRunTransition) -> (&'static str, Option<&CronFailure>) {
    match transition {
        CronRunTransition::Success { .. } => ("success", None),
        CronRunTransition::StopCheckContinues => ("continue", None),
        CronRunTransition::Failure(failure) => ("failure", Some(failure)),
        CronRunTransition::Stopped { .. } => ("stopped", None),
        CronRunTransition::SkippedInFlight => ("skipped", None),
    }
}
fn lifecycle_event(transition: &CronRunTransition) -> Option<CronLifecycleEvent> {
    match transition {
        CronRunTransition::Failure(failure) => Some(CronLifecycleEvent::Error {
            failure: failure.clone(),
        }),
        CronRunTransition::Stopped { reason } => Some(CronLifecycleEvent::Stopped {
            message: reason.clone(),
        }),
        _ => None,
    }
}
fn outcome(transition: &CronRunTransition) -> Option<String> {
    match transition {
        CronRunTransition::Success {
            outcome: CronSuccess::Ok,
        } => Some("ok".to_owned()),
        CronRunTransition::Success {
            outcome: CronSuccess::Actions { count },
        } => Some(format!("actions:{count}")),
        CronRunTransition::Failure(failure) => Some(format!("failure:{}", failure.detail.as_str())),
        CronRunTransition::Stopped { reason } => Some(format!("stopped: {}", reason.as_str())),
        CronRunTransition::StopCheckContinues | CronRunTransition::SkippedInFlight => None,
    }
}

fn encode(intent: &CronDeliveryIntent) -> Result<String, AtmError> {
    let stored = match intent {
        CronDeliveryIntent::Lifecycle(intent) => StoredIntent::Lifecycle {
            team: intent.team().as_str().to_owned(),
            recipient: intent.recipient().as_str().to_owned(),
            name: intent.name().as_str().to_owned(),
            event: match intent.event() {
                CronLifecycleEvent::Error { failure } => StoredEvent::Error {
                    failure_kind: failure.kind,
                    code: failure.code.map(|code| code.to_string()),
                    detail: failure.detail.as_str().to_owned(),
                },
                CronLifecycleEvent::Stopped { message } => StoredEvent::Stopped {
                    message: message.as_str().to_owned(),
                },
            },
        },
        CronDeliveryIntent::ModuleMail(intent) => StoredIntent::ModuleMail {
            message_id: intent.message_id.to_string(),
            team: intent.team.as_str().to_owned(),
            recipient: intent.recipient.as_str().to_owned(),
            name: intent.name.as_str().to_owned(),
            output: intent.output.clone(),
        },
    };
    serde_json::to_string(&stored).map_err(|error| internal(INTENTS, error))
}

fn decode(key: CronDeliveryKey, payload: &str) -> Result<CronDeliveryIntent, AtmError> {
    let stored: StoredIntent =
        serde_json::from_str(payload).map_err(|error| internal(INTENTS, error))?;
    match (key, stored) {
        (
            CronDeliveryKey::Lifecycle(key),
            StoredIntent::Lifecycle {
                team,
                recipient,
                name,
                event,
            },
        ) => {
            let event = match event {
                StoredEvent::Error {
                    failure_kind,
                    code,
                    detail,
                } => CronLifecycleEvent::Error {
                    failure: CronFailure {
                        kind: failure_kind,
                        code: code
                            .map(|value| {
                                AtmErrorCode::from_str(&value)
                                    .map_err(|error| internal(INTENTS, error))
                            })
                            .transpose()?,
                        detail: CronDiagnosticDetail::parse(detail)
                            .map_err(|error| internal(INTENTS, error.message()))?,
                    },
                },
                StoredEvent::Stopped { message } => CronLifecycleEvent::Stopped {
                    message: CronDiagnosticDetail::parse(message)
                        .map_err(|error| internal(INTENTS, error.message()))?,
                },
            };
            let record = SqliteCommittedCronNoticeRecord {
                key,
                team: TeamName::from_str(&team)
                    .map_err(|error| internal(INTENTS, error.message()))?,
                recipient: AgentName::from_str(&recipient)
                    .map_err(|error| internal(INTENTS, error.message()))?,
                name: CronName::parse(&name).map_err(|error| internal(INTENTS, error.message()))?,
                event,
            };
            Ok(CronDeliveryIntent::Lifecycle(
                CronNoticeIntent::from_committed(&record),
            ))
        }
        (
            key @ CronDeliveryKey::ModuleMail { .. },
            StoredIntent::ModuleMail {
                message_id,
                team,
                recipient,
                name,
                output,
            },
        ) => Ok(CronDeliveryIntent::ModuleMail(CronModuleMailIntent {
            key,
            message_id: AtmMessageId::from_str(&message_id)
                .map_err(|error| internal(INTENTS, error))?,
            team: TeamName::from_str(&team).map_err(|error| internal(INTENTS, error.message()))?,
            recipient: AgentName::from_str(&recipient)
                .map_err(|error| internal(INTENTS, error.message()))?,
            name: CronName::parse(&name).map_err(|error| internal(INTENTS, error.message()))?,
            output,
        })),
        _ => Err(internal(
            INTENTS,
            "stored delivery kind does not match its key",
        )),
    }
}

impl SqliteCronTransitionStore {
    fn persist_intent(
        transaction: &Transaction<'_>,
        intent: &CronDeliveryIntent,
    ) -> Result<(), AtmError> {
        let (id, generation, sequence, output_index, kind) = match intent {
            CronDeliveryIntent::Lifecycle(intent) => (
                intent.key().schedule_id,
                intent.key().generation,
                intent.key().transition_seq,
                None,
                "lifecycle",
            ),
            CronDeliveryIntent::ModuleMail(intent) => match intent.key {
                CronDeliveryKey::ModuleMail {
                    schedule_id,
                    generation,
                    transition_seq,
                    output_index,
                } => (
                    schedule_id,
                    generation,
                    transition_seq,
                    Some(i64::from(output_index)),
                    "module_mail",
                ),
                _ => unreachable!("module mail has module key"),
            },
        };
        transaction.execute("INSERT INTO cron_delivery_intent (schedule_id, generation, transition_seq, output_index, kind, payload) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![stored_id(id)?, i64::from(generation.get()), stored_seq(sequence)?, output_index, kind, encode(intent)?]).map_err(|error| AtmError::new(AtmErrorCode::StoreQueryFailed, format!("failed to persist cron delivery intent: {error}")))?;
        Ok(())
    }
}

impl CronTransitionStore for SqliteCronTransitionStore {
    fn commit(&self, write: CronTransitionWrite) -> Result<CronCommitResult, AtmError> {
        if matches!(write.transition, CronRunTransition::SkippedInFlight) {
            return Ok(CronCommitResult::Applied { intents: vec![] });
        }
        self.db.with_transaction(|transaction| {
            let id = stored_id(write.id)?;
            let schedule: Option<(String, String, String)> = transaction.query_row("SELECT team, name, created_by FROM cron_schedule WHERE id = ?1 AND generation = ?2 AND active = 1", params![id, i64::from(write.generation.get())], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional().map_err(|error| AtmError::new(AtmErrorCode::StoreQueryFailed, format!("failed to load cron schedule: {error}")))?;
            let Some((team, name, creator)) = schedule else { return Ok(CronCommitResult::StaleGeneration); };
            let (kind, failure) = failure_columns(&write.transition);
            let prior_failure: Option<(String, Option<String>, String)> = if kind == "failure" {
                transaction.query_row("SELECT failure_kind, failure_code, failure_detail FROM cron_transition_event WHERE schedule_id = ?1 AND generation = ?2 ORDER BY transition_seq DESC LIMIT 1", params![id, i64::from(write.generation.get())], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional().map_err(|error| AtmError::new(AtmErrorCode::StoreQueryFailed, format!("failed to read prior cron failure: {error}")))?
            } else { None };
            let next: i64 = transaction.query_row("SELECT COALESCE(MAX(transition_seq), 0) + 1 FROM cron_transition_event WHERE schedule_id = ?1 AND generation = ?2", params![id, i64::from(write.generation.get())], |row| row.get(0)).map_err(|error| AtmError::new(AtmErrorCode::StoreQueryFailed, format!("failed to select cron transition sequence: {error}")))?;
            transaction.execute("INSERT INTO cron_transition_event (schedule_id, generation, transition_seq, transition_at, transition_kind, failure_kind, failure_code, failure_detail) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)", params![id, i64::from(write.generation.get()), next, stamp(write.at), kind, failure.map(|f| format!("{:?}", f.kind)), failure.and_then(|f| f.code.map(|code| code.to_string())), failure.map(|f| f.detail.as_str())]).map_err(|error| write_error(&self.db, EVENTS, error))?;
            if let Some(outcome) = outcome(&write.transition) { transaction.execute("UPDATE cron_schedule SET last_run_at = ?1, last_outcome = ?2, ended_at = CASE WHEN ?3 = 'stopped' THEN ?1 ELSE ended_at END, active = CASE WHEN ?3 = 'stopped' THEN 0 ELSE active END WHERE id = ?4 AND generation = ?5 AND active = 1", params![stamp(write.at), outcome, kind, id, i64::from(write.generation.get())]).map_err(|error| write_error(&self.db, "cron_schedule", error))?; }
            let sequence = u64::try_from(next).map_err(|_| internal(EVENTS, "negative transition sequence"))?;
            let notice_key = CronNoticeKey { schedule_id: write.id, generation: write.generation, transition_seq: sequence };
            let mut intents = Vec::new();
            let repeated_failure = failure.is_some_and(|current| prior_failure.as_ref().is_some_and(|prior| prior.0 == format!("{:?}", current.kind) && prior.1.as_deref() == current.code.map(|code| code.to_string()).as_deref() && prior.2 == current.detail.as_str()));
            if let Some(event) = lifecycle_event(&write.transition).filter(|_| !repeated_failure) { let record = SqliteCommittedCronNoticeRecord { key: notice_key, team: TeamName::from_str(&team).map_err(|error| internal(EVENTS, error.message()))?, recipient: AgentName::from_str(&creator).map_err(|error| internal(EVENTS, error.message()))?, name: CronName::parse(&name).map_err(|error| internal(EVENTS, error.message()))?, event }; intents.push(CronDeliveryIntent::Lifecycle(CronNoticeIntent::from_committed(&record))); }
            for (index, output) in write.outputs.into_iter().enumerate() { let output_index = u16::try_from(index).map_err(|_| AtmError::validation("cron transition has too many outputs"))?; intents.push(CronDeliveryIntent::ModuleMail(CronModuleMailIntent { key: CronDeliveryKey::ModuleMail { schedule_id: write.id, generation: write.generation, transition_seq: sequence, output_index }, message_id: AtmMessageId::new(), team: TeamName::from_str(&team).map_err(|error| internal(EVENTS, error.message()))?, recipient: AgentName::from_str(&creator).map_err(|error| internal(EVENTS, error.message()))?, name: CronName::parse(&name).map_err(|error| internal(EVENTS, error.message()))?, output })); }
            for intent in &intents { Self::persist_intent(transaction, intent)?; }
            Ok(CronCommitResult::Applied { intents })
        })
    }

    fn pending(&self, limit: usize) -> Result<Vec<PendingCronDelivery>, AtmError> {
        self.db.read(move |connection| {
            let mut statement = connection.prepare("SELECT schedule_id, generation, transition_seq, output_index, kind, payload, state, retry_cause FROM cron_delivery_intent WHERE state IN ('pending', 'unknown') ORDER BY schedule_id, generation, transition_seq, COALESCE(output_index, -1) LIMIT ?1").map_err(|error| AtmError::new(AtmErrorCode::StoreQueryFailed, format!("failed to prepare cron pending query: {error}")))?;
            let rows = statement.query_map(params![i64::try_from(limit).unwrap_or(i64::MAX)], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?, row.get::<_, Option<i64>>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?, row.get::<_, String>(6)?, row.get::<_, Option<String>>(7)?))).map_err(|error| AtmError::new(AtmErrorCode::StoreQueryFailed, format!("failed to query cron pending intents: {error}")))?;
            rows.map(|row| { let (id, generation, sequence, output_index, kind, payload, state, retry_cause) = row.map_err(|error| AtmError::new(AtmErrorCode::StoreQueryFailed, format!("failed to decode cron pending row: {error}")))?; let schedule_id = CronScheduleId::new(u64::try_from(id).map_err(|_| internal(INTENTS, "negative schedule id"))?); let generation = CronGeneration::new(u32::try_from(generation).map_err(|_| internal(INTENTS, "invalid generation"))?).map_err(|_| internal(INTENTS, "zero generation"))?; let transition_seq = u64::try_from(sequence).map_err(|_| internal(INTENTS, "negative sequence"))?; let key = if kind == "lifecycle" { CronDeliveryKey::Lifecycle(CronNoticeKey { schedule_id, generation, transition_seq }) } else { CronDeliveryKey::ModuleMail { schedule_id, generation, transition_seq, output_index: u16::try_from(output_index.ok_or_else(|| internal(INTENTS, "module output missing index"))?).map_err(|_| internal(INTENTS, "invalid output index"))? } }; let intent = decode(key, &payload)?; if state == "unknown" { Ok(PendingCronDelivery::ReconciliationRequired { intent, retry_cause: DeliveryRetryCause::parse(retry_cause.ok_or_else(|| internal(INTENTS, "unknown delivery missing retry cause"))?).map_err(|error| internal(INTENTS, error.message()))? }) } else { Ok(PendingCronDelivery::ReadyToSend(intent)) } }).collect()
        })
    }

    fn mark_delivered(
        &self,
        key: CronDeliveryKey,
        receipt: DeliveryReceipt,
    ) -> Result<bool, AtmError> {
        self.db.with_transaction(|transaction| {
            let (id, generation, sequence, output_index) = match key { CronDeliveryKey::Lifecycle(key) => (key.schedule_id, key.generation, key.transition_seq, None), CronDeliveryKey::ModuleMail { schedule_id, generation, transition_seq, output_index } => (schedule_id, generation, transition_seq, Some(i64::from(output_index))) };
            let (state, receipt_text, retry_cause) = match receipt { DeliveryReceipt::Accepted { receipt_id } => ("delivered", Some(receipt_id), None), DeliveryReceipt::Suppressed => ("suppressed", None, None), DeliveryReceipt::RecipientUnavailable => ("recipient_unavailable", None, None), DeliveryReceipt::Unknown { retry_cause } => ("unknown", None, Some(retry_cause.as_str().to_owned())) };
            let changed = transaction.execute("UPDATE cron_delivery_intent SET state = ?1, receipt = ?2, retry_cause = ?3 WHERE schedule_id = ?4 AND generation = ?5 AND transition_seq = ?6 AND ((output_index IS NULL AND ?7 IS NULL) OR output_index = ?7) AND state = 'pending'", params![state, receipt_text, retry_cause, stored_id(id)?, i64::from(generation.get()), stored_seq(sequence)?, output_index]).map_err(|error| write_error(&self.db, INTENTS, error))?;
            Ok(changed == 1)
        })
    }
}
