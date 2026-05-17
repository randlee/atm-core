# Phase Y Delivery State Diagrams

These diagrams are the visual companion to
[delivery-state-machines.md](./delivery-state-machines.md).

Source of truth:
- authored Mermaid files live in `docs/phase-Y/*.mmd`
- rendered report lives at [delivery-state-diagrams.html](../reports/delivery-state-diagrams.html)
- rendered panel fragments live under `docs/reports/panels/`

Generation:
- `python3 docs/reports/generate_diagram_pages.py`

Diagrams:
- coordinator:
  [delivery-policy-coordinator.mmd](./delivery-policy-coordinator.mmd)
- Claude new message:
  [new-message-claude.mmd](./new-message-claude.mmd)
- non-Claude new message:
  [new-message-non-claude.mmd](./new-message-non-claude.mmd)
- thread update:
  [thread-update.mmd](./thread-update.mmd)
- ack reply:
  [ack-reply.mmd](./ack-reply.mmd)
- inbox repair:
  [inbox-repair.mmd](./inbox-repair.mmd)
- restore inbox rebuild:
  [restore-inbox-rebuild.mmd](./restore-inbox-rebuild.mmd)

Viewer:
- [delivery-state-diagrams.html](../reports/delivery-state-diagrams.html)
