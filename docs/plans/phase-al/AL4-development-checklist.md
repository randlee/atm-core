# AL.4 Development Checklist — Shared Standard HTTP Client

Baseline: `integrate/phase-al` at `ffcceae1`; worktree:
`feature/pal-s4-shared-client`.

- [x] **AL4-01 — Inventory and async boundary.** Located the four frozen
  implementations and their `Arc<dyn DaemonApiClient>` callers: CLI, graft,
  fake, and loopback; migrated the existing sealed trait and all four to
  `#[async_trait]`.
- [x] **AL4-01a — Resolve adapter activation boundary.** Team-lead approved
  the write-only boundary: the CLI and graft write call chains await
  `DaemonApiClient`; physical connector construction remains AL.5–AL.7 and
  bootstrap/probe plus non-write routes retain direct dispatch until then.
- [x] **AL4-02 — Shared client contract.** Added one connector-neutral
  `HttpRuntimeClient<Connector>` in `atm-http-runtime`, with exactly one
  canonical request encoder, `/v1/atm/messages` selector, response decoder,
  and outcome mapper. Do not construct connectors in this sprint.
- [x] **AL4-03 — Budget and failure mapping.** Threaded one absolute request
  deadline through endpoint resolution, connect, TLS, write, first-byte/body
  response, cancellation, and shutdown. Map each named failure to the
  existing structured `AtmError` contract without retry/replay.
- [x] **AL4-04 — Test connectors and framework facilities.** Added
  deterministic connector tests for future UDS, loopback, and TLS callers;
  use Tokio time control for timeout/cancellation. Add source guards forbidding
  raw framing, `block_on`, manual future vtables, a second client trait, and
  retry/replay or `message[]` construction.
- [x] **AL4-05 — Graft outbound migration.** Migrated the graft public send
  path to the shared async client boundary while retaining its physical legacy
  direct-dispatch adapter under the approved AL.4 activation boundary; graft
  startup remains independent and neither daemon nor HTTP runtime depends on
  `atm-graft`.
- [x] **AL4-06 — Compatibility and every implementation.** Updated each
  allowlisted `DaemonApiClient` implementation and compatibility/preflight
  flow to compile and use the shared encoding/decoding path.
- [x] **AL4-07 — Sprint closure.** Reviewed all changed boundaries and all
  async call sites, then passed `cargo check --workspace --all-targets`, the
  focused runtime/graft/Python/CLI/core/architecture suites, `just test`, and
  `just lint all`. The graft smoke example is now an async Tokio entry point;
  it is covered by the workspace target check. Commit, push, and QA request
  follow this checklist update.
