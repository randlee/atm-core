use super::{
    MAX_RECONCILE_DEBOUNCE_EXTENSIONS, MAX_RECONCILE_FINGERPRINT_KEYS,
    MAX_RECONCILE_FINGERPRINTS_PER_KEY, ReconcileRuntime,
};
use crate::worker_support::{
    reap_retained_join_helpers, reap_retained_join_helpers_until_empty_for_test,
    retained_join_helper_count_for_test,
};
use atm_core::boundary::{
    self, InboxIngress, InboxIngressDiagnosticsRequest, InboxIngressDiagnosticsResponse,
    InboxIngressIdentityFingerprintRequest, InboxIngressIdentityFingerprintResponse,
    InboxIngressImportRequest, InboxIngressImportResponse, NotificationEvent, NotificationSink,
    ReconcileRequest, RosterStore, RosterStoreHealthSnapshot, RosterStoreHealthSnapshotRequest,
    RosterStoreHealthSnapshotResponse, RosterStoreListTeamsRequest, RosterStoreListTeamsResponse,
    RosterStoreLoadRosterRequest, RosterStoreLoadRosterResponse, RosterStoreQueryMembershipRequest,
    RosterStoreQueryMembershipResponse, RosterStoreReplaceRosterRequest,
    RosterStoreReplaceRosterResponse, WatchEventBatch, WatchEventSource, WatchSubscriptionRequest,
};
use atm_core::error::AtmError;
use atm_core::protocol::ReconcileResult;
use atm_core::roles::ROLE_TEAM_LEAD;
use atm_core::schema::{AtmMessageId, MessageEnvelope};
use atm_core::types::IsoTimestamp;
use chrono::Utc;
use serde_json::{Map, json};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use tempfile::TempDir;

fn unique_home_dir() -> std::path::PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "atm-reconcile-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn request() -> ReconcileRequest {
    ReconcileRequest {
        home_dir: unique_home_dir(),
        team: "test-team".parse().expect("team"),
        agent: "test-agent".parse().expect("agent"),
    }
}

fn request_for(agent: &str) -> ReconcileRequest {
    ReconcileRequest {
        home_dir: unique_home_dir(),
        team: "test-team".parse().expect("team"),
        agent: agent.parse().expect("agent"),
    }
}

fn prepare_started_runtime(
    runtime: &ReconcileRuntime,
) -> std::sync::mpsc::Receiver<super::ReconcileCommand> {
    let (command_tx, command_rx) = std::sync::mpsc::sync_channel(16);
    runtime
        .inner
        .command_tx
        .set(command_tx)
        .expect("set command tx");
    runtime.inner.start_claimed.store(true, Ordering::Release);
    runtime.inner.mark_started();
    command_rx
}

fn spawn_runtime_worker(
    runtime: &ReconcileRuntime,
    command_rx: std::sync::mpsc::Receiver<super::ReconcileCommand>,
) {
    let inner = Arc::clone(&runtime.inner);
    let handle = std::thread::Builder::new()
        .name("atm-daemon-reconcile-test".to_string())
        .spawn(move || super::reconcile_worker_loop(inner, command_rx))
        .expect("spawn worker");
    runtime
        .inner
        .worker
        .install(handle)
        .expect("install worker");
}

fn dispatch_reconcile_command_for_test(
    runtime: &ReconcileRuntime,
    request: ReconcileRequest,
) -> std::sync::mpsc::Receiver<Result<ReconcileResult, AtmError>> {
    runtime
        .dispatch_reconcile_command(request)
        .expect("dispatch command")
        .0
}

#[test]
fn reconcile_runtime_actor_coalesces_identical_requests_into_one_worker_run() {
    let calls = Arc::new(Mutex::new(0usize));
    let runtime = ReconcileRuntime::new_for_test(
        Arc::new({
            let calls = Arc::clone(&calls);
            move |_| {
                *calls.lock().expect("calls") += 1;
                Ok(super::ReconcileExecution {
                    result: ReconcileResult {
                        observed_paths: 2,
                        imported_sources: 1,
                    },
                    current_fingerprints: Some(Default::default()),
                })
            }
        }),
        Duration::from_millis(200),
    );
    let command_rx = prepare_started_runtime(&runtime);

    let request = request();
    let first = dispatch_reconcile_command_for_test(&runtime, request.clone());
    let second = dispatch_reconcile_command_for_test(&runtime, request);
    spawn_runtime_worker(&runtime, command_rx);

    assert_eq!(
        first.recv().expect("first reply").expect("first"),
        ReconcileResult {
            observed_paths: 2,
            imported_sources: 1,
        }
    );
    assert_eq!(
        second.recv().expect("second reply").expect("second"),
        ReconcileResult {
            observed_paths: 2,
            imported_sources: 1,
        }
    );
    assert_eq!(*calls.lock().expect("calls"), 1);
    runtime.shutdown().expect("shutdown");
}

