---
title: Nudge Templates
audience: end-user
reviewed_for_release: 1.4.3
---

# Nudge Templates

ATM supports built-in nudge behavior and bounded operator override surfaces.

## Purpose

Nudges provide small operator-facing notifications when ATM workflows require
attention.

## Scope

This document covers supported template usage and override behavior. It does
not authorize direct database edits or unsupported template engines.

## Six Built-In Template Kinds

ATM ships exactly six built-in template kinds:

- `delivery`
- `delivery_ack`
- `delivery_task`
- `delivery_task_ack`
- `acknowledge`
- `acknowledge_task`

The `delivery*` forms are for delivered work notifications. The
`acknowledge*` forms are intentionally compact acknowledgement nudges.

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
atm teams set-nudge-template --team atm-dev --kind delivery_ack --template-body '<atm from="{{from}}" message-id="{{message_id}}"><action>read atm --team {{team}}</action><action>ack the message</action><description>{{description}}</description><action>execute the assigned task</action><when idle="immediate" busy="after-current-task"/><console announce="concise" pause="false"/></atm>'
atm teams disable-nudge-template --team atm-dev --kind delivery_ack
atm teams clear-nudge-template --team atm-dev --kind delivery_ack
```

## Default XML Bodies

Delivery without required acknowledgement:

```xml
<atm from="{{from}}" message-id="{{message_id}}">
  <action>read atm --team {{team}}</action>
  <description>{{description}}</description>
  <action>execute the assigned task</action>
  <when idle="immediate" busy="after-current-task"/>
  <console announce="concise" pause="false"/>
</atm>
```

Delivery with required acknowledgement:

```xml
<atm from="{{from}}" message-id="{{message_id}}">
  <action>read atm --team {{team}}</action>
  <action>ack the message</action>
  <description>{{description}}</description>
  <action>execute the assigned task</action>
  <when idle="immediate" busy="after-current-task"/>
  <console announce="concise" pause="false"/>
</atm>
```

Task delivery without required acknowledgement:

```xml
<atm from="{{from}}" message-id="{{message_id}}">
  <action>read atm --team {{team}}</action>
  <task id="{{task_id}}">{{description}}</task>
  <action>execute the assigned task</action>
  <when idle="immediate" busy="after-current-task"/>
  <console announce="concise" pause="false"/>
</atm>
```

Task delivery with required acknowledgement:

```xml
<atm from="{{from}}" message-id="{{message_id}}">
  <action>read atm --team {{team}}</action>
  <action>ack the message</action>
  <task id="{{task_id}}">{{description}}</task>
  <action>execute the assigned task</action>
  <when idle="immediate" busy="after-current-task"/>
  <console announce="concise" pause="false"/>
</atm>
```

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
