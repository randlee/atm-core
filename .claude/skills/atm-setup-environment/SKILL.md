---
name: atm-setup-environment
description: Verify the ATM daemon, roster and doctor are ready before any integration test. Run it first on every fixture (local host or the colima testbed); it makes every expected team member exist in the roster and requires `atm doctor` to pass. Produces the standard ATM test report.
---

# atm-setup-environment

One sentence triggers it: "run the atm-setup-environment skill and send the report to <agent@team>".
The test is identical on every fixture. Only the report's `fixture` line differs.

## Inputs

- `ATM_IDENTITY` / `ATM_TEAM`: your own identity (must already be set).
- Expected roster: the `agent@team` list given in the request. If none is given, the roster is
  whatever `atm teams` already holds and step 2 is a no-op.
- Fixture name: `$ATM_TEST_FIXTURE` if set, otherwise `hostname`.

## Steps

Record PASS or FAIL for every step with the observable that decided it.

1. **Daemon answers.** `atm doctor --json` returns exit 0 within 10 s.
   Observable: `.summary.status`, `.client_context.version`, `.daemon_context.version`.
   FAIL if the command errors, times out, or the two versions differ.
2. **Roster complete.** For every team named in the expected roster, `atm members --team <team>`
   lists every expected member. Add each missing one with
   `atm teams add-member <team> <member> --agent-type <type>` (types: `lead`, `worker`,
   `qa`, `general-purpose`) and re-list. Observable: members added, members still missing.
   FAIL if any expected member is still missing after the add.
3. **Doctor passes.** `atm doctor --json`: `.summary.error_count` must be 0.
   List every warning `code` once with its count. `ATM_ROSTER_NO_LEAD` on teams that are not in the
   expected roster is acceptable; any warning on an expected team is FAIL.
4. **Self round trip.** `atm send <you> --stdin <<'EOF' … EOF` (one line), then
   `atm read --message-id <id> --json` returns that message, then `atm list --unread` no longer
   shows it. Observable: the message id and the three exit codes.

## Report

Send exactly one message to the requester, using the template `../atm-smoke/REPORT.md` (sibling skill directory), skill name `atm-setup-environment`.
