# AQ2.5 bare-CLI queue delivery-trigger evidence

Host: `clean-runner-linux`
Commit: `bc3c9ee959d990652eff3a4082508e7360d3388b`
Status: **PASS**

## Scenario 1 — two queue-kind messages drain one per Stop

| Pull | stdout | Parsed block |
| --- | --- | --- |
| 1 | `{"decision":"block","reason":"queue-item-one"}` | `{'decision': 'block', 'reason': 'queue-item-one'}` |
| 2 | `{"decision":"block","reason":"queue-item-two"}` | `{'decision': 'block', 'reason': 'queue-item-two'}` |
| 3 | `(empty)` | `None` |

One-per-Stop confirmed: **True**

## Scenario 2 — two steer-kind messages drain together on one Stop

stdout: `{"decision":"block","reason":"steer-item-one\nsteer-item-two"}`

Full-batch drain confirmed: **True**