#[test]
#[serial_test::serial(env)]
fn reconcile_runtime_actor_fans_one_result_to_all_waiters_for_a_key() {
    reap_retained_join_helpers();
    let calls = Arc::new(Mutex::new(0usize));
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let runtime = ReconcileRuntime::new_for_test(
        Arc::new({
            let calls = Arc::clone(&calls);
            let gate = Arc::clone(&gate);
            move |_| {
                *calls.lock().expect("calls") += 1;
                let (ready_lock, ready_wake) = &*gate;
                let mut ready = ready_lock.lock().expect("ready lock");
                while !*ready {
                    ready = ready_wake.wait(ready).expect("ready wait");
                }
                Ok(super::ReconcileExecution {
                    result: ReconcileResult {
                        observed_paths: 7,
                        imported_sources: 3,
                    },
                    current_fingerprints: Some(Default::default()),
                })
            }
        }),
        Duration::from_millis(200),
    );
    let command_rx = prepare_started_runtime(&runtime);

    let request = request();
    let (submitted_tx, submitted_rx) = std::sync::mpsc::sync_channel(2);
    for request in [request.clone(), request] {
        let runtime = runtime.clone();
        let submitted_tx = submitted_tx.clone();
        std::thread::spawn(move || {
            let reply_rx = dispatch_reconcile_command_for_test(&runtime, request);
            submitted_tx
                .send(reply_rx)
                .expect("submitted reconcile command");
        });
    }
    drop(submitted_tx);
    let first = submitted_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("first submitted");
    let second = submitted_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("second submitted");
    spawn_runtime_worker(&runtime, command_rx);

    {
        let (ready_lock, ready_wake) = &*gate;
        *ready_lock.lock().expect("ready lock") = true;
        ready_wake.notify_all();
    }

    let first_result = first.recv().expect("first recv").expect("first");
    let second_result = second.recv().expect("second recv").expect("second");
    assert_eq!(first_result, second_result);
    assert_eq!(first_result.observed_paths, 7);
    assert_eq!(first_result.imported_sources, 3);
    assert_eq!(*calls.lock().expect("calls"), 1);
    runtime.shutdown().expect("shutdown");
}

#[test]
#[serial_test::serial(env)]
fn reconcile_runtime_actor_preserves_bounded_debounce_extensions() {
    reap_retained_join_helpers();
    let calls = Arc::new(Mutex::new(0usize));
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let runtime = ReconcileRuntime::new_for_test(
        Arc::new({
            let calls = Arc::clone(&calls);
            let gate = Arc::clone(&gate);
            move |_| {
                *calls.lock().expect("calls") += 1;
                let (ready_lock, ready_wake) = &*gate;
                let mut ready = ready_lock.lock().expect("ready lock");
                while !*ready {
                    ready = ready_wake.wait(ready).expect("ready wait");
                }
                Ok(super::ReconcileExecution {
                    result: ReconcileResult {
                        observed_paths: 1,
                        imported_sources: 1,
                    },
                    current_fingerprints: Some(Default::default()),
                })
            }
        }),
        Duration::from_millis(25),
    );
    let command_rx = prepare_started_runtime(&runtime);

    let request = request();
    let submission_count = 2 + MAX_RECONCILE_DEBOUNCE_EXTENSIONS as usize;
    let (submitted_tx, submitted_rx) = std::sync::mpsc::sync_channel(submission_count);
    let mut replies = Vec::new();
    for _ in 0..submission_count {
        let runtime = runtime.clone();
        let request = request.clone();
        let submitted_tx = submitted_tx.clone();
        std::thread::spawn(move || {
            let reply_rx = dispatch_reconcile_command_for_test(&runtime, request);
            submitted_tx
                .send(reply_rx)
                .expect("submitted reconcile command");
        });
    }
    drop(submitted_tx);
    for _ in 0..submission_count {
        replies.push(
            submitted_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("submitted reconcile command"),
        );
    }
    spawn_runtime_worker(&runtime, command_rx);

    {
        let (ready_lock, ready_wake) = &*gate;
        *ready_lock.lock().expect("ready lock") = true;
        ready_wake.notify_all();
    }

    for reply in replies {
        let result = reply.recv().expect("recv").expect("result");
        assert_eq!(result.observed_paths, 1);
        assert_eq!(result.imported_sources, 1);
    }
    assert_eq!(*calls.lock().expect("calls"), 2);
    runtime.shutdown().expect("shutdown");
}

