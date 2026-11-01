# Sprint bo-14: lite templates, install, release, one live lite phase

This is the old bo-11 sprint, renumbered and unchanged. It depends on bo-10, bo-11, bo-12 and bo-13.

## Acceptance
- [ ] A lite phase runs end to end from release assets with one agent (ANC-015). Steps: dev started, dev acquire/record/close, QA assigned by the close hook, QA FAIL and fix, fix acquire/record/close, QA PASS, and the stack landed.
- [ ] An orchestration phase file with no `profile` key runs unchanged, and orchestration's tests pass unedited (ANC-039).
- [ ] Every hook result of the run appears in the bo-13 audit log.

## Validation
- `just lint`, `just test`.
- The live run's `atm task events` and `bd show` output for every bead.
