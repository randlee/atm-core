# AQ4 cross-host transfer live evidence

Host: `clean-runner-macos`
Commit: `d0d54e0047b0293e35fd735dc73c3f98924e0dc8`
Status: **PASS**

## Real `atm send --attach` over a loopback `sshd`

Command: `/Users/runner/work/atm-core/atm-core/target/release/atm send aq4-receiver@aq4-transfer-evidence AQ4 live transfer evidence: see attached file --host localhost --attach /var/folders/df/djsxfhc17x95674wsm_g8s980000gn/T/aq4-evidence-hhxbm8tn/attach-source/aq4-report.pdf`
Exit code: `0`

Landed path (from the receiver's real mailbox): `/tmp/atm-501/send-to/01M12VGJ2Z7CGT04M2S1NG97XM/aq4-report.pdf`
Matches `send_to_staging_dir` convention (`.../send-to/<transfer-id>`): **True**
Landed file exists: **True**
Landed content byte-for-byte matches the source attachment: **True**
