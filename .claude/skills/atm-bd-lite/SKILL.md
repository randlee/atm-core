---
name: atm-bd-lite
version: 0.19.16
description: Lightweight ATM Beads profile with task hooks and QA, without a sanity stage.
---

# atm-bd-lite

The lite profile provides the minimal ATM Beads workflow: task assignment, task start, and task close
hooks, followed by QA. It does not add a sanity stage. The profile manifest is `profile.toml`; hook
implementations and their behavior tests are kept under this skill.

Install the profile for a phase using a checkout of this repository. Replace `<checkout>` with that checkout,
`<primary>` with the phase repository, `<phase>` with the phase name, and `<owner/repo>` with its provider repository:

```sh
uv run --project <checkout>/plugins/atm-bd-orchestration/core python -m atm_bd_core.install \
  <checkout>/plugins/atm-bd-orchestration/skills/atm-bd-lite/profile.toml <primary> <phase> \
  --checkout <checkout> --repo <owner/repo>
```

The installer registers all three hooks and the phase cron using generated launchers and verifies the registrations.

The profile policy routes QA to `quality-mgr` with `templates/qa.xml.j2`.
