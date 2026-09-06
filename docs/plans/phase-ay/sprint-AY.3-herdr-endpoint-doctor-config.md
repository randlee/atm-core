---
id: AY.3
phase: AY
sprint: AY.3
title: Herdr endpoint doctor and daemon configuration
branch: feature/ay3-herdr-endpoint-doctor-config
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ay3-herdr-endpoint-doctor-config
integration_branch: integrate/phase-ay
status: draft
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_track: core
parallel_with: []
stack_parent: feature/ay2-herdr-transport-seam
pr_target: feature/ay2-herdr-transport-seam
target: feature/ay2-herdr-transport-seam
dependency_relations:
  - prerequisite: AY.2
    dependent: AY.3
    relation: must_follow
    rationale: AY.3 consumes the private HerdrIo CLI transport seam and portable fake/replay fixtures delivered by AY.2, then owns the complete public config/doctor surface; the AY.3 PR is stacked directly on AY.2.
  - prerequisite: AY.3
    dependent: AY.4
    relation: must_follow
    rationale: AY.4 consumes the complete typed endpoint observations, doctor projection, configuration wiring, and presence correlation delivered here.
  - prerequisite: AY.3
    dependent: AY.8
    relation: must_follow
    rationale: AY.8 extends the endpoint/transport contracts and edits the architecture gate established here, so its independent multi-parent branch is created only after AY.1, AY.2, and AY.3 merge.
---

# AY.3 — Herdr endpoint doctor and daemon configuration

Deliver the complete production endpoint-diagnostics boundary: validated
daemon configuration, composition into the CLI invoker and probe, one typed
observation per configured endpoint, presence correlation, deterministic human
and JSON doctor output, and machine-enforced ownership. Herdr remains optional
for startup and readiness; construction performs no Herdr I/O.

## Delivery topology and `/gh-stack`

AY.3 is the first child in the linear implementation stack:

```text
integrate/phase-ay <- AY.2 <- AY.3 <- AY.4 <- AY.5 <- AY.6 <- AY.7
```

Executors use the `/gh-stack` skill and noninteractive forms:

```bash
git config rerere.enabled true
git config remote.pushDefault origin
gh stack link --base integrate/phase-ay \
  feature/ay2-herdr-transport-seam \
  feature/ay3-herdr-endpoint-doctor-config
gh pr view feature/ay3-herdr-endpoint-doctor-config \
  --json headRefName,baseRefName,state
```

Append later children with `gh stack link <stack-number> <branch>`. `link` is
the `/gh-stack` operation for the external `sc-git-worktree` workflow and does
not create local stack tracking, so verify bases with `gh pr view --json`.
Phase AY forbids `gh stack rebase`, `gh stack sync`, and
`gh stack merge`; use merge commits and no force-push. Parent development
pushed, not QA completion, triggers a merge commit from AY.2 into AY.3 before
every development or fix round. Parent PRs merge into `integrate/phase-ay`
first.

AY.8 socket work is not in this stack. It becomes eligible only after AY.1,
AY.2, and AY.3 have merged and then runs independently in parallel with
AY.4–AY.7.

## Preconditions

- P-A — the Phase AY plan's P-A is satisfied: Phase AX has merged to develop,
  `integrate/phase-ay` was cut from that exact fetched develop head, and the
  recorded merge-base and symbol-presence checks all pass.
- P-B — the Phase AY plan has dated approval from Rand.
- AY.2 development and contracts are pushed. AY.3 is created from
  `feature/ay2-herdr-transport-seam`; AY.2 need not have merged for the stacked
  child to start.
- P-E(a) — after AY.2's private transport development/contracts are pushed and
  before AY.3 development begins, `boundary-guard` approves both
  `boundaries/atm-core/herdr-endpoint-doctor.toml` and the public-contract
  revision to `boundaries/atm-herdr/herdr-process-adapter.toml`, including the
  matching boundary-document inventory. The approved boundary bundle is
  AY.3's first commit. This review does not wait for AY.2 to merge, and the
  developer neither authors nor weakens the ruling.

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial or shape-only
completion fails the sprint.

- [ ] D1 — Add public, validated, `Clone` `HerdrClientConfig` in `atm-herdr`,
  plus `crates/atm-daemon-bootstrap/src/herdr_config.rs`, and compose one value
  into the single `HerdrProcessInvoker::new` site and `HerdrDoctorProbe`.
  Missing file or `[herdr]` table selects PATH lookup with no socket override;
  malformed or invalid config fails startup with structured
  `ConfigParseFailed` context. No Herdr command runs at startup.
