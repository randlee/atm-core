//! Shared bounded substrate for all SQLite read lanes.
//!
//! A lane owns independent defensive read-only connections.  The pool never
//! acquires the writer lane and is intentionally usable by mailbox and search
//! adapters alike so their fan-out and deadline behavior cannot drift.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use atm_storage::{AtmError, ReadLaneError};
use rusqlite::{Connection, InterruptHandle};

use crate::shared_db::{SharedDbTarget, open_read_connection_for_target};

pub(crate) struct ReaderPool {
    workers: Vec<Worker>,
    next_worker: AtomicUsize,
}

/// Bounded knobs for one independent read lane.  AV.1b adds the doctor lane
/// to the same configuration surface; keeping the budget arithmetic here
/// makes it impossible for a future lane to silently oversubscribe SQLite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReaderPoolConfig {
    pub(crate) pool_size: usize,
    pub(crate) queue_depth: usize,
    pub(crate) interrupt_grace: Duration,
    pub(crate) max_quarantined: usize,
}

impl ReaderPoolConfig {
    pub(crate) const fn mailbox_defaults() -> Self {
        Self {
            pool_size: 4,
            queue_depth: 16,
            interrupt_grace: Duration::from_millis(250),
            max_quarantined: 4,
        }
    }

    pub(crate) const fn search_defaults() -> Self {
        Self {
            pool_size: 2,
            queue_depth: 8,
            interrupt_grace: Duration::from_millis(250),
            max_quarantined: 2,
        }
    }
}

/// AV.1b's doctor lane is included now so the connection cap remains stable
/// as the stacked handler-cutover branch lands.
pub(crate) const DEFAULT_DOCTOR_READER_CONFIG: ReaderPoolConfig = ReaderPoolConfig {
    pool_size: 4,
    queue_depth: 16,
    interrupt_grace: Duration::from_millis(250),
    max_quarantined: 4,
};

pub(crate) const DEFAULT_MAX_READER_CONNECTIONS: usize = 32;

pub(crate) fn validate_connection_budget(
    mailbox: ReaderPoolConfig,
    search: ReaderPoolConfig,
    doctor: ReaderPoolConfig,
    max_connections: usize,
) -> Result<(), AtmError> {
    let worst_case = 1usize
        .saturating_add(mailbox.pool_size)
        .saturating_add(search.pool_size)
        .saturating_add(doctor.pool_size)
        .saturating_add(mailbox.max_quarantined)
        .saturating_add(search.max_quarantined)
        .saturating_add(doctor.max_quarantined)
        .saturating_add(1); // analyst RO connection
    if mailbox.pool_size == 0
        || search.pool_size == 0
        || doctor.pool_size == 0
        || mailbox.queue_depth == 0
        || search.queue_depth == 0
        || doctor.queue_depth == 0
        || max_connections == 0
        || worst_case > max_connections
    {
        return Err(AtmError::validation(format!(
            "SQLite reader connection budget exceeds max_connections: writer=1, mailbox_pool={}, search_pool={}, doctor_pool={}, mailbox_max_quarantined={}, search_max_quarantined={}, doctor_max_quarantined={}, analyst=1, total={worst_case}, max_connections={max_connections}",
            mailbox.pool_size,
            search.pool_size,
            doctor.pool_size,
            mailbox.max_quarantined,
            search.max_quarantined,
            doctor.max_quarantined,
        )));
    }
    Ok(())
}

struct Worker {
    sender: tokio::sync::mpsc::Sender<Request>,
    interrupt: InterruptHandle,
}

struct Request {
    deadline: Instant,
    run: Box<dyn FnOnce(&Connection, &SharedDbTarget, bool) + Send>,
}

impl std::fmt::Debug for ReaderPool {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReaderPool")
            .field("worker_count", &self.workers.len())
            .finish_non_exhaustive()
    }
}

