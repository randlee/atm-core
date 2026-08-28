# AQ4 cross-host transfer live evidence

Host: `clean-runner-linux`
Commit: `9c9f9918a2870193344459bcec227d2e12d3162b`
Status: **PASS**

## Real `atm send --attach` over a loopback `sshd`

Command: `/home/runner/work/atm-core/atm-core/target/release/atm send aq4-receiver@aq4-transfer-evidence AQ4 live transfer evidence: see attached file --host localhost --attach /tmp/aq4-evidence-ujxjc6a6/attach-source/aq4-report.pdf`
Exit code: `0`

Landed path (from the receiver's real mailbox): `/tmp/atm-1001/send-to/01M13CQZSMD5FGDFK9WEDMDH0X/aq4-report.pdf`
Matches `send_to_staging_dir` convention (`.../send-to/<transfer-id>`): **True**
Landed file exists: **True**
Landed content byte-for-byte matches the source attachment: **True**
