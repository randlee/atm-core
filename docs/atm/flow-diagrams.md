# ATM Flow Diagrams

This document indexes the Phase T target flow diagrams for:
- CLI commands
- shared daemon packet/message flows
- `atm-graft` client-interface messages

Design goals captured by these diagrams:
- SQLite is the ATM mailbox SSOT for all ATM-owned read paths
- historical Claude Code JSONL read/write stayed private to the
  watcher/import/export boundary on the pre-`ADR-019` line
- normal mailbox queries start from mutable message-status state
- full message content is fetched only for messages that must be rendered
- post-commit notification/nudge behavior runs only after durable SQLite
  commit

Shared command/message diagrams:
- `atm send` and `AtmGraftClient::send`:
  [atm-send-compose.mmd](./atm-send-compose.mmd)
- `atm ack` and `AtmGraftClient::ack`:
  [atm-send-ack.mmd](./atm-send-ack.mmd)
- `atm list`:
  [atm-list.mmd](./atm-list.mmd)
- `atm read` and `AtmGraftClient::read`:
  [atm-read.mmd](./atm-read.mmd)
- `atm clear`:
  [atm-clear.mmd](./atm-clear.mmd)
- `atm doctor`:
  [atm-doctor.mmd](./atm-doctor.mmd)
- `atm log`:
  [atm-log.mmd](./atm-log.mmd)
- `atm teams`:
  [atm-teams.mmd](./atm-teams.mmd)
- `atm members`:
  [atm-members.mmd](./atm-members.mmd)

`atm-graft`-specific daemon packet/message diagrams:
- register consumer session:
  [atm-graft-register.mmd](./atm-graft-register.mmd)
- unregister consumer session:
  [atm-graft-unregister.mmd](./atm-graft-unregister.mmd)
- fetch advisory nudges:
  [atm-graft-fetch.mmd](./atm-graft-fetch.mmd)
- drain advisory nudges:
  [atm-graft-drain.mmd](./atm-graft-drain.mmd)

Static HTML viewers:
- CLI interface panels:
  [cli-diagrams.html](../reports/cli-diagrams.html)
- client-interface panels:
  [client-interface-diagrams.html](../reports/client-interface-diagrams.html)

Simplified target model:
- historical Claude JSONL ingest/export remains documentation-only and is not
  part of the accepted runtime after `ADR-019`
- one SQLite-backed mailbox projection rooted in message-status
- one generic post-commit notification/nudge subsystem
- one generic registered-consumer registration/fetch/drain surface