impl ReaderPool {
    pub(crate) fn start(
        lane: &'static str,
        target: Arc<SharedDbTarget>,
        config: ReaderPoolConfig,
    ) -> Result<Self, AtmError> {
        let pool_size = config.pool_size;
        let queue_depth = config.queue_depth;
        if pool_size == 0 || queue_depth == 0 {
            return Err(AtmError::validation(format!(
                "SQLite {lane} reader pool requires non-zero pool_size and queue_depth"
            )));
        }
        let queue_per_worker = queue_depth.div_ceil(pool_size);
        let mut workers = Vec::with_capacity(pool_size);
        for index in 0..pool_size {
            let connection = open_read_connection_for_target(target.as_ref())?;
            let interrupt = connection.get_interrupt_handle();
            let (sender, mut receiver) = tokio::sync::mpsc::channel::<Request>(queue_per_worker);
            let worker_target = Arc::clone(&target);
            thread::Builder::new()
                .name(format!("atm-sqlite-{lane}-reader-{index}"))
                .spawn(move || {
                    while let Some(request) = receiver.blocking_recv() {
                        let expired_in_queue = Instant::now() >= request.deadline;
                        (request.run)(&connection, worker_target.as_ref(), expired_in_queue);
                    }
                })
                .map_err(|error| {
                    AtmError::daemon_unavailable(format!(
                        "failed to start SQLite {lane} reader worker {index}: {error}"
                    ))
                })?;
            workers.push(Worker { sender, interrupt });
        }
        Ok(Self {
            workers,
            next_worker: AtomicUsize::new(0),
        })
    }

