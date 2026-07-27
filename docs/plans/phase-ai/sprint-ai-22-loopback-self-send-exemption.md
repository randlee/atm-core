---
id: AI.22
title: Loopback and self-IP addressing must not trip the self-send guard
status: proposed
branch: feature/pAI-s22-loopback-self-send-exemption
worktree: ../atm-core-worktrees/feature/pAI-s22-loopback-self-send-exemption
target: integrate/phase-AI
---

# Sprint AI.22 — Loopback and self-IP addressing must not trip the self-send guard

## Goal

- A self-send is rejected only for the caller's exact agent/team destination
  **when the destination has no host**.
- Every host-qualified destination, including
  `<agent>@<team>.localhost`, `<agent>@<team>.127.0.0.1`, and the daemon's
  own advertised IP, bypasses that identity-only guard and continues through
  the ordinary remote-host HTTP route. The required transport proof uses the
  advertised/bound virtual-Ethernet IP, so bytes traverse the same TCP
  interface as a remote peer; `localhost` is grammar coverage only.
- The exemption performs no DNS, local-interface, trust, or socket lookup.
  A host is a routing selector; peer authority and TLS decide later whether a
  host-qualified request is accepted.

## Hard Dependencies

- AI.21-pre
- AI.11–AI.16

## Release candidate

- First commit: set the workspace release for every releasable ATM assembly to
  `1.3.2-beta.22`. CLI and daemon versions must move together; `atm doctor
  --json` is the authoritative runtime check before same-host evidence.

## Exact Targets

- `crates/atm-core/src/address.rs`
- `crates/atm-core/src/send/mod.rs`
- `crates/atm/src/commands/send.rs`
- `crates/atm-core/src/error_codes.rs`

## Deliverables

Every listed deliverable is expected to land at a production-ready level for
the scope this sprint claims. If that cannot be done cleanly in one sprint, the
sprint must be split before implementation begins. No deliverable may be
silently dropped or partially deferred.

- Preserve the existing explicit `host: Option<HostName>` field on
  `AgentAddress`, parsed from the first `.` after the team segment. This
  sprint does not introduce a second host representation: DNS-versus-literal
  IP authority belongs to AI.25. Team-name parsing must reject an embedded `.`
  and hand the remainder to host parsing; it must never silently coerce a
  malformed team segment into `None`.
- Fix the silent-fallback bug in `resolve_recipient` (verified against
  `origin/integrate/phase-AI@cb3af95188c1ba685ed93cec0512e7d38fa7f655`; cite
  the function name rather than a volatile line number): today
  `target_address.team.as_deref().and_then(|team| team.parse().ok()).or_else(|| Some(caller_team.clone()))`
  swallows a team-parse failure and silently substitutes the caller's own
  team instead of returning `AddressParseFailed`. This is the mechanism that
  let `team-lead@atm-dev.192.168.128.82` silently resolve to
  `team-lead@atm-dev` (self) instead of failing to parse or resolving a host.
  A malformed or unrecognized team/host segment must be a typed parse error,
  never a silent identity substitution.
- Extend `validate_non_self_recipient` (verified against
  `origin/integrate/phase-AI@cb3af95188c1ba685ed93cec0512e7d38fa7f655`; cite
  the function name rather than a volatile line number) to reject only an exact same
  agent/team recipient whose `host` is `None`. The function receives the
  already parsed host; it must not import DNS, peer trust, interface, or
  transport code. Any present host, including an unrecognized remote host,
  continues to the one post-write route and receives that route's typed
  authority/transport result.
- Preserve the parsed `AgentAddress`—including `host`—unchanged in the
  canonical `WriteRequest` and CLI request encoding. `ResolvedRecipient`
  remains the roster identity projection and must not become a second host
  carrier. `crates/atm/src/commands/send.rs` must use this shared address
  grammar only; this sprint adds no `--host` flag or alternate input form.

