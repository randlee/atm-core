//! Bounded control-path SQLite connections.
//!
//! Control-path reads and writes (roster, graft receiver leases, pending
//! nudges, template catalog, peer config, mailbox metadata) do not go through
//! the bounded reader lanes; they borrow a connection directly through
//! [`crate::shared_db::SharedDb::with_connection`].
//!
//! Parking idle connections for reuse is not by itself a bound: a burst of
//! concurrent admissions each finds the idle set empty and opens its own
//! connection, so live connections still scale with in-flight admissions.
//! Each open costs the database file plus its `-wal` sidecar, so past the
//! process descriptor limit SQLite reports `SQLITE_CANTOPEN` both at
//! `Connection::open` *and* at query time on an already-open connection,
//! because the WAL sidecars are opened lazily on first read. The admission
//! path then surfaces a mailbox write failure for a row that already
//! committed. This pool therefore bounds *live* connections, not just idle
//! ones: a borrow past the bound waits for a returned connection instead of
//! opening another descriptor.

use crate::shared_db::{SharedDbTarget, SqliteConnection, open_connection_for_target};
use atm_storage::AtmError;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

/// Simultaneously live control-path connections. Bounds the control path's
/// descriptor demand independently of the in-flight admission count.
///
/// The runtime raises the macOS launchd soft descriptor limit before opening
/// SQLite or listeners. A budget of 64 still leaves substantial descriptor
/// headroom while allowing a normal admission burst to avoid serialising at
/// the control-path borrow gate.
pub(crate) const MAX_CONTROL_PATH_CONNECTIONS: usize = 64;

#[derive(Default)]
struct PoolState {
    idle: Vec<SqliteConnection>,
    live: usize,
    waiting: usize,
}

/// Bounded reuse of the direct control-path SQLite connections.
///
/// Connections are parked only after an operation completes normally: an
/// operation that returned an error may have left connection state behind,
/// and a panicking one unwinds through [`ControlPathLease`]'s drop, which
/// releases the bound rather than leaking it.
pub(crate) struct ControlPathConnections {
    target: Arc<SharedDbTarget>,
    state: Mutex<PoolState>,
    returned: Condvar,
}

impl ControlPathConnections {
    pub(crate) fn new(target: Arc<SharedDbTarget>) -> Self {
        Self {
            target,
            state: Mutex::new(PoolState::default()),
            returned: Condvar::new(),
        }
    }

    /// Number of borrows currently blocked on the connection bound.
    #[cfg(test)]
    pub(crate) fn waiting_borrows(&self) -> usize {
        self.lock().waiting
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, PoolState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Bounded starvation is reported with the same typed lock-timeout
    /// contract SQLite's own busy timeout uses, never by opening an
    /// unbudgeted descriptor.
    fn starvation_error(&self) -> AtmError {
        match self.target.as_ref() {
            SharedDbTarget::Path(path) => AtmError::mailbox_lock_timeout(path),
            #[cfg(test)]
            SharedDbTarget::InMemory { .. } => AtmError::mailbox_lock(format!(
                "timed out waiting for a control-path sqlite connection on {}",
                self.target.display()
            )),
        }
    }

    fn park(&self, connection: SqliteConnection) {
        self.lock().idle.push(connection);
        self.returned.notify_one();
    }

    fn release(&self) {
        self.lock().live -= 1;
        self.returned.notify_one();
    }

    fn discard(&self, connection: SqliteConnection) {
        drop(connection);
        self.release();
    }
}

impl std::fmt::Debug for ControlPathConnections {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlPathConnections")
            .field("target", &self.target.display())
            .finish()
    }
}

// sc-boundary SCB-CYCLE-003: kept as free functions, not inherent associated
// functions on `ControlPathConnections`. Naming `ControlPathLease` from a
// method on the pool creates a two-owner reference cycle with the lease, which
// already has to name the pool it returns its bound to.