#[test]
fn reconcile_runtime_actor_cutover_removes_shared_state_runtime_path() {
    let source = include_str!("reconcile_runtime.rs");
    assert!(
        !source.contains("Mutex<ReconcileState>"),
        "shared-state reconcile runtime lock path must be absent after cutover"
    );
    assert!(
        !source.contains("Condvar"),
        "shared-state reconcile runtime condvar path must be absent after cutover"
    );
    assert!(
        !source.contains("Arc<Mutex<NotificationFingerprintRegistry>>"),
        "fingerprint registry side mutex must be absent after cutover"
    );
}

#[test]
#[serial_test::serial(env)]
fn reconcile_runtime_actor_shutdown_stays_bounded() {
    reap_retained_join_helpers();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (worker_done_tx, worker_done_rx) = std::sync::mpsc::sync_channel(1);
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let runtime = ReconcileRuntime::new_for_test(
        Arc::new({
            let release = Arc::clone(&release);
            let worker_done_tx = worker_done_tx.clone();
            move |_| {
                started_tx.send(()).expect("started");
                let (released, wake) = &*release;
                let mut released = released.lock().expect("released");
                while !*released {
                    let wait = wake
                        .wait_timeout(released, Duration::from_secs(1))
                        .expect("wait release");
                    released = wait.0;
                }
                worker_done_tx.send(()).expect("worker done");
                Ok(super::ReconcileExecution {
                    result: ReconcileResult {
                        observed_paths: 1,
                        imported_sources: 1,
                    },
                    current_fingerprints: Some(Default::default()),
                })
            }
        }),
        Duration::from_millis(10),
    );
    runtime.start().expect("start");

    let runtime_for_thread = runtime.clone();
    let join = std::thread::spawn(move || runtime_for_thread.reconcile(request()));
    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("started");

    let error = runtime.shutdown().expect_err("shutdown should time out");
    assert!(
        error
            .message
            .contains("reconcile runtime worker exceeded the bounded shutdown deadline")
    );
    assert_eq!(retained_join_helper_count_for_test(), 1);

    let recovery_runtime = ReconcileRuntime::new_for_test(
        Arc::new(|_| {
            Ok(super::ReconcileExecution {
                result: ReconcileResult {
                    observed_paths: 1,
                    imported_sources: 1,
                },
                current_fingerprints: Some(Default::default()),
            })
        }),
        Duration::from_millis(10),
    );
    recovery_runtime.start().expect("recovery start");
    recovery_runtime.shutdown().expect("recovery shutdown");

    let (released, wake) = &*release;
    *released.lock().expect("released") = true;
    wake.notify_all();
    let result = join.join().expect("join");
    assert!(
        result.is_ok(),
        "reconcile worker thread panicked during bounded shutdown test: {result:?}"
    );
    worker_done_rx.recv().expect("worker done recv");
    reap_retained_join_helpers_until_empty_for_test();
}

#[test]
#[serial_test::serial(env)]
fn reconcile_runtime_returns_executor_failures() {
    reap_retained_join_helpers();
    let runtime = ReconcileRuntime::new_for_test(
        Arc::new(|_| Err(AtmError::daemon_unavailable("reconcile failed"))),
        Duration::from_millis(10),
    );
    runtime.start().expect("start");
    let error = runtime.reconcile(request()).expect_err("failure");
    assert!(error.message.contains("reconcile failed"));
    runtime.shutdown().expect("shutdown");
}

