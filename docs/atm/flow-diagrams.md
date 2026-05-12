# ATM Flow Diagrams

This document indexes the Phase T target flow diagrams for:
- CLI commands
- shared daemon packet/message flows
- `atm-graft` client-interface messages

Design goals captured by these diagrams:
- SQLite is the ATM mailbox SSOT for all ATM-owned read paths
- Claude Code JSONL read/write stays private to the watcher/import/export
  boundary
- normal mailbox queries start from mutable message-status state
- full message content is fetched only for messages that must be rendered
- post-commit notification/nudge behavior runs only after durable SQLite
  commit

Shared command/message diagrams:
- `atm send` and `AtmGraftClient::send`:
  [atm-send-compose.mmd](./diagrams/atm-send-compose.mmd)
- `atm ack` and `AtmGraftClient::ack`:
  [atm-send-ack.mmd](./diagrams/atm-send-ack.mmd)
- `atm list`:
  [atm-list.mmd](./diagrams/atm-list.mmd)
- `atm read` and `AtmGraftClient::read`:
  [atm-read.mmd](./diagrams/atm-read.mmd)
- `atm clear`:
  [atm-clear.mmd](./diagrams/atm-clear.mmd)
- `atm doctor`:
  [atm-doctor.mmd](./diagrams/atm-doctor.mmd)
- `atm log`:
  [atm-log.mmd](./diagrams/atm-log.mmd)
- `atm teams`:
  [atm-teams.mmd](./diagrams/atm-teams.mmd)
- `atm members`:
  [atm-members.mmd](./diagrams/atm-members.mmd)

`atm-graft`-specific daemon packet/message diagrams:
- register consumer session:
  [atm-graft-register.mmd](./diagrams/atm-graft-register.mmd)
- unregister consumer session:
  [atm-graft-unregister.mmd](./diagrams/atm-graft-unregister.mmd)
- fetch advisory nudges:
  [atm-graft-fetch.mmd](./diagrams/atm-graft-fetch.mmd)
- drain advisory nudges:
  [atm-graft-drain.mmd](./diagrams/atm-graft-drain.mmd)

Static HTML viewers:
- CLI interface panels:
  [cli-diagrams.html](./cli-diagrams.html)
- client-interface panels:
  [client-interface-diagrams.html](./client-interface-diagrams.html)

Simplified target model:
- one private Claude compatibility boundary for JSONL ingest/export
- one SQLite-backed mailbox projection rooted in message-status
- one generic post-commit notification/nudge subsystem
- one generic registered-consumer registration/fetch/drain surface
