//! The task-command filter: consults the registered hook chain for each
//! classified task command before the un-hooked body runs (design D-5, D-10, D-f).
//!
//! Registrations are shared `Arc`s, and each hook request is built only when
//! its hook actually runs. A command with no registration never spawns a chain.

// Built ahead of its composition consumer (bk-5 installs the filter at daemon
// startup); this allow lifts with that install.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError, RwLock};
use std::time::Instant;

use async_trait::async_trait;
use atm_core::api::{ApiResponse, AuthenticatedIngress, RequestDeadline};
use atm_core::boundary::{
    DispatchSpec, HookActionDispatcher, HookExitCause, HookName, HookRegistration,
    HookRegistrationStore, HookRunner, PhaseId, TaskAssign, TaskCallCtx, TaskClose, TaskCommand,
    TaskCommands, TaskEvents, TaskList, TaskStart, sealed,
};
use atm_core::error::AtmError;
use atm_core::hook::{
    HookAction, HookActionsResponse, HookCaller, HookReply, HookRequest, HookTarget,
};
use atm_core::protocol::{ResponseEnvelope, TaskMoveRequest};
use atm_core::types::{AgentName, TeamName};

use crate::hook_actions::UnhookedRouter;
use crate::hook_chains::ChainTracker;

type Chains = HashMap<(TeamName, HookName), Arc<[Arc<HookRegistration>]>>;

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
                Err(error)
            }
        }
    }

    /// The active chain for one team and hook; reloads first when dirty.
    pub fn active(
        &self,
        team: &TeamName,
        hook: &HookName,
    ) -> Result<Arc<[Arc<HookRegistration>]>, AtmError> {
        if self.dirty.load(Ordering::SeqCst) {
            self.refresh()?;
        }
        let chains = Arc::clone(&self.map.read().unwrap_or_else(PoisonError::into_inner));
        Ok(chains
            .get(&(team.clone(), hook.clone()))
            .cloned()
            .unwrap_or_else(|| Arc::from(Vec::new())))
    }
}

fn build_chains(store: &dyn HookRegistrationStore) -> Result<Chains, AtmError> {
    let mut grouped: HashMap<(TeamName, HookName), Vec<Arc<HookRegistration>>> = HashMap::new();
    for row in store.active_all()? {
        grouped
            .entry((row.team.clone(), row.hook.clone()))
            .or_default()
            .push(Arc::new(row));
    }
    Ok(grouped
        .into_iter()
        .map(|(key, rows)| (key, Arc::from(rows)))
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
}

enum Decision {
    Inner,
    Refused(String),
    Done(Box<ApiResponse>),
}

impl<D: HookActionDispatcher> HookedTaskCommands<D> {
    pub fn new(
        inner: Arc<dyn TaskCommands>,
        actions: Arc<D>,
        snapshot: Arc<ActiveHookSnapshot>,
        runner: Arc<dyn HookRunner>,
        chains: Arc<ChainTracker>,
    ) -> Self {
        Self {
            inner,
            actions,
            snapshot,
            runner,
            chains,
        }
    }
}

impl<D: HookActionDispatcher + 'static> HookedTaskCommands<D> {
    async fn filter(&self, cmd: TaskCommand, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError> {
        if ctx.ingress != AuthenticatedIngress::Local {
            return cmd.call(&*self.inner, ctx).await;
        }
        let hook = cmd.hook_name();
        let regs = self.snapshot.active(&ctx.caller_team, &hook)?;
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
            dry_run: dry_run_of(&cmd),
            body: cmd.clone(),
            hook,
        };
        let chain = self.chains.spawn(run_chain(run))?;
        match chain.await {
            Err(join) if join.is_panic() => Err(AtmError::hook_exit(HookExitCause::Panic(
                panic_text(join.into_panic()),
            ))),
            Err(_) => Err(AtmError::daemon_unavailable(
                "hook chain cancelled: daemon shutting down",
            )),
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
                log_run(&run.team, &reg.phase, &run.hook, "error", started);
                return Err(error);
            }
        };
        log_run(
            &run.team,
            &reg.phase,
            &run.hook,
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
        .find(|(command, _)| command.as_str() == run.hook.as_str())
        .map(|(_, response)| response);
    Ok(Decision::Done(Box::new(if run.dry_run {
        hook_actions(0)
    } else {
        last.unwrap_or_else(|| hook_actions(n))
    })))
}

/// One ADR-067 `run` entry per hook call, logged by the chain that made the call.
fn log_run(team: &TeamName, phase: &PhaseId, hook: &HookName, outcome: &str, started: Instant) {
    tracing::info!(
        subsystem = "atm_hook",
        action = "run",
        team = %team,
        phase = %phase,
        hook = %hook,
        outcome,
        elapsed_ms = elapsed_ms(started),
    );
}

fn reply_outcome(reply: &HookReply) -> &'static str {
    match reply {
        HookReply::NotApplicable => "not_applicable",
        HookReply::PassThrough => "pass_through",
        HookReply::Refused { .. } => "refused",
        HookReply::Handled(_) => "handled",
    }
}

