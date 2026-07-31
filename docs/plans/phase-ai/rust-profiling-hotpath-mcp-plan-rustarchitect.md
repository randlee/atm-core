# Live Hot-Path Profiling for `atm-daemon` via hotpath.rs MCP

**Plan owner:** rust-architect (independent investigation)
**Target path:** `docs/plans/phase-ai/rust-profiling-hotpath-mcp-plan-rustarchitect.md`
**Repo:** `/Users/randlee/Documents/github/atm-core` (workspace version `1.4.0-beta-ai`, edition 2024, rust-version 1.94.1)
**Scope:** integrate [hotpath.rs](https://hotpath.rs/mcp) MCP server mode into `atm-daemon` so a running daemon can be interrogated conversationally for live performance data across (1) local UDS/TCP admission + dispatch, (2) read-path throughput, (3) cross-host/peer write delivery.

---

## 0. Executive summary and the one decision that shapes everything

`atm-daemon` is **fully synchronous and thread-based**. There is no `tokio` anywhere in `/Users/randlee/Documents/github/atm-core/crates/atm-daemon/Cargo.toml`; the entire ingress is `std::thread::scope` + `interprocess` + blocking `std::sync::mpsc` (`local_ipc_transport.rs:392`, `local_ipc_transport/accept_loop.rs:140`, `local_ipc_transport/request_worker.rs:204`). hotpath's core profiler is runtime-agnostic ("sync functions operate independently without requiring an async runtime"), so **the instrumentation half of this integration is a natural fit**. The MCP server half is an HTTP server that hotpath owns internally, and it is the only part that may drag an async runtime into a deliberately runtime-free daemon.

**Decision:** instrument broadly and permanently in source (attributes compile to no-ops), gate everything behind Cargo features that are **off by default and never enabled in a released binary**, and additionally require an **explicit CLI opt-in** at daemon startup before the MCP listener is bound — mirroring the existing `--peer-wire-security` precedent, which `crates/atm-daemon/src/lib.rs:146-149` documents as deliberately *not* readable from config or environment.

The single highest-risk finding in this investigation is not architectural, it is a two-line bug the integration would silently hit:

> `crates/atm-daemon/src/main.rs:20` calls `std::process::exit(exit_code)`.
> `std::process::exit` **does not run destructors**. `#[hotpath::main]` and `HotpathGuard` are RAII types whose `Drop` flushes the profile and tears down the MCP listener. Placing `#[hotpath::main]` on this `main()` as documented would produce **no report and no clean MCP shutdown, ever**.

The plan below restructures the entrypoint so the guard's scope closes before `process::exit` is reached.

---

## 1. Codebase grounding — what actually runs on each hot path

### 1.1 Unix local ingress (macOS/Linux)

```
PreparedRuntimeServer::serve_with_deadlines_and_accept_probe   local_ipc_transport.rs:341
  └─ thread::scope                                             local_ipc_transport.rs:392
       ├─ spawn_lifecycle_waiter  ("local-ipc-lifecycle-waiter")  :608
       ├─ start_tcp_loopback_server ("local-loopback-tcp-http")    :567   [unix only]
       └─ run_accept_loop                                          :675
            ├─ prepare_accept_iteration                            :696
            │    └─ registry.reap_finished_dispatches()   active_connection_registry.rs:155
            │    └─ listener.accept()                              :737
            └─ handle_accepted_stream                              :754
                 ├─ reject_connection_when_capped   accept_loop.rs:75   (cap = 64, :51)
                 └─ spawn_connection_worker         accept_loop.rs:127  ("local-ipc-connection-worker")
                      └─ handle_connection          request_worker.rs:40
                           ├─ RequestDeadline::after(3s)            :49
                           ├─ read_bounded_http_request             :225  ← spawns thread #2
                           ├─ decode_request                        :55
                           ├─ dispatch_request                      :157
                           │    ├─ spawn_dispatch_worker            :189  ← spawns thread #3
                           │    │    └─ dispatcher.route(...)  runtime_health.rs:857
                           │    ├─ registry.push_dispatch_handle    :169
                           │    └─ await_dispatch_response          :251  ← blocking recv_timeout
                           └─ write_http_response                   :71
```

Three OS threads are created **per admitted request** (connection worker, read worker, dispatch worker), plus three `sync_channel(1)` handoffs (`request_worker.rs:195`, `:196`, `:231`). At the documented 64-way cap this is the primary suspect for latency-under-fan-out, and it is exactly what hotpath's thread-activity and channel tools are built to quantify.

### 1.2 Windows / loopback-TCP local ingress

`crates/atm-daemon/src/local_tcp_transport.rs:92` `serve_until_terminated` is structurally different and this matters:

```rust
// local_tcp_transport.rs:102-110
match self.listener.accept() {
    Ok((stream, peer)) if peer.ip().is_loopback() => {
        handle_connection(stream, Arc::clone(&router), &self.capability, &AtomicBool::new(false))?;
    }
```

`handle_connection` is called **inline on the accept thread** — the loopback TCP transport serializes local requests, where the UDS transport fans out. Combined with the 25 ms `ACCEPT_POLL_INTERVAL` non-blocking poll (`:112-114`), Windows local latency and Unix local latency are governed by materially different mechanics. Profiling must cover both or the Windows numbers will be silently wrong.

Both transports converge on the same `ApiRouter`: `DaemonRequestDispatcher::route` (`runtime_health.rs:857`) → `dispatch_with_deadline` (`runtime_health.rs:549`).

### 1.3 Read path

`runtime_health.rs:598-603` routes `Peek`/`Receive` into `atm-core`:

```
peek_mail_with_runtime          read/mod.rs:368  →  peek_mail_with_runtime_impl  :376
read_mail_with_runtime          read/mod.rs:360  →  read_mail_with_runtime_impl  :440
   ├─ resolve_read_context                                                        :778
   │    └─ validate_target_member_in_roster                                       :814
   ├─ load_read_selection                                                         :535
   │    └─ query_mailbox_metadata_rows   atm-storage-rusqlite/src/mailbox_metadata.rs:205
   ├─ wait_for_selection_candidates                                               :641   (blocking wait)
   ├─ resolve_read_display                                                        :589
   │    ├─ filter_metadata_backed_contains_candidates   read/metadata_selection.rs
   │    ├─ selection_state_for_mailbox_metadata_rows    read/metadata_selection.rs
   │    └─ sort_and_limit_selected                      read/metadata_selection.rs
   ├─ output_messages_from_metadata_selection                                      :872
   └─ apply_display_mutations_to_store                                             :897   (write on read!)
```

Note `MAX_CONTAINS_FILTER_LEN = 1024` (`read/mod.rs:28`) — `--contains` filtering is an unindexed scan and the leading allocation suspect. And `apply_display_mutations_to_store` means a "read" is frequently a read-modify-write; hotpath will separate that cost cleanly.

### 1.4 Cross-host / peer write

```
PostWriteRouter::dispatch                 runtime_health/peer_delivery_router.rs:16
  ├─ self.peer_config_store.list_trusted_peers()   :40   ← storage read on EVERY peer-addressed write
  ├─ resolve_peer_authority                 runtime_health/peer_authority.rs:25
  ├─ record_peer_delivery_event             :43
  └─ deliver_to_peer                        :77
       └─ peer_delivery_coordinator.deliver_after_persist   peer_drain_coordinator.rs:541
            ├─ acquire (per-host lease)                      :163
            ├─ slots() — ONE global Mutex<BTreeMap<HostName, PeerDrainSlot>>  :134
            ├─ drain                                          :278
            │    ├─ page_for_peer                             :349
            │    ├─ decode_page_requests (JSON parse/page)    :366
            │    └─ deliver_current                           :415
            └─ release                                        :191
                 └─ HttpsTransport::deliver          https_transport.rs:187
                      └─ open_connection             https_transport.rs:242
                           ├─ resolve_peer_address (DNS, every connection)  :253
                           ├─ TcpStream::connect_timeout                    :255
                           ├─ client_config(identity, peer)  ← rebuilt per connection :266
                           ├─ ClientConnection::new                          :273
                           └─ complete_handshake_with_deadline               :281
```

Two structural observations worth quantifying before optimizing:

* `open_connection` is called **per `deliver`** (`https_transport.rs:193`). Only `deliver_page` (`:203`) amortizes a connection across requests. There is no pooling, so each single-message cross-host send pays full DNS + TCP + rustls `ServerConfig`/`ClientConfig` construction + full TLS handshake.
* `client_config(identity, peer)` at `:266` reconstructs the rustls client config inside the connection path. rustls config construction is expensive relative to a handshake and is trivially cacheable.

These are precisely the claims hotpath's MCP mode lets you settle conversationally against a live daemon instead of arguing about in review.

### 1.5 Constraints the integration must respect

| Constraint | Location | Implication |
|---|---|---|
| `#![deny(unsafe_code)]` | `atm-daemon/src/lib.rs:3` | any `#[global_allocator]` wiring for `hotpath-alloc` needs an explicit, reasoned `#[expect]` per M-LINT-OVERRIDE-EXPECT |
| `std::process::exit` in `main` | `atm-daemon/src/main.rs:20` | RAII profiler guard would never drop (see §0) |
| Bounded shutdown contract | `composition.rs:410-411`, `lib.rs:72-73` (`GRACEFUL_DRAIN_DEADLINE=2s`, `FORCE_CANCEL_DEADLINE=3s`) | an MCP listener is *outside* this contract and must be stopped before drain begins |
| Host-singleton daemon | ADR-002, `host_ownership.rs` | one daemon per host, so no self-collision on 6771; a second hotpath process on the host still collides |
| `warn!`/`error!` need `subsystem=`/`action=`/`outcome=` | guidelines.txt:7-10 | every profiling-subsystem log line must carry them |
| `just lint` gates: `cargo-shear`, `cargo-deny`, `cargo-modules`, boundary/portability/env-var lints | `justfile:34-130` | optional/cfg-gated deps and a new module both interact with these |
| `deny.toml` `[graph] all-features = false` | `deny.toml:2` | hotpath's transitive graph would be **invisible** to `cargo deny` by default — a real supply-chain blind spot |
| CI runs `cargo clippy --workspace --all-targets -- -D warnings` with **no feature matrix** | `.github/workflows/ci.yml:109` | the instrumented build would never be compiled in CI and would bit-rot within weeks |
| Env-var boundary lint covers only `ATM_TEAM`/`ATM_IDENTITY` | `.just/check_env_var_boundary.py:2-24` | `HOTPATH_MCP_*` env reads are not blocked by lint, but see §4 for why we still don't want them to be the only gate |

---

## 2. Instrumentation map — concrete functions and spans

hotpath's macro surface used here: `#[measure]`, `measure_block!`, `channel!`, `mutex!`, `gauge!`. Deliberately **not** used: `val!`, `dbg!`, `http!`, `io!`, `stream!`, `future!` (see §4.3 on data exposure).

Naming convention for every `measure_block!` / `gauge!` label: `atm.<subsystem>.<action>` — matching the existing structured-logging convention (`subsystem=`, `action=`) so MCP answers line up with retained-log projections. **All labels must be `'static` string literals with bounded cardinality.** Never interpolate an agent name, team name, peer hostname, message body, message id, or SQL text into a label.

### 2.1 Scope 1 — local UDS/TCP admission + dispatch

| Target | File:line | Instrumentation | Question it answers |
|---|---|---|---|
| `prepare_accept_iteration` | `local_ipc_transport.rs:696` | `#[measure]` | how much accept-loop time is reap/bookkeeping vs. blocking in `accept()` |
| `handle_accepted_stream` | `local_ipc_transport.rs:754` | `#[measure]` | admission-decision cost per connection |
| `reject_connection_when_capped` | `accept_loop.rs:75` | `#[measure]` | is the 64-cap actually being hit, and what does rejection cost |
| `spawn_connection_worker` | `accept_loop.rs:127` | `#[measure]` | OS thread-spawn cost under fan-out |
| `handle_connection` (UDS) | `request_worker.rs:40` | `#[measure]` | end-to-end per-connection service time |
| `read_bounded_http_request` | `request_worker.rs:225` | `#[measure]` | cost of the *second* thread spawned per request just to bound a read |
| `dispatch_request` | `request_worker.rs:157` | `#[measure]` | dispatch orchestration overhead |
| `spawn_dispatch_worker` | `request_worker.rs:189` | `#[measure]` | cost of the *third* thread per request |
| `await_dispatch_response` | `request_worker.rs:251` | `#[measure]` | **queue wait vs. work time** — the single most valuable split on this path |
| dispatch result channel | `request_worker.rs:195` | `channel!("atm.local_ipc.dispatch_result", ...)` | handoff latency / depth on the `sync_channel(1)` |
| dispatch completion channel | `request_worker.rs:196` | `channel!("atm.local_ipc.dispatch_completion", ...)` | shutdown-accounting handoff |
| bounded-read channel | `request_worker.rs:231` | `channel!("atm.local_ipc.request_read", ...)` | read-worker handoff |
| `reap_finished_dispatches` | `active_connection_registry.rs:155` | `#[measure]` | per-iteration reap cost as tracked handles accumulate |
| `lock_dispatch_handles` | `active_connection_registry.rs:116` | `mutex!` | **contention on the single dispatch-handle mutex at 64-way fan-out** |
| `active_connections()` / `active_work_items()` | `active_connection_registry.rs:103`, `:107` | `gauge!("atm.local_ipc.active_connections")`, `gauge!("atm.local_ipc.active_work_items")`, sampled once per accept iteration | live headroom against `MAX_CONCURRENT_CONNECTIONS` (`:51`) |
| `push_dispatch_handle` | `active_connection_registry.rs:124` | `#[measure]` | bounded-registry insert cost |
| `DaemonRequestDispatcher::route` | `runtime_health.rs:857` | `#[measure]` | shared convergence point for both transports |
| `dispatch_with_deadline` | `runtime_health.rs:549` | `#[measure]` | per-verb routing cost |
| `route_write` | `runtime_health.rs:560` | `#[measure]` | write-path entry (feeds scope 3) |
| `serve_until_terminated` (TCP) | `local_tcp_transport.rs:92` | `measure_block!("atm.local_tcp.accept_poll")` around the `accept()`/sleep arm | how much of the loop is the 25 ms idle poll |
| `handle_connection` (TCP) | `local_tcp_transport.rs:312` | `#[measure]` | **serialized** Windows-path service time vs. fanned-out Unix path |

Deliberately **not** instrumented: `ServeLoopSignals::record_accept_error` (`local_ipc_transport.rs:75`), `SocketEndpointGuard::unpublish` (`:117`), `local_ipc_wake.rs` — all cold control-plane paths where instrumentation is pure noise.

### 2.2 Scope 2 — read-path throughput

| Target | File:line | Instrumentation |
|---|---|---|
| `read_mail_with_runtime` | `atm-core/src/read/mod.rs:360` | `#[measure]` |
| `peek_mail_with_runtime` | `read/mod.rs:368` | `#[measure]` |
| `peek_mail_with_runtime_impl` | `read/mod.rs:376` | `#[measure]` |
| `read_mail_with_runtime_impl` | `read/mod.rs:440` | `#[measure]` |
| `resolve_read_context` | `read/mod.rs:778` | `#[measure]` |
| `validate_target_member_in_roster` | `read/mod.rs:814` | `#[measure]` — per-read roster lookup |
| `load_read_selection` | `read/mod.rs:535` | `#[measure]` — storage query entry |
| `wait_for_selection_candidates` | `read/mod.rs:641` | `#[measure]` — **must be read as wait time, not work time**; document this in the module doc so MCP answers aren't misread |
| `resolve_read_display` | `read/mod.rs:589` | `#[measure]` |
| `build_unmodified_read_display` / `build_mutated_read_display` | `read/mod.rs:696`, `:725` | `#[measure]` — isolates the read-only vs. read-modify-write split |
| `apply_display_mutations_to_store` | `read/mod.rs:897` | `#[measure]` — write amplification on read |
| `output_messages_from_metadata_selection` | `read/mod.rs:872` | `#[measure]` |
| `load_checked_read_metadata` | `read/mod.rs:862` | `#[measure]` |
| `filter_metadata_backed_contains_candidates` | `read/metadata_selection.rs` | `#[measure]` — `--contains` scan, top allocation suspect |
| `selection_state_for_mailbox_metadata_rows` | `read/metadata_selection.rs` | `#[measure]` |
| `sort_and_limit_selected` | `read/metadata_selection.rs` | `#[measure]` |
| `load_seen_watermark` | `atm-core/src/read/seen_state.rs:15` | `#[measure]` |
| `query_mailbox_metadata_rows` | `atm-storage-rusqlite/src/mailbox_metadata.rs:205` | `#[measure]` + inner `measure_block!("atm.sql.mailbox_metadata_rows")` |
| `load_message` | `atm-storage-rusqlite/src/lib.rs:299` | `#[measure]` + `measure_block!("atm.sql.load_message")` |
| `list_messages` | `atm-storage-rusqlite/src/lib.rs:350` | `#[measure]` + `measure_block!("atm.sql.list_messages")` |

**SQL note:** hotpath's SQL tooling targets `sqlx`/`diesel`. This workspace uses `rusqlite` (`crates/atm-storage-rusqlite`), which hotpath does not natively hook. Use `measure_block!` with **static** labels per query site. Do **not** attempt to feed the rendered SQL string into hotpath — it can carry message bodies and agent identities into an unauthenticated-ish HTTP surface (§4.3).

`mailbox/mod.rs:68` / `:92` (`load_compat_mailbox_*`) should be instrumented **only after confirming the compat path is still reachable in the SQLite-backed runtime**; if it is dead, instrumenting it is misleading. Open question OQ-7.

### 2.3 Scope 3 — cross-host / peer write delivery

| Target | File:line | Instrumentation |
|---|---|---|
| `PostWriteRouter::dispatch` | `runtime_health/peer_delivery_router.rs:16` | `#[measure]` |
| trusted-peer lookup | `peer_delivery_router.rs:40` | `measure_block!("atm.peer.trusted_peer_lookup", { ... })` — **storage read per peer-addressed write; prime caching candidate** |
| `resolve_peer_authority` | `runtime_health/peer_authority.rs:25` | `#[measure]` |
| `deliver_to_peer` | `peer_delivery_router.rs:77` | `#[measure]` |
| `record_peer_delivery_failure` | `peer_delivery_router.rs:100` | `#[measure]` |
| `deliver_after_persist` | `peer_drain_coordinator.rs:541` | `#[measure]` |
| `acquire` / `release` | `peer_drain_coordinator.rs:163`, `:191` | `#[measure]` — per-host lease wait |
| `slots()` mutex | `peer_drain_coordinator.rs:134` | `mutex!` — **one global mutex guards every peer's slot; contention risk** |
| `drain` | `peer_drain_coordinator.rs:278` | `#[measure]` |
| `page_for_peer` | `peer_drain_coordinator.rs:349` | `#[measure]` |
| `decode_page_requests` | `peer_drain_coordinator.rs:366` | `#[measure]` — JSON parse per page, allocation suspect |
| `deliver_current` | `peer_drain_coordinator.rs:415` | `#[measure]` |
| `run_scheduled_recovery` | `peer_drain_coordinator.rs:507` | `#[measure]` |
| slot occupancy | `peer_drain_coordinator.rs:143` `reserve_slot` | `gauge!("atm.peer.drain_slots_occupied")` |
| `HttpsTransport::deliver` | `https_transport.rs:187` | `#[measure]` |
| `HttpsTransport::deliver_page` | `https_transport.rs:197` | `#[measure]` — amortization comparison against `deliver` |
| `HttpsPeerConnection::deliver` | `https_transport.rs:218` | `#[measure]` |
| `open_connection` | `https_transport.rs:242` | `#[measure]`, decomposed with `measure_block!`: `atm.peer.dns_resolve` (`:253`), `atm.peer.tcp_connect` (`:255`), `atm.peer.tls_config_build` (`:266`), `atm.peer.tls_client_new` (`:273`), `atm.peer.tls_handshake` (`:281`) |

That five-way decomposition of `open_connection` is the deliverable that most directly justifies this whole integration: it turns "cross-host sends feel slow" into a per-phase millisecond budget you can query live.

---

## 3. Cargo feature strategy

### 3.1 The dependency shape

hotpath is explicitly designed for this: *"all the lib dependencies are optional (i.e. not compiled) and all macros are noop unless profiling is enabled."* That gives two viable shapes, and they should be chosen **differently per crate** based on whether the crate is published for third-party consumption.

**`atm-daemon` (binary + private lib) — non-optional dependency, bare attributes.**

```toml
# crates/atm-daemon/Cargo.toml
[dependencies]
hotpath = { version = "0.22", default-features = false }

[features]
# Off by default. Never enabled in a released artifact.
# `hotpath` alone gives timing/call-stats with no server and no allocator hook.
hotpath = ["hotpath/hotpath", "agent-team-mail-core/hotpath", "atm-storage-rusqlite/hotpath"]
# Adds the MCP HTTP server (default port 6771). Requires `hotpath`.
hotpath-mcp = ["hotpath", "hotpath/hotpath-mcp"]
# Global-allocator wrapper. Process-wide; deliberately NOT part of `hotpath`.
hotpath-alloc = ["hotpath", "hotpath/hotpath-alloc"]
# CPU sampling. Platform support must be verified per-OS before use (OQ-5).
hotpath-cpu = ["hotpath", "hotpath/hotpath-cpu"]
```

Rationale: `atm-daemon` is an application crate, so `#[hotpath::measure]` can be written bare with no `cfg_attr` wrapper. The hot paths stay readable, the attributes are always type-checked by the normal `cargo clippy --workspace` leg, and they cannot bit-rot. When the features are off, hotpath compiles to nothing beyond an empty façade crate.

**`agent-team-mail-core` and `atm-storage-rusqlite` (published libraries) — optional dependency, `cfg_attr`.**

```toml
# crates/atm-core/Cargo.toml
[features]
test-utils = []
hotpath = ["dep:hotpath"]

[dependencies]
hotpath = { version = "0.22", default-features = false, optional = true }

[package.metadata.cargo-shear]
ignored = ["hotpath"]   # only referenced from cfg_attr attribute position
```

```rust
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn read_mail_with_runtime(/* ... */) -> Result<ReadOutcome, AtmError> { /* ... */ }
```

Rationale: `agent-team-mail-core` is published to crates.io with `documentation = "https://docs.rs/agent-team-mail-core"` (`crates/atm-core/Cargo.toml:11`). Per M-OOBE, we must not push a profiling dependency onto every downstream consumer of the library just to profile our own daemon. The `cfg_attr` noise is bounded to ~20 attributes on `atm-core` and 3 on `atm-storage-rusqlite`, which is acceptable; it is *not* acceptable across the daemon's ~40 sites, hence the split.

This is a deliberate, asymmetric choice. Do not "normalize" it later without re-reading this section.

### 3.2 Dev-only vs. always-available — the decision

**Decision: always-available-in-manifest, off-by-default at compile time, and additionally opt-in at runtime.**

Rejected alternatives and why:

* **`[dev-dependencies]` only.** Structurally impossible: dev-dependencies are not available to `src/main.rs` or `src/lib.rs` in a normal build, so the daemon binary could never be built with profiling. Also would exclude the instrumentation from `cargo clippy --workspace --all-targets`, guaranteeing rot.
* **A separate `atm-daemon-profiling` crate.** M-SMALLER-CRATES favours splitting, but the instrumentation is *inline attributes on private functions*, which cannot live in another crate. A separate crate would only hold the MCP bootstrap — not worth a workspace member.
* **Enabled by default with a runtime kill switch.** Rejected on security grounds (§4). A default-on profiling HTTP server on a long-lived host-singleton daemon that also terminates mTLS cross-host traffic is not a defensible default.

Consequences that must be handled:

* **CI must build the feature or it will rot.** `.github/workflows/ci.yml:109` runs clippy with no features. Add one Linux-only leg:
  ```yaml
  - name: Clippy (profiling features)
    run: cargo clippy -p atm-daemon --features hotpath-mcp --all-targets -- -D warnings
  ```
  Linux-only is sufficient and keeps macOS/Windows CI time flat; the instrumentation is platform-neutral except the `local_tcp_transport.rs` sites, which are `cfg(any(unix, windows, test))` and therefore still compiled on Linux.
* **`cargo deny` blindness.** `deny.toml:2` sets `all-features = false`, so hotpath's transitive graph (axum/tokio/etc. behind `hotpath-mcp`) is never license- or advisory-scanned. Add a dedicated `cargo deny --all-features check` invocation in `.just/lint_cargo_deny.py`, or at minimum a one-time audited snapshot of the `--features hotpath-mcp` graph recorded in this plan's follow-up. `deny.toml:21-31` allows only Apache-2.0/MIT/0BSD/Unicode-3.0/Zlib/BSD-3-Clause/ISC — a single MPL or BSD-2 transitive dep would fail the gate. Must be verified before merge (OQ-3).
* **`cargo shear`** (`justfile:42`) will flag an optional dep referenced only from attribute position. Handle with `[package.metadata.cargo-shear] ignored = ["hotpath"]` in the two library manifests.
* **`cargo modules`** (`justfile:34`) will see the new `profiling` module; expect a transcript update.
* **M-FEATURES-ADDITIVE** holds: none of these features add, remove, or change a public item. Worth stating explicitly in the feature docs.

---

## 4. MCP server wiring, lifecycle, and security

### 4.1 Ownership and module placement

**New module: `crates/atm-daemon/src/profiling.rs`, declared from `main.rs`, not `lib.rs`.**

Precedent: `main.rs:6` already declares `mod daemon_observability;` as a binary-only module. Profiling belongs there for the same reason — the guard's lifetime must equal *process* lifetime, and `lib.rs`'s `run_daemon_with_observability*` entrypoints are also driven from integration tests that must never bind a listener.

The module is unconditional; its contents are feature-gated:

```rust
//! Optional live-profiling bootstrap for the ATM daemon.
//!
//! This module owns the process-lifetime hotpath profiler guard and, when the
//! `hotpath-mcp` feature is compiled in *and* profiling is requested on the
//! command line, the local MCP HTTP endpoint used to interrogate the running
//! daemon.
//!
//! # Security
//! The MCP endpoint is not covered by the daemon's capability-based local auth
//! model and is not part of the bounded shutdown drain. It must never be
//! enabled in a released build. See `docs/plans/phase-ai/rust-profiling-hotpath-mcp-plan-rustarchitect.md`.
```

Public surface (three items, all `Debug` per M-PUBLIC-DEBUG):

```rust
/// Whether live profiling was requested for this daemon process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ProfilingMode {
    #[default]
    Disabled,
    /// Collect metrics in-process only; no network listener is bound.
    Collect,
    /// Collect metrics and expose the MCP endpoint on loopback.
    McpEndpoint,
}

/// Process-lifetime profiling guard; flushing happens on drop.
#[derive(Debug)]
pub(crate) struct ProfilingSession { /* opaque; unit struct when disabled */ }

/// Starts profiling for `mode`.
///
/// # Errors
/// Returns [`AtmError`] with `Validation` when `ProfilingMode::McpEndpoint`
/// is requested but the build lacks the `hotpath-mcp` feature, or when the
/// required auth token is absent or too short.
pub(crate) fn start(mode: ProfilingMode) -> Result<ProfilingSession, AtmError>;
```

When features are off, `ProfilingSession` is a zero-sized struct and `start` returns `Err` for anything but `Disabled` — so a production binary given `--profiling-mcp` fails loudly at startup rather than silently ignoring the flag. That is the correct failure mode: a silent no-op would let an operator believe profiling is off when they meant it on, or vice versa.

### 4.2 Entrypoint restructuring — fixing the `process::exit` guard bug

Current `main.rs:12-31`:

```rust
fn main() {
    let exit_code = match run() { Ok(()) => 0, Err(error) => { eprintln!("{error}"); ... } };
    std::process::exit(exit_code);   // ← destructors skipped
}
```

Required shape:

```rust
mod daemon_observability;
mod profiling;

fn main() {
    // `run` owns every RAII guard, including the profiling session. It must
    // return before `process::exit` skips destructors.
    let exit_code = match run() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            atm_daemon::daemon_exit_code_for_error(&error).as_i32()
        }
    };
    std::process::exit(exit_code);
}

fn run() -> Result<(), AtmError> {
    let args = parse_daemon_args(std::env::args_os().skip(1))?;
    // Started first so daemon startup itself is profiled; dropped last.
    let _profiling = profiling::start(args.profiling)?;
    let observability: Arc<dyn atm_daemon::DaemonRuntimeObservability> =
        Arc::new(DaemonObservability::bootstrap()?);
    atm_daemon::run_daemon_with_observability_and_peer_wire_security(
        observability,
        args.peer_wire_security,
    )
}
```

The existing `parse_peer_wire_security` (`main.rs:33-54`) is generalized into `parse_daemon_args`, returning a small `DaemonArgs { peer_wire_security, profiling }`. Note that `main.rs:47-51` currently *rejects unknown arguments*, so adding a flag requires touching this function regardless — there is no additive shortcut.

New flags:

* `--profiling collect` → `ProfilingMode::Collect`
* `--profiling mcp` → `ProfilingMode::McpEndpoint`
* absent → `ProfilingMode::Disabled`

Mirroring `--peer-wire-security`, this is **CLI-only**. The daemon must not decide to expose a profiling endpoint because an environment variable leaked in from a parent shell. The `lib.rs:146-149` doc comment states this principle for peer wire security; the same reasoning applies with more force here.

Port and token remain hotpath's own `HOTPATH_MCP_PORT` / `HOTPATH_MCP_AUTH_TOKEN` env vars, because hotpath reads them internally and we should not fork its configuration surface. But `profiling::start` **pre-validates** them and fails closed:

```rust
// A short or absent token would leave the profiling endpoint effectively
// unauthenticated. 32 chars ~= 192 bits of base64 entropy, matching the
// entropy floor already used for the local capability token.
const MIN_MCP_AUTH_TOKEN_LEN: usize = 32;
```

If `HOTPATH_MCP_AUTH_TOKEN` is missing or shorter than that, `start` returns `AtmError::validation(...)` and the daemon does not boot. Fail-closed, consistent with `composition.rs:704-715` refusing to start with an invalid peer configuration.

### 4.3 Security and operational analysis

**Existing local auth model, for contrast.** Every current daemon ingress has a real authenticator:

* Unix UDS — filesystem permissions on the endpoint path, plus host-singleton ownership (`host_ownership.rs`, ADR-002).
* Loopback TCP — `LocalCapability` header, with the capability record written to an **owner-only ACL file** (`local_tcp_transport.rs:422` `restrict_record_to_current_owner`, verified by `local_tcp_transport.rs:645` `endpoint_record_is_owner_readable_only`), plus a loopback-source check (`local_tcp_transport.rs:103`).
* Cross-host — mutual TLS with a pinned client verifier (`https_transport.rs:298` `PinnedClientVerifier`).

The hotpath MCP endpoint has **none of these**. Its only control is a shared bearer token read from an environment variable — which means it is visible in `/proc/<pid>/environ` to the same user, inherited by every child process the daemon spawns, and not rotatable without a restart. **It is strictly weaker than every existing ATM ingress.** That asymmetry alone justifies default-off and never-in-release.

**Attack surface, concretely.** An MCP client that reaches the endpoint can enumerate 28 tools returning: internal function names and module paths (full internal architecture disclosure), per-function timing and call counts (a timing side channel against message-processing paths), allocation profiles, thread activity, mutex/RwLock contention, channel depth, and detailed per-function execution logs. On a daemon that already terminates cross-host mTLS traffic and holds the host's mail database, this is a meaningful reconnaissance surface even without write capability.

**Bind address is a hard blocker (OQ-1).** hotpath's documentation states the server "listens on port 6771" without specifying the interface. If it binds `0.0.0.0`, then on any host with an enabled peer interface (`composition.rs:283-285`) the profiling endpoint is reachable from the same network as the mTLS listener, guarded only by a bearer token. **This must be verified in hotpath's source before any implementation begins.** If it cannot be constrained to `127.0.0.1`, the `hotpath-mcp` feature must be documented as developer-workstation-only and the plan should carry a follow-up to upstream a bind-address option.

**Data exposure discipline.** Per M-LOG-STRUCTURED's "Redact Sensitive Data" and the OWASP guidance it cites:

* Never pass agent names, team names, message ids, message bodies, peer hostnames, or file paths into `gauge!`, `val!`, `dbg!`, or `measure_block!` labels.
* Do not feed rendered SQL into hotpath's SQL tools — `atm-storage-rusqlite` queries carry message content and identities.
* `val!` and `dbg!` are excluded from the sanctioned macro set entirely; if a future need arises, it should be reviewed as a separate change against this constraint.
* Add a `just` lint (or extend `.just/lint_silent_emit.py`'s pattern) asserting that every `hotpath::` macro label in the workspace is a string literal, not an interpolation. Cheap, and it makes the rule enforceable rather than aspirational.

**Shutdown contract.** The daemon's teardown is bounded: `GRACEFUL_DRAIN_DEADLINE = 2s`, `FORCE_CANCEL_DEADLINE = 3s` (`lib.rs:72-73`), threaded through `composition.rs:410-411` into `finalize_serve_loop`. The MCP listener is entirely outside that accounting. Two requirements follow:

1. `ProfilingSession::drop` must stop the listener with its own bounded deadline and must never block daemon exit indefinitely.
2. Because `_profiling` is bound first in `run()`, it drops **last** — after the serve loop returns. That ordering is correct (profiling covers the full lifecycle) but means a wedged MCP shutdown would delay process exit past the documented 3 s force-cancel budget. Bound it explicitly and log with `subsystem = "profiling"`, `action = "mcp_shutdown"`, `outcome = "deadline_exceeded"` per the guidelines advisory.

**Bind-failure policy.** If port 6771 is already taken, `profiling::start` should **fail the daemon startup** when `--profiling mcp` was explicitly requested. Silently degrading to `Collect` would leave an operator waiting on an endpoint that will never answer. This is consistent with `composition.rs`'s fail-closed startup posture.

**Allocator hook.** `hotpath-alloc` installs a process-wide global allocator wrapper. It is deliberately excluded from the `hotpath` feature and given its own flag because (a) it perturbs the very allocation behaviour being measured, (b) it applies to *all* daemon subsystems, not just the three in scope, and (c) `#![deny(unsafe_code)]` (`lib.rs:3`) means any wiring it requires needs a reviewed `#[expect(unsafe_code, reason = ...)]`. Guidelines note M-MIMALLOC-APPS recommends mimalloc for applications; the daemon does not currently set a global allocator, so there is no conflict today — but if mimalloc is ever adopted, `hotpath-alloc` and mimalloc are mutually exclusive and that must be encoded as a `compile_error!`.

**Observability duplication.** The daemon already has a first-class observability subsystem (`daemon_runtime_observability.rs`, `DaemonSubsystem`, `SubsystemObservability`, retained JSONL logs). hotpath is **not** a replacement and must not become a second logging path. Its role is bounded to *live, interactive performance interrogation during development*. All durable, operator-facing signal continues to go through `SubsystemObservability`. State this explicitly in the module docs so a future contributor doesn't start emitting business events into `gauge!`.

---

## 5. File-by-file change list and effort

| # | Path | Action | Changes | Effort |
|---|---|---|---|---|
| 1 | `crates/atm-daemon/Cargo.toml` | modify | add `hotpath` (non-optional, `default-features = false`); add `[features]` block with `hotpath`, `hotpath-mcp`, `hotpath-alloc`, `hotpath-cpu`, each documented with a comment per M-DOCUMENTED-MAGIC | 0.25 d |
| 2 | `crates/atm-core/Cargo.toml` | modify | add optional `hotpath` dep, `hotpath` feature, `[package.metadata.cargo-shear] ignored` | 0.25 d |
| 3 | `crates/atm-storage-rusqlite/Cargo.toml` | modify | same optional pattern | 0.1 d |
| 4 | `crates/atm-daemon/src/profiling.rs` | **create** | `ProfilingMode`, `ProfilingSession`, `start()`; feature-gated bodies; token pre-validation (`MIN_MCP_AUTH_TOKEN_LEN`); bounded shutdown in `Drop`; full module docs per M-MODULE-DOCS incl. the security section | 1.5 d |
| 5 | `crates/atm-daemon/src/main.rs` | modify | `mod profiling;`; restructure `run()` to own the guard before `process::exit` (§4.2); generalize `parse_peer_wire_security` → `parse_daemon_args` returning `DaemonArgs`; extend the two existing arg tests (`main.rs:61-78`) and add rejection tests for `--profiling` with an unknown mode and for `--profiling mcp` on a non-`hotpath-mcp` build | 0.75 d |
| 6 | `crates/atm-daemon/src/local_ipc_transport.rs` | modify | `#[measure]` on `prepare_accept_iteration` (:696), `handle_accepted_stream` (:754); `gauge!` sampling of registry counters once per accept iteration | 0.5 d |
| 7 | `crates/atm-daemon/src/local_ipc_transport/accept_loop.rs` | modify | `#[measure]` on `reject_connection_when_capped` (:75), `spawn_connection_worker` (:127) | 0.25 d |
| 8 | `crates/atm-daemon/src/local_ipc_transport/request_worker.rs` | modify | `#[measure]` on :40, :157, :189, :225, :251; `channel!` wrappers at :195, :196, :231 | 0.75 d |
| 9 | `crates/atm-daemon/src/active_connection_registry.rs` | modify | `#[measure]` on :124, :155; `mutex!` on the dispatch-handle mutex behind :116; gauge accessors :103/:107 | 0.5 d |
| 10 | `crates/atm-daemon/src/local_tcp_transport.rs` | modify | `#[measure]` on `handle_connection` (:312); `measure_block!` around the accept/poll arm in `serve_until_terminated` (:102-120) | 0.5 d |
| 11 | `crates/atm-daemon/src/runtime_health.rs` | modify | `#[measure]` on `route` (:857), `dispatch_with_deadline` (:549), `route_write` (:560) | 0.25 d |
| 12 | `crates/atm-daemon/src/runtime_health/peer_delivery_router.rs` | modify | `#[measure]` on :16, :77, :100; `measure_block!("atm.peer.trusted_peer_lookup")` at :40 | 0.5 d |
| 13 | `crates/atm-daemon/src/runtime_health/peer_authority.rs` | modify | `#[measure]` on `resolve_peer_authority` (:25) | 0.1 d |
| 14 | `crates/atm-daemon/src/peer_drain_coordinator.rs` | modify | `#[measure]` on :163, :191, :278, :349, :366, :415, :507, :541; `mutex!` on the slots mutex (:134); `gauge!` in `reserve_slot` (:143) | 1.0 d |
| 15 | `crates/atm-daemon/src/https_transport.rs` | modify | `#[measure]` on :187, :197, :218, :242; five `measure_block!` phases inside `open_connection` (:253/:255/:266/:273/:281) | 0.75 d |
| 16 | `crates/atm-core/src/read/mod.rs` | modify | `cfg_attr` `#[measure]` on :360, :368, :376, :440, :535, :589, :641, :696, :725, :778, :814, :862, :872, :897 | 1.0 d |
| 17 | `crates/atm-core/src/read/metadata_selection.rs` | modify | `cfg_attr` `#[measure]` on the three selection helpers | 0.25 d |
| 18 | `crates/atm-core/src/read/seen_state.rs` | modify | `cfg_attr` `#[measure]` on `load_seen_watermark` (:15) | 0.1 d |
| 19 | `crates/atm-storage-rusqlite/src/mailbox_metadata.rs` | modify | `cfg_attr` `#[measure]` + static-label `measure_block!` on `query_mailbox_metadata_rows` (:205) | 0.25 d |
| 20 | `crates/atm-storage-rusqlite/src/lib.rs` | modify | same on `load_message` (:299), `list_messages` (:350) | 0.25 d |
| 21 | `.github/workflows/ci.yml` | modify | add a Linux-only `cargo clippy -p atm-daemon --features hotpath-mcp --all-targets -- -D warnings` step after the existing clippy job (:109) | 0.25 d |
| 22 | `.just/lint_cargo_deny.py` | modify | add an `--all-features` pass so hotpath's transitive graph is license/advisory-scanned despite `deny.toml:2` | 0.5 d |
| 23 | `.just/` (new lint) | **create** | assert every `hotpath::` macro label in the workspace is a `'static` literal, never an interpolation (§4.3) | 0.5 d |
| 24 | `docs/` | modify | operator/developer runbook: how to build with `--features hotpath-mcp`, set `HOTPATH_MCP_AUTH_TOKEN`, launch with `--profiling mcp`, attach via `claude mcp add --transport http hotpath http://localhost:6771/mcp`, and the "never in release" rule | 0.5 d |
| 25 | `crates/atm-daemon/src/tests*.rs` | modify | test that `ProfilingMode::Disabled` binds no listener; that a missing/short token fails closed; that a `--profiling mcp` request on a non-`hotpath-mcp` build errors rather than no-ops | 0.5 d |

**Total: ~12 person-days**, plus 1–2 days of pre-work resolving the blocking open questions in §6. Suggested sequencing: OQ-1/OQ-2/OQ-3 spike → rows 1–5 (skeleton, features, entrypoint, security) → row 21/22/23 (gates, before instrumentation lands so it can never rot) → rows 6–11 (scope 1) → rows 16–20 (scope 2) → rows 12–15 (scope 3) → rows 24–25.

Scope 1 is deliberately first: it is the highest-traffic path, it is where the three-threads-per-request design is most likely to be measurably wrong, and it validates the whole toolchain against a small blast radius before touching the published `atm-core` crate.

---

## 6. Open questions and risks

Blocking items must be closed **before** implementation starts.

| ID | Severity | Question / risk | Resolution path |
|---|---|---|---|
| **OQ-1** | **Blocking** | What interface does hotpath's MCP server bind? If `0.0.0.0`, the profiling endpoint is network-reachable on any host with an enabled peer interface, guarded only by a bearer token. | Read hotpath's MCP server source. If not loopback-constrained and not configurable, restrict the feature to developer workstations in docs and open an upstream request for a bind-address option. |
| **OQ-2** | **Blocking** | Does `hotpath-mcp` pull in and start a tokio runtime inside a daemon that is deliberately tokio-free? What thread does it own, and how does it interact with `thread::scope` in `serve_runtime_scope` (`local_ipc_transport.rs:392`)? | Build a spike with `--features hotpath-mcp` and inspect `cargo tree` plus runtime thread names. If a runtime is required, confine it to a single current-thread runtime on one dedicated thread owned by `ProfilingSession`. |
| **OQ-3** | **Blocking** | Does hotpath's `hotpath-mcp` transitive graph satisfy `deny.toml:21-31`? A single MPL-2.0 or BSD-2-Clause transitive dep fails the gate. | Run `cargo deny --all-features check licenses` on the spike branch. |
| **OQ-4** | High | Does hotpath 0.22 support edition 2024 / rust-version 1.94.1 (`Cargo.toml:25-26`)? | Verify hotpath's MSRV; if it exceeds ours, this integration is blocked until the workspace MSRV moves. |
| **OQ-5** | Medium | `hotpath-cpu` sampling support on macOS/Windows — CPU samplers are frequently Linux-only or need elevated privileges. | Verify per-platform. If Linux-only, mark the feature Linux-only in the manifest comment and skip it on other hosts. |
| **OQ-6** | Medium | Does `hotpath-alloc` require the consumer to declare `#[global_allocator]`, and does that wiring need `unsafe`? `atm-daemon/src/lib.rs:3` is `#![deny(unsafe_code)]`. | Verify; if `unsafe` is needed, add a reasoned `#[expect(unsafe_code, reason = ...)]` in `main.rs` only, never in `lib.rs`. |
| **OQ-7** | Low | Is the compat mailbox path (`atm-core/src/mailbox/mod.rs:68`, `:92`) still reachable under the SQLite-backed runtime? | Confirm before instrumenting; instrumenting a dead path produces misleading MCP answers. |
| **R-1** | High | **Observer effect.** `#[measure]` on functions called per-request across three crates adds per-call overhead precisely where we are measuring microseconds. Scope-1 sites are the most exposed. | Never enable profiling in a build used for absolute latency SLO measurement. Treat hotpath output as *relative* attribution. Cross-check against an external profiler once before trusting the ranking. |
| **R-2** | High | **Feature rot.** Without the CI leg in row 21, the instrumented build stops compiling within weeks and nobody notices until they need it. | Row 21 lands *before* the bulk instrumentation (rows 6–20). Non-negotiable sequencing. |
| **R-3** | Medium | **Label-cardinality blowup / PII.** A well-intentioned future `gauge!("peer_" + host)` both explodes cardinality and leaks peer identity to the MCP surface. | The static-label lint (row 23) makes this mechanically impossible rather than a review convention. |
| **R-4** | Medium | **Shutdown regression.** `ProfilingSession` drops after the serve loop; a wedged MCP teardown pushes process exit past the documented 3 s force-cancel budget (`lib.rs:73`). | Bounded stop inside `Drop`, with a structured `warn!` carrying `subsystem`/`action`/`outcome`. Add a test asserting drop completes within the force-cancel budget. |
| **R-5** | Medium | **Windows coverage gap.** CI excludes `atm-daemon` tests from the general Windows workspace run and runs them separately (`ci.yml:164-172`); the Windows local path is the *serialized* `local_tcp_transport` path, structurally unlike Unix. Profiling only on macOS/Linux would produce conclusions that do not transfer. | Explicitly plan at least one Windows-host profiling session against `local_tcp_transport.rs:92`/`:312` before acting on any local-ingress optimization. |
| **R-6** | Medium | **Port collision.** 6771 is fixed and process-global. The daemon is a host singleton (ADR-002) so it cannot collide with itself, but any other hotpath-enabled process on the host will. | Fail closed on bind error when `--profiling mcp` was explicitly requested; document `HOTPATH_MCP_PORT` as the override. |
| **R-7** | Low | **Two observability systems.** Contributors may start routing durable signal through `gauge!` instead of `SubsystemObservability`. | Stated explicitly in the `profiling.rs` module docs; reinforced in the row-24 runbook. |
| **R-8** | Low | **`atm-core` API-surface churn.** Adding a `hotpath` feature to a published crate is a public manifest change even though no public item changes. | Documented as additive per M-FEATURES-ADDITIVE; note it in the crate changelog. |

---

## 7. What "done" looks like

1. `cargo build -p atm-daemon` (default features) produces a binary with **zero** hotpath code, no new runtime threads, and no listener.
2. `cargo build -p atm-daemon --features hotpath-mcp` plus `HOTPATH_MCP_AUTH_TOKEN=<32+ chars> atm-daemon --profiling mcp` exposes `http://127.0.0.1:6771/mcp`.
3. `claude mcp add --transport http hotpath http://localhost:6771/mcp` attaches, and the following are answerable live against a running daemon:
   - "What fraction of local request latency is dispatch queue wait versus actual routing work?" → `await_dispatch_response` vs. `route`
   - "Are we hitting the 64-connection cap, and how close do we get?" → `atm.local_ipc.active_connections` gauge vs. `reject_connection_when_capped`
   - "How much of a cross-host send is TLS setup versus wire time?" → the five `open_connection` phase blocks
   - "Is the peer drain slots mutex contended?" → `mutex!` on `peer_drain_coordinator.rs:134`
   - "What dominates a `--contains` read?" → `filter_metadata_backed_contains_candidates` vs. `query_mailbox_metadata_rows`
4. Default-feature `just lint` and full CI stay green; the new profiling-features clippy leg is green.
5. No released artifact ships with any `hotpath*` feature enabled, and that is asserted in the release workflow rather than remembered.

---

**Files referenced in this plan (absolute paths):**

- `/Users/randlee/Documents/github/atm-core/crates/atm-daemon/src/main.rs`
- `/Users/randlee/Documents/github/atm-core/crates/atm-daemon/src/lib.rs`
- `/Users/randlee/Documents/github/atm-core/crates/atm-daemon/src/composition.rs`
- `/Users/randlee/Documents/github/atm-core/crates/atm-daemon/src/local_ipc_transport.rs`
- `/Users/randlee/Documents/github/atm-core/crates/atm-daemon/src/local_ipc_transport/accept_loop.rs`
- `/Users/randlee/Documents/github/atm-core/crates/atm-daemon/src/local_ipc_transport/request_worker.rs`
- `/Users/randlee/Documents/github/atm-core/crates/atm-daemon/src/local_tcp_transport.rs`
- `/Users/randlee/Documents/github/atm-core/crates/atm-daemon/src/active_connection_registry.rs`
- `/Users/randlee/Documents/github/atm-core/crates/atm-daemon/src/runtime_health.rs`
- `/Users/randlee/Documents/github/atm-core/crates/atm-daemon/src/runtime_health/peer_delivery_router.rs`
- `/Users/randlee/Documents/github/atm-core/crates/atm-daemon/src/runtime_health/peer_authority.rs`
- `/Users/randlee/Documents/github/atm-core/crates/atm-daemon/src/peer_drain_coordinator.rs`
- `/Users/randlee/Documents/github/atm-core/crates/atm-daemon/src/https_transport.rs`
- `/Users/randlee/Documents/github/atm-core/crates/atm-core/src/read/mod.rs`
- `/Users/randlee/Documents/github/atm-core/crates/atm-core/src/read/state.rs`
- `/Users/randlee/Documents/github/atm-core/crates/atm-core/src/read/seen_state.rs`
- `/Users/randlee/Documents/github/atm-core/crates/atm-core/src/mailbox/mod.rs`
- `/Users/randlee/Documents/github/atm-core/crates/atm-storage-rusqlite/src/mailbox_metadata.rs`
- `/Users/randlee/Documents/github/atm-core/crates/atm-storage-rusqlite/src/lib.rs`
- `/Users/randlee/Documents/github/atm-core/crates/atm-daemon/Cargo.toml`
- `/Users/randlee/Documents/github/atm-core/crates/atm-core/Cargo.toml`
- `/Users/randlee/Documents/github/atm-core/Cargo.toml`
- `/Users/randlee/Documents/github/atm-core/deny.toml`
- `/Users/randlee/Documents/github/atm-core/justfile`
- `/Users/randlee/Documents/github/atm-core/.github/workflows/ci.yml`
- `/Users/randlee/Documents/github/atm-core/.just/check_env_var_boundary.py`
- `/Users/randlee/Documents/github/atm-core/.claude/skills/rust-development/guidelines.txt`
