# Changelog

## 1.1.3

- complete Phase Q’s SQLite-backed mail migration and daemon-runtime hardening
  across send, ack, read, clear, doctor, and team recovery flows
- retire mailbox-lock correctness dependencies by moving durable truth into the
  SQLite store while preserving compatibility inbox projections and ingest
- add the Phase Q release gate: RULE-003 source splits, release manifests,
  publish wiring, and final packaging validation for `agent-team-mail-core` and
  `agent-team-mail`

## 1.1.1

- preserve Claude inbox files in JSON array format during ATM shared-inbox writes
  so ATM-authored messages inject into live Claude sessions correctly
- keep ATM machine metadata under `metadata.atm` for supported fields while
  leaving alert fields on their current top-level compatibility shape for this
  sprint
- keep forward `metadata.atm.messageId` values as real ULIDs assigned by ATM
  send/ack flows rather than deriving them from legacy UUID compatibility ids
