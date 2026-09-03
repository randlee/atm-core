//! Daemon-owned reuse for authenticated outbound peer HTTP/1 connections.
//!
//! This module is deliberately TLS-opaque. It accepts only the established
//! stream supplied by [`crate::PeerStreamAdapter`] and retains completed
//! HTTP/1 handshakes, never raw pre-handshake streams.

use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::num::NonZeroU16;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use atm_core::api::{HttpRequest, RequestDeadline};
use atm_core::types::HostName;
use axum::body::Body;
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::PeerStreamAdapter;
use crate::client::{
    HttpRuntimeClientFailure, direct_peer_authority, direct_peer_connection_failure,
    execute_opaque_peer_request_with_sender, peer_connect_deadline_failure,
};

const DRIVER_ABORT_GRACE: Duration = Duration::from_millis(100);

/// Fixed operational bounds for daemon-owned authenticated peer connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerPoolConfig {
    pub max_per_peer: usize,
    pub max_pooled_total: usize,
    pub idle_timeout: Duration,
}

impl Default for PeerPoolConfig {
    fn default() -> Self {
        Self {
            max_per_peer: 4,
            max_pooled_total: 32,
            idle_timeout: Duration::from_secs(60),
        }
    }
}

impl PeerPoolConfig {
    /// Validates that every pool bound is usable before runtime startup.
    pub fn validate(self) -> Result<Self, atm_core::error::AtmError> {
        if self.max_per_peer == 0 {
            return Err(atm_core::error::AtmError::config(
                "peer pool max_per_peer must be greater than zero",
            ));
        }
        if self.max_pooled_total == 0 {
            return Err(atm_core::error::AtmError::config(
                "peer pool max_pooled_total must be greater than zero",
            ));
        }
        if self.idle_timeout.is_zero() {
            return Err(atm_core::error::AtmError::config(
                "peer pool idle_timeout must be greater than zero",
            ));
        }
        Ok(self)
    }
}

/// Bounded pool keyed by configured peer authority, never a resolved IP.
#[derive(Clone)]
pub struct PeerConnectionPool {
    shared: Arc<PoolShared>,
}

struct PoolShared {
    config: PeerPoolConfig,
    adapter: Arc<dyn PeerStreamAdapter>,
    state: Mutex<PoolState>,
    addresses: PeerAddressCache,
    eviction_task: Mutex<Option<JoinHandle<()>>>,
    #[cfg(test)]
    completed_drivers: Arc<AtomicUsize>,
}

#[derive(Default)]
struct PoolState {
    idle: HashMap<HostName, Vec<IdlePeerConnection>>,
    reservations_by_peer: HashMap<HostName, usize>,
    pooled_total: usize,
}