- [ ] D2 — Add the complete public doctor data/probe surface: atm-core-owned
  DTOs in `doctor/herdr_state.rs`, concrete public `HerdrDoctorProbe` and
  exhaustive `HerdrError` mapping in `atm-herdr`, the public-item/signature
  architecture pins, and the approved atm-herdr boundary contract/docs
  inventory. Then replace `HerdrPresenceDoctor` with the ADR-001-sealed
  `HerdrEndpointDoctor`, `ClosedHerdrEndpointDoctor`, and exactly one production
  `HerdrEndpointDoctorAdapter`. Land the approved boundary bundle first, add
  both boundary-document updates, and enforce exactly two implementations in
  `crates/atm-architecture/tests/boundary_enforcement.rs`.
- [ ] D3 — Add the pure `herdr_is_configured` predicate and one-pass endpoint
  grouping/presence correlation. A single roster snapshot feeds both the
  configured decision and observations. The probe performs one bounded server
  query per endpoint and one bounded `get` per routed member under breaker
  bypass; no member is probed twice.
- [ ] D4 — Expose production-ready human and `atm doctor --json` Herdr output:
  configured status, deterministic endpoint list, endpoint provenance,
  transport, binary resolution, typed state/remedy, live-handoff capability,
  exact member outcomes, and the existing separate host-wide breaker report.
  Endpoint strings are privacy-preserving display values under C6; raw home,
  config-root, socket, and pipe values never cross the `atm-herdr` boundary.
  Document the schema in `docs/atm/cli-reference-1-5-0.md`.
- [ ] D5 — Add focused config, boundary, endpoint, state-mapping, presence, and
  rendering tests that close every contract and error row below on all three CI
  lanes. Remove `HerdrPresenceDoctor` completely. Add the long-lived-child
  architecture guard proving neither `atm-daemon-bootstrap` nor
  `atm-http-runtime` spawns `herdr server` or another long-lived Herdr child.

### Paths to delete

None.

## Required work and exact targets

The approved boundary bundle is the first commit. D1–D5 then land as one production
closure; there is no stage-only or boundary-only completion claim.

| Ownership | Exact targets |
| --- | --- |
| Core DTO/port/projection | `crates/atm-core/src/doctor/herdr_state.rs`, `crates/atm-core/src/doctor/mod.rs`, `crates/atm-core/src/doctor/report.rs`, `crates/atm-core/src/herdr_configured.rs` |
| Async doctor projection | `crates/atm-runtime/src/doctor_projection.rs` and Tokio/Axum doctor composition only |
| Herdr public config/probe | `crates/atm-herdr/src/transport.rs`, `crates/atm-herdr/src/doctor_probe.rs`, and `crates/atm-herdr/src/lib.rs` over AY.2's private transport seam |
| Composition/config | `crates/atm-daemon-bootstrap/src/herdr_config.rs`, `crates/atm-daemon-bootstrap/src/replacement_handler.rs`, and focused bootstrap tests |
| CLI rendering/docs | `crates/atm/src/output.rs`, `docs/atm/cli-reference-1-5-0.md` |
| Boundary enforcement | `boundaries/atm-core/herdr-endpoint-doctor.toml`, `boundaries/atm-herdr/herdr-process-adapter.toml`, `docs/atm-core/boundaries.md`, `docs/atm-herdr/boundaries.md`, `crates/atm-architecture/tests/boundary_enforcement.rs` |

`atm-core` remains I/O-free and Tokio-free and gains no `atm-core -> atm-herdr`
edge. Reuse the ADR-001 seal without changing `pub mod sealed` visibility or
authorizing another implementation. The legacy synchronous daemon is frozen:
do not patch, harden, remodel, or add Herdr behavior to it.

## Code contracts

### C1 — Public config, reader, and shared value

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HerdrClientConfig {
    binary_path: Option<PathBuf>,
    socket_path: Option<PathBuf>,
}

impl HerdrClientConfig {
    /// Pure fallible construction; both configured paths must be absolute.
    pub fn try_new(
        binary_path: Option<PathBuf>,
        socket_path: Option<PathBuf>,
    ) -> Result<Self, AtmError>;

