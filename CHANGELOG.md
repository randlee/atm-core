# Changelog

## 1.1.1

- preserve Claude inbox files in JSON array format during ATM shared-inbox writes
  so ATM-authored messages inject into live Claude sessions correctly
- keep ATM machine metadata under `metadata.atm` for supported fields while
  leaving alert fields on their current top-level compatibility shape for this
  sprint
- keep forward `metadata.atm.messageId` values as real ULIDs assigned by ATM
  send/ack flows rather than deriving them from legacy UUID compatibility ids
