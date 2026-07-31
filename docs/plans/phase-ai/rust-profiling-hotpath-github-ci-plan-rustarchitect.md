# Hotpath.rs GitHub CI Integration — Architecture Plan (atm-core)

**Scope:** automated performance-regression detection for atm-daemon hot paths via hotpath.rs GitHub CI mode.
**Author role:** Rust architect (independent investigation — GitHub CI track only).
**Repo:** `/Users/randlee/Documents/github/atm-core` (workspace version `1.4.0-beta-ai`, edition 2024, MSRV 1.94.1).

---

## 1. Grounding: what the codebase actually looks like

### 1.1 Workspace shape

`/Users/randlee/Documents/github/atm-core/Cargo.toml:1-21` declares 17 member crates. Relevant ones:

| Crate dir | Package name | Role |
|---|---|---|
| `crates/atm-core` | `agent-team-mail-core` | library: read path, mailbox, api/router traits |
| `crates/atm` | `agent-team-mail` | CLI |
| `crates/atm-daemon` | `atm-daemon` | transports + dispatch + peer delivery |
| `crates/atm-daemon-client` | `atm-daemon-client` | client-side connect/exchange |
| `crates/atm-runtime-test-support` | — | SQLite runtime install helpers |
| `crates/atm-storage-rusqlite` | — | SQLite storage incl. `peer_config_store` |

Key fact for benchmark design: **the daemon is thread-based, not Tokio.** `spawn_dispatch_worker` at `/Users/randlee/Documents/github/atm-core/crates/atm-daemon/src/local_ipc_transport/request_worker.rs:189-218` uses `std::thread::Builder` + `std::sync::mpsc::sync_channel`. There is no async runtime in the local ingress path, so `#[hotpath::measure]` applies in its plain synchronous form and the async-instrumentation caveats do not arise for scope areas (1) and (2).

### 1.2 The visibility problem (drives the whole design)

`crates/atm-daemon/src/lib.rs:11-54` declares essentially every interesting module **private**:

```
mod local_ipc_transport;      // line 29
mod local_tcp_transport;      // line 33
mod https_transport;          // line 24
mod runtime_health;           // line 39
mod active_connection_registry; // line 11
```

The only public entry points are `run_daemon_with_observability` (`lib.rs:140`), `run_daemon_with_observability_and_peer_wire_security` (`lib.rs:150`), `PeerWireSecurity` (`lib.rs:64`), and `daemon_exit_code_for_error` (`lib.rs:90`).

The server construction API a benchmark needs is `pub(crate)`:
- `LocalIpcServer::prepare_runtime_at_socket_path_for_home` — `crates/atm-daemon/src/local_ipc_transport.rs:844`
- `PreparedRuntimeServer::serve_with_runtime_hooks` — `local_ipc_transport.rs:328`
- `PeerWire::bind_plaintext_test` / `bind_enabled` — `crates/atm-daemon/src/https_transport.rs:346` / `:320`
- `MAX_CONCURRENT_CONNECTIONS = 64` — `local_ipc_transport.rs:51`; `REQUEST_DEADLINE = 3s` — `local_ipc_transport.rs:52`

Cargo `examples/` are **external crates**, so they see only the public surface. Existing test fakes such as `DoctorOnlyDispatcher` live behind `#[cfg(test)]` (`request_worker.rs:335`) and are unreachable from examples.

Consequence: hotpath benchmark examples cannot reach the daemon hot path today. This is the single largest piece of work in this plan and must be resolved explicitly (see §3.1).

### 1.3 Read path

`crates/atm-core/src/read/mod.rs` exposes four entry points at lines 344/352/360/368: `read_mail`, `peek_mail`, `read_mail_with_runtime`, `peek_mail_with_runtime`. The `_with_runtime` variants take `&LocalServiceRuntime` and are the benchmark-friendly ones — no daemon, no global runtime resolution. Internals worth per-function attribution: `resolve_read_context`, `load_read_selection`, `resolve_read_display` (`read/mod.rs:388-404`, `:445+`), plus `read/metadata_selection.rs` helpers imported at `read/mod.rs:23-26` (`filter_metadata_backed_contains_candidates`, `load_durable_metadata_message`, `selection_state_for_mailbox_metadata_rows`, `sort_and_limit_selected`).

A real SQLite-backed runtime is already reachable from outside the crate: `install_sqlite_retained_runtime_factory()` and `SqliteRuntimeGuard::install(path)` in `crates/atm-runtime-test-support/src/lib.rs:24-57`. **The read-path benchmark needs no new visibility work at all.**

### 1.4 Peer / cross-host write path

`crates/atm-daemon/src/runtime_health/peer_delivery_router.rs:14-53` — `PostWriteRouter::dispatch` for `DaemonRequestDispatcher`:
1. peer-receipt short-circuit → `emit_local_post_write` (lines 21-30)
2. host extraction from `outbound_request.to` (lines 31-39)
3. `resolve_peer_authority(host, &self.peer_config_store.list_trusted_peers()?)` (line 40)
4. `record_peer_delivery_event(WritePersisted)` (lines 43-51)
5. `deliver_to_peer` → `peer_delivery_coordinator.deliver_after_persist(&message.outbound_request, deadline)` (lines 77-98), egress through `https_transport.rs`

The `RequestDeadline` established at local admission (`request_worker.rs:49`) is threaded unchanged into the peer hop — so a peer benchmark measures a path that shares budget with local admission. That makes peer regressions user-visible as local timeouts, which is a good argument for covering it.

### 1.5 Existing CI

