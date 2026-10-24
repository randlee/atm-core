//! The task-command filter: consults the registered hook chain for each
//! classified task command before the un-hooked body runs (design D-5, D-10, D-f).
//!
//! Registrations are shared `Arc`s, and each hook request is built only when
//! its hook actually runs. A command with no registration never spawns a chain.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, PoisonError, RwLock};
use std::time::Instant;

use async_trait::async_trait;
use atm_core::api::{ApiResponse, AuthenticatedIngress, RequestDeadline};
use atm_core::boundary::{
    DispatchSpec, HookActionDispatcher, HookExitCause, HookName, HookRegistration,
    HookRegistrationStore, HookRunner, PhaseId, TaskAssign, TaskCallCtx, TaskClose, TaskCommand,
    TaskCommands, TaskEvents, TaskList, TaskStart, sealed,
};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::hook::{
    HookAction, HookActionsResponse, HookCaller, HookReply, HookRequest, HookTarget,
};
use atm_core::protocol::{ResponseEnvelope, TaskMoveRequest};
use atm_core::types::{AgentName, TeamName};

use crate::hook_actions::UnhookedRouter;
use crate::hook_chains::ChainTracker;
use crate::router_support::ControlPathSyncBridge;

type HookChains = HashMap<HookName, Arc<[Arc<HookRegistration>]>>;
type Chains = HashMap<TeamName, HookChains>;

static EMPTY_CHAIN: LazyLock<Arc<[Arc<HookRegistration>]>> = LazyLock::new(|| Arc::from([]));

/// The active registrations, grouped by `(team, hook)` in store order.
///
/// Writers replace the whole map, so a poisoned lock is recovered with
/// `into_inner` and never exposes partial state. The read lock is held only to
/// clone the `Arc`, never across an await. `dirty` is set when a refresh after a
/// commit fails; the next `active` reloads before it answers.
pub struct ActiveHookSnapshot {
    map: RwLock<Arc<Chains>>,
    dirty: AtomicBool,
    store: Arc<dyn HookRegistrationStore>,
}

impl ActiveHookSnapshot {
    /// Defers the first durable read until a deadline-aware request needs hook state.
    ///
    /// A failed initial read stays dirty, so the next request retries before using hooks.
    pub fn deferred(store: Arc<dyn HookRegistrationStore>) -> Self {
        Self {
            map: RwLock::new(Arc::new(HashMap::new())),
            dirty: AtomicBool::new(true),
            store,
        }
    }

    pub fn load(store: Arc<dyn HookRegistrationStore>) -> Result<Self, AtmError> {
        let map = build_chains(store.as_ref())?;
        Ok(Self {
            map: RwLock::new(Arc::new(map)),
            dirty: AtomicBool::new(false),
            store,
        })
    }

    /// Replaces the map from the store. A failure marks the snapshot dirty.
    pub fn refresh(&self) -> Result<(), AtmError> {
        match build_chains(self.store.as_ref()) {
            Ok(map) => {
                *self.map.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(map);
                self.dirty.store(false, Ordering::SeqCst);
                Ok(())
            }
            Err(error) => {
                self.dirty.store(true, Ordering::SeqCst);
                tracing::error!(error = %error, "hook snapshot reload failed");
                Err(error)
            }
        }
    }

    /// The active chain for one team and hook. A dirty snapshot reloads first,
    /// as a job on the control-path `bridge`: the reload then serializes with
    /// every registry write, so it can never build from the store before a
    /// commit and install that stale map after the commit's own refresh.
    pub(crate) async fn current(
        self: &Arc<Self>,
        bridge: &ControlPathSyncBridge,
        deadline: RequestDeadline,
        team: &TeamName,
        hook: &HookName,
    ) -> Result<Arc<[Arc<HookRegistration>]>, AtmError> {
        if self.dirty.load(Ordering::SeqCst) {
            let snapshot = Arc::clone(self);
            bridge
                .run(deadline, move || snapshot.reload_if_dirty())
                .await?;
        }
        Ok(self.chain(team, hook))
    }

    /// Reloads only when the dirty flag is still set once the bridge admits
    /// the job, so a reload that a queued write already performed is skipped.
    fn reload_if_dirty(&self) -> Result<(), AtmError> {
        if self.dirty.load(Ordering::SeqCst) {
            self.refresh()
        } else {
            Ok(())
        }
    }

    fn chain(&self, team: &TeamName, hook: &HookName) -> Arc<[Arc<HookRegistration>]> {
        let chains = Arc::clone(&self.map.read().unwrap_or_else(PoisonError::into_inner));
        chains
            .get(team)
            .and_then(|hooks| hooks.get(hook))
            .cloned()
            .unwrap_or_else(|| Arc::clone(&EMPTY_CHAIN))
    }
}

fn build_chains(store: &dyn HookRegistrationStore) -> Result<Chains, AtmError> {
    let mut grouped: HashMap<TeamName, HashMap<HookName, Vec<Arc<HookRegistration>>>> =
        HashMap::new();
    for row in store.active_all()? {
        grouped
            .entry(row.team.clone())
            .or_default()
            .entry(row.hook.clone())
            .or_default()
            .push(Arc::new(row));
    }
    Ok(grouped
        .into_iter()
        .map(|(team, hooks)| {
            let hooks = hooks
                .into_iter()
                .map(|(hook, rows)| (hook, Arc::from(rows)))
                .collect();
            (team, hooks)
        })
        .collect())
}

/// Wraps the un-hooked task commands: every classified local command consults
/// its hook chain first, and only an `Inner` decision runs the inner body.
pub struct HookedTaskCommands<D: HookActionDispatcher = UnhookedRouter> {
    inner: Arc<dyn TaskCommands>,
    actions: Arc<D>,
    snapshot: Arc<ActiveHookSnapshot>,
    runner: Arc<dyn HookRunner>,
    chains: Arc<ChainTracker>,
    bridge: ControlPathSyncBridge,
}

enum Decision {
    Inner,
    Refused(String),
    Done(Box<ApiResponse>),
}

impl<D: HookActionDispatcher> HookedTaskCommands<D> {
    pub(crate) fn new(
        inner: Arc<dyn TaskCommands>,
        actions: Arc<D>,
        snapshot: Arc<ActiveHookSnapshot>,
        runner: Arc<dyn HookRunner>,
        chains: Arc<ChainTracker>,
        bridge: ControlPathSyncBridge,
    ) -> Self {
        Self {
            inner,
            actions,
            snapshot,
            runner,
            chains,
            bridge,
        }
    }

