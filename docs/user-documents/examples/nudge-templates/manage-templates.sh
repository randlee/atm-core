#!/usr/bin/env bash
set -euo pipefail

atm teams set-nudge-template \
  --team atm-dev \
  --kind delivery_ack \
  --template-body '<atm from="{{from}}" message-id="{{message_id}}"><action>atm read --message-id {{message_id}}</action><action>ack the message</action><description>{{description}}</description><action>execute the assigned task</action><when idle="immediate" busy="after-current-task"/><console announce="concise" pause="false"/></atm>'

atm teams disable-nudge-template --team atm-dev --kind delivery_ack
atm teams clear-nudge-template --team atm-dev --kind delivery_ack