#[test]
#[serial_test::serial(env)]
fn reconcile_runtime_cleans_up_pending_waiters_during_shutdown() {
    reap_retained_join_helpers();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let runtime = ReconcileRuntime::new_for_test(
        Arc::new({
            let gate = Arc::clone(&gate);
            move |_| {
                let (released, wake) = &*gate;
                let mut released = released.lock().expect("released");
                while !*released {
                    released = wake.wait(released).expect("wait");
                }
                Ok(super::ReconcileExecution {
                    result: ReconcileResult {
                        observed_paths: 1,
                        imported_sources: 1,
                    },
                    current_fingerprints: Some(Default::default()),
                })
            }
        }),
        Duration::from_millis(250),
    );
    runtime.start().expect("start");

    let runtime_for_thread = runtime.clone();
    let join = std::thread::spawn(move || runtime_for_thread.reconcile(request()));
    runtime.shutdown().expect("shutdown");

    let error = join
        .join()
        .expect("join")
        .expect_err("shutdown interruption");
    assert!(
        error.message.contains("shut down before completion")
            || error.message.contains("unavailable during daemon shutdown")
    );
    if retained_join_helper_count_for_test() > 0 {
        let (released, wake) = &*gate;
        *released.lock().expect("released") = true;
        wake.notify_all();
        reap_retained_join_helpers_until_empty_for_test();
    }
}

#[test]
fn reconcile_runtime_preserves_trigger_order_and_signals_completion() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let runtime = ReconcileRuntime::new_for_test(
        Arc::new({
            let order = Arc::clone(&order);
            let started_tx = started_tx.clone();
            let release = Arc::clone(&release);
            move |request| {
                order
                    .lock()
                    .expect("order")
                    .push(request.agent.as_str().to_string());
                started_tx
                    .send(request.agent.as_str().to_string())
                    .expect("started");
                if request.agent.as_str() == "agent-a" {
                    let (released, wake) = &*release;
                    let mut released = released.lock().expect("released");
                    while !*released {
                        let wait = wake
                            .wait_timeout(released, Duration::from_secs(1))
                            .expect("wait release");
                        released = wait.0;
                        assert!(!wait.1.timed_out(), "agent-a release timed out");
                    }
                }
                Ok(super::ReconcileExecution {
                    result: ReconcileResult {
                        observed_paths: 1,
                        imported_sources: 1,
                    },
                    current_fingerprints: Some(Default::default()),
                })
            }
        }),
        Duration::from_millis(10),
    );
    let command_rx = prepare_started_runtime(&runtime);

    let first = dispatch_reconcile_command_for_test(&runtime, request_for("agent-a"));
    let second = dispatch_reconcile_command_for_test(&runtime, request_for("agent-b"));
    spawn_runtime_worker(&runtime, command_rx);
    assert_eq!(started_rx.recv().expect("first started"), "agent-a");
    let (released, wake) = &*release;
    *released.lock().expect("released") = true;
    wake.notify_all();

    let first_result = first.recv().expect("first join").expect("first result");
    let second_result = second.recv().expect("second join").expect("second result");
    assert_eq!(first_result.observed_paths, 1);
    assert_eq!(second_result.imported_sources, 1);
    assert_eq!(
        order.lock().expect("order").as_slice(),
        ["agent-a".to_string(), "agent-b".to_string()]
    );
    runtime.shutdown().expect("shutdown");
}

#[derive(Clone)]
struct FakeWatchSource;

impl boundary::sealed::Sealed for FakeWatchSource {}

impl WatchEventSource for FakeWatchSource {
    fn poll(
        &self,
        _request: WatchSubscriptionRequest,
    ) -> Result<WatchEventBatch, atm_core::error::AtmError> {
        Ok(WatchEventBatch {
            paths: vec![std::env::temp_dir().join("watch.json")],
        })
    }
}

#[derive(Clone)]
struct FakeInboxIngress {
    imports: Arc<Mutex<Vec<InboxIngressImportResponse>>>,
}

impl FakeInboxIngress {
    fn new(imports: Vec<InboxIngressImportResponse>) -> Self {
        Self {
            imports: Arc::new(Mutex::new(imports)),
        }
    }
}

impl boundary::sealed::Sealed for FakeInboxIngress {}