struct IdlePeerConnection {
    sender: http1::SendRequest<Body>,
    driver: JoinHandle<()>,
    idle_since: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConnectionOrigin {
    Pooled,
    Overflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GuardHealth {
    Unused,
    Healthy,
    Failed,
}

/// RAII lease. Callers can issue one request through the lease but never gain
/// direct access to the sender, so a failed exchange cannot return to idle.
pub(crate) struct PooledPeerConnection {
    peer: HostName,
    authority: String,
    origin: ConnectionOrigin,
    sender: Option<http1::SendRequest<Body>>,
    driver: Option<JoinHandle<()>>,
    health: GuardHealth,
    pool: Weak<PoolShared>,
}

impl PeerConnectionPool {
    #[must_use]
    pub fn new(config: PeerPoolConfig, adapter: Arc<dyn PeerStreamAdapter>) -> Self {
        let config = config
            .validate()
            .expect("peer pool configuration must be validated before composition");
        let shared = Arc::new(PoolShared {
            config,
            adapter,
            state: Mutex::new(PoolState::default()),
            addresses: PeerAddressCache::new(PEER_ADDRESS_CACHE_TTL),
            eviction_task: Mutex::new(None),
            #[cfg(test)]
            completed_drivers: Arc::new(AtomicUsize::new(0)),
        });
        spawn_idle_eviction_task(&shared);
        Self { shared }
    }

    pub(crate) async fn acquire(
        &self,
        peer: &HostName,
        port: NonZeroU16,
        deadline: RequestDeadline,
    ) -> Result<PooledPeerConnection, HttpRuntimeClientFailure> {
        let authority = direct_peer_authority(peer, port);
        let (idle, expired) = {
            let mut state = self.shared.state.lock().expect("peer pool state");
            let expired = take_expired_idle(&mut state, peer, self.shared.config.idle_timeout);
            let idle = state.idle.get_mut(peer).and_then(Vec::pop);
            (idle, expired)
        };
        drop(expired);

        if let Some(mut idle) = idle {
            let connection_target = format!("direct peer `{authority}`");
            let ready = if idle.sender.is_closed() {
                false
            } else {
                let remaining = deadline.remaining().ok_or_else(|| {
                    self.shared.release_reservation(peer);
                    peer_connect_deadline_failure(&authority, &connection_target)
                })?;
                match tokio::time::timeout(remaining, idle.sender.ready()).await {
                    Ok(Ok(())) => true,
                    Ok(Err(_)) => false,
                    Err(_) => {
                        self.shared.release_reservation(peer);
                        return Err(peer_connect_deadline_failure(
                            &authority,
                            &connection_target,
                        ));
                    }
                }
            };
            if ready {
                tracing::debug!(%peer, %authority, "reusing pooled opaque peer connection");
                return Ok(PooledPeerConnection {
                    peer: peer.clone(),
                    authority,
                    origin: ConnectionOrigin::Pooled,
                    sender: Some(idle.sender),
                    driver: Some(idle.driver),
                    health: GuardHealth::Unused,
                    pool: Arc::downgrade(&self.shared),
                });
            }
            drop(idle);
            // A stale entry keeps its reservation while being replaced. A
            // failed replacement releases it before surfacing the error.
            return self
                .acquire_replacement(peer, port, deadline, authority)
                .await;
        }

        let origin = self.shared.reserve_or_overflow(peer);
        tracing::debug!(%peer, %authority, ?origin, "opening opaque peer connection");
        self.acquire_fresh(peer, port, deadline, authority, origin)
            .await
    }

    async fn acquire_replacement(
        &self,
        peer: &HostName,
        port: NonZeroU16,
        deadline: RequestDeadline,
        authority: String,
    ) -> Result<PooledPeerConnection, HttpRuntimeClientFailure> {
        match self.open(peer, port, deadline, &authority).await {
            Ok((sender, driver)) => Ok(PooledPeerConnection {
                peer: peer.clone(),
                authority,
                origin: ConnectionOrigin::Pooled,
                sender: Some(sender),
                driver: Some(driver),
                health: GuardHealth::Unused,
                pool: Arc::downgrade(&self.shared),
            }),
            Err(error) => {
                self.shared.release_reservation(peer);
                Err(error)
            }
        }
    }

    async fn acquire_fresh(
        &self,
        peer: &HostName,
        port: NonZeroU16,
        deadline: RequestDeadline,
        authority: String,
        origin: ConnectionOrigin,
    ) -> Result<PooledPeerConnection, HttpRuntimeClientFailure> {
        match self.open(peer, port, deadline, &authority).await {
            Ok((sender, driver)) => Ok(PooledPeerConnection {
                peer: peer.clone(),
                authority,
                origin,
                sender: Some(sender),
                driver: Some(driver),
                health: GuardHealth::Unused,
                pool: Arc::downgrade(&self.shared),
            }),
            Err(error) => {
                if origin == ConnectionOrigin::Pooled {
                    self.shared.release_reservation(peer);
                }
                Err(error)
            }
        }
    }

    async fn open(
        &self,
        peer: &HostName,
        port: NonZeroU16,
        deadline: RequestDeadline,
        authority: &str,
    ) -> Result<(http1::SendRequest<Body>, JoinHandle<()>), HttpRuntimeClientFailure> {
        let target = format!("direct peer `{authority}`");
        let connect_timeout = || peer_connect_deadline_failure(authority, &target);
        let remaining = deadline.remaining().ok_or_else(connect_timeout)?;
        let stream = tokio::time::timeout(
            remaining,
            self.shared.addresses.connect(
                peer,
                port,
                deadline,
                resolve_peer_addresses,
                TcpStream::connect,
            ),
        )
        .await
        .map_err(|_| {
            tracing::warn!(
                %peer,
                %authority,
                "direct peer TCP connect exhausted the request budget"
            );
            connect_timeout()
        })?
        .map_err(|cause| {
            tracing::warn!(%peer, %authority, %cause, "direct peer TCP connect failed");
            HttpRuntimeClientFailure::PeerConnect {
                target: authority.to_owned(),
                cause,
            }
        })?;
        let remaining = deadline.remaining().ok_or_else(connect_timeout)?;
        let stream = tokio::time::timeout(remaining, self.shared.adapter.connect(stream, peer))
            .await
            .map_err(|_| connect_timeout())?
            .map_err(|error| {
                let cause = error.to_string();
                tracing::warn!(
                    %peer,
                    %authority,
                    %cause,
                    "direct peer stream adapter rejected the connection"
                );
                HttpRuntimeClientFailure::PeerConnect {
                    target: authority.to_owned(),
                    cause,
                }
            })?;
        let remaining = deadline.remaining().ok_or_else(connect_timeout)?;
        let (sender, connection) = tokio::time::timeout(
            remaining,
            http1::handshake(TokioIo::new(stream)),
        )
        .await
        .map_err(|_| connect_timeout())?
        .map_err(|source| {
            let cause = source.to_string();
            tracing::warn!(%peer, %authority, %cause, "direct peer HTTP/1 handshake failed");
            HttpRuntimeClientFailure::Connect(cause)
        })?;
        #[cfg(test)]
        let completed_drivers = Arc::clone(&self.shared.completed_drivers);
        let driver = tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::debug!(%error, "pooled opaque peer HTTP client connection ended");
            }
            #[cfg(test)]
            completed_drivers.fetch_add(1, Ordering::SeqCst);
        });
        Ok((sender, driver))
    }

    #[cfg(test)]
    pub(crate) fn pooled_count(&self) -> usize {
        self.shared
            .state
            .lock()
            .expect("peer pool state")
            .pooled_total
    }

    #[cfg(test)]
    pub(crate) fn evict_idle_now(&self) {
        self.shared.evict_expired();
    }

    /// Closes retained senders and waits only a bounded interval for their
    /// connection drivers. Daemon shutdown invokes this after request drain;
    /// guard `Drop` stays synchronous and never waits for a task.
    pub(crate) async fn shutdown(&self, deadline: Duration) {
        let (idle, eviction_task) = {
            let mut state = self.shared.state.lock().expect("peer pool state");
            let idle = std::mem::take(&mut state.idle)
                .into_values()
                .flatten()
                .collect::<Vec<_>>();
            state.reservations_by_peer.clear();
            state.pooled_total = 0;
            let eviction_task = self
                .shared
                .eviction_task
                .lock()
                .expect("peer pool eviction task")
                .take();
            (idle, eviction_task)
        };
        let shutdown_deadline = Instant::now() + deadline;
        if let Some(task) = eviction_task {
            task.abort();
            drain_driver(task, shutdown_deadline).await;
        }
        for connection in idle {
            drop(connection.sender);
            drain_driver(connection.driver, shutdown_deadline).await;
        }
    }

    #[cfg(test)]
    pub(crate) fn completed_driver_count(&self) -> usize {
        self.shared.completed_drivers.load(Ordering::SeqCst)
    }
}

impl PooledPeerConnection {
    pub(crate) async fn exchange(
        &mut self,
        request: HttpRequest,
        deadline: RequestDeadline,
    ) -> Result<axum::http::Response<Vec<u8>>, HttpRuntimeClientFailure> {
        let sender = self.sender.as_mut().expect("pool guard owns sender");
        let result = execute_opaque_peer_request_with_sender(sender, request, deadline)
            .await
            .map_err(|failure| direct_peer_connection_failure(&self.authority, failure));
        self.health = if result.is_ok() {
            GuardHealth::Healthy
        } else {
            GuardHealth::Failed
        };
        result
    }
}

impl Drop for PooledPeerConnection {
    fn drop(&mut self) {
        let Some(sender) = self.sender.take() else {
            return;
        };
        let Some(driver) = self.driver.take() else {
            return;
        };
        let Some(pool) = self.pool.upgrade() else {
            drop((sender, driver));
            return;
        };
        if self.origin == ConnectionOrigin::Pooled && self.health == GuardHealth::Healthy {
            pool.state
                .lock()
                .expect("peer pool state")
                .idle
                .entry(self.peer.clone())
                .or_default()
                .push(IdlePeerConnection {
                    sender,
                    driver,
                    idle_since: Instant::now(),
                });
            return;
        }
        if self.origin == ConnectionOrigin::Pooled {
            pool.release_reservation(&self.peer);
        }
        drop((sender, driver));
    }
}

impl PoolShared {
    fn reserve_or_overflow(&self, peer: &HostName) -> ConnectionOrigin {
        let mut state = self.state.lock().expect("peer pool state");
        let per_peer = state
            .reservations_by_peer
            .get(peer)
            .copied()
            .unwrap_or_default();
        if per_peer >= self.config.max_per_peer
            || state.pooled_total >= self.config.max_pooled_total
        {
            return ConnectionOrigin::Overflow;
        }
        state.pooled_total += 1;
        state
            .reservations_by_peer
            .insert(peer.clone(), per_peer + 1);
        ConnectionOrigin::Pooled
    }

    fn release_reservation(&self, peer: &HostName) {
        let mut state = self.state.lock().expect("peer pool state");
        let per_peer = state
            .reservations_by_peer
            .get_mut(peer)
            .expect("pooled reservation exists before release");
        *per_peer -= 1;
        if *per_peer == 0 {
            state.reservations_by_peer.remove(peer);
        }
        state.pooled_total -= 1;
    }