    pub(crate) async fn submit<T, F>(
        &self,
        deadline: Duration,
        operation: F,
    ) -> Result<T, ReadLaneError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection, &SharedDbTarget) -> Result<T, ReadLaneError> + Send + 'static,
    {
        let expires_at = Instant::now() + deadline;
        let worker_index = self.reserve_worker()?;
        let (reply, response) = tokio::sync::oneshot::channel();
        let request = Request {
            deadline: expires_at,
            run: Box::new(move |connection, target, expired_in_queue| {
                let result = if expired_in_queue {
                    Err(ReadLaneError::DeadlineExpired {
                        stage: "waiting in queue",
                    })
                } else {
                    operation(connection, target)
                };
                let _ = reply.send(result);
            }),
        };
        let remaining = expires_at.saturating_duration_since(Instant::now());
        tokio::time::timeout(remaining, self.workers[worker_index].sender.send(request))
            .await
            .map_err(|_| ReadLaneError::DeadlineExpired {
                stage: "waiting in queue",
            })?
            .map_err(|_| ReadLaneError::Unavailable {
                message: "reader worker stopped".to_owned(),
            })?;
        let remaining = expires_at.saturating_duration_since(Instant::now());
        tokio::time::timeout(remaining, response)
            .await
            .map_err(|_| {
                self.workers[worker_index].interrupt.interrupt();
                ReadLaneError::DeadlineExpired {
                    stage: "executing active query",
                }
            })?
            .map_err(|_| ReadLaneError::Unavailable {
                message: "reader worker closed its reply channel".to_owned(),
            })?
    }

    pub(crate) fn submit_blocking<T, F>(
        &self,
        deadline: Duration,
        operation: F,
    ) -> Result<T, ReadLaneError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection, &SharedDbTarget) -> Result<T, ReadLaneError> + Send + 'static,
    {
        let expires_at = Instant::now() + deadline;
        let worker_index = self.reserve_worker()?;
        let (reply, response) = mpsc::sync_channel(1);
        let mut request = Request {
            deadline: expires_at,
            run: Box::new(move |connection, target, expired_in_queue| {
                let result = if expired_in_queue {
                    Err(ReadLaneError::DeadlineExpired {
                        stage: "waiting in queue",
                    })
                } else {
                    operation(connection, target)
                };
                let _ = reply.send(result);
            }),
        };
        loop {
            match self.workers[worker_index].sender.try_send(request) {
                Ok(()) => break,
                Err(tokio::sync::mpsc::error::TrySendError::Full(returned)) => {
                    if Instant::now() >= expires_at {
                        return Err(ReadLaneError::DeadlineExpired {
                            stage: "waiting in queue",
                        });
                    }
                    request = returned;
                    thread::park_timeout(Duration::from_millis(2));
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    return Err(ReadLaneError::Unavailable {
                        message: "reader worker stopped".to_owned(),
                    });
                }
            }
        }
        response
            .recv_timeout(expires_at.saturating_duration_since(Instant::now()))
            .map_err(|error| match error {
                RecvTimeoutError::Timeout => {
                    self.workers[worker_index].interrupt.interrupt();
                    ReadLaneError::DeadlineExpired {
                        stage: "executing active query",
                    }
                }
                RecvTimeoutError::Disconnected => ReadLaneError::Unavailable {
                    message: "reader worker closed its reply channel".to_owned(),
                },
            })?
    }

    fn reserve_worker(&self) -> Result<usize, ReadLaneError> {
        let count = self.workers.len();
        if count == 0 {
            return Err(ReadLaneError::Unavailable {
                message: "reader pool has no workers".to_owned(),
            });
        }
        let start = self.next_worker.fetch_add(1, Ordering::Relaxed);
        for offset in 0..count {
            let index = (start + offset) % count;
            if !self.workers[index].sender.is_closed() && self.workers[index].sender.capacity() > 0
            {
                return Ok(index);
            }
        }
        Err(ReadLaneError::Saturated {
            reason: "all bounded reader queues are full",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_DOCTOR_READER_CONFIG, DEFAULT_MAX_READER_CONNECTIONS, ReaderPool, ReaderPoolConfig,
        validate_connection_budget,
    };
    use crate::shared_db::SharedDbTarget;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    #[test]
    fn documented_default_connection_budget_is_within_the_cap() {
        validate_connection_budget(
            ReaderPoolConfig::mailbox_defaults(),
            ReaderPoolConfig::search_defaults(),
            DEFAULT_DOCTOR_READER_CONFIG,
            DEFAULT_MAX_READER_CONNECTIONS,
        )
        .expect("1 writer + 4 mailbox + 2 search + 4 doctor + 10 quarantine + 1 analyst = 22");
    }

    #[test]
    fn connection_budget_fails_closed_and_names_each_contributor() {
        let error = validate_connection_budget(
            ReaderPoolConfig::mailbox_defaults(),
            ReaderPoolConfig::search_defaults(),
            DEFAULT_DOCTOR_READER_CONFIG,
            21,
        )
        .expect_err("22 reader connections must not fit under a cap of 21");
        let message = error.message();
        assert!(message.contains("mailbox_pool=4"));
        assert!(message.contains("search_pool=2"));
        assert!(message.contains("doctor_pool=4"));
        assert!(message.contains("max_connections=21"));
    }

    #[tokio::test]
    async fn two_reader_workers_execute_independent_queries_in_parallel() {
        let database = tempfile::NamedTempFile::new().expect("temporary database path");
        rusqlite::Connection::open(database.path()).expect("initialize sqlite file");
        let pool = ReaderPool::start(
            "test",
            Arc::new(SharedDbTarget::Path(database.path().to_path_buf())),
            ReaderPoolConfig {
                pool_size: 2,
                queue_depth: 2,
                interrupt_grace: Duration::from_millis(250),
                max_quarantined: 2,
            },
        )
        .expect("reader pool");
        let started = Instant::now();
        let (left, right) = tokio::join!(
            pool.submit(Duration::from_secs(1), |_, _| {
                std::thread::sleep(Duration::from_millis(80));
                Ok::<_, atm_storage::ReadLaneError>("left")
            }),
            pool.submit(Duration::from_secs(1), |_, _| {
                std::thread::sleep(Duration::from_millis(80));
                Ok::<_, atm_storage::ReadLaneError>("right")
            })
        );
        assert_eq!(left.expect("left query"), "left");
        assert_eq!(right.expect("right query"), "right");
        assert!(
            started.elapsed() < Duration::from_millis(145),
            "two worker pool must not serialize independent reads"
        );
    }
}