impl InboxIngress for FakeInboxIngress {
    fn import_inbox_source(
        &self,
        _request: InboxIngressImportRequest,
    ) -> Result<InboxIngressImportResponse, atm_core::error::AtmError> {
        let mut imports = self.imports.lock().expect("imports");
        if imports.is_empty() {
            return Ok(InboxIngressImportResponse {
                source_files: Vec::new(),
            });
        }
        Ok(imports.remove(0))
    }

    fn compute_identity_fingerprint(
        &self,
        request: InboxIngressIdentityFingerprintRequest,
    ) -> InboxIngressIdentityFingerprintResponse {
        InboxIngressIdentityFingerprintResponse {
            fingerprint: request.message.message_id.map(|message_id| {
                atm_core::boundary::MessageFingerprint::from(message_id.to_string())
            }),
        }
    }

    fn report_diagnostics(
        &self,
        _request: InboxIngressDiagnosticsRequest,
    ) -> InboxIngressDiagnosticsResponse {
        InboxIngressDiagnosticsResponse {
            duplicate_message_ids: 0,
            messages_without_ids: 0,
        }
    }
}

#[derive(Clone)]
struct FakeNotificationSink {
    delivered: Arc<Mutex<Vec<NotificationEvent>>>,
}

impl boundary::sealed::Sealed for FakeNotificationSink {}

impl NotificationSink for FakeNotificationSink {
    fn deliver(&self, event: NotificationEvent) -> Result<(), atm_core::error::AtmError> {
        self.delivered.lock().expect("delivered").push(event);
        Ok(())
    }
}

#[derive(Clone, Default)]
struct RecordingRosterStore {
    state: Arc<Mutex<RecordingRosterState>>,
}

#[derive(Default)]
struct RecordingRosterState {
    rosters: HashMap<atm_core::types::TeamName, Vec<boundary::RosterMemberRecord>>,
    replace_count: u64,
}

impl RecordingRosterStore {
    fn replace_count(&self) -> u64 {
        self.state.lock().expect("roster state").replace_count
    }

    fn members_for(&self, team: &atm_core::types::TeamName) -> Vec<boundary::RosterMemberRecord> {
        self.state
            .lock()
            .expect("roster state")
            .rosters
            .get(team)
            .cloned()
            .unwrap_or_default()
    }
}

impl boundary::sealed::Sealed for RecordingRosterStore {}

impl RosterStore for RecordingRosterStore {
    fn replace_roster(
        &self,
        request: RosterStoreReplaceRosterRequest,
    ) -> Result<RosterStoreReplaceRosterResponse, AtmError> {
        let mut state = self.state.lock().expect("roster state");
        let previous_member_count = state
            .rosters
            .get(&request.team)
            .map_or(0, |members| members.len() as u64);
        let current_member_count = request.members.len() as u64;
        state.rosters.insert(request.team.clone(), request.members);
        state.replace_count += 1;
        Ok(RosterStoreReplaceRosterResponse {
            team: request.team,
            previous_member_count,
            current_member_count,
            replaced: true,
        })
    }

    fn load_roster(
        &self,
        request: RosterStoreLoadRosterRequest,
    ) -> Result<RosterStoreLoadRosterResponse, AtmError> {
        Ok(RosterStoreLoadRosterResponse {
            team: request.team.clone(),
            members: self.members_for(&request.team),
        })
    }

    fn query_membership(
        &self,
        request: RosterStoreQueryMembershipRequest,
    ) -> Result<RosterStoreQueryMembershipResponse, AtmError> {
        let member = self
            .members_for(&request.team)
            .into_iter()
            .find(|record| record.agent_name == request.member);
        Ok(RosterStoreQueryMembershipResponse {
            team: request.team,
            is_member: member.is_some(),
            member,
        })
    }

    fn list_teams(
        &self,
        _request: RosterStoreListTeamsRequest,
    ) -> Result<RosterStoreListTeamsResponse, AtmError> {
        Ok(RosterStoreListTeamsResponse {
            teams: self
                .state
                .lock()
                .expect("roster state")
                .rosters
                .keys()
                .cloned()
                .collect(),
        })
    }

    fn health_snapshot(
        &self,
        request: RosterStoreHealthSnapshotRequest,
    ) -> Result<RosterStoreHealthSnapshotResponse, AtmError> {
        Ok(RosterStoreHealthSnapshotResponse {
            snapshot: RosterStoreHealthSnapshot {
                team: request.team.clone(),
                member_count: self.members_for(&request.team).len() as u64,
                stale: false,
                refreshed_at: Some(IsoTimestamp::from_datetime(Utc::now())),
            },
        })
    }
}