    fn evict_expired(&self) {
        let mut expired = Vec::new();
        {
            let mut state = self.state.lock().expect("peer pool state");
            let peers = state.idle.keys().cloned().collect::<Vec<_>>();
            for peer in peers {
                expired.extend(take_expired_idle(
                    &mut state,
                    &peer,
                    self.config.idle_timeout,
                ));
            }
        }
        drop(expired);
    }
}

fn take_expired_idle(
    state: &mut PoolState,
    peer: &HostName,
    idle_timeout: Duration,
) -> Vec<IdlePeerConnection> {
    let now = Instant::now();
    let mut expired = Vec::new();
    if let Some(entries) = state.idle.get_mut(peer) {
        let mut retained = Vec::with_capacity(entries.len());
        for entry in entries.drain(..) {
            if now.duration_since(entry.idle_since) >= idle_timeout {
                expired.push(entry);
            } else {
                retained.push(entry);
            }
        }
        *entries = retained;
    }
    if state.idle.get(peer).is_some_and(Vec::is_empty) {
        state.idle.remove(peer);
    }
    if !expired.is_empty() {
        let per_peer = state
            .reservations_by_peer
            .get_mut(peer)
            .expect("idle connection owns pooled reservation");
        *per_peer -= expired.len();
        if *per_peer == 0 {
            state.reservations_by_peer.remove(peer);
        }
        state.pooled_total -= expired.len();
    }
    expired
}

fn spawn_idle_eviction_task(shared: &Arc<PoolShared>) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let weak = Arc::downgrade(shared);
    let interval = shared.config.idle_timeout;
    let task = handle.spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            let Some(pool) = weak.upgrade() else {
                return;
            };
            pool.evict_expired();
        }
    });
    *shared
        .eviction_task
        .lock()
        .expect("peer pool eviction task") = Some(task);
}

async fn drain_driver(mut driver: JoinHandle<()>, shutdown_deadline: Instant) {
    let Some(remaining) = shutdown_deadline.checked_duration_since(Instant::now()) else {
        driver.abort();
        let _ = tokio::time::timeout(DRIVER_ABORT_GRACE, &mut driver).await;
        return;
    };
    if tokio::time::timeout(remaining, &mut driver).await.is_err() {
        driver.abort();
        let _ = tokio::time::timeout(DRIVER_ABORT_GRACE, &mut driver).await;
    }
}

/// Resolved dial order for one direct peer authority.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ConnectCandidates {
    usable: Vec<SocketAddr>,
    skipped_link_local: usize,
}

/// Orders resolved addresses so a dial loop never stalls on an address the
/// operating system cannot route.
///
/// macOS multicast DNS answers a `.local` peer name with both an IPv4 address
/// and a scope-less link-local IPv6 address (`fe80::/10` with no `%interface`
/// scope). The kernel cannot route the scope-less form, so a plain
/// `TcpStream::connect(host)` either fails immediately when it is the only
/// answer, or stalls for the whole request budget when it is tried first.
/// Such candidates are dropped and IPv4 candidates are dialled before the
/// remaining IPv6 candidates. Scoped link-local addresses stay usable.
fn order_connect_candidates(resolved: impl IntoIterator<Item = SocketAddr>) -> ConnectCandidates {
    let mut ipv4 = Vec::new();
    let mut ipv6 = Vec::new();
    let mut skipped_link_local = 0;
    for address in resolved {
        match address {
            SocketAddr::V4(_) => ipv4.push(address),
            SocketAddr::V6(v6) if is_scope_less_link_local(v6) => skipped_link_local += 1,
            SocketAddr::V6(_) => ipv6.push(address),
        }
    }
    ipv4.extend(ipv6);
    ConnectCandidates {
        usable: ipv4,
        skipped_link_local,
    }
}

fn is_scope_less_link_local(address: std::net::SocketAddrV6) -> bool {
    address.ip().is_unicast_link_local() && address.scope_id() == 0
}

/// How long one resolved address set is reused before the peer name is looked
/// up again. The TTL only bounds how long a *working* address is trusted
/// without re-asking the resolver; a wired/Wi-Fi/VPN move is followed
/// immediately regardless, because an entry that fails to dial is discarded
/// and the name is resolved again inside the same request.
const PEER_ADDRESS_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

/// Short-term, in-memory memory of where a peer name last resolved.
///
/// This is process memory only: resolved addresses are never persisted or
/// treated as an alias for the registered hostname
/// (`REQ-CORE-TRANSPORT-002D`). When cached addresses no longer accept a
/// connection, the name is resolved again and dialled again inside the same
/// request budget, so a peer whose address changed is reached without an
/// error surfacing to the caller.
struct PeerAddressCache {
    ttl: Duration,
    entries: Mutex<HashMap<(HostName, NonZeroU16), CachedAddresses>>,
}

#[derive(Debug, Clone)]
struct CachedAddresses {
    candidates: ConnectCandidates,
    resolved_at: Instant,
}

impl PeerAddressCache {
    fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: Mutex::new(HashMap::new()),
        }
    }

    fn fresh(&self, key: &(HostName, NonZeroU16)) -> Option<ConnectCandidates> {
        let entries = self.entries.lock().expect("peer address cache");
        entries
            .get(key)
            .filter(|entry| entry.resolved_at.elapsed() < self.ttl)
            .map(|entry| entry.candidates.clone())
    }

    fn store(&self, key: (HostName, NonZeroU16), candidates: ConnectCandidates) {
        let mut entries = self.entries.lock().expect("peer address cache");
        entries.insert(
            key,
            CachedAddresses {
                candidates,
                resolved_at: Instant::now(),
            },
        );
    }

    fn forget(&self, key: &(HostName, NonZeroU16)) {
        let mut entries = self.entries.lock().expect("peer address cache");
        entries.remove(key);
    }

    /// Connects to `peer`, reusing a fresh cached address set when one exists
    /// and otherwise resolving the name through `resolve`. A dial failure on
    /// cached addresses falls through to one fresh resolution and dial.
    async fn connect<Resolve, Resolving, Connect, Dial>(
        &self,
        peer: &HostName,
        port: NonZeroU16,
        deadline: RequestDeadline,
        resolve: Resolve,
        connect: Connect,
    ) -> Result<TcpStream, String>
    where
        Resolve: Fn(HostName, NonZeroU16) -> Resolving,
        Resolving: Future<Output = std::io::Result<Vec<SocketAddr>>>,
        Connect: Fn(SocketAddr) -> Dial + Copy,
        Dial: Future<Output = std::io::Result<TcpStream>>,
    {
        let key = (peer.clone(), port);
        if let Some(cached) = self.fresh(&key) {
            // Cached addresses may be stale, so they get at most half of the
            // remaining budget; the other half is reserved for the fresh
            // lookup and dial that follow when they no longer answer.
            let cached_deadline = deadline
                .remaining()
                .map_or(deadline, |remaining| RequestDeadline::after(remaining / 2));
            match dial_candidates(cached, cached_deadline, connect).await {
                Ok(stream) => return Ok(stream),
                Err(cause) => {
                    tracing::info!(
                        %peer,
                        %cause,
                        "cached peer addresses no longer connect; resolving the peer name again"
                    );
                    self.forget(&key);
                }
            }
        }

        let resolved = resolve(peer.clone(), port)
            .await
            .map_err(|error| format!("DNS resolution of `{peer}` failed: {error}"))?;
        let candidates = order_connect_candidates(resolved);
        self.store(key.clone(), candidates.clone());
        match dial_candidates(candidates, deadline, connect).await {
            Ok(stream) => Ok(stream),
            Err(cause) => {
                self.forget(&key);
                Err(cause)
            }
        }
    }
}

