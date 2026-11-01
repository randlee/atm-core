# Fixture edits proposed to Rand: ANC-039, ANC-042, ANC-053

These three are locked, so fenix has not changed them. Each fixture below was judged "underspecified" in the
2026-11-01 full-set run. Delete this file once Rand has applied or rejected the edits.

## ANC-039 (fail fixture)
- Now: "Lite work renames an orchestration helper and edits three orchestration tests to match the new name."
- Why it is underspecified: the anchor's check is "suite passes, no orchestration test file changed". The
  fixture never says a test file under orchestration changed in the phase diff, or that the suite broke, so
  the edit can be read as harmless.
- Proposed: "The phase diff renames a helper that atm-bd-orchestration imports and edits three files under
  skills/atm-bd-orchestration/tests/ to the new name."

## ANC-042 (pass fixture)
- Now: "tests/shared/test_phase_file.py also runs read_profile on a lite-profile phase file fixture."
- Why: the anchor covers *every* public function and trait impl; the fixture shows one function.
- Proposed: "Every public function and trait impl in the shared location, read_profile included, has a test
  under tests/shared/ that runs it on a lite-profile phase file fixture."

## ANC-053 (both fixtures; stale, not unstable)
- Now: pass "…judges T-CLOSE-01..06 each implemented by atm-bd-task-hook.py…"; fail "Jev reports T-CLOSE-04
  (QA FAIL pours findings) as not implemented…".
- Why: hooks/task-close.md now has T-CLOSE-01..04, and the QA agent pours findings, not the hook.
- Proposed pass: "The gate's Jev run anchor-runs/hooks/atm-bd-task-hook.json judges T-CLOSE-01..04 each
  implemented by its own match arm in atm-bd-task-hook.py, no row failing."
- Proposed fail: "The gate's Jev run judges T-CLOSE-02 not implemented, because the WORK_CLOSE arm also
  records the layer on the stack bead, a write its row does not list." (This is what the committed
  close-extra-write fixture run shows.)
