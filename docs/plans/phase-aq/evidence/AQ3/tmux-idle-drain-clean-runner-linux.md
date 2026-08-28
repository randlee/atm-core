# AQ3 tmux idle-transition-drain evidence

Host: `clean-runner-linux`
Commit: `35a28588ab4aa319eabbb972ca68931d9a61f05e`
Status: **PASS**

## Steer-kind message: immediate delivery

stdout: `Sent to aq3-idle-receiver@aq3-tmux-idle-drain [message_id: 01M12SPJE4V5XZ041WPCQFEJSV]`

Delivered before any idle transition: **True**

## Queue-kind messages: FIFO idle-transition drain

| Transition | drained_delta | pane contains second item yet? |
| --- | --- | --- |
| 1st Active->Idle | 1 | False |
| 2nd Active->Idle | 1 | n/a |

FIFO order confirmed: **True**

Single drain per transition confirmed (via `queue_messages_drained_total`): **True**
