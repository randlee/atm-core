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

- A sprint doc names one agent from this table in `recommended_agent`, and
  its model in `recommended_model`. Choose by the tier the work needs. Do not
  spend `solar` on work `cipher` can close.
- Plans do not count agents. Staffing is the lead's job at dispatch: one
  agent runs one sprint at a time, and when ready sprints outnumber idle
  agents of the needed tier, the lead starts another agent of that tier and
  adds it to this table. A ready sprint is never queued behind a busy agent
  to fit the current roster.
- The lead dispatches with
  `atm task assign <agent> --template <template> --vars <json>`, and may
  substitute an idle agent of the same or a higher tier for the named one.
- `quality-mgr` is the QA coordinator and is not a developer; it is never
  named here.