#[derive(Clone)]
struct StaticWatchSource {
    batch: WatchEventBatch,
}

impl boundary::sealed::Sealed for StaticWatchSource {}

impl WatchEventSource for StaticWatchSource {
    fn poll(&self, _request: WatchSubscriptionRequest) -> Result<WatchEventBatch, AtmError> {
        Ok(self.batch.clone())
    }
}

fn write_team_config(
    home_dir: &Path,
    team: &atm_core::types::TeamName,
    members: &[&str],
) -> PathBuf {
    let team_dir = atm_core::home::team_dir_from_home(home_dir, team).expect("team dir");
    fs::create_dir_all(&team_dir).expect("create team dir");
    let config_path = team_dir.join("config.json");
    let document = json!({
        "members": members
            .iter()
            .enumerate()
            .map(|(index, member)| {
                if index == 0 {
                    json!({"name": member, "tmuxPaneId": "%1"})
                } else {
                    json!({"name": member})
                }
            })
            .collect::<Vec<_>>(),
    });
    fs::write(
        &config_path,
        serde_json::to_vec_pretty(&document).expect("config bytes"),
    )
    .expect("write team config");
    config_path
}

#[test]
fn z8_watcher_ingest_hydrates_atm_roster_truth_for_new_team() {
    let home_dir = TempDir::new().expect("tempdir");
    let request = ReconcileRequest {
        home_dir: home_dir.path().to_path_buf(),
        team: "test-team".parse().expect("team"),
        agent: "test-agent".parse().expect("agent"),
    };
    let config_path =
        write_team_config(home_dir.path(), &request.team, &[ROLE_TEAM_LEAD, "worker"]);
    let roster_store = RecordingRosterStore::default();
    let runtime = ReconcileRuntime::new(
        Arc::new(StaticWatchSource {
            batch: WatchEventBatch {
                paths: vec![config_path],
            },
        }),
        Arc::new(FakeInboxIngress::new(vec![])),
        Arc::new(roster_store.clone()),
        Arc::new(FakeNotificationSink {
            delivered: Arc::new(Mutex::new(Vec::new())),
        }),
    );
    runtime.start().expect("start");

    let result = runtime.reconcile(request.clone()).expect("reconcile");
    assert_eq!(result.observed_paths, 1);
    assert_eq!(result.imported_sources, 0);
    assert_eq!(roster_store.replace_count(), 1);

    let members = roster_store.members_for(&request.team);
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].team_name, request.team);
    assert_eq!(members[0].agent_name.as_str(), ROLE_TEAM_LEAD);
    assert_eq!(members[0].recipient_pane_id.as_deref(), Some("%1"));

    runtime.shutdown().expect("shutdown");
}

#[test]
fn z8_projection_write_suppression_is_process_local() {
    let home_dir = TempDir::new().expect("tempdir");
    let request = ReconcileRequest {
        home_dir: home_dir.path().to_path_buf(),
        team: "test-team".parse().expect("team"),
        agent: "test-agent".parse().expect("agent"),
    };
    let config_path = write_team_config(home_dir.path(), &request.team, &[ROLE_TEAM_LEAD]);
    let watch_source = StaticWatchSource {
        batch: WatchEventBatch {
            paths: vec![config_path.clone()],
        },
    };
    let roster_store = RecordingRosterStore::default();
    let runtime = ReconcileRuntime::new(
        Arc::new(watch_source.clone()),
        Arc::new(FakeInboxIngress::new(vec![])),
        Arc::new(roster_store.clone()),
        Arc::new(FakeNotificationSink {
            delivered: Arc::new(Mutex::new(Vec::new())),
        }),
    );
    runtime.start().expect("start");
    runtime
        .record_projected_config_write_for_test(&config_path)
        .expect("record projection write");

    runtime
        .reconcile(request.clone())
        .expect("suppressed reconcile");
    assert_eq!(roster_store.replace_count(), 0);

    runtime
        .reconcile(request.clone())
        .expect("second reconcile imports");
    assert_eq!(roster_store.replace_count(), 1);
    runtime.shutdown().expect("shutdown");

    let fresh_store = RecordingRosterStore::default();
    let fresh_runtime = ReconcileRuntime::new(
        Arc::new(watch_source),
        Arc::new(FakeInboxIngress::new(vec![])),
        Arc::new(fresh_store.clone()),
        Arc::new(FakeNotificationSink {
            delivered: Arc::new(Mutex::new(Vec::new())),
        }),
    );
    fresh_runtime.start().expect("fresh start");
    fresh_runtime
        .reconcile(request)
        .expect("restart reconcile imports");
    assert_eq!(fresh_store.replace_count(), 1);
    fresh_runtime.shutdown().expect("fresh shutdown");
}

