# Post-mortem: HERMES-SKILL-TESTS run 1, ATM 1.5.6, fixture: local developer host

Purpose of the run: verify the skills work outside the fixture (plan step 2), not to judge 1.5.6.
Oversight agent: fenix. Hermes agent: skillrx. ATM test agent role: fenix by hand (the CLI agent
was on hold while the plan was being written).

## Reports

| skill | agent | tools | result | elapsed |
| --- | --- | --- | --- | --- |
| atm-setup-environment | Hermes agent | cli | FAIL 3/4 (step 4 self round trip = #1298, cause line present) | 188 s |
| atm-setup-environment | oversight, by hand | cli | steps 1–3 PASS; step 4 could not run (see finding 1) | 2 s |
| atm-smoke | Hermes agent | native+cli | FAIL 5/8: read-by-id count=0 (#1298), read-mark cascade, stale connection after 5 s idle (#1297); partner ack in 27 s | 209 s |
| atm-smoke | oversight, by hand | cli | steps 1, 4 (count=1, mutation_applied=true), 6, 7 PASS; 2, 3, 5 unobservable as written (finding 2); 8 PASS at ~3 min (finding 3) | ~9 min incl. waits |

Not run this time: atm-hermes-ready, atm-nudge-roundtrip (need the CLI test agent).

## Findings about the skills (fixed in the same sitting)

1. **Self-addressed sends are invalid** on the CLI (`SelfAddressedSendInvalid`), so every self-send
   step was wrong. All skills now use a partner; `atm-smoke` is symmetric (both sides run it).
2. **CLI listing surfaces are disjoint.** An ack-required message is listed by
   `atm list --pending-ack`, never by `--unread`. Steps that watched `--unread` for an ack-required
   message could not see it and would have reported a false FAIL. Observables now use the
   pending-ack rows' `read` / `pending_ack` fields.
3. **Hermes turn latency is about 3 minutes** from sentence to first tool call. 120 s waits were
   too short; all partner waits are 300 s. This is agent latency, not ATM latency (the ATM ack
   itself arrived in 27 s once the agent acted).
4. **Step 8 observable was the sender's view of pending-ack**, which does not exist; the observable
   is the partner's ack reply arriving.

## Product observations (already tracked)

- #1298: native read by exact id returns count=0 while the row exists with read=0; the same exact-id
  read from the CLI returns count=1 and marks read. Fix in PR #1299 (1.5.7).
- #1297: native client reports `MAY_HAVE_EXECUTED` after a 5 s idle. Fix in PR #1299 (1.5.7).

## Interventions by the oversight agent

- None on the environment. Roster and doctor were already clean (0 errors; 15 `ATM_ROSTER_NO_LEAD`
  warnings on teams outside the expected roster).

## Recommended changes

- Skills: done above (four fixes). Next run uses the CLI test agent for its role, so the by-hand
  column disappears.
- Fixture/testbed: none yet; first container run pending the testbed PR.
- ATM: none beyond #1297/#1298. Worth noting for the CLI reference, not a defect: the disjoint
  `--unread` / `--pending-ack` surfaces surprise every agent the first time.