`/Users/randlee/Documents/github/atm-core/.github/workflows/ci.yml`:
- Triggers: `pull_request` on `[develop, main, "integrate/*"]`, `push` on `[develop, main]` (lines 3-7).
- Jobs: `just-lint` (3 OS, heaviest — installs cargo-deny/audit/shear/binstall/cargo-modules/codespell), `fmt` (ubuntu), `clippy` (ubuntu, `--workspace --all-targets -D warnings`, line 109), `test` (3 OS, needs clippy).
- Caching: `actions/cache@v4` on `~/.cargo/registry`, `~/.cargo/git`, `target`, keyed on `${{ runner.os }}` + `hashFiles('**/Cargo.lock')` (lines 129-145).
- The `test` job also builds release binaries and runs `scripts/smoke/run_thorough_shared_host.py` (line 176) — meaning the ubuntu/macos/windows `test` runner **already hosts a live daemon singleton**. Any benchmark job that also binds daemon sockets must be a *separate job* with an isolated `ATM_HOME`, or it will collide with host-ownership/singleton logic (`crates/atm-daemon/src/host_ownership.rs`, `shutdown_beacon.rs`).

### 1.6 Lint gates a new crate must survive

`justfile:29-130` runs, among others: clippy `-D warnings` on `--all-targets`, `cargo-deny`, `cargo-shear` (unused deps), `cargo-modules`, `lint_manifests.py`, `check_line_counts.py` (RULE-003, **1000 lines max per production file**, `.just/check_line_counts.py:19`), `check-function-length.py`, `check_test_identity_literals.py` (RULE-008 — no `atm-dev` literal), `check_fixed_sleep_hygiene.py`, `lint_sc_boundary.py`, `lint_unix_gating.py`, `lint_same_host_portability.py`.

Any new bench crate and any new cargo feature must be designed against these, not retrofitted.

---

## 2. Architecture decision (one choice, committed)

> **Add a non-default, additive `hotpath` instrumentation feature to `agent-team-mail-core` and `atm-daemon`, plus a non-default `bench-harness` feature on `atm-daemon` that publicly re-exports the already-existing `pub(crate)` server-construction API. Put all benchmark examples in a new `publish = false` workspace crate `crates/atm-bench`, which runs the daemon *in-process* so a single hotpath collector sees both the load generator and the daemon-side frames. Wire two GitHub workflows (`hotpath-profile` + `hotpath-comment`), gated to `ubuntu-latest` per PR with macOS/Windows on a schedule, and treat all output as informational — never a required check.**

### 2.1 Why in-process rather than driving the real daemon binary

hotpath instruments *the process it is linked into* and flushes at `#[hotpath::main]` exit. Two options existed:

| Option | Mechanism | Verdict |
|---|---|---|
| **A. Separate daemon process** | build `atm-daemon` with `--features hotpath`, run it, drive it from a client example, stop it, merge two JSON artifacts | **Rejected.** Requires clean daemon shutdown-and-flush per benchmark, two artifacts per scope area per branch (8 artifacts/PR), a merge step hotpath-utils does not provide, and it entangles the benchmark with the singleton/host-ownership/beacon lifecycle (`host_ownership.rs`, `shutdown_beacon.rs`). Highest flake surface. |
| **B. In-process daemon (chosen)** | example binds a real UDS/TCP listener via `serve_with_runtime_hooks` on a thread, drives it over a real socket from the same process | **Chosen.** One `#[hotpath::main]`, one JSON file per benchmark, no lifecycle coupling, and the real transport is still exercised end-to-end (real socket, real HTTP/1.1 framing, real `decode_request`, real thread-per-connection admission). |

Option B does *not* degrade fidelity in a way that matters: `handle_connection` (`request_worker.rs:40`) takes a `LocalSocketStream` from a real listener either way. What we lose is process-boundary effects (separate address space, separate allocator arena). That is acceptable for regression *deltas*, which is what this system measures.

### 2.2 Why a separate `crates/atm-bench` crate rather than `examples/` inside `atm-daemon`

- `crates/atm-daemon/Cargo.toml` currently has no `[dev-dependencies]` on the load-generation surface it would need (`atm-daemon-client` is dev-only at line 38 — usable, but examples share dev-deps, and adding `hotpath` there pulls it into every `cargo test -p atm-daemon` build).
- Guidelines M-SMALLER-CRATES favors the split; a `publish = false` crate keeps `hotpath` out of the published dependency graph entirely.
- `cargo clippy --workspace --all-targets -- -D warnings` (`ci.yml:109`) compiles examples. Keeping them in a dedicated crate whose default feature set is empty means the default clippy leg compiles only trivial code, and the bench-specific clippy run is opt-in.

**Trade-off accepted:** a member crate that most contributors never build; mitigated by `publish = false` + a README + no default features.

### 2.3 Feature design (M-FEATURES-ADDITIVE compliance)

```toml
# crates/atm-core/Cargo.toml
[features]
default = []
test-utils = [...]                       # existing
hotpath = ["dep:hotpath"]                # NEW: additive, adds no public items

[dependencies]
hotpath = { version = "0.x", optional = true }
```

```toml
# crates/atm-daemon/Cargo.toml
[features]
default = []
hotpath = ["dep:hotpath", "atm-core/hotpath"]
bench-harness = []                       # NEW: widens visibility only
```

Instrumentation sites use the zero-cost gate hotpath documents:

```rust
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn handle_connection(/* ... */) -> Result<(), AtmError> { /* unchanged */ }
```

Rules I am committing to:
- `hotpath` **must not** change any public item, any behavior, or any error text. It is purely additive attributes. This satisfies M-FEATURES-ADDITIVE and keeps the feature invisible to `cargo hack`.
- `bench-harness` **only** flips `pub(crate)` → `pub` on a small, enumerated list of constructors behind `#[cfg(feature = "bench-harness")]`. No new logic. It must be added to the `sc-lint-boundary` allowlist explicitly (`.just/lint_sc_boundary.py`), not silently.
- Neither feature is ever enabled in the default `ci.yml` legs.