#[test]
fn z8_deletes_startup_only_config_bootstrap_helper() {
    let boundary_support = include_str!("../../atm-core/src/boundary_support.rs");
    let direct_boundaries = include_str!("../../atm-core/src/direct_boundaries.rs");

    assert!(
        !boundary_support.contains("hydrate_roster_from_team_config_once_at_startup_if_empty"),
        "boundary support must not retain the startup-only config bootstrap helper after Z.8",
    );
    assert!(
        !direct_boundaries.contains("hydrate_roster_from_team_config_once_at_startup_if_empty"),
        "direct boundaries must not forward the startup-only config bootstrap helper after Z.8",
    );
}

#[test]
fn reconcile_runtime_routes_notifications_through_notification_sink_boundary() {
    let delivered = Arc::new(Mutex::new(Vec::new()));
    let ingress = FakeInboxIngress::new(vec![InboxIngressImportResponse {
        source_files: vec![inbox_source_with_message(sample_message(
            "projected message",
        ))],
    }]);
    let runtime = ReconcileRuntime::new(
        Arc::new(FakeWatchSource),
        Arc::new(ingress),
        Arc::new(RecordingRosterStore::default()),
        Arc::new(FakeNotificationSink {
            delivered: Arc::clone(&delivered),
        }),
    );
    runtime.start().expect("start");

    let result = runtime.reconcile(request()).expect("reconcile result");
    assert_eq!(result.observed_paths, 1);
    assert_eq!(result.imported_sources, 1);

    let delivered = delivered.lock().expect("delivered");
    assert_eq!(delivered.len(), 1);
    assert_eq!(
        delivered[0].kind,
        atm_core::protocol::NotificationKind::ReconcileComplete
    );

    runtime.shutdown().expect("shutdown");
}

#[test]
fn reconcile_runtime_projects_worker_liveness_across_start_and_shutdown() {
    let runtime = ReconcileRuntime::new(
        Arc::new(FakeWatchSource),
        Arc::new(FakeInboxIngress::new(vec![])),
        Arc::new(RecordingRosterStore::default()),
        Arc::new(FakeNotificationSink {
            delivered: Arc::new(Mutex::new(Vec::new())),
        }),
    );
    assert_eq!(
        runtime.worker_liveness(),
        super::ReconcileWorkerLiveness::Stopped
    );
    runtime.start().expect("start");
    assert_eq!(
        runtime.worker_liveness(),
        super::ReconcileWorkerLiveness::Live
    );
    runtime.shutdown().expect("shutdown");
    assert_eq!(
        runtime.worker_liveness(),
        super::ReconcileWorkerLiveness::Stopped
    );
}

#[test]
fn reconcile_runtime_actor_notification_fingerprint_registry_is_worker_owned() {
    let delivered = Arc::new(Mutex::new(Vec::new()));
    let repeated_message = sample_message("same logical message");
    let repeated_source = inbox_source_with_message(repeated_message);
    let runtime = ReconcileRuntime::new(
        Arc::new(CountingWatchSource {
            calls: Arc::new(AtomicU64::new(0)),
        }),
        Arc::new(FakeInboxIngress::new(vec![
            InboxIngressImportResponse {
                source_files: vec![repeated_source.clone()],
            },
            InboxIngressImportResponse {
                source_files: vec![repeated_source],
            },
        ])),
        Arc::new(RecordingRosterStore::default()),
        Arc::new(FakeNotificationSink {
            delivered: Arc::clone(&delivered),
        }),
    );
    runtime.start().expect("start");

    let request = request();
    let first = runtime.reconcile(request.clone()).expect("first reconcile");
    let second = runtime.reconcile(request).expect("second reconcile");
    assert_eq!(first.imported_sources, 1);
    assert_eq!(second.imported_sources, 1);

    let delivered = delivered.lock().expect("delivered");
    assert_eq!(delivered.len(), 1);
    assert_eq!(
        delivered[0].kind,
        atm_core::protocol::NotificationKind::ReconcileComplete
    );

    runtime.shutdown().expect("shutdown");
}

