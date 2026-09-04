//! Address-family-aware dialing of direct peer authorities.
//!
//! A peer is one registered hostname (`REQ-CORE-TRANSPORT-002D`). The name is
//! forward-resolved through the operating-system resolver (DNS, DDNS, or mDNS
//! for `.local` names) at connect time; resolved addresses are never stored
//! durably or treated as an alias for the name. This module owns everything
//! between that lookup and an established TCP stream, and is shared by the
//! pooled mTLS connector and the plaintext-test `reqwest` connector so both
//! wire-security modes dial identically.

use std::collections::HashMap;
use std::future::Future;
use std::net::{SocketAddr, SocketAddrV6};
use std::num::NonZeroU16;
use std::sync::Mutex;
use std::time::Duration;

use atm_core::api::RequestDeadline;
use atm_core::types::HostName;
use tokio::net::TcpStream;
use tokio::time::Instant;

/// Upper bound on addresses dialled for one peer. A resolver answer larger
/// than this would otherwise split the request budget into shares too small
/// for any single address to connect, defeating the per-attempt bound.
pub(crate) const MAX_DIAL_CANDIDATES: usize = 4;

/// Slice of the request budget reserved *out of* the dial loop's deadline so
/// the loop finishes, and reports its per-address diagnosis, before the
/// caller's request timeout fires at the full deadline. The total stays
/// bounded by the request budget; when the budget is at or below the grace
/// the loop simply uses what remains.
pub(crate) const DIAL_REPORT_GRACE: Duration = Duration::from_millis(250);

/// Longest a dial against *cached* addresses may run before the name is
/// resolved again. A live LAN or VPN address connects in a few milliseconds,
/// so a healthy peer never feels this bound; a peer that moved gives up its
/// old address early enough that the fresh mDNS lookup, which can take up to
/// about two seconds with retransmits, still fits inside the request budget.
pub(crate) const STALE_ADDRESS_DIAL_CAP: Duration = Duration::from_millis(500);

/// Resolved dial order for one direct peer authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConnectCandidates {
    pub(crate) usable: Vec<SocketAddr>,
    pub(crate) skipped_link_local: usize,
    pub(crate) truncated: usize,
}

/// Orders resolved addresses so a dial loop never stalls on an address the
/// operating system cannot route.
///
/// macOS multicast DNS answers a `.local` peer name with both an IPv4 address
/// and a scope-less link-local IPv6 address (`fe80::/10` with no `%interface`
/// scope). The kernel cannot route the scope-less form, so a plain
/// `TcpStream::connect(host)` either fails immediately when it is the only
/// answer, or stalls for the whole request budget when it is tried first.
/// Such candidates are dropped, IPv4 candidates are dialled before the
/// remaining IPv6 candidates, and the list is capped at
/// [`MAX_DIAL_CANDIDATES`]. Scoped link-local addresses stay usable.
pub(crate) fn order_connect_candidates(
    resolved: impl IntoIterator<Item = SocketAddr>,
) -> ConnectCandidates {
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
    let truncated = ipv4.len().saturating_sub(MAX_DIAL_CANDIDATES);
    ipv4.truncate(MAX_DIAL_CANDIDATES);
    ConnectCandidates {
        usable: ipv4,
        skipped_link_local,
        truncated,
    }
}

fn is_scope_less_link_local(address: SocketAddrV6) -> bool {
    address.ip().is_unicast_link_local() && address.scope_id() == 0
}

/// Dials each candidate in order and returns the first established stream.
///
/// Every attempt is bounded by an even share of the remaining request budget
/// across the candidates still untried, so one unresponsive address can never
/// consume the budget that a later, reachable address needs. The returned
/// cause names every attempt so the operator can see which address failed
/// and why.
pub(crate) async fn dial_candidates<Connect, Dial>(
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
        truncated,
    } = candidates;
    let notes = || {
        let mut notes = Vec::new();
        if skipped_link_local > 0 {
            notes.push(format!(
                "{skipped_link_local} scope-less link-local IPv6 address(es) skipped as unroutable"
            ));
        }
        if truncated > 0 {
            notes.push(format!(
                "{truncated} further resolved address(es) not tried (limit {MAX_DIAL_CANDIDATES})"
            ));
        }
        notes
    };
    if usable.is_empty() {
        let notes = notes();
        return Err(if notes.is_empty() {
            "the host resolved to no addresses".to_owned()
        } else {
            format!(
                "the host resolved to no routable address ({})",
                notes.join("; ")
            )
        });
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
    failures.extend(notes());
    Err(failures.join("; "))
}