    pub fn binary_path(&self) -> Option<&Path>;
    pub fn socket_path(&self) -> Option<&Path>;
}
```

`HerdrClientConfig` is one of exactly two new public `atm-herdr` items in this
sprint. Its fields remain private; `Default` is the valid no-path value and all
non-default construction goes through `try_new`, so an invalid relative path is
not representable through the public API. Construction performs no I/O and
returns the existing `AtmErrorCode::ConfigParseFailed`; no string error or new
error family is added.

```rust
// crates/atm-daemon-bootstrap/src/herdr_config.rs
pub(crate) fn daemon_herdr_client_config(
    env: &dyn EnvSource,
) -> Result<HerdrClientConfig, AtmError>;
```

The optional `[herdr]` table has only optional `binary_path` and `socket_path`
string keys and uses `deny_unknown_fields`. The reader calls this sprint's
`try_new`; it never probes the filesystem beyond reading `.atm.toml`.
`HerdrClientConfig` implements `Clone`, so composition reads and validates once
and gives the same value to both consumers:

```rust
let config = daemon_herdr_client_config(env)?;
let invoker = HerdrProcessInvoker::new(Arc::clone(&breaker), config.clone());
let probe = HerdrDoctorProbe::new(config);
```

| Failure | Code | Context and recovery |
| --- | --- | --- |
| Home cannot be resolved | `ConfigParseFailed` | detail identifies `.atm.toml [herdr]`; no source cause; configure a resolvable home |
| File unreadable | `ConfigParseFailed` | detail names the file; preserve I/O cause; fix access |
| TOML malformed | `ConfigParseFailed` | detail names the file; preserve parser cause; correct TOML |
| Unknown `[herdr]` key | `ConfigParseFailed` | detail names file and key; preserve parser cause; remove/correct key |
| Config validation fails | `ConfigParseFailed` | detail names file and key; preserve validation `AtmError`; use absolute paths |

### C2 — Public doctor DTOs and concrete probe

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HerdrTransportKind { Cli, Socket }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HerdrEndpointProvenance { Session, SocketPath, HerdrDefault }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct HerdrEndpointDisplay(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HerdrEndpointDisplayRoot {
    XdgConfigHome,
    Home,
    AppData,
    Configured,
}

impl HerdrEndpointDisplay {
    /// Accepts only relative normal components; root/prefix/parent components
    /// fail with ConfigParseFailed. `Configured` retains only the file name.
    pub fn from_relative(
        root: HerdrEndpointDisplayRoot,
        relative: &Path,
        named_pipe: bool,
    ) -> Result<Self, AtmError>;

    pub fn as_str(&self) -> &str;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HerdrBinaryProvenance { Configured, Path }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HerdrBinaryResolution {
    pub path: PathBuf,
    pub provenance: HerdrBinaryProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HerdrPresenceOutcome {
    Visible,
    Finding { finding: DoctorFinding },
    Infrastructure { code: AtmErrorCode, detail: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct HerdrVersion(String);

impl HerdrVersion {
    pub fn parse(value: impl Into<String>) -> Result<Self, AtmError>;
    pub fn as_str(&self) -> &str;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HerdrRosterMember {
    pub ordinal: usize,
    pub name: AgentName,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HerdrMemberPresence {
    #[serde(skip)]
    pub ordinal: usize,
    pub name: AgentName,
    pub outcome: HerdrPresenceOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HerdrEndpointObservation {
    pub session: Option<HerdrSession>,
    pub provenance: HerdrEndpointProvenance,
    pub transport: HerdrTransportKind,
    pub endpoint: Option<HerdrEndpointDisplay>,
    pub binary: Option<HerdrBinaryResolution>,
    pub state: HerdrDoctorState,
    pub live_handoff: Option<bool>,
    pub members: Vec<HerdrMemberPresence>,
}

pub struct HerdrDoctorProbe { /* private HerdrIo + public config */ }

impl HerdrDoctorProbe {
    pub fn new(config: HerdrClientConfig) -> Self;

    pub async fn observe(
        &self,
        session: Option<&HerdrSession>,
        members: &[HerdrRosterMember],
        deadline: RequestDeadline,
    ) -> HerdrEndpointObservation;
}
```

`HerdrVersion` has no public inner field. `parse`, `TryFrom<String>`, and custom
deserialization accept only the supported semantic-version syntax and map an
invalid server value to the typed unexpected-response path; `as_str`,
`Display`, and serialization expose it read-only.

