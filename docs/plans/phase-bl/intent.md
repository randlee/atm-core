# Intent - phase-bl

Written 2026-10-21 by Rand's planning session (Claude). Source: docs/plans/phase-bl/phase-bl-plan.md goal.

## What this plan is for

Phase bl delivers atm-bd-orchestration 0.18.5. atm-bd collects data from separate sources (bd, ATM, git, GitHub), saves it as current JSON state, evaluates that state to raise event requests, debounces and aggregates those requests, and delivers ATM messages from a separately scheduled stage, so the phase lead is not spammed. The console and cron share the same collection, and the console gains the Quality tab and DAG changes Rand specified. No database is introduced; design.md carries Rand's verbatim authority.

## What success looks like

- Collection, state evaluation and event delivery are three separate stages over JSON files, and tests drive them with controlled inputs (no timing races).
- The console and cron collect identically without doubling query load, and cron prints the same status whether or not it collected.
- The console Quality tab and DAG show what Rand specified, in colour.

## What is deliberately out of scope

- Any database (SQLite, dolt tables) for atm-bd state.
- Changes to the carried phase-bj console icon, status-table and Jev contracts beyond the updated ANC-009 and ANC-084.

## Anchors

Anchors for this plan live in `anchors.json` (record of truth) and are rendered to `anchors.md`.
