# Rust hot-path GitHub CI profiling plan

Status: design proposal; no workflow or benchmark code is changed by this
document.

## Objective and CI contract

Add a repeatable hotpath-rs benchmark suite that compares pull-request head
metrics with the PR base and publishes a useful, low-noise profiling diff. The
first iteration should cover the same three user-visible areas already treated
as performance-sensitive in atm-core: local UDS/TCP admission plus dispatch,
mailbox read throughput, and cross-host peer-write delivery.

The upstream design uses two workflows: a read-only `pull_request` profile job
that uploads head/base JSON artifacts, followed by a privileged
`workflow_run` comment job that downloads those artifacts and invokes
`hotpath-utils profile-pr`. This split is required for fork PRs, where the
profile job must not receive a write-capable token. The documented comparison
includes calls, average latency, p99, total time, and optional allocation
sections; the default warning emoji threshold is 20%. See
<https://hotpath.rs/github_ci> for the upstream contract.

## Benchmark examples

Keep each benchmark as a focused Rust example with a stable `benchmark-id`.
Each example has a warmup phase, a fixed operation count, deterministic fixture
data, and a small `#[hotpath::main]` entry point. The workload should call the
real production boundary, not a copy of the algorithm.

### 1. Local admission and dispatch

Add `crates/atm-daemon/examples/hotpath_local_transport.rs`.

The example starts the existing test runtime on an ephemeral UDS (Unix) or
loopback TCP endpoint (Windows), creates the same authenticated local request
shape as the CLI client, performs a warmup, then sends a fixed mix of read-only
requests through the local transport. The benchmark must report separate
labels for UDS and TCP when the platform supports the path; the common
`ApiRouter` dispatch remains comparable. Use the existing
`RuntimeComposition::start_with_socket_path_for_test`, local capability record,
and daemon-client request encoder rather than calling private worker helpers
directly. If those test constructors cannot be reused by an example, add a
narrow `test-support` feature/module exposing only an ephemeral endpoint
builder; do not make production transport internals public.

Measure admission/dispatch latency and throughput under sequential load first.
A second bounded batch (for example, 8 concurrent clients, below the current
64-connection cap) can reveal queueing without making the result a load test.
Stop the runtime and remove the endpoint before the example exits.

### 2. Mailbox read throughput

Add `crates/atm-core/examples/hotpath_read.rs`.

Construct a temporary mailbox/runtime fixture with deterministic messages and
run `peek_mail_with_runtime` and `read_mail_with_runtime` through the public
read API using a no-op observability port. Include three named cases: empty
mailbox, a representative mailbox (for example 1,000 messages with mixed
ack/read states), and a contains-filter query that forces body resolution.
Warm each case separately and use identical fixture generation for head and
base. Record selected-row count as benchmark metadata, not as a metric label,
so a changed fixture cannot masquerade as a performance change. The example
must exercise metadata selection, optional body loading, display construction,
and the read-state mutation path without depending on a developer's home
directory.

### 3. Cross-host peer delivery

Add `crates/atm-daemon/examples/hotpath_peer_delivery.rs`.

The preferred implementation starts two isolated daemon runtimes with distinct
temporary homes, trusted-peer records, and ephemeral HTTPS/plaintext-test
ports. It sends a fixed batch from source to receiver through the ordinary
`DaemonRequestDispatcher`, then verifies the receiver's persisted message and
the sender's delivery result before reporting metrics. This covers peer
authority lookup, drain admission, connection setup, wire request, remote
dispatch, and receipt handling.

There are two practical CI modes:

* **Deterministic PR mode:** run the peer router against a test coordinator or
  loopback peer transport that preserves the `RequestDeadline`, queue, and
  response contracts. This is fast and works on all three existing OS legs,
  but it must be clearly named as router/coordinator coverage and must not be
  presented as TLS latency.
* **End-to-end peer mode:** start two real daemon instances and use the
  existing plaintext-test or mTLS test harness with ephemeral ports. Run this
  on `ubuntu-latest` in the profile workflow initially, and schedule it on the
  other OSes only after startup and certificate timing are stable. A separate
  nightly/manual job can exercise all three OSes without making every PR wait
  for two daemon lifecycles.

The PR artifact should contain both modes when available, with distinct
benchmark IDs (`peer-router` and `peer-e2e`) so a mocked result never hides a
network regression.

## Profile workflow

Create `.github/workflows/hotpath-profile.yml` alongside, rather than inside,
the existing `ci.yml` jobs. Trigger on `pull_request` for the same protected
branches. Grant only `contents: read`; use pinned action SHAs in the eventual
implementation. The job should:

1. check out the PR head with full history and install the repository's pinned
   Rust toolchain;
2. install the exact hotpath release/tooling and cache Cargo dependencies using
   keys that include OS, toolchain, lockfile, and profiling feature set;
3. run the three examples in release mode with
   `HOTPATH_OUTPUT_FORMAT=json` and separate `HOTPATH_OUTPUT_PATH` files;
4. check out `${{ github.event.pull_request.base.sha }}` and repeat the exact
   commands and fixture sizes;
