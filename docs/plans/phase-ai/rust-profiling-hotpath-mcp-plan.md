# Rust hot-path MCP profiling plan

Status: design proposal; no runtime or dependency changes are made by this
document.

## Goal and constraints

Add an opt-in profiling build of `atm-daemon` that exposes hotpath-rs metrics
through an MCP HTTP endpoint while preserving the normal daemon binary's
startup, transport, shutdown, and security contracts. The profiler must make
the three user-visible paths distinguishable: local admission and dispatch,
mailbox reads, and cross-host peer writes. Instrumentation should be placed at
stable ownership boundaries rather than every helper, so reports remain useful
after refactors and the measurement overhead is bounded.

The hotpath MCP documentation describes `hotpath`, `hotpath-alloc`, and
`hotpath-mcp` feature combinations, summary/detail tools, `HOTPATH_MCP_PORT`
(default 6771), and optional `HOTPATH_MCP_AUTH_TOKEN` authentication. See
<https://hotpath.rs/mcp> and <https://hotpath.rs/configuration>. The exact
crate API used to start/stop the MCP server must be confirmed against the
selected hotpath release before implementation; this plan does not assume
that the daemon can simply adopt `#[hotpath::main]` as its existing `main`.

## Instrumentation map

Use `#[cfg_attr(feature = "profiling", hotpath::measure)]` (or the equivalent
hotpath-supported wrapper) on the following boundaries. Keep labels stable and
explicit if the crate supports a label argument.

### Local UDS/TCP admission and dispatch

Instrument the common request lifecycle in
`crates/atm-daemon/src/local_ipc_transport/request_worker.rs`:

* `handle_connection` — complete request-worker latency, including bounded
  HTTP read and response write.
* `read_bounded_http_request` — framing/read cost and rejected oversized input.
* `dispatch_request` and `await_dispatch_response` — queue/worker handoff and
  deadline wait, separately from socket I/O.
* `spawn_dispatch_worker` — dispatch execution time in the shared
  `ApiRouter` path.

Instrument the platform-specific admission points:

* `local_ipc_transport::handle_accepted_stream` and
  `accept_loop::spawn_connection_worker` for UDS accept/capacity behavior.
* `local_tcp_transport::handle_connection` and its accept-loop registration
  for Windows loopback admission and capability rejection.
* `ActiveConnectionRegistry::try_register`, `reap_finished_dispatches`, and
  the bounded-drain join path only if the macro can measure these small sync
  functions without making shutdown measurements dominate normal traffic.

Both transports converge at the request worker, so report names must make the
transport prefix explicit (`local_uds.*` versus `local_tcp.*`) while retaining
one comparable `dispatch.*` measurement. Add gauges for active connections and
capacity rejections if hotpath's gauge API is available; do not expose request
bodies, agent identities, or auth material as debug values.

### Read path

In `crates/atm-core/src/read/mod.rs`, measure the public entry points and the
large, reusable phases:

* `read_mail_with_runtime_impl` and `peek_mail_with_runtime_impl` — end-to-end
  mutating and non-mutating surfaces.
* `resolve_read_context` — target/roster/config resolution.
* `load_read_selection` — metadata selection and filtering.
* `wait_for_selection_candidates` — polling timeout behavior, kept separate so
  intentional waits are not mistaken for storage latency.
* `resolve_read_display`, `build_unmodified_read_display`, and
  `build_mutated_read_display` — display construction and read-state mutation.
* `load_checked_read_metadata` plus the underlying mailbox query boundary in
  `crates/atm-core/src/mailbox/mod.rs` (or the concrete retained-store adapter)
  — storage time without duplicating the entire read wrapper.

Do not measure tiny state classifiers in `read/state.rs` initially. If a report
shows classification cost is material, add a separately named measurement in a
follow-up. Add a count/size gauge for selected rows only if it can be bounded;
never record message bodies or mailbox paths in profiling logs.

### Cross-host peer write

Instrument the delivery ownership boundaries:

* `DaemonRequestDispatcher::dispatch` and `deliver_to_peer` in
  `runtime_health/peer_delivery_router.rs` — persisted-write-to-peer outcome
  and foreground deadline.
* `resolve_peer_authority` in `runtime_health/peer_authority.rs` — trusted
  peer lookup and bounded DNS/authority resolution.
* The peer drain coordinator's public `deliver_after_persist` operation (the
  coordinator module's actual path/name must be confirmed; it is currently
  owned by the runtime-health module) — queue admission, retry classification,
  and backoff.
* `HttpsTransport::deliver`, `deliver_page`, `open_connection`, and the
  server-side `handle_peer_connection`/`route_peer_http_request` boundaries in
  `https_transport.rs` — connection setup, TLS handshake, wire request, and
  remote dispatch separately.

Use a gauge for queued peer jobs and in-flight peer connections if hotpath can
observe the existing coordinator counters. Metrics must use a stable peer
host label only after confirming that hostnames are not sensitive in the
deployment; otherwise aggregate by role (`peer`) and outcome. Never record
certificates, fingerprints, bearer tokens, request bodies, or full addresses.

## Feature and build strategy

Add an opt-in `profiling` feature to the crates that contain measured code.
The daemon feature should forward the compatible hotpath features to
`atm-core` and enable `hotpath-mcp`; the core feature should enable only the
measurement dependency. Keep the dependency optional and workspace-pinned.
Use `hotpath-alloc` only in an explicitly named allocation-profiling build,
because allocator instrumentation can materially change daemon behavior.

