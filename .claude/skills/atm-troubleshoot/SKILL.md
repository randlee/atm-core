---
name: atm-troubleshoot
description: Root-cause one failed ATM test step on any fixture (local host or colima testbed) from the daemon's own evidence: the mail database, `atm log`, `atm doctor`, a probe message to the affected agent, and the agent's gateway log. Ends in a cause statement with the evidence row that proves it, not a retry.
---

# atm-troubleshoot

One sentence triggers it: "run the atm-troubleshoot skill for <report skill> step <n> on
<agent@team> and send the cause to <agent@team>". Input is one FAIL line from an ATM test
report: skill, step, agent, observable (message id, error code, count).

The fixture is not a black box. Every step below reads state you can show; a finding without its
row or log line is not a finding.

## Steps

1. **Message state.** For the message id in the FAIL line, read the mail database read-only
   (default `~/.atm/db/mail.db`; the fixture README names any other path):
   ```
   sqlite3 -readonly ~/.atm/db/mail.db "select m.team, m.agent, m.from_agent, m.message_at, s.read, s.pending_ack_at, s.acknowledged_at, s.nudge_pending_at, s.nudge_attempts from mail_messages m left join mail_message_states s on s.team=m.team and s.agent=m.agent and s.message_key=m.message_key where m.message_id='<id>'"
   ```
   Observable: the row (never `message_text`, `envelope_json`, chat ids), or "no row".
   No row after a successful send return = admission/write defect; row present but the reader saw
   count=0 = read-path or scope defect; row with `read=0` after a normal read = read-mark handoff
   defect; `nudge_attempts` > 0 with no agent reaction = nudge delivery or agent side.
2. **Daemon log.** `atm log --agent <agent> --since <5 min before the FAIL>` and
   `atm log --type error --since <same>`; keep the lines carrying the message id, the request id or
   the error code. Observable: the matching lines (ids redacted to `<id>`).
3. **Doctor.** `atm doctor --team <team> --json`: `.summary`, the findings with severity other
   than info, and for a Hermes agent its `.graft_receivers.receivers[]` entry (`agent`,
   `last_seen` only; never `endpoint` or `capability`). Observable: missing or stale receiver,
   error findings.
4. **Probe.** `atm send <agent> --requires-ack --stdin` with one line asking for an immediate ack
   reply "probe <unix time>"; wait 60 s; `atm list --unread --json`. Observable: seconds to ack or
   timeout. Timeout with a fresh receiver entry = the agent's side (gateway/tool); timeout with no
   receiver = registration; ack arrives = the original failure was in the test step, not delivery.
5. **Agent side (Hermes only).** Read the agent's gateway log for the same window
   (`~/.hermes/profiles/<agent>/logs/` or the path the fixture README names) and keep the lines
   around the message id; redact every run of 8 or more digits to `<id>`; never copy chat or user
   ids. Observable: the tool call and its error, or the absence of any tool call.
6. **Cause.** One sentence naming the component (send admission, storage write, read path, read-mark
   handoff, receiver registration, nudge delivery, agent tool call, agent runtime) and the evidence
   row or line from steps 1–5 that proves it. If two steps disagree, say which and stop; do not
   guess.

## Report

Exactly one message to the requester in the shape of `../atm-smoke/REPORT.md`, skill name
`atm-troubleshoot`, steps 1–5 each carrying their observable, and the `result:` line replaced by
`cause: <component> — <evidence>`. Fix nothing from this skill; the cause goes to whoever owns the
component.
