# Developer Roster

The repo-local list of developer agents that sprint plans assign work to. The
shared planning and orchestration skills never name an agent; they point
here. Update this file when the pool changes.

| Agent | Model | Tier | Assign when the sprint is |
|---|---|---|---|
| `cipher` | luna | fast | bounded and mechanical: docs, templates, pass-through edits, test additions, small fix layers, a thin layer sprint against a fixed contract |
| `arch-ctm` | terra | workhorse | standard implementation: a full layer sprint (implementer or consumer), a contract sprint with a settled design, integration wiring |
| `solar` | sol | difficult | hard or open-ended: a contract sprint that still needs design decisions, algorithmic, concurrency or performance work, schema migrations with data risk, defects that resisted a first fix |

Rules:

- Every sprint doc names one agent from this table in `recommended_agent`,
  and its model in `recommended_model`. Choose by the tier the work needs,
  not by who is free. Do not spend `solar` on work `cipher` can close.
- One agent runs one sprint at a time. Two sprints in the same wave that name
  the same agent are serial in practice; the plan's width and critical path
  must count them that way. Spread a wave across the roster, or stack the two
  sprints as one track.
- The lead dispatches to the named agent with
  `atm task assign <agent> --template <template> --vars <json>`. If that
  agent is busy and another of the same or a higher tier is idle, the lead
  may substitute and records the change in the sprint doc.
- `quality-mgr` is the QA coordinator and is not a developer; it is never
  named here.
