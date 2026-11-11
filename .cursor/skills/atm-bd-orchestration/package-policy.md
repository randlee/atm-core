# atm-bd-orchestration package QA policy

The installer overwrites this file on every install; do not edit it. The
repository's own QA policy (`policy_path`, default
`.claude/project/quality-policy.md`) stays the repository's: the installer
never writes it. quality-mgr reads this file first, then `policy_path`. The
repository policy adds or tightens rules; it never weakens one below.

## Mandatory in every repository

1. **Ceremony screen on every QA run.** `ceremony-finding-screen` screens
   every finding of every quality-mgr round, sprint review, fix verification
   and plan review alike, before anything is filed or poured.
2. **Ceremony review of every plan.** `ceremony-qa` reviews every plan in
   plan review round 1 and recommends removals.
3. **No flaky tests.** A test that relies on a race, timing, sleep,
   wall-clock deadline or assertion, poll-until-timeout, event-loop ordering
   or host load, and any wait that can block forever, is a blocking finding
   whatever its severity. Its remedy redesigns the test to be deterministic
   with controlled inputs (an injected clock or fake timers, or an explicit
   handshake). A failure-only hang deadline is allowed only as a backstop
   that fails with captured output. A passing run, even repeated under load,
   is not evidence that no race exists. `flaky-test-qa` runs in every round,
   sprint or fix, whose layer changes test code.

*Why (the maintainer's ruling):* `ceremony-finding-screen` and `ceremony-qa`
each remove more than 30% of a set of findings or of a plan; without them QA
loops spiral and grow a 10k-line project to 50k lines.

The package ships the three agents these rules dispatch:
`ceremony-finding-screen`, `ceremony-qa` and `flaky-test-qa`.
