# Windows↔macOS cross-host smoke — informational pre-work (2026-07-17)

> **Status: INFORMATIONAL / OUT-OF-SEQUENCE PRE-WORK. This does NOT close any AG-VAL row
> and does NOT satisfy AG.16's entry gate.**
>
> AG-VAL-022 (Windows↔macOS) is owned by **AG.16**, which cannot open until AG.15 lands
> (`readiness.md:69`), and AG.15 lists Windows/macOS heterogeneous-host closure as
> **Out-Of-Scope** (`sprint-AG15.md:84`). The frozen matrix in
> `../cross-host-smoke-checklist.md` keeps AG-VAL-022A–F at `PENDING`, and this record does
> **not** modify it. These observations are recorded only as early evidence that the
> baseline cross-host mechanism behaves as designed on a real Windows↔macOS pair; formal
> closure must happen inside AG.16 with `quality-mgr` sign-off once the sprint sequence
> reaches it.

Observed against the pinned baseline commit `feature/pAG-s15-othermac-smoke` @ `328993e1`
(the metadata `remoteHost` ack-routing mechanism), not any external branch.

## Topology
- macOS host `team-lead` @ `192.168.1.178` (route-reachable iface) ↔ Windows host `dev-win`
  (`2023-001`) @ `192.168.1.146`, TCP `43101`.
- Transport: `secure-required`, mutual pinned-SHA-256-fingerprint TLS (ADR-032).
- Control plane: `atm daemon interfaces add` / `hosts allow <ip>` / `security trust approve <ip>`.
- Env note: disposable `ATM_HOME`/`ATM_CONFIG_HOME`/`ATM_LOG_DIR` were used, but the daemon
  runtime scope derives from the OS home, so evidence is labeled **host-env**, not strict clean-room.

## Observations (NOT closure verdicts)

| AG-VAL-022 row (owned by AG.16) | Intent | Observed |
|-----|--------|----------|
| 022A | unauthorized-host rejection before mailbox mutation | observed OK — `hosts deny` then send → exit 3 `host … is disabled`; no receiver mutation |
| 022B | authorized durable send (both directions) | observed OK — `outcome=sent` |
| 022C | receiver read after send | observed OK — `mutation_applied=true` |
| 022D | cross-host ack round-trip to origin host | observed OK — reply routes to origin host; `acknowledgesMessageId` matches the source |
| 022 (reverse) | Windows→macOS send + read | observed OK |
| 022E | degraded-notification classification | not independently forced — durable send returns `sent` independent of any nudge (no notification channel in a headless CLI) |
| 022F | retry-visible interruption/recovery | interruption → visible failure (see finding below); recovery send after daemon restart → `outcome=sent` |

Additionally, a live **atm-dev** cross-host membership check: Windows `dev-win` was added to
the `atm-dev` roster and completed a `--requires-ack` handshake in both directions over
secure transport. (Also informational — not an AG-VAL closure.)

## Findings surfaced (tracked formally in `../cross-host-findings-ledger.md` via the companion #572 branch)
- **Unreachable-peer error classification** (open, non-blocking): a cross-host send to a
  stopped/unreachable peer surfaces as a local `failed to read daemon response frame
  (Resource temporarily unavailable, os error 35)` with recovery text pointing at the local
  daemon/socket — misleading for a remote-peer-down case.
- **Dotted-target parse clarity**: `.` is allowed in local team/agent names (REQ-SEC-001) but
  is the reserved `<agent>@<team>.<host>` delimiter, so a dotted team like `dev.qa` is not a
  valid cross-host remote target. The companion fix scopes a clear typed parse-time rejection
  to cross-host remote-target parsing per ADR-031 (it does not change local name validity).