`HerdrDoctorProbe` is the other new public `atm-herdr` item. The atm-herdr
boundary `[contracts]` inventory adds request types `HerdrClientConfig` and
`HerdrRosterMember`, response type `HerdrEndpointObservation`, and error type
`AtmError`. Its docs name the only `AtmError` construction sites:
`From<HerdrError> for AtmError` and `HerdrClientConfig::try_new`. The
architecture test pins the Phase AX public list plus only
`HerdrClientConfig` and `HerdrDoctorProbe` and pins both public signatures.

### C3 — Configured predicate and doctor port

```rust
pub fn herdr_is_configured(roster: &MembersList) -> bool {
    roster.members.iter().any(|member| matches!(
        member.local_message_received_backend(),
        Some(LocalMessageReceivedBackend::Herdr { .. })
    ))
}

pub trait HerdrEndpointDoctor:
    crate::boundary::sealed::Sealed + Send + Sync
{
    fn observe<'a>(
        &'a self,
        roster: &'a MembersList,
        caller_deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Vec<HerdrEndpointObservation>> + Send + 'a>>;
}

pub struct ClosedHerdrEndpointDoctor;
```

The closed implementation returns an empty vector. The bootstrap adapter is the
only production implementation and delegates endpoint work to this sprint's
concrete `HerdrDoctorProbe`, which uses AY.2's private transport seam.

### C4 — Typed endpoint state

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HerdrDoctorState {
    Ok { version: HerdrVersion, protocol: u32 },
    NotConfigured,
    BinaryNotFound { searched: Vec<PathBuf> },
    BinaryNotExecutable { path: PathBuf, cause: String },
    BelowMinimum { version: HerdrVersion, minimum: HerdrVersion },
    ServerNotRunning { endpoint_named_by_herdr: Option<HerdrEndpointDisplay> },
    ClientServerMismatch {
        client: Option<HerdrVersion>,
        server: Option<HerdrVersion>,
    },
    EndpointUnreachable { endpoint: HerdrEndpointDisplay },
    PermissionDenied { endpoint: HerdrEndpointDisplay },
    ProbeTimedOut { after: Duration },
    UnexpectedResponse { code: Option<String>, detail: String },
    Other { code: AtmErrorCode, detail: String },
}
```

Every variant has `remedy(&self) -> &'static str`. `atm-herdr`, which owns
`HerdrError` and `HERDR_MINIMUM_VERSION`, owns the exhaustive mapping. No listed
`HerdrError` may fall into `Other`; an unclassified future variant still
retains its stable `AtmErrorCode` and safe detail rather than a debug string.
There is no `BreakerOpen` endpoint state;
the existing `HerdrBreakerDoctorReport` remains separate.

### C5 — Presence projection

`presence_findings(&[HerdrEndpointObservation]) -> Vec<DoctorFinding>` flattens
all endpoint members, sorts by nonserialized roster ordinal, preserves each
typed `Finding`, remembers only the first infrastructure code/detail, and appends
exactly one global informational `HerdrUnavailable` after member findings.
`Visible` emits nothing. `HerdrMemberPresence` serializes exactly `name` and
`outcome`; no `member` wrapper or `ordinal` key is allowed.

### C6 — Doctor JSON

Endpoints are ordered default first, then named sessions bytewise. There is no
aggregate `herdr.state` or `herdr.remedy`.

