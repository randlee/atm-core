---
version: 0.6.1
---

# Development-review workflow

### D. Development gate (sprint review)

Read `references/development-gate.md` and `references/triage-and-iteration.md`. At sprint delivery, the quality manager delegates to `anchor-sprint-review` with the sprint's epic bead ID and the evidence (sprint report, PR number or body, test summary). It runs the development question set, whose evidence question separates "we followed it" from "here is where", and returns the deterministic checks to pair with each `satisfied` or `asserted_only` anchor. Run and record those checks; Jev judges text, and only a diff or git history proves the code. Then the loop of C: triage each failing verdict (work defect, report defect, anchor defect, question defect, Jev error), fix where the defect is, re-run, diff. Question design: `references/code-review-questions.md`; dev-gate wording is tuned in the dev-questions bead.

### E. Report

Reports quote the user's own words back (`references/report-format.md`). Post the markdown from `report --run <run> --format md` as a comment on the judged bead (`bd comment <id> --file <report.md>`) or on the PR. Posting happens in the main session; agents never write to beads.

<!-- The workflow text above is preserved verbatim from the 0.4.3 entrypoint. -->