Recommended profiles:

* default/release: no hotpath dependency expansion and zero instrumentation;
* `--features profiling`: timing, channels/locks/gauges as selected, MCP
  server available only when explicitly enabled by runtime configuration;
* `--features profiling,profiling-alloc`: allocation collection for a short
  diagnostic run, never the default CI or production package.

The implementation must verify whether hotpath's macros are true no-ops when
the feature is disabled. If not, place the attributes behind `cfg_attr` and
provide a small local compatibility macro/module so the default build remains
dependency-free. Add compile checks for both feature sets.

## MCP server lifecycle and configuration

Create a daemon-owned `hotpath_runtime` module behind `cfg(feature =
"profiling")`. `RuntimeComposition::start` should start the profiler after
runtime assembly and before transport serving, retaining a guard/handle on
`RuntimeComposition` so the MCP server and its worker are joined or stopped on
the existing draining path. Startup failure must be explicit: in a profiling
build, fail closed rather than silently reporting an endpoint that is not
listening; in the default build the module is absent.

Use `HOTPATH_MCP_PORT` as the initial configuration surface (default 6771),
with a future `atm` config field only if operational users need per-team
configuration. Bind to loopback by default. Require
`HOTPATH_MCP_AUTH_TOKEN` for any non-loopback bind and pass the token through
the hotpath Authorization-header mechanism. Do not print the token or include
it in retained daemon observability. If hotpath cannot constrain bind address,
place it behind a loopback-only reverse proxy or reject non-loopback operation
in the daemon wrapper.

Expose readiness and shutdown events through the existing daemon observability
subsystem, including the selected port and an `auth_required` boolean, never
the secret. Add a doctor section that is present only in profiling builds and
reports disabled/listening/degraded state without making normal health depend
on the optional endpoint.

## Security and operational controls

The MCP tools expose timing, allocation, thread, channel, and debug data from a
long-lived process. Treat the endpoint as a diagnostic control plane:

1. disabled by default and absent from release service units;
2. loopback-only unless an explicit profiling deployment enables remote access;
3. mandatory non-empty auth token for remote access, with startup rejection for
   a missing token;
4. no request-body, mailbox-content, certificate, or secret values in metrics;
5. bounded log/history limits and sampling rates to prevent profiler memory
   growth; document `HOTPATH_TIME_SAMPLING_RATE`, `HOTPATH_LOGS_LIMIT`, and
   `HOTPATH_OUTPUT_FORMAT=none` for quiet runs;
6. clear shutdown ownership and a test proving the endpoint disappears after
   daemon drain; and
7. a threat-model note covering token rotation (restart required unless the
   dependency provides a safe reload), localhost compromise, and MCP client
   trust.

## File-by-file implementation sequence and estimate

1. `Cargo.toml`, `crates/atm-core/Cargo.toml`, and
   `crates/atm-daemon/Cargo.toml`: optional dependency/version and feature
   forwarding (0.5 day).
2. `crates/atm-core/src/read/mod.rs`, `mailbox/mod.rs`, and selected lock or
   queue constructors: read/storage measurements and bounded gauges (0.5–1
   day).
3. `crates/atm-daemon/src/local_ipc_transport/request_worker.rs`,
   `local_ipc_transport.rs`, `local_ipc_transport/accept_loop.rs`, and
   `local_tcp_transport.rs`: admission/dispatch measurements (0.5–1 day).
4. `runtime_health/peer_delivery_router.rs`, `peer_authority.rs`, the peer
   drain coordinator module, and `https_transport.rs`: peer/TLS measurements
   (0.75–1.5 days).
5. New `crates/atm-daemon/src/hotpath_runtime.rs`, plus `lib.rs` and
   `composition.rs`: feature-gated startup, lifecycle, readiness, and doctor
   projection (1–1.5 days).
6. Targeted unit/integration tests under `crates/atm-daemon/tests` and
   existing composition tests: default-build absence, profiled startup,
   auth/bind rejection, shutdown, and representative metric labels (0.75–1
   day).
7. `docs/` operator guidance and a `just` recipe for a local MCP profiling run
   (0.25 day).

Expected implementation effort: 4–6 engineer-days, plus one performance review
to validate that instrumentation does not perturb the measured paths.

## Open questions and risks

* Which hotpath release and exact startup/guard API support embedding an MCP
  server in an existing synchronous/threaded daemon rather than a macro-owned
  `main`?
* Does the MCP server bind loopback only, and can it be stopped/reconfigured
  without process restart? The answer determines whether daemon-side bind and
  token validation are mandatory wrappers.
* Are `#[hotpath::measure]` attributes valid on generic functions, trait-driven
  methods, and functions compiled for Windows? Add a small compile probe before
  broad annotation.
* How much overhead do measurement and allocator features add at the daemon's
  64-connection cap? Establish an uninstrumented baseline and a profiled budget
  before enabling any allocation mode.
* Existing `https_transport` and local transports have strict deadline and
  shutdown tests; profiler workers must not consume those budgets or introduce
  detached threads.
* Hostname labels, debug entries, and thread names may reveal deployment
  topology. Default to aggregate labels until the security review approves
  richer dimensions.
