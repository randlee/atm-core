---
title: AF-2 — Observability and release gates
status: complete
branch: integrate/phase-AF
worktree: /Users/randlee/Documents/github/atm-core-worktrees/integrate/phase-AF
---

# AF-2 — Observability and release gates

## Sprint intent

Close the remaining 1.3.0 dogfood findings at production quality after AF-1's
singleton design is accepted. This sprint makes the active delivery path and
daemon health diagnosable, and makes release smoke reproducible against the
installed artifacts.

## Boundary contracts

The following report and wire-boundary shapes are mandatory; field names may
be refined only without weakening their information or redaction guarantees.

```rust
pub struct PostSendDoctorReport {
    pub config_root: PathBuf,
    pub external_rules: Vec<PostSendHookRuleReport>,
    pub recipient_paths: Vec<RecipientDeliveryPathReport>,
}

pub struct PostSendHookRuleReport {
    pub recipient_matcher: String,
    pub executable: PathBuf,
    pub argv: Vec<String>,
    pub config_root: PathBuf,
}

pub struct RecipientDeliveryPathReport {
    pub recipient: AgentName,
    pub path: RecipientDeliveryPath,
}

pub struct HookRuleIndex(u32);

pub enum RecipientDeliveryPath {
    BuiltIn,
    ExternalOverride { rule: HookRuleIndex },
    Disabled,
}

pub struct DaemonConnectionFailureFields {
    pub code: AtmErrorCode,
    pub request_id: RequestId,
    pub classification: ConnectionFailureClassification,
}

pub enum ConnectionFailureClassification {
    ExpectedPeerDisconnect,
    MalformedRequest,
    TransportFailure,
    RequestFailure,
}

pub struct ReleaseVersion(String); // parsed, normalized semantic version

pub struct CompatibilityPreflight {
    pub client_release: ReleaseVersion,
    pub wire_version: u16,
}

pub enum CompatibilityVerdict {
    Compatible { daemon_release: ReleaseVersion },
    Incompatible {
        client_release: ReleaseVersion,
        daemon_release: ReleaseVersion,
        code: AtmErrorCode,
    },
}

pub struct Unverified;
pub struct VersionVerified {
    daemon_release: ReleaseVersion,
}

pub struct Connection<State> {
    transport: LocalIpcTransport,
    _state: PhantomData<State>,
}

impl Connection<Unverified> {
    pub fn verify_compatibility(
        self,
        preflight: CompatibilityPreflight,
    ) -> Result<Connection<VersionVerified>, AtmError>;
}

impl Connection<VersionVerified> {
    pub fn dispatch_write(
        &mut self,
        request: WriteRequestEnvelope,
    ) -> Result<ResponseEnvelope, AtmError>;
}
```

`PostSendHookRuleReport` exposes only recipient matcher, resolved executable
and argv, and declaring config root. It must not contain an `ATM_POST_SEND`
payload or inherited environment. `ExpectedPeerDisconnect` is not an
error-level retained event. `ReleaseVersion` is a validated semantic version
newtype; `HookRuleIndex` prevents an external rule position from escaping as a
raw integer. The compatibility preflight runs after local-IPC connection but
before any write-shaped request dispatch. This ordering is type-enforced:
write-shaped dispatch is implemented only for `Connection<VersionVerified>`;
`Connection<Unverified>` can only perform the compatibility transition. An
incompatible verdict returns `ATM_CLIENT_DAEMON_VERSION_INCOMPATIBLE` with
recovery guidance and no write.

## Hard dependencies and delivery standard

AF-2 starts only after AF1-D5's singleton preflight and cleanup assertions are
available. D1, D2, D3, and D5 may proceed independently on that baseline; D4
is the final integration row and must consume all four completed contracts.
Every row is expected to land production-ready with its implementation,
governing docs/boundary records, error behavior, and validation; none may be
carried forward silently.

## Error inventory

| Failure mode | Stable code / classification | Required recovery |
| --- | --- | --- |
| Client and daemon are incompatible | documented version-compatibility `AtmErrorCode` | Install a compatible CLI/daemon pair; do not write through the incompatible pair. |
| Local connection cap is reached | `ATM_DAEMON_CONNECTION_SATURATED` | Wait for capacity and retry through the same host singleton endpoint; do not start another daemon. |
| Malformed local IPC request | `MalformedRequest` with request ID | Correct the caller/request and inspect the correlated retained record. |
| Local transport failure | existing typed `AtmErrorCode` with `TransportFailure` classification and request ID | Check the singleton endpoint and retained log, then retry only when the error is recoverable. |
| Valid external post-send override | informational doctor path, not an error | Inspect the reported matcher/argv/config root; remove the override only when built-in delivery is intended. |

## Authoritative deliverables

