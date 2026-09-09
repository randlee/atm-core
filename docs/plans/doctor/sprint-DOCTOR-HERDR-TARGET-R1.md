# Sprint DOCTOR-HERDR-TARGET-R1 — doctor diagnoses unresolvable Herdr nudge targets

Issues: #1360 (launcher labels panes, ATM targets agent names), #1357 (breaker masks the cause).
Branch: `fix/doctor-herdr-target-resolution` → `develop`. Base includes #1353 (doctor uses the ping probe, so per-member presence probes now run).

## Problem

A Herdr-backed roster row nudges `params.target` = the row's alias if set, else its agent name. Herdr 0.8.2 resolves that against agent names (`herdr agent rename`) and pane ids, never pane labels (`herdr pane rename`). On rand-m4 (sc-lint, hmux-launched, atm 1.5.11) every nudge failed and `atm doctor` did not say why. Three shapes were observed:

1. **Label-only pane.** herdr holds an agent with no name whose pane carries the member's label. `agent.get <target>` → `agent_not_found`. Doctor today: generic not-found warning (post-#1353) with no hint that an unnamed agent is sitting right there.
2. **Breaker masking.** After repeated `agent_not_found`, the process breaker opens and every later member probe reports "Herdr process breaker is open" as infrastructure, hiding the real cause (#1357).
3. **Stale session.** Rows registered with `herdrSession = w5` where no named server session exists. Doctor reports the endpoint unreachable but does not name the rows that point at it.

## Deliverables

**D1 — Target-resolution diagnosis (atm-herdr `doctor_probe.rs`, atm-core doctor types).**
When a member probe returns `AgentNotFound`, classify it using the observation's agent list (existing `HerdrOp::List`, fetched at most once per endpoint observation; no new IPC op):
- (a) one or more agents have **no name** → finding (Warning) with a new code in the `ATM_WARNING_HERDR_*` family (register it the way `AgentNotFound`'s code is registered), message naming the target and the count of unnamed agents, remediation `herdr agent rename <pane_id> <target>` listing each unnamed agent's pane id. This needs `pane_id` parsed additively into `AgentSnapshot` (`snapshot_from_value`); absent field stays `None`.
- (b) a **named** agent equals the member's canonical name while the row targets an alias (or the reverse) → finding naming both strings, remediation: either `herdr agent rename <pane_id> <target>` or `atm teams update-member <team> <member> --alias <herdr name>`.
- (c) neither → the existing not-found finding, unchanged.
`presence_findings` keeps roster order; the text renderer prints one line per member: name, effective target, code, remediation. JSON is additive (new optional fields only; note the minor bump in the interface version record per ADR-061).

**D2 — Doctor probes are not masked by the breaker (#1357).**
The doctor presence probe is a bounded, read-only diagnostic. Decide and implement one of: (i) doctor `Get`/`List` calls bypass the spawn breaker, or (ii) when the breaker is open the member row still carries the last underlying error code and detail. Either way `HerdrBreakerDoctorReport` gains optional `last_error_code` / `last_error_detail` (the failure that tripped it), so an operator reading `atm doctor` sees `agent_not_found`, not only "breaker open". Record the choice and the reason in the PR description.

**D3 — Stale session rows are attributed.**
When a named-session observation ends in `ServerNotRunning` / `EndpointUnreachable`, the finding lists the roster members whose `herdrSession` selects that session, with remediation `atm teams update-member <team> <member> --backend herdr` (default server) or the correct `--session`. If this attribution already exists, prove it with a test instead of re-implementing.

**D4 — Tests (fake Herdr IO, no live daemon, no test daemon on this host).**
Unit tests in atm-herdr / atm-core for D1 (a), (b), (c); D2 (breaker open → row still shows the cause, or probe bypasses breaker); D3 (two rows on a missing named session → one finding naming both). The D1(a) test must fail before the change.

**D5 — Docs.**
Extend the doctor/Herdr troubleshooting section that already lists `ATM_WARNING_ROSTER_DRIFT` / `ATM_ROSTER_NO_LEAD` with the new code(s), the label-vs-name explanation in two sentences, and the two remediations. Cross-reference #1360.

## Constraints

No tokio in atm-core. No edits to `.just/lint-config.toml`, `.just/allowlists/`, `boundaries/*.toml`, `.just/lint_boundaries.py`. No unused code, no production switches for test reach. Never patch the legacy synchronous daemon path. Never restart or probe the live `~/.atm` daemon; unit tests use fake IO. Redact ids (8+ digits) and fingerprints (16+ hex) in anything pasted into the PR.

## Acceptance

- `cargo test -p atm-herdr -p atm-core` green with the new tests; `cargo fmt --all --check`; `just lint` clean (pre-existing bootstrap gaps may be noted as such).
- `atm doctor --json` on a host with a label-only pane reports the member with the new code and the pane id in its remediation; after `herdr agent rename <pane> <target>` the same member reports `visible`. The colima fixture (`atm-hermes-testbed`, `bringup.sh` runs `herdr agent rename <pane> tester`) is the place to prove this end-to-end; a unit test with fake IO is the merge gate.
- PR description names the tests, the breaker decision (D2), and the interface minor bump.
- QA report posted on the PR before merge.
