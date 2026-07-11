---
title: Phase AD Follow-Up ATM Messaging Fixes
status: complete
branch: plan/phase-ad-followup-atm-messaging-fixes
worktree: /Users/randlee/Documents/github/atm-core-worktrees/plan/phase-ad-followup-atm-messaging-fixes
target: develop
---

# Phase AD Follow-Up ATM Messaging Fixes

## Goal

Author the Phase `AD` follow-up sprint line for the three release-blocking ATM
messaging defects captured in GitHub issues:

- `#498` ATM allows self-addressed messages (`from == to`) to be created
- `#499` `atm ack` on a self-addressed message creates an unbreakable ack
  reply loop
- `#500` `atm read` still manufactures ack obligations on display instead of
  respecting sender-owned durable intent

This follow-up stays inside Phase `AD` because it tightens the same caller
ownership, send/read/ack contract, and dogfood messaging model that the rest
of the Phase `AD` correction line already owns.

## Why This Is Still Phase AD

The new defects do not open a new product direction. They are still part of
the same release-blocking correction line because they all violate the Phase
`AD` target model:

- caller identity must be explicit and owner-controlled
- send owns message creation
- read must not fabricate new workflow obligations
- ack must terminate required acknowledgement state instead of expanding it

Creating a separate phase would hide that these are still mandatory fixes to
the same accepted ATM behavior model.

## Root-Cause Summary

`#500` is the structural defect. The current accepted line still lets the read
path create ack-required state on display, which means ack obligation is not a
sender-owned durable property of the message. `#498` and `#499` are poison
paths exposed by that looser model:

- self-addressed sends should never be created
- even if a historical self-addressed message exists, acking it must converge
  instead of producing a new self-addressed pending-ack message

## Recommended Sprint Line

This follow-up is split into five sprints so each closure type stays
production-ready and mechanically reviewable on its own:

1. `AD.31` command-surface and ownership reset
2. `AD.32` durable ack-intent persistence and read-semantics reset
3. `AD.33` self-addressed send rejection
4. `AD.34` self-ack loop termination and historical poison handling
5. `AD.35` operator protocol, docs, and end-to-end regression closeout

The split is deliberate:

- `AD.31` resets the mutating-vs-inspection API boundary
- `AD.32` fixes the structural message-state model
- `AD.33` and `AD.34` close the two remaining poison paths independently
- `AD.35` is the only sprint allowed to call the new operator/documentation
  surface complete after the regression matrix is green

## Sprint Map

| Sprint | Closure | Primary issues |
| --- | --- | --- |
| `AD.31` | replace `atm read --no-mark` with an explicit non-mutating `atm peek` command and remove impersonation from all mutating mailbox/message commands | `#500` |
| `AD.32` | make `requires_ack` a durable persisted message field and delete read-time ack creation | `#500` |
| `AD.33` | reject self-addressed sends in the shared send path before persistence | `#498` |
| `AD.34` | make `atm ack` terminate historical self-addressed poison messages without emitting a new reply | `#499` |
| `AD.35` | update team protocol, command docs, help text, and land one authoritative messaging regression matrix | `#498`, `#499`, `#500` |

## New Contract Direction

The sprint line below is built around these contract decisions:

- mutating commands act only as the resolved caller identity
- mutating commands fail closed if caller identity or team is unresolved
- no mutating command may impersonate another member, even when
  `ATM_IDENTITY` is absent
- inspection-only commands may inspect another member's queue state
- `atm read` remains the owner-only mutating command
- `atm peek` becomes the explicit non-mutating inspection command
- sender intent to require acknowledgement is durable message data, not a
  display-time side effect
- ack replies are explicitly non-ack-requiring unless a future approved
  requirement says otherwise

## Deliverables

- `docs/plans/phase-AD/sprint-AD31.md` through
  `docs/plans/phase-AD/sprint-AD35.md`
- `docs/plans/phase-AD/plan-phase-AD.md` updated to include the new follow-up
  line
- `docs/plans/phase-AD/readiness.md` updated so the authoritative readiness
  artifact reflects the added `AD.31` through `AD.35` closure gate and
  `AD.35`'s sole-authorship role for the final Phase `AD` verdict
- `docs/project-plan.md` updated to list the new follow-up sprint line inside
  Phase `AD`

## Acceptance

- every issue in `#498`, `#499`, and `#500` has one clear sprint owner
- the `#498` / `#499` dependency relationship is called out explicitly:
  `AD.33` is the root-cause send-side closure and `AD.34` is the ack-side
  defense-in-depth plus historical poison cleanup
- each sprint doc has:
  - complete frontmatter
  - explicit exact targets
  - explicit interface/code samples where implementation choices would
    otherwise drift
  - one authoritative deliverables list
  - one authoritative acceptance-criteria list
  - one authoritative required-validation list
- the new plan is ready for plan-hardening and quality review without needing
  more scoping work