#[test]
fn reconcile_runtime_bounds_notification_fingerprint_registry_and_re_emits_after_eviction() {
    let delivered = Arc::new(Mutex::new(Vec::new()));
    let imports = (0..=MAX_RECONCILE_FINGERPRINT_KEYS)
        .map(|index| InboxIngressImportResponse {
            source_files: vec![inbox_source_with_message(sample_message(&format!(
                "message-{index}"
            )))],
        })
        .collect::<Vec<_>>();
    let runtime = ReconcileRuntime::new(
        Arc::new(FakeWatchSource),
        Arc::new(FakeInboxIngress::new(imports)),
        Arc::new(RecordingRosterStore::default()),
        Arc::new(FakeNotificationSink {
            delivered: Arc::clone(&delivered),
        }),
    );
    runtime.start().expect("start");

    let first_request = request_for("agent-0");
    runtime
        .reconcile(first_request.clone())
        .expect("first reconcile");
    for index in 1..=MAX_RECONCILE_FINGERPRINT_KEYS {
        runtime
            .reconcile(request_for(&format!("agent-{index}")))
            .expect("bounded reconcile");
    }
    runtime
        .reconcile(first_request)
        .expect("reconcile after eviction");

    let delivered = delivered.lock().expect("delivered");
    assert_eq!(delivered.len(), MAX_RECONCILE_FINGERPRINT_KEYS + 2);

    runtime.shutdown().expect("shutdown");
}

#[test]
fn reconcile_runtime_bounds_per_key_fingerprint_sets() {
    let delivered = Arc::new(Mutex::new(Vec::new()));
    let repeated_import = InboxIngressImportResponse {
        source_files: (0..=MAX_RECONCILE_FINGERPRINTS_PER_KEY)
            .map(|index| inbox_source_with_message(sample_message(&format!("message-{index}"))))
            .collect(),
    };
    let runtime = ReconcileRuntime::new(
        Arc::new(FakeWatchSource),
        Arc::new(FakeInboxIngress::new(vec![
            repeated_import.clone(),
            repeated_import,
        ])),
        Arc::new(RecordingRosterStore::default()),
        Arc::new(FakeNotificationSink {
            delivered: Arc::clone(&delivered),
        }),
    );
    runtime.start().expect("start");
    let request = request();
    runtime.reconcile(request.clone()).expect("reconcile");
    runtime.reconcile(request).expect("reconcile repeat");
    assert_eq!(delivered.lock().expect("delivered").len(), 1);
    runtime.shutdown().expect("shutdown");
}

#[derive(Clone)]
struct CountingWatchSource {
    calls: Arc<AtomicU64>,
}

impl boundary::sealed::Sealed for CountingWatchSource {}

impl WatchEventSource for CountingWatchSource {
    fn poll(
        &self,
        _request: WatchSubscriptionRequest,
    ) -> Result<WatchEventBatch, atm_core::error::AtmError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(WatchEventBatch {
            paths: vec![std::env::temp_dir().join("watch.json")],
        })
    }
}

fn inbox_source_with_message(
    message: MessageEnvelope,
) -> atm_core::boundary::InboxSourceFileRecord {
    atm_core::boundary::InboxSourceFileRecord {
        path: std::env::temp_dir().join("watch.json"),
        messages: vec![message],
    }
}

fn sample_message(text: &str) -> MessageEnvelope {
    let message_id = AtmMessageId::new();

    MessageEnvelope {
        from: ROLE_TEAM_LEAD.parse().expect("agent"),
        text: text.to_string(),
        timestamp: IsoTimestamp::from_datetime(Utc::now()),
        read: false,
        source_team: Some("test-team".parse().expect("team")),
        summary: Some("summary".to_string()),
        message_id: Some(message_id),
        pending_ack_at: None,
        acknowledged_at: None,
        acknowledges_message_id: None,
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        task_id: None,
        extra: Map::new(),
    }
}