/// Whole milliseconds since `started`, saturating; timing is diagnostic only.
pub(crate) fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// The write's own `dry_run`; move, list and events are never dry runs.
fn dry_run_of(cmd: &TaskCommand) -> bool {
    match cmd {
        TaskCommand::Assign(TaskAssign(request))
        | TaskCommand::Start(TaskStart(request))
        | TaskCommand::Close(TaskClose(request)) => request.dry_run,
        TaskCommand::Move(_) | TaskCommand::List(_) | TaskCommand::Events(_) => false,
    }
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

    use atm_core::api::{ApiResponse, AuthenticatedIngress, RequestDeadline};
    use atm_core::boundary::HookOutcome;
    use atm_core::boundary::{
        DummyHookRunner, HookName, HookRegistration, HookRegistrationStore, PhaseId,
        RecordingHookActionDispatcher, RecordingTaskCommands, TaskCallCtx,
    };
    use atm_core::error::AtmError;
    use atm_core::hook::HookAction;
    use atm_core::hook::HookReply;
    use atm_core::protocol::{ResponseEnvelope, next_request_id};
    use atm_core::send::{SendMessageSource, WriteRequest};
    use atm_core::types::{AgentName, HostName, IsoTimestamp, TeamName};
    use atm_runtime_test_support::InMemoryHookRegistrationStore;

    use super::{ActiveHookSnapshot, HookedTaskCommands};
    use crate::hook_chains::ChainTracker;

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
            agent("solar"),
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

    pub fn handled_action(command: &str, request: &WriteRequest) -> HookAction {
        HookAction {
            command: command.to_owned(),
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
        );
        Harness {
            inner,
            actions,
            runner,
            commands,
        }
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
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    use atm_core::boundary::{
        HookRegistrationStore, RecordingHookActionDispatcher, TaskAssign, TaskClose, TaskCommands,
        TaskList,
    };
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::hook::HookReply;
    use atm_core::protocol::ResponseEnvelope;
    use atm_runtime_test_support::InMemoryHookRegistrationStore;

    use super::ActiveHookSnapshot;
    use super::test_support::{
        agent, assign_request, ctx, deadline, expect_err, handled_action, harness, harness_with,
        hook, local_host, phase, register, registration, reply, reply_error, targeted_phases, team,
    };

    fn list_query() -> atm_core::list::ListQuery {
        atm_core::list::ListQuery {
            home_dir: "/home".into(),
            current_dir: "/work".into(),
            caller_identity: agent("solar"),
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

    fn close_request() -> atm_core::send::WriteRequest {
        let mut request = assign_request(false);
        request.task_op = Some(atm_core::boundary::TaskOp::Start);
        request
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
        assert_eq!(store.calls.load(Ordering::SeqCst), before);
    }

    #[tokio::test]
    async fn each_method_consults_only_its_own_hook() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        register(&store, "task.close", "bk", 1);
        let h = harness(&store);
        reply(&h.runner, HookReply::PassThrough);
        h.commands
            .assign(
                TaskAssign(Box::new(assign_request(false))),
                &ctx(deadline()),
            )
            .await
            .expect("assign");
        assert!(h.runner.calls.lock().expect("calls").is_empty());
        h.commands
            .close(TaskClose(Box::new(close_request())), &ctx(deadline()))
            .await
            .expect("close");
        let calls = h.runner.calls.lock().expect("calls");
        assert_eq!(calls.len(), 1);
        match &calls[0].1 {
            atm_core::boundary::HookCall::Run(request) => {
                assert_eq!(request.hook.as_str(), "task.close");
            }
            other => panic!("unexpected hook call {other:?}"),
        }
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
            HookReply::Handled(vec![handled_action("task.assign", &request)]),
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

    #[tokio::test]
    async fn chain_handled_stops_and_runs_actions() {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        register(&store, "task.assign", "bk", 1);
        let h = harness(&store);
        let request = assign_request(false);
        reply(
            &h.runner,
            HookReply::Handled(vec![handled_action("task.assign", &request)]),
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
            HookReply::Handled(vec![handled_action("task.assign", &request)]),
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
            HookReply::Handled(vec![handled_action("task.assign", &request)]),
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
            HookReply::Handled(vec![handled_action("task.assign", &request)]),
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
        assert!(h.actions.calls.lock().expect("calls").is_empty());
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
        let h = harness(&store);
        reply(&h.runner, HookReply::PassThrough);
        let response = h
            .commands
            .assign(
                TaskAssign(Box::new(assign_request(false))),
                &ctx(deadline()),
            )
            .await
            .expect("assign");
        assert!(matches!(
            response.into_inner(),
            ResponseEnvelope::RuntimeViewReloaded
        ));
        assert_eq!(h.inner.calls.lock().expect("calls").len(), 1);
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
        let snapshot = ActiveHookSnapshot::load(store.clone()).expect("snapshot");
        store
            .register(registration("task.assign", "bk", 2))
            .expect("register");
        store.fail_active_all.store(true, Ordering::SeqCst);
        assert!(snapshot.refresh().is_err());
        store.fail_active_all.store(false, Ordering::SeqCst);
        let rows = snapshot
            .active(&team(), &hook("task.assign"))
            .expect("reloaded active");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].phase, phase("bk"));
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
            HookReply::Handled(vec![handled_action("task.assign", &request)]),
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
        let router = fixture
            .router
            .clone()
            .with_hook_snapshot(snapshot)
            .with_hook_runner(runner.clone());
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
