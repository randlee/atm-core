# Migration Summary

**Lifecycle**: Temporary migration artifact

This document exists only to summarize the rewrite strategy while the migration
program is active. It should be retired once the permanent docs fully describe
the post-migration system.

`file-migration-plan.md` is the authoritative migration document.

This file is only a summary of the rewrite strategy:

1. keep the `send`, `read`, `ack`, `clear`, `log`, and `doctor` command surface
2. remove daemon dependencies instead of removing core mail behavior
3. move reusable logic into `atm-core`
4. keep retained CLI-only formatting in `atm`
5. preserve the two-axis workflow model and three-bucket read presentation
6. add an early `sc-observability` gap-analysis sprint before ATM depends on shared log query/follow APIs
7. make every file decision explicit in `file-migration-plan.md`