    /// The hook runner this wrapper dispatches chain actions through.
    pub fn runner(&self) -> &Arc<dyn HookRunner> {
        &self.runner
    }

    /// The action dispatcher this wrapper was composed with.
    pub fn actions(&self) -> &Arc<D> {
        &self.actions
    }

    /// The active hook snapshot this wrapper reads its registrations from.
    pub fn snapshot(&self) -> &Arc<ActiveHookSnapshot> {
        &self.snapshot
    }

    /// The chain tracker this wrapper runs its hook chains under.
    pub fn chains(&self) -> &Arc<ChainTracker> {
        &self.chains
    }

    /// The control-path bridge that registry writes and snapshot reloads share.
    pub(crate) fn bridge(&self) -> &ControlPathSyncBridge {
        &self.bridge
    }
}

impl<D: HookActionDispatcher + 'static> HookedTaskCommands<D> {
    async fn filter(&self, cmd: TaskCommand, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError> {
        if ctx.ingress != AuthenticatedIngress::Local {
            return cmd.call(&*self.inner, ctx).await;
        }
        let hook = cmd.hook_name();
        let regs = self
            .snapshot
            .current(&self.bridge, ctx.deadline, &ctx.caller_team, &hook)
            .await?;
        if regs.is_empty() {
            return cmd.call(&*self.inner, ctx).await;
        }
        let run = ChainRun {
            runner: Arc::clone(&self.runner),
            actions: Arc::clone(&self.actions),
            regs,
            caller: HookCaller {
                identity: ctx.caller_identity.clone(),
                team: ctx.caller_team.clone(),
                host: ctx.host.clone(),
            },
            team: ctx.caller_team.clone(),
            who: ctx.caller_identity.clone(),
            deadline: ctx.deadline,
            dry_run: cmd.dry_run(),
            body: cmd.clone(),
            hook,
        };
        let chain = self.chains.spawn(run_chain(run))?;
        match chain.await {
            Err(join) if join.is_panic() => Err(AtmError::hook_exit(HookExitCause::Panic(
                panic_text(join.into_panic()),
            ))),
            Err(join) => Err(AtmError::daemon_unavailable_with_recovery(
                "hook chain was cancelled while the daemon was shutting down",
                "Retry the task after the daemon is serving again; if cancellation repeats, inspect daemon shutdown logs.",
            )
            .with_cause(join)),
            Ok(Err(error)) => Err(error),
            Ok(Ok(Decision::Refused(message))) => Err(AtmError::hook_refused(&message)),
            Ok(Ok(Decision::Done(response))) => Ok(*response),
            // Runs outside the spawned chain, so the inner body never runs in it.
            Ok(Ok(Decision::Inner)) => cmd.call(&*self.inner, ctx).await,
        }
    }
}

/// The values one spawned hook chain owns, cloned out of the calling task.
struct ChainRun<D> {
    runner: Arc<dyn HookRunner>,
    actions: Arc<D>,
    regs: Arc<[Arc<HookRegistration>]>,
    caller: HookCaller,
    team: TeamName,
    who: AgentName,
    hook: HookName,
    body: TaskCommand,
    deadline: RequestDeadline,
    dry_run: bool,
}

/// Runs the registrations of one hook in order until one decides the command.
async fn run_chain<D: HookActionDispatcher>(run: ChainRun<D>) -> Result<Decision, AtmError> {
    for reg in run.regs.iter() {
        let target = HookTarget::from(&**reg);
        let request =
            HookRequest::for_task(&target, run.hook.clone(), run.caller.clone(), &run.body);
        let started = Instant::now();
        let reply = match run.runner.run(&target, &request, run.deadline).await {
            Ok(outcome) => outcome.reply,
            Err(error) => {
                log_run(
                    &run.team,
                    &reg.phase,
                    &run.hook,
                    &run.caller.identity,
                    0,
                    runner_error_outcome(&error),
                    started,
                );
                return Err(error);
            }
        };
        log_run(
            &run.team,
            &reg.phase,
            &run.hook,
            &run.caller.identity,
            reply_action_count(&reply),
            reply_outcome(&reply),
            started,
        );
        match reply {
            HookReply::NotApplicable => continue,
            HookReply::PassThrough => return Ok(Decision::Inner),
            HookReply::Refused { message } => return Ok(Decision::Refused(message)),
            HookReply::Handled(list) => return dispatch_handled(&run, &reg.phase, list).await,
        }
    }
    Ok(Decision::Inner)
}

/// Dispatches one hook's actions and returns the response the caller sees.
async fn dispatch_handled<D: HookActionDispatcher>(
    run: &ChainRun<D>,
    phase: &PhaseId,
    list: Vec<HookAction>,
) -> Result<Decision, AtmError> {
    let n = list.len();
    let spec = DispatchSpec {
        hook: run.hook.clone(),
        team: run.team.clone(),
        phase: phase.clone(),
        default_as: run.who.clone(),
        deadline: Some(run.deadline),
        dry_run: run.dry_run,
    };
    let done = run.actions.dispatch_actions(spec, list).await?;
    let last = done
        .into_iter()
        .rev()
        .find(|dispatched| dispatched.command.task_hook().as_ref() == Some(&run.hook))
        .map(|dispatched| dispatched.response);
    Ok(Decision::Done(Box::new(if run.dry_run {
        hook_actions(0)
    } else {
        last.unwrap_or_else(|| hook_actions(n))
    })))
}

/// One ADR-067 `run` entry per hook call, logged by the chain that made the call.
fn log_run(
    team: &TeamName,
    phase: &PhaseId,
    hook: &HookName,
    caller: &impl std::fmt::Display,
    action_count: usize,
    outcome: &str,
    started: Instant,
) {
    tracing::info!(
        subsystem = "atm_hook",
        action = "run",
        team = %team,
        phase = %phase,
        hook = %hook,
        caller = %caller,
        action_count,
        outcome,
        elapsed_ms = elapsed_ms(started),
    );
}

/// The ADR-067 outcome for a reply the runner returned.
fn reply_outcome(reply: &HookReply) -> &'static str {
    match reply {
        HookReply::NotApplicable => "not_applicable",
        HookReply::PassThrough => "pass_through",
        HookReply::Refused { .. } => "refused",
        HookReply::Handled(_) => "actions",
    }
}