/// Forward resolution of the registered peer hostname through the operating
/// system resolver (DNS, DDNS, or mDNS for `.local` names).
async fn resolve_peer_addresses(
    peer: HostName,
    port: NonZeroU16,
) -> std::io::Result<Vec<SocketAddr>> {
    tokio::net::lookup_host((peer.as_str(), port.get()))
        .await
        .map(Iterator::collect)
}

/// Dials each candidate in order and returns the first established stream.
///
/// Every attempt is bounded by an even share of the remaining request budget
/// across the candidates still untried, so one unresponsive address can never
/// consume the budget that a later, reachable address needs. The returned
/// cause names every attempt so the operator can see which address failed
/// and why.
async fn dial_candidates<Connect, Dial>(
    candidates: ConnectCandidates,
    deadline: RequestDeadline,
    connect: Connect,
) -> Result<TcpStream, String>
where
    Connect: Fn(SocketAddr) -> Dial,
    Dial: Future<Output = std::io::Result<TcpStream>>,
{
    let ConnectCandidates {
        usable,
        skipped_link_local,
    } = candidates;
    let skipped_note = || {
        (skipped_link_local > 0).then(|| {
            format!(
                "{skipped_link_local} scope-less link-local IPv6 address(es) skipped as unroutable"
            )
        })
    };
    if usable.is_empty() {
        return Err(skipped_note().map_or_else(
            || "the host resolved to no addresses".to_owned(),
            |note| format!("the host resolved to no routable address ({note})"),
        ));
    }

    let total = usable.len();
    let mut failures = Vec::with_capacity(total);
    for (index, address) in usable.into_iter().enumerate() {
        let Some(remaining) = deadline.remaining() else {
            failures.push(format!(
                "{address}: request budget elapsed before the attempt"
            ));
            break;
        };
        let untried = u32::try_from(total - index).unwrap_or(u32::MAX);
        let budget = remaining / untried;
        match tokio::time::timeout(budget, connect(address)).await {
            Ok(Ok(stream)) => {
                if !failures.is_empty() {
                    tracing::info!(
                        %address,
                        earlier_attempts = failures.join("; "),
                        "direct peer connected after earlier candidate addresses failed"
                    );
                }
                return Ok(stream);
            }
            Ok(Err(error)) => failures.push(format!("{address}: {error}")),
            Err(_) => failures.push(format!(
                "{address}: no response within its {}ms share of the request budget",
                budget.as_millis()
            )),
        }
    }
    failures.extend(skipped_note());
    Err(failures.join("; "))
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use axum::Router;
    use axum::http::{StatusCode, header};
    use axum::routing::post;
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::watch;

    use super::{
        ConnectCandidates, PeerConnectionPool, PeerPoolConfig, dial_candidates,
        order_connect_candidates,
    };
    use crate::http1_server::serve_connection;
    use crate::{AcceptedPeerStream, EstablishedPeerStream, PeerStreamAdapter, PeerStreamFuture};

    // A test-only allowance for resolving `localhost` to the IPv4-only test
    // listener on Windows. Production callers retain their supplied deadline.
    const AUTHORITY_ISOLATION_TEST_DEADLINE: Duration = Duration::from_secs(5);

    #[derive(Default)]
    struct CountingPassthroughAdapter {
        connects: AtomicUsize,
    }

    impl PeerStreamAdapter for CountingPassthroughAdapter {
        fn connect<'a>(
            &'a self,
            stream: tokio::net::TcpStream,
            _peer: &'a atm_core::types::HostName,
        ) -> PeerStreamFuture<'a, EstablishedPeerStream> {
            self.connects.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Ok(Box::new(stream) as EstablishedPeerStream) })
        }

        fn accept<'a>(
            &'a self,
            _stream: tokio::net::TcpStream,
        ) -> PeerStreamFuture<'a, AcceptedPeerStream> {
            Box::pin(async { Err(atm_core::error::AtmError::config("test accepts no peers")) })
        }
    }

    async fn start_keep_alive_peer() -> u16 {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("test listener binds");
        let port = listener.local_addr().expect("listener address").port();
        tokio::spawn(async move {
            let router = Router::new().route(
                "/v1/atm/messages",
                post(|| async { Result::<_, Infallible>::Ok(StatusCode::CREATED) }),
            );
            loop {
                let (stream, _) = listener.accept().await.expect("peer accepts connection");
                let router = router.clone();
                let (shutdown_tx, shutdown_rx) = watch::channel(());
                tokio::spawn(async move {
                    let _shutdown_tx = shutdown_tx;
                    let _ = serve_connection(
                        hyper_util::rt::TokioIo::new(stream),
                        router,
                        std::time::Duration::from_secs(30),
                        shutdown_rx,
                    )
                    .await;
                });
            }
        });
        port
    }

    async fn start_close_after_response_peer() -> u16 {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("test listener binds");
        let port = listener.local_addr().expect("listener address").port();
        tokio::spawn(async move {
            let router = Router::new().route(
                "/v1/atm/messages",
                post(|| async { ([(header::CONNECTION, "close")], StatusCode::CREATED) }),
            );
            loop {
                let (stream, _) = listener.accept().await.expect("peer accepts connection");
                let router = router.clone();
                let (shutdown_tx, shutdown_rx) = watch::channel(());
                tokio::spawn(async move {
                    let _shutdown_tx = shutdown_tx;
                    let _ = serve_connection(
                        hyper_util::rt::TokioIo::new(stream),
                        router,
                        std::time::Duration::from_secs(30),
                        shutdown_rx,
                    )
                    .await;
                });
            }
        });
        port
    }

    async fn start_drop_peer() -> (u16, Arc<AtomicUsize>) {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("test listener binds");
        let port = listener.local_addr().expect("listener address").port();
        let accepted = Arc::new(AtomicUsize::new(0));
        let accepted_for_task = Arc::clone(&accepted);
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.expect("peer accepts connection");
                accepted_for_task.fetch_add(1, Ordering::SeqCst);
                drop(stream);
            }
        });
        (port, accepted)
    }

    async fn start_close_peer_with_listener_shutdown() -> (
        u16,
        tokio::sync::oneshot::Sender<()>,
        tokio::sync::oneshot::Receiver<()>,
    ) {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("test listener binds");
        let port = listener.local_addr().expect("listener address").port();
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();
        let (stopped_tx, stopped_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let router = Router::new().route(
                "/v1/atm/messages",
                post(|| async { ([(header::CONNECTION, "close")], StatusCode::CREATED) }),
            );
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.expect("peer accepts connection");
                        let router = router.clone();
                        let (connection_shutdown_tx, connection_shutdown_rx) = watch::channel(());
                        tokio::spawn(async move {
                            let _connection_shutdown_tx = connection_shutdown_tx;
                            let _ = serve_connection(
                                hyper_util::rt::TokioIo::new(stream),
                                router,
                                std::time::Duration::from_secs(30),
                                connection_shutdown_rx,
                            )
                            .await;
                        });
                    }
                }
            }
            let _ = stopped_tx.send(());
        });
        (port, shutdown_tx, stopped_rx)
    }

    fn request() -> atm_core::api::HttpRequest {
        atm_core::api::HttpRequest {
            method: "POST".to_owned(),
            path: "/v1/atm/messages".to_owned(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    async fn complete_one_request(connection: &mut super::PooledPeerConnection) {
        let response = connection
            .exchange(
                request(),
                atm_core::api::RequestDeadline::after(std::time::Duration::from_secs(1)),
            )
            .await
            .expect("peer request succeeds");
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    async fn assert_driver_completions(pool: &PeerConnectionPool, minimum: usize, context: &str) {
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while pool.completed_driver_count() < minimum {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("connection driver did not complete after {context}"));
    }

    fn deadline(duration: std::time::Duration) -> atm_core::api::RequestDeadline {
        atm_core::api::RequestDeadline::after(duration)
    }

    fn v6(address: &str, port: u16, scope_id: u32) -> std::net::SocketAddr {
        std::net::SocketAddr::V6(std::net::SocketAddrV6::new(
            address.parse().expect("ipv6 literal"),
            port,
            0,
            scope_id,
        ))
    }

    fn v4(address: &str, port: u16) -> std::net::SocketAddr {
        std::net::SocketAddr::V4(std::net::SocketAddrV4::new(
            address.parse().expect("ipv4 literal"),
            port,
        ))
    }

    /// Dials loopback for real and never answers any other address, which is
    /// how an unroutable link-local or black-holed address behaves.
    fn loopback_only_connector(
        address: std::net::SocketAddr,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::io::Result<TcpStream>> + Send>>
    {
        Box::pin(async move {
            if address.ip().is_loopback() {
                TcpStream::connect(address).await
            } else {
                std::future::pending().await
            }
        })
    }

    #[test]
    fn dial_order_drops_scope_less_link_local_ipv6_and_prefers_ipv4() {
        // mDNS answer shape observed for `rand-m5.local` on macOS: a routable
        // IPv4 record and a scope-less link-local IPv6 record.
        let candidates = order_connect_candidates([
            v6("fe80::4af:1", 43_101, 0),
            v4("192.168.1.155", 43_101),
            v6("2001:db8::10", 43_101, 0),
            v6("fe80::4af:2", 43_101, 7),
        ]);
        assert_eq!(
            candidates,
            ConnectCandidates {
                usable: vec![
                    v4("192.168.1.155", 43_101),
                    v6("2001:db8::10", 43_101, 0),
                    v6("fe80::4af:2", 43_101, 7),
                ],
                skipped_link_local: 1,
            },
            "IPv4 first, then routable IPv6; scoped link-local stays usable"
        );
    }

    #[tokio::test]
    async fn mixed_family_host_connects_without_stalling_on_link_local_ipv6() {
        let port = start_keep_alive_peer().await;
        let started = std::time::Instant::now();
        let stream = dial_candidates(
            order_connect_candidates([v6("fe80::4af:1", port, 0), v4("127.0.0.1", port)]),
            deadline(std::time::Duration::from_secs(10)),
            loopback_only_connector,
        )
        .await
        .expect("the routable IPv4 address must be reached");
        assert!(stream.peer_addr().is_ok());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "the unroutable IPv6 answer must not consume the request budget"
        );
    }

    #[tokio::test]
    async fn unresponsive_first_address_is_bounded_so_the_next_address_still_connects() {
        let port = start_keep_alive_peer().await;
        let started = std::time::Instant::now();
        let stream = dial_candidates(
            ConnectCandidates {
                usable: vec![v4("10.255.255.1", port), v4("127.0.0.1", port)],
                skipped_link_local: 0,
            },
            deadline(std::time::Duration::from_millis(600)),
            loopback_only_connector,
        )
        .await
        .expect("the second address must be dialled within the same request budget");
        assert!(stream.peer_addr().is_ok());
        let elapsed = started.elapsed();
        assert!(
            elapsed >= std::time::Duration::from_millis(250)
                && elapsed < std::time::Duration::from_millis(600),
            "first attempt gets an even share of the budget, not all of it: {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn link_local_only_answer_fails_fast_with_an_actionable_cause() {
        let started = std::time::Instant::now();
        let cause = dial_candidates(
            order_connect_candidates([v6("fe80::4af:1", 43_101, 0)]),
            deadline(std::time::Duration::from_secs(10)),
            loopback_only_connector,
        )
        .await
        .expect_err("an unroutable-only answer must not be dialled");
        assert!(cause.contains("no routable address"), "{cause}");
        assert!(cause.contains("link-local IPv6"), "{cause}");
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }

    #[tokio::test]
    async fn every_failed_attempt_is_named_in_the_cause() {
        let cause = dial_candidates(
            ConnectCandidates {
                usable: vec![v4("10.255.255.1", 43_101), v4("127.0.0.1", 1)],
                skipped_link_local: 1,
            },
            deadline(std::time::Duration::from_millis(400)),
            loopback_only_connector,
        )
        .await
        .expect_err("no candidate is reachable");
        assert!(
            cause.contains("10.255.255.1:43101: no response within"),
            "{cause}"
        );
        assert!(cause.contains("127.0.0.1:1: "), "{cause}");
        assert!(
            cause.contains("1 scope-less link-local IPv6 address(es) skipped"),
            "{cause}"
        );
    }

    type ResolverAnswer = std::pin::Pin<
        Box<dyn std::future::Future<Output = std::io::Result<Vec<std::net::SocketAddr>>> + Send>,
    >;

    fn counting_resolver(
        answers: Arc<Mutex<Vec<Vec<std::net::SocketAddr>>>>,
        calls: Arc<AtomicUsize>,
    ) -> impl Fn(atm_core::types::HostName, std::num::NonZeroU16) -> ResolverAnswer {
        move |_peer, _port| {
            calls.fetch_add(1, Ordering::SeqCst);
            let mut answers = answers.lock().expect("resolver answers");
            let answer = if answers.len() > 1 {
                answers.remove(0)
            } else {
                answers[0].clone()
            };
            Box::pin(async move { Ok(answer) })
        }
    }

    #[tokio::test]
    async fn cached_addresses_are_reused_within_the_ttl() {
        let port = start_keep_alive_peer().await;
        let cache = super::PeerAddressCache::new(Duration::from_secs(30));
        let calls = Arc::new(AtomicUsize::new(0));
        let resolver = counting_resolver(
            Arc::new(Mutex::new(vec![vec![v4("127.0.0.1", port)]])),
            calls.clone(),
        );
        let peer: atm_core::types::HostName = "rand-m5.local".parse().expect("peer host");
        let port = std::num::NonZeroU16::new(port).expect("non-zero test port");
        for _ in 0..3 {
            cache
                .connect(
                    &peer,
                    port,
                    deadline(Duration::from_secs(5)),
                    &resolver,
                    loopback_only_connector,
                )
                .await
                .expect("cached loopback address connects");
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "one lookup serves the burst"
        );
    }

    #[tokio::test]
    async fn changed_peer_address_is_re_resolved_and_reached_without_an_error() {
        let port = start_keep_alive_peer().await;
        let cache = super::PeerAddressCache::new(Duration::from_secs(30));
        let calls = Arc::new(AtomicUsize::new(0));
        // The cache holds the address the peer had on its previous network;
        // it no longer responds. The resolver now answers with the new one.
        let resolver = counting_resolver(
            Arc::new(Mutex::new(vec![vec![v4("127.0.0.1", port)]])),
            calls.clone(),
        );
        let peer: atm_core::types::HostName = "rand-m5.local".parse().expect("peer host");
        let port = std::num::NonZeroU16::new(port).expect("non-zero test port");
        // Seed the cache with the old address without dialling it.
        cache.store(
            (peer.clone(), port),
            order_connect_candidates([v4("10.255.255.1", port.get())]),
        );
        let started = std::time::Instant::now();
        let stream = cache
            .connect(
                &peer,
                port,
                deadline(Duration::from_secs(2)),
                &resolver,
                loopback_only_connector,
            )
            .await
            .expect("the moved peer is reached inside the same request");
        assert!(stream.peer_addr().is_ok());
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the stale entry triggers exactly one fresh lookup"
        );
        assert_eq!(
            cache.fresh(&(peer, port)).map(|c| c.usable),
            Some(vec![v4("127.0.0.1", port.get())]),
            "the fresh answer replaces the stale entry"
        );
    }

    #[tokio::test]
    async fn expired_cache_entries_are_resolved_again() {
        let port = start_keep_alive_peer().await;
        let cache = super::PeerAddressCache::new(Duration::from_millis(20));
        let calls = Arc::new(AtomicUsize::new(0));
        let resolver = counting_resolver(
            Arc::new(Mutex::new(vec![vec![v4("127.0.0.1", port)]])),
            calls.clone(),
        );
        let peer: atm_core::types::HostName = "rand-m5.local".parse().expect("peer host");
        let port = std::num::NonZeroU16::new(port).expect("non-zero test port");
        for _ in 0..2 {
            cache
                .connect(
                    &peer,
                    port,
                    deadline(Duration::from_secs(5)),
                    &resolver,
                    loopback_only_connector,
                )
                .await
                .expect("loopback connects");
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "an expired entry is not reused"
        );
    }

    #[tokio::test]
    async fn unreachable_peer_leaves_no_cache_entry_behind() {
        let cache = super::PeerAddressCache::new(Duration::from_secs(30));
        let calls = Arc::new(AtomicUsize::new(0));
        let resolver = counting_resolver(
            Arc::new(Mutex::new(vec![vec![v4("10.255.255.1", 43_101)]])),
            calls.clone(),
        );
        let peer: atm_core::types::HostName = "rand-m5.local".parse().expect("peer host");
        let port = std::num::NonZeroU16::new(43_101).expect("non-zero port");
        let cause = cache
            .connect(
                &peer,
                port,
                deadline(Duration::from_millis(200)),
                &resolver,
                loopback_only_connector,
            )
            .await
            .expect_err("nothing answers");
        assert!(cause.contains("10.255.255.1:43101"), "{cause}");
        assert!(cache.fresh(&(peer, port)).is_none());
    }

    #[tokio::test]
    async fn sequential_writes_reuse_one_authenticated_http1_connection() {
        let port = start_keep_alive_peer().await;
        let adapter = std::sync::Arc::new(CountingPassthroughAdapter::default());
        let pool = PeerConnectionPool::new(PeerPoolConfig::default(), adapter.clone());
        let peer = "127.0.0.1".parse().expect("peer authority");

        for _ in 0..3 {
            let mut connection = pool
                .acquire(
                    &peer,
                    std::num::NonZeroU16::new(port).expect("non-zero test port"),
                    atm_core::api::RequestDeadline::after(std::time::Duration::from_secs(1)),
                )
                .await
                .expect("pool acquires peer connection");
            complete_one_request(&mut connection).await;
        }

        assert_eq!(adapter.connects.load(Ordering::SeqCst), 1);
        assert_eq!(pool.pooled_count(), 1);
        let completed_before_shutdown = pool.completed_driver_count();
        pool.shutdown(std::time::Duration::from_secs(1)).await;
        assert_eq!(pool.pooled_count(), 0);
        assert_driver_completions(&pool, completed_before_shutdown + 1, "pool teardown").await;
    }

    #[tokio::test]
    async fn pool_never_reuses_a_connection_across_configured_authorities() {
        let port = start_keep_alive_peer().await;
        let adapter = std::sync::Arc::new(CountingPassthroughAdapter::default());
        let pool = PeerConnectionPool::new(PeerPoolConfig::default(), adapter.clone());
        for peer in ["127.0.0.1", "localhost"] {
            let peer = peer.parse().expect("peer authority");
            let mut connection = pool
                .acquire(
                    &peer,
                    std::num::NonZeroU16::new(port).expect("non-zero test port"),
                    atm_core::api::RequestDeadline::after(AUTHORITY_ISOLATION_TEST_DEADLINE),
                )
                .await
                .expect("pool acquires peer connection");
            complete_one_request(&mut connection).await;
        }

        assert_eq!(adapter.connects.load(Ordering::SeqCst), 2);
        assert_eq!(pool.pooled_count(), 2);
        let completed_before_shutdown = pool.completed_driver_count();
        pool.shutdown(std::time::Duration::from_secs(1)).await;
        assert_driver_completions(&pool, completed_before_shutdown + 2, "two-peer teardown").await;
    }

    #[tokio::test]
    async fn overflow_connection_is_closed_instead_of_becoming_a_retained_entry() {
        let port = start_keep_alive_peer().await;
        let adapter = std::sync::Arc::new(CountingPassthroughAdapter::default());
        let pool = PeerConnectionPool::new(
            PeerPoolConfig {
                max_per_peer: 1,
                max_pooled_total: 1,
                ..PeerPoolConfig::default()
            },
            adapter,
        );
        let peer = "127.0.0.1".parse().expect("peer authority");
        let mut retained = pool
            .acquire(
                &peer,
                std::num::NonZeroU16::new(port).expect("non-zero test port"),
                atm_core::api::RequestDeadline::after(std::time::Duration::from_secs(1)),
            )
            .await
            .expect("first connection reserves pooled slot");
        let mut overflow = pool
            .acquire(
                &peer,
                std::num::NonZeroU16::new(port).expect("non-zero test port"),
                atm_core::api::RequestDeadline::after(std::time::Duration::from_secs(1)),
            )
            .await
            .expect("overflow falls back without queueing");
        assert_eq!(pool.pooled_count(), 1);
        complete_one_request(&mut retained).await;
        complete_one_request(&mut overflow).await;
        let completed_before_drop = pool.completed_driver_count();
        drop(overflow);
        assert_eq!(pool.pooled_count(), 1);
        assert_driver_completions(&pool, completed_before_drop + 1, "overflow drop").await;
        drop(retained);
        assert_eq!(pool.pooled_count(), 1);
        pool.shutdown(std::time::Duration::from_secs(1)).await;
        assert_driver_completions(&pool, completed_before_drop + 2, "retained teardown").await;
    }

    #[tokio::test]
    async fn overflow_drop_after_failed_exchange_closes_its_driver() {
        let port = start_keep_alive_peer().await;
        let pool = PeerConnectionPool::new(
            PeerPoolConfig {
                max_per_peer: 1,
                max_pooled_total: 1,
                ..PeerPoolConfig::default()
            },
            Arc::new(CountingPassthroughAdapter::default()),
        );
        let peer = "127.0.0.1".parse().expect("peer authority");
        let retained = pool
            .acquire(
                &peer,
                std::num::NonZeroU16::new(port).expect("non-zero test port"),
                atm_core::api::RequestDeadline::after(std::time::Duration::from_secs(1)),
            )
            .await
            .expect("first borrow reserves the only retained slot");
        let mut overflow = pool
            .acquire(
                &peer,
                std::num::NonZeroU16::new(port).expect("non-zero test port"),
                atm_core::api::RequestDeadline::after(std::time::Duration::from_secs(1)),
            )
            .await
            .expect("second borrow dials overflow connection");

        let failure = overflow
            .exchange(
                atm_core::api::HttpRequest {
                    method: "POST".to_owned(),
                    path: "/v1/atm/messages".to_owned(),
                    headers: vec!["malformed-header".to_owned()],
                    body: Vec::new(),
                },
                atm_core::api::RequestDeadline::after(std::time::Duration::from_secs(1)),
            )
            .await
            .expect_err("malformed request marks the overflow guard failed before write");
        assert!(matches!(
            failure,
            super::HttpRuntimeClientFailure::PeerRequestWrite { .. }
        ));

        let completed_before_drop = pool.completed_driver_count();
        drop(overflow);
        assert_eq!(
            pool.pooled_count(),
            1,
            "a failed overflow borrow never consumes the retained reservation"
        );
        assert_driver_completions(
            &pool,
            completed_before_drop + 1,
            "failed overflow exchange drop",
        )
        .await;
        drop(retained);
        pool.shutdown(std::time::Duration::from_secs(1)).await;
    }

    #[tokio::test]
    async fn overflow_drop_without_exchange_closes_its_driver() {
        let port = start_keep_alive_peer().await;
        let pool = PeerConnectionPool::new(
            PeerPoolConfig {
                max_per_peer: 1,
                max_pooled_total: 1,
                ..PeerPoolConfig::default()
            },
            Arc::new(CountingPassthroughAdapter::default()),
        );
        let peer = "127.0.0.1".parse().expect("peer authority");
        let retained = pool
            .acquire(
                &peer,
                std::num::NonZeroU16::new(port).expect("non-zero test port"),
                atm_core::api::RequestDeadline::after(std::time::Duration::from_secs(1)),
            )
            .await
            .expect("first borrow reserves the only retained slot");
        let overflow = pool
            .acquire(
                &peer,
                std::num::NonZeroU16::new(port).expect("non-zero test port"),
                atm_core::api::RequestDeadline::after(std::time::Duration::from_secs(1)),
            )
            .await
            .expect("second borrow dials overflow connection");

        let completed_before_drop = pool.completed_driver_count();
        drop(overflow);
        assert_eq!(
            pool.pooled_count(),
            1,
            "an unused overflow borrow never enters the retained pool"
        );
        assert_driver_completions(&pool, completed_before_drop + 1, "unused overflow drop").await;
        drop(retained);
        pool.shutdown(std::time::Duration::from_secs(1)).await;
    }

    #[tokio::test(start_paused = true)]
    async fn expired_idle_connection_is_evicted_and_redialed() {
        let port = start_keep_alive_peer().await;
        let adapter = std::sync::Arc::new(CountingPassthroughAdapter::default());
        let pool = PeerConnectionPool::new(
            PeerPoolConfig {
                idle_timeout: std::time::Duration::from_millis(5),
                ..PeerPoolConfig::default()
            },
            adapter.clone(),
        );
        let peer = "127.0.0.1".parse().expect("peer authority");
        let mut connection = pool
            .acquire(
                &peer,
                std::num::NonZeroU16::new(port).expect("non-zero test port"),
                atm_core::api::RequestDeadline::after(std::time::Duration::from_secs(1)),
            )
            .await
            .expect("initial connection");
        complete_one_request(&mut connection).await;
        drop(connection);
        let completed_before_eviction = pool.completed_driver_count();
        tokio::time::advance(std::time::Duration::from_millis(15)).await;
        pool.evict_idle_now();
        assert_eq!(pool.pooled_count(), 0);
        assert_driver_completions(&pool, completed_before_eviction + 1, "idle eviction").await;
        let mut redial = pool
            .acquire(
                &peer,
                std::num::NonZeroU16::new(port).expect("non-zero test port"),
                atm_core::api::RequestDeadline::after(std::time::Duration::from_secs(1)),
            )
            .await
            .expect("post-eviction redial");
        complete_one_request(&mut redial).await;
        assert_eq!(adapter.connects.load(Ordering::SeqCst), 2);
        let completed_before_shutdown = pool.completed_driver_count();
        drop(redial);
        pool.shutdown(std::time::Duration::from_secs(1)).await;
        assert_driver_completions(&pool, completed_before_shutdown + 1, "redial teardown").await;
    }

    #[tokio::test]
    async fn unused_pooled_borrow_closes_its_driver_and_releases_its_reservation() {
        let port = start_keep_alive_peer().await;
        let pool = PeerConnectionPool::new(
            PeerPoolConfig {
                max_per_peer: 1,
                max_pooled_total: 1,
                ..PeerPoolConfig::default()
            },
            std::sync::Arc::new(CountingPassthroughAdapter::default()),
        );
        let peer = "127.0.0.1".parse().expect("peer authority");
        let connection = pool
            .acquire(
                &peer,
                std::num::NonZeroU16::new(port).expect("non-zero test port"),
                atm_core::api::RequestDeadline::after(std::time::Duration::from_secs(1)),
            )
            .await
            .expect("pooled connection is acquired");
        let completed_before_drop = pool.completed_driver_count();
        drop(connection);
        assert_eq!(pool.pooled_count(), 0);
        assert_driver_completions(
            &pool,
            completed_before_drop + 1,
            "unused pooled borrow drop",
        )
        .await;
    }

    #[tokio::test]
    async fn stale_idle_sender_redials_once_before_the_next_request() {
        let port = start_close_after_response_peer().await;
        let adapter = Arc::new(CountingPassthroughAdapter::default());
        let pool = PeerConnectionPool::new(PeerPoolConfig::default(), adapter.clone());
        let peer = "127.0.0.1".parse().expect("peer authority");

        let mut first = pool
            .acquire(
                &peer,
                std::num::NonZeroU16::new(port).expect("non-zero test port"),
                atm_core::api::RequestDeadline::after(std::time::Duration::from_secs(1)),
            )
            .await
            .expect("first connection opens");
        complete_one_request(&mut first).await;
        drop(first);
        assert_driver_completions(&pool, 1, "server-requested close").await;

        let mut redialed = pool
            .acquire(
                &peer,
                std::num::NonZeroU16::new(port).expect("non-zero test port"),
                atm_core::api::RequestDeadline::after(std::time::Duration::from_secs(1)),
            )
            .await
            .expect("stale idle entry redials before a new exchange");
        complete_one_request(&mut redialed).await;
        assert_eq!(adapter.connects.load(Ordering::SeqCst), 2);
        drop(redialed);
        pool.shutdown(std::time::Duration::from_secs(1)).await;
    }

    #[tokio::test]
    async fn expired_deadline_releases_idle_reservation_without_probe_or_redial() {
        let port = start_keep_alive_peer().await;
        let adapter = Arc::new(CountingPassthroughAdapter::default());
        let pool = PeerConnectionPool::new(PeerPoolConfig::default(), adapter.clone());
        let peer = "127.0.0.1".parse().expect("peer authority");
        let mut initial = pool
            .acquire(
                &peer,
                std::num::NonZeroU16::new(port).expect("non-zero test port"),
                atm_core::api::RequestDeadline::after(std::time::Duration::from_secs(1)),
            )
            .await
            .expect("initial connection is pooled");
        complete_one_request(&mut initial).await;
        drop(initial);

        let result = pool
            .acquire(
                &peer,
                std::num::NonZeroU16::new(port).expect("non-zero test port"),
                atm_core::api::RequestDeadline::after(std::time::Duration::ZERO),
            )
            .await;
        let failure = match result {
            Ok(_) => panic!("expired budget rejects liveness probing before a redial"),
            Err(failure) => failure,
        };
        assert!(matches!(
            failure,
            super::HttpRuntimeClientFailure::PeerConnectTimeout { .. }
        ));
        assert_eq!(
            adapter.connects.load(Ordering::SeqCst),
            1,
            "expired acquire neither probes into a replacement dial nor connects again"
        );
        assert_eq!(
            pool.pooled_count(),
            0,
            "the popped idle reservation is released when no request budget remains"
        );
    }

    #[tokio::test]
    async fn failed_exchange_is_not_retried_after_the_request_is_handed_to_the_sender() {
        let (port, accepted) = start_drop_peer().await;
        let adapter = Arc::new(CountingPassthroughAdapter::default());
        let pool = PeerConnectionPool::new(PeerPoolConfig::default(), adapter.clone());
        let peer = "127.0.0.1".parse().expect("peer authority");
        let mut connection = pool
            .acquire(
                &peer,
                std::num::NonZeroU16::new(port).expect("non-zero test port"),
                atm_core::api::RequestDeadline::after(std::time::Duration::from_secs(1)),
            )
            .await
            .expect("TCP and HTTP/1 setup completes before the peer drop is observed");

        let failure = connection
            .exchange(
                request(),
                atm_core::api::RequestDeadline::after(std::time::Duration::from_secs(1)),
            )
            .await
            .expect_err("peer close during request is surfaced");
        assert!(matches!(
            failure,
            super::HttpRuntimeClientFailure::PeerConnect { .. }
        ));
        drop(connection);
        assert_driver_completions(&pool, 1, "failed exchange drop").await;
        assert_eq!(adapter.connects.load(Ordering::SeqCst), 1);
        assert_eq!(accepted.load(Ordering::SeqCst), 1);
        assert_eq!(pool.pooled_count(), 0);
    }

    #[tokio::test]
    async fn stale_redial_failure_releases_the_existing_reservation() {
        let (port, shutdown_listener, listener_stopped) =
            start_close_peer_with_listener_shutdown().await;
        let adapter = Arc::new(CountingPassthroughAdapter::default());
        let pool = PeerConnectionPool::new(PeerPoolConfig::default(), adapter.clone());
        let peer = "127.0.0.1".parse().expect("peer authority");
        let mut first = pool
            .acquire(
                &peer,
                std::num::NonZeroU16::new(port).expect("non-zero test port"),
                atm_core::api::RequestDeadline::after(std::time::Duration::from_secs(1)),
            )
            .await
            .expect("initial pooled connection opens");
        complete_one_request(&mut first).await;
        drop(first);
        assert_driver_completions(&pool, 1, "stale seed connection").await;
        shutdown_listener
            .send(())
            .expect("listener shutdown requested");
        listener_stopped
            .await
            .expect("listener closed before redial");

        let result = pool
            .acquire(
                &peer,
                std::num::NonZeroU16::new(port).expect("non-zero test port"),
                atm_core::api::RequestDeadline::after(std::time::Duration::from_secs(1)),
            )
            .await;
        let failure = match result {
            Ok(_) => panic!("replacement dial fails after listener shutdown"),
            Err(error) => error,
        };
        assert!(matches!(
            failure,
            super::HttpRuntimeClientFailure::PeerConnect { .. }
                | super::HttpRuntimeClientFailure::PeerConnectTimeout { .. }
        ));
        assert_eq!(
            adapter.connects.load(Ordering::SeqCst),
            1,
            "the replacement TCP dial failed before it could invoke the stream adapter"
        );
        assert_eq!(
            pool.pooled_count(),
            0,
            "redial failure releases the seed reservation"
        );
    }

    #[test]
    fn pool_config_rejects_zero_capacity_and_zero_idle_timeout() {
        for config in [
            PeerPoolConfig {
                max_per_peer: 0,
                ..PeerPoolConfig::default()
            },
            PeerPoolConfig {
                max_pooled_total: 0,
                ..PeerPoolConfig::default()
            },
            PeerPoolConfig {
                idle_timeout: std::time::Duration::ZERO,
                ..PeerPoolConfig::default()
            },
        ] {
            assert!(config.validate().is_err());
        }
    }
}