/// Borrows a bounded control-path connection, waiting for a returned one
/// rather than opening an unbudgeted descriptor once the bound is reached.
pub(crate) fn checkout(pool: &ControlPathConnections) -> Result<ControlPathLease<'_>, AtmError> {
    let deadline = Instant::now() + atm_storage::request_budget::SQLITE_BUSY_TIMEOUT;
    let mut state = pool.lock();
    loop {
        if let Some(connection) = state.idle.pop() {
            return Ok(lease(pool, connection));
        }
        if state.live < MAX_CONTROL_PATH_CONNECTIONS {
            state.live += 1;
            drop(state);
            return match open_connection_for_target(pool.target.as_ref()) {
                Ok(connection) => Ok(lease(pool, connection)),
                Err(error) => {
                    pool.release();
                    Err(error)
                }
            };
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return Err(pool.starvation_error());
        };
        state.waiting += 1;
        let (guard, _) = pool
            .returned
            .wait_timeout(state, remaining)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state = guard;
        state.waiting -= 1;
    }
}

fn lease(pool: &ControlPathConnections, connection: SqliteConnection) -> ControlPathLease<'_> {
    ControlPathLease {
        pool,
        connection: Some(connection),
    }
}

/// Borrowed control-path connection.
///
/// Dropping the lease without [`ControlPathLease::park`] returns the bound
/// without parking the connection, which is what an errored or panicking
/// operation must do: its connection state is no longer known-good.
pub(crate) struct ControlPathLease<'pool> {
    pool: &'pool ControlPathConnections,
    connection: Option<SqliteConnection>,
}

impl ControlPathLease<'_> {
    pub(crate) fn connection(&mut self) -> &mut SqliteConnection {
        self.connection
            .as_mut()
            .expect("control-path lease holds its connection until it is parked or dropped")
    }

    pub(crate) fn park(mut self) {
        if let Some(connection) = self.connection.take() {
            self.pool.park(connection);
        }
    }
}

