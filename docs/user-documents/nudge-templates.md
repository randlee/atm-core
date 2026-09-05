---
title: Nudge Templates
audience: end-user
reviewed_for_release: 1.4.4
---

# Nudge Templates

ATM supports built-in nudge behavior and bounded operator override surfaces.

## Purpose

Nudges provide small operator-facing notifications when ATM workflows require
attention.

## Scope

This document covers supported template usage and override behavior. It does
not authorize direct database edits or unsupported template engines.

## Seven Built-In Template Kinds

ATM ships exactly seven built-in template kinds:

- `delivery`
- `delivery_ack`
- `queue`
- `queue_ack`
- `task`
- `acknowledge`
- `acknowledge_task`

`NudgeKind` selects the delivery (`delivery`, `delivery_ack`) or queue
(`queue`, `queue_ack`) family. Task-tagged messages always use `task` and are
always queued. The `acknowledge*` forms are intentionally compact
acknowledgement nudges.

## Supported Placeholders

Built-in template rendering supports exactly these placeholders:

- `{{from}}`
- `{{team}}`
- `{{message_id}}`
- `{{description}}`
- `{{task_id}}`

There is no Jinja evaluation, no conditionals, and no template-side branching.
ATM performs direct placeholder substitution only.

## Precedence

Built-in nudge selection uses this order:

1. matching external `[[atm.post_send_hooks]]` command
2. team-scoped built-in template override for the selected kind
3. product default template body for that kind

## Override Lifecycle

Override lifecycle is explicit:

- no stored row means product default
- override row means use the stored non-empty template body
- disabled row means emit no built-in nudge for that template kind
- clear/reset deletes the row and restores the product default

Empty-string template bodies are invalid. Use the explicit team-admin commands
instead:

```bash
atm teams set-nudge-template --team atm-dev --kind delivery_ack --template-body '<atm from="{{from}}" message-id="{{message_id}}"><action>atm read --message-id {{message_id}}</action><action>ack the message</action><description>{{description}}</description><action>execute the assigned task</action><when idle="immediate" busy="after-current-task"/><console announce="concise" pause="false"/></atm>'
atm teams disable-nudge-template --team atm-dev --kind delivery_ack
atm teams clear-nudge-template --team atm-dev --kind delivery_ack
```

## Default XML Bodies

Delivery without required acknowledgement:

```xml
<atm from="{{from}}" message-id="{{message_id}}">
  <action>atm read --message-id {{message_id}}</action>
  <description>{{description}}</description>
  <action>execute the assigned task</action>
  <when idle="immediate" busy="after-current-task"/>
  <console announce="concise" pause="false"/>
</atm>
```

Delivery with required acknowledgement:

```xml
<atm from="{{from}}" message-id="{{message_id}}">
  <action>atm read --message-id {{message_id}}</action>
  <action>ack the message</action>
  <description>{{description}}</description>
  <action>execute the assigned task</action>
  <when idle="immediate" busy="after-current-task"/>
  <console announce="concise" pause="false"/>
</atm>
```

Queue without required acknowledgement:

```xml
<atm from="{{from}}" message-id="{{message_id}}">
  <action>atm read --message-id {{message_id}}</action>
  <description>{{description}}</description>
  <action>execute the assigned task</action>
  <console announce="concise" pause="false"/>
</atm>
```

Queue with required acknowledgement:

```xml
<atm from="{{from}}" message-id="{{message_id}}">
  <action>atm read --message-id {{message_id}}</action>
  <action>ack the message</action>
  <description>{{description}}</description>
  <action>execute the assigned task</action>
  <console announce="concise" pause="false"/>
</atm>
```

Task messages are always queued and require acknowledgement:

```xml
<atm from="{{from}}" message-id="{{message_id}}">
  <action>atm read --message-id {{message_id}}</action>
  <action>ack the message</action>
  <task id="{{task_id}}">{{description}}</task>
  <action>execute the assigned task</action>
  <console announce="concise" pause="false"/>
</atm>
```

Two former task steer kinds were retired in phase AX and are rejected on
input; see the ADR-019 amendment for their names and the migration.

On database open, ATM upgrades the override table to the seven-kind constraint,
preserves every supported row, and removes only retired rows. The migration is
idempotent and accepts new `queue`, `queue_ack`, and `task` overrides after the
upgrade.

Compact acknowledgement defaults:

```xml
<atm kind="ack" from="{{from}}" message-id="{{message_id}}"/>
```

```xml
<atm kind="ack" from="{{from}}" message-id="{{message_id}}" task-id="{{task_id}}"/>
```

Example files live in [examples/nudge-templates/](./examples/nudge-templates/).

## Related Topics

- hook integration: [Hooks](./hooks.md)
- mailbox workflows that produce or consume notifications: [Mailbox Workflows](./mailbox-workflows.md)
- recovery guidance: [Troubleshooting](./troubleshooting.md)

Return to the [ATM User Guide](./README.md).