Every endpoint-bearing string in this DTO, including endpoint fields inside a
state variant, uses `HerdrEndpointDisplay`. atm-herdr resolves the raw endpoint,
reduces it to a validated root plus relative components, and calls
`from_relative`; an architecture pin permits this construction only in the
atm-herdr sanitizer. The formatter emits `$XDG_CONFIG_HOME`, `$HOME`, or
`%APPDATA%`; an explicit path outside those roots emits
`<configured>/<file-name>`. The Windows `\\.\pipe\` prefix is retained after
the path portion is sanitized. Custom deserialization accepts only those
symbolic prefixes, and raw values remain transport inputs that never enter
atm-core, JSON, human output, snapshots, or logs.

```json
{
  "herdr": {
    "configured": true,
    "endpoints": [
      {
        "session": "default",
        "provenance": "herdr_default",
        "transport": "cli",
        "endpoint": null,
        "binary": {
          "path": "/absolute/path/to/herdr",
          "provenance": "path"
        },
        "state": {
          "kind": "ok",
          "version": "0.8.2",
          "protocol": 20
        },
        "remedy": "none",
        "capabilities": { "live_handoff": true },
        "members": [
          { "name": "fenix", "outcome": { "kind": "visible" } }
        ]
      }
    ],
    "breaker": {
      "state": "closed",
      "retry_after_ms": null,
      "consecutive_failures": 0
    }
  }
}
```

On CLI transport, `endpoint` is `null`; atm does not compute a socket path.
`live_handoff` is true/false whenever the server answered, including mismatch
and below-minimum states, and null only when retrieval failed. Unreadable config
or roster yields `configured: null` plus an error field, never a guessed value.

## Doctor error inventory

| State code | Cause | Required recovery |
| --- | --- | --- |
| `ok` | Compatible endpoint answered | None |
| `not_configured` | No Herdr-backed member | Configure Herdr only if desired |
| `binary_not_found` | CLI binary unresolved | Install Herdr or correct `binary_path` |
| `binary_not_executable` | Resolved path invalid | Correct permissions/path |
| `below_minimum` | Server below minimum | Upgrade to `HERDR_MINIMUM_VERSION` or later |
| `server_not_running` | Endpoint reports stopped | Start the endpoint as the same user |
| `client_server_mismatch` | Client/server protocols differ | Use AY.6 restart coordination or Herdr handoff |
| `endpoint_unreachable` | Socket/pipe absent or refused | Start Herdr or correct endpoint config |
| `permission_denied` | Account cannot access endpoint | Align per-user ownership/permissions |
| `probe_timed_out` | Bounded deadline exhausted | Inspect Herdr health and retry |
| `unexpected_response` | Malformed/oversized/unrecognized response | Verify supported Herdr and capture response |
| `other` | Unclassified closed-enum error | Follow detail and file a compatibility finding |

## Acceptance criteria

- [ ] A1 — Config fixtures prove every C1 row, error code, detail, key/file
  name, source-cause rule, default case, both keys, and relative-path refusal;
  a source/API pin proves config fields are private and every non-default value
  is constructed by `try_new`.
- [ ] A2 — The P-E-approved boundary bundle is the first commit; `just lint boundaries`
  passes; the architecture test finds exactly the closed and bootstrap adapter
  implementations, pins the two public atm-herdr items/signatures and complete
  request/response/error inventory; `rg "HerdrPresenceDoctor"` finds nothing.
- [ ] A3 — Every state-inventory row has human and JSON snapshots with its
  remedy, no listed `HerdrError` maps to `other`, and an injected unclassified
  variant preserves its typed code/detail without debug formatting. Version
  fixtures prove invalid strings cannot construct or deserialize
  `HerdrVersion`.
- [ ] A4 — Endpoint snapshots prove default/session ordering, all three
  provenances, mismatch capability retention, CLI `endpoint: null`, and exact
  member keys. Constructor/deserializer and construction-site pins prove raw or
  parent-traversing endpoint strings cannot inhabit `HerdrEndpointDisplay`.
- [ ] A5 — Presence zero-regression fixtures compare the complete ordered
  `Vec<DoctorFinding>` to Phase AX behavior for all statuses, typed failures,
  one/multiple infrastructure failures, and interleaved endpoints.
- [ ] A6 — No Herdr command runs during construction; daemon readiness remains
  independent of Herdr; `atm-core` has no Tokio or `atm-herdr` dependency.
- [ ] A7 — Merge gate is 0 blocking, 0 important, and 0 minor in scope;
  quality-mgr posts PASS and CI is green at merge time.

## Required validation

This is the authoritative validation list.

- [ ] V1 — `cargo test -p atm-core -p atm-herdr -p atm-daemon-bootstrap -p atm-runtime -p atm`
  exits zero for focused endpoint/config/doctor tests.
- [ ] V2 — `just lint boundaries` exits zero.
- [ ] V3 — `just validate` exits zero on all CI lanes.
- [ ] V4 — `python3 .just/check_line_counts.py` exits zero.
- [ ] V5 — `gh pr view feature/ay3-herdr-endpoint-doctor-config --json
  headRefName,baseRefName,state` reports base
  `feature/ay2-herdr-transport-seam`.

## Non-closure and out of scope

- Breaker-open escalation and end-to-end failure/recovery lifecycle tests are
  AY.4; this sprint closes the endpoint/config/doctor surface itself.
- Herdr entry installation and restart coordination are AY.5 and AY.6.
- Windows process fixes are AY.7; socket transport and cutover are AY.8/AY.9;
  live proof is release readiness, not a sprint.
- No legacy synchronous-daemon runtime or dispatch work is permitted.
