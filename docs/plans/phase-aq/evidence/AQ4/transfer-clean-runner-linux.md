# AQ4 cross-host transfer live evidence

Host: `clean-runner-linux`
Commit: `d0d54e0047b0293e35fd735dc73c3f98924e0dc8`
Status: **PASS**

## Real `atm send --attach` over a loopback `sshd`

Command: `/home/runner/work/atm-core/atm-core/target/release/atm send aq4-receiver@aq4-transfer-evidence AQ4 live transfer evidence: see attached file --host localhost --attach /tmp/aq4-evidence-fhq8qvzs/attach-source/aq4-report.pdf`
Exit code: `0`

Landed path (from the receiver's real mailbox): `/tmp/atm-1001/send-to/01M12V1YCDH90N9C5J4CFHZPDM/aq4-report.pdf`
Matches `send_to_staging_dir` convention (`.../send-to/<transfer-id>`): **True**
Landed file exists: **True**
Landed content byte-for-byte matches the source attachment: **True**