## Required Work

- Update `docs/atm/commands/send.md`, `docs/requirements.md`, and
  `docs/atm-core/requirements.md` to document the inline host grammar and the
  identity-only self-send guard.

## Explicit Code Samples

```rust
// crates/atm-core/src/address.rs
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentAddress {
    pub agent: AgentName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<ChatId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<TeamName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<HostName>,
}

// crates/atm-core/src/send/mod.rs
pub(crate) fn validate_non_self_recipient(
    sender: &AgentName,
    sender_team: &TeamName,
    recipient: &ResolvedRecipient,
    target: &AgentAddress,
) -> Result<(), AtmError> {
    let is_same_identity = sender.as_str().eq_ignore_ascii_case(recipient.agent.as_str())
        && sender_team.as_str().eq_ignore_ascii_case(recipient.team.as_str());
    if !is_same_identity {
        return Ok(());
    }
    if is_same_identity && target.host.is_none() {
        return Err(AtmError::self_addressed_send_invalid(format!(
            "self-addressed messages are invalid ATM input: '{sender}@{sender_team}' may not send to itself"
        )));
    }
    Ok(())
}
```

## This Sprint Does Not Close

- This sprint does not implement peer authority, TLS, DNS resolution, or the
  post-write router. A host-qualified same-identity send may fail downstream
  with the ordinary typed authority/transport error; that is never rewritten
  as a self-send error.
- This sprint does not change `REQ-CORE-TRANSPORT-002`'s cross-host
  authorization, mTLS, or peer-trust behavior (`AI.25`, `REQ-CORE-TRANSPORT-002B/D`).
- This sprint does not add reverse-DNS inference or persist resolved hosts —
  the `Host` value is carried through the request only.

## Acceptance Criteria

- Exact same agent/team with no host returns `SelfAddressedSendInvalid` before
  persistence. Exact same agent/team with any syntactically valid host does
  not return that error.
- `<agent>@<team>.<advertised-ip>` uses the daemon's virtual-Ethernet TCP
  interface and encodes the same canonical `WriteRequest` as a remote host's
  `atm send`/`atm ack`. The receiver enters the same HTTP write resource,
  `ApiRouter::route`, dispatcher, persistence method, and `PostWriteRouter`.
- `localhost` and `127.0.0.1` are parsing/host-preservation regressions only;
  they may not be used to satisfy the virtual-Ethernet same-host proof.
- The ordinary local CLI `atm send` and `atm ack`, own-IP HTTPS receipt, and
  remote-host HTTPS receipt converge before the canonical write handler. Tests
  fail if own-IP uses a direct local-mailbox shortcut or a separate
  write/nudge path.
- A syntactically valid but untrusted host reaches downstream peer authority
  validation and returns its typed authority error, not self-send rejection.
- A malformed team or host segment returns a typed parse error; it never
  silently substitutes the caller's own team.
- Existing non-host-qualified self-send behavior (`<agent>@<team>` with no
  host, sending to itself) is unchanged and still rejected.
- `agent:<chat-id>@team.<host>` parses and renders with the same `chat_id` and
  `host`; both values pass unchanged through `validate_non_self_recipient` and
  `resolve_recipient` into the canonical request.
- Release-built CLI and daemon both report `1.3.2-beta.22` through
  `atm doctor --json` before the proof runs.
- An independent quality review runs the release-built branch daemon through
  the real CLI and advertised-IP TCP listener. Its evidence names the router,
  dispatcher, persistence, and post-write log events observed; a mock router
  or direct-dispatch test cannot close this sprint.

## Required Validation

- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `cargo build --release --bin atm --bin atm-daemon`
- `git diff --check`
- Switch the CLI and daemon together with `daemon-switch`, restart exactly one
  managed branch daemon, verify `atm doctor --json`, then execute the
  advertised-IP same-host send row and retain its sanitized log. Leave that
  branch daemon running for quality review.