| ID | Deliverable | Primary paths | Acceptance criteria | Required validation |
| --- | --- | --- | --- | --- |
| AF2-D1 | Doctor reports active post-send delivery configuration | `crates/atm-core/src/doctor/mod.rs`, doctor CLI renderers/tests | JSON and text doctor output include an informational `post_send` section. For each configured external rule it shows recipient matcher, resolved executable/argv, and declaring config root; for each known recipient it shows built-in, external-override, or disabled-template selection. Valid overrides do not reduce health or create warnings. The daemon health projection reports liveness and readiness as distinct fields; liveness means the process responds, while readiness means it is serving and not draining. Payloads, environment values, and message content are never shown. | Fixture with one hook verifies JSON and text; fixture with no hook verifies built-in; disabled template verifies disabled state; liveness/readiness fixtures cover serving and draining; secret-like env/payload strings are absent. |
| AF2-D2 | Connection-worker errors, capacity, and deadlines are actionable | `crates/atm-daemon/src/local_ipc_transport/accept_loop.rs`, observability/log snapshot code | The accept loop enforces the existing 64 concurrent-connection cap with a bounded permit/registry; it must not create an unbounded application queue or task set. On saturation it rejects the connection with a typed overload response when a frame can be written, otherwise closes promptly, and retains one classified saturation event with request/connection context. Every admitted connection applies the ICD same-host fixed-header (`1s`), payload (`2s`), and response-write (`3s`) deadlines; an idle or stalled peer is closed and its permit released. Expected peer disconnects are not retained as errors. Every remaining error record includes classified ATM code, safe message, request/correlation ID, and transport context. `atm log snapshot --level error` reads the same `level` schema written to JSONL. | End-to-end send/read/ack against one daemon yields zero error-level records; induced malformed request is returned and retained with code/request ID; 65 held connections prove the 64-connection cap and typed saturation recovery; a stalled fixed-header/payload peer times out, releases capacity, and cannot pin a worker; snapshot finds retained classified records. |
| AF2-D3 | Hermetic doctor unit environment | `crates/atm/src/commands/doctor.rs` tests | Direct-local doctor test clears `ATM_DAEMON_BIN`, socket, home, and bootstrap-affecting inherited variables rather than inheriting caller state. | Test passes with a deliberately set daemon-binary override and proves no daemon report/process is created. |
| AF2-D4 | Installed-artifact smoke preflight | `scripts/smoke/run_thorough_shared_host.py`, release CI/release checklist | Harness uses only released CLI syntax and runs the packaged `atm` and `atm-daemon` selected for release. It refuses to start if AF-1 singleton preflight is absent, records daemon PID/count before and after, and fails on any leaked process. | Fresh OS-user/CI-host run executes all rows; unsupported-flag mutation fails preflight; injected leaked child is detected; AF-1 assertions in the shared script remain intact. |
| AF2-D5 | Version-cutover and configuration guardrails | `docs/adr/ADR-027-client-daemon-version-compatibility.md`, `docs/adr/INDEX.md`, `docs/architecture.md`, `docs/atm-daemon/protocol-icd.md`, `boundaries/atm-daemon-client/rpc-envelope.toml`, daemon/client handshake, `atm doctor`, release documentation | ADR-027 is added to the ADR index and defines compared client/daemon fields, handshake location before writes, fail-closed rationale, error/recovery contract, and composition with AF-1 HostRuntimeScope admission. The ICD and RPC boundary record define `CompatibilityPreflight`/`CompatibilityVerdict` before write-shaped dispatch. `Connection<Unverified>` transitions to `Connection<VersionVerified>` only after a compatible verdict, and only the verified type exposes write dispatch. Unsupported mixed-version clients fail closed with `ATM_CLIENT_DAEMON_VERSION_INCOMPATIBLE` and remediation; doctor labels caller identity/team and daemon-process identity/team as separate contexts, and displays client invocation version and daemon version separately. Default dogfood configuration contains no compatibility hook or wrapper requirement. | A compile-fail typestate test proves unverified connections cannot dispatch writes; old-client/new-daemon incompatibility test causes no database write; matching 1.3.1 pair works; doctor displays both contexts and versions; clean-config built-in nudge test passes on tmux and Windows-compatible fallback paths. |

## Finding-to-deliverable mapping

| Dogfood finding | Closing deliverable |
| --- | --- |
| `SMOKE-FIND-002` (runner retained removed CLI flags) | AF2-D4 |
| `SMOKE-FIND-003` (doctor unit environment leak) | AF2-D3 |
| `SMOKE-FIND-004` (compatibility hooks masked built-in nudge) | AF2-D1 and AF2-D5 |
| `SMOKE-FIND-005` (successful traffic retained generic daemon errors) | AF2-D2 |
| `SMOKE-FIND-006` (obsolete wrapper contract) | AF2-D5 |
| `SMOKE-FIND-007` (doctor hides active post-send overrides) | AF2-D1 |

## Paths to delete or replace

| Retired path | Required replacement / proof |
| --- | --- |
| Generic `connection_worker failed` retained event without original ATM error fields | D2's classified event with code and request ID; expected disconnects are non-error events. |
| Log snapshot's separate `severity` query interpretation | D2's single writer/query level schema. |

## Non-closure

AF-2 does not preserve obsolete `atm_send`, `atm_read`, or `atm_ack` wrapper
scripts. Native CLI commands are the supported interface. It also does not
weaken AF-1 by adding an environment-selectable test daemon or socket.
