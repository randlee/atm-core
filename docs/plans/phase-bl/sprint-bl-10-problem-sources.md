# Sprint bl-10 — problem collectors and state derivation

`BOUNDARY-ProblemSources`; wave 1; consumes bl-1 collector records. GitHub,
BD, and atm-bd source modules collect independently; state derivation produces
typed actionable problems from current records without triggering reads or
messages. Retain latest non-cancelled check selection, branch fallback, sprint
labels, and `--exclude-label anchor`; fixtures prove independent failure/rate
state and exact derived fields.
