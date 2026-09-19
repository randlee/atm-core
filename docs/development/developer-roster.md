# Developer Roster

The repo-local list of developer agents that sprint plans assign work to. The
shared planning and orchestration skills never name an agent; they point
here. Update this file when the pool changes.

| Tier | Agent (model) | Or | Assign when the sprint is |
|---|---|---|---|
| workhorse (default) | `arch-ctm` (terra) | `rust-developer` background agent on Sonnet or Opus | typical work: a full layer sprint (implementer or consumer), a contract sprint with a settled design, integration wiring |
| fast | `cipher` (luna) | another luna agent | easier work: docs, templates, pass-through edits, test additions, small fix layers, a thin layer sprint against a fixed contract |
| difficult | `solar` (sol) | `rust-developer` background agent on Fable | hard or open-ended work: a contract sprint that still needs design decisions, algorithmic, concurrency or performance work, schema migrations with data risk, defects that resisted a first fix |

Typical work goes to the workhorse tier. Move a sprint down to the fast tier
or up to the difficult tier only when the work calls for it.

Rules:

- A sprint doc names one agent from this table in `recommended_agent`, and
  its model in `recommended_model`. Choose by the tier the work needs. Do not
  spend the difficult tier on work the fast tier can close.
- Prefer the named team member. Use the background-agent equivalent only
  when no named agent of that tier is available. A background developer is
  always the `rust-developer` agent, run on the model its tier names; a
  sprint doc records it as `recommended_agent: rust-developer` with that
  model.
- Fixes are routed by tier too. `cipher` is the default for easy fixes,
  because speed matters most there; typical fixes go to the workhorse and a
  defect that resisted a first fix goes to the difficult tier.
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
