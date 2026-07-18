# AG-VAL-022 Windows↔macOS cross-host smoke — live results (2026-07-17)

Validated against the **current baseline** `feature/pAG-s15-othermac-smoke` @ `328993e1`
(the metadata `remoteHost` ack-routing mechanism), not any external branch. Recorded so the
still-`PENDING` AG-VAL-022 rows have real host-pair evidence.

## Topology
- macOS host `team-lead` @ `192.168.1.178` (route-reachable iface) ↔ Windows host `dev-win`
  (`2023-001`) @ `192.168.1.146`, TCP `43101`.
- Transport: `secure-required`, mutual pinned-SHA-256-fingerprint TLS (ADR-032).
- Control plane: `atm daemon interfaces add` / `hosts allow <ip>` / `security trust approve <ip>`.
- Env note: disposable `ATM_HOME`/`ATM_CONFIG_HOME`/`ATM_LOG_DIR` were used, but the daemon
  runtime scope derives from the OS home, so evidence is labeled **host-env**, not strict clean-room.

## Results

| Row | Intent | Result |
|-----|--------|--------|
| AG-VAL-022A | unauthorized-host rejection before mailbox mutation | **PASS** — `hosts deny` then send → exit 3 `host … is disabled`; no receiver mutation |
| AG-VAL-022B | authorized durable send (both directions) | **PASS** — `outcome=sent` |
| AG-VAL-022C | receiver read after send | **PASS** — `mutation_applied=true` |
| AG-VAL-022D | cross-host ack round-trip to origin host | **PASS** — reply routes to origin host; `acknowledgesMessageId` matches the source |
| AG-VAL-022 (reverse) | Windows→macOS send + read | **PASS** |
| AG-VAL-022E | degraded-notification classification | **covered-by-design** — durable send returns `sent` independent of any nudge; not force-able in a headless CLI |
| AG-VAL-022F | retry-visible interruption/recovery | interruption → visible failure (see AG-FIND-006 error-quality); recovery send after daemon restart → `sent` **PASS** |

Additionally, live **atm-dev promotion**: Windows `dev-win` was added to the real `atm-dev`
roster and completed a `--requires-ack` handshake in both directions over secure transport.

## Findings surfaced (see `../cross-host-findings-ledger.md`)
- **AG-FIND-006** (open, non-blocking): a cross-host send to a stopped/unreachable peer surfaces
  as a local `failed to read daemon response frame (Resource temporarily unavailable, os error 35)`
  with recovery text pointing at the local daemon/socket — misleading for a remote-peer-down case.
- **AG-FIND-007** (fixed on `feature/pAG-crosshost-hardening`): `.` was permitted in agent/team
  names but is the reserved `<agent>@<team>.<host>` delimiter, so a legal team like `dev.qa` was
  unusable cross-host (`cannot combine inline remote host syntax with --host`).
