# atm-bd-lite hooks: design

Replaced on 2026-11-01. The earlier drafts here called `bd` directly from 2.5 s hooks, which ANC-063 forbids, and they are superseded.

- **State transitions, errors and diagrams:** `design/lite-code/docs/state-transitions.md`. It ships as the lite skill's docs, one file per hook.
- **Code:** `design/lite-code/` (pyright strict against `typings/atm_bd_types/__init__.pyi`).
- **Sprints:** bo-9 (types and the beads-cache adapter) and bo-10 (all lite hooks). jev-check is a separate proposal (`design/jev-check-proposal.md`).