/// The ADR-067 outcome for a runner failure, by its error code.
fn runner_error_outcome(error: &AtmError) -> &'static str {
    match error.code() {
        AtmErrorCode::HookTimeout => "timeout",
        AtmErrorCode::HookMalformed => "malformed",
        _ => "exit",
    }
}

/// The actions a `handled` reply names; every other reply names none.
fn reply_action_count(reply: &HookReply) -> usize {
    match reply {
        HookReply::Handled(list) => list.len(),
        _ => 0,
    }
}

/// Whole milliseconds since `started`, saturating; timing is diagnostic only.
pub(crate) fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn hook_actions(dispatched: usize) -> ApiResponse {
    ApiResponse::new(ResponseEnvelope::HookActions(HookActionsResponse {
        dispatched,
    }))
}

fn panic_text(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_owned())
}

impl<D: HookActionDispatcher + 'static> sealed::Sealed for HookedTaskCommands<D> {}

#[async_trait]
impl<D: HookActionDispatcher + 'static> TaskCommands for HookedTaskCommands<D> {
    async fn assign(&self, req: TaskAssign, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError> {
        self.filter(TaskCommand::Assign(req), ctx).await
    }
    async fn start(&self, req: TaskStart, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError> {
        self.filter(TaskCommand::Start(req), ctx).await
    }
    async fn close(&self, req: TaskClose, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError> {
        self.filter(TaskCommand::Close(req), ctx).await
    }
    async fn move_task(
        &self,
        req: TaskMoveRequest,
        ctx: &TaskCallCtx,
    ) -> Result<ApiResponse, AtmError> {
        self.filter(TaskCommand::Move(req), ctx).await
    }
    async fn list(&self, req: TaskList, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError> {
        self.filter(TaskCommand::List(req), ctx).await
    }
    async fn events(&self, req: TaskEvents, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError> {
        self.filter(TaskCommand::Events(req), ctx).await
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::Arc;

    /// Test-only agent identity; production-like ATM identities are not used in fixtures (RULE-009).
    pub const TEST_AGENT: &str = "test-agent";

    use atm_core::api::{ApiResponse, AuthenticatedIngress, RequestDeadline};
    use atm_core::boundary::HookOutcome;
    use atm_core::boundary::{
        DummyHookRunner, HookName, HookRegistration, HookRegistrationStore, PhaseId,
        RecordingHookActionDispatcher, RecordingTaskCommands, TaskCallCtx,
    };
    use atm_core::error::AtmError;
    use atm_core::hook::HookReply;
    use atm_core::hook::{HookAction, HookActionCommand};
    use atm_core::protocol::{ResponseEnvelope, next_request_id};
    use atm_core::send::{SendMessageSource, WriteRequest};
    use atm_core::types::{AgentName, HostName, IsoTimestamp, TeamName};
    use atm_runtime_test_support::InMemoryHookRegistrationStore;

    use super::{ActiveHookSnapshot, HookedTaskCommands};
    use crate::hook_chains::ChainTracker;
    use crate::router_support::ControlPathSyncBridge;

    pub fn team() -> TeamName {
        "test-team".parse().expect("team")
    }

    pub fn agent(name: &str) -> AgentName {
        name.parse().expect("agent")
    }

    pub fn phase(name: &str) -> PhaseId {
        PhaseId::parse(name).expect("phase")
    }

    pub fn hook(name: &str) -> HookName {
        HookName::parse_task_hook(name).expect("hook")
    }

    pub fn local_host() -> HostName {
        "local-host".parse().expect("host")
    }

    pub fn deadline() -> RequestDeadline {
        RequestDeadline::after(std::time::Duration::from_secs(30))
    }

    /// A control-path bridge with the registry routes' capacity of one.
    pub fn bridge() -> ControlPathSyncBridge {
        ControlPathSyncBridge::new(
            std::num::NonZeroUsize::new(1).expect("one bridge job"),
            crate::RuntimeHealth::default(),
        )
    }

    pub fn ctx(deadline: RequestDeadline) -> TaskCallCtx {
        TaskCallCtx {
            caller_identity: agent(atm_core::roles::ROLE_TEAM_LEAD),
            caller_team: team(),
            host: Some(local_host()),
            ingress: AuthenticatedIngress::Local,
            deadline,
            request_id: next_request_id(),
        }
    }

    /// A task write with a task id and no task op, which classifies as `task.assign`.
    pub fn assign_request(dry_run: bool) -> WriteRequest {
        WriteRequest::new(
            "/home".into(),
            "/work".into(),
            agent(TEST_AGENT),
            "dev@test-team",
            team(),
            SendMessageSource::Inline("body".into()),
            None,
            false,
            Some("bk-2".parse().expect("task id")),
            dry_run,
        )
        .expect("write request")
    }

    pub fn handled_action(command: HookActionCommand, request: &WriteRequest) -> HookAction {
        HookAction {
            command,
            run_as: None,
            request: serde_json::to_value(request).expect("request json"),
        }
    }

    /// A fixed timestamp, so registration order never depends on the clock.
    pub fn at(seconds: u32) -> IsoTimestamp {
        format!("2026-10-19T00:00:{seconds:02}Z")
            .parse()
            .expect("timestamp")
    }

    pub fn registration(hook_name: &str, phase_name: &str, seconds: u32) -> HookRegistration {
        HookRegistration {
            team: team(),
            phase: phase(phase_name),
            hook: hook(hook_name),
            module_path: format!("/modules/{phase_name}.wasm").into(),
            cwd: "/work".into(),
            timeout_ms: 1_000,
            custom: serde_json::Map::new(),
            active: true,
            registered_by: agent(atm_core::roles::ROLE_TEAM_LEAD),
            registered_at: at(seconds),
            disabled_at: None,
        }
    }

    pub fn register(
        store: &InMemoryHookRegistrationStore,
        hook_name: &str,
        phase_name: &str,
        seconds: u32,
    ) {
        store
            .register(registration(hook_name, phase_name, seconds))
            .expect("register");
    }

    pub struct Harness {
        pub inner: Arc<RecordingTaskCommands>,
        pub actions: Arc<RecordingHookActionDispatcher>,
        pub runner: Arc<DummyHookRunner>,
        pub commands: HookedTaskCommands<RecordingHookActionDispatcher>,
    }

    /// Builds the filter over the rows the store holds now; register first.
    pub fn harness(store: &Arc<InMemoryHookRegistrationStore>) -> Harness {
        harness_with(store, RecordingHookActionDispatcher::default())
    }

    /// Like `harness`, with a configured dispatcher (for `fail_at` or `panic_at`).
    pub fn harness_with(
        store: &Arc<InMemoryHookRegistrationStore>,
        dispatcher: RecordingHookActionDispatcher,
    ) -> Harness {
        let inner = Arc::new(RecordingTaskCommands::default());
        *inner.response.lock().expect("response") =
            Some(ApiResponse::new(ResponseEnvelope::RuntimeViewReloaded));
        let actions = Arc::new(dispatcher);
        let runner = Arc::new(DummyHookRunner::default());
        let snapshot = Arc::new(ActiveHookSnapshot::load(store.clone()).expect("snapshot"));
        let commands = HookedTaskCommands::new(
            inner.clone(),
            actions.clone(),
            snapshot.clone(),
            runner.clone(),
            Arc::new(ChainTracker::new()),
            bridge(),
        );
        Harness {
            inner,
            actions,
            runner,
            commands,
        }
    }

    /// Installs a filter over `router` that takes its chain tracker from the
    /// router, with the given snapshot and runner, and returns the router.
    pub fn install_filter(
        router: crate::storage_and_nudge_router::StorageAndNudgeRouter,
        snapshot: Arc<ActiveHookSnapshot>,
        runner: Arc<dyn atm_core::boundary::HookRunner>,
    ) -> crate::storage_and_nudge_router::StorageAndNudgeRouter {
        let filter = HookedTaskCommands::new(
            Arc::new(crate::task_commands::NormalTaskCommands::new(
                router.clone(),
            )),
            Arc::new(router.unhooked()),
            snapshot,
            runner,
            router.hook_chains(),
            router.control_path_sync_bridge.clone(),
        );
        router.with_hook_filter(Arc::new(filter))
    }

    pub fn reply(runner: &DummyHookRunner, reply: HookReply) {
        runner
            .replies
            .lock()
            .expect("replies")
            .push_back(Ok(HookOutcome {
                reply,
                elapsed_ms: 0,
            }));
    }

    pub fn reply_error(runner: &DummyHookRunner, error: AtmError) {
        runner
            .replies
            .lock()
            .expect("replies")
            .push_back(Err(error));
    }

    pub fn targeted_phases(runner: &DummyHookRunner) -> Vec<String> {
        runner
            .calls
            .lock()
            .expect("calls")
            .iter()
            .map(|(target, _, _)| target.phase.as_str().to_owned())
            .collect()
    }

    pub fn expect_err<T>(result: Result<T, AtmError>) -> AtmError {
        match result {
            Err(error) => error,
            Ok(_) => panic!("expected an error"),
        }
    }

    /// Each leaf replaced by its JSON type name: two responses compare equal when
    /// they carry the same fields, whatever the generated ids and timestamps are.
    pub fn shape(value: &serde_json::Value) -> serde_json::Value {
        use serde_json::Value;
        let name = match value {
            Value::Object(map) => {
                return Value::Object(map.iter().map(|(k, v)| (k.clone(), shape(v))).collect());
            }
            Value::Array(items) => {
                return Value::Array(items.iter().map(shape).collect());
            }
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Number(_) => "number",
            Value::String(_) => "string",
        };
        Value::String(name.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::Ordering;

    use atm_core::boundary::{
        DummyHookRunner, HookCall, HookRegistrationStore, RecordingHookActionDispatcher,
        TaskAssign, TaskClose, TaskCommands, TaskEvents, TaskList, TaskStart,
    };
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::hook::{HookActionCommand, HookReply};
    use atm_runtime_test_support::InMemoryHookRegistrationStore;
    use tracing_subscriber::prelude::*;

    use super::ActiveHookSnapshot;
    use super::HookedTaskCommands;
    use super::test_support::{
        agent, assign_request, bridge, ctx, deadline, expect_err, handled_action, harness,
        harness_with, hook, install_filter, local_host, phase, register, registration, reply,
        reply_error, shape, targeted_phases, team,
    };
    use crate::hook_chains::ChainTracker;
    use crate::storage_and_nudge_router::StorageAndNudgeRouter;
    use crate::storage_and_nudge_router::tests::{fixture, task_write_request};
    use crate::task_commands::NormalTaskCommands;

    type CapturedEventFields = Vec<(String, String)>;
    type CapturedEventsInner = Vec<CapturedEventFields>;

    #[derive(Clone, Default)]
    struct CapturedEvents(Arc<Mutex<CapturedEventsInner>>);

    #[derive(Default)]
    struct EventFields(Vec<(String, String)>);

    impl tracing::field::Visit for EventFields {
        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            self.0.push((field.name().to_owned(), value.to_owned()));
        }

        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.0.push((field.name().to_owned(), format!("{value:?}")));
        }
    }

    struct EventCapture(CapturedEvents);

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for EventCapture {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _context: tracing_subscriber::layer::Context<'_, S>,
        ) {
            let mut fields = EventFields::default();
            event.record(&mut fields);
            self.0
                .0
                .lock()
                .expect("captured hook snapshot events")
                .push(fields.0);
        }
    }

    fn list_query() -> atm_core::list::ListQuery {
        atm_core::list::ListQuery {
            home_dir: "/home".into(),
            current_dir: "/work".into(),
            caller_identity: agent(super::test_support::TEST_AGENT),
            caller_team: team(),
            target_address: None,
            selection_mode: atm_core::types::ReadSelection::All,
            seen_state_filter: false,
            limit: None,
            sender_filter: None,
            timestamp_filter: None,
            task_filter: None,
            contains_filter: None,
            task_ledger: None,
        }
    }

    fn start_request() -> atm_core::send::WriteRequest {
        let mut request = assign_request(false);
        request.task_op = Some(atm_core::boundary::TaskOp::Start);
        request
    }

    fn close_request() -> atm_core::send::WriteRequest {
        let mut request = assign_request(false);
        request.task_op = Some(atm_core::boundary::TaskOp::Close {
            outcome: atm_core::boundary::TaskCloseOutcome::Completed,
            reason: None,
        });
        request
    }

    fn move_request() -> atm_core::protocol::TaskMoveRequest {
        atm_core::protocol::TaskMoveRequest {
            caller_identity: agent(atm_core::roles::ROLE_TEAM_LEAD),
            caller_team: team(),
            task_id: "bk-2".parse().expect("task id"),
            target: atm_core::test_support::MoveTarget::End,
        }
    }

    fn events_query() -> atm_core::list::ListQuery {
        atm_core::list::ListQuery {
            task_ledger: Some(atm_core::list::TaskLedgerQuery::Events {
                task_id: "bk-2".parse().expect("task id"),
                member: None,
            }),
            ..list_query()
        }
    }

    /// A filter over a real router: the un-hooked body is `NormalTaskCommands`,
    /// so a pass-through or refusal is judged against the real ledger.
    fn filter_over(
        base: &StorageAndNudgeRouter,
        snapshot: Arc<ActiveHookSnapshot>,
        runner: Arc<DummyHookRunner>,
    ) -> impl TaskCommands {
        HookedTaskCommands::new(
            Arc::new(NormalTaskCommands::new(base.clone())),
            Arc::new(base.unhooked()),
            snapshot,
            runner,
            base.hook_chains(),
            base.control_path_sync_bridge.clone(),
        )
    }

    /// The team's task rows as the store holds them: the ledger a write changes.
    fn ledger(
        store: &(dyn atm_core::test_support::TaskStore + Send + Sync),
    ) -> Vec<atm_core::test_support::TaskRow> {
        store.list_tasks(&team(), None).expect("ledger rows")
    }

    #[test]
    fn accessors_return_the_composed_runner_and_actions() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        let h = harness(&store);
        let runner: Arc<dyn atm_core::boundary::HookRunner> = h.runner.clone();
        assert!(Arc::ptr_eq(h.commands.runner(), &runner));
        assert!(Arc::ptr_eq(h.commands.actions(), &h.actions));
    }

    #[tokio::test]
    async fn unregistered_method_calls_inner_without_spawn() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        let h = harness(&store);
        h.commands
            .assign(
                TaskAssign(Box::new(assign_request(false))),
                &ctx(deadline()),
            )
            .await
            .expect("unhooked assign");
        assert_eq!(h.inner.calls.lock().expect("calls").len(), 1);
        assert!(h.runner.calls.lock().expect("calls").is_empty());
    }

    #[tokio::test]
    async fn no_registration_request_path_makes_no_store_call() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        let h = harness(&store);
        let before = store.calls.load(Ordering::SeqCst);
        h.commands
            .assign(
                TaskAssign(Box::new(assign_request(false))),
                &ctx(deadline()),
            )
            .await
            .expect("assign");
        h.commands
            .list(TaskList(list_query()), &ctx(deadline()))
            .await
            .expect("list");
        h.commands
            .move_task(move_request(), &ctx(deadline()))
            .await
            .expect("move");
        h.commands
            .events(TaskEvents(events_query()), &ctx(deadline()))
            .await
            .expect("events");
        assert_eq!(store.calls.load(Ordering::SeqCst), before);
    }

    #[tokio::test]
    async fn each_method_consults_only_its_own_hook() {
        let names = [
            "task.assign",
            "task.start",
            "task.close",
            "task.move",
            "task.list",
            "task.events",
        ];
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        for name in names {
            register(&store, name, "bk", 1);
        }
        let h = harness(&store);
        for _ in names {
            reply(&h.runner, HookReply::PassThrough);
        }
        h.commands
            .assign(
                TaskAssign(Box::new(assign_request(false))),
                &ctx(deadline()),
            )
            .await
            .expect("assign");
        h.commands
            .start(TaskStart(Box::new(start_request())), &ctx(deadline()))
            .await
            .expect("start");
        h.commands
            .close(TaskClose(Box::new(close_request())), &ctx(deadline()))
            .await
            .expect("close");
        h.commands
            .move_task(move_request(), &ctx(deadline()))
            .await
            .expect("move");
        h.commands
            .list(TaskList(list_query()), &ctx(deadline()))
            .await
            .expect("list");
        h.commands
            .events(TaskEvents(events_query()), &ctx(deadline()))
            .await
            .expect("events");
        let consulted: Vec<String> = h
            .runner
            .calls
            .lock()
            .expect("calls")
            .iter()
            .map(|(_, call, _)| match call {
                HookCall::Run(request) => request.hook.as_str().to_owned(),
                other => panic!("unexpected hook call {other:?}"),
            })
            .collect();
        assert_eq!(consulted, names);
        let inner: Vec<String> = h
            .inner
            .calls
            .lock()
            .expect("calls")
            .iter()
            .map(|(hook, _)| hook.as_str().to_owned())
            .collect();
        assert_eq!(inner, names);
    }

    #[tokio::test]
    async fn dry_run_task_assign_consults_hook() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        register(&store, "task.assign", "bk", 1);
        let h = harness(&store);
        reply(&h.runner, HookReply::PassThrough);
        h.commands
            .assign(TaskAssign(Box::new(assign_request(true))), &ctx(deadline()))
            .await
            .expect("dry run assign");
        assert_eq!(targeted_phases(&h.runner), vec!["bk".to_owned()]);
    }

    #[tokio::test]
    async fn two_phases_each_hook_handles_its_own_task() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        register(&store, "task.assign", "bj", 1);
        register(&store, "task.assign", "bk", 2);
        let h = harness(&store);
        reply(&h.runner, HookReply::NotApplicable);
        let request = assign_request(false);
        reply(
            &h.runner,
            HookReply::Handled(vec![handled_action(
                HookActionCommand::TaskAssign,
                &request,
            )]),
        );
        h.commands
            .assign(TaskAssign(Box::new(request)), &ctx(deadline()))
            .await
            .expect("assign");
        assert_eq!(
            targeted_phases(&h.runner),
            vec!["bj".to_owned(), "bk".to_owned()]
        );
        let calls = h.actions.calls.lock().expect("calls");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].spec.phase.as_str(), "bk");
    }

    #[tokio::test]
    async fn chain_not_applicable_continues_to_next_hook() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        register(&store, "task.assign", "bj", 1);
        register(&store, "task.assign", "bk", 2);
        let h = harness(&store);
        reply(&h.runner, HookReply::NotApplicable);
        reply(&h.runner, HookReply::PassThrough);
        h.commands
            .assign(
                TaskAssign(Box::new(assign_request(false))),
                &ctx(deadline()),
            )
            .await
            .expect("assign");
        assert_eq!(targeted_phases(&h.runner).len(), 2);
        assert_eq!(h.inner.calls.lock().expect("calls").len(), 1);
    }

    #[tokio::test]
    async fn chain_pass_through_stops_and_runs_inner() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        register(&store, "task.assign", "bk", 1);
        let h = harness(&store);
        reply(&h.runner, HookReply::PassThrough);
        h.commands
            .assign(
                TaskAssign(Box::new(assign_request(false))),
                &ctx(deadline()),
            )
            .await
            .expect("assign");
        assert_eq!(targeted_phases(&h.runner).len(), 1);
        assert_eq!(h.inner.calls.lock().expect("calls").len(), 1);
        assert!(h.actions.calls.lock().expect("calls").is_empty());
    }

    /// The router takes its runner, snapshot and chain tracker from the filter it
    /// installs, so no handle can differ from the ones the filter runs through.
    #[tokio::test]
    async fn router_adopts_the_installed_filters_handles() {
        let fixture = crate::storage_and_nudge_router::tests::fixture(true, None, None);
        let store = Arc::new(InMemoryHookRegistrationStore::default());
        let snapshot = Arc::new(ActiveHookSnapshot::load(store.clone()).expect("snapshot"));
        let runner = Arc::new(atm_core::boundary::DummyHookRunner::default());
        let filter = Arc::new(HookedTaskCommands::new(
            Arc::new(crate::task_commands::NormalTaskCommands::new(
                fixture.router.clone(),
            )),
            Arc::new(fixture.router.unhooked()),
            snapshot.clone(),
            runner.clone(),
            Arc::new(ChainTracker::new()),
            fixture.router.control_path_sync_bridge.clone(),
        ));
        let router = fixture.router.clone().with_hook_filter(filter.clone());
        assert!(Arc::ptr_eq(&router.hook_chains(), filter.chains()));
        assert!(Arc::ptr_eq(
            router.hooks.snapshot.as_ref().expect("snapshot"),
            &snapshot
        ));
        assert!(Arc::ptr_eq(
            router.hooks.runner.as_ref().expect("runner"),
            filter.runner()
        ));
    }

    #[tokio::test]
    async fn chain_handled_stops_and_runs_actions() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        register(&store, "task.assign", "bk", 1);
        let h = harness(&store);
        let request = assign_request(false);
        reply(
            &h.runner,
            HookReply::Handled(vec![handled_action(
                HookActionCommand::TaskAssign,
                &request,
            )]),
        );
        h.commands
            .assign(TaskAssign(Box::new(request)), &ctx(deadline()))
            .await
            .expect("assign");
        assert_eq!(h.actions.calls.lock().expect("calls").len(), 1);
        assert!(h.inner.calls.lock().expect("calls").is_empty());
    }

    #[tokio::test]
    async fn all_not_applicable_runs_inner() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        register(&store, "task.assign", "bj", 1);
        register(&store, "task.assign", "bk", 2);
        let h = harness(&store);
        reply(&h.runner, HookReply::NotApplicable);
        reply(&h.runner, HookReply::NotApplicable);
        h.commands
            .assign(
                TaskAssign(Box::new(assign_request(false))),
                &ctx(deadline()),
            )
            .await
            .expect("assign");
        assert_eq!(h.inner.calls.lock().expect("calls").len(), 1);
        assert!(h.actions.calls.lock().expect("calls").is_empty());
    }

    #[tokio::test]
    async fn chain_not_applicable_then_refused_skips_inner() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        register(&store, "task.assign", "bj", 1);
        register(&store, "task.assign", "bk", 2);
        let h = harness(&store);
        reply(&h.runner, HookReply::NotApplicable);
        reply(
            &h.runner,
            HookReply::Refused {
                message: "bd not ready".into(),
            },
        );
        let error = expect_err(
            h.commands
                .assign(
                    TaskAssign(Box::new(assign_request(false))),
                    &ctx(deadline()),
                )
                .await,
        );
        assert_eq!(error.code(), AtmErrorCode::HookRefused);
        assert!(error.message().contains("bd not ready"));
        assert!(h.inner.calls.lock().expect("calls").is_empty());
    }

    #[tokio::test]
    async fn first_handler_wins_and_later_hooks_are_not_run() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        register(&store, "task.assign", "bj", 1);
        register(&store, "task.assign", "bk", 2);
        let h = harness(&store);
        let request = assign_request(false);
        reply(
            &h.runner,
            HookReply::Handled(vec![handled_action(
                HookActionCommand::TaskAssign,
                &request,
            )]),
        );
        reply(&h.runner, HookReply::PassThrough);
        h.commands
            .assign(TaskAssign(Box::new(request)), &ctx(deadline()))
            .await
            .expect("assign");
        assert_eq!(targeted_phases(&h.runner), vec!["bj".to_owned()]);
        assert_eq!(h.actions.calls.lock().expect("calls").len(), 1);
        assert!(h.inner.calls.lock().expect("calls").is_empty());
    }

    #[tokio::test]
    async fn runner_error_returns_unchanged_and_stops_the_chain() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        register(&store, "task.assign", "bi", 1);
        register(&store, "task.assign", "bj", 2);
        register(&store, "task.assign", "bk", 3);
        let h = harness(&store);
        reply(&h.runner, HookReply::NotApplicable);
        reply_error(
            &h.runner,
            AtmError::hook_malformed("runner stdout was not JSON"),
        );
        reply(&h.runner, HookReply::PassThrough);
        let ctx = ctx(deadline());
        let error = expect_err(
            h.commands
                .assign(TaskAssign(Box::new(assign_request(false))), &ctx)
                .await,
        );
        assert_eq!(error.code(), AtmErrorCode::HookMalformed);
        assert!(error.message().contains("runner stdout was not JSON"));
        let calls = h.runner.calls.lock().expect("calls");
        assert_eq!(calls.len(), 2);
        assert!(
            calls
                .iter()
                .all(|(_, _, call_deadline)| *call_deadline == ctx.deadline)
        );
        assert!(h.inner.calls.lock().expect("calls").is_empty());
    }

    #[tokio::test]
    async fn requests_are_built_only_for_hooks_that_run() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        register(&store, "task.assign", "bj", 1);
        register(&store, "task.assign", "bk", 2);
        register(&store, "task.assign", "bl", 3);
        let h = harness(&store);
        let request = assign_request(false);
        reply(
            &h.runner,
            HookReply::Handled(vec![handled_action(
                HookActionCommand::TaskAssign,
                &request,
            )]),
        );
        h.commands
            .assign(TaskAssign(Box::new(request)), &ctx(deadline()))
            .await
            .expect("assign");
        let phases = targeted_phases(&h.runner);
        assert_eq!(phases, vec!["bj".to_owned()]);
        assert!(!phases.iter().any(|phase| phase == "bl"));
    }

    #[tokio::test]
    async fn handled_passes_the_dispatch_spec() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        register(&store, "task.assign", "bk", 1);
        let h = harness(&store);
        let request = assign_request(false);
        reply(
            &h.runner,
            HookReply::Handled(vec![handled_action(
                HookActionCommand::TaskAssign,
                &request,
            )]),
        );
        let ctx = ctx(deadline());
        h.commands
            .assign(TaskAssign(Box::new(request)), &ctx)
            .await
            .expect("assign");
        let calls = h.actions.calls.lock().expect("calls");
        assert_eq!(calls.len(), 1);
        let spec = &calls[0].spec;
        assert_eq!(spec.deadline, Some(ctx.deadline));
        assert!(!spec.dry_run);
        assert_eq!(spec.phase.as_str(), "bk");
        assert_eq!(spec.hook.as_str(), "task.assign");
        assert_eq!(spec.default_as, agent(atm_core::roles::ROLE_TEAM_LEAD));
    }

    #[tokio::test]
    async fn refused_hook_writes_nothing_and_returns_hook_refused() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        register(&store, "task.assign", "bk", 1);
        let h = harness(&store);
        reply(
            &h.runner,
            HookReply::Refused {
                message: "bd not ready".into(),
            },
        );
        let error = expect_err(
            h.commands
                .assign(
                    TaskAssign(Box::new(assign_request(false))),
                    &ctx(deadline()),
                )
                .await,
        );
        assert_eq!(error.code(), AtmErrorCode::HookRefused);
        assert!(error.message().contains("bd not ready"));
        assert!(h.actions.calls.lock().expect("calls").is_empty());
        assert!(h.inner.calls.lock().expect("calls").is_empty());
    }

    #[tokio::test]
    async fn refused_task_write_leaves_the_real_ledger_unchanged() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        register(&store, "task.assign", "bk", 1);
        let snapshot = Arc::new(ActiveHookSnapshot::load(store.clone()).expect("snapshot"));
        let fx = fixture(true, None, None);
        let base = fx.router.clone();
        NormalTaskCommands::new(base.clone())
            .assign(
                TaskAssign(Box::new(task_write_request(&fx, "T-SEED"))),
                &ctx(deadline()),
            )
            .await
            .expect("seed the ledger without a hook");
        let before = ledger(fx.task_store.as_ref());
        assert_eq!(
            before.len(),
            1,
            "the seeded task is in the ledger before the refusal"
        );
        let runner = Arc::new(DummyHookRunner::default());
        reply(
            &runner,
            HookReply::Refused {
                message: "bd not ready".into(),
            },
        );
        let filter = filter_over(&base, snapshot, runner);
        let error = expect_err(
            filter
                .assign(
                    TaskAssign(Box::new(task_write_request(&fx, "T-REFUSED"))),
                    &ctx(deadline()),
                )
                .await,
        );
        assert_eq!(error.code(), AtmErrorCode::HookRefused);
        assert!(error.message().contains("bd not ready"));
        assert_eq!(
            ledger(fx.task_store.as_ref()),
            before,
            "a refused write leaves the task ledger as it was"
        );
    }

    #[tokio::test]
    async fn hook_replaces_the_call_before_any_write() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        register(&store, "task.assign", "bk", 1);
        let h = harness(&store);
        reply(
            &h.runner,
            HookReply::Refused {
                message: "bd not ready".into(),
            },
        );
        let _ = h
            .commands
            .assign(
                TaskAssign(Box::new(assign_request(false))),
                &ctx(deadline()),
            )
            .await;
        assert!(h.inner.calls.lock().expect("calls").is_empty());
    }

    #[tokio::test]
    async fn pass_through_row_equals_unhooked_row() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        register(&store, "task.assign", "bk", 1);
        let snapshot = Arc::new(ActiveHookSnapshot::load(store.clone()).expect("snapshot"));

        let unhooked = fixture(true, None, None);
        let unhooked_response = NormalTaskCommands::new(unhooked.router.clone())
            .assign(
                TaskAssign(Box::new(task_write_request(&unhooked, "T-PASS"))),
                &ctx(deadline()),
            )
            .await
            .expect("unhooked assign");

        let hooked = fixture(true, None, None);
        let runner = Arc::new(DummyHookRunner::default());
        reply(&runner, HookReply::PassThrough);
        let filter = filter_over(&hooked.router, snapshot, runner.clone());
        let hooked_response = filter
            .assign(
                TaskAssign(Box::new(task_write_request(&hooked, "T-PASS"))),
                &ctx(deadline()),
            )
            .await
            .expect("pass-through assign");

        assert_eq!(runner.calls.lock().expect("calls").len(), 1);
        let unhooked_shape =
            shape(&serde_json::to_value(unhooked_response.into_inner()).expect("json"));
        let hooked_shape =
            shape(&serde_json::to_value(hooked_response.into_inner()).expect("json"));
        assert_eq!(hooked_shape, unhooked_shape);
    }

    #[tokio::test]
    async fn malformed_reply_error_never_calls_inner() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        register(&store, "task.assign", "bk", 1);
        let h = harness(&store);
        reply_error(&h.runner, AtmError::hook_malformed("reply is not JSON"));
        let error = expect_err(
            h.commands
                .assign(
                    TaskAssign(Box::new(assign_request(false))),
                    &ctx(deadline()),
                )
                .await,
        );
        assert_eq!(error.code(), AtmErrorCode::HookMalformed);
        assert!(h.inner.calls.lock().expect("calls").is_empty());
    }

    #[tokio::test]
    async fn runner_receives_request_body_and_the_registration_custom() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        let mut row = registration("task.assign", "bk", 1);
        row.custom = serde_json::json!({"goal": "ship"})
            .as_object()
            .expect("object")
            .clone();
        store.register(row).expect("register");
        let h = harness(&store);
        reply(&h.runner, HookReply::PassThrough);
        let write = assign_request(false);
        h.commands
            .assign(TaskAssign(Box::new(write.clone())), &ctx(deadline()))
            .await
            .expect("assign");
        let calls = h.runner.calls.lock().expect("calls");
        match &calls[0].1 {
            atm_core::boundary::HookCall::Run(request) => {
                let value = serde_json::to_value(request).expect("request json");
                let keys: std::collections::BTreeSet<&str> = value
                    .as_object()
                    .expect("request object")
                    .keys()
                    .map(String::as_str)
                    .collect();
                assert_eq!(
                    keys,
                    std::collections::BTreeSet::from([
                        "caller",
                        "custom",
                        "hook",
                        "phase",
                        "request",
                        "schema_version",
                        "team",
                    ])
                );
                assert_eq!(value["hook"], serde_json::json!("task.assign"));
                assert_eq!(value["team"], serde_json::json!("test-team"));
                assert_eq!(value["phase"], serde_json::json!("bk"));
                assert_eq!(request.custom.get("goal"), Some(&serde_json::json!("ship")));
                assert_eq!(request.caller.host, Some(local_host()));
                assert_eq!(request.request, serde_json::to_value(&write).expect("json"));
            }
            other => panic!("unexpected hook call {other:?}"),
        }
    }

    #[tokio::test]
    async fn refresh_failure_marks_dirty_and_the_next_call_reloads() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        register(&store, "task.assign", "bj", 1);
        let snapshot = Arc::new(ActiveHookSnapshot::load(store.clone()).expect("snapshot"));
        store
            .register(registration("task.assign", "bk", 2))
            .expect("register");
        store.fail_active_all.store(true, Ordering::SeqCst);
        let events = CapturedEvents::default();
        let subscriber = tracing_subscriber::registry().with(EventCapture(events.clone()));
        let result = tracing::subscriber::with_default(subscriber, || snapshot.refresh());
        let error = result.expect_err("injected refresh failure");
        let captured = {
            events
                .0
                .lock()
                .expect("captured hook snapshot events")
                .clone()
        };
        assert_eq!(captured.len(), 1);
        assert!(
            captured[0].iter().any(|(name, value)| {
                name == "message" && value == "hook snapshot reload failed"
            })
        );
        assert!(captured[0].iter().any(|(name, value)| {
            name == "error" && value.contains("injected hook registration active_all failure")
        }));
        assert!(
            error
                .message()
                .contains("injected hook registration active_all failure")
        );
        store.fail_active_all.store(false, Ordering::SeqCst);
        let rows = snapshot
            .current(&bridge(), deadline(), &team(), &hook("task.assign"))
            .await
            .expect("reloaded active");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].phase, phase("bk"));
    }

    #[test]
    fn missing_chain_uses_shared_empty_slice() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        let snapshot = ActiveHookSnapshot::load(store).expect("snapshot");

        let first = snapshot.chain(&team(), &hook("task.assign"));
        let second = snapshot.chain(&team(), &hook("task.assign"));

        assert!(first.is_empty());
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[tokio::test]
    async fn runner_task_panic_returns_hook_exit_and_daemon_keeps_serving() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        register(&store, "task.assign", "bk", 1);
        let h = harness(&store);
        h.runner.panic_on_run.store(true, Ordering::SeqCst);
        let error = expect_err(
            h.commands
                .assign(
                    TaskAssign(Box::new(assign_request(false))),
                    &ctx(deadline()),
                )
                .await,
        );
        assert_eq!(error.code(), AtmErrorCode::HookExit);
        assert!(error.message().contains("hook runner panicked: "));
        h.runner.panic_on_run.store(false, Ordering::SeqCst);
        h.commands
            .list(TaskList(list_query()), &ctx(deadline()))
            .await
            .expect("list keeps serving after the panic");
    }

    #[tokio::test]
    async fn panic_in_action_dispatch_returns_hook_exit() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        register(&store, "task.assign", "bk", 1);
        let dispatcher = RecordingHookActionDispatcher {
            panic_at: Some(0),
            ..Default::default()
        };
        let h = harness_with(&store, dispatcher);
        let request = assign_request(false);
        reply(
            &h.runner,
            HookReply::Handled(vec![handled_action(
                HookActionCommand::TaskAssign,
                &request,
            )]),
        );
        let error = expect_err(
            h.commands
                .assign(TaskAssign(Box::new(request)), &ctx(deadline()))
                .await,
        );
        assert_eq!(error.code(), AtmErrorCode::HookExit);
    }

    #[test]
    fn task_history_never_consults_a_hook() {
        // Only the Tasks and Events views classify as task commands; history stays
        // on the plain message path, so no task hook is ever consulted for it.
        let mut query = list_query();
        query.task_ledger = Some(atm_core::list::TaskLedgerQuery::History {
            member: None,
            limit: 5,
        });
        let history = atm_core::api::ApiRequest::Messages(Box::new(
            atm_core::api::MessageCollectionRequest::List(query),
        ));
        assert!(
            atm_core::boundary::TaskCommand::classify(history).is_err(),
            "task history is not a hookable task command"
        );
    }

    #[tokio::test]
    async fn internal_list_messages_caller_does_not_consult() {
        // The router's own list path is the un-hooked body; a hook registered for
        // task.list is never consulted by it, even with the filter's state installed.
        let fixture = crate::storage_and_nudge_router::tests::fixture(true, None, None);
        let store = Arc::new(InMemoryHookRegistrationStore::default());
        register(&store, "task.list", "bk", 1);
        let runner = Arc::new(atm_core::boundary::DummyHookRunner::default());
        let snapshot = Arc::new(ActiveHookSnapshot::load(store.clone()).expect("snapshot"));
        let router = install_filter(fixture.router.clone(), snapshot, runner.clone());
        let mut query = list_query();
        query.caller_identity = agent("sender");
        query.caller_team = team();
        query.task_ledger = Some(atm_core::list::TaskLedgerQuery::Tasks { member: None });

        router
            .list_messages(query, deadline())
            .await
            .expect("internal list");

        assert!(
            runner.calls.lock().expect("calls").is_empty(),
            "the internal list path never runs a hook"
        );
    }
}