/// Forward resolution of the registered peer hostname through the operating
/// system resolver (DNS, DDNS, or mDNS for `.local` names).
pub(crate) async fn resolve_peer_addresses(
    peer: HostName,
    port: NonZeroU16,
) -> std::io::Result<Vec<SocketAddr>> {
    tokio::net::lookup_host((peer.as_str(), port.get()))
        .await
        .map(Iterator::collect)
}

/// Cache key for one peer: the `.local` suffix is a resolver implementation
/// detail and host names compare ASCII-case-insensitively (ADR-040), so
/// `rand-m5`, `rand-m5.local`, and `RAND-M5.local` share one entry.
fn cache_key(peer: &HostName, port: NonZeroU16) -> (String, NonZeroU16) {
    let name = peer.as_str().to_ascii_lowercase();
    let name = name
        .strip_suffix(".local")
        .map_or(name.as_str(), |bare| bare)
        .to_owned();
    (name, port)
}

/// Short-term, in-memory memory of where a peer name last resolved.
///
/// This is process memory only: resolved addresses are never persisted or
/// treated as an alias for the registered hostname
/// (`REQ-CORE-TRANSPORT-002D`). When cached addresses no longer accept a
/// connection, the name is resolved again and dialled again inside the same
/// request budget, so a peer whose address changed is reached without an
/// error surfacing to the caller.
pub(crate) struct PeerAddressCache {
    ttl: Duration,
    entries: Mutex<HashMap<(String, NonZeroU16), CachedAddresses>>,
}

#[derive(Debug, Clone)]
struct CachedAddresses {
    candidates: ConnectCandidates,
    resolved_at: Instant,
}

impl PeerAddressCache {
    pub(crate) fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn fresh(&self, peer: &HostName, port: NonZeroU16) -> Option<ConnectCandidates> {
        let entries = self.entries.lock().expect("peer address cache");
        entries
            .get(&cache_key(peer, port))
            .filter(|entry| entry.resolved_at.elapsed() < self.ttl)
            .map(|entry| entry.candidates.clone())
    }

    pub(crate) fn store(&self, peer: &HostName, port: NonZeroU16, candidates: ConnectCandidates) {
        let mut entries = self.entries.lock().expect("peer address cache");
        entries.insert(
            cache_key(peer, port),
            CachedAddresses {
                candidates,
                resolved_at: Instant::now(),
            },
        );
    }

    /// Test hook: makes an entry look `by` older than it is, so TTL expiry is
    /// exercised without sleeping.
    #[cfg(test)]
    fn backdate(&self, peer: &HostName, port: NonZeroU16, by: Duration) {
        let mut entries = self.entries.lock().expect("peer address cache");
        if let Some(entry) = entries.get_mut(&cache_key(peer, port)) {
            entry.resolved_at = entry
                .resolved_at
                .checked_sub(by)
                .expect("backdated instant stays representable");
        }
    }

    fn forget(&self, peer: &HostName, port: NonZeroU16) {
        let mut entries = self.entries.lock().expect("peer address cache");
        entries.remove(&cache_key(peer, port));
    }

    /// Connects to `peer`, reusing a fresh cached address set when one exists
    /// and otherwise resolving the name through `resolve`. A dial failure on
    /// cached addresses falls through to one fresh resolution and dial.
    pub(crate) async fn connect<Resolve, Resolving, Connect, Dial>(
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
        if let Some(cached) = self.fresh(peer, port) {
            // Cached addresses may be stale, so they get the smaller of half
            // the remaining budget and `STALE_ADDRESS_DIAL_CAP`; the rest is
            // reserved for the fresh lookup and dial that follow when they no
            // longer answer.
            let cached_deadline = deadline.remaining().map_or(deadline, |remaining| {
                RequestDeadline::after((remaining / 2).min(STALE_ADDRESS_DIAL_CAP))
            });
            match dial_candidates(cached, cached_deadline, connect).await {
                Ok(stream) => return Ok(stream),
                Err(cause) => {
                    tracing::info!(
                        %peer,
                        %cause,
                        "cached peer addresses no longer connect; resolving the peer name again"
                    );
                    self.forget(peer, port);
                }
            }
        }

        let remaining = deadline
            .remaining()
            .ok_or_else(|| "request budget elapsed before the peer name was resolved".to_owned())?;
        let resolved = tokio::time::timeout(remaining, resolve(peer.clone(), port))
            .await
            .map_err(|_| {
                format!("DNS resolution of `{peer}` did not finish within the request budget")
            })?
            .map_err(|error| format!("DNS resolution of `{peer}` failed: {error}"))?;
        let candidates = order_connect_candidates(resolved);
        self.store(peer, port, candidates.clone());
        match dial_candidates(candidates, deadline, connect).await {
            Ok(stream) => Ok(stream),
            Err(cause) => {
                self.forget(peer, port);
                Err(cause)
            }
        }
    }
}

