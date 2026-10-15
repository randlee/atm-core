# Intent - phase-bh: atm-bd-console

Written 2026-10-14 by fenix, from Rand's statements of 2026-10-14. Jev judges the anchors, not this page.

## What this plan is for

A small CLI, `atm-bd-console`, that replaces the separate bead-queues report and bead-dispatch script with one tick: build the phase snapshot, dispatch ready sanity and QA, escalate problems, and show the dispatch dashboard. Rand: "small cli w/ reliability and clear logic as goals". The rules live in one place and behave the same whether the app or the cron runs them.

Why now: on sc-lint 2026-10-14, 11 of 19 open tasks were on beads outside phase-k and bead-queues showed none of them.

## What success looks like

- One tick function; the app (60 s) and the cron (`--once --headless`, 10-15 min) produce the same dispatches, messages and state.
- The dashboard shows every agent's open tasks and marks the ones outside the phase or without a bead.
- The dispatcher automates the production rules and adds none; GitHub is called only at dispatch.

## What is deliberately out of scope

- Quick-fix QA dispatch (stays with the lead).
- New dispatch rules or gates.
- The stalled-agent marker and other later options.

## Anchors

Anchors live in `anchors.json` (record of truth) and are rendered to `anchors.md`.
