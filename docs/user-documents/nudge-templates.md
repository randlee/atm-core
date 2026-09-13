---
title: Nudge Templates
audience: end-user
reviewed_for_release: 1.4.4
---

# Nudge Templates

ATM supports built-in nudge behavior and bounded operator override surfaces.

Repeated open-task reminders also produce daemon escalation notifications.
At reminders 10, 20, and later multiples of 10, the unique roster lead is
notified. A blocked member is escalated after 60 seconds and re-notified every
10 minutes while the blocked episode continues. These daemon messages are
system-generated and are not controlled by the eleven built-in template kinds.

## Purpose

Nudges provide small operator-facing notifications when ATM workflows require
attention.

## Scope

This document covers supported template usage and override behavior. It does
not authorize direct database edits or unsupported template engines.

## Eleven Built-In Template Kinds

ATM ships exactly eleven built-in template kinds:

- `delivery`
- `delivery_ack`
- `queue`
- `queue_ack`
- `acknowledge`
- `task_queued`
- `task_ready`
- `task_reminder`
- `task_started`
- `task_complete`
- `task_closed`

`NudgeKind` selects the delivery (`delivery`, `delivery_ack`) or queue
(`queue`, `queue_ack`) family. Task transitions select one of the six task
kinds; their durable state and event audit are defined in ADR-062. The
`acknowledge` form is an intentionally compact acknowledgement nudge.

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

Task-ready messages are emitted by the task pass and never require
acknowledgement:

```xml
<atm task="{{task_id}}" ready message="{{message_id}}" from="{{from}}">
  <action>atm read --message-id {{message_id}}</action>
  <action>atm task start {{task_id}}</action>
  <action>execute the assigned task</action>
  <console announce="concise" pause="false"/>
</atm>
```

The former `task` and `acknowledge_task` kinds are retired and rejected on
input; task transitions use the six named kinds above.

On database open, ATM upgrades the override table to the eleven-kind constraint,
preserves every supported row, and removes only retired rows. The migration is
idempotent and accepts new task-transition overrides after the upgrade.

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