5. write PR number, head/base SHAs, benchmark IDs, OS, toolchain, and workload
   parameters into a small metadata file; and
6. upload the JSON files and metadata as one short-retention artifact.

Use a matrix only where it adds signal. Recommended initial matrix:

* Ubuntu: all three benchmarks, including the real two-daemon peer mode;
* macOS and Windows: local transport and read benchmarks, plus the deterministic
  peer-router mode; and
* a manual/nightly all-OS end-to-end peer job after the first results establish
  stable startup and TLS behavior.

This keeps the normal three-OS `ci.yml` correctness gate unchanged while
providing early portability signal. Do not run untrusted PR code in a
`pull_request_target` job or grant it repository write permissions.

## Comment workflow

Create `.github/workflows/hotpath-comment.yml` triggered by
`workflow_run` completion of `hotpath-profile`. Grant `contents: read` and
`pull-requests: write` only to this workflow. Before commenting, verify that
the completed run belongs to a pull request in this repository and that its
conclusion is successful. Download the named artifact using the run ID, install
the pinned `hotpath-utils` version (or a repository-pinned source revision),
and invoke one `profile-pr` call per benchmark ID with the recorded PR number
and head/base metrics.

The comment should be idempotent by benchmark ID and include the OS/mode in its
heading. Retain the raw artifact for one to three days for diagnosis, but do
not paste raw debug/allocation records into the PR. Treat artifact metadata as
untrusted input: validate SHAs and PR number before passing them to the CLI,
and never interpolate them into shell code without quoting.

If the upstream utility is not published in a reproducibly installable form,
vendor a minimal wrapper or pin its Git revision in a dedicated tool manifest;
do not use an unpinned `cargo install` in a privileged workflow.

## Regression policy and merge gating

Phase one is informational. The comment workflow may mark a benchmark row as a
warning when a metric exceeds the upstream 20% change threshold, but it must
not fail the required `ci.yml` check. This respects atm-core's existing
zero-Blocking merge-gate policy: correctness, security, and boundary checks
remain the only merge blockers while runner noise and benchmark evolution are
being characterized.

After at least several weeks of stable artifacts, define per-benchmark budgets
using a median of repeated samples and compare p99/total time separately. A
candidate hard gate should require both a percentage threshold (for example,
20% p99 or total-time regression) and an absolute floor to avoid flagging
sub-millisecond noise. Require the regression to reproduce across two runs or
be confirmed by a maintainer before changing the required-check policy.

Keep allocation and peer-E2E results informational until their fixture and
runner variance is understood. If a future gate is introduced, make it a
separate explicitly named required check rather than making the comment job's
generic failure status block all PRs. Allow a documented baseline refresh for
intentional algorithm or toolchain changes, recording the reason and the old/new
commit in the PR.

## File-by-file change list and effort

* `crates/atm-daemon/examples/hotpath_local_transport.rs`: local UDS/TCP
  example and bounded concurrency cases (1–1.5 days).
* `crates/atm-core/examples/hotpath_read.rs`: deterministic mailbox fixture and
  read/peek/filter cases (0.5–1 day).
* `crates/atm-daemon/examples/hotpath_peer_delivery.rs`: two-daemon harness,
  deterministic coordinator mode, verification, and cleanup (1.5–2 days).
* `crates/atm-daemon/src` test-support surface or a new shared helper under
  `crates/atm-daemon/tests/support`: ephemeral endpoint and isolated-home
  lifecycle APIs, only if existing helpers cannot be called from examples
  (0.5–1 day).
* `Cargo.toml`/crate manifests and `Cargo.lock`: dev-only hotpath dependency,
  feature selection, and pinned tool version (0.25 day).
* `.github/workflows/hotpath-profile.yml`: head/base matrix, caches, metrics
  artifact, metadata, and permissions (0.5–1 day).
* `.github/workflows/hotpath-comment.yml`: secure artifact download, utility
  install, idempotent comments, and permissions (0.5 day).
* `justfile` plus `docs/` operator/benchmark notes: local reproduction and
  baseline refresh procedure (0.25 day).

Expected initial effort: 5–7 engineer-days, including one pass to stabilize
the two-daemon peer fixture on CI runners and one review of workflow security.

## Open questions and risks

* Is the required hotpath crate/tool version available in a reproducible
  package form, and does its JSON schema remain stable enough to archive?
* Can examples access the daemon's current test runtime without widening
  production visibility, or is a small `test-support` API required?
* Should Windows local transport metrics be compared only to Windows base (the
  recommended choice) rather than cross-OS values? Absolute latency across OS
  runners is not comparable.
* Two-daemon TLS runs can fail for certificate, port, or startup reasons that
  are unrelated to performance. The benchmark must classify setup failures
  separately and retain logs as an artifact.
* GitHub-hosted runner hardware and load are noisy. Pin release profile,
  workload size, toolchain, and concurrency; compare head/base on the same job
  whenever possible.
* Fork PRs cannot write comments from the profile job. The workflow-run
  handoff must reject stale or unrelated artifacts and avoid executing PR code
  in the privileged comment job.
* A toolchain upgrade can shift all timings. Require explicit baseline refresh
  metadata instead of silently accepting a new baseline.