**Trade-offs:**
- Two more feature permutations for `cargo-hack`/`cargo-deny` to consider. Mitigated: both are leaf features with no cross-crate fan-out beyond `atm-core/hotpath`.
- `bench-harness` is a real encapsulation concession. The alternative (make the constructors permanently `pub`) is worse; the alternative (duplicate a fake server in the bench crate) would benchmark code that isn't the production path, which defeats the purpose.

---

## 3. Benchmark inventory

All benchmark examples live in `crates/atm-bench/examples/`. Shared fixture code lives in `crates/atm-bench/src/lib.rs` (+ submodules), so each example file stays well under the RULE-003 1000-line cap and the function-length cap.

### 3.0 Shared harness (`crates/atm-bench/src/`)

| Module | Responsibility |
|---|---|
| `corpus.rs` | Deterministic mailbox corpus builder: N agents × M messages, fixed ULIDs derived from a seeded generator so head and base runs see byte-identical data. **Must use `"test-team"` / non-`atm-dev` identity literals** (RULE-008, `.just/check_test_identity_literals.py`). |
| `runtime.rs` | Wraps `atm_runtime_test_support::SqliteRuntimeGuard::install` (`crates/atm-runtime-test-support/src/lib.rs:38`) over a `tempfile::TempDir`, returns a warmed `LocalServiceRuntime`. |
| `daemon.rs` | Starts an in-process daemon on an isolated `ATM_HOME` temp dir via the `bench-harness` API; returns an endpoint + `Drop`-based shutdown. Unix → UDS path under the temp dir; Windows → loopback TCP. |
| `load.rs` | Fixed-iteration-count drivers (never time-boxed — deterministic call counts are required for hotpath's per-function `calls` column to be comparable across head/base). |
| `peer.rs` | Two-instance loopback peer fixture (see §3.3). |

Iteration counts are named constants with M-DOCUMENTED-MAGIC comments explaining the wall-clock budget they were chosen for.

### 3.1 Scope area 1 — local transport admission + dispatch

**Example:** `crates/atm-bench/examples/local_admission.rs` (`#[hotpath::main]`)

**What it does:** binds a real listener via the `bench-harness`-exposed `LocalIpcServer::prepare_runtime_at_socket_path_for_home` / `serve_with_runtime_hooks` (`local_ipc_transport.rs:844`, `:328`), then issues a fixed number of requests over a real socket using `atm_daemon_client::exchange_request` (`crates/atm-daemon-client/src/lib.rs:424`). Two shapes:
- `doctor` (read-only, `RequestExecutionRisk::ReadOnly` per `request_worker.rs:282`) — pure admission+dispatch overhead, minimal downstream work.
- `messages` read request — admission + real read work.

Two concurrency profiles: serial (1 in flight) and saturating (48 in flight, deliberately under `MAX_CONCURRENT_CONNECTIONS = 64` at `local_ipc_transport.rs:51` so the benchmark measures throughput, not rejection).

**Instrumented functions (`#[cfg_attr(feature = "hotpath", hotpath::measure)]`):**

| Function | Location | Why |
|---|---|---|
| `request_worker::handle_connection` | `request_worker.rs:40` | top-level per-connection cost |
| `request_worker::read_bounded_http_request` | `request_worker.rs:225` | spawns a *second* thread per request (`:232`) — a known cost worth watching |
| `request_worker::dispatch_request` | `request_worker.rs:157` | registry push + worker spawn |
| `request_worker::spawn_dispatch_worker` | `request_worker.rs:189` | thread spawn cost, isolated |
| `request_worker::await_dispatch_response` | `request_worker.rs:251` | blocking recv; separates queueing from work |
| `atm_core::api::decode_request` | `atm-core` api module (imported `request_worker.rs:5-8`) | deserialization cost |
| `atm_core::api::read_http_request` / `write_http_response` | same | framing cost |
| `accept_loop::spawn_connection_worker` | `local_ipc_transport.rs` accept_loop submodule | accept-side admission |
| `ActiveConnectionRegistry::push_dispatch_handle` / `reap_finished_dispatches` | `crates/atm-daemon/src/active_connection_registry.rs` | contention/reap cost, called at `request_worker.rs:93`,`:173` |

**Windows counterpart:** `crates/atm-bench/examples/local_admission_tcp.rs`, same structure against `local_tcp_transport.rs` (loopback TCP, `LocalCapability` header auth, 25ms accept poll). Gated `#[cfg(windows)]` at the example level and only run on the Windows leg (§4.3). The 25ms accept-poll interval means this benchmark's latency floor is quantized — the harness must issue enough iterations that poll granularity averages out, and the plan should expect the p99 column here to be dominated by that constant, not by code changes. **This is the weakest of the three signals; treat it as informational-only permanently.**

**Note on the two-thread-per-request design:** `handle_connection` spawns a read thread (`request_worker.rs:232`) *and* a dispatch thread (`:204`) per request. Under the 48-concurrent profile that is ~96 live threads. hotpath aggregates per-function across threads, which is what we want, but wall-clock noise on a 2-core GitHub runner will be substantial. Mitigation in §6.

### 3.2 Scope area 2 — read path throughput

**Example:** `crates/atm-bench/examples/read_path.rs` (`#[hotpath::main]`)

**No daemon, no sockets, no `bench-harness` feature needed.** Installs a SQLite runtime via `SqliteRuntimeGuard::install` (`crates/atm-runtime-test-support/src/lib.rs:38`), builds the deterministic corpus, then calls `read_mail_with_runtime` / `peek_mail_with_runtime` (`crates/atm-core/src/read/mod.rs:360`, `:368`) with an `ObservabilityPort` no-op.

This is the **highest-value, lowest-risk benchmark** in the plan: it is the highest-frequency operation, it is pure CPU + SQLite, it has no thread-spawn noise, and it needs zero visibility changes. **Land this one first.**

Query shapes (separate `--benchmark-id` per shape is overkill; instead run all shapes in one example and let hotpath's per-function table separate them):

1. `ReadSelection` default, empty inbox (cold-path floor)
2. default selection over 500-message mailbox (the throughput case)
3. `contains_filter` scan (worst case — `MAX_CONTAINS_FILTER_LEN = 1024`, `read/mod.rs:28`)
4. `sender_filter` + `task_filter` combined
5. `peek_mail_with_runtime` (non-mutating, `DisplayMutationMode::NonMutatingPeek`, `read/mod.rs:402`) vs `read_mail_with_runtime` (mutating seen-state) — isolates seen-state write cost

**Instrumented functions:**

| Function | Location |
|---|---|
| `read_mail_with_runtime_impl` | `crates/atm-core/src/read/mod.rs:440` |
| `peek_mail_with_runtime_impl` | `read/mod.rs:376` |
| `resolve_read_context` | `read/state.rs` (called `read/mod.rs:393`) |
| `load_read_selection` | `read/state.rs` (called `read/mod.rs:395`) |
| `resolve_read_display` | `read/state.rs` (called `read/mod.rs:396`) |
| `filter_metadata_backed_contains_candidates` | `read/metadata_selection.rs` (imported `read/mod.rs:24`) |
| `load_durable_metadata_message` | `read/metadata_selection.rs` |
| `selection_state_for_mailbox_metadata_rows` | `read/metadata_selection.rs` |
| `sort_and_limit_selected` | `read/metadata_selection.rs` |
| `mailbox::source::resolve_target` | imported at `read/mod.rs:14` |

Add `hotpath-alloc` for this example specifically. The read path is the most allocation-sensitive surface (guidelines M-HOTPATH explicitly calls out `String`/collection cloning), and allocation counts are far less noisy on shared runners than wall-clock. **Allocation-byte deltas should be the primary regression signal for the read path; latency is secondary.**

### 3.3 Scope area 3 — cross-host / peer write delivery

Split into **two** benchmarks with different fidelity/cost/noise profiles. This directly answers the "two daemon instances vs. mocked peer" question: **do both, and gate them differently.**

#### 3.3a Mocked-peer micro-benchmark (per-PR)

**Example:** `crates/atm-bench/examples/peer_dispatch.rs` (`#[hotpath::main]`)

Exercises `PostWriteRouter::dispatch` (`peer_delivery_router.rs:15`) with the peer *wire* replaced by an in-memory sink. Covers everything up to and including the coordinator handoff, but not TLS/socket egress:

- `resolve_peer_authority` (`runtime_health/peer_authority.rs`, called `peer_delivery_router.rs:40`) against a `peer_config_store` (`crates/atm-storage-rusqlite/src/peer_config_store.rs`) seeded with 1, 8, and 64 trusted peers — this measures the `list_trusted_peers()?` + resolve cost, which is an O(n) scan per write and a genuine regression candidate as peer lists grow.
- `record_peer_delivery_event` (`peer_delivery_router.rs:43`, `:107`)
- `deliver_to_peer` → `peer_delivery_coordinator.deliver_after_persist` (`peer_delivery_router.rs:91`)
- `emit_local_post_write` short-circuit (`peer_delivery_router.rs:57`) — the local-only baseline

The mock is injected via the existing `PeerWire` abstraction; if no trait seam exists at the coordinator boundary, `bench-harness` exposes a constructor that takes a stub. **Cheap (single process, no TLS handshake, no sockets), deterministic, low-noise → runs on every PR.**

#### 3.3b Two-instance loopback benchmark (scheduled)

**Example:** `crates/atm-bench/examples/peer_delivery_loopback.rs` (`#[hotpath::main]`)

Both "hosts" run **in one process**, each with its own `ATM_HOME` temp dir and its own SQLite store:

1. Instance B binds a real HTTPS ingress via `PeerWire::bind_enabled` (`https_transport.rs:320`) with an `rcgen`-generated self-signed cert — `rcgen 0.13` is already a dev-dependency (`crates/atm-daemon/Cargo.toml:45`), so no new dependency risk.
2. Instance A's `peer_config_store` is seeded with B as a trusted peer at `127.0.0.1:<port>`.
3. A local write addressed to `agent@team@<B-host-label>` is issued into A, driving the full `dispatch → resolve_peer_authority → deliver_to_peer → peer_drain_coordinator → https_transport` egress and B's ingress + peer-receipt post-write (`peer_delivery_router.rs:21-30`).

A `plaintext_test` variant using `PeerWire::bind_plaintext_test` (`https_transport.rs:346`) runs alongside, so the TLS handshake cost is isolable by subtraction (`tls_enabled_total − plaintext_total`).

**Why one process, not two:** a single hotpath collector must see both the sender-side and receiver-side frames for the comparison table to be meaningful. Two processes would require merging two JSON files, which `hotpath-utils profile-pr` does not do (it takes exactly one `--head-metrics` and one `--base-metrics`).

**Why scheduled, not per-PR:** TLS handshakes + loopback sockets + two SQLite stores on a 2-core runner produce the noisiest numbers in the whole plan. Per-PR it would generate constant false ⚠️ markers and train reviewers to ignore the comment. On a nightly schedule against `develop` it becomes a trend line, which is what it's actually good for.

**Risk to call out now:** the daemon has host-ownership and singleton enforcement (`crates/atm-daemon/src/host_ownership.rs`, `scripts/lint_daemon_singleton.py`, and the `run_thorough_shared_host.py` smoke at `ci.yml:176`). Two in-process instances may trip a same-host singleton guard. If it does, `bench-harness` must expose an explicit "isolated instance" constructor that takes the home dir rather than resolving it globally — `prepare_runtime_at_socket_path_for_home` (`local_ipc_transport.rs:844`) suggests that seam already exists on the local side; the equivalent needs verifying on the peer/HTTPS side. **This is the highest-uncertainty item in the plan (see §6, R-3).**

---

## 4. GitHub Actions design

### 4.1 Why two workflows

atm-core PRs are currently internal (branch-based, targeting `develop`/`main`/`integrate/*` per `ci.yml:5`), so fork write-permission restrictions do not bite *today*. But hotpath's split design should still be adopted verbatim, because:
- it is the upstream-supported shape (deviating means owning the divergence),
- `pull_request_target` alternatives are a known security footgun (they check out untrusted code with write tokens),
- if the repo ever accepts an external contribution, the single-workflow design silently breaks with a 403 on comment.

### 4.2 `hotpath-profile.yml`

```yaml
name: Hotpath Profile
on:
  pull_request:
    branches: [develop, main, "integrate/*"]
permissions:
  contents: read            # deliberately no write / no pull-requests scope
concurrency:
  group: hotpath-profile-${{ github.event.pull_request.number }}
  cancel-in-progress: true
```

Single job `profile` on `ubuntu-latest`, `continue-on-error: true`.

Steps:
1. `actions/checkout@v4` with `fetch-depth: 0` (needed to check out the base SHA).
2. `dtolnay/rust-toolchain@stable` pinned to `"1.94.1"` — must match `ci.yml:25` exactly; a toolchain skew between the head and base runs would show up as a phantom regression.
3. Cargo caches with keys **distinct from** the existing `ci.yml` keys (`ci.yml:129-145` uses `${{ runner.os }}-cargo-build-target-...`). Use `hotpath-${{ runner.os }}-...` — sharing the `target` cache with a differently-featured build causes constant rebuilds and cache thrash.
4. **Head run:**
   ```
   HOTPATH_OUTPUT_FORMAT=json
   HOTPATH_OUTPUT_PATH=metrics/head-<id>.json
   cargo run --release -p atm-bench --features hotpath --example <name>
   ```
   for each of: `read_path`, `local_admission`, `peer_dispatch`.
5. **Base run:** `git worktree add ../base $(git merge-base HEAD origin/${{ github.base_ref }})` — use the merge-base, not the base tip, so unrelated commits landing on `develop` during a long-lived PR do not distort the comparison. Re-run the same three examples with `HOTPATH_OUTPUT_PATH=metrics/base-<id>.json`.
   - **Critical failure mode:** if a benchmark example is *new* in this PR, the base build fails. The step must tolerate a missing/failing base example and emit an empty-but-valid base JSON so `hotpath-utils` renders everything as 🆕 rather than the whole job failing.
6. Write `metrics/pr-number.txt` (the comment workflow cannot read PR context from a `workflow_run` payload reliably).
7. `actions/upload-artifact@v4` name `hotpath-metrics`, `retention-days: 7`.

### 4.3 OS-leg scope — **ubuntu-only per PR**

**Decision: per-PR profiling runs on `ubuntu-latest` only.** Rationale:

- **Cost.** Each PR run compiles the workspace in release **twice** (head + base). The existing `test` job already does a debug + release build per OS (`ci.yml:147-151`) and takes the longest wall-clock in CI. Adding 3 OS × 2 builds would roughly double total CI minutes. GitHub bills macOS runners at 10× and Windows at 2×; a 3-OS hotpath leg would dominate the entire repo's Actions spend for a signal that is mostly noise on the non-Linux runners.
- **Signal quality.** macOS GitHub runners are the noisiest of the three for wall-clock timing. Windows adds the 25ms accept-poll quantization (§3.1).
- **Coverage argument.** The Unix UDS path and the Windows TCP path converge on the same `ApiRouter` dispatch immediately after admission. The transport-specific delta is genuinely platform-specific, but it is *admission* cost, which is the least algorithmically interesting part of the path. Regressions in the shared dispatch/read/peer code — where real regressions live — are fully visible on Linux.

**Scheduled coverage** fills the gap: a `hotpath-nightly.yml` (`schedule: cron` daily, plus `workflow_dispatch`) runs on `[ubuntu-latest, macos-latest, windows-latest]` against `develop` HEAD, including `local_admission_tcp` (Windows) and `peer_delivery_loopback` (all three). It uploads artifacts and, on ubuntu, compares against the previous nightly. It never comments on PRs and never gates.

Summary matrix:

| Benchmark | PR (ubuntu) | Nightly (3 OS) |
|---|---|---|
| `read_path` | ✅ | ✅ |
| `local_admission` (UDS) | ✅ | ✅ unix legs |
| `local_admission_tcp` | ❌ | ✅ windows leg |
| `peer_dispatch` (mocked) | ✅ | ✅ |
| `peer_delivery_loopback` (2-instance) | ❌ | ✅ |

### 4.4 `hotpath-comment.yml`

```yaml
name: Hotpath Comment
on:
  workflow_run:
    workflows: ["Hotpath Profile"]
    types: [completed]
permissions:
  pull-requests: write
  contents: read
```

Steps: download the `hotpath-metrics` artifact from the triggering run, read `pr-number.txt`, install `hotpath-utils`, then one invocation per benchmark:

```
hotpath-utils profile-pr \
  --head-metrics metrics/head-read-path.json \
  --base-metrics metrics/base-read-path.json \
  --github-token ${{ secrets.GITHUB_TOKEN }} \
  --pr-number "$PR" \
  --benchmark-id read-path \
  --emoji-threshold 35
```

`--benchmark-id` is **mandatory per benchmark** — without it the three comparisons collide into one comment and overwrite each other. Threshold `35` rather than the default `20`, because GitHub runner variance on this workload class routinely exceeds 20% run-to-run; a 20% threshold would flag noise on most PRs and destroy the signal's credibility within two weeks.

**Do not** `cargo install --path crates/hotpath` (the upstream doc's in-repo form). Use `cargo install hotpath-utils --version <pinned>` or `taiki-e/install-action`, consistent with how `ci.yml:36-51` pins tool versions (`cargo-deny@0.19.9`). An unpinned tool version would silently change comment formatting and thresholds.

This workflow **must not** check out PR head code. It only handles JSON artifacts and a token. That is the entire security rationale for the split.

---

## 5. Gating philosophy

### Decision: **informational PR comment only. Never a required check. Never `-D`-style failure.**

Concretely:
- `hotpath-profile` runs `continue-on-error: true` and is **not** added to branch protection required checks.
- `hotpath-comment` posts and exits 0 regardless of deltas.
- No workflow step ever calls `exit 1` on a threshold breach.

### Why, and how it interacts with the 0-Blocking merge gate

atm-core's merge policy requires **0 Blocking findings** on a per-branch merge (with the full 0B+0I+0m gate reserved for phase-final merges). Findings are canonical `.ttl` triage records produced by QA agents — a deliberate, human/agent-reviewed pipeline. Injecting an automated statistical signal into that gate would be a category error:

1. **False positives would become merge blockers.** GitHub-hosted runners are shared, burstable, and thermally variable. A 40% p99 swing on the `local_admission` benchmark with zero code change is entirely plausible (96 live threads on 2 cores). Under a hard gate, every such swing becomes a Blocking finding that must be triaged, re-run, and waived — exactly the "suspicious waiver ruling" churn the project already guards against.
2. **The 0-Blocking gate derives its authority from being trustworthy.** Adding a noisy automated producer of Blocking findings degrades the credibility of every Blocking finding.
3. **Real regressions are not single-PR events in this codebase.** The daemon's cost profile changes through structural work (transport consolidation, deletion sprints), not through single hot-loop edits. A trend line across nightlies catches those; a per-PR threshold does not.

### The escalation path that *does* touch the gate

The hotpath comment is **evidence**, not a verdict. A regression becomes a Blocking finding only through the normal triage pipeline:

1. hotpath comment shows a ⚠️ on a function in the **critical-function list** (an explicit, checked-in list — see §6 file list — containing e.g. `read_mail_with_runtime_impl`, `handle_connection`, `resolve_peer_authority`).
2. QA reproduces it: re-run the profile workflow (or `workflow_dispatch` the nightly) and confirm the regression persists across ≥2 independent runs.
3. If confirmed **and** the delta exceeds the escalation threshold (proposal: **>50% mean or p99, or >2× allocated bytes** on a critical-path function), QA writes a canonical `.ttl` finding.
4. Default severity: **Minor**, upgraded to **Blocking** only when the regression pushes a path toward the 3s `REQUEST_DEADLINE` (`local_ipc_transport.rs:52`) or represents an algorithmic complexity change (e.g. `resolve_peer_authority` going superlinear in peer count).

This keeps hotpath entirely outside CI gating mechanics while still giving it a route to Blocking severity when a human/QA agent confirms a real problem. It is consistent with the existing rule that quality-mgr is the sole completion authority — a CI comment is not a QA verdict.

### Allocation as the exception

Allocation-byte counts from `hotpath-alloc` on `read_path` are **deterministic** — they do not vary with runner load. A tighter escalation threshold is justified there (>10% allocated-bytes growth on a read-path function warrants a Minor finding on first observation, no reproduction run required). This is the one place where the signal is strong enough to act on immediately, and it is also where guidelines M-HOTPATH says the wins are ("frequent re-allocations, esp. cloned, growing or `format!` assembled strings").

---

## 6. File-by-file change list

### New files

| Path | Action | Contents / notes | Effort |
|---|---|---|---|
| `crates/atm-bench/Cargo.toml` | create | `publish = false`, `[features] hotpath = [...]`, deps on `agent-team-mail-core`, `atm-daemon` (with `bench-harness`), `atm-daemon-client`, `atm-runtime-test-support`, `tempfile`, `rcgen`, optional `hotpath` | 0.25d |
| `crates/atm-bench/README.md` | create | how to run locally, why the crate exists, iteration-count rationale | 0.25d |
| `crates/atm-bench/src/lib.rs` | create | crate docs (M-MODULE-DOCS), re-exports of the harness modules | 0.25d |
| `crates/atm-bench/src/corpus.rs` | create | deterministic corpus builder; RULE-008-safe identity literals | 1.0d |
| `crates/atm-bench/src/runtime.rs` | create | SQLite runtime fixture over `SqliteRuntimeGuard` | 0.5d |
| `crates/atm-bench/src/daemon.rs` | create | in-process daemon fixture (UDS/TCP), `Drop` shutdown | 1.5d |
| `crates/atm-bench/src/load.rs` | create | fixed-iteration serial + concurrent drivers | 0.75d |
| `crates/atm-bench/src/peer.rs` | create | two-instance loopback fixture + mock peer wire | 2.0d |
| `crates/atm-bench/examples/read_path.rs` | create | 5 query shapes, `#[hotpath::main]` | 1.0d |
| `crates/atm-bench/examples/local_admission.rs` | create | UDS, serial + 48-concurrent | 1.0d |
| `crates/atm-bench/examples/local_admission_tcp.rs` | create | `#[cfg(windows)]` loopback TCP variant | 0.75d |
| `crates/atm-bench/examples/peer_dispatch.rs` | create | mocked-peer, 1/8/64 trusted peers | 1.0d |
| `crates/atm-bench/examples/peer_delivery_loopback.rs` | create | two-instance TLS + plaintext | 1.5d |
| `.github/workflows/hotpath-profile.yml` | create | §4.2 | 0.75d |
| `.github/workflows/hotpath-comment.yml` | create | §4.4 | 0.5d |
| `.github/workflows/hotpath-nightly.yml` | create | §4.3, 3-OS schedule | 0.5d |
| `docs/performance/critical-functions.md` | create | the escalation list referenced in §5; also satisfies M-HOTPATH's "document the most performance sensitive areas" | 0.5d |

### Modified files

| Path | Action | Changes | Effort |
|---|---|---|---|
| `Cargo.toml` (root, line 2-20) | modify | add `"crates/atm-bench"` to `members`; add `hotpath` to `[workspace.dependencies]` with a pinned version | 0.1d |
| `Cargo.toml` (root) | modify | add `[profile.bench] debug = 1` and consider `[profile.release] debug = 1` for the bench feature path — M-HOTPATH explicitly recommends this for meaningful attribution | 0.1d |
| `crates/atm-core/Cargo.toml` | modify | optional `hotpath` dep + `hotpath` feature | 0.1d |
| `crates/atm-core/src/read/mod.rs` | modify | `#[cfg_attr(feature = "hotpath", hotpath::measure)]` on `read_mail_with_runtime_impl` (`:440`), `peek_mail_with_runtime_impl` (`:376`) | 0.25d |
| `crates/atm-core/src/read/state.rs` | modify | attributes on `resolve_read_context`, `load_read_selection`, `resolve_read_display` | 0.25d |
| `crates/atm-core/src/read/metadata_selection.rs` | modify | attributes on the 4 selection helpers | 0.25d |
| `crates/atm-core/src/mailbox/source.rs` | modify | attribute on `resolve_target` | 0.1d |
| `crates/atm-core/src/api/` (framing + `decode_request`) | modify | attributes on `read_http_request`, `write_http_response`, `decode_request` | 0.25d |
| `crates/atm-daemon/Cargo.toml` | modify | optional `hotpath` dep, `hotpath` + `bench-harness` features | 0.1d |
| `crates/atm-daemon/src/lib.rs` | modify | `#[cfg(feature = "bench-harness")] pub mod` re-export shim for the enumerated constructors | 0.5d |
| `crates/atm-daemon/src/local_ipc_transport.rs` | modify | `bench-harness`-gated visibility on `prepare_runtime_at_socket_path_for_home` (`:844`), `serve_with_runtime_hooks` (`:328`), `MAX_CONCURRENT_CONNECTIONS` (`:51`) | 0.5d |
| `crates/atm-daemon/src/local_ipc_transport/request_worker.rs` | modify | attributes on `handle_connection` (`:40`), `read_bounded_http_request` (`:225`), `dispatch_request` (`:157`), `spawn_dispatch_worker` (`:189`), `await_dispatch_response` (`:251`) | 0.5d |
| `crates/atm-daemon/src/local_tcp_transport.rs` | modify | equivalent attributes + `bench-harness` visibility | 0.5d |
| `crates/atm-daemon/src/active_connection_registry.rs` | modify | attributes on `push_dispatch_handle`, `reap_finished_dispatches` | 0.25d |
| `crates/atm-daemon/src/runtime_health/peer_delivery_router.rs` | modify | attributes on `dispatch` (`:15`), `deliver_to_peer` (`:77`), `emit_local_post_write` (`:57`), `record_peer_delivery_failure` (`:100`) | 0.25d |
| `crates/atm-daemon/src/runtime_health/peer_authority.rs` | modify | attribute on `resolve_peer_authority` | 0.1d |
| `crates/atm-storage-rusqlite/src/peer_config_store.rs` | modify | attribute on `list_trusted_peers` (called every peer write, `peer_delivery_router.rs:40`) — requires adding the `hotpath` feature to this crate too | 0.25d |
| `crates/atm-daemon/src/https_transport.rs` | modify | `bench-harness` visibility on `bind_enabled` (`:320`) / `bind_plaintext_test` (`:346`); attributes on the egress send path | 0.75d |
| `crates/atm-daemon/src/peer_drain_coordinator.rs` | modify | attribute on `deliver_after_persist` | 0.1d |
| `.just/lint_sc_boundary.py` (or its config) | modify | allowlist the `bench-harness` re-export shim | 0.5d |
| `.just/lint_manifests.py` (or its config) | modify | accept the new `publish = false` member crate | 0.25d |
| `.just/check_line_counts.py` config | modify | if it enumerates crates, register `crates/atm-bench` | 0.1d |
| `deny.toml` | modify | license/advisory clearance for `hotpath` and its transitive deps | 0.25d |
| `justfile` | modify | add a `bench` recipe (`just bench [name]`) so contributors can reproduce CI locally | 0.25d |
| `docs/requirements.md` | modify | document the performance-regression subsystem and its non-gating status | 0.5d |

### Effort roll-up

| Phase | Content | Effort |
|---|---|---|
| **P0 — Read path only** | `atm-bench` skeleton + `corpus.rs` + `runtime.rs` + `read_path.rs` + `atm-core` instrumentation + both workflows (single benchmark) + lint/manifest/deny fixes | **~6 dev-days** |
| **P1 — Local admission** | `bench-harness` feature + `daemon.rs` + `load.rs` + `local_admission.rs` + daemon instrumentation | **~5 dev-days** |
| **P2 — Peer (mocked)** | `peer.rs` mock half + `peer_dispatch.rs` + peer instrumentation | **~3 dev-days** |
| **P3 — Nightly + Windows + two-instance** | `hotpath-nightly.yml` + `local_admission_tcp.rs` + `peer_delivery_loopback.rs` + `peer.rs` loopback half | **~4.5 dev-days** |
| **Total** | | **~18.5 dev-days** (~3 sprint-weeks single-dev) |

P0 is independently shippable and delivers most of the value. **Do not bundle P0–P3 into one PR** — the visibility changes in P1 will attract far more review scrutiny than P0's additive attributes, and coupling them delays the useful part.

---

## 7. Open questions and risks

### R-1 — Global allocator conflict (**high**, blocks P0's alloc tracking)
Guidelines M-MIMALLOC-APPS recommends `mimalloc` as the global allocator for applications. `hotpath-alloc` works by installing its own tracking global allocator. Only one `#[global_allocator]` may exist per binary. **Question:** does atm-core currently set `mimalloc` anywhere (`crates/atm/src/main.rs`, `crates/atm-daemon/src/main.rs`)? If yes, the `hotpath-alloc` benchmark binaries must not link it — resolvable because the benchmarks are separate `atm-bench` example binaries with their own `main`, but it means **allocation numbers are measured under a different allocator than production**. That is fine for *relative* deltas, misleading for absolute figures. Document this explicitly in `crates/atm-bench/README.md`.

### R-2 — Instrumentation overhead vs. the 3s deadline (**medium**)
`REQUEST_DEADLINE` is 3s (`local_ipc_transport.rs:52`) and `await_dispatch_response` (`request_worker.rs:251`) hard-fails past it, returning `DaemonMayHaveExecuted` for side-effecting requests (`request_worker.rs:298`). Under `--features hotpath` with high instrumentation density on a loaded runner, requests could plausibly cross that budget and turn the benchmark into a timeout-error benchmark. **Mitigation:** the bench harness must assert a zero-error invariant and fail loudly if any response is an `AtmError`; a benchmark producing errors produces garbage metrics. Also consider a `bench-harness`-only deadline override.

### R-3 — Two in-process daemon instances may be structurally impossible (**high**, scopes P3)
Host ownership (`crates/atm-daemon/src/host_ownership.rs`), the shutdown beacon (`shutdown_beacon.rs`), `scripts/lint_daemon_singleton.py`, and the `run_thorough_shared_host.py` smoke (`ci.yml:176`) all suggest strong same-host singleton enforcement. If instance A and instance B cannot coexist in one process (shared statics, a process-global runtime store — note `default_runtime()` at `crates/atm-core/src/read/mod.rs:348` resolves a *global*), the loopback peer benchmark must fall back to two processes, and then only the sender side is instrumentable in one artifact. **Spike this before committing to P3.** Fallback: instrument only the sender side and treat B as an unmeasured black box — still useful, since the sender-side resolve+coordinator cost is the part we control.

### R-4 — Runner variance destroying the signal (**medium**, mitigated by design)
2-core `ubuntu-latest` runners under a 96-thread benchmark. **Mitigations already in the plan:** `--emoji-threshold 35`; allocation-first interpretation for the read path; the 48-concurrent profile deliberately under the 64 cap; informational-only gating; nightly trend lines. **Additional mitigation to evaluate:** pin the profile job to a self-hosted runner if one becomes available (a Windows host became available to this project recently; a dedicated Linux bench host would materially improve signal quality and is the single highest-leverage improvement to this whole system).

### R-5 — Base-branch build cost (**medium**)
Building head + base in release doubles the job. With the merge-base checkout, the base build is highly cacheable across PR pushes (the merge-base rarely moves), so a dedicated `hotpath-base-${{ merge-base-sha }}` cache key should be added. Without it, expect ~20–30 min per profile run.

### R-6 — `--all-targets` clippy compiles the bench examples (**low**)
`ci.yml:109` runs `cargo clippy --workspace --all-targets -- -D warnings`. With `atm-bench` in `members`, its examples are compiled *without* the `hotpath` feature — which is fine (attributes vanish), but the examples still must be clippy-clean at pedantic level and under the function-length and RULE-003 line limits. Budget for this; it is why the harness is split across five `src/` modules rather than one.

### R-7 — `cargo-shear` and `cargo-deny` on optional deps (**low**)
`cargo-shear` (`justfile:42`) flags unused dependencies. An `optional = true` `hotpath` dep referenced only inside `#[cfg_attr]` may or may not be detected as used depending on shear's analysis. If it false-positives, an explicit ignore entry is needed. `deny.toml` must clear hotpath's license and advisory status before the first PR.

### R-8 — MSRV (**low but must verify first**)
Everything is pinned to Rust `1.94.1` (`Cargo.toml:26`, `ci.yml:25`) and edition 2024. **Verify hotpath's MSRV and edition compatibility before any implementation work begins.** If hotpath requires a newer toolchain, the entire plan is blocked on either a workspace MSRV bump (which touches the release contract) or a separately-pinned toolchain for the bench crate only (feasible via `rust-toolchain.toml` scoping, but ugly).

### R-9 — New-benchmark bootstrapping (**low, but a guaranteed first-run failure**)
The very first PR that introduces `hotpath-profile.yml` has no base metrics — the base commit has no `atm-bench` crate. The base-run step must tolerate a total build failure and synthesize an empty base JSON. Without this, the introducing PR's own CI comment fails and creates a confusing first impression.

### R-10 — RULE-008 identity literals in fixtures (**low, easily missed**)
`.just/check_test_identity_literals.py` enforces RULE-008. The corpus builder generates team/agent names; it must never emit the `atm-dev` literal and should use `"test-team"`-style names. This check may only currently scan `#[cfg(test)]` and `tests/` — if it does not scan `examples/` and non-test `src/` in a bench crate, **extend it**, because the bench corpus is exactly the kind of code that would otherwise drift.

### Open questions requiring a decision from the project owner
1. **Self-hosted bench runner** — is one available or acquirable? This changes the gating calculus materially (a dedicated host could justify tightening `--emoji-threshold` to 15 and would make R-4 largely moot).
2. **Escalation thresholds** — are >50% latency / >10% allocation the right lines for a `.ttl` finding? I have proposed them; they should be ratified before the first comment lands so triage does not improvise.
3. **P3 scope** — proceed with the two-instance loopback benchmark only after the R-3 spike, or drop it and rely on the mocked-peer benchmark plus the existing cross-host smoke suite (`justfile:173`) for cross-host coverage? My recommendation: **spike R-3 for half a day; if singleton enforcement blocks it, drop P3's loopback benchmark entirely** rather than fighting the daemon's ownership model for a nightly-only signal.