/// `reqwest` resolver for the plaintext-test peer client so it dials the same
/// ordered, link-local-free address list as the pooled connector.
///
/// hyper's connector prefers the address family of the first returned entry
/// and races the other family after a short delay, so returning IPv4 first
/// both prefers IPv4 and keeps a dead IPv6 answer from stalling the connect.
/// No private cache is layered here: the operating-system resolver cache
/// already holds the most recent answer and serves it in microseconds.
pub(crate) struct OrderedPeerResolver;

impl reqwest::dns::Resolve for OrderedPeerResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let name = name.as_str().to_owned();
        Box::pin(async move {
            let resolved = tokio::net::lookup_host((name.as_str(), 0)).await?;
            let candidates = order_connect_candidates(resolved);
            if candidates.skipped_link_local > 0 || candidates.truncated > 0 {
                tracing::debug!(
                    %name,
                    skipped_link_local = candidates.skipped_link_local,
                    truncated = candidates.truncated,
                    "resolved peer addresses filtered before dial"
                );
            }
            let addresses: reqwest::dns::Addrs = Box::new(candidates.usable.into_iter());
            Ok(addresses)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::num::NonZeroU16;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use atm_core::api::RequestDeadline;
    use atm_core::types::HostName;
    use tokio::net::{TcpListener, TcpStream};

    use super::{
        ConnectCandidates, MAX_DIAL_CANDIDATES, OrderedPeerResolver, PeerAddressCache,
        STALE_ADDRESS_DIAL_CAP, dial_candidates, order_connect_candidates,
    };

    async fn start_listener() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let _keep_open = stream;
                    std::future::pending::<()>().await;
                });
            }
        });
        port
    }

    fn deadline(duration: Duration) -> RequestDeadline {
        RequestDeadline::after(duration)
    }

    fn v6(address: &str, port: u16, scope_id: u32) -> SocketAddr {
        SocketAddr::V6(std::net::SocketAddrV6::new(
            address.parse().expect("ipv6 literal"),
            port,
            0,
            scope_id,
        ))
    }

    fn v4(address: &str, port: u16) -> SocketAddr {
        SocketAddr::V4(std::net::SocketAddrV4::new(
            address.parse().expect("ipv4 literal"),
            port,
        ))
    }

    fn host(name: &str) -> HostName {
        name.parse().expect("peer host")
    }

    fn port(value: u16) -> NonZeroU16 {
        NonZeroU16::new(value).expect("non-zero port")
    }

    /// Dials loopback for real and never answers any other address, which is
    /// how an unroutable link-local or black-holed address behaves.
    fn loopback_only_connector(
        address: SocketAddr,
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

    type ResolverAnswer = std::pin::Pin<
        Box<dyn std::future::Future<Output = std::io::Result<Vec<SocketAddr>>> + Send>,
    >;

    fn counting_resolver(
        answers: Vec<SocketAddr>,
        calls: Arc<AtomicUsize>,
    ) -> impl Fn(HostName, NonZeroU16) -> ResolverAnswer {
        let answers = Arc::new(Mutex::new(answers));
        move |_peer, _port| {
            calls.fetch_add(1, Ordering::SeqCst);
            let answer = answers.lock().expect("resolver answers").clone();
            Box::pin(async move { Ok(answer) })
        }
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
                truncated: 0,
            },
            "IPv4 first, then routable IPv6; scoped link-local stays usable"
        );
    }

    #[test]
    fn dial_order_is_capped_so_the_budget_share_stays_usable() {
        let answer: Vec<SocketAddr> = (1..=10).map(|i| v4(&format!("10.0.0.{i}"), 1)).collect();
        let candidates = order_connect_candidates(answer.clone());
        assert_eq!(candidates.usable, answer[..MAX_DIAL_CANDIDATES].to_vec());
        assert_eq!(candidates.truncated, 10 - MAX_DIAL_CANDIDATES);
    }

    #[tokio::test]
    async fn mixed_family_host_connects_without_stalling_on_link_local_ipv6() {
        let port = start_listener().await;
        let started = std::time::Instant::now();
        let stream = dial_candidates(
            order_connect_candidates([v6("fe80::4af:1", port, 0), v4("127.0.0.1", port)]),
            deadline(Duration::from_secs(10)),
            loopback_only_connector,
        )
        .await
        .expect("the routable IPv4 address must be reached");
        assert!(stream.peer_addr().is_ok());
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the unroutable IPv6 answer must not consume the request budget"
        );
    }

    #[tokio::test]
    async fn unresponsive_first_address_is_bounded_so_the_next_address_still_connects() {
        let port = start_listener().await;
        let started = std::time::Instant::now();
        let stream = dial_candidates(
            ConnectCandidates {
                usable: vec![v4("10.255.255.1", port), v4("127.0.0.1", port)],
                skipped_link_local: 0,
                truncated: 0,
            },
            deadline(Duration::from_millis(600)),
            loopback_only_connector,
        )
        .await
        .expect("the second address must be dialled within the same request budget");
        assert!(stream.peer_addr().is_ok());
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_millis(250) && elapsed < Duration::from_millis(600),
            "first attempt gets an even share of the budget, not all of it: {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn link_local_only_answer_fails_fast_with_an_actionable_cause() {
        let started = std::time::Instant::now();
        let cause = dial_candidates(
            order_connect_candidates([v6("fe80::4af:1", 43_101, 0)]),
            deadline(Duration::from_secs(10)),
            loopback_only_connector,
        )
        .await
        .expect_err("an unroutable-only answer must not be dialled");
        assert!(cause.contains("no routable address"), "{cause}");
        assert!(cause.contains("link-local IPv6"), "{cause}");
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn every_failed_attempt_is_named_in_the_cause() {
        let cause = dial_candidates(
            ConnectCandidates {
                usable: vec![v4("10.255.255.1", 43_101), v4("127.0.0.1", 1)],
                skipped_link_local: 1,
                truncated: 2,
            },
            deadline(Duration::from_millis(400)),
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
        assert!(
            cause.contains("2 further resolved address(es) not tried"),
            "{cause}"
        );
    }

    #[tokio::test]
    async fn cached_addresses_are_reused_within_the_ttl() {
        let listener_port = start_listener().await;
        let cache = PeerAddressCache::new(Duration::from_secs(300));
        let calls = Arc::new(AtomicUsize::new(0));
        let resolver = counting_resolver(vec![v4("127.0.0.1", listener_port)], calls.clone());
        for _ in 0..3 {
            cache
                .connect(
                    &host("rand-m5.local"),
                    port(listener_port),
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
    async fn bare_and_dot_local_names_share_one_cache_entry() {
        let listener_port = start_listener().await;
        let cache = PeerAddressCache::new(Duration::from_secs(300));
        let calls = Arc::new(AtomicUsize::new(0));
        let resolver = counting_resolver(vec![v4("127.0.0.1", listener_port)], calls.clone());
        for name in ["rand-m5.local", "rand-m5", "RAND-M5.local", "Rand-M5"] {
            cache
                .connect(
                    &host(name),
                    port(listener_port),
                    deadline(Duration::from_secs(5)),
                    &resolver,
                    loopback_only_connector,
                )
                .await
                .expect("loopback connects");
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "`.local` and ASCII case are irrelevant to the cache entry"
        );
        assert!(cache.fresh(&host("rand-m5"), port(listener_port)).is_some());
    }

    #[tokio::test]
    async fn changed_peer_address_is_re_resolved_and_reached_without_an_error() {
        let listener_port = start_listener().await;
        let cache = PeerAddressCache::new(Duration::from_secs(300));
        let calls = Arc::new(AtomicUsize::new(0));
        // The cache holds the address the peer had on its previous network;
        // it no longer responds. The resolver now answers with the new one.
        let resolver = counting_resolver(vec![v4("127.0.0.1", listener_port)], calls.clone());
        let peer = host("rand-m5.local");
        cache.store(
            &peer,
            port(listener_port),
            order_connect_candidates([v4("10.255.255.1", listener_port)]),
        );
        let started = std::time::Instant::now();
        let stream = cache
            .connect(
                &peer,
                port(listener_port),
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
            cache
                .fresh(&host("rand-m5"), port(listener_port))
                .map(|candidates| candidates.usable),
            Some(vec![v4("127.0.0.1", listener_port)]),
            "the fresh answer replaces the stale entry"
        );
    }

    #[tokio::test]
    async fn expired_cache_entries_are_resolved_again() {
        let listener_port = start_listener().await;
        let ttl = Duration::from_secs(300);
        let cache = PeerAddressCache::new(ttl);
        let calls = Arc::new(AtomicUsize::new(0));
        let resolver = counting_resolver(vec![v4("127.0.0.1", listener_port)], calls.clone());
        let peer = host("rand-m5.local");
        for _ in 0..2 {
            cache
                .connect(
                    &peer,
                    port(listener_port),
                    deadline(Duration::from_secs(5)),
                    &resolver,
                    loopback_only_connector,
                )
                .await
                .expect("loopback connects");
            cache.backdate(&peer, port(listener_port), ttl + Duration::from_millis(1));
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "an expired entry is not reused"
        );
    }

    #[tokio::test]
    async fn stale_cached_address_gives_up_within_the_dial_cap() {
        let listener_port = start_listener().await;
        let cache = PeerAddressCache::new(Duration::from_secs(300));
        let calls = Arc::new(AtomicUsize::new(0));
        let resolver = counting_resolver(vec![v4("127.0.0.1", listener_port)], calls.clone());
        let peer = host("rand-m5");
        // Black-holed old address in the cache, generous 3 s request budget:
        // half the budget would be 1.5 s, the cap makes it 500 ms.
        cache.store(
            &peer,
            port(listener_port),
            order_connect_candidates([v4("10.255.255.1", listener_port)]),
        );
        let started = std::time::Instant::now();
        cache
            .connect(
                &peer,
                port(listener_port),
                deadline(Duration::from_secs(3)),
                &resolver,
                loopback_only_connector,
            )
            .await
            .expect("the moved peer is reached");
        let elapsed = started.elapsed();
        assert!(
            elapsed >= STALE_ADDRESS_DIAL_CAP && elapsed < Duration::from_millis(1_000),
            "stale dial is bounded by the cap, not half the budget: {elapsed:?}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unreachable_peer_leaves_no_cache_entry_behind() {
        let cache = PeerAddressCache::new(Duration::from_secs(300));
        let calls = Arc::new(AtomicUsize::new(0));
        let resolver = counting_resolver(vec![v4("10.255.255.1", 43_101)], calls.clone());
        let cause = cache
            .connect(
                &host("rand-m5.local"),
                port(43_101),
                deadline(Duration::from_millis(200)),
                &resolver,
                loopback_only_connector,
            )
            .await
            .expect_err("nothing answers");
        assert!(cause.contains("10.255.255.1:43101"), "{cause}");
        assert!(cache.fresh(&host("rand-m5"), port(43_101)).is_none());
    }

    #[tokio::test]
    async fn reqwest_resolver_returns_ordered_routable_addresses() {
        use reqwest::dns::Resolve;
        let name: reqwest::dns::Name = "localhost".parse().expect("valid name");
        let addresses: Vec<SocketAddr> = OrderedPeerResolver
            .resolve(name)
            .await
            .expect("localhost resolves")
            .collect();
        assert!(!addresses.is_empty());
        assert!(addresses.len() <= MAX_DIAL_CANDIDATES);
        assert!(
            addresses[0].is_ipv4() || addresses.iter().all(SocketAddr::is_ipv6),
            "IPv4 leads whenever it is present: {addresses:?}"
        );
        assert!(addresses.iter().all(|address| match address {
            SocketAddr::V6(v6) => !(v6.ip().is_unicast_link_local() && v6.scope_id() == 0),
            SocketAddr::V4(_) => true,
        }));
    }
}
