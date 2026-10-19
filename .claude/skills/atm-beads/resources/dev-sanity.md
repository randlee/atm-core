# Dev Sanity Check

The assignment for a sanity check bead. When and how it runs is in
[`orchestrating.md`](orchestrating.md) ("Dev Sanity Check").

## Recipient

The member that fills the `dev-sanity` role (not a dev or fix agent, which
would make sanity checks wait behind their work). The repository maps the
role to a team-unique member in `roles:` of `.claude/agents/registry.yaml`
(`{{ dev_sanity_member }}` here), and the member's `[startup.<member>]` prompt in
`.atm.toml` names the prompt it runs (`.claude/agents/dev-sanity.md`, which
is the whole role and spawns the `sc-sanity-llm` and `sc-sanity-jev`
subagents).

The plan names no assignee; at dispatch the lead assigns the sanity check bead to that member:
`.claude/skills/atm-beads/scripts/resolve-role dev-sanity`. If the team has no
such member, lead adds one before the plan is imported.

Here the member runs a normal-class model. Its checks run a fast-class model
under Codex, and the `sc-sanity-llm` frontmatter model (sonnet) under Claude. Candidates to replace it later:
Haiku 5.5 once released (Haiku 4.5 is too weak for this), and typesafe.ai
(<https://typesafe.ai/>), to be evaluated; a new check is a new subagent
that `dev-sanity.md` spawns.

## Message

The dev-sanity member sends each check this message with one numbered
deliverable of the checked bead in its fenced JSON payload (`.claude/agents/sc-sanity-llm.md` "Inputs"). The sanity check bead's description
carries the same message, so the instruction can be taken from it:
`bd show <dev-sanity-bead> --json | jq -r '.[0].description'`.


Verify that the work is done:

- each numbered deliverable is written;
- lint passes.

This is not QA.