impl Drop for ControlPathLease<'_> {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.take() {
            self.pool.discard(connection);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared_db::{opened_connection_count, reset_opened_connection_count};
    use crate::shared_db_reader_lanes::SharedDb;
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    /// Concurrency modelled well above the bound so an unbounded pool opens a
    /// descriptor per in-flight borrow rather than reusing the bounded set.
    const BURST_BORROWS: usize = 64;

    /// Bounds the spin below so a regression stalls the test with a verdict
    /// instead of hanging the suite. This is a failure ceiling, never a
    /// synchronisation delay: the assertions are driven by observed state.
    const SPIN_CEILING: Duration = Duration::from_secs(30);

    fn spin_until(what: &str, mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + SPIN_CEILING;
        while !condition() {
            assert!(Instant::now() < deadline, "timed out waiting until {what}");
            std::thread::yield_now();
        }
    }

    #[test]
    fn a_concurrent_burst_never_opens_more_connections_than_the_bound() {
        let db = SharedDb::open_in_memory_for_test().expect("in-memory sqlite boundary");
        reset_opened_connection_count(db.target());
        let start = Arc::new(Barrier::new(BURST_BORROWS));

        std::thread::scope(|scope| {
            for _ in 0..BURST_BORROWS {
                let pool = Arc::clone(&db.control_path);
                let start = Arc::clone(&start);
                scope.spawn(move || {
                    start.wait();
                    let lease = checkout(&pool).expect("bounded control-path borrow");
                    lease.park();
                });
            }
        });

        assert!(
            opened_connection_count(db.target()) <= MAX_CONTROL_PATH_CONNECTIONS,
            "a control-path burst must never open more than {MAX_CONTROL_PATH_CONNECTIONS} \
             connections; opening one per in-flight borrow exhausts the process descriptor \
             limit and SQLite then reports CannotOpen at query time as a mailbox write failure"
        );
    }

    #[test]
    fn a_borrow_past_the_bound_waits_instead_of_opening_another_connection() {
        let db = SharedDb::open_in_memory_for_test().expect("in-memory sqlite boundary");
        reset_opened_connection_count(db.target());
        let held = Arc::new(Barrier::new(MAX_CONTROL_PATH_CONNECTIONS + 1));
        let release = Arc::new((Mutex::new(false), Condvar::new()));

        std::thread::scope(|scope| {
            for _ in 0..MAX_CONTROL_PATH_CONNECTIONS {
                let pool = Arc::clone(&db.control_path);
                let held = Arc::clone(&held);
                let release = Arc::clone(&release);
                scope.spawn(move || {
                    let lease = checkout(&pool).expect("bounded control-path borrow");
                    held.wait();
                    let (lock, signal) = release.as_ref();
                    let mut released = lock.lock().expect("release lock");
                    while !*released {
                        released = signal.wait(released).expect("release signal");
                    }
                    drop(released);
                    lease.park();
                });
            }
            // Every bounded connection is now checked out and none is parked.
            held.wait();

            let extra_pool = Arc::clone(&db.control_path);
            let extra = scope.spawn(move || {
                checkout(&extra_pool)
                    .expect("bounded control-path borrow")
                    .park();
            });
            // Either the extra borrow blocks on the bound (correct) or it
            // opens an unbudgeted connection (the regression). Both outcomes
            // are observable state, so neither branch needs a delay.
            spin_until(
                "the extra borrow blocks or opens its own connection",
                || {
                    db.control_path.waiting_borrows() == 1
                        || opened_connection_count(db.target()) > MAX_CONTROL_PATH_CONNECTIONS
                },
            );

            let (lock, signal) = release.as_ref();
            *lock.lock().expect("release lock") = true;
            signal.notify_all();
            extra.join().expect("extra borrow thread");
        });

        assert_eq!(
            opened_connection_count(db.target()),
            MAX_CONTROL_PATH_CONNECTIONS,
            "a borrow past the bound must wait for a returned connection; opening another \
             one makes control-path descriptor demand scale with in-flight admissions"
        );
    }

    #[test]
    fn an_admission_sized_burst_does_not_queue_behind_an_eight_slot_gate() {
        const ADMISSION_BURST: usize = 64;

        let db = SharedDb::open_in_memory_for_test().expect("in-memory sqlite boundary");
        let acquired = Arc::new(AtomicUsize::new(0));
        let release = Arc::new((Mutex::new(false), Condvar::new()));

        std::thread::scope(|scope| {
            for _ in 0..ADMISSION_BURST {
                let pool = Arc::clone(&db.control_path);
                let acquired = Arc::clone(&acquired);
                let release = Arc::clone(&release);
                scope.spawn(move || {
                    let lease = checkout(&pool).expect("control-path borrow");
                    acquired.fetch_add(1, Ordering::Release);
                    let (lock, signal) = release.as_ref();
                    let mut released = lock.lock().expect("release lock");
                    while !*released {
                        released = signal.wait(released).expect("release signal");
                    }
                    drop(released);
                    lease.park();
                });
            }

            spin_until("the admission burst acquires or queues at the pool", || {
                acquired.load(Ordering::Acquire) == ADMISSION_BURST
                    || db.control_path.waiting_borrows() > 0
            });
            let acquired_without_queue = acquired.load(Ordering::Acquire);

            let (lock, signal) = release.as_ref();
            *lock.lock().expect("release lock") = true;
            signal.notify_all();

            assert_eq!(
                acquired_without_queue, ADMISSION_BURST,
                "an admission-sized burst must not queue behind the r4 eight-slot control-path gate"
            );
        });
    }

    #[test]
    fn an_errored_borrow_returns_its_bound_rather_than_leaking_it() {
        let db = SharedDb::open_in_memory_for_test().expect("in-memory sqlite boundary");
        reset_opened_connection_count(db.target());

        for _ in 0..(MAX_CONTROL_PATH_CONNECTIONS * 4) {
            let lease = checkout(&db.control_path).expect("control-path borrow");
            // Dropping without parking models an operation that failed or
            // panicked: the connection is discarded, never reused.
            drop(lease);
        }

        checkout(&db.control_path)
            .expect("a discarded borrow must not permanently consume the bound")
            .park();
    }
}
