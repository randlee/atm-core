# Changelog

## 1.2.0

- complete Phase Z release validation, including fast/normal/thorough smoke,
  `atm-dev` canary and dogfood, final release sign-off, and retained-log
  maintenance adoption through `sc-observability` `v1.1.0`
- validate the same-host `atm-graft` ICD path in thorough smoke and carry the
  final READY release verdict into the authoritative Phase Z readiness records

## 1.1.2

- add the first production-readiness hardening follow-up line for shared-host
  behavior, timeout/retry clarity, and retained-log background maintenance
- land coverage reporting and the smoke execution/reporting skill line used for
  fast, normal, and thorough release validation

## 1.1.1

- preserve Claude inbox files in JSON array format during ATM shared-inbox writes
  so ATM-authored messages inject into live Claude sessions correctly
- keep ATM machine metadata under `metadata.atm` for supported fields while
  leaving alert fields on their current top-level compatibility shape for this
  sprint
- keep forward `metadata.atm.messageId` values as real ULIDs assigned by ATM
  send/ack flows rather than deriving them from legacy UUID compatibility ids
